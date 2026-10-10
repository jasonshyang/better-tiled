//! The object clipboard: copied objects with the tilesets their tiles come from, so they paste
//! into any map (Tiled's `ClipboardManager::copySelection` and `pasteObjectGroup`,
//! `reference/tiled/src/tiled/clipboardmanager.cpp`, with `ObjectReferencesHelper`,
//! `tiled/objectreferenceshelper.cpp`).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bt_core::{
    Cell, LayerId, Map, MapGeometry, MapObject, ObjectId, ObjectLayer, TilesetRef, TilesetSlot,
};
use glam::DVec2;

use crate::error::EditError;
use crate::ids::NewIds;
use crate::objects::bounds::{object_bounds, union_of};
use crate::objects::edit::{remove_objects, remove_objects_in, renumbered};
use crate::objects::find::{ids_by_layer, object_layer, object_layer_mut, objects_in};
use crate::objects::snap::{SnapMode, snap};
use crate::objects::space::ObjectSpace;
use crate::report::ObjectsReport;
use crate::tileset_ops::unify_tileset;

/// Objects put aside by Copy or Cut, with the tilesets their tiles come from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjectClip {
    /// The objects as they were (their ids and positions where they came from), in map order.
    pub objects: Vec<MapObject>,
    /// The tilesets tile objects' cells name: a cell's slot is a place in this list.
    pub tilesets: Vec<TilesetRef>,
}

/// Copies objects of `map`, in map order, with the tilesets their tiles use.
pub fn copy_objects(map: &Map, ids: &[ObjectId]) -> Result<ObjectClip, EditError> {
    let mut copied = Vec::new();
    for group in ids_by_layer(map, ids)? {
        let wanted: HashSet<ObjectId> = group.ids.iter().copied().collect();
        let objects = object_layer(map, group.layer)?;
        copied.extend(
            objects
                .objects
                .iter()
                .filter(|o| wanted.contains(&o.id))
                .cloned(),
        );
    }
    clip_of(copied, &map.tilesets)
}

/// Copies objects of a list, in list order, their tiles from `space`'s tilesets.
pub fn copy_objects_in(
    objects: &ObjectLayer,
    space: &ObjectSpace,
    ids: &[ObjectId],
) -> Result<ObjectClip, EditError> {
    let wanted: HashSet<ObjectId> = objects_in(objects, ids)?.iter().map(|o| o.id).collect();
    let copied = objects
        .objects
        .iter()
        .filter(|o| wanted.contains(&o.id))
        .cloned()
        .collect();
    clip_of(copied, space.tilesets())
}

/// Cut: copies objects of `map`, then removes them.
pub fn cut_objects(map: &mut Map, ids: &[ObjectId]) -> Result<ObjectClip, EditError> {
    let clip = copy_objects(map, ids)?;
    remove_objects(map, ids)?;
    Ok(clip)
}

/// Cut on a list.
pub fn cut_objects_in(
    objects: &mut ObjectLayer,
    space: &ObjectSpace,
    ids: &[ObjectId],
) -> Result<ObjectClip, EditError> {
    let clip = copy_objects_in(objects, space, ids)?;
    remove_objects_in(objects, ids)?;
    Ok(clip)
}

/// A clip of `objects`, their cells renumbered into the tilesets they use, in the order first
/// used.
fn clip_of(mut objects: Vec<MapObject>, tilesets: &[TilesetRef]) -> Result<ObjectClip, EditError> {
    let mut used: Vec<TilesetRef> = Vec::new();
    let mut slots: HashMap<TilesetSlot, TilesetSlot> = HashMap::new();
    for object in &mut objects {
        let Some(cell) = object.tile else {
            continue;
        };
        let slot = cell.slot().ok_or(EditError::EmptyCell)?;
        let clip_slot = match slots.get(&slot) {
            Some(&clip_slot) => clip_slot,
            None => {
                let tileset = tilesets
                    .get(slot.index())
                    .ok_or(EditError::NoSuchTileset(slot))?;
                let clip_slot = TilesetSlot::new(used.len()).ok_or(EditError::TooManyTilesets)?;
                used.push(tileset.clone());
                slots.insert(slot, clip_slot);
                clip_slot
            }
        };
        object.tile = Some(Cell::new(clip_slot, cell.tile()).with_flip(cell.flip()));
    }
    Ok(ObjectClip {
        objects,
        tilesets: used,
    })
}

