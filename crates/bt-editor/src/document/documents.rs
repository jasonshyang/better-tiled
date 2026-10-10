//! The documents: the tab order and which tab is current. Documents are named by their id
//! everywhere; nothing holds a tab index.

use std::collections::HashMap;

use super::document::{Document, DocumentId};

/// The open documents.
#[derive(Clone, Debug, Default)]
pub struct Documents {
    docs: HashMap<DocumentId, Document>,
    order: Vec<DocumentId>,
    current: Option<DocumentId>,
    next_id: u64,
}

impl Documents {
    /// A fresh id.
    pub(crate) fn next_id(&mut self) -> DocumentId {
        self.next_id += 1;
        DocumentId(self.next_id)
    }

    /// Adds a document after the current tab and makes it current.
    pub(crate) fn add(&mut self, document: Document) {
        let id = document.id;
        let at = self
            .current
            .and_then(|current| self.order.iter().position(|d| *d == current))
            .map_or(self.order.len(), |position| position + 1);
        self.order.insert(at, id);
        self.docs.insert(id, document);
        self.current = Some(id);
    }

    /// Takes a document out; the tab after it (or before, at the end) becomes current.
    pub(crate) fn remove(&mut self, id: DocumentId) -> Option<Document> {
        let document = self.docs.remove(&id)?;
        if let Some(position) = self.order.iter().position(|d| *d == id) {
            self.order.remove(position);
            if self.current == Some(id) {
                self.current = self
                    .order
                    .get(position)
                    .or_else(|| self.order.last())
                    .copied();
            }
        }
        Some(document)
    }

    /// A document.
    pub fn get(&self, id: DocumentId) -> Option<&Document> {
        self.docs.get(&id)
    }

    pub(crate) fn get_mut(&mut self, id: DocumentId) -> Option<&mut Document> {
        self.docs.get_mut(&id)
    }

    /// Every document, to change, in no particular order.
    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut Document> {
        self.docs.values_mut()
    }

    /// The documents in tab order.
    pub fn in_order(&self) -> impl Iterator<Item = &Document> {
        self.order.iter().filter_map(|id| self.docs.get(id))
    }

    /// The current document.
    pub fn current(&self) -> Option<DocumentId> {
        self.current
    }

    /// Makes a document current.
    pub(crate) fn set_current(&mut self, id: DocumentId) {
        if self.docs.contains_key(&id) {
            self.current = Some(id);
        }
    }

    /// The tab `step` places after (or before) the current one, wrapping.
    pub(crate) fn neighbour(&self, step: isize) -> Option<DocumentId> {
        let current = self.current?;
        let position = self.order.iter().position(|d| *d == current)?;
        let count = self.order.len() as isize;
        let next = (position as isize + step).rem_euclid(count.max(1));
        self.order.get(next as usize).copied()
    }

    /// Every document's id, in tab order.
    pub fn ids(&self) -> Vec<DocumentId> {
        self.order.clone()
    }

    /// How many are open.
    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// Whether none is open.
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}
