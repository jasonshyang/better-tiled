//! Copy, Cut and erasing: tiles taken out of a map's tile layers inside a region (Tiled's
//! `Map::copyLayers`, `MapDocument::eraseTileLayers` and the Cut and Delete actions,
//! `reference/tiled/src/libtiled/map.cpp`, `reference/tiled/src/tiled/mapdocument.cpp` and
//! `mapdocumentactionhandler.cpp`).

use std::collections::BTreeSet;

use bt_core::{CHUNK_SIZE, Cell, Layer, LayerId, Map, PlacedCell, TileLayer};
use bt_utils::IRect;
use glam::IVec2;

use super::{ClipLayer, ClipTilesets, TileClip};
use crate::error::EditError;
use crate::region::TileRegion;
use crate::report::{EraseReport, LayerChange};

/// What Cut took and what it emptied.
#[derive(Clone, Debug, PartialEq)]
pub struct TileCut {
    /// The tiles taken, as [`copy`] gives them.
    pub clip: TileClip,
    /// The cells emptied, and the locked layers whose tiles stayed.
    pub erased: EraseReport,
}

/// The tile layers among `layers`, in the map's order (bottom to top, groups' children in
/// place of the group), each once. Layers of other kinds are passed over: a selection of layers
/// holds them too. An id the map does not have is an error.
pub(crate) fn tile_layers_among(map: &Map, layers: &[LayerId]) -> Result<Vec<LayerId>, EditError> {
    let wanted: BTreeSet<LayerId> = layers.iter().copied().collect();
    if let Some(&missing) = wanted.iter().find(|&&id| map.layer(id).is_none()) {
        return Err(EditError::NoSuchLayer(missing));
    }
    let mut found = Vec::new();
    map.for_each_layer(|layer, _| {
        if layer.as_tile_layer().is_some() && wanted.contains(&layer.id) {
            found.push(layer.id);
        }
    });
    Ok(found)
}

/// The tiles of `layer` inside `region`, row by row. Visits whichever is smaller: the region's
/// cells or the layer's allocated chunks.
pub(crate) fn tiles_in(layer: &TileLayer, region: &TileRegion) -> Vec<PlacedCell> {
    let allocated = layer.chunks().count() * usize::try_from(CHUNK_SIZE * CHUNK_SIZE).unwrap_or(0);
    let mut tiles: Vec<PlacedCell> = if region.len() <= allocated {
        region
            .iter()
            .map(|at| PlacedCell {
                at,
                cell: layer.cell(at),
            })
            .filter(|placed| !placed.cell.is_empty())
            .collect()
    } else {
        layer
            .cells()
            .filter(|placed| region.contains(placed.at))
            .collect()
    };
    tiles.sort_unstable_by_key(|placed| (placed.at.y, placed.at.x));
    tiles
}

/// Copy: the tiles of the tile layers among `layers` inside `region`, one clip layer for each
/// in the map's order with its name, opacity and tint, whether locked or not. The clip covers
/// the region's bounding rectangle, which is its origin and size; cells outside the region stay
/// empty, and the tilesets listed are the ones the copied cells use. An empty region gives the
/// layers without tiles.
pub fn copy(map: &Map, layers: &[LayerId], region: &TileRegion) -> Result<TileClip, EditError> {
    let bounds = region.bounds().unwrap_or_default();
    let mut tilesets = ClipTilesets::new();
    let mut clip_layers = Vec::new();
    for id in tile_layers_among(map, layers)? {
        let Some(layer) = map.layer(id) else {
            continue;
        };
        let Some(cells) = layer.as_tile_layer() else {
            continue;
        };
        let mut copied = ClipLayer {
            opacity: layer.opacity,
            tint: layer.tint,
            ..ClipLayer::named(layer.name.clone())
        };
        for placed in tiles_in(cells, region) {
            let cell = tilesets.adopt(&map.tilesets, placed.cell)?;
            copied.put(placed.at - bounds.min(), cell);
        }
        clip_layers.push(copied);
    }
    Ok(TileClip {
        origin: bounds.min(),
        size: bounds.size(),
        layers: clip_layers,
        tilesets: tilesets.into_list(),
    })
}

/// Cut: [`copy`], then [`erase`] the same cells, which leaves the locked layers' tiles where
/// they are (they are in the clip all the same).
pub fn cut(map: &mut Map, layers: &[LayerId], region: &TileRegion) -> Result<TileCut, EditError> {
    let clip = copy(map, layers, region)?;
    let erased = erase(map, layers, region)?;
    Ok(TileCut { clip, erased })
}

