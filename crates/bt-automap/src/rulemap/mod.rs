//! Compiling one rule map as Tiled 1.12 reads it (`AutoMapper::AutoMapper`, `setupRules`): the
//! roles of its layers and its options, the special tiles by `MatchType`, the rules as the
//! 8-connected components of its input and output cells ordered by their top-left, the
//! `rule_options` rectangles, and each rule's input sets and output sets, compiled once. Tiles
//! are numbered in the rule set's tileset table and bound to a map's tilesets when a run starts.

mod extract;
mod inputs;
mod layers;
mod match_type;
mod options;
mod outputs;
mod rule;
mod tilesets;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_core::{CustomTypes, Map, Properties, TileLayer};
use bt_utils::hash::CellHashSet;
use bt_utils::rng::RandomPicker;
use glam::IVec2;

use crate::bx::Bx;
use crate::cell_set::CellSet;
use crate::object_cells::object_tile_rect;
use crate::problem::{Problem, ProblemKind};

pub(crate) use inputs::InputSet;
pub use inputs::MatchCell;
pub(crate) use match_type::MatchTypeCache;
pub(crate) use options::{MapOptions, RuleOptions};
pub(crate) use outputs::OutputSet;
pub(crate) use rule::{Alternative, Rule};
pub use tilesets::TableTile;
pub(crate) use tilesets::{TableEntry, TilesetTable};

use extract::components;
use inputs::{InputContext, compile_input_set};
use layers::{LayerSetup, read_layers};
use match_type::CellMeanings;
use options::OptionReader;
use outputs::{OutputContext, compile_output_set};

/// An output layer of a rule map, as a target.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OutputLayerInfo {
    /// The target layer's name.
    pub(crate) name: String,
    /// Whether it places objects (else it writes tiles).
    pub(crate) objects: bool,
    /// What it merges into the target layer's properties when it is written.
    pub(crate) properties: Properties,
}

/// A rule map, compiled. What §9 of the design calls a rule map's information is read from it:
/// its path, rules, whether it is used, what it reads and writes, and its problems.
#[derive(Clone, Debug)]
pub struct RuleMap {
    path: PathBuf,
    map: Arc<Map>,
    pub(crate) options: MapOptions,
    pub(crate) rules: Vec<Rule>,
    pub(crate) input_names: Vec<String>,
    pub(crate) tile_outputs: Vec<String>,
    pub(crate) object_outputs: Vec<String>,
    pub(crate) output_layers: Vec<OutputLayerInfo>,
    problems: Vec<Problem>,
    used: bool,
}

impl RuleMap {
    /// The rule map's file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The rule map as it was read.
    pub fn map(&self) -> &Arc<Map> {
        &self.map
    }

    /// How many rules it has.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Whether it runs: no error keeps it from being used.
    pub fn is_used(&self) -> bool {
        self.used
    }

    /// What compiling it found: errors (when it is not used), warnings and notes.
    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    /// The names of the layers its rules read, in the order they first appear.
    pub fn inputs(&self) -> &[String] {
        &self.input_names
    }

    /// The names of the tile layers its rules write.
    pub fn tile_outputs(&self) -> &[String] {
        &self.tile_outputs
    }

    /// The names of the object layers its rules place objects on.
    pub fn object_outputs(&self) -> &[String] {
        &self.object_outputs
    }

    /// Adds a problem found outside the compiler (by the file reader, say).
    pub(crate) fn record_problem(&mut self, problem: Problem) {
        self.problems.push(problem);
    }
}

/// What compiling the rule maps of a rule set shares.
pub(crate) struct CompileContext<'c> {
    /// The rule set's tilesets.
    pub(crate) table: &'c mut TilesetTable,
    /// The project's custom types, which give tiles their classes' `MatchType`.
    pub(crate) types: &'c CustomTypes,
    /// The meanings of the tiles met so far.
    pub(crate) cache: &'c mut MatchTypeCache,
}