/// Pastes a clip into the object layer `layer` of `map`, on top, moved by `offset` (see
/// [`paste_offset`]; Paste in Place uses none): the objects take the map's next ids in clip
/// order, references among them follow them, and their tiles' tilesets are added to the map
/// unless it has them (Tiled's `unifyTilesets`). Reports the new ids in clip order.
pub fn paste_objects(
    map: &mut Map,
    layer: LayerId,
    clip: &ObjectClip,
    offset: DVec2,
) -> Result<ObjectsReport, EditError> {
    object_layer(map, layer)?;
    for object in &clip.objects {
        if let Some(cell) = object.tile {
            let slot = cell.slot().ok_or(EditError::EmptyCell)?;
            if clip.tilesets.get(slot.index()).is_none() {
                return Err(EditError::NoSuchTileset(slot));
            }
        }
    }
    let mut ids = NewIds::from_map(map);
    let mut pasted = renumbered(clip.objects.clone(), offset, &mut ids)?;
    let mut slots: HashMap<TilesetSlot, TilesetSlot> = HashMap::new();
    for copy in &mut pasted {
        let Some(cell) = copy.object.tile else {
            continue;
        };
        let clip_slot = cell.slot().ok_or(EditError::EmptyCell)?;
        let slot = match slots.get(&clip_slot) {
            Some(&slot) => slot,
            None => {
                let tileset = clip
                    .tilesets
                    .get(clip_slot.index())
                    .ok_or(EditError::NoSuchTileset(clip_slot))?;
                let slot = unify_tileset(map, tileset)?.slot;
                slots.insert(clip_slot, slot);
                slot
            }
        };
        copy.object.tile = Some(Cell::new(slot, cell.tile()).with_flip(cell.flip()));
    }
    let changed = pasted.iter().map(|c| c.object.id).collect();
    Arc::make_mut(&mut object_layer_mut(map, layer)?.objects)
        .extend(pasted.into_iter().map(|c| c.object));
    ids.store_in(map);
    Ok(ObjectsReport { changed })
}

/// Pastes a clip into a list, on top, moved by `offset`, with ids from `ids`. Tile objects are
/// left out: their tiles need a map's tilesets, and a tile's collision shapes draw none (Tiled
/// pastes into the collision editor the same way). Reports the new ids in clip order.
pub fn paste_objects_in(
    objects: &mut ObjectLayer,
    clip: &ObjectClip,
    offset: DVec2,
    ids: &mut NewIds,
) -> Result<ObjectsReport, EditError> {
    let shapes = clip
        .objects
        .iter()
        .filter(|o| o.tile.is_none())
        .cloned()
        .collect();
    let pasted = renumbered(shapes, offset, ids)?;
    if pasted.is_empty() {
        return Ok(ObjectsReport::default());
    }
    let changed = pasted.iter().map(|c| c.object.id).collect();
    Arc::make_mut(&mut objects.objects).extend(pasted.into_iter().map(|c| c.object));
    Ok(ObjectsReport { changed })
}

/// Where Paste puts a clip: moved so the middle of what its objects cover is under `cursor`
/// (pixel space of the target layer), the move itself snapped by `mode`, as Tiled snaps it.
/// The clip is laid out on the target's grid.
pub fn paste_offset(
    geometry: &MapGeometry,
    clip: &ObjectClip,
    cursor: DVec2,
    mode: SnapMode,
) -> DVec2 {
    let space = ObjectSpace::from_parts(*geometry, &clip.tilesets);
    let middle = union_of(clip.objects.iter().map(|o| object_bounds(&space, o)))
        .map_or(DVec2::ZERO, |bounds| (bounds.min + bounds.max) * 0.5);
    snap(geometry, cursor - middle, mode)
}

#[cfg(test)]
mod tests {
    use bt_core::{FileRef, Property, TileId, Value};
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::UVec2;

    use super::*;
    use crate::objects::find::find_object;
    use crate::testutil::{assert_valid, ids_of, rect_object};

