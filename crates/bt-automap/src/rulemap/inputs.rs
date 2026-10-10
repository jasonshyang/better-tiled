//! A rule's input sets, compiled once as Tiled compiles them on every run
//! (`AutoMapper::compileInputSet`, `optimizeAnyNoneOf`): for each input set and each target layer,
//! the positions with the tiles that must be there ("any of") or must not ("none of"),
//! deduplicated, "any" subsuming "none", a contradiction making the set impossible and a set with
//! no conditions and no `Ignore` never matching.

use bt_core::{Cell, Flip};
use glam::IVec2;

use crate::rulemap::layers::{InputLayer, InputSetLayers};
use crate::rulemap::match_type::{MatchType, RuleCell};
use crate::rulemap::tilesets::TableTile;

/// What a condition asks of a cell: a tile, compared on the flips in `mask`, or the empty cell.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct MatchCell {
    /// The tile; `None` for the empty cell.
    pub tile: Option<TableTile>,
    /// The flips that must agree.
    pub mask: Flip,
}

impl MatchCell {
    /// The empty cell, as Tiled's `MatchCell()`.
    pub const EMPTY: MatchCell = MatchCell {
        tile: None,
        mask: Flip::all(),
    };

    /// Whether it asks for the empty cell.
    pub fn is_empty(self) -> bool {
        self.tile.is_none()
    }

    /// Tiled's order of match cells: by tileset (none first), tile, flips, then mask.
    fn key(self) -> (usize, u32, u8, u8) {
        match self.tile {
            None => (0, 0, 0, self.mask.bits()),
            Some(t) => (t.tileset + 1, t.tile.0, t.flip.bits(), self.mask.bits()),
        }
    }
}

/// Why an input set can never match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Never {
    /// At this position on this target the set asks for tiles it also forbids, and nothing is
    /// left.
    Contradiction {
        /// The target layer's name.
        layer: String,
        /// The position, relative to the rule's input box.
        at: IVec2,
    },
    /// The set has no condition anywhere and no `Ignore` tile (Tiled 1.12 then never matches).
    NoConditions,
}

/// One position of a target layer with its conditions: `any` cells from `start` in the set's
/// cells, then `none` cells.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Position {
    /// Where, relative to the rule's input box.
    pub(crate) offset: IVec2,
    /// The first of its cells in [`InputSet::cells`].
    pub(crate) start: usize,
    /// How many "any of" cells: the cell must be one of them (none: no such condition).
    pub(crate) any: usize,
    /// How many "none of" cells, after the "any of" ones: the cell must be none of them.
    pub(crate) none: usize,
}

/// The conditions of an input set on one target layer.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LayerConditions {
    /// The target layer, as an index into the rule map's input names.
    pub(crate) target: usize,
    /// The positions with conditions.
    pub(crate) positions: Vec<Position>,
}

/// A compiled input set: a rule matches where any of its input sets holds.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct InputSet {
    /// The index of its layers' names.
    pub(crate) index: String,
    /// Its conditions per target layer, in the order of the targets' names.
    pub(crate) layers: Vec<LayerConditions>,
    /// The cells the positions refer to.
    pub(crate) cells: Vec<MatchCell>,
    /// Whether an `Ignore` tile makes it match even without conditions.
    pub(crate) has_ignore: bool,
    /// Why it can never match, if it cannot.
    pub(crate) never: Option<Never>,
}

impl InputSet {
    /// The "any of" cells of a position.
    pub(crate) fn any_of(&self, position: &Position) -> &[MatchCell] {
        self.cells
            .get(position.start..position.start + position.any)
            .unwrap_or_default()
    }

    /// The "none of" cells of a position.
    pub(crate) fn none_of(&self, position: &Position) -> &[MatchCell] {
        let start = position.start + position.any;
        self.cells
            .get(start..start + position.none)
            .unwrap_or_default()
    }
}

/// What compiling a rule's input sets needs.
pub(crate) struct InputContext<'a> {
    /// The rule's input cells in the rule map.
    pub(crate) region: &'a [IVec2],
    /// The top-left of the rule's input box, which positions are relative to.
    pub(crate) origin: IVec2,
    /// Whether the rule map has legacy region layers.
    pub(crate) legacy: bool,
    /// The rule map's input names, which targets are indexes into.
    pub(crate) input_names: &'a [String],
}