/// Empties `region` on the tile layers among `layers`, skipping the locked ones (a layer in a
/// locked group is locked), as Tiled's eraser does; the report lists the cells emptied per layer
/// and the layers skipped. Which layers the eraser works on is [`erase_targets`]'s.
pub fn erase(
    map: &mut Map,
    layers: &[LayerId],
    region: &TileRegion,
) -> Result<EraseReport, EditError> {
    let mut report = EraseReport::default();
    for id in tile_layers_among(map, layers)? {
        if map.effective_layer(id).is_some_and(|layer| layer.locked) {
            report.skipped_locked.push(id);
            continue;
        }
        let Some(cells) = map.layer_mut(id).and_then(Layer::as_tile_layer_mut) else {
            continue;
        };
        let mut changed: Option<IRect> = None;
        for placed in tiles_in(cells, region) {
            if cells.set_cell(placed.at, Cell::EMPTY)? {
                let cell = IRect::from_min_size(placed.at, IVec2::ONE);
                changed = Some(changed.map_or(cell, |rect| rect.union(cell)));
            }
        }
        if let Some(rect) = changed {
            cells.prune_empty_chunks();
            report.changed.push(LayerChange { layer: id, rect });
        }
    }
    Ok(report)
}

/// The layers the eraser empties: the tile layers among `selected` that are shown and
/// unlocked, or with `all` every shown, unlocked tile layer of the map, in the map's order. A
/// layer is shown and unlocked when its groups are too. Hidden layers are never erased, unlike
/// Tiled's Shift-erase: an edit to a layer the user cannot see is a surprise.
pub fn erase_targets(
    map: &Map,
    selected: &[LayerId],
    all: bool,
) -> Result<Vec<LayerId>, EditError> {
    let candidates = if all {
        let mut every = Vec::new();
        map.for_each_layer(|layer, _| {
            if layer.as_tile_layer().is_some() {
                every.push(layer.id);
            }
        });
        every
    } else {
        tile_layers_among(map, selected)?
    };
    Ok(candidates
        .into_iter()
        .filter(|&id| {
            map.effective_layer(id)
                .is_some_and(|layer| layer.visible && !layer.locked)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use bt_core::{Color, LayerKind, TilesetRef};
    use bt_test_data::fixtures::MapBuilder;
    use glam::UVec2;

    use super::*;
    use crate::testutil::{assert_valid, atlas, clip_grid, drawn, map_grid, tiles, trimmed};

    /// Two tilesets; ground (1), a group (2) holding walls (3), an object layer (4) and top (5).
    fn painted() -> Map {
        let walls = Layer {
            // Numbered when the map is built.
            id: LayerId(0),
            ..MapBuilder::new(UVec2::splat(16), UVec2::new(4, 3))
                .tile_layer("walls", ". 5 . .\n. 6@1 7 .\n. . . .")
                .build()
                .unwrap()
                .layers
                .remove(0)
        };
        let group = Layer::new(LayerId(0), "group", LayerKind::Group(vec![walls]));
        MapBuilder::new(UVec2::splat(16), UVec2::new(4, 3))
            .tileset(atlas("a"))
            .tileset(atlas("b"))
            .tile_layer("ground", "1 2 3 4\n1 2@1 3 4\n1 2 3h 4")
            .layer(group)
            .object_layer("things", Vec::new())
            .tile_layer("top", ". . 9 .\n. . . .\n. . . .")
            .build()
            .unwrap()
    }

    #[test]
    fn copy_takes_the_selected_tile_layers_in_map_order_inside_the_region() {
        let mut map = painted();
        map.layer_mut(LayerId(1)).unwrap().opacity = 0.5;
        map.layer_mut(LayerId(5)).unwrap().tint = Some(Color::rgb(1, 2, 3));
        let region = drawn(
            "
            ....
            .##.
            .#..
            ",
        );
        // Asked for top first, with the object layer and a group: the tile layers come in map
        // order and nothing else is copied.
        let ids = [LayerId(5), LayerId(4), LayerId(2), LayerId(1)];
        let clip = copy(&map, &ids, &region).unwrap();
        assert_eq!(
            (clip.origin, clip.size),
            (IVec2::new(1, 1), IVec2::new(2, 2))
        );
        let names: Vec<&str> = clip.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["ground", "top"]);
        assert_eq!(
            (clip.layers[0].opacity, clip.layers[1].tint),
            (0.5, Some(Color::rgb(1, 2, 3)))
        );
        // The second tileset is the only one met first: it is slot 0 in the clip.
        assert_eq!(clip_grid(&clip, 0), trimmed("2 3@1\n2@1 ."));
        assert_eq!(clip_grid(&clip, 1), trimmed(". .\n. ."));
        assert_eq!(clip.tilesets.len(), 2);
        assert_eq!(clip.tilesets[0].tileset.name, "b");
        let walls = copy(&map, &[LayerId(3)], &region).unwrap();
        assert_eq!(clip_grid(&walls, 0), trimmed("6 7@1\n. ."));
        assert_eq!(
            copy(&map, &[LayerId(9)], &region),
            Err(EditError::NoSuchLayer(LayerId(9)))
        );
        let nothing = copy(&map, &[LayerId(1)], &TileRegion::new()).unwrap();
        assert_eq!((nothing.size, nothing.layers.len()), (IVec2::ZERO, 1));
        assert!(nothing.is_empty() && nothing.tilesets.is_empty());
    }

    #[test]
    fn erasing_skips_locked_layers_and_layers_in_locked_groups() {
        let mut map = painted();
        map.layer_mut(LayerId(2)).unwrap().locked = true;
        map.layer_mut(LayerId(5)).unwrap().locked = true;
        let region = drawn("####\n####");
        let ids = [LayerId(1), LayerId(3), LayerId(5)];
        let report = erase(&mut map, &ids, &region).unwrap();
        assert_eq!(
            report,
            EraseReport {
                changed: vec![LayerChange {
                    layer: LayerId(1),
                    rect: IRect {
                        x: 0,
                        y: 0,
                        w: 4,
                        h: 2
                    }
                }],
                skipped_locked: vec![LayerId(3), LayerId(5)],
            }
        );
        assert_eq!(
            map_grid(&map, LayerId(1)),
            trimmed(". . . .\n. . . .\n1 2 3h 4")
        );
        assert_eq!(
            map_grid(&map, LayerId(3)),
            trimmed(". 5 . .\n. 6@1 7 .\n. . . .")
        );
        assert_valid(&map);
        let again = erase(&mut map, &ids, &region).unwrap();
        assert!(again.changed.is_empty(), "nothing left to erase");
    }

    #[test]
    fn cut_copies_every_selected_layer_and_erases_the_unlocked_ones() {
        let mut map = painted();
        map.layer_mut(LayerId(5)).unwrap().locked = true;
        let region = drawn("..#.\n..#.");
        let cut = cut(&mut map, &[LayerId(1), LayerId(5)], &region).unwrap();
        assert_eq!(clip_grid(&cut.clip, 0), trimmed("3\n3"));
        assert_eq!(clip_grid(&cut.clip, 1), trimmed("9\n."));
        assert_eq!(cut.erased.skipped_locked, [LayerId(5)]);
        assert_eq!(
            map_grid(&map, LayerId(1)),
            trimmed("1 2 . 4\n1 2@1 . 4\n1 2 3h 4")
        );
        assert_eq!(
            tiles(&map, LayerId(5)).cell(IVec2::new(2, 0)),
            crate::testutil::tile(9)
        );
        assert_valid(&map);
    }

    #[test]
    fn erasing_on_infinite_maps_drops_the_chunks_it_empties() {
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::splat(4))
            .infinite()
            .tileset(atlas("a"))
            .tile_layer_at("ground", IVec2::new(-20, -3), "1 2\n3 4")
            .tile_layer_at("ground", IVec2::new(40, 0), "5")
            .build()
            .unwrap();
        let region = TileRegion::from_rect(IRect {
            x: -100,
            y: -100,
            w: 120,
            h: 200,
        });
        let report = erase(&mut map, &[LayerId(1), LayerId(2)], &region).unwrap();
        assert_eq!(
            report.changed,
            [LayerChange {
                layer: LayerId(1),
                rect: IRect {
                    x: -20,
                    y: -3,
                    w: 2,
                    h: 2
                }
            }]
        );
        assert_eq!(tiles(&map, LayerId(1)).chunks().count(), 0);
        assert_eq!(
            tiles(&map, LayerId(2)).cells().count(),
            1,
            "outside the region"
        );
        assert_valid(&map);
    }

    #[test]
    fn the_eraser_takes_shown_unlocked_tile_layers_only() {
        let mut map = painted();
        // Hidden: top. Locked: nothing yet. The group is shown.
        map.layer_mut(LayerId(5)).unwrap().visible = false;
        let selected = [LayerId(1), LayerId(3), LayerId(4), LayerId(5)];
        assert_eq!(
            erase_targets(&map, &selected, false).unwrap(),
            [LayerId(1), LayerId(3)]
        );
        assert_eq!(
            erase_targets(&map, &[LayerId(1)], true).unwrap(),
            [LayerId(1), LayerId(3)],
            "every shown unlocked tile layer"
        );
        // A hidden group hides its walls; a locked ground is left alone.
        map.layer_mut(LayerId(2)).unwrap().visible = false;
        map.layer_mut(LayerId(1)).unwrap().locked = true;
        assert!(erase_targets(&map, &selected, true).unwrap().is_empty());
        assert_eq!(
            erase_targets(&map, &[LayerId(7)], false),
            Err(EditError::NoSuchLayer(LayerId(7)))
        );
    }

    #[test]
    fn a_copy_lists_only_the_tilesets_its_tiles_use() {
        let map = painted();
        let clip = copy(&map, &[LayerId(1)], &drawn("##")).unwrap();
        assert_eq!(
            clip.tilesets,
            vec![TilesetRef::embedded(map.tilesets[0].tileset.clone())]
        );
    }
}
