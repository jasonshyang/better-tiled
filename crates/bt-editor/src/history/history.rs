//! A document's history: the transactions made in it, oldest first, and the position between
//! what is done and what can be redone.

use super::transaction::Transaction;

/// The most transactions a history keeps; the oldest go first.
pub const DEFAULT_HISTORY_LIMIT: usize = 500;

/// A document's transactions.
#[derive(Clone, Debug)]
pub struct History {
    pub(crate) entries: Vec<Transaction>,
    /// How many entries are done; the rest can be redone.
    pub(crate) position: usize,
    /// Whether the last transaction may still take in a merging one.
    pub(crate) merge_open: bool,
    pub(crate) limit: usize,
}

impl Default for History {
    fn default() -> History {
        History {
            entries: Vec::new(),
            position: 0,
            merge_open: false,
            limit: DEFAULT_HISTORY_LIMIT,
        }
    }
}

/// A line of the History panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    /// The transaction's label.
    pub label: String,
    /// Whether it is done (undo would undo it or one before it).
    pub done: bool,
}

impl History {
    /// The entries, oldest first.
    pub fn entries(&self) -> Vec<HistoryEntry> {
        self.entries
            .iter()
            .enumerate()
            .map(|(index, transaction)| HistoryEntry {
                label: transaction.label.clone(),
                done: index < self.position,
            })
            .collect()
    }

    /// How many entries are done.
    pub fn position(&self) -> usize {
        self.position
    }

    /// The transaction undo would undo.
    pub fn undoable(&self) -> Option<&Transaction> {
        self.position
            .checked_sub(1)
            .and_then(|i| self.entries.get(i))
    }

    /// The transaction redo would redo.
    pub fn redoable(&self) -> Option<&Transaction> {
        self.entries.get(self.position)
    }

    /// Records a new transaction: the redo side is dropped, and the oldest entries beyond the
    /// limit.
    pub(crate) fn record(&mut self, transaction: Transaction, merge_open: bool) {
        self.entries.truncate(self.position);
        self.entries.push(transaction);
        let excess = self.entries.len().saturating_sub(self.limit);
        if excess > 0 {
            self.entries.drain(..excess);
        }
        self.position = self.entries.len();
        self.merge_open = merge_open;
    }

    /// The last transaction, to fold a merging one into, when nothing was undone since and the
    /// run was not broken.
    pub(crate) fn mergeable(&mut self) -> Option<&mut Transaction> {
        if !self.merge_open || self.position != self.entries.len() {
            return None;
        }
        self.entries.last_mut()
    }

    /// Drops references to an explicitly unloaded asset; mixed map/world transactions retain
    /// their map changes, and world-only entries are removed without disturbing the cursor.
    pub(crate) fn forget_asset(&mut self, asset: crate::AssetId) {
        let mut position = 0;
        let mut index = 0;
        self.entries.retain_mut(|entry| {
            let touched = entry.changes.iter().any(|change| change.asset == asset);
            entry.changes.retain(|change| change.asset != asset);
            let keep = !touched || !entry.changes.is_empty();
            if keep && index < self.position {
                position += 1;
            }
            index += 1;
            keep
        });
        self.position = position;
        self.merge_open = false;
    }

    /// Sets the limit (a setting).
    pub(crate) fn set_limit(&mut self, limit: usize) {
        self.limit = limit.max(1);
    }
}
