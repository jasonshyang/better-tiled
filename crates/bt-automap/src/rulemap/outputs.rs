//! A rule's output sets, as Tiled compiles them (`AutoMapper::compileOutputSet`): for each output
//! index, the tile layers with something inside the rule's output region and the objects whose
//! cells meet it. What a tile layer writes is its ordinary tiles, with their flips, and the
//! empty cell where it has the `Empty` tile; its other special tiles write nothing but still
//! count as the rule's footprint for `NoOverlappingOutput`.

use bt_core::{Cell, Map, MapObject};
use glam::IVec2;

use crate::cell_set::CellSet;
use crate::object_cells::{object_tile_rect, objects_in_region};
use crate::rulemap::layers::{LayerSetup, OutputSetLayers};
use crate::rulemap::match_type::{MatchType, RuleCell};
use crate::rulemap::tilesets::TableTile;

/// A cell a tile output writes, relative to the rule's input box.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct TileWrite {
    /// Where.
    pub(crate) offset: IVec2,
    /// The tile written; `None` erases.
    pub(crate) tile: Option<TableTile>,
}

/// What one output tile layer writes for a rule.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TileOutput {
    /// The target layer, as an index into the rule map's tile output names.
    pub(crate) target: usize,
    /// The output layer, as an index into the rule map's output layers.
    pub(crate) layer: usize,
    /// The cells written, row by row.
    pub(crate) writes: Vec<TileWrite>,
    /// Every cell the layer has inside the rule's output region, relative to the input box.
    pub(crate) footprint: CellSet,
}

/// An object an object output places, with its tile in the rule set's numbering.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OutputObject {
    /// The object as the rule map has it, without its tile.
    pub(crate) object: MapObject,
    /// The tile of a tile object.
    pub(crate) tile: Option<TableTile>,
}

/// What one output object layer places for a rule.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ObjectOutput {
    /// The target layer, as an index into the rule map's object output names.
    pub(crate) target: usize,
    /// The output layer, as an index into the rule map's output layers.
    pub(crate) layer: usize,
    /// The objects placed.
    pub(crate) objects: Vec<OutputObject>,
    /// The cells the objects cover, relative to the input box.
    pub(crate) footprint: CellSet,
}

/// One output index of a rule.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OutputSet {
    /// The index.
    pub(crate) index: String,
    /// Its tile layers with something in the rule's region.
    pub(crate) tiles: Vec<TileOutput>,
    /// Its object layers with objects in the rule's region.
    pub(crate) objects: Vec<ObjectOutput>,
}

impl OutputSet {
    /// Whether it writes and places nothing.
    pub(crate) fn is_empty(&self) -> bool {
        self.tiles.is_empty() && self.objects.is_empty()
    }
}

/// What compiling a rule's output sets needs.
pub(crate) struct OutputContext<'a> {
    /// The rule map, whose geometry places objects.
    pub(crate) rule_map: &'a Map,
    /// The rule's output cells in the rule map.
    pub(crate) region: &'a CellSet,
    /// The top-left of the rule's input box, which offsets are relative to.
    pub(crate) origin: IVec2,
}

