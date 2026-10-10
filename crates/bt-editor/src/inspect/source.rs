//! What the things a document's Properties panel inspects are read from: the document's map or
//! the tileset its tileset editor edits, with the templates of inspected template instances.
//! It holds its own references, so a transaction can run while targets borrow from it.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use bt_core::{CustomTypes, Map, ObjectId, ObjectTemplate, Tileset};

use super::inspected::Inspected;
use crate::document::DocumentId;
use crate::editor::Editor;
use crate::error::EditError;
use crate::properties::{
    AttributeRow, PropertyError, Target, layer_attributes, map_attributes, object_attributes,
    terrain_attributes, terrain_set_attributes, tile_attributes, tileset_attributes,
};
use crate::store::{AssetId, AssetKind, TilesetHome};

/// Where inspected things live.
pub(crate) enum Place {
    /// The document's map.
    Map {
        /// Its asset.
        asset: AssetId,
        /// The map, as the store has it.
        map: Arc<Map>,
    },
    /// The tileset the document's tileset editor edits.
    Tileset {
        /// Where it lives.
        home: TilesetHome,
        /// The tileset, as the store has it.
        tileset: Arc<Tileset>,
    },
}

/// What inspected things are read from.
pub(crate) struct Source {
    /// Where they live.
    pub(crate) place: Place,
    /// The templates of inspected template instances, by object.
    templates: BTreeMap<ObjectId, Arc<ObjectTemplate>>,
}

impl Place {
    /// The map's asset; refused for a tileset.
    pub(crate) fn map_asset(&self) -> Result<AssetId, EditError> {
        match self {
            Place::Map { asset, .. } => Ok(*asset),
            Place::Tileset { home, .. } => Err(EditError::WrongKind {
                asset: home.asset(),
                expected: AssetKind::Map,
            }),
        }
    }

    /// The tileset's home; refused for a map.
    pub(crate) fn tileset_home(&self) -> Result<TilesetHome, EditError> {
        match self {
            Place::Tileset { home, .. } => Ok(*home),
            Place::Map { asset, .. } => Err(EditError::WrongKind {
                asset: *asset,
                expected: AssetKind::Tileset,
            }),
        }
    }

    /// The asset whose file holds them: the map, the tileset's file, or the map that embeds
    /// the tileset.
    fn file_asset(&self) -> AssetId {
        match self {
            Place::Map { asset, .. } => *asset,
            Place::Tileset { home, .. } => home.asset(),
        }
    }
}