    /// A fixture atlas; “props” is cut differently, so no other stands in for it.
    fn atlas(name: &str) -> Arc<bt_core::Tileset> {
        let builder = Atlas::builder().name(name);
        let builder = if name == "props" {
            builder.grid(8, 2)
        } else {
            builder
        };
        builder.build_embedded().unwrap().tileset
    }

    fn tile_of(slot: usize, id: u32) -> Cell {
        Cell::new(TilesetSlot::new(slot).unwrap(), TileId(id))
    }

    /// A map with two tilesets; objects 1 (a box), 2 (a tile of the second tileset, pointing at
    /// 1 and at 3) and 3 (far off).
    fn source() -> Map {
        let mut linked = MapObject {
            tile: Some(tile_of(1, 2).with_flip(bt_core::Flip::H)),
            ..rect_object(32.0, 48.0, 16.0, 16.0)
        };
        linked
            .properties
            .insert("target".into(), Property::new(Value::Object(ObjectId(1))));
        linked.properties.insert(
            "elsewhere".into(),
            Property::new(Value::Object(ObjectId(3))),
        );
        MapBuilder::new(UVec2::splat(16), UVec2::splat(10))
            .tileset(atlas("ground"))
            .tileset(atlas("props"))
            .object_layer(
                "things",
                vec![
                    rect_object(0.0, 0.0, 16.0, 16.0),
                    linked,
                    rect_object(100.0, 100.0, 4.0, 4.0),
                ],
            )
            .build()
            .unwrap()
    }

    #[test]
    fn copies_carry_the_tilesets_they_use() {
        let map = source();
        let clip = copy_objects(&map, &[ObjectId(2), ObjectId(1)]).unwrap();
        assert_eq!(
            clip.objects.iter().map(|o| o.id).collect::<Vec<_>>(),
            [ObjectId(1), ObjectId(2)],
            "in map order"
        );
        assert_eq!(clip.tilesets.len(), 1, "only the tile object's tileset");
        assert_eq!(clip.tilesets[0].tileset.name, "props");
        assert_eq!(
            clip.objects[1].tile,
            Some(tile_of(0, 2).with_flip(bt_core::Flip::H))
        );
        assert_eq!(
            copy_objects(&map, &[ObjectId(9)]),
            Err(EditError::NoSuchObject(ObjectId(9)))
        );
    }

    #[test]
    fn pasting_renumbers_rewires_and_unifies_tilesets() {
        let clip = copy_objects(&source(), &[ObjectId(1), ObjectId(2)]).unwrap();
        let mut other = MapBuilder::new(UVec2::splat(16), UVec2::splat(4))
            .tileset(atlas("other"))
            .object_layer("target", vec![rect_object(0.0, 0.0, 1.0, 1.0)])
            .build()
            .unwrap();
        other.next_object_id = 50;
        let report = paste_objects(&mut other, LayerId(1), &clip, DVec2::new(10.0, 0.0)).unwrap();
        assert_eq!(report.changed, [ObjectId(50), ObjectId(51)]);
        assert_eq!(other.tilesets.len(), 2, "props was added");
        let pasted = find_object(&other, ObjectId(51)).unwrap().object;
        assert_eq!(pasted.tile, Some(tile_of(1, 2).with_flip(bt_core::Flip::H)));
        assert_eq!(pasted.pos, DVec2::new(42.0, 48.0));
        assert_eq!(
            pasted.properties["target"].value,
            Value::Object(ObjectId(50)),
            "references follow the copies"
        );
        assert_eq!(
            pasted.properties["elsewhere"].value,
            Value::Object(ObjectId(3)),
            "and leave the rest"
        );
        assert_eq!(other.next_object_id, 52);
        paste_objects(&mut other, LayerId(1), &clip, DVec2::ZERO).unwrap();
        assert_eq!(other.tilesets.len(), 2, "the tileset is found again");
        assert_eq!(
            ids_of(other.layers[0].as_object_layer().unwrap()),
            [1, 50, 51, 52, 53]
        );
        assert_valid(&other);
        assert_eq!(
            paste_objects(&mut other, LayerId(9), &clip, DVec2::ZERO),
            Err(EditError::NoSuchLayer(LayerId(9)))
        );
        let broken = ObjectClip {
            objects: vec![
                clip.objects[1].clone(),
                MapObject {
                    tile: Some(tile_of(3, 0)),
                    ..rect_object(0.0, 0.0, 16.0, 16.0)
                },
            ],
            tilesets: clip.tilesets.clone(),
        };
        let mut fresh = MapBuilder::new(UVec2::splat(16), UVec2::splat(4))
            .object_layer("target", Vec::new())
            .build()
            .unwrap();
        let before = fresh.clone();
        assert_eq!(
            paste_objects(&mut fresh, LayerId(1), &broken, DVec2::ZERO),
            Err(EditError::NoSuchTileset(TilesetSlot::new(3).unwrap()))
        );
        assert_eq!(
            fresh, before,
            "no tileset added for a clip that cannot paste"
        );
    }

