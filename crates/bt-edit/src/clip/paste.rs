//! Pasting a clip: which layers its layers go to, and painting them (Tiled's
//! `MapDocument::findTargetLayers` and `paintTileLayers`, and the clipping of
//! `TilePainter::paintableRegion`; `reference/tiled/src/tiled/mapdocument.cpp`,
//! `tilepainter.cpp`).

use std::collections::BTreeMap;

use bt_core::{Cell, Layer, LayerId, LayerKind, Map, PlacedCell, TileLayer, TilesetSlot};
use bt_utils::IRect;
use glam::{IVec2, UVec2};

use super::{ClipLayer, TileClip};
use crate::error::EditError;
use crate::ids::take_layer_id;
use crate::region::TileRegion;
use crate::report::{LayerChange, PasteReport};
use crate::tileset_ops::unify_tileset;

/// Where a clip layer is pasted.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PasteTarget {
    /// This tile layer.
    Layer(LayerId),
    /// A tile layer made for it, with its name, opacity and tint, beside the last layer the
    /// paste painted.
    NewLayer,
}

/// A tile layer the name rule may pick.
struct Candidate<'map> {
    id: LayerId,
    name: &'map str,
}

/// The layers a clip's layers are pasted into, by Tiled's three rules:
///
/// 1. a clip of one layer goes to the current layer when that is a tile layer, so a piece of one
///    layer can be painted into another;
/// 2. when as many tile layers are selected as the clip has layers, they are the targets, bottom
///    to top in the map's order;
/// 3. otherwise each clip layer goes to a tile layer of its name, each used once, looking on
///    from the layer found for the clip layer before (and then from the bottom), and one without
///    such a layer gets [`PasteTarget::NewLayer`].
///
/// A current or selected id the map does not have is an error.
pub fn find_target_layers(
    map: &Map,
    clip: &TileClip,
    current: Option<LayerId>,
    selected: &[LayerId],
) -> Result<Vec<PasteTarget>, EditError> {
    if let Some(&missing) = current
        .iter()
        .chain(selected)
        .find(|&&id| map.layer(id).is_none())
    {
        return Err(EditError::NoSuchLayer(missing));
    }
    let mut tile_layers = Vec::new();
    map.for_each_layer(|layer, _| {
        if layer.as_tile_layer().is_some() {
            tile_layers.push(Candidate {
                id: layer.id,
                name: &layer.name,
            });
        }
    });
    if let [_] = clip.layers.as_slice()
        && let Some(current) = current
        && tile_layers.iter().any(|candidate| candidate.id == current)
    {
        return Ok(vec![PasteTarget::Layer(current)]);
    }
    let selected_tile_layers: Vec<PasteTarget> = tile_layers
        .iter()
        .filter(|candidate| selected.contains(&candidate.id))
        .map(|candidate| PasteTarget::Layer(candidate.id))
        .collect();
    if selected_tile_layers.len() == clip.layers.len() {
        return Ok(selected_tile_layers);
    }
    let mut targets = Vec::with_capacity(clip.layers.len());
    let mut cursor = None;
    for layer in &clip.layers {
        let found = next_named(&tile_layers, &layer.name, &targets, &mut cursor);
        targets.push(found.map_or(PasteTarget::NewLayer, PasteTarget::Layer));
    }
    Ok(targets)
}

/// The next tile layer called `name` that is not taken yet, looking on from `cursor` (the layer
/// found last) to the top and then from the bottom up to it, which leaves `cursor` where Tiled's
/// layer iterator stops: on the layer found, on the starting layer when the search went all the
/// way round, or nowhere when it ran off the top.
fn next_named(
    tile_layers: &[Candidate<'_>],
    name: &str,
    taken: &[PasteTarget],
    cursor: &mut Option<usize>,
) -> Option<LayerId> {
    let usable = |candidate: &Candidate<'_>| {
        candidate.name == name && !taken.contains(&PasteTarget::Layer(candidate.id))
    };
    let start = *cursor;
    if let Some(start) = start {
        let after = tile_layers.iter().enumerate().skip(start + 1);
        if let Some((index, candidate)) = after.clone().find(|(_, candidate)| usable(candidate)) {
            *cursor = Some(index);
            return Some(candidate.id);
        }
    }
    *cursor = None;
    for (index, candidate) in tile_layers.iter().enumerate() {
        if Some(index) == start {
            *cursor = start;
            return None;
        }
        if usable(candidate) {
            *cursor = Some(index);
            return Some(candidate.id);
        }
    }
    None
}

