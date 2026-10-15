//! Template bases and their instances. Fields explicitly overridden by the instance stay its
//! own; properties follow the same value-difference rule as Tiled's file writer.

use crate::{EditError, unify_tileset};
use bt_core::{Cell, FileRef, Map, MapObject, ObjectId, ObjectOverrides, ObjectTemplate};
use std::path::Path;

/// Makes an instance at the origin, adopting its tile's tileset into `map` if needed. The
/// caller supplies its position and inserts it with the map's normal id allocator.
pub fn instantiate_template(
    map: &mut Map,
    template: &ObjectTemplate,
    file: FileRef,
) -> Result<MapObject, EditError> {
    let mut object = template
        .object
        .clone()
        .ok_or(EditError::TemplateBaseRequired)?;
    object.id = ObjectId(0);
    object.pos = glam::DVec2::ZERO;
    object.overrides = ObjectOverrides::empty();
    object.template = Some(file);
    object.tile = match object.tile {
        Some(cell) => {
            let slot = cell.slot().ok_or(EditError::EmptyCell)?;
            let reference = template
                .tilesets
                .get(slot.index())
                .ok_or(EditError::NoSuchTileset(slot))?;
            let adopted = unify_tileset(map, reference)?;
            let pixels = reference.tileset.tile_pixel_size(cell.tile()).as_dvec2();
            if object.size.x == 0.0 {
                object.size.x = pixels.x;
            }
            if object.size.y == 0.0 {
                object.size.y = pixels.y;
            }
            Some(Cell::new(adopted.slot, cell.tile()).with_flip(cell.flip()))
        }
        None => None,
    };
    Ok(object)
}

/// Applies a new template base to every matching instance, preserving ids, positions, field
/// overrides and own properties. References are compared by resolved file identity.
pub fn refresh_template_instances(
    map: &mut Map,
    path: &Path,
    before: &ObjectTemplate,
    after: &ObjectTemplate,
) -> Result<Vec<ObjectId>, EditError> {
    let old = before
        .object
        .as_ref()
        .ok_or(EditError::TemplateBaseRequired)?;
    let mut ids = Vec::new();
    map.for_each_layer(|layer, _| {
        if let Some(objects) = layer.as_object_layer() {
            ids.extend(
                objects
                    .objects
                    .iter()
                    .filter(|object| {
                        object
                            .template
                            .as_ref()
                            .is_some_and(|file| file.path == path)
                    })
                    .map(|object| object.id),
            );
        }
    });
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let needs_tile = ids.iter().any(|id| {
        map.object(*id)
            .is_some_and(|object| !object.overrides.contains(ObjectOverrides::TILE))
    });
    let base = if needs_tile {
        instantiate_template(map, after, FileRef::new("", path))?
    } else {
        after
            .object
            .clone()
            .ok_or(EditError::TemplateBaseRequired)?
    };
    let mut changed = Vec::new();
    for id in ids {
        let object = crate::object_mut(map, id)?;
        let previous = object.clone();
        refresh_template_object(object, old, &base);
        if *object != previous {
            changed.push(id);
        }
    }
    Ok(changed)
}

/// Refreshes one instance from a base already remapped to its owning map. Its identity,
/// position, explicitly overridden fields and own properties stay intact.
pub fn refresh_template_object(object: &mut MapObject, old: &MapObject, base: &MapObject) {
    let own = object
        .properties
        .iter()
        .filter(|(name, value)| old.properties.get(*name) != Some(*value))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect::<bt_core::Properties>();
    object.properties = base.properties.clone();
    object.properties.extend(own);
    let overrides = object.overrides;
    if !overrides.contains(ObjectOverrides::NAME) {
        object.name = base.name.clone();
    }
    if !overrides.contains(ObjectOverrides::CLASS) {
        object.class = base.class.clone();
    }
    if !overrides.contains(ObjectOverrides::SIZE) {
        object.size = base.size;
    }
    if !overrides.contains(ObjectOverrides::ROTATION) {
        object.rotation = base.rotation;
    }
    if !overrides.contains(ObjectOverrides::OPACITY) {
        object.opacity = base.opacity;
    }
    if !overrides.contains(ObjectOverrides::VISIBLE) {
        object.visible = base.visible;
    }
    if !overrides.contains(ObjectOverrides::TILE) {
        object.tile = base.tile;
    }
    if !overrides.intersects(ObjectOverrides::SHAPE | ObjectOverrides::TEXT) {
        object.shape = base.shape.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_core::{Flip, Layer, LayerId, LayerKind, ObjectLayer, Shape, TileId};
    use bt_test_data::fixtures::Atlas;
    use glam::DVec2;
    use std::sync::Arc;

    #[test]
    fn adopting_a_tile_template_remaps_slots_and_refresh_preserves_all_explicit_fields() {
        let first = Atlas::builder().name("first").build_embedded().unwrap();
        let second = Atlas::builder()
            .name("second")
            .tile_size(24, 12)
            .build_embedded()
            .unwrap();
        let mut map = Map {
            tilesets: vec![bt_core::TilesetRef::embedded(first.tileset.clone())],
            ..Map::default()
        };
        let base = MapObject {
            name: "base".into(),
            tile: Some(
                Cell::new(bt_core::TilesetSlot::new(0).unwrap(), TileId(1)).with_flip(Flip::H),
            ),
            ..MapObject::default()
        };
        let template = ObjectTemplate {
            object: Some(base),
            tilesets: vec![bt_core::TilesetRef::embedded(second.tileset)],
            ..ObjectTemplate::default()
        };
        let path = Path::new("/templates/unit.tx");
        let mut inherited =
            instantiate_template(&mut map, &template, FileRef::new("unit.tx", path)).unwrap();
        assert_eq!(map.tilesets.len(), 2);
        assert_eq!(inherited.tile.unwrap().slot().unwrap().index(), 1);
        assert_eq!(inherited.tile.unwrap().flip(), Flip::H);
        assert_eq!(inherited.size, DVec2::new(24.0, 12.0));
        inherited.id = ObjectId(1);
        inherited.pos = DVec2::new(4.0, 8.0);
        let own = MapObject {
            id: ObjectId(2),
            overrides: ObjectOverrides::all(),
            name: "own".into(),
            visible: false,
            opacity: 0.4,
            rotation: 15.0,
            size: DVec2::splat(7.0),
            shape: Shape::Ellipse,
            ..inherited.clone()
        };
        map.layers.push(Layer::new(
            LayerId(1),
            "Objects",
            LayerKind::Object(ObjectLayer {
                objects: vec![inherited, own.clone()].into(),
                ..ObjectLayer::default()
            }),
        ));
        let mut after = template.clone();
        let object = after.object.as_mut().unwrap();
        object.name = "new base".into();
        object.size = DVec2::new(48.0, 24.0);
        object.rotation = 90.0;
        object.opacity = 0.8;
        object.visible = false;
        object.shape = Shape::Point;
        assert_eq!(
            refresh_template_instances(&mut map, path, &template, &after).unwrap(),
            vec![ObjectId(1)]
        );
        let changed = map.object(ObjectId(1)).unwrap();
        assert_eq!(changed.pos, DVec2::new(4.0, 8.0));
        assert_eq!(changed.name, "new base");
        assert_eq!(changed.size, DVec2::new(48.0, 24.0));
        assert_eq!(changed.shape, Shape::Point);
        assert_eq!(map.object(ObjectId(2)).unwrap(), &own);
        assert!(Arc::ptr_eq(
            &map.tilesets[1].tileset,
            &after.tilesets[0].tileset
        ));
    }
}
