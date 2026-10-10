//! Making transactions, and undoing and redoing them. A transaction is recorded once, in the
//! document it was made in; it is undone only while every asset it touched is still at the
//! version it left (later changes win), and redone only from the version it started from.

use std::collections::BTreeMap;

use crate::document::{DocumentId, Selection};
use crate::editor::Editor;
use crate::error::EditError;
use crate::history::{AssetChange, MergeKey, SelectionChange, Transaction, Tx};
use crate::store::{AssetId, Root};

impl Editor {
    /// Runs `f` on copies of the roots it asks for and records one transaction labelled `label`
    /// in document `doc` when any root differs afterwards. A closure that changes no asset
    /// leaves no history (a change of selection alone is kept, not recorded).
    pub fn transact<R, E: From<EditError>>(
        &mut self,
        doc: DocumentId,
        label: &str,
        f: impl FnOnce(&mut Tx<'_>) -> Result<R, E>,
    ) -> Result<R, E> {
        self.transact_with(doc, label, None, f)
    }

    /// Like [`Editor::transact`], but folds into the document's last transaction when it has the
    /// same key and nothing was undone or broken since (typing in a field).
    pub fn transact_merging<R, E: From<EditError>>(
        &mut self,
        doc: DocumentId,
        label: &str,
        key: MergeKey,
        f: impl FnOnce(&mut Tx<'_>) -> Result<R, E>,
    ) -> Result<R, E> {
        self.transact_with(doc, label, Some(key), f)
    }

    /// Ends a run of merging transactions (a field losing focus).
    pub fn break_merge(&mut self, doc: DocumentId) {
        if let Some(document) = self.documents.get_mut(doc) {
            document.history.merge_open = false;
        }
    }

    fn transact_with<R, E: From<EditError>>(
        &mut self,
        doc: DocumentId,
        label: &str,
        key: Option<MergeKey>,
        f: impl FnOnce(&mut Tx<'_>) -> Result<R, E>,
    ) -> Result<R, E> {
        if self.gesture.is_some() {
            return Err(EditError::GestureUnderWay.into());
        }
        let before_selection = self
            .documents
            .get(doc)
            .ok_or(EditError::NoSuchDocument(doc))?
            .selection
            .clone();
        let mut tx = Tx::new(&self.store, before_selection.clone());
        let result = f(&mut tx)?;
        let mut after_selection = tx.selection.clone();
        let keeps_carry = tx.keeps_carry;
        let changed = self.changed_roots(tx.working);
        settle_carry(&mut after_selection, keeps_carry || changed.is_empty());
        if let Some(document) = self.documents.get_mut(doc) {
            document.selection = after_selection.clone();
            if after_selection.objects != before_selection.objects {
                document.inspect_selected_objects();
            }
        }
        if changed.is_empty() {
            return Ok(result);
        }
        let changes = self.apply_roots(changed);
        let selection = SelectionChange {
            before: before_selection,
            after: after_selection,
        };
        self.record(doc, label, key, selection, changes);
        Ok(result)
    }

    /// The working roots that differ from the store's.
    pub(crate) fn changed_roots(
        &self,
        working: BTreeMap<AssetId, Root>,
    ) -> BTreeMap<AssetId, Root> {
        working
            .into_iter()
            .filter(|(id, root)| {
                self.store
                    .get(*id)
                    .is_some_and(|asset| !asset.root().same_as(root))
            })
            .collect()
    }

    /// Puts changed roots in the store at new versions; the changes, with what they replaced.
    pub(crate) fn apply_roots(&mut self, changed: BTreeMap<AssetId, Root>) -> Vec<AssetChange> {
        let mut changes = Vec::with_capacity(changed.len());
        for (id, root) in changed {
            let Some(asset) = self.store.get(id) else {
                continue;
            };
            let before = asset.root().clone();
            let before_version = asset.version();
            let after_version = self.store.next_version(id);
            self.store.set_root(id, root.clone(), after_version);
            changes.push(AssetChange {
                asset: id,
                before,
                after: root,
                before_version,
                after_version,
            });
        }
        self.refresh_changed_maps(changes.iter().map(|change| change.asset));
        changes
    }

    /// Puts the store's tileset roots into maps whose roots were just set.
    fn refresh_changed_maps(&mut self, assets: impl Iterator<Item = AssetId>) {
        let ids: Vec<AssetId> = assets.collect();
        for id in ids {
            let Some(map) = self.store.map(id).cloned() else {
                continue;
            };
            let mut refreshed = (*map).clone();
            if self.store.refresh_tilesets(&mut refreshed) {
                let version = self.store.get(id).map_or(1, |asset| asset.version());
                self.store
                    .set_root(id, Root::Map(std::sync::Arc::new(refreshed)), version);
            }
        }
    }

    /// Records a transaction, or folds it into the last one when it merges.
    fn record(
        &mut self,
        doc: DocumentId,
        label: &str,
        key: Option<MergeKey>,
        selection: SelectionChange,
        changes: Vec<AssetChange>,
    ) {
        let Some(document) = self.documents.get_mut(doc) else {
            return;
        };
        if let Some(key) = &key
            && let Some(last) = document.history.mergeable()
            && last.merge_key.as_ref() == Some(key)
        {
            for change in changes {
                match last.changes.iter_mut().find(|c| c.asset == change.asset) {
                    Some(existing) => {
                        existing.after = change.after;
                        existing.after_version = change.after_version;
                    }
                    None => last.changes.push(change),
                }
            }
            last.selection.after = selection.after;
            return;
        }
        let merging = key.is_some();
        document.history.record(
            Transaction {
                label: label.to_owned(),
                merge_key: key,
                document: doc,
                selection,
                changes,
            },
            merging,
        );
    }

    /// Undoes the document's last done transaction.
    pub fn undo(&mut self, doc: DocumentId) -> Result<(), EditError> {
        if self.gesture.is_some() {
            return Err(EditError::GestureUnderWay);
        }
        let document = self
            .documents
            .get(doc)
            .ok_or(EditError::NoSuchDocument(doc))?;
        let transaction = document
            .history
            .undoable()
            .ok_or(EditError::NothingToUndo)?
            .clone();
        for change in &transaction.changes {
            let asset = self
                .store
                .get(change.asset)
                .ok_or(EditError::NoSuchAsset(change.asset))?;
            if asset.version() != change.after_version {
                return Err(self.blocked(change.asset));
            }
        }
        self.restore(&transaction.changes, Side::Before);
        if let Some(document) = self.documents.get_mut(doc) {
            document.history.position -= 1;
            document.history.merge_open = false;
            document.selection = transaction.selection.before;
        }
        Ok(())
    }

    /// Redoes the document's next undone transaction.
    pub fn redo(&mut self, doc: DocumentId) -> Result<(), EditError> {
        if self.gesture.is_some() {
            return Err(EditError::GestureUnderWay);
        }
        let document = self
            .documents
            .get(doc)
            .ok_or(EditError::NoSuchDocument(doc))?;
        let transaction = document
            .history
            .redoable()
            .ok_or(EditError::NothingToRedo)?
            .clone();
        for change in &transaction.changes {
            let asset = self
                .store
                .get(change.asset)
                .ok_or(EditError::NoSuchAsset(change.asset))?;
            if asset.version() != change.before_version {
                return Err(self.blocked(change.asset));
            }
        }
        self.restore(&transaction.changes, Side::After);
        if let Some(document) = self.documents.get_mut(doc) {
            document.history.position += 1;
            document.history.merge_open = false;
            document.selection = transaction.selection.after;
        }
        Ok(())
    }

    /// Undoes or redoes until `position` transactions of the document are done (the History
    /// panel); stops at the first that cannot be.
    pub fn travel_to(&mut self, doc: DocumentId, position: usize) -> Result<(), EditError> {
        loop {
            let current = self
                .documents
                .get(doc)
                .ok_or(EditError::NoSuchDocument(doc))?
                .history
                .position;
            match current.cmp(&position) {
                std::cmp::Ordering::Equal => return Ok(()),
                std::cmp::Ordering::Greater => self.undo(doc)?,
                std::cmp::Ordering::Less => self.redo(doc)?,
            }
        }
    }

    /// Puts one side of some changes back.
    fn restore(&mut self, changes: &[AssetChange], side: Side) {
        for change in changes {
            match side {
                Side::Before => {
                    self.store
                        .set_root(change.asset, change.before.clone(), change.before_version);
                }
                Side::After => {
                    self.store
                        .set_root(change.asset, change.after.clone(), change.after_version);
                }
            }
        }
        self.refresh_changed_maps(changes.iter().map(|change| change.asset));
    }

    /// The error naming the transaction that made an asset's current version.
    fn blocked(&self, asset: AssetId) -> EditError {
        let Some(version) = self.store.get(asset).map(|a| a.version()) else {
            return EditError::NoSuchAsset(asset);
        };
        for document in self.documents.in_order() {
            let done = document
                .history
                .entries
                .get(..document.history.position)
                .unwrap_or(&[]);
            for transaction in done.iter().rev() {
                if transaction
                    .changes
                    .iter()
                    .any(|c| c.asset == asset && c.after_version == version)
                {
                    return EditError::Blocked {
                        asset,
                        document: document.id(),
                        label: transaction.label.clone(),
                    };
                }
            }
        }
        EditError::ChangedSince { asset }
    }
}

/// Which side of a change to put back.
#[derive(Copy, Clone)]
enum Side {
    Before,
    After,
}

/// Lets the carried tiles go unless the transaction kept them (or changed nothing), or when the
/// selected cells or layers are no longer theirs.
pub(crate) fn settle_carry(selection: &mut Selection, kept: bool) {
    if !kept {
        selection.carried = None;
    }
    selection.drop_stale_carry();
}