/// What one clip layer paints.
struct Plan<'clip> {
    /// The clip layer's place in the clip.
    index: usize,
    layer: &'clip ClipLayer,
    target: PasteTarget,
    /// The cells to paint, where they go, with the clip's slots.
    cells: Vec<PlacedCell>,
}

/// Where a layer made by a paste goes.
#[derive(Copy, Clone)]
enum Beside {
    /// Just above this layer, among its siblings.
    Above(LayerId),
    /// Just below this layer, among its siblings.
    Below(LayerId),
}

/// Pastes `clip` with its top-left at `top_left`, its layers into `targets` (one each, as
/// [`find_target_layers`] gives them): Tiled's `paintTileLayers`, which every painting tool,
/// Paste in Place and a placed brush go through.
///
/// Only cells holding a tile are painted, so the clip's empty cells leave the map as it is.
/// Cells outside a finite layer are left out, and so are those outside `mask` (the tile
/// selection; an empty mask does not restrict). A locked target (or one in a locked group) is
/// left alone and listed in the report. The tilesets of the painted cells are found in the map
/// or added to it ([`crate::unify_tileset`]). A [`PasteTarget::NewLayer`] becomes a tile layer
/// with the clip layer's name, opacity and tint, put just above the last layer painted, else
/// just below the next existing target, else at the top of the map; none is made when nothing
/// would be painted on it.
///
/// A target that is missing or not a tile layer, a number of targets other than the clip's
/// layers, or a clip cell whose slot the clip lists no tileset for is an error, and the map is
/// left as it was.
pub fn paste(
    map: &mut Map,
    clip: &TileClip,
    top_left: IVec2,
    targets: &[PasteTarget],
    mask: &TileRegion,
) -> Result<PasteReport, EditError> {
    if targets.len() != clip.layers.len() {
        return Err(EditError::TargetCount {
            layers: clip.layers.len(),
            targets: targets.len(),
        });
    }
    for target in targets {
        if let PasteTarget::Layer(id) = *target {
            let layer = map.layer(id).ok_or(EditError::NoSuchLayer(id))?;
            if layer.as_tile_layer().is_none() {
                return Err(EditError::NotATileLayer(id));
            }
        }
    }
    for layer in &clip.layers {
        for placed in layer.cells.cells() {
            if let Some(slot) = placed.cell.slot()
                && clip.tilesets.get(slot.index()).is_none()
            {
                return Err(EditError::NoSuchTileset(slot));
            }
        }
    }

    let mut report = PasteReport::default();
    let size = UVec2::new(map.width, map.height);
    let blank = if map.infinite {
        TileLayer::infinite(size)
    } else {
        TileLayer::finite(size)
    };
    let mut plans = Vec::new();
    for (index, (layer, &target)) in clip.layers.iter().zip(targets).enumerate() {
        if layer.cells.cells().next().is_none() {
            continue;
        }
        let bounds = match target {
            PasteTarget::Layer(id) => {
                if map.effective_layer(id).is_some_and(|layer| layer.locked) {
                    report.skipped_locked.push(id);
                    continue;
                }
                map.layer(id)
                    .and_then(Layer::as_tile_layer)
                    .ok_or(EditError::NotATileLayer(id))?
            }
            PasteTarget::NewLayer => &blank,
        };
        let cells: Vec<PlacedCell> = layer
            .cells
            .cells()
            .map(|placed| PlacedCell {
                at: placed.at + top_left,
                cell: placed.cell,
            })
            .filter(|placed| {
                bounds.contains(placed.at) && (mask.is_empty() || mask.contains(placed.at))
            })
            .collect();
        if !cells.is_empty() {
            plans.push(Plan {
                index,
                layer,
                target,
                cells,
            });
        }
    }

    let mut slots: BTreeMap<TilesetSlot, TilesetSlot> = BTreeMap::new();
    for placed in plans.iter().flat_map(|plan| &plan.cells) {
        let Some(slot) = placed.cell.slot() else {
            continue;
        };
        if slots.contains_key(&slot) {
            continue;
        }
        let tileset = clip
            .tilesets
            .get(slot.index())
            .ok_or(EditError::NoSuchTileset(slot))?;
        let unified = unify_tileset(map, tileset)?;
        if unified.added {
            report.added_tilesets.push(unified.slot);
        }
        slots.insert(slot, unified.slot);
    }

    let mut last_painted = None;
    for plan in plans {
        let id = match plan.target {
            PasteTarget::Layer(id) => id,
            PasteTarget::NewLayer => {
                let id = take_layer_id(map)?;
                let made = Layer {
                    opacity: plan.layer.opacity,
                    tint: plan.layer.tint,
                    ..Layer::new(id, plan.layer.name.clone(), LayerKind::Tile(blank.clone()))
                };
                let beside = last_painted.map(Beside::Above).or_else(|| {
                    targets
                        .iter()
                        .skip(plan.index + 1)
                        .find_map(|target| match *target {
                            PasteTarget::Layer(next) => Some(Beside::Below(next)),
                            PasteTarget::NewLayer => None,
                        })
                });
                insert_layer(map, made, beside);
                report.created_layers.push(id);
                id
            }
        };
        let cells = map
            .layer_mut(id)
            .and_then(Layer::as_tile_layer_mut)
            .ok_or(EditError::NotATileLayer(id))?;
        let mut changed: Option<IRect> = None;
        for placed in plan.cells {
            let Some(cell) = in_map(placed.cell, &slots) else {
                continue;
            };
            if cells.set_cell(placed.at, cell)? {
                let rect = IRect::from_min_size(placed.at, IVec2::ONE);
                changed = Some(changed.map_or(rect, |so_far| so_far.union(rect)));
            }
        }
        if let Some(rect) = changed {
            record_change(&mut report, id, rect);
        }
        last_painted = Some(id);
    }
    Ok(report)
}