/// Compiles one input set of a rule; `meaning` says what a rule map cell is.
pub(crate) fn compile_input_set(
    set: &InputSetLayers<'_>,
    context: &InputContext<'_>,
    meaning: &mut dyn FnMut(Cell) -> RuleCell,
) -> InputSet {
    let mut compiled = InputSet {
        index: set.index.clone(),
        layers: Vec::new(),
        cells: Vec::new(),
        has_ignore: false,
        never: None,
    };
    for target in &set.targets {
        let mut used: Option<Vec<MatchCell>> = None;
        let mut contradiction: Option<IVec2> = None;
        let mut positions = Vec::new();
        for &p in context.region {
            let mut any = Vec::new();
            let mut none = Vec::new();
            let mut negate = false;
            for (layer, inverted) in target
                .yes
                .iter()
                .map(|l| (l, false))
                .chain(target.no.iter().map(|l| (l, true)))
            {
                let (wanted, unwanted) = if inverted {
                    (&mut none, &mut any)
                } else {
                    (&mut any, &mut none)
                };
                match meaning(layer.tiles.cell(p)) {
                    RuleCell::Empty => {
                        if layer.options.auto_empty {
                            wanted.push(MatchCell {
                                tile: None,
                                mask: layer.options.mask,
                            });
                        }
                    }
                    RuleCell::Unknown(tile) => {
                        if layer.options.auto_empty {
                            wanted.push(MatchCell {
                                tile: Some(tile),
                                mask: layer.options.mask,
                            });
                        }
                    }
                    RuleCell::Typed(MatchType::Tile, tile) => wanted.push(MatchCell {
                        tile: Some(tile),
                        mask: layer.options.mask,
                    }),
                    RuleCell::Typed(MatchType::Empty, _) => wanted.push(MatchCell::EMPTY),
                    RuleCell::Typed(MatchType::NonEmpty, _) => unwanted.push(MatchCell::EMPTY),
                    RuleCell::Typed(MatchType::Other, _) => {
                        // "Any other tile" is "none of the tiles used"; "not any other tile" is
                        // "any of them".
                        let used = used.get_or_insert_with(|| {
                            cells_used(&target.yes, context.region, meaning)
                        });
                        unwanted.extend(used.iter().copied());
                    }
                    RuleCell::Typed(MatchType::Negate, _) => negate = true,
                    RuleCell::Typed(MatchType::Ignore, _) => compiled.has_ignore = true,
                }
            }
            // With legacy regions, a position no input layer of the index names accepts none of
            // the tiles the rule uses, nor the empty cell.
            if context.legacy && any.is_empty() && target.no.is_empty() {
                let used =
                    used.get_or_insert_with(|| cells_used(&target.yes, context.region, meaning));
                none.extend(used.iter().copied());
                none.push(MatchCell::EMPTY);
            }
            if negate {
                std::mem::swap(&mut any, &mut none);
            }
            if !optimize(&mut any, &mut none) {
                contradiction.get_or_insert(p - context.origin);
            }
            if !any.is_empty() || !none.is_empty() {
                positions.push(Position {
                    offset: p - context.origin,
                    start: compiled.cells.len(),
                    any: any.len(),
                    none: none.len(),
                });
                compiled.cells.extend(any);
                compiled.cells.extend(none);
            }
        }
        if let Some(at) = contradiction {
            compiled.never = Some(Never::Contradiction {
                layer: target.name.clone(),
                at,
            });
            return compiled;
        }
        if !positions.is_empty() {
            let index = context
                .input_names
                .iter()
                .position(|n| *n == target.name)
                .unwrap_or_default();
            compiled.layers.push(LayerConditions {
                target: index,
                positions,
            });
        }
    }
    if compiled.layers.is_empty() && !compiled.has_ignore {
        compiled.never = Some(Never::NoConditions);
    }
    compiled
}

