//! Re-indexing tiles after an atlas image changed its column count, so that tiles keep showing
//! the same picture: a tile keeps its row and column, and its id follows them. Ported from
//! Tiled's `AdjustTileIndexes` and `AdjustTileMetaData`
//! (`reference/tiled/src/tiled/adjusttileindexes.cpp`), with the check of
//! `mayNeedColumnCountAdjustment` (`tiled/documentmanager.cpp`). The editor asks before
//! applying them, as Tiled does.

use std::collections::BTreeMap;
use std::sync::Arc;

use bt_core::{Cell, Frame, Map, MapObject, ObjectOverrides, TileId, Tileset, TilesetSlot};
use glam::UVec2;

use crate::error::EditError;
use crate::report::AdjustReport;

/// Whether tile references need adjusting after an atlas grid went from `old` to `new` columns
/// by rows: the column count changed, neither grid is empty of columns, and both have at least
/// two rows (in a single row a tile's id is its column, whatever the count).
pub fn columns_changed(old: UVec2, new: UVec2) -> bool {
    old.x != new.x && old.x > 0 && new.x > 0 && old.y >= 2 && new.y >= 2
}

/// Where tile `id` of a grid of `old` columns goes in a grid of `new` columns: the same row
/// and column, or `None` when its column is gone.
fn adjusted(id: TileId, old: u32, new: u32) -> Option<TileId> {
    let row = id.0.checked_div(old)?;
    let column = id.0.checked_rem(old)?;
    if column >= new {
        return None;
    }
    row.checked_mul(new)?.checked_add(column).map(TileId)
}

/// Adjust Tile Indexes in a map: the cells and tile objects showing tiles of the atlas in
/// `slot`, which went from `old_columns` to `new_columns` columns, name the tile at the same row
/// and column again, keeping their flips. Cells whose column is gone are erased and counted;
/// tile objects whose column is gone lose their tile and stay, as in Tiled. A template
/// instance keeps its template's tile unless it sets its own. Nothing changes when either count
/// is zero or they are equal.
pub fn adjust_tile_indexes(
    map: &mut Map,
    slot: TilesetSlot,
    old_columns: u32,
    new_columns: u32,
) -> Result<AdjustReport, EditError> {
    if map.tilesets.get(slot.index()).is_none() {
        return Err(EditError::NoSuchTileset(slot));
    }
    let mut report = AdjustReport::default();
    if old_columns == 0 || new_columns == 0 || old_columns == new_columns {
        return Ok(report);
    }
    let adjust = |cell: Cell| -> Cell {
        if cell.slot() != Some(slot) {
            return cell;
        }
        match adjusted(cell.tile(), old_columns, new_columns) {
            Some(tile) => Cell::new(slot, tile).with_flip(cell.flip()),
            None => Cell::EMPTY,
        }
    };
    map.for_each_layer_mut(|layer| {
        let Some(tiles) = layer.as_tile_layer_mut() else {
            return;
        };
        let mut moved = 0;
        let mut erased = 0;
        tiles.rewrite_cells(|cell| {
            let new = adjust(cell);
            if new.is_empty() {
                erased += 1;
            } else if new != cell {
                moved += 1;
            }
            new
        });
        if erased > 0 {
            tiles.prune_empty_chunks();
        }
        report.moved_cells += moved;
        report.erased_cells += erased;
    });
    // A template instance that does not set its tile shows its template's.
    let own_tile = |object: &MapObject| {
        object.template.is_none() || object.overrides.contains(ObjectOverrides::TILE)
    };
    let changes = |object: &MapObject| {
        own_tile(object) && object.tile.is_some_and(|tile| adjust(tile) != tile)
    };
    map.for_each_layer_mut(|layer| {
        let Some(objects) = layer.as_object_layer_mut() else {
            return;
        };
        if !objects.objects.iter().any(changes) {
            return;
        }
        for object in Arc::make_mut(&mut objects.objects) {
            if !changes(object) {
                continue;
            }
            let new = object.tile.map(adjust).filter(|cell| !cell.is_empty());
            match new {
                Some(_) => report.moved_objects.push(object.id),
                None => report.untiled_objects.push(object.id),
            }
            object.tile = new;
        }
    });
    Ok(report)
}

