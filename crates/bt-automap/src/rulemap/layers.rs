//! The roles of a rule map's layers, as Tiled reads them (`AutoMapper::setupRuleMapLayers`):
//! layers are visited depth-first, groups and image layers skipped; `//` marks a note; `regions`,
//! `regions_input` and `regions_output` are the legacy region layers; `rule_options` holds
//! rectangles of rule options; every other name is `input`, `inputnot` or `output` with an index,
//! an underscore and the target layer's name.

use bt_core::{Layer, LayerKind, Map, Properties, TileLayer};

use crate::object_cells::{QtRect, object_tile_rect};
use crate::problem::{Problem, ProblemKind};
use crate::rulemap::options::{InputLayerOptions, OptionReader, RuleOptionValues};

/// An input or inputnot layer of the rule map.
#[derive(Clone, Debug)]
pub(crate) struct InputLayer<'m> {
    /// Its cells.
    pub(crate) tiles: &'m TileLayer,
    /// What its properties set.
    pub(crate) options: InputLayerOptions,
}

/// The input layers of one input set that target one layer.
#[derive(Clone, Debug)]
pub(crate) struct InputTarget<'m> {
    /// The target layer's name.
    pub(crate) name: String,
    /// The `input` layers.
    pub(crate) yes: Vec<InputLayer<'m>>,
    /// The `inputnot` layers.
    pub(crate) no: Vec<InputLayer<'m>>,
}

/// The layers of one input index.
#[derive(Clone, Debug)]
pub(crate) struct InputSetLayers<'m> {
    /// The index: what is between `input`/`inputnot` and the underscore.
    pub(crate) index: String,
    /// Its target layers, sorted by name as Tiled sorts them.
    pub(crate) targets: Vec<InputTarget<'m>>,
}

/// An output layer of the rule map.
#[derive(Clone, Debug)]
pub(crate) struct OutputLayer<'m> {
    /// The target layer's name.
    pub(crate) name: String,
    /// The layer: a tile or an object layer.
    pub(crate) layer: &'m Layer,
    /// Its properties but `Probability`, merged into the target when it is written.
    pub(crate) properties: Properties,
}

/// The layers of one output index.
#[derive(Clone, Debug)]
pub(crate) struct OutputSetLayers {
    /// The index: what is between `output` and the underscore.
    pub(crate) index: String,
    /// Its layers, as indexes into [`LayerSetup::output_layers`], in the rule map's order.
    pub(crate) layers: Vec<usize>,
    /// The weight of the index: the last `Probability` its layers set, else 1.
    pub(crate) probability: f64,
}

/// A rectangle of the `rule_options` layer.
#[derive(Clone, Debug)]
pub(crate) struct OptionsArea {
    /// Its cells in the rule map; rules entirely inside get its options.
    pub(crate) area: QtRect,
    /// The options it sets.
    pub(crate) values: RuleOptionValues,
}

/// What a rule map's layers are for.
#[derive(Clone, Debug, Default)]
pub(crate) struct LayerSetup<'m> {
    /// The legacy `regions` layer.
    pub(crate) regions: Option<&'m TileLayer>,
    /// The legacy `regions_input` layer.
    pub(crate) regions_input: Option<&'m TileLayer>,
    /// The legacy `regions_output` layer.
    pub(crate) regions_output: Option<&'m TileLayer>,
    /// The `rule_options` rectangles, in order (later ones win).
    pub(crate) options_areas: Vec<OptionsArea>,
    /// The input sets, in the order their indexes first appear.
    pub(crate) input_sets: Vec<InputSetLayers<'m>>,
    /// The output sets, in the order their indexes first appear.
    pub(crate) output_sets: Vec<OutputSetLayers>,
    /// The output layers, in the rule map's order.
    pub(crate) output_layers: Vec<OutputLayer<'m>>,
    /// The names of the layers the rules read, in the order they first appear.
    pub(crate) input_names: Vec<String>,
    /// The names of the tile layers the rules write, in order.
    pub(crate) tile_output_names: Vec<String>,
    /// The names of the object layers the rules place objects on, in order.
    pub(crate) object_output_names: Vec<String>,
    /// Whether an error keeps the rule map from being used.
    pub(crate) failed: bool,
}

/// Which of the legacy region layers a layer is.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum RegionsKind {
    Both,
    Input,
    Output,
}

impl RegionsKind {
    fn name(self) -> &'static str {
        match self {
            RegionsKind::Both => "regions",
            RegionsKind::Input => "regions_input",
            RegionsKind::Output => "regions_output",
        }
    }
}

