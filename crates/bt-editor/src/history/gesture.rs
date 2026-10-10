//! Gestures: a brush stroke or a drag applies its edits live, so every panel shows it as it
//! happens, and is recorded once, from the roots at its start to the roots at its end.

use std::collections::BTreeMap;

use crate::document::{DocumentId, Selection};
use crate::editor::Editor;
use crate::error::EditError;
use crate::history::{AssetChange, SelectionChange, Transaction, Tx};
use crate::store::{AssetId, Root};

/// A gesture under way.
#[derive(Clone, Debug)]
pub(crate) struct Gesture {
    document: DocumentId,
    label: String,
    /// Each touched asset's root and version at the start.
    start: BTreeMap<AssetId, Start>,
    selection: Selection,
}

/// An asset as a gesture found it.
#[derive(Clone, Debug)]
struct Start {
    root: Root,
    version: u64,
}

impl Editor {
    /// Starts a gesture in document `doc`; one at a time.
    pub fn begin_gesture(&mut self, doc: DocumentId, label: &str) -> Result<(), EditError> {
        if self.gesture.is_some() {
            return Err(EditError::GestureUnderWay);
        }
        let selection = self
            .documents
            .get(doc)
            .ok_or(EditError::NoSuchDocument(doc))?
            .selection
            .clone();
        self.gesture = Some(Gesture {
            document: doc,
            label: label.to_owned(),
            start: BTreeMap::new(),
            selection,
        });
        Ok(())
    }

    /// Applies an edit of the gesture under way at once.
    pub fn gesture_apply<R, E: From<EditError>>(
        &mut self,
        f: impl FnOnce(&mut Tx<'_>) -> Result<R, E>,
    ) -> Result<R, E> {
        let doc = self.gesture.as_ref().ok_or(EditError::NoGesture)?.document;
        let selection = self
            .documents
            .get(doc)
            .ok_or(EditError::NoSuchDocument(doc))?
            .selection
            .clone();
        let mut tx = Tx::new(&self.store, selection);
        let result = f(&mut tx)?;
        let mut after_selection = tx.selection.clone();
        let keeps_carry = tx.keeps_carry;
        let changed = self.changed_roots(tx.working);
        super::apply::settle_carry(&mut after_selection, keeps_carry || changed.is_empty());
        for id in changed.keys() {
            let Some(asset) = self.store.get(*id) else {
                continue;
            };
            let start = Start {
                root: asset.root().clone(),
                version: asset.version(),
            };
            if let Some(gesture) = &mut self.gesture {
                gesture.start.entry(*id).or_insert(start);
            }
        }
        self.apply_roots(changed);
        if let Some(document) = self.documents.get_mut(doc) {
            document.selection = after_selection;
        }
        Ok(result)
    }

    /// Ends the gesture: one transaction from its start to now, or none when nothing changed;
    /// whether one was recorded.
    pub fn end_gesture(&mut self) -> Result<bool, EditError> {
        let gesture = self.gesture.take().ok_or(EditError::NoGesture)?;
        let mut changes = Vec::new();
        for (id, start) in gesture.start {
            let Some(asset) = self.store.get(id) else {
                continue;
            };
            if asset.root().same_as(&start.root) {
                // Back where it started: the versions in between never happened.
                self.store.set_root(id, start.root, start.version);
                continue;
            }
            changes.push(AssetChange {
                asset: id,
                before: start.root,
                after: asset.root().clone(),
                before_version: start.version,
                after_version: asset.version(),
            });
        }
        let Some(document) = self.documents.get_mut(gesture.document) else {
            return Ok(false);
        };
        if changes.is_empty() {
            return Ok(false);
        }
        let selection = SelectionChange {
            before: gesture.selection,
            after: document.selection.clone(),
        };
        document.history.record(
            Transaction {
                label: gesture.label,
                merge_key: None,
                document: gesture.document,
                selection,
                changes,
            },
            false,
        );
        Ok(true)
    }

    /// Cancels the gesture: every touched asset goes back to its start, and the selection.
    pub fn cancel_gesture(&mut self) -> Result<(), EditError> {
        let gesture = self.gesture.take().ok_or(EditError::NoGesture)?;
        for (id, start) in gesture.start {
            self.store.set_root(id, start.root, start.version);
        }
        if let Some(document) = self.documents.get_mut(gesture.document) {
            document.selection = gesture.selection;
        }
        Ok(())
    }

    /// Whether a gesture is under way.
    pub fn in_gesture(&self) -> bool {
        self.gesture.is_some()
    }
}