/// The cells of a tile layer that hold anything.
fn cells_of(layer: &TileLayer) -> impl Iterator<Item = IVec2> + '_ {
    layer.cells().map(|placed| placed.at)
}

/// Lists positions for a message: the first few, and how many more.
fn listed(positions: &[IVec2]) -> String {
    const SHOWN: usize = 8;
    let mut text = positions
        .iter()
        .take(SHOWN)
        .map(|p| format!("({}, {})", p.x, p.y))
        .collect::<Vec<_>>()
        .join(", ");
    if positions.len() > SHOWN {
        text.push_str(&format!(" and {} more", positions.len() - SHOWN));
    }
    text
}

/// The input and output cells of a rule map, as Tiled derives them.
struct Regions {
    input: CellHashSet<IVec2>,
    output: CellHashSet<IVec2>,
    legacy: bool,
}

fn read_regions(map: &Map, setup: &LayerSetup<'_>, problems: &mut Vec<Problem>) -> Regions {
    let mut input = CellHashSet::default();
    let mut output = CellHashSet::default();
    if let Some(regions) = setup.regions {
        input.extend(cells_of(regions));
        output.extend(cells_of(regions));
    }
    if let Some(regions) = setup.regions_input {
        input.extend(cells_of(regions));
    }
    if let Some(regions) = setup.regions_output {
        output.extend(cells_of(regions));
    }
    let legacy = setup.regions.is_some() || setup.regions_input.is_some();
    let input_layers = setup
        .input_sets
        .iter()
        .flat_map(|set| &set.targets)
        .flat_map(|target| target.yes.iter().chain(&target.no));
    if legacy {
        let outside = input_layers
            .flat_map(|layer| cells_of(layer.tiles))
            .filter(|p| !input.contains(p))
            .count();
        if outside > 0 {
            problems.push(Problem::new(
                ProblemKind::TilesOutsideRegions,
                format!(
                    "{outside} tiles of the input layers lie outside the regions layers, so \
                     they belong to no rule"
                ),
            ));
        }
    } else {
        input.extend(input_layers.flat_map(|layer| cells_of(layer.tiles)));
    }
    let outputs_given = setup.regions.is_some() || setup.regions_output.is_some();
    let mut outside = 0usize;
    for output_layer in &setup.output_layers {
        if let Some(tiles) = output_layer.layer.as_tile_layer() {
            for p in cells_of(tiles) {
                if !outputs_given {
                    output.insert(p);
                } else if !output.contains(&p) {
                    outside += 1;
                }
            }
        } else if let Some(group) = output_layer.layer.as_object_layer() {
            for object in group.objects.iter() {
                match object_tile_rect(map, object) {
                    Some(rect) => {
                        if !outputs_given && let Some(cells) = rect.cells() {
                            output.extend(cells.points());
                        }
                    }
                    None => problems.push(Problem::new(
                        ProblemKind::OutputObjectOutOfRange,
                        format!(
                            "object {} on layer “{}” has a position or size that cannot be \
                             placed, so it is never placed",
                            object.id, output_layer.layer.name
                        ),
                    )),
                }
            }
        }
    }
    if outside > 0 {
        problems.push(Problem::new(
            ProblemKind::TilesOutsideRegions,
            format!(
                "{outside} tiles of the output layers lie outside the regions layers, so they \
                 belong to no rule"
            ),
        ));
    }
    Regions {
        input,
        output,
        legacy,
    }
}