fn starts_with_ignoring_case(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

fn ends_with_ignoring_case(text: &str, suffix: &str) -> bool {
    text.len() >= suffix.len()
        && text
            .get(text.len() - suffix.len()..)
            .is_some_and(|end| end.eq_ignore_ascii_case(suffix))
}

fn push_unique(names: &mut Vec<String>, name: &str) {
    if !names.iter().any(|n| n == name) {
        names.push(name.to_owned());
    }
}

/// Reads the roles of `map`'s layers, recording problems in `problems`.
pub(crate) fn read_layers<'m>(map: &'m Map, problems: &mut Vec<Problem>) -> LayerSetup<'m> {
    let mut setup = LayerSetup::default();
    let mut layers = Vec::new();
    map.for_each_layer(|layer, _| layers.push(layer));
    for layer in layers {
        read_layer(map, layer, &mut setup, problems);
    }
    if setup.input_sets.is_empty() {
        fail(
            &mut setup,
            problems,
            ProblemKind::NoInputLayers,
            "the rule map has no input_<name> or inputnot_<name> layer".to_owned(),
        );
    }
    if setup.output_sets.is_empty() {
        fail(
            &mut setup,
            problems,
            ProblemKind::NoOutputLayers,
            "the rule map has no output_<name> layer".to_owned(),
        );
    }
    // Tiled matches an input set's targets in the order of their names.
    for set in &mut setup.input_sets {
        set.targets.sort_by(|a, b| a.name.cmp(&b.name));
    }
    setup
}

fn fail(setup: &mut LayerSetup<'_>, problems: &mut Vec<Problem>, kind: ProblemKind, text: String) {
    setup.failed = true;
    problems.push(Problem::new(kind, text));
}

fn read_layer<'m>(
    map: &'m Map,
    layer: &'m Layer,
    setup: &mut LayerSetup<'m>,
    problems: &mut Vec<Problem>,
) {
    if matches!(layer.kind, LayerKind::Group(_) | LayerKind::Image(_)) {
        return;
    }
    let name = layer.name.as_str();
    if name.starts_with("//") {
        return;
    }
    if starts_with_ignoring_case(name, "regions") {
        read_regions_layer(layer, setup, problems);
        return;
    }
    if name.eq_ignore_ascii_case("rule_options") {
        read_rule_options(map, layer, setup, problems);
        return;
    }
    let Some(underscore) = name.find('_') else {
        fail(
            setup,
            problems,
            ProblemKind::LayerNameWithoutUnderscore,
            format!(
                "layer “{name}” has no underscore, so it names no target layer; \
                 a name starting with // makes a layer a note"
            ),
        );
        return;
    };
    let target = name.get(underscore + 1..).unwrap_or_default();
    let set_name = name.get(..underscore).unwrap_or_default();
    let index = if starts_with_ignoring_case(set_name, "output") {
        set_name.get(6..)
    } else if starts_with_ignoring_case(set_name, "inputnot") {
        set_name.get(8..)
    } else if starts_with_ignoring_case(set_name, "input") {
        set_name.get(5..)
    } else {
        Some(set_name)
    }
    .unwrap_or_default();

    if starts_with_ignoring_case(name, "input") {
        let LayerKind::Tile(tiles) = &layer.kind else {
            fail(
                setup,
                problems,
                ProblemKind::InputLayerNotTiles,
                format!("layer “{name}” is an input layer but not a tile layer"),
            );
            return;
        };
        push_unique(&mut setup.input_names, target);
        let options = OptionReader::new(format!("layer “{name}”"), problems)
            .read_input_layer_options(&layer.properties);
        let input = InputLayer { tiles, options };
        let set = match setup.input_sets.iter().position(|s| s.index == index) {
            Some(i) => setup.input_sets.get_mut(i),
            None => {
                setup.input_sets.push(InputSetLayers {
                    index: index.to_owned(),
                    targets: Vec::new(),
                });
                setup.input_sets.last_mut()
            }
        };
        let Some(set) = set else {
            return;
        };
        let conditions = match set.targets.iter().position(|t| t.name == target) {
            Some(i) => set.targets.get_mut(i),
            None => {
                set.targets.push(InputTarget {
                    name: target.to_owned(),
                    yes: Vec::new(),
                    no: Vec::new(),
                });
                set.targets.last_mut()
            }
        };
        if let Some(conditions) = conditions {
            if starts_with_ignoring_case(name, "inputnot") {
                conditions.no.push(input);
            } else {
                conditions.yes.push(input);
            }
        }
        return;
    }

    if starts_with_ignoring_case(name, "output") {
        match &layer.kind {
            LayerKind::Tile(_) => push_unique(&mut setup.tile_output_names, target),
            LayerKind::Object(_) => push_unique(&mut setup.object_output_names, target),
            LayerKind::Image(_) | LayerKind::Group(_) => return,
        }
        let options = OptionReader::new(format!("layer “{name}”"), problems)
            .read_output_layer_options(&layer.properties);
        setup.output_layers.push(OutputLayer {
            name: target.to_owned(),
            layer,
            properties: options.properties,
        });
        let layer_index = setup.output_layers.len() - 1;
        match setup.output_sets.iter_mut().find(|s| s.index == index) {
            Some(set) => {
                set.layers.push(layer_index);
                if let Some(p) = options.probability {
                    set.probability = p;
                }
            }
            None => setup.output_sets.push(OutputSetLayers {
                index: index.to_owned(),
                layers: vec![layer_index],
                probability: options.probability.unwrap_or(1.0),
            }),
        }
        return;
    }

    problems.push(Problem::new(
        ProblemKind::UnknownLayer,
        format!("layer “{name}” is not an AutoMap layer, so it is ignored"),
    ));
}

