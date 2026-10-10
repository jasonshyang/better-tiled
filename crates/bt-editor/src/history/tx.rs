//! What a transaction's closure works on: copy-on-write copies of the roots it asks for, and
//! the document's selection.

use std::collections::BTreeMap;
use std::sync::Arc;

use bt_core::{Map, Project, Tileset};

use crate::document::Selection;
use crate::error::EditError;
use crate::store::{AssetId, AssetKind, AssetStore, Root, TilesetHome};

/// A transaction in the making. Roots asked for mutably are copied on first use (their chunks,
/// object lists and tilesets stay shared until changed); the store sees them only if the
/// transaction is recorded.
pub struct Tx<'s> {
    store: &'s AssetStore,
    pub(crate) working: BTreeMap<AssetId, Root>,
    /// The document's selection; the closure may change it.
    pub selection: Selection,
    /// The closure changed the carried tiles: the carry goes on. Any other change lets them go.
    pub(crate) keeps_carry: bool,
}

impl<'s> Tx<'s> {
    pub(crate) fn new(store: &'s AssetStore, selection: Selection) -> Tx<'s> {
        Tx {
            store,
            working: BTreeMap::new(),
            selection,
            keeps_carry: false,
        }
    }

    /// The selection carries `carried` from now on: its cells are where the tiles are, and the
    /// transaction keeps the carry going.
    pub(crate) fn carry(&mut self, carried: crate::document::Carried) {
        self.selection.tiles = carried.tiles.region();
        self.selection.carried = Some(carried);
        self.keeps_carry = true;
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

    /// The tileset at `home`, to change: an external tileset's root, or the copy of it a map
    /// embeds.
    pub fn tileset_at_mut(&mut self, home: TilesetHome) -> Result<&mut Tileset, EditError> {
        match home {
            TilesetHome::Asset(asset) => self.tileset_mut(asset),
            TilesetHome::Embedded { map, slot } => {
                let reference = self
                    .map_mut(map)?
                    .tilesets
                    .get_mut(slot.index())
                    .filter(|reference| reference.file.is_none())
                    .ok_or(bt_edit::EditError::NoSuchTileset(slot))?;
                Ok(Arc::make_mut(&mut reference.tileset))
            }
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

    /// A project to change.
    pub fn project_mut(&mut self, asset: AssetId) -> Result<&mut Project, EditError> {
        self.take(asset, AssetKind::Project)?;
        match self.working.get_mut(&asset) {
            Some(Root::Project(project)) => Ok(Arc::make_mut(project)),
            _ => Err(EditError::WrongKind {
                asset,
                expected: AssetKind::Project,
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