    #[test]
    fn cut_and_paste_in_place_puts_objects_back() {
        let mut map = source();
        let clip = cut_objects(&mut map, &[ObjectId(3)]).unwrap();
        assert_eq!(ids_of(map.layers[0].as_object_layer().unwrap()), [1, 2]);
        let report = paste_objects(&mut map, LayerId(1), &clip, DVec2::ZERO).unwrap();
        let pasted = find_object(&map, report.changed[0]).unwrap().object;
        assert_eq!(pasted.pos, DVec2::new(100.0, 100.0));
        assert_eq!(pasted.id, ObjectId(4), "a new id; 3 is not given again");
        assert_valid(&map);
    }

    #[test]
    fn the_paste_offset_centres_the_clip_under_the_cursor() {
        let map = source();
        let geometry = map.geometry();
        let one = copy_objects(&map, &[ObjectId(1)]).unwrap();
        assert_eq!(
            paste_offset(&geometry, &one, DVec2::new(100.0, 100.0), SnapMode::None),
            DVec2::new(92.0, 92.0)
        );
        assert_eq!(
            paste_offset(&geometry, &one, DVec2::new(100.0, 100.0), SnapMode::Grid),
            DVec2::new(96.0, 96.0)
        );
        // The tile object hangs up from its position: its image covers 32..48 down.
        let two = copy_objects(&map, &[ObjectId(1), ObjectId(2)]).unwrap();
        assert_eq!(
            paste_offset(&geometry, &two, DVec2::ZERO, SnapMode::None),
            DVec2::new(-24.0, -24.0)
        );
        assert_eq!(
            paste_offset(
                &geometry,
                &ObjectClip::default(),
                DVec2::new(3.0, 4.0),
                SnapMode::None
            ),
            DVec2::new(3.0, 4.0)
        );
    }

    #[test]
    fn collision_shapes_paste_without_tile_objects() {
        let clip = copy_objects(&source(), &[ObjectId(1), ObjectId(2), ObjectId(3)]).unwrap();
        let mut shapes = ObjectLayer {
            objects: Arc::new(vec![MapObject {
                id: ObjectId(4),
                ..rect_object(0.0, 0.0, 2.0, 2.0)
            }]),
            ..ObjectLayer::default()
        };
        let mut ids = NewIds::after_highest(&shapes);
        let report = paste_objects_in(&mut shapes, &clip, DVec2::ZERO, &mut ids).unwrap();
        assert_eq!(report.changed, [ObjectId(5), ObjectId(6)]);
        assert_eq!(ids_of(&shapes), [4, 5, 6]);
        let tileset = atlas("walls");
        let space = ObjectSpace::of_tile(&tileset, TileId(0));
        let cut = cut_objects_in(&mut shapes, &space, &[ObjectId(5)]).unwrap();
        assert_eq!(cut.objects.len(), 1);
        assert!(cut.tilesets.is_empty());
        assert_eq!(ids_of(&shapes), [4, 6]);
    }

    #[test]
    fn template_instances_paste_as_instances() {
        let mut map = source();
        crate::objects::find::object_mut(&mut map, ObjectId(1))
            .unwrap()
            .template = Some(FileRef::new("box.tx", "/maps/box.tx"));
        let clip = copy_objects(&map, &[ObjectId(1)]).unwrap();
        let report = paste_objects(&mut map, LayerId(1), &clip, DVec2::ZERO).unwrap();
        let pasted = find_object(&map, report.changed[0]).unwrap().object;
        assert_eq!(
            pasted.template,
            Some(FileRef::new("box.tx", "/maps/box.tx"))
        );
        assert_valid(&map);
    }
}