impl Source {
    /// The targets of the property-editing model for `inspected`, read with `types`.
    pub(crate) fn targets<'a>(
        &'a self,
        inspected: &Inspected,
        types: &'a CustomTypes,
    ) -> Result<Vec<Target<'a>>, PropertyError> {
        match (&self.place, inspected) {
            (Place::Map { map, .. }, Inspected::Map) => Ok(vec![Target::map(map, types)]),
            (Place::Map { map, .. }, Inspected::Layers(layers)) => layers
                .iter()
                .map(|id| Target::layer(map, *id, types))
                .collect(),
            (Place::Map { map, .. }, Inspected::Objects(objects)) => objects
                .iter()
                .map(|id| {
                    let template = self.templates.get(id).map(Arc::as_ref);
                    Target::object(map, *id, template, types)
                })
                .collect(),
            (Place::Tileset { tileset, .. }, Inspected::Tileset) => {
                Ok(vec![Target::tileset(tileset, types)])
            }
            (Place::Tileset { tileset, .. }, Inspected::Tiles(tiles)) => tiles
                .iter()
                .map(|id| Target::tile(tileset, *id, types))
                .collect(),
            (Place::Tileset { tileset, .. }, Inspected::TerrainSet(set)) => {
                Ok(vec![Target::terrain_set(tileset, *set, types)?])
            }
            (Place::Tileset { tileset, .. }, Inspected::Terrain { set, color }) => {
                Ok(vec![Target::terrain(tileset, *set, color.0, types)?])
            }
            (
                Place::Map { .. },
                Inspected::Tileset
                | Inspected::Tiles(_)
                | Inspected::TerrainSet(_)
                | Inspected::Terrain { .. },
            ) => Err(PropertyError::NotInMap(
                crate::properties::TargetId::Tileset,
            )),
            (Place::Tileset { .. }, _) => Err(PropertyError::NotInTileset(
                crate::properties::TargetId::Map,
            )),
        }
    }

    /// The built-in attribute rows of `inspected`.
    pub(crate) fn attribute_rows(
        &self,
        inspected: &Inspected,
    ) -> Result<Vec<AttributeRow>, PropertyError> {
        match (&self.place, inspected) {
            (Place::Map { map, .. }, Inspected::Map) => Ok(map_attributes(map)),
            (Place::Map { map, .. }, Inspected::Layers(layers)) => layer_attributes(map, layers),
            (Place::Map { map, .. }, Inspected::Objects(objects)) => {
                object_attributes(map, objects)
            }
            (Place::Tileset { tileset, .. }, Inspected::Tileset) => Ok(tileset_attributes(tileset)),
            (Place::Tileset { tileset, .. }, Inspected::Tiles(tiles)) => {
                tile_attributes(tileset, tiles)
            }
            (Place::Tileset { tileset, .. }, Inspected::TerrainSet(set)) => {
                terrain_set_attributes(tileset, *set)
            }
            (Place::Tileset { tileset, .. }, Inspected::Terrain { set, color }) => {
                terrain_attributes(tileset, *set, *color)
            }
            (
                Place::Map { .. },
                Inspected::Tileset
                | Inspected::Tiles(_)
                | Inspected::TerrainSet(_)
                | Inspected::Terrain { .. },
            ) => Err(PropertyError::NotInMap(
                crate::properties::TargetId::Tileset,
            )),
            (Place::Tileset { .. }, _) => Err(PropertyError::NotInTileset(
                crate::properties::TargetId::Map,
            )),
        }
    }
}

impl Editor {
    /// What the things `doc` inspects as `inspected` are read from: the document's map, or the
    /// tileset its tileset editor edits.
    pub(crate) fn source_of(
        &self,
        doc: DocumentId,
        inspected: &Inspected,
    ) -> Result<Source, EditError> {
        let document = self
            .documents
            .get(doc)
            .ok_or(EditError::NoSuchDocument(doc))?;
        let place = match inspected {
            Inspected::Tileset
            | Inspected::Tiles(_)
            | Inspected::TerrainSet(_)
            | Inspected::Terrain { .. } => {
                let home = document
                    .tileset_editing()
                    .map(|editing| editing.home())
                    .ok_or(EditError::NoTilesetShown(doc))?;
                let tileset = self
                    .store
                    .tileset_at(home)
                    .cloned()
                    .ok_or(EditError::NoSuchAsset(home.asset()))?;
                Place::Tileset { home, tileset }
            }
            Inspected::Map | Inspected::Layers(_) | Inspected::Objects(_) => {
                let asset = document.asset();
                let map = self.store.map(asset).cloned().ok_or(EditError::WrongKind {
                    asset,
                    expected: AssetKind::Map,
                })?;
                Place::Map { asset, map }
            }
        };
        let mut templates = BTreeMap::new();
        if let (Place::Map { asset, map }, Inspected::Objects(objects)) = (&place, inspected) {
            let loader = self.files.loaders.get(asset);
            for id in objects {
                let template = map
                    .object(*id)
                    .and_then(|object| object.template.as_ref())
                    .and_then(|file| loader?.cached_template(&file.path));
                if let Some(template) = template {
                    templates.insert(*id, Arc::clone(template));
                }
            }
        }
        Ok(Source { place, templates })
    }

    /// The folder of the file that holds what is inspected, which file values are relative to;
    /// `None` while that file has never been saved.
    pub(crate) fn folder_of(&self, source: &Source) -> Option<&Path> {
        self.store
            .get(source.place.file_asset())?
            .path()
            .and_then(Path::parent)
    }
}