/// A clip cell with the map's slot for its tileset.
fn in_map(cell: Cell, slots: &BTreeMap<TilesetSlot, TilesetSlot>) -> Option<Cell> {
    let slot = slots.get(&cell.slot()?)?;
    Some(Cell::new(*slot, cell.tile()).with_flip(cell.flip()))
}

/// Adds `rect` to what the report says changed on `layer`.
fn record_change(report: &mut PasteReport, layer: LayerId, rect: IRect) {
    match report
        .changed
        .iter_mut()
        .find(|change| change.layer == layer)
    {
        Some(change) => change.rect = change.rect.union(rect),
        None => report.changed.push(LayerChange { layer, rect }),
    }
}

/// Puts a made layer beside another, or at the top of the map.
fn insert_layer(map: &mut Map, layer: Layer, beside: Option<Beside>) {
    let unplaced = match beside {
        Some(beside) => insert_beside(&mut map.layers, layer, beside),
        None => Some(layer),
    };
    // The layer beside it is a target the paste checked, so it is there; were it not, the top
    // of the map is where Tiled puts a layer with nothing to go beside.
    if let Some(layer) = unplaced {
        map.layers.push(layer);
    }
}

/// Inserts `layer` beside the layer `beside` names among `siblings` or their children; gives
/// the layer back when that layer is not there.
fn insert_beside(siblings: &mut Vec<Layer>, layer: Layer, beside: Beside) -> Option<Layer> {
    let (Beside::Above(anchor) | Beside::Below(anchor)) = beside;
    if let Some(index) = siblings.iter().position(|sibling| sibling.id == anchor) {
        let at = match beside {
            Beside::Above(_) => index + 1,
            Beside::Below(_) => index,
        };
        siblings.insert(at, layer);
        return None;
    }
    let mut layer = layer;
    for sibling in siblings.iter_mut() {
        if let Some(children) = sibling.children_mut() {
            match insert_beside(children, layer, beside) {
                None => return None,
                Some(back) => layer = back,
            }
        }
    }
    Some(layer)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{Color, TilesetRef};
    use bt_test_data::Fuzz;
    use bt_test_data::fixtures::MapBuilder;
    use bt_utils::rng::Rng;

    use super::*;
    use crate::clip::{copy, erase};
    use crate::testutil::{assert_valid, atlas, drawn, map_grid, tile, tiles, trimmed};

    fn names(map: &Map) -> Vec<String> {
        let mut names = Vec::new();
        map.for_each_layer(|layer, depth| {
            names.push(format!("{}{}", " ".repeat(depth), layer.name))
        });
        names
    }

    fn clip_named(names: &[&str]) -> TileClip {
        TileClip {
            layers: names.iter().map(|&name| ClipLayer::named(name)).collect(),
            ..TileClip::default()
        }
    }

    /// Tile layers a (1), b (2), a (3), objects (4) inside a group (5) with c (6).
    fn layered() -> Map {
        let inner = Layer::new(
            LayerId(6),
            "c",
            LayerKind::Tile(TileLayer::finite(UVec2::splat(4))),
        );
        let group = Layer::new(LayerId(5), "group", LayerKind::Group(vec![inner]));
        MapBuilder::new(UVec2::splat(16), UVec2::splat(4))
            .tileset(atlas("a"))
            .tile_layer("a", ". . . .")
            .tile_layer("b", ". . . .")
            .tile_layer("a", ". . . .")
            .object_layer("objects", Vec::new())
            .layer(group)
            .build()
            .unwrap()
    }

    #[test]
    fn one_layer_goes_to_the_current_tile_layer() {
        let map = layered();
        let clip = clip_named(&["anything"]);
        let selected = [LayerId(1), LayerId(2)];
        assert_eq!(
            find_target_layers(&map, &clip, Some(LayerId(3)), &selected).unwrap(),
            [PasteTarget::Layer(LayerId(3))]
        );
        // With an object layer current the selection decides; one selected tile layer is it.
        assert_eq!(
            find_target_layers(&map, &clip, Some(LayerId(4)), &[LayerId(6), LayerId(4)]).unwrap(),
            [PasteTarget::Layer(LayerId(6))]
        );
        assert_eq!(
            find_target_layers(&map, &clip, Some(LayerId(9)), &[]),
            Err(EditError::NoSuchLayer(LayerId(9)))
        );
    }

    #[test]
    fn as_many_selected_tile_layers_are_the_targets_in_map_order() {
        let map = layered();
        let clip = clip_named(&["x", "y"]);
        let selected = [LayerId(6), LayerId(4), LayerId(2)];
        assert_eq!(
            find_target_layers(&map, &clip, Some(LayerId(1)), &selected).unwrap(),
            [
                PasteTarget::Layer(LayerId(2)),
                PasteTarget::Layer(LayerId(6))
            ]
        );
    }

    #[test]
    fn otherwise_layers_go_by_name_looking_on_from_the_last_found() {
        let map = layered();
        let clip = clip_named(&["a", "a", "b", "z"]);
        assert_eq!(
            find_target_layers(&map, &clip, None, &[LayerId(1)]).unwrap(),
            [
                PasteTarget::Layer(LayerId(1)),
                PasteTarget::Layer(LayerId(3)),
                // From the second a nothing is above: the search starts again at the bottom.
                PasteTarget::Layer(LayerId(2)),
                PasteTarget::NewLayer,
            ]
        );
        assert_eq!(
            find_target_layers(&map, &clip_named(&["c", "a", "a", "a"]), None, &[]).unwrap(),
            [
                PasteTarget::Layer(LayerId(6)),
                PasteTarget::Layer(LayerId(1)),
                PasteTarget::Layer(LayerId(3)),
                PasteTarget::NewLayer,
            ]
        );
        assert!(
            find_target_layers(&map, &clip_named(&[]), None, &[])
                .unwrap()
                .is_empty()
        );
    }

    /// A map with ground (1) and a group (2) holding walls (3), on one tileset.
    fn grounds() -> Map {
        let walls = Layer::new(
            LayerId(0),
            "walls",
            LayerKind::Tile(TileLayer::finite(UVec2::new(4, 3))),
        );
        MapBuilder::new(UVec2::splat(16), UVec2::new(4, 3))
            .tileset(atlas("a"))
            .tile_layer("ground", "1 1 1 1\n1 1 1 1\n1 1 1 1")
            .layer(Layer::new(
                LayerId(0),
                "group",
                LayerKind::Group(vec![walls]),
            ))
            .build()
            .unwrap()
    }

    fn one_layer_clip(map: &Map, grid: &str) -> TileClip {
        let source = MapBuilder::new(UVec2::splat(16), UVec2::splat(8))
            .tileset(Arc::clone(&map.tilesets[0].tileset))
            .tile_layer("ground", grid)
            .build()
            .unwrap();
        copy(
            &source,
            &[LayerId(1)],
            &TileRegion::from_rect(IRect {
                x: 0,
                y: 0,
                w: 8,
                h: 8,
            }),
        )
        .unwrap()
        .trimmed()
    }

    #[test]
    fn empty_cells_never_erase_and_finite_layers_clip() {
        let mut map = grounds();
        let clip = one_layer_clip(&map, "2 .\n. 3");
        let target = [PasteTarget::Layer(LayerId(1))];
        // The empty cell lands on (3, 2), the 3 outside the map.
        let report = paste(
            &mut map,
            &clip,
            IVec2::new(3, 1),
            &target,
            &TileRegion::new(),
        )
        .unwrap();
        assert_eq!(
            map_grid(&map, LayerId(1)),
            trimmed("1 1 1 1\n1 1 1 2\n1 1 1 1")
        );
        assert_eq!(
            report,
            PasteReport {
                changed: vec![LayerChange {
                    layer: LayerId(1),
                    rect: IRect {
                        x: 3,
                        y: 1,
                        w: 1,
                        h: 1
                    }
                }],
                ..PasteReport::default()
            }
        );
        assert_valid(&map);
        let unchanged = paste(
            &mut map,
            &clip,
            IVec2::new(3, 1),
            &target,
            &TileRegion::new(),
        );
        assert_eq!(
            unchanged,
            Ok(PasteReport::default()),
            "the same tiles again change nothing"
        );
    }

    #[test]
    fn the_mask_keeps_painting_inside_the_selection() {
        let mut map = grounds();
        let clip = one_layer_clip(&map, "2 2\n2 2");
        let target = [PasteTarget::Layer(LayerId(1))];
        paste(&mut map, &clip, IVec2::ZERO, &target, &drawn(".#\n#.")).unwrap();
        assert_eq!(
            map_grid(&map, LayerId(1)),
            trimmed("1 2 1 1\n2 1 1 1\n1 1 1 1")
        );
    }

    #[test]
    fn locked_targets_and_targets_in_locked_groups_are_left_alone() {
        let mut map = grounds();
        map.layer_mut(LayerId(2)).unwrap().locked = true;
        let clip = one_layer_clip(&map, "2");
        for id in [LayerId(3), LayerId(1)] {
            if id == LayerId(1) {
                map.layer_mut(LayerId(1)).unwrap().locked = true;
            }
            let before = map.clone();
            let report = paste(
                &mut map,
                &clip,
                IVec2::ZERO,
                &[PasteTarget::Layer(id)],
                &TileRegion::new(),
            )
            .unwrap();
            assert_eq!(report.skipped_locked, [id]);
            assert!(report.changed.is_empty());
            assert_eq!(map, before);
        }
    }

    /// An atlas of 16 tiles in `columns` columns: atlases of other shapes have other images, so
    /// none stands in for another.
    fn shaped_atlas(name: &str, columns: u32) -> Arc<bt_core::Tileset> {
        bt_test_data::fixtures::Atlas::builder()
            .name(name)
            .grid(columns, 16 / columns)
            .build_embedded()
            .unwrap()
            .tileset
    }

    #[test]
    fn pasting_into_another_map_adds_the_tilesets_once() {
        let source = MapBuilder::new(UVec2::splat(16), UVec2::new(3, 2))
            .tileset(shaped_atlas("a", 4))
            .tileset(shaped_atlas("b", 8))
            .tile_layer("ground", "1 2@1 3@1h")
            .tile_layer("top", "4@1 . 5")
            .build()
            .unwrap();
        let clip = copy(&source, &[LayerId(1), LayerId(2)], &drawn("###")).unwrap();
        // The target has a tileset similar to a (the same image and grid) and lacks b.
        let a_again = Arc::new(source.tilesets[0].tileset.as_ref().clone());
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::new(3, 2))
            .tileset(shaped_atlas("c", 2))
            .tileset(a_again)
            .tile_layer("ground", ". . .\n. . .")
            .build()
            .unwrap();
        let targets = find_target_layers(&map, &clip, Some(LayerId(1)), &[LayerId(1)]).unwrap();
        assert_eq!(
            targets,
            [PasteTarget::Layer(LayerId(1)), PasteTarget::NewLayer]
        );
        let report = paste(
            &mut map,
            &clip,
            IVec2::new(0, 1),
            &targets,
            &TileRegion::new(),
        )
        .unwrap();
        assert_eq!(report.added_tilesets, [TilesetSlot::new(2).unwrap()]);
        assert_eq!(report.created_layers, [LayerId(2)]);
        assert_eq!(map.tilesets.len(), 3);
        assert_eq!(map.tilesets[2].tileset.name, "b");
        assert_eq!(map_grid(&map, LayerId(1)), trimmed(". . .\n1@1 2@2 3@2h"));
        assert_eq!(map_grid(&map, LayerId(2)), trimmed(". . .\n4@2 . 5@1"));
        assert_eq!(names(&map), ["ground", "top"]);
        assert_valid(&map);
    }

    #[test]
    fn made_layers_take_the_clip_layers_attributes_and_go_beside_the_targets() {
        let mut map = grounds();
        let mut clip = one_layer_clip(&map, "2");
        clip.layers[0].opacity = 0.25;
        clip.layers[0].tint = Some(Color::rgb(9, 8, 7));
        let mut top = clip.layers[0].clone();
        top.name = "top".into();
        let mut below = clip.layers[0].clone();
        below.name = "below".into();
        clip.layers = vec![below, clip.layers[0].clone(), top];
        // Before anything is painted a made layer goes below the next target; after, above the
        // last one painted, inside its group.
        let targets = [
            PasteTarget::NewLayer,
            PasteTarget::Layer(LayerId(3)),
            PasteTarget::NewLayer,
        ];
        let report = paste(&mut map, &clip, IVec2::ZERO, &targets, &TileRegion::new()).unwrap();
        assert_eq!(report.created_layers, [LayerId(4), LayerId(5)]);
        assert_eq!(names(&map), ["ground", "group", " below", " walls", " top"]);
        let made = map.layer(LayerId(5)).unwrap();
        assert_eq!((made.opacity, made.tint), (0.25, Some(Color::rgb(9, 8, 7))));
        assert_eq!(tiles(&map, LayerId(5)).cell(IVec2::ZERO), tile(2));
        assert_eq!(map.next_layer_id, 6);
        assert_valid(&map);
        // With no target at all it goes on top of the map.
        let only = TileClip {
            layers: vec![clip.layers[2].clone()],
            ..clip.clone()
        };
        let report = paste(
            &mut map,
            &only,
            IVec2::ZERO,
            &[PasteTarget::NewLayer],
            &TileRegion::new(),
        )
        .unwrap();
        assert_eq!(report.created_layers, [LayerId(6)]);
        assert_eq!(map.layers.last().unwrap().id, LayerId(6));
    }

    #[test]
    fn no_layer_is_made_when_nothing_would_be_painted_on_it() {
        let mut map = grounds();
        let clip = one_layer_clip(&map, "2");
        let before = map.clone();
        let outside = paste(
            &mut map,
            &clip,
            IVec2::new(9, 9),
            &[PasteTarget::NewLayer],
            &TileRegion::new(),
        );
        assert_eq!(outside, Ok(PasteReport::default()));
        let masked = paste(
            &mut map,
            &clip,
            IVec2::ZERO,
            &[PasteTarget::NewLayer],
            &drawn(".#"),
        );
        assert_eq!(masked, Ok(PasteReport::default()));
        assert_eq!(map, before);
    }

    #[test]
    fn impossible_pastes_are_errors_that_change_nothing() {
        let mut map = grounds();
        let clip = one_layer_clip(&map, "2");
        let before = map.clone();
        let mask = TileRegion::new();
        assert_eq!(
            paste(&mut map, &clip, IVec2::ZERO, &[], &mask),
            Err(EditError::TargetCount {
                layers: 1,
                targets: 0
            })
        );
        assert_eq!(
            paste(
                &mut map,
                &clip,
                IVec2::ZERO,
                &[PasteTarget::Layer(LayerId(2))],
                &mask
            ),
            Err(EditError::NotATileLayer(LayerId(2)))
        );
        assert_eq!(
            paste(
                &mut map,
                &clip,
                IVec2::ZERO,
                &[PasteTarget::Layer(LayerId(8))],
                &mask
            ),
            Err(EditError::NoSuchLayer(LayerId(8)))
        );
        let mut broken = clip.clone();
        broken.tilesets.clear();
        assert_eq!(
            paste(
                &mut map,
                &broken,
                IVec2::ZERO,
                &[PasteTarget::NewLayer],
                &mask
            ),
            Err(EditError::NoSuchTileset(TilesetSlot::new(0).unwrap()))
        );
        assert_eq!(map, before);
    }

    #[test]
    fn paste_in_place_puts_a_copy_back_where_it_was_on_an_infinite_map() {
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::splat(4))
            .infinite()
            .tileset(atlas("a"))
            .tile_layer_at("ground", IVec2::new(-30, -5), "1 2\n3 .")
            .tile_layer_at("top", IVec2::new(-30, -5), ". 4\n. 5")
            .build()
            .unwrap();
        let original = map.clone();
        let region = TileRegion::from_rect(IRect {
            x: -31,
            y: -6,
            w: 4,
            h: 4,
        });
        let layers = [LayerId(1), LayerId(2)];
        let cut = crate::clip::cut(&mut map, &layers, &region).unwrap();
        assert_eq!(cut.clip.origin, IVec2::new(-31, -6));
        assert!(tiles(&map, LayerId(1)).cells().next().is_none());
        let targets = find_target_layers(&map, &cut.clip, Some(LayerId(1)), &layers).unwrap();
        paste(
            &mut map,
            &cut.clip,
            cut.clip.origin,
            &targets,
            &TileRegion::new(),
        )
        .unwrap();
        assert_eq!(map, original);
    }

    /// A finite map of random tiles on two layers, with an empty band at the bottom.
    fn random_map(rng: &mut Rng) -> Map {
        let mut rows = |rows: usize| {
            (0..8)
                .map(|y| {
                    (0..8)
                        .map(|_| match rng.below(3) {
                            Some(0) => ".".to_owned(),
                            _ if y >= rows => ".".to_owned(),
                            _ => format!("{}", rng.below(16).unwrap()),
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let (ground, top) = (rows(4), rows(4));
        MapBuilder::new(UVec2::splat(16), UVec2::splat(8))
            .tileset(atlas("a"))
            .tile_layer("ground", &ground)
            .tile_layer("top", &top)
            .build()
            .unwrap()
    }

    #[test]
    fn paste_then_erase_of_the_same_cells_leaves_the_map_unchanged() {
        let fuzz = Fuzz::from_env(0xC11_9A57E, 200);
        for case in 0..fuzz.cases {
            let mut rng = Rng::new(fuzz.case_seed(case));
            let mut map = random_map(&mut rng);
            let original = map.clone();
            let corner = IVec2::new(rng.below(5).unwrap() as i32, rng.below(4).unwrap() as i32);
            let size = IVec2::new(
                1 + rng.below(4).unwrap() as i32,
                1 + rng.below(4).unwrap() as i32,
            );
            let region: TileRegion = IRect::from_min_size(corner, size)
                .points()
                .filter(|_| rng.below(4) != Some(0))
                .collect();
            let layers = [LayerId(1), LayerId(2)];
            let clip = copy(&map, &layers, &region).unwrap();
            // Into the empty band, partly off the map's right edge.
            let top_left = IVec2::new(
                rng.below(7).unwrap() as i32,
                4 + rng.below(2).unwrap() as i32,
            );
            let targets = find_target_layers(&map, &clip, None, &layers).unwrap();
            let pasted = paste(&mut map, &clip, top_left, &targets, &TileRegion::new()).unwrap();
            assert_valid(&map);
            assert!(pasted.created_layers.is_empty() && pasted.added_tilesets.is_empty());
            let painted = clip.content_region().translated(top_left);
            erase(&mut map, &layers, &painted).unwrap();
            assert_valid(&map);
            assert_eq!(map, original, "case {case} ({})", fuzz.reproduce());
        }
    }

    #[test]
    fn a_clip_pastes_its_tileset_by_reference() {
        let map = grounds();
        let clip = one_layer_clip(&map, "2");
        assert_eq!(
            clip.tilesets,
            vec![TilesetRef::embedded(Arc::clone(&map.tilesets[0].tileset))]
        );
    }
}
