//! What a transaction's closure works on: copy-on-write copies of the roots it asks for, and
//! the document's selection.

use std::collections::BTreeMap;
use std::sync::Arc;

use bt_core::{Map, Tileset};

use crate::document::Selection;
use crate::error::EditError;
use crate::store::{AssetId, AssetKind, AssetStore, Root};

/// A transaction in the making. Roots asked for mutably are copied on first use (their chunks,
/// object lists and tilesets stay shared until changed); the store sees them only if the
/// transaction is recorded.
pub struct Tx<'s> {
    store: &'s AssetStore,
    pub(crate) working: BTreeMap<AssetId, Root>,
    /// The document's selection; the closure may change it.
    pub selection: Selection,
}

impl<'s> Tx<'s> {
    pub(crate) fn new(store: &'s AssetStore, selection: Selection) -> Tx<'s> {
        Tx {
            store,
            working: BTreeMap::new(),
            selection,
        }
    }

    /// A map as it is in this transaction so far.
    pub fn map(&self, asset: AssetId) -> Result<&Map, EditError> {
        match self.working.get(&asset) {
            Some(Root::Map(map)) => Ok(map),
            Some(_) => Err(EditError::WrongKind {
                asset,
                expected: AssetKind::Map,
            }),
            None => self
                .store
                .map(asset)
                .map(Arc::as_ref)
                .ok_or(EditError::NoSuchAsset(asset)),
        }
    }

    /// A map to change.
    pub fn map_mut(&mut self, asset: AssetId) -> Result<&mut Map, EditError> {
        self.take(asset, AssetKind::Map)?;
        match self.working.get_mut(&asset) {
            Some(Root::Map(map)) => Ok(Arc::make_mut(map)),
            _ => Err(EditError::WrongKind {
                asset,
                expected: AssetKind::Map,
            }),
        }
    }

    /// A tileset to change.
    pub fn tileset_mut(&mut self, asset: AssetId) -> Result<&mut Tileset, EditError> {
        self.take(asset, AssetKind::Tileset)?;
        match self.working.get_mut(&asset) {
            Some(Root::Tileset(tileset)) => Ok(Arc::make_mut(tileset)),
            _ => Err(EditError::WrongKind {
                asset,
                expected: AssetKind::Tileset,
            }),
        }
    }

    /// Puts a copy of the store's root in the working set.
    fn take(&mut self, asset: AssetId, kind: AssetKind) -> Result<(), EditError> {
        if self.working.contains_key(&asset) {
            return Ok(());
        }
        let root = self
            .store
            .get(asset)
            .ok_or(EditError::NoSuchAsset(asset))?
            .root()
            .clone();
        if root.kind() != kind {
            return Err(EditError::WrongKind {
                asset,
                expected: kind,
            });
        }
        self.working.insert(asset, root);
        Ok(())
    }
}
