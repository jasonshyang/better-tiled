//! Finding the things a change edits inside the map or tileset it is given.

use std::sync::Arc;

use bt_core::{CustomTypeId, CustomTypes, Map, MapObject, ObjectId, Properties, Tileset};

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
        TargetId::Tileset | TargetId::Tile(_) | TargetId::Members(_) => {
            Err(PropertyError::NotInMap(id))
        }
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
        TargetId::Tileset | TargetId::Tile(_) | TargetId::Members(_) => {
            Err(PropertyError::NotInMap(id))
        }
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
        TargetId::Map | TargetId::Layer(_) | TargetId::Object(_) | TargetId::Members(_) => {
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
        TargetId::Map | TargetId::Layer(_) | TargetId::Object(_) | TargetId::Members(_) => {
            Err(PropertyError::NotInTileset(id))
        }
    }
}

/// The members of the project's class `id`.
pub(crate) fn class_members(
    types: &CustomTypes,
    id: CustomTypeId,
) -> Result<&Properties, PropertyError> {
    let found = types.find_by_id(id).ok_or(PropertyError::NoSuchType(id))?;
    found
        .as_class()
        .map(|class| &class.members)
        .ok_or_else(|| PropertyError::NotAClass(found.name.clone()))
}

/// The members of the project's class `id`, to change.
pub(crate) fn class_members_mut(
    types: &mut CustomTypes,
    id: CustomTypeId,
) -> Result<&mut Properties, PropertyError> {
    let found = types
        .types
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or(PropertyError::NoSuchType(id))?;
    match &mut found.kind {
        bt_core::CustomTypeKind::Class(class) => Ok(&mut class.members),
        bt_core::CustomTypeKind::Enum(_) => Err(PropertyError::NotAClass(found.name.clone())),
    }
}
