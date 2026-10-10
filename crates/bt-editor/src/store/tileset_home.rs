//! Where a tileset lives that is shown or edited: the asset of its own file, or the map that
//! embeds it. A change to it is a change to that asset.

use std::sync::Arc;

use bt_core::{Tileset, TilesetSlot};

use super::asset::AssetId;
use super::asset_store::AssetStore;

/// Where a tileset lives.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TilesetHome {
    /// An external tileset: the asset of its file.
    Asset(AssetId),
    /// An embedded tileset: the map that holds it, and its slot there.
    Embedded {
        /// The map's asset.
        map: AssetId,
        /// The tileset's slot in the map.
        slot: TilesetSlot,
    },
}

impl TilesetHome {
    /// The asset a change to the tileset changes.
    pub fn asset(self) -> AssetId {
        match self {
            TilesetHome::Asset(asset) => asset,
            TilesetHome::Embedded { map, .. } => map,
        }
    }
}

impl AssetStore {
    /// The tileset at `home`: an external tileset's root, or the one a map embeds in the slot.
    pub fn tileset_at(&self, home: TilesetHome) -> Option<&Arc<Tileset>> {
        match home {
            TilesetHome::Asset(asset) => self.tileset(asset),
            TilesetHome::Embedded { map, slot } => self
                .map(map)?
                .tilesets
                .get(slot.index())
                .filter(|reference| reference.file.is_none())
                .map(|reference| &reference.tileset),
        }
    }

    /// Where the tileset in a map's slot lives: the asset of its file when it is external (and
    /// the store has it), else the map.
    pub fn home_of_slot(&self, map: AssetId, slot: TilesetSlot) -> Option<TilesetHome> {
        let reference = self.map(map)?.tilesets.get(slot.index())?;
        match &reference.file {
            Some(file) => self.by_path(&file.path).map(TilesetHome::Asset),
            None => Some(TilesetHome::Embedded { map, slot }),
        }
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{FileRef, TilesetRef};
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::UVec2;

    use super::*;
    use crate::store::{NewAsset, Root};

    #[test]
    fn a_tileset_lives_in_its_file_or_in_the_map_that_embeds_it() {
        let embedded = Atlas::builder().name("inside").build_embedded().unwrap();
        let external = Atlas::builder().name("outside").build_embedded().unwrap();
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::splat(2))
            .tileset(embedded.tileset.clone())
            .tileset(external.tileset.clone())
            .build()
            .unwrap();
        map.tilesets[1] = TilesetRef {
            file: Some(FileRef::new("outside.tsx", "/maps/outside.tsx")),
            first_gid: Some(1000),
            tileset: Arc::clone(&external.tileset),
        };
        let mut store = AssetStore::new();
        let tileset_asset = store.insert(NewAsset {
            root: Root::Tileset(Arc::clone(&external.tileset)),
            path: Some("/maps/outside.tsx".into()),
            content_hash: None,
            warnings: Vec::new(),
        });
        let map_asset = store.insert(NewAsset {
            root: Root::Map(Arc::new(map)),
            path: None,
            content_hash: None,
            warnings: Vec::new(),
        });
        let first = TilesetSlot::new(0).unwrap();
        let second = TilesetSlot::new(1).unwrap();
        let inside = store.home_of_slot(map_asset, first).unwrap();
        assert_eq!(
            inside,
            TilesetHome::Embedded {
                map: map_asset,
                slot: first
            }
        );
        assert_eq!(inside.asset(), map_asset);
        assert_eq!(store.tileset_at(inside).unwrap().name, "inside");
        let outside = store.home_of_slot(map_asset, second).unwrap();
        assert_eq!(outside, TilesetHome::Asset(tileset_asset));
        assert_eq!(store.tileset_at(outside).unwrap().name, "outside");
        assert_eq!(
            store.tileset_at(TilesetHome::Embedded {
                map: map_asset,
                slot: second
            }),
            None,
            "an external slot is not an embedded home"
        );
    }
}
