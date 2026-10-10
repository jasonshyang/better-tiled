//! Finding the things a change edits inside the map or tileset it is given.

use std::sync::Arc;

use bt_core::{Map, MapObject, ObjectId, Properties, Tileset};

use super::error::PropertyError;
use super::target_id::TargetId;

/// The properties of a tile that has no data yet.
pub(crate) static NO_PROPERTIES: Properties = Properties::new();

/// An object of the map, to change. Its layer's object list is copied if it is shared.
pub(crate) fn object_mut(map: &mut Map, id: ObjectId) -> Option<&mut MapObject> {
    let layer = map.object_layer_of(id)?;
    let objects = map.layer_mut(layer)?.as_object_layer_mut()?;
    Arc::make_mut(&mut objects.objects)
        .iter_mut()
        .find(|object| object.id == id)
}

/// The properties a target of a map keeps.
pub(crate) fn map_properties(map: &Map, id: TargetId) -> Result<&Properties, PropertyError> {
    match id {
        TargetId::Map => Ok(&map.properties),
        TargetId::Layer(layer) => map
            .layer(layer)
            .map(|layer| &layer.properties)
            .ok_or(PropertyError::NoSuchLayer(layer)),
        TargetId::Object(object) => map
            .object(object)
            .map(|object| &object.properties)
            .ok_or(PropertyError::NoSuchObject(object)),
        TargetId::Tileset | TargetId::Tile(_) => Err(PropertyError::NotInMap(id)),
    }
}

/// The properties a target of a map keeps, to change.
pub(crate) fn map_properties_mut(
    map: &mut Map,
    id: TargetId,
) -> Result<&mut Properties, PropertyError> {
    match id {
        TargetId::Map => Ok(&mut map.properties),
        TargetId::Layer(layer) => map
            .layer_mut(layer)
            .map(|layer| &mut layer.properties)
            .ok_or(PropertyError::NoSuchLayer(layer)),
        TargetId::Object(object) => object_mut(map, object)
            .map(|object| &mut object.properties)
            .ok_or(PropertyError::NoSuchObject(object)),
        TargetId::Tileset | TargetId::Tile(_) => Err(PropertyError::NotInMap(id)),
    }
}

/// The properties a target of a tileset keeps (none for a tile without data).
pub(crate) fn tileset_properties(
    tileset: &Tileset,
    id: TargetId,
) -> Result<&Properties, PropertyError> {
    match id {
        TargetId::Tileset => Ok(&tileset.properties),
        TargetId::Tile(tile) if tileset.has_tile(tile) => Ok(tileset
            .tile(tile)
            .map_or(&NO_PROPERTIES, |data| &data.properties)),
        TargetId::Tile(tile) => Err(PropertyError::NoSuchTile(tile)),
        TargetId::Map | TargetId::Layer(_) | TargetId::Object(_) => {
            Err(PropertyError::NotInTileset(id))
        }
    }
}

/// The properties a target of a tileset keeps, to change; a tile without data gets some.
pub(crate) fn tileset_properties_mut(
    tileset: &mut Tileset,
    id: TargetId,
) -> Result<&mut Properties, PropertyError> {
    match id {
        TargetId::Tileset => Ok(&mut tileset.properties),
        TargetId::Tile(tile) if tileset.has_tile(tile) => {
            Ok(&mut tileset.tiles.entry(tile).or_default().properties)
        }
        TargetId::Tile(tile) => Err(PropertyError::NoSuchTile(tile)),
        TargetId::Map | TargetId::Layer(_) | TargetId::Object(_) => {
            Err(PropertyError::NotInTileset(id))
        }
    }
}