fn read_regions_layer<'m>(
    layer: &'m Layer,
    setup: &mut LayerSetup<'m>,
    problems: &mut Vec<Problem>,
) {
    let name = layer.name.as_str();
    let kind = if name.eq_ignore_ascii_case("regions") {
        RegionsKind::Both
    } else if ends_with_ignoring_case(name, "input") {
        RegionsKind::Input
    } else if ends_with_ignoring_case(name, "output") {
        RegionsKind::Output
    } else {
        problems.push(Problem::new(
            ProblemKind::UnknownLayer,
            format!("layer “{name}” is not an AutoMap layer, so it is ignored"),
        ));
        return;
    };
    let slot = match kind {
        RegionsKind::Both => &mut setup.regions,
        RegionsKind::Input => &mut setup.regions_input,
        RegionsKind::Output => &mut setup.regions_output,
    };
    let twice = slot.is_some();
    let tiles = layer.as_tile_layer();
    if let Some(tiles) = tiles {
        *slot = Some(tiles);
    }
    if twice {
        fail(
            setup,
            problems,
            ProblemKind::RegionsLayerTwice,
            format!("the “{}” layer occurs more than once", kind.name()),
        );
    }
    if tiles.is_none() {
        fail(
            setup,
            problems,
            ProblemKind::RegionsLayerNotTiles,
            format!("layer “{name}” is a regions layer but not a tile layer"),
        );
    }
}