/// Adjust Tile Indexes in an atlas tileset itself: its per-tile data (class, probability,
/// properties, collision shapes, animation), its animation frames and its terrain sets' labels
/// and pictures move to the tile at the same row and column after the atlas went from
/// `old_columns` to `new_columns` columns, as the cells do. What sits in a column that is gone
/// is dropped. Returns whether anything changed; nothing does for a collection, which has no
/// columns, or when either count is zero or they are equal.
pub fn adjust_tile_metadata(tileset: &mut Tileset, old_columns: u32, new_columns: u32) -> bool {
    if tileset.is_collection() || old_columns == 0 || new_columns == 0 || old_columns == new_columns
    {
        return false;
    }
    let adjust = |id: TileId| adjusted(id, old_columns, new_columns);
    let frames = |frames: &[Frame]| -> Vec<Frame> {
        frames
            .iter()
            .filter_map(|frame| {
                Some(Frame {
                    tile: adjust(frame.tile)?,
                    duration_ms: frame.duration_ms,
                })
            })
            .collect()
    };
    let mut tiles = BTreeMap::new();
    for (&id, data) in &tileset.tiles {
        let Some(to) = adjust(id) else {
            continue;
        };
        let mut moved = data.clone();
        moved.animation = frames(&data.animation);
        tiles.insert(to, moved);
    }
    let mut changed = tiles != tileset.tiles;
    tileset.tiles = tiles;
    for set in &mut tileset.wang_sets {
        let picture = set.tile.and_then(adjust);
        changed |= picture != set.tile;
        set.tile = picture;
        for color in &mut set.colors {
            let picture = color.tile.and_then(adjust);
            changed |= picture != color.tile;
            color.tile = picture;
        }
        let mut labels = BTreeMap::new();
        for (&tile, &id) in &set.tiles {
            if let Some(to) = adjust(tile) {
                labels.insert(to, id);
            }
        }
        changed |= labels != set.tiles;
        set.tiles = labels;
    }
    changed
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bt_core::{
        Color, FileRef, Property, TileData, Value, WangColor, WangId, WangSet, WangSetType,
    };
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use bt_utils::IRect;

    use super::*;
    use crate::testutil::{grid_of, rect_object, tile, trimmed};

    #[test]
    fn only_grids_of_two_rows_or_more_need_adjusting() {
        let grid = UVec2::new;
        assert!(columns_changed(grid(8, 4), grid(10, 4)));
        assert!(columns_changed(grid(8, 4), grid(3, 2)));
        assert!(!columns_changed(grid(8, 4), grid(8, 5)), "the same columns");
        assert!(!columns_changed(grid(8, 1), grid(10, 1)), "one row");
        assert!(!columns_changed(grid(8, 4), grid(10, 1)), "one row after");
        assert!(!columns_changed(grid(0, 4), grid(10, 4)));
        assert!(!columns_changed(grid(8, 4), grid(0, 4)));
    }

    #[test]
    fn there_and_back_keeps_every_tile_whose_column_stays() {
        let fuzz = bt_test_data::Fuzz::from_env(0xAD_1057, 200);
        for case in 0..fuzz.cases {
            let mut rng = bt_utils::rng::Rng::new(fuzz.case_seed(case));
            let mut pick =
                |n: u32| u32::try_from(rng.below(u64::from(n)).unwrap_or(0)).unwrap_or(0);
            let old = 1 + pick(8);
            let new = 1 + pick(8);
            let rows = 1 + pick(6);
            let atlas = Atlas::builder()
                .grid(old.max(new), rows)
                .build_embedded()
                .unwrap();
            let size = UVec2::new(6, 4);
            let mut map = MapBuilder::new(UVec2::splat(16), size)
                .tileset(atlas.tileset)
                .tile_layer(
                    "ground",
                    ". . . . . .\n. . . . . .\n. . . . . .\n. . . . . .",
                )
                .build()
                .unwrap();
            let mut kept = 0;
            if let Some(ground) = map.layers[0].as_tile_layer_mut() {
                for at in IRect::from_min_size(glam::IVec2::ZERO, size.as_ivec2()).points() {
                    if pick(3) == 0 {
                        continue;
                    }
                    let id = pick(rows) * old + pick(old);
                    kept += usize::from(id % old < new);
                    ground.set_cell(at, tile(id)).unwrap();
                }
            }
            let original = map.clone();
            let slot = TilesetSlot::new(0).unwrap();
            let there = adjust_tile_indexes(&mut map, slot, old, new).unwrap();
            let back = adjust_tile_indexes(&mut map, slot, new, old).unwrap();
            let context = format!("case {case} ({}): {old} → {new}", fuzz.reproduce());
            assert_eq!(back.erased_cells, 0, "{context}");
            let before = original.layers[0].as_tile_layer().unwrap();
            let after = map.layers[0].as_tile_layer().unwrap();
            assert_eq!(after.cells().count(), kept, "{context}");
            for placed in before.cells() {
                let column = placed.cell.tile().0 % old;
                let expected = if column < new {
                    placed.cell
                } else {
                    Cell::EMPTY
                };
                assert_eq!(
                    after.cell(placed.at),
                    expected,
                    "{context} at {}",
                    placed.at
                );
            }
            assert_eq!(
                there.erased_cells + kept,
                before.cells().count(),
                "{context}"
            );
        }
    }

    #[test]
    fn tiles_keep_their_row_and_column() {
        // Four columns become three: (row 1, column 1) was 5 and is 4.
        assert_eq!(adjusted(TileId(5), 4, 3), Some(TileId(4)));
        assert_eq!(adjusted(TileId(7), 4, 3), None, "column 3 is gone");
        assert_eq!(
            adjusted(TileId(2), 4, 3),
            Some(TileId(2)),
            "the first row stays"
        );
        assert_eq!(adjusted(TileId(8), 4, 3), Some(TileId(6)));
        // Three become five.
        assert_eq!(adjusted(TileId(3), 3, 5), Some(TileId(5)));
        assert_eq!(adjusted(TileId(8), 3, 5), Some(TileId(12)));
        assert_eq!(
            adjusted(TileId(u32::MAX), 1, 2),
            None,
            "past what an id holds"
        );
    }

    fn template() -> Option<FileRef> {
        Some(FileRef::new("crate.tx", PathBuf::from("/maps/crate.tx")))
    }

    #[test]
    fn cells_and_tile_objects_follow_their_pictures() {
        // The atlas is now three columns wide; the map still names tiles of four columns.
        let narrow = Atlas::builder().grid(3, 4).build_embedded().unwrap();
        let other = Atlas::builder().name("other").build_embedded().unwrap();
        let instance = MapObject {
            tile: Some(tile(5)),
            template: template(),
            ..rect_object(0.0, 0.0, 16.0, 16.0)
        };
        let overriding = MapObject {
            tile: Some(tile(6)),
            template: template(),
            overrides: ObjectOverrides::TILE,
            ..rect_object(0.0, 0.0, 16.0, 16.0)
        };
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::new(4, 2))
            .tileset(narrow.tileset)
            .tileset(other.tileset)
            .tile_layer("ground", "5h 7 2 8\n. 5@1 3 .")
            .object_layer(
                "things",
                vec![
                    MapObject {
                        tile: Some(tile(5)),
                        ..rect_object(0.0, 0.0, 16.0, 16.0)
                    },
                    instance,
                    overriding,
                    MapObject {
                        tile: Some(tile(3)),
                        ..rect_object(0.0, 0.0, 16.0, 16.0)
                    },
                ],
            )
            .build()
            .unwrap();
        let slot = TilesetSlot::new(0).unwrap();
        let report = adjust_tile_indexes(&mut map, slot, 4, 3).unwrap();
        assert_eq!(
            report,
            AdjustReport {
                moved_cells: 2,
                erased_cells: 2,
                moved_objects: vec![bt_core::ObjectId(1), bt_core::ObjectId(3)],
                untiled_objects: vec![bt_core::ObjectId(4)],
            }
        );
        let all = IRect {
            x: 0,
            y: 0,
            w: 4,
            h: 2,
        };
        let ground = map.layers[0].as_tile_layer().unwrap();
        assert_eq!(
            grid_of(ground, all),
            trimmed("4h . 2 6\n. 5@1 . ."),
            "flips stay, other tilesets are left alone"
        );
        let objects = &map.layers[1].as_object_layer().unwrap().objects;
        assert_eq!(
            objects.iter().map(|o| o.tile).collect::<Vec<_>>(),
            [Some(tile(4)), Some(tile(5)), Some(tile(5)), None],
            "the plain instance keeps its template's tile"
        );
        assert_eq!(map.validate(), []);
        assert_eq!(
            adjust_tile_indexes(&mut map, slot, 3, 3),
            Ok(AdjustReport::default())
        );
        assert_eq!(
            adjust_tile_indexes(&mut map, TilesetSlot::new(5).unwrap(), 4, 3),
            Err(EditError::NoSuchTileset(TilesetSlot::new(5).unwrap()))
        );
    }

    #[test]
    fn widening_moves_tiles_down_the_ids() {
        let wide = Atlas::builder().grid(5, 3).build_embedded().unwrap();
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::new(3, 1))
            .tileset(wide.tileset)
            .tile_layer("ground", "3 4 8")
            .build()
            .unwrap();
        let report = adjust_tile_indexes(&mut map, TilesetSlot::new(0).unwrap(), 3, 5).unwrap();
        assert_eq!((report.moved_cells, report.erased_cells), (3, 0));
        let all = IRect {
            x: 0,
            y: 0,
            w: 3,
            h: 1,
        };
        assert_eq!(
            grid_of(map.layers[0].as_tile_layer().unwrap(), all),
            "5 6 12"
        );
        assert_eq!(map.validate(), []);
    }

    fn named(name: &str) -> TileData {
        let mut data = TileData::default();
        data.properties
            .insert("name".into(), Property::new(Value::String(name.into())));
        data
    }

    fn name_of(tileset: &Tileset, id: u32) -> Option<String> {
        match &tileset.tile(TileId(id))?.properties.get("name")?.value {
            Value::String(name) => Some(name.clone()),
            _ => None,
        }
    }

    fn frame(tile: u32, duration_ms: u32) -> Frame {
        Frame {
            tile: TileId(tile),
            duration_ms,
        }
    }

    fn terrain(tile: Option<u32>, color_tile: Option<u32>, labels: &[u32]) -> WangSet {
        WangSet {
            name: "ground".into(),
            class: String::new(),
            kind: WangSetType::Corner,
            tile: tile.map(TileId),
            colors: vec![WangColor {
                name: "grass".into(),
                class: String::new(),
                color: Color::rgb(0, 255, 0),
                tile: color_tile.map(TileId),
                probability: 1.0,
                properties: Default::default(),
            }],
            tiles: labels
                .iter()
                .map(|&id| (TileId(id), WangId::from_lanes([0, 1, 0, 1, 0, 1, 0, 1])))
                .collect(),
            properties: Default::default(),
        }
    }

    #[test]
    fn tile_data_animations_and_terrains_move_with_their_pictures() {
        // A 3×3 atlas widened to 4 columns.
        let mut tileset = Tileset::clone(
            &Atlas::builder()
                .grid(4, 3)
                .build_embedded()
                .unwrap()
                .tileset,
        );
        tileset.tiles = [
            (TileId(4), named("middle")),
            (TileId(5), named("right")),
            (TileId(2), named("top right")),
        ]
        .into();
        if let Some(middle) = tileset.tiles.get_mut(&TileId(4)) {
            middle.animation = vec![frame(4, 100), frame(8, 50)];
        }
        if let Some(top) = tileset.tiles.get_mut(&TileId(2)) {
            top.animation = vec![frame(5, 10)];
        }
        tileset.wang_sets = vec![terrain(Some(4), Some(8), &[3, 1])];
        assert!(adjust_tile_metadata(&mut tileset, 3, 4));
        assert_eq!(
            name_of(&tileset, 5).as_deref(),
            Some("middle"),
            "4 = (1, 1) → 5"
        );
        assert_eq!(
            name_of(&tileset, 6).as_deref(),
            Some("right"),
            "5 = (1, 2) → 6"
        );
        assert_eq!(
            name_of(&tileset, 2).as_deref(),
            Some("top right"),
            "the first row stays"
        );
        assert_eq!(
            name_of(&tileset, 4),
            None,
            "3 = (1, 0) had nothing to bring"
        );
        assert_eq!(
            tileset.tiles[&TileId(5)].animation,
            [frame(5, 100), frame(10, 50)]
        );
        assert_eq!(
            tileset.tiles[&TileId(2)].animation,
            [frame(6, 10)],
            "the frames of a tile that stays follow their pictures too"
        );
        let set = &tileset.wang_sets[0];
        assert_eq!(
            (set.tile, set.colors[0].tile),
            (Some(TileId(5)), Some(TileId(10)))
        );
        assert_eq!(
            set.tiles.keys().copied().collect::<Vec<_>>(),
            [TileId(1), TileId(4)]
        );
        assert!(!adjust_tile_metadata(&mut tileset, 4, 4));
    }

    #[test]
    fn narrowing_drops_what_was_in_the_lost_columns() {
        // Four columns become three: column 3 is gone.
        let mut tileset = Tileset::clone(
            &Atlas::builder()
                .grid(3, 4)
                .build_embedded()
                .unwrap()
                .tileset,
        );
        tileset.tiles = [(TileId(3), named("edge")), (TileId(6), named("inner"))].into();
        if let Some(inner) = tileset.tiles.get_mut(&TileId(6)) {
            inner.animation = vec![frame(7, 10), frame(6, 10)];
        }
        tileset.wang_sets = vec![terrain(Some(7), Some(5), &[7, 5])];
        assert!(adjust_tile_metadata(&mut tileset, 4, 3));
        assert_eq!(name_of(&tileset, 3), None);
        assert_eq!(
            name_of(&tileset, 5).as_deref(),
            Some("inner"),
            "6 = (1, 2) → 5"
        );
        assert_eq!(tileset.tiles[&TileId(5)].animation, [frame(5, 10)]);
        let set = &tileset.wang_sets[0];
        assert_eq!(set.tile, None, "its picture is gone");
        assert_eq!(set.colors[0].tile, Some(TileId(4)));
        assert_eq!(set.tiles.keys().copied().collect::<Vec<_>>(), [TileId(4)]);
        let mut collection = Tileset::default();
        assert!(!adjust_tile_metadata(&mut collection, 4, 3), "no columns");
    }
}