/// Compiles one output index of a rule; `meaning` says what a rule map cell is (and numbers a
/// tile object's tile).
pub(crate) fn compile_output_set(
    set: &OutputSetLayers,
    setup: &LayerSetup<'_>,
    context: &OutputContext<'_>,
    meaning: &mut dyn FnMut(Cell) -> RuleCell,
) -> OutputSet {
    let mut compiled = OutputSet {
        index: set.index.clone(),
        tiles: Vec::new(),
        objects: Vec::new(),
    };
    for &layer_index in &set.layers {
        let Some(output) = setup.output_layers.get(layer_index) else {
            continue;
        };
        if let Some(tiles) = output.layer.as_tile_layer() {
            let mut writes = Vec::new();
            let mut footprint = Vec::new();
            for p in context.region.cells() {
                let cell = tiles.cell(p);
                if cell.is_empty() {
                    continue;
                }
                let offset = p - context.origin;
                footprint.push(offset);
                match meaning(cell) {
                    RuleCell::Typed(MatchType::Tile, tile) => writes.push(TileWrite {
                        offset,
                        tile: Some(tile),
                    }),
                    RuleCell::Typed(MatchType::Empty, _) => {
                        writes.push(TileWrite { offset, tile: None });
                    }
                    _ => {}
                }
            }
            if footprint.is_empty() {
                continue;
            }
            let target = setup
                .tile_output_names
                .iter()
                .position(|n| *n == output.name)
                .unwrap_or_default();
            compiled.tiles.push(TileOutput {
                target,
                layer: layer_index,
                writes,
                footprint: CellSet::from_cells(footprint),
            });
        } else if let Some(group) = output.layer.as_object_layer() {
            let placed = objects_in_region(context.rule_map, &group.objects, context.region);
            if placed.is_empty() {
                continue;
            }
            let mut footprint = CellSet::new();
            let mut objects = Vec::new();
            for object in placed {
                // Objects in the region have cells; the compiler reports those without.
                if let Some(cells) =
                    object_tile_rect(context.rule_map, object).and_then(|rect| rect.cells())
                {
                    footprint.insert_rect(cells.translated(-context.origin));
                }
                let tile = object.tile.and_then(|cell| match meaning(cell) {
                    RuleCell::Typed(_, tile) | RuleCell::Unknown(tile) => Some(tile),
                    RuleCell::Empty => None,
                });
                objects.push(OutputObject {
                    object: MapObject {
                        tile: None,
                        ..object.clone()
                    },
                    tile,
                });
            }
            let target = setup
                .object_output_names
                .iter()
                .position(|n| *n == output.name)
                .unwrap_or_default();
            compiled.objects.push(ObjectOutput {
                target,
                layer: layer_index,
                objects,
                footprint,
            });
        }
    }
    compiled
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{
        Layer, LayerId, LayerKind, ObjectId, ObjectLayer, TileId, TileLayer, TilesetSlot,
    };
    use bt_utils::IRect;
    use glam::{DVec2, UVec2};

    use super::*;
    use crate::rulemap::layers::read_layers;

    fn meaning(cell: Cell) -> RuleCell {
        if cell.is_empty() {
            return RuleCell::Empty;
        }
        let tile = TableTile {
            tileset: 0,
            tile: cell.tile(),
            flip: cell.flip(),
        };
        match cell.tile().0 {
            100 => RuleCell::Typed(MatchType::Empty, tile),
            104 => RuleCell::Typed(MatchType::Ignore, tile),
            _ => RuleCell::Typed(MatchType::Tile, tile),
        }
    }

    fn tiles(name: &str, cells: &[(i32, i32, u32)]) -> Layer {
        let mut layer = TileLayer::finite(UVec2::new(8, 8));
        for &(x, y, id) in cells {
            layer
                .set_cell(
                    IVec2::new(x, y),
                    Cell::new(TilesetSlot::new(0).unwrap(), TileId(id)),
                )
                .unwrap();
        }
        Layer::new(LayerId(0), name, LayerKind::Tile(layer))
    }

    #[test]
    fn tile_layers_write_tiles_and_erase_with_empty() {
        let map = Map {
            width: 8,
            height: 8,
            tile_width: 16,
            tile_height: 16,
            layers: vec![
                tiles("input_a", &[(1, 1, 1)]),
                tiles(
                    "output_a",
                    &[(1, 1, 2), (2, 1, 100), (1, 2, 104), (6, 6, 9)],
                ),
                tiles("outputX_a", &[(6, 6, 9)]),
            ],
            ..Map::default()
        };
        let mut problems = Vec::new();
        let setup = read_layers(&map, &mut problems);
        let region = CellSet::from_rect(IRect {
            x: 1,
            y: 1,
            w: 2,
            h: 2,
        });
        let context = OutputContext {
            rule_map: &map,
            region: &region,
            origin: IVec2::new(1, 1),
        };
        let set = setup.output_sets.first().unwrap();
        let compiled = compile_output_set(set, &setup, &context, &mut meaning);
        assert_eq!(compiled.tiles.len(), 1);
        let output = &compiled.tiles[0];
        let written: Vec<(IVec2, Option<u32>)> = output
            .writes
            .iter()
            .map(|w| (w.offset, w.tile.map(|t| t.tile.0)))
            .collect();
        assert_eq!(
            written,
            [(IVec2::new(0, 0), Some(2)), (IVec2::new(1, 0), None)]
        );
        assert_eq!(output.footprint.len(), 3, "Ignore is part of the footprint");
        // outputX_a has nothing in this rule's region.
        let b = setup.output_sets.get(1).unwrap();
        assert!(compile_output_set(b, &setup, &context, &mut meaning).is_empty());
    }

    #[test]
    fn objects_meeting_the_region_are_placed() {
        let object = |id: u32, x: f64| MapObject {
            id: ObjectId(id),
            pos: DVec2::new(x, 16.0),
            size: DVec2::new(16.0, 16.0),
            ..MapObject::default()
        };
        let objects = Layer::new(
            LayerId(0),
            "output_things",
            LayerKind::Object(ObjectLayer {
                objects: Arc::new(vec![object(1, 16.0), object(2, 96.0)]),
                ..ObjectLayer::default()
            }),
        );
        let map = Map {
            width: 8,
            height: 8,
            tile_width: 16,
            tile_height: 16,
            layers: vec![tiles("input_a", &[(1, 1, 1)]), objects],
            ..Map::default()
        };
        let mut problems = Vec::new();
        let setup = read_layers(&map, &mut problems);
        let region = CellSet::from_rect(IRect {
            x: 1,
            y: 1,
            w: 1,
            h: 1,
        });
        let context = OutputContext {
            rule_map: &map,
            region: &region,
            origin: IVec2::new(1, 1),
        };
        let set = setup.output_sets.first().unwrap();
        let compiled = compile_output_set(set, &setup, &context, &mut meaning);
        assert_eq!(compiled.objects.len(), 1);
        let placed: Vec<ObjectId> = compiled.objects[0]
            .objects
            .iter()
            .map(|o| o.object.id)
            .collect();
        assert_eq!(placed, [ObjectId(1)]);
        assert_eq!(compiled.objects[0].footprint.len(), 1);
        assert!(compiled.objects[0].footprint.contains(IVec2::ZERO));
    }
}