fn read_rule_options<'m>(
    map: &'m Map,
    layer: &'m Layer,
    setup: &mut LayerSetup<'m>,
    problems: &mut Vec<Problem>,
) {
    let Some(objects) = layer.as_object_layer() else {
        fail(
            setup,
            problems,
            ProblemKind::RuleOptionsNotObjects,
            format!("layer “{}” is not an object layer", layer.name),
        );
        return;
    };
    for object in objects.objects.iter() {
        if object.shape != bt_core::Shape::Rectangle || object.tile.is_some() {
            problems.push(Problem::new(
                ProblemKind::RuleOptionsNotRectangle,
                format!(
                    "object {} on layer “{}” is a {}; only rectangles set rule options",
                    object.id,
                    layer.name,
                    object.kind_name()
                ),
            ));
            continue;
        }
        if object.rotation != 0.0 {
            problems.push(Problem::new(
                ProblemKind::RuleOptionsRotated,
                format!(
                    "rectangle {} on layer “{}” is rotated, so it sets no rule options",
                    object.id, layer.name
                ),
            ));
            continue;
        }
        let Some(area) = object_tile_rect(map, object) else {
            problems.push(Problem::new(
                ProblemKind::RuleOptionsNotRectangle,
                format!(
                    "rectangle {} on layer “{}” has a position or size that cannot be placed",
                    object.id, layer.name
                ),
            ));
            continue;
        };
        let values = OptionReader::new(
            format!("rectangle {} on layer “{}”", object.id, layer.name),
            problems,
        )
        .read_area_options(&object.properties);
        setup.options_areas.push(OptionsArea { area, values });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{LayerId, MapObject, ObjectLayer, Property, Value};
    use glam::{DVec2, UVec2};

    use super::*;

    fn tile_layer(name: &str) -> Layer {
        Layer::new(
            LayerId(0),
            name,
            LayerKind::Tile(TileLayer::finite(UVec2::new(4, 4))),
        )
    }

    fn map_of(layers: Vec<Layer>) -> Map {
        Map {
            width: 4,
            height: 4,
            tile_width: 16,
            tile_height: 16,
            layers,
            ..Map::default()
        }
    }

    fn kinds(problems: &[Problem]) -> Vec<ProblemKind> {
        problems.iter().map(|p| p.kind).collect()
    }

    #[test]
    fn names_give_roles_indexes_and_targets() {
        let mut probability = tile_layer("outputA_walls");
        probability
            .properties
            .insert("Probability".into(), Property::new(Value::Float(3.0)));
        let map = map_of(vec![
            tile_layer("Input_walls"),
            tile_layer("inputnot2_floor_top"),
            tile_layer("input2_walls"),
            tile_layer("output_walls"),
            probability,
            tile_layer("outputA_deco"),
            tile_layer("// notes"),
            Layer::new(LayerId(0), "group", LayerKind::Group(vec![])),
        ]);
        let mut problems = Vec::new();
        let setup = read_layers(&map, &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        assert!(!setup.failed);
        let sets: Vec<(&str, Vec<&str>)> = setup
            .input_sets
            .iter()
            .map(|s| {
                (
                    s.index.as_str(),
                    s.targets.iter().map(|t| t.name.as_str()).collect(),
                )
            })
            .collect();
        assert_eq!(
            sets,
            [("", vec!["walls"]), ("2", vec!["floor_top", "walls"])]
        );
        assert_eq!(setup.input_names, ["walls", "floor_top"]);
        assert_eq!(setup.tile_output_names, ["walls", "deco"]);
        let outputs: Vec<(&str, Vec<usize>, f64)> = setup
            .output_sets
            .iter()
            .map(|s| (s.index.as_str(), s.layers.clone(), s.probability))
            .collect();
        assert_eq!(outputs, [("", vec![0], 1.0), ("A", vec![1, 2], 3.0)]);
        let floor = &setup.input_sets[1].targets[0];
        assert_eq!((floor.yes.len(), floor.no.len()), (0, 1));
    }

    #[test]
    fn a_name_without_an_underscore_disables_the_rule_map() {
        let map = map_of(vec![
            tile_layer("input_walls"),
            tile_layer("output_walls"),
            tile_layer("walls"),
        ]);
        let mut problems = Vec::new();
        let setup = read_layers(&map, &mut problems);
        assert!(setup.failed);
        assert_eq!(kinds(&problems), [ProblemKind::LayerNameWithoutUnderscore]);
    }

    #[test]
    fn region_layers_are_tile_layers_found_once() {
        let objects = Layer::new(
            LayerId(0),
            "regions_output",
            LayerKind::Object(ObjectLayer::default()),
        );
        let map = map_of(vec![
            tile_layer("regions"),
            tile_layer("REGIONS"),
            tile_layer("regions_input"),
            objects,
            tile_layer("regionsX"),
            tile_layer("input_a"),
            tile_layer("output_a"),
        ]);
        let mut problems = Vec::new();
        let setup = read_layers(&map, &mut problems);
        assert!(setup.failed);
        assert_eq!(
            kinds(&problems),
            [
                ProblemKind::RegionsLayerTwice,
                ProblemKind::RegionsLayerNotTiles,
                ProblemKind::UnknownLayer
            ]
        );
        assert!(setup.regions.is_some() && setup.regions_input.is_some());
    }

    #[test]
    fn a_map_needs_input_and_output_layers() {
        let mut problems = Vec::new();
        let map = map_of(vec![tile_layer("foo_bar")]);
        let setup = read_layers(&map, &mut problems);
        assert!(setup.failed);
        assert_eq!(
            kinds(&problems),
            [
                ProblemKind::UnknownLayer,
                ProblemKind::NoInputLayers,
                ProblemKind::NoOutputLayers
            ]
        );
    }

    #[test]
    fn rule_options_are_plain_rectangles() {
        let mut options = MapObject {
            pos: DVec2::new(44.0, 12.0),
            size: DVec2::new(24.0, 24.0),
            ..MapObject::default()
        };
        options
            .properties
            .insert("ModX".into(), Property::new(Value::Int(2)));
        let rotated = MapObject {
            rotation: 45.0,
            ..options.clone()
        };
        let point = MapObject {
            shape: bt_core::Shape::Point,
            ..options.clone()
        };
        let layer = Layer::new(
            LayerId(0),
            "rule_options",
            LayerKind::Object(ObjectLayer {
                objects: Arc::new(vec![options, rotated, point]),
                ..ObjectLayer::default()
            }),
        );
        let map = map_of(vec![tile_layer("input_a"), tile_layer("output_a"), layer]);
        let mut problems = Vec::new();
        let setup = read_layers(&map, &mut problems);
        assert_eq!(
            kinds(&problems),
            [
                ProblemKind::RuleOptionsRotated,
                ProblemKind::RuleOptionsNotRectangle
            ]
        );
        assert_eq!(setup.options_areas.len(), 1);
        assert_eq!(setup.options_areas[0].values.mod_x, Some(2));
        assert_eq!(
            setup.options_areas[0].area,
            QtRect {
                x: 2,
                y: 0,
                w: 3,
                h: 3
            }
        );
    }
}