/// The tiles an index's input layers use for a target across the rule (Tiled's
/// `collectCellsInRegion`): its ordinary tiles, and the empty cell where the `Empty` tile is used.
fn cells_used(
    layers: &[InputLayer<'_>],
    region: &[IVec2],
    meaning: &mut dyn FnMut(Cell) -> RuleCell,
) -> Vec<MatchCell> {
    let mut cells: Vec<MatchCell> = Vec::new();
    let mut push = |cell: MatchCell| {
        if !cells.contains(&cell) {
            cells.push(cell);
        }
    };
    for layer in layers {
        for &p in region {
            match meaning(layer.tiles.cell(p)) {
                RuleCell::Typed(MatchType::Tile, tile) => push(MatchCell {
                    tile: Some(tile),
                    mask: layer.options.mask,
                }),
                RuleCell::Typed(MatchType::Empty, _) => push(MatchCell::EMPTY),
                _ => {}
            }
        }
    }
    cells
}

/// Tiled's `optimizeAnyNoneOf`: duplicates go; when there are "any of" cells, the "none of"
/// cells are taken out of them and dropped. `false` when nothing is left that "any of" accepts.
fn optimize(any: &mut Vec<MatchCell>, none: &mut Vec<MatchCell>) -> bool {
    none.sort_by_key(|c| c.key());
    none.dedup();
    if any.is_empty() {
        return true;
    }
    any.sort_by_key(|c| c.key());
    any.dedup();
    any.retain(|c| !none.contains(c));
    none.clear();
    !any.is_empty()
}

#[cfg(test)]
mod tests {
    use bt_core::{TileId, TileLayer};
    use glam::UVec2;

    use super::*;
    use crate::rulemap::layers::InputTarget;
    use crate::rulemap::options::InputLayerOptions;

    /// Tile ids below 100 are ordinary tiles of tileset 0; 100 is Empty, 101 NonEmpty, 102
    /// Other, 103 Negate, 104 Ignore; 200 and above are tiles the tileset lacks.
    fn meaning(cell: Cell) -> RuleCell {
        if cell.is_empty() {
            return RuleCell::Empty;
        }
        let tile = TableTile {
            tileset: 0,
            tile: cell.tile(),
            flip: cell.flip(),
        };
        let kind = match cell.tile().0 {
            100 => MatchType::Empty,
            101 => MatchType::NonEmpty,
            102 => MatchType::Other,
            103 => MatchType::Negate,
            104 => MatchType::Ignore,
            id if id >= 200 => return RuleCell::Unknown(tile),
            _ => MatchType::Tile,
        };
        RuleCell::Typed(kind, tile)
    }

    fn layer(cells: &[(i32, i32, u32)]) -> TileLayer {
        let mut layer = TileLayer::finite(UVec2::new(8, 8));
        for &(x, y, id) in cells {
            layer
                .set_cell(
                    IVec2::new(x, y),
                    Cell::new(bt_core::TilesetSlot::new(0).unwrap(), TileId(id)),
                )
                .unwrap();
        }
        layer
    }

    fn input(tiles: &TileLayer) -> InputLayer<'_> {
        InputLayer {
            tiles,
            options: InputLayerOptions {
                auto_empty: false,
                mask: Flip::all(),
            },
        }
    }

    fn tile(id: u32) -> MatchCell {
        MatchCell {
            tile: Some(TableTile {
                tileset: 0,
                tile: TileId(id),
                flip: Flip::empty(),
            }),
            mask: Flip::all(),
        }
    }

    fn compile(yes: &[&TileLayer], no: &[&TileLayer], region: &[IVec2], legacy: bool) -> InputSet {
        let set = InputSetLayers {
            index: String::new(),
            targets: vec![InputTarget {
                name: "set".into(),
                yes: yes.iter().map(|t| input(t)).collect(),
                no: no.iter().map(|t| input(t)).collect(),
            }],
        };
        let names = ["set".to_owned()];
        let context = InputContext {
            region,
            origin: IVec2::ZERO,
            legacy,
            input_names: &names,
        };
        compile_input_set(&set, &context, &mut meaning)
    }

    fn conditions(set: &InputSet) -> Vec<(IVec2, Vec<MatchCell>, Vec<MatchCell>)> {
        set.layers
            .iter()
            .flat_map(|l| &l.positions)
            .map(|p| (p.offset, set.any_of(p).to_vec(), set.none_of(p).to_vec()))
            .collect()
    }

    #[test]
    fn layers_of_one_index_give_alternatives_at_a_position() {
        let a = layer(&[(0, 0, 1), (1, 0, 2)]);
        let b = layer(&[(0, 0, 3), (1, 0, 2)]);
        let region = [IVec2::new(0, 0), IVec2::new(1, 0)];
        let set = compile(&[&a, &b], &[], &region, false);
        assert_eq!(set.never, None);
        assert_eq!(
            conditions(&set),
            [
                (IVec2::new(0, 0), vec![tile(1), tile(3)], vec![]),
                (IVec2::new(1, 0), vec![tile(2)], vec![]),
            ]
        );
    }

    #[test]
    fn inputnot_and_negate_swap_any_and_none() {
        let yes = layer(&[(0, 0, 1), (1, 0, 2)]);
        let no = layer(&[(0, 0, 5)]);
        let negate = layer(&[(1, 0, 103)]);
        let region = [IVec2::new(0, 0), IVec2::new(1, 0)];
        let set = compile(&[&yes, &negate], &[&no], &region, false);
        assert_eq!(
            conditions(&set),
            [
                // "any of 1" with "none of 5" drops the "none": 1 is not 5 already.
                (IVec2::new(0, 0), vec![tile(1)], vec![]),
                // Negate turns "any of 2" into "none of 2".
                (IVec2::new(1, 0), vec![], vec![tile(2)]),
            ]
        );
    }

    #[test]
    fn special_tiles_ask_for_empty_and_non_empty_cells() {
        let yes = layer(&[(0, 0, 100), (1, 0, 101), (2, 0, 104)]);
        let region = [IVec2::new(0, 0), IVec2::new(1, 0), IVec2::new(2, 0)];
        let set = compile(&[&yes], &[], &region, false);
        assert!(set.has_ignore);
        assert_eq!(
            conditions(&set),
            [
                (IVec2::new(0, 0), vec![MatchCell::EMPTY], vec![]),
                (IVec2::new(1, 0), vec![], vec![MatchCell::EMPTY]),
            ]
        );
    }

    #[test]
    fn other_forbids_every_tile_the_target_uses() {
        let yes = layer(&[(0, 0, 1), (1, 0, 102), (2, 0, 2), (3, 0, 100)]);
        let region: Vec<IVec2> = (0..4).map(|x| IVec2::new(x, 0)).collect();
        let set = compile(&[&yes], &[], &region, false);
        let other = conditions(&set)
            .into_iter()
            .find(|(at, _, _)| *at == IVec2::new(1, 0))
            .unwrap();
        assert_eq!(other.1, []);
        assert_eq!(other.2, [MatchCell::EMPTY, tile(1), tile(2)]);
    }

    #[test]
    fn asking_for_and_forbidding_the_same_tile_can_never_match() {
        let yes = layer(&[(0, 0, 1)]);
        let no = layer(&[(0, 0, 1)]);
        let set = compile(&[&yes], &[&no], &[IVec2::ZERO], false);
        assert_eq!(
            set.never,
            Some(Never::Contradiction {
                layer: "set".into(),
                at: IVec2::ZERO
            })
        );
    }

    #[test]
    fn a_set_without_conditions_never_matches_unless_it_has_ignore() {
        let empty = layer(&[(0, 0, 103)]);
        let set = compile(&[&empty], &[], &[IVec2::ZERO], false);
        assert_eq!(set.never, Some(Never::NoConditions));
        let ignore = layer(&[(0, 0, 104)]);
        let set = compile(&[&ignore], &[], &[IVec2::ZERO], false);
        assert_eq!(set.never, None);
        assert!(set.layers.is_empty());
    }

    #[test]
    fn legacy_regions_forbid_the_used_tiles_and_the_empty_cell_where_nothing_is_asked() {
        let yes = layer(&[(0, 0, 1)]);
        let region = [IVec2::new(0, 0), IVec2::new(1, 0)];
        let set = compile(&[&yes], &[], &region, true);
        assert_eq!(
            conditions(&set),
            [
                (IVec2::new(0, 0), vec![tile(1)], vec![]),
                (IVec2::new(1, 0), vec![], vec![MatchCell::EMPTY, tile(1)]),
            ]
        );
    }

    #[test]
    fn auto_empty_asks_for_the_empty_cell_where_a_layer_has_none() {
        let a = layer(&[(0, 0, 1)]);
        let b = layer(&[(1, 0, 2)]);
        let mut auto = input(&b);
        auto.options.auto_empty = true;
        let set = InputSetLayers {
            index: String::new(),
            targets: vec![InputTarget {
                name: "set".into(),
                yes: vec![input(&a), auto],
                no: vec![],
            }],
        };
        let names = ["set".to_owned()];
        let region = [IVec2::new(0, 0), IVec2::new(1, 0)];
        let context = InputContext {
            region: &region,
            origin: IVec2::ZERO,
            legacy: false,
            input_names: &names,
        };
        let compiled = compile_input_set(&set, &context, &mut meaning);
        assert_eq!(
            conditions(&compiled),
            [
                (IVec2::new(0, 0), vec![MatchCell::EMPTY, tile(1)], vec![]),
                (IVec2::new(1, 0), vec![tile(2)], vec![]),
            ]
        );
    }
}