/// Compiles the rule map `map`, read from `path`, as rule map `index` of a rule set.
pub(crate) fn compile_rule_map(
    map: Arc<Map>,
    path: PathBuf,
    index: usize,
    context: &mut CompileContext<'_>,
) -> RuleMap {
    let mut problems = Vec::new();
    let mut options =
        OptionReader::new("the rule map", &mut problems).read_map_options(&map.properties);
    let slots: Vec<usize> = map
        .tilesets
        .iter()
        .map(|reference| context.table.insert_tileset(reference, index))
        .collect();
    let setup = read_layers(&map, &mut problems);
    let output_layers = setup
        .output_layers
        .iter()
        .map(|layer| OutputLayerInfo {
            name: layer.name.clone(),
            objects: layer.layer.as_object_layer().is_some(),
            properties: layer.properties.clone(),
        })
        .collect();
    let mut compiled = RuleMap {
        path,
        map: Arc::clone(&map),
        options: MapOptions::default(),
        rules: Vec::new(),
        input_names: setup.input_names.clone(),
        tile_outputs: setup.tile_output_names.clone(),
        object_outputs: setup.object_output_names.clone(),
        output_layers,
        problems: Vec::new(),
        used: !setup.failed,
    };
    if setup.failed {
        compiled.options = options;
        compiled.problems = problems;
        return compiled;
    }

    let regions = read_regions(&map, &setup, &mut problems);
    // Legacy region layers make rules match in order unless the map says otherwise.
    if regions.legacy && !options.match_in_order_set {
        options.match_in_order = true;
    }
    let mut meanings = CellMeanings {
        slots: &slots,
        table: &*context.table,
        types: context.types,
        cache: &mut *context.cache,
        unknown_names: Vec::new(),
    };
    let mut all: CellHashSet<IVec2> = regions.input.clone();
    all.extend(regions.output.iter().copied());
    let mut orphan_inputs = Vec::new();
    let mut orphan_outputs = Vec::new();
    for component in components(&all) {
        let input: Vec<IVec2> = component
            .cells
            .iter()
            .copied()
            .filter(|p| regions.input.contains(p))
            .collect();
        let output: Vec<IVec2> = component
            .cells
            .iter()
            .copied()
            .filter(|p| regions.output.contains(p))
            .collect();
        let (Some(input_box), Some(output_box)) = (
            Bx::bounding(input.iter().copied()),
            Bx::bounding(output.iter().copied()),
        ) else {
            if output.is_empty() {
                orphan_inputs.push(component.bounds.min);
            } else {
                orphan_outputs.push(component.bounds.min);
            }
            continue;
        };
        let origin = input_box.min;
        let mut rule_options = options.rule_defaults;
        for area in &setup.options_areas {
            let inside = area
                .area
                .cells()
                .is_some_and(|rect| component.cells.iter().all(|p| rect.contains(*p)));
            if inside {
                rule_options = rule_options.merged(&area.values);
            }
        }
        let input_context = InputContext {
            region: &input,
            origin,
            legacy: regions.legacy,
            input_names: &setup.input_names,
        };
        let input_sets = setup
            .input_sets
            .iter()
            .map(|set| compile_input_set(set, &input_context, &mut |cell| meanings.of(cell)))
            .collect();
        let output_cells = CellSet::from_cells(output.iter().copied());
        let output_context = OutputContext {
            rule_map: &map,
            region: &output_cells,
            origin,
        };
        let mut fixed = None;
        let mut alternatives = Vec::new();
        for set in &setup.output_sets {
            let compiled_set =
                compile_output_set(set, &setup, &output_context, &mut |cell| meanings.of(cell));
            if compiled_set.is_empty() && !regions.legacy {
                continue;
            }
            if set.index.is_empty() && !regions.legacy {
                fixed = Some(compiled_set);
            } else {
                alternatives.push(Alternative {
                    set: compiled_set,
                    weight: set.probability,
                });
            }
        }
        let mut picker = RandomPicker::new();
        for (i, alternative) in alternatives.iter().enumerate() {
            picker.add(i, alternative.weight);
        }
        compiled.rules.push(Rule {
            origin,
            input_box: input_box.translated(-origin),
            bounds: component.bounds,
            output_cells: output_cells.translated(-origin),
            output_origin: output_box.min,
            options: rule_options,
            input_sets,
            fixed,
            alternatives,
            picker,
        });
    }
    for unknown in &meanings.unknown_names {
        let tileset = context
            .table
            .get(unknown.tileset)
            .map(TableEntry::label)
            .unwrap_or_default();
        problems.push(Problem::new(
            ProblemKind::UnknownMatchType,
            format!(
                "tile {} of {tileset} has the MatchType “{}”, which is none of Empty, \
                 NonEmpty, Other, Negate and Ignore (they are case-sensitive), so it counts as \
                 an ordinary tile",
                unknown.tile, unknown.name
            ),
        ));
    }
    if !orphan_inputs.is_empty() {
        problems.push(Problem::new(
            ProblemKind::OrphanInput,
            format!(
                "input tiles touching no output tiles make no rule: at {}",
                listed(&orphan_inputs)
            ),
        ));
    }
    if !orphan_outputs.is_empty() {
        problems.push(Problem::new(
            ProblemKind::OrphanOutput,
            format!(
                "output tiles or objects touching no input tiles make no rule: at {}",
                listed(&orphan_outputs)
            ),
        ));
    }
    for (i, rule) in compiled.rules.iter().enumerate() {
        if let Some(reason) = rule.never_matches() {
            problems.push(Problem::new(
                ProblemKind::RuleNeverMatches,
                format!(
                    "rule {} at ({}, {}) can never match: {reason}",
                    i + 1,
                    rule.bounds.min.x,
                    rule.bounds.min.y
                ),
            ));
        }
    }
    if compiled.rules.is_empty() {
        problems.push(Problem::new(
            ProblemKind::NoRules,
            "the rule map has no rules: no input tiles touch output tiles".to_owned(),
        ));
    }
    compiled.options = options;
    compiled.problems = problems;
    compiled
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{
        ClassType, ClassUsage, CustomType, Layer, LayerId, LayerKind, MapObject, ObjectLayer,
        Property, TileData, TileId, Tileset, Value,
    };
    use bt_test_data::fixtures::{Atlas, MapBuilder, grid_layer, rule_tiles};
    use glam::{DVec2, UVec2};

    use super::*;
    use crate::rulemap::inputs::MatchCell;

    fn atlas() -> Arc<Tileset> {
        Atlas::builder().build_embedded().unwrap().tileset
    }

    fn rule_map(size: UVec2) -> MapBuilder {
        MapBuilder::new(UVec2::splat(16), size)
            .tileset(atlas())
            .tileset(rule_tiles().unwrap())
    }

    fn compile_with(map: Map, types: &CustomTypes) -> RuleMap {
        let mut table = TilesetTable::new();
        let mut cache = MatchTypeCache::default();
        let mut context = CompileContext {
            table: &mut table,
            types,
            cache: &mut cache,
        };
        compile_rule_map(
            Arc::new(map),
            PathBuf::from("/rules/rules.tmx"),
            0,
            &mut context,
        )
    }

    fn compile(map: Map) -> RuleMap {
        compile_with(map, &CustomTypes::new())
    }

    fn kinds(rule_map: &RuleMap) -> Vec<ProblemKind> {
        rule_map.problems().iter().map(|p| p.kind).collect()
    }

    fn options_rectangle(x: f64, w: f64, name: &str, value: Value) -> MapObject {
        let mut object = MapObject {
            pos: DVec2::new(x, 12.0),
            size: DVec2::new(w, 24.0),
            ..MapObject::default()
        };
        object
            .properties
            .insert(name.to_owned(), Property::new(value));
        object
    }

    #[test]
    fn rules_are_found_in_order_with_their_options() {
        let options = Layer::new(
            LayerId(0),
            "rule_options",
            LayerKind::Object(ObjectLayer {
                objects: Arc::new(vec![
                    options_rectangle(44.0, 24.0, "OffsetX", Value::Int(1)),
                    options_rectangle(75.0, 26.0, "Probability", Value::Float(0.1)),
                ]),
                ..ObjectLayer::default()
            }),
        );
        let mut map = rule_map(UVec2::new(7, 3))
            .tile_layer("input_set", ". . . . . . .\n. 2 . 2 . 2 .\n. . . . . . .")
            .tile_layer("output_auto", ". . . . . . .\n. 5 . 6 . 7 .\n. . . . . . .")
            .layer(options)
            .build()
            .unwrap();
        map.properties
            .insert("ModX".into(), Property::new(Value::Int(2)));
        let compiled = compile(map);
        assert!(compiled.is_used());
        assert_eq!(kinds(&compiled), []);
        let found: Vec<(IVec2, i32, i32, f64)> = compiled
            .rules
            .iter()
            .map(|r| {
                (
                    r.origin,
                    r.options.mod_x,
                    r.options.offset_x,
                    r.options.probability,
                )
            })
            .collect();
        assert_eq!(
            found,
            [
                (IVec2::new(1, 1), 2, 0, 1.0),
                (IVec2::new(3, 1), 2, 1, 1.0),
                (IVec2::new(5, 1), 2, 0, 0.1),
            ]
        );
        let first = &compiled.rules[0];
        assert!(first.fixed.is_some() && first.alternatives.is_empty());
        assert_eq!(compiled.inputs(), ["set"]);
        assert_eq!(compiled.tile_outputs(), ["auto"]);
    }

    #[test]
    fn tiles_touching_nothing_make_no_rule_and_say_so() {
        // Tiled's ignore-empty-region case: the input and output tiles are two cells apart.
        let map = rule_map(UVec2::new(5, 3))
            .tile_layer("input_set", ". . . . .\n. 3 . . .\n. . . . .")
            .tile_layer("output_set", ". . . . .\n. . . 4 .\n. . . . .")
            .build()
            .unwrap();
        let compiled = compile(map);
        assert!(compiled.is_used());
        assert_eq!(compiled.rule_count(), 0);
        assert_eq!(
            kinds(&compiled),
            [
                ProblemKind::OrphanInput,
                ProblemKind::OrphanOutput,
                ProblemKind::NoRules
            ]
        );
        assert!(compiled.problems()[0].text.contains("(1, 1)"));
    }

    #[test]
    fn special_tiles_and_ignore_join_rule_parts() {
        let map = rule_map(UVec2::new(5, 3))
            .tile_layer("input_set", ". . . . .\n. 3 I I .\n. . . . .")
            .tile_layer("output_set", ". . . . .\n. . . 4 .\n. . . . .")
            .tile_layer("input_set", ". . . . .\n. . . . E\n. . . . .")
            .build()
            .unwrap();
        let compiled = compile(map);
        assert_eq!(compiled.rule_count(), 1);
        let rule = &compiled.rules[0];
        let set = &rule.input_sets[0];
        assert!(set.has_ignore);
        let positions: Vec<(IVec2, usize, usize)> = set.layers[0]
            .positions
            .iter()
            .map(|p| (p.offset, p.any, p.none))
            .collect();
        assert_eq!(
            positions,
            [(IVec2::new(0, 0), 1, 0), (IVec2::new(3, 0), 1, 0)]
        );
        assert_eq!(set.any_of(&set.layers[0].positions[1]), [MatchCell::EMPTY]);
        assert_eq!(rule.input_box.size(), IVec2::new(4, 1));
    }

    #[test]
    fn legacy_regions_turn_on_match_in_order_and_leave_out_tiles_outside() {
        let map = rule_map(UVec2::new(6, 3))
            .tile_layer("regions", ". . . . . .\n. 1 1 . . .\n. . . . . .")
            .tile_layer("input_set", ". . . . . .\n. 2 . . 2 .\n. . . . . .")
            .tile_layer("output_set", ". . . . . .\n. . 3 . . .\n. . . . . .")
            .build()
            .unwrap();
        let compiled = compile(map);
        assert!(compiled.options.match_in_order);
        assert_eq!(compiled.rule_count(), 1);
        assert_eq!(kinds(&compiled), [ProblemKind::TilesOutsideRegions]);
        // The unnamed output is a random one in legacy mode.
        let rule = &compiled.rules[0];
        assert!(rule.fixed.is_none());
        assert_eq!(rule.alternatives.len(), 1);
        // The empty input cell of the region accepts neither the used tile nor the empty cell.
        let set = &rule.input_sets[0];
        let second = &set.layers[0].positions[1];
        assert_eq!(
            (second.offset, second.any, second.none),
            (IVec2::new(1, 0), 0, 2)
        );
    }

    #[test]
    fn a_map_with_a_bad_layer_name_is_not_used() {
        let map = rule_map(UVec2::new(3, 3))
            .tile_layer("input_set", ". . .\n. 2 .\n. . .")
            .tile_layer("output_set", ". . .\n. 3 .\n. . .")
            .tile_layer("set", ". . .\n. . .\n. . .")
            .build()
            .unwrap();
        let compiled = compile(map);
        assert!(!compiled.is_used());
        assert_eq!(compiled.rule_count(), 0);
        assert_eq!(kinds(&compiled), [ProblemKind::LayerNameWithoutUnderscore]);
    }

    fn atlas_with_tile(data: TileData) -> Arc<Tileset> {
        let mut tileset = Tileset::clone(&atlas());
        tileset.tiles.insert(TileId(9), data);
        Arc::new(tileset)
    }

    #[test]
    fn match_types_come_from_properties_or_classes_and_unknown_ones_are_warned() {
        let mut properties = Properties::new();
        properties.insert(
            "MatchType".into(),
            Property::new(Value::String("empty".into())),
        );
        let odd = atlas_with_tile(TileData {
            properties,
            ..TileData::default()
        });
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(3, 1))
            .tileset(odd)
            .tile_layer("input_set", "9 . .")
            .tile_layer("output_set", "1 . .")
            .build()
            .unwrap();
        let compiled = compile(map);
        assert_eq!(kinds(&compiled), [ProblemKind::UnknownMatchType]);
        assert!(compiled.problems()[0].text.contains("“empty”"));

        let classy = atlas_with_tile(TileData {
            class: "Hole".into(),
            ..TileData::default()
        });
        let mut members = Properties::new();
        members.insert(
            "MatchType".into(),
            Property::new(Value::String("Empty".into())),
        );
        let mut types = CustomTypes::new();
        types.add(CustomType::class(
            "Hole",
            ClassType {
                members,
                use_as: ClassUsage::TILE,
                ..ClassType::default()
            },
        ));
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(3, 1))
            .tileset(classy)
            .tile_layer("input_set", "9 . .")
            .tile_layer("output_set", "1 . .")
            .build()
            .unwrap();
        let compiled = compile_with(map, &types);
        let set = &compiled.rules[0].input_sets[0];
        assert_eq!(set.any_of(&set.layers[0].positions[0]), [MatchCell::EMPTY]);
    }

    #[test]
    fn outputs_with_indexes_are_weighted_alternatives_and_empty_ones_are_left_out() {
        let mut probable = Layer::new(
            LayerId(0),
            "outputA_auto",
            LayerKind::Tile(grid_layer(". .\n. 6").unwrap()),
        );
        probable
            .properties
            .insert("Probability".into(), Property::new(Value::Float(3.0)));
        let map = rule_map(UVec2::new(2, 2))
            .tile_layer("input_set", ". .\n. 2")
            .tile_layer("outputB_auto", ". .\n. 7")
            .layer(probable)
            .tile_layer("outputC_auto", ". .\n. .")
            .tile_layer("output_auto_base", ". .\n. 5")
            .build()
            .unwrap();
        let compiled = compile(map);
        let rule = &compiled.rules[0];
        let weights: Vec<(String, f64)> = rule
            .alternatives
            .iter()
            .map(|a| (a.set.index.clone(), a.weight))
            .collect();
        assert_eq!(weights, [("B".to_owned(), 1.0), ("A".to_owned(), 3.0)]);
        assert_eq!(rule.picker.len(), 2);
        assert!(rule.fixed.is_some());
        assert_eq!(compiled.tile_outputs(), ["auto", "auto_base"]);
    }
}
