//! `bt info` (`10-agents.md`, §10.1): what a map is made of. Its grid and size, how a grid that
//! is not square sits on screen, whether the editor holds it with unsaved changes (the caller
//! says), its class and properties, its tilesets with their terrains, its layers top first, the
//! AutoMap rules that apply to it (or, on a rule map, what its rules read and write and what
//! keeps tiles from making rules), and the warnings of reading it.
//!
//! Reading files is the caller's: it gives the map, its reading warnings and the rules it found
//! for it, read through the map's loader; a rule map's own rules are compiled here from the map.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_automap::layers::Setup;
use bt_automap::{Problem, RuleSet};
use bt_core::{
    CustomTypes, Layer, LayerKind, Map, Orientation, StaggerAxis, StaggerIndex, TileRenderSize,
    Tileset, TilesetSlot,
};
use bt_utils::IRect;
use bt_utils::text::{and_list, plural};
use glam::{DVec2, UVec2};
use serde::{Deserialize, Serialize};

use crate::layer_tree::layers_top_first;
use crate::names::{Names, object_type};
use crate::property_text::{properties_text, quoted};
use crate::report::{Line, LineKind, Outcome, Report, Section};
use crate::rule_words::{near_miss_layer, problem_line, rules_file_name, tile_layer_named};
use crate::shapes::map_cells;
use crate::symbols::TileKey;
use crate::tally::{Tally, counts_text};
use crate::words::{number_text, region_text};

/// The kinds of tiles a layer's line names before "and N more".
const KINDS_SHOWN: usize = 5;

/// The terrains a tileset's line names before "and N more".
const TERRAINS_SHOWN: usize = 8;

/// What `bt info` is asked about: the map, what the caller knows of it, and the rules it found.
pub struct InfoRequest<'a> {
    map: &'a Map,
    names: &'a Names,
    file: String,
    editor: Option<EditorCopy>,
    warnings: Vec<String>,
    rules: Option<MapRules<'a>>,
    custom_types: Arc<CustomTypes>,
}

/// Whether the editor holds the map with changes not saved yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorCopy {
    /// Open in the editor, saved.
    Saved,
    /// Open in the editor with unsaved changes: what bt read is the editor's copy.
    Unsaved,
}

/// The AutoMap rules that apply to a map, as the caller found and read them.
pub enum MapRules<'r> {
    /// The map's AutoMap layers: their setup, their rules or why those can't be read, and what
    /// rebuilding what was painted could not put back.
    Layers {
        /// The setup the project file keeps.
        setup: &'r Setup,
        /// The rules, or why they can't be read.
        rules: Result<&'r RuleSet, String>,
        /// The entries of the project file that did not apply.
        notes: &'r [Problem],
        /// The project file keeping what was painted, as the map's folder reaches it.
        kept_in: Option<&'r str>,
    },
    /// A rules file found for the map: the `rules.txt` beside it, say.
    File {
        /// The rules file.
        path: &'r Path,
        /// Its rules, or why they can't be read.
        rules: Result<&'r RuleSet, String>,
    },
}

impl<'a> InfoRequest<'a> {
    /// `bt info` on `map`, whose file is called `file`.
    pub fn new(map: &'a Map, names: &'a Names, file: impl Into<String>) -> InfoRequest<'a> {
        InfoRequest {
            map,
            names,
            file: file.into(),
            editor: None,
            warnings: Vec::new(),
            rules: None,
            custom_types: Arc::new(CustomTypes::default()),
        }
    }

    /// The map is open in the editor, saved or not.
    pub fn editor(mut self, copy: EditorCopy) -> InfoRequest<'a> {
        self.editor = Some(copy);
        self
    }

    /// What reading the map warned about.
    pub fn warnings(mut self, warnings: Vec<String>) -> InfoRequest<'a> {
        self.warnings = warnings;
        self
    }

    /// The AutoMap rules that apply to the map. A rule map is described by its own rules
    /// instead, whatever rules file lists it.
    pub fn rules(mut self, rules: MapRules<'a>) -> InfoRequest<'a> {
        self.rules = Some(rules);
        self
    }

    /// The project's custom types, which give a rule map's tiles their classes' `MatchType`.
    pub fn custom_types(mut self, types: Arc<CustomTypes>) -> InfoRequest<'a> {
        self.custom_types = types;
        self
    }
}

/// `bt info MAP`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InfoOutcome {
    /// The map's file name.
    pub file: String,
    /// The grid: `orthogonal`, `isometric`, `staggered`, `hexagonal` or `oblique`.
    pub orientation: String,
    /// The size in cells; for an infinite map, the size it was saved with.
    pub size: UVec2,
    /// For an infinite map, the cells painted so far (or its saved size while it has none).
    pub infinite: Option<IRect>,
    /// The size of a cell in pixels.
    pub tile_size: UVec2,
    /// How a grid that is not square sits on screen, in words.
    pub layout: Option<String>,
    /// Whether the editor holds the map, and with unsaved changes.
    pub editor: Option<EditorCopy>,
    /// The map's class; empty for none.
    pub class: String,
    /// The map's properties, their values as scripts write them.
    pub properties: BTreeMap<String, String>,
    /// The tilesets, in the map's order.
    pub tilesets: Vec<TilesetInfo>,
    /// The layers, the top one first, each group before the layers in it.
    pub layers: Vec<LayerInfo>,
    /// The AutoMap rules that apply to the map, or the map's own rules when it is a rule map.
    pub automap: Option<RulesInfo>,
    /// What reading the map warned about.
    pub warnings: Vec<String>,
}

/// A tileset of the map.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TilesetInfo {
    /// Its name, as `TILESET:ID` writes it.
    pub name: String,
    /// Whether its tiles are images of their own (sprites).
    pub collection: bool,
    /// How many tiles it has.
    pub tiles: u32,
    /// Its tile size in pixels.
    pub tile_size: UVec2,
    /// An atlas's columns; 0 for a collection.
    pub columns: u32,
    /// Its terrains, by name.
    pub terrains: Vec<String>,
    /// Where a tile placed in cell X,Y is drawn, when that is not the cell: tiles larger than a
    /// cell, or drawn offset.
    pub drawn_over: Option<String>,
}

/// A layer of the map.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerInfo {
    /// Its id.
    pub id: u32,
    /// Its name.
    pub name: String,
    /// How many groups it is in.
    pub depth: usize,
    /// Whether another layer has its name, so it goes by `#ID`.
    pub name_shared: bool,
    /// What it holds, in words: `tiles: 96 cells in 0,0 12x8; grass ×96`.
    pub holds: String,
    /// Whether it is hidden.
    pub hidden: bool,
    /// Whether it is locked.
    pub locked: bool,
    /// Its offset in pixels, when it has one.
    pub offset: Option<DVec2>,
    /// Its parallax factor, when it is not 1.
    pub parallax: Option<DVec2>,
}

/// What the AutoMap rules of a map are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RulesKind {
    /// The map's AutoMap layers: rules that run after every edit, on what was painted.
    Layers,
    /// A rules file found for the map; the edit step `automap` runs it.
    File,
    /// The map is a rule map: these are its own rules.
    RuleMap,
}

/// AutoMap rules, as `bt info` tells them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RulesInfo {
    /// What the rules are to the map.
    pub kind: RulesKind,
    /// The rules file or rule map, by name.
    pub rules: String,
    /// Why they can't be read, when they can't.
    pub unreadable: Option<String>,
    /// How many rule maps there are.
    pub rule_maps: usize,
    /// How many rules there are.
    pub rule_count: usize,
    /// AutoMap layers' runs, each as the `RULE-LAYER=MAP-LAYER` bindings it was started with.
    pub runs: Vec<String>,
    /// AutoMap layers' seed.
    pub seed: Option<u64>,
    /// The layers the rules read, in the map's names (on a rule map, the names the rules use).
    pub reads: Vec<String>,
    /// The layers they write.
    pub writes: Vec<String>,
    /// What keeps them from running, and their other problems and notes.
    pub notes: Vec<RuleNote>,
    /// For AutoMap layers, the project file keeping what was painted, as the map's folder
    /// reaches it.
    #[serde(default)]
    pub kept_in: Option<String>,
}

/// A line about rules: a warning, a note, or a fact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleNote {
    /// How the line reads to its reader.
    pub kind: LineKind,
    /// What it says.
    pub text: String,
}

impl InfoOutcome {
    /// What `request` asks about the map.
    pub fn from_request(request: &InfoRequest<'_>) -> InfoOutcome {
        let map = request.map;
        let names = request.names;
        InfoOutcome {
            file: request.file.clone(),
            orientation: orientation_name(map.orientation).to_owned(),
            size: UVec2::new(map.width, map.height),
            infinite: map.infinite.then(|| map_cells(map)),
            tile_size: map.tile_size(),
            layout: layout_text(map),
            editor: request.editor,
            class: map.class.clone(),
            properties: properties_text(&map.properties),
            tilesets: tilesets_info(map, names),
            layers: layers_info(map, names),
            automap: rules_info(request),
            warnings: request.warnings.clone(),
        }
    }
}

/// The grid's name in words.
pub(crate) fn orientation_name(orientation: Orientation) -> &'static str {
    match orientation {
        Orientation::Orthogonal => "orthogonal",
        Orientation::Isometric => "isometric",
        Orientation::Staggered => "staggered",
        Orientation::Hexagonal => "hexagonal",
        Orientation::Oblique => "oblique",
    }
}

/// How the cells of a map that is not orthogonal sit on screen; cells are `X,Y` in a grid all
/// the same, as `bt view` prints them.
pub(crate) fn layout_text(map: &Map) -> Option<String> {
    let which = match map.stagger_index {
        StaggerIndex::Odd => "odd",
        StaggerIndex::Even => "even",
    };
    let (lines, shift, view) = match map.stagger_axis {
        StaggerAxis::Y => (
            "rows",
            "to the right",
            "bt view shows the shift: two characters a cell, the shifted rows one further right",
        ),
        StaggerAxis::X => (
            "columns",
            "lower",
            "bt view prints the columns without the shift",
        ),
    };
    let last = |size: u32| i64::from(size) - 1;
    match map.orientation {
        Orientation::Orthogonal => None,
        Orientation::Isometric if map.infinite => Some(
            "on screen the grid is turned into diamonds: x runs down-right and y down-left"
                .to_owned(),
        ),
        Orientation::Isometric => Some(format!(
            "on screen the grid is a diamond: 0,0 is its top corner, x runs down-right and y \
             down-left; its edges are y=0 (upper right), x={} (lower right), y={} (lower left) \
             and x=0 (upper left)",
            last(map.width),
            last(map.height)
        )),
        Orientation::Hexagonal => Some(format!(
            "hexagons with sides of {} px: {which} {lines} sit half a hexagon {shift}; each \
             touches 6 others ({view})",
            map.hex_side_length
        )),
        Orientation::Staggered => Some(format!(
            "diamonds in staggered {lines}: {which} {lines} sit half a diamond {shift}; a cell's \
             sides face its 4 diagonal neighbours ({view})"
        )),
        Orientation::Oblique => Some(format!(
            "an oblique grid: each row shifts {} px right and each column {} px down",
            map.skew.x, map.skew.y
        )),
    }
}

fn tilesets_info(map: &Map, names: &Names) -> Vec<TilesetInfo> {
    let mut tilesets = Vec::new();
    for (index, used) in map.tilesets.iter().enumerate() {
        let Some(slot) = TilesetSlot::new(index) else {
            continue;
        };
        let tileset = &used.tileset;
        tilesets.push(TilesetInfo {
            name: names
                .tileset_name(slot)
                .map_or_else(|| tileset.name.clone(), str::to_owned),
            collection: tileset.is_collection(),
            tiles: tileset.tile_count(),
            tile_size: tileset.tile_size(),
            columns: tileset.columns(),
            terrains: names
                .terrains()
                .iter()
                .filter(|t| t.slot == slot)
                .map(|t| t.name.clone())
                .collect(),
            drawn_over: drawn_over(map, tileset),
        });
    }
    tilesets
}

/// Where a tile placed in cell X,Y is drawn, when it is not the cell (orthogonal maps): Tiled
/// draws a tile from its cell's bottom-left corner, moved by the tileset's offset.
fn drawn_over(map: &Map, tileset: &Tileset) -> Option<String> {
    if tileset.is_collection()
        || map.orientation != Orientation::Orthogonal
        || map.tile_width == 0
        || map.tile_height == 0
    {
        return None;
    }
    let cell = map.tile_size().as_dvec2();
    let size = match tileset.tile_render_size {
        TileRenderSize::Grid => cell,
        TileRenderSize::Tile => tileset.tile_size().as_dvec2(),
    };
    let offset = tileset.tile_offset.as_dvec2();
    if size == cell && offset == DVec2::ZERO {
        return None;
    }
    let left = offset.x / cell.x;
    let right = (offset.x + size.x) / cell.x;
    let top = 1.0 - (size.y - offset.y) / cell.y;
    let bottom = 1.0 + offset.y / cell.y;
    let at = |axis: &str, value: f64| {
        if value.abs() < 0.005 {
            axis.to_owned()
        } else if value > 0.0 {
            format!("{axis}+{}", number_text(value))
        } else {
            format!("{axis}-{}", number_text(-value))
        }
    };
    Some(format!(
        "a tile in cell X,Y is drawn over {}..{}, {}..{} (cells)",
        at("X", left),
        at("X", right),
        at("Y", top),
        at("Y", bottom)
    ))
}

fn layers_info(map: &Map, names: &Names) -> Vec<LayerInfo> {
    let listed = layers_top_first(map);
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for at in &listed {
        *seen.entry(at.layer.name.as_str()).or_default() += 1;
    }
    listed
        .iter()
        .map(|at| {
            let layer = at.layer;
            LayerInfo {
                id: layer.id.0,
                name: layer.name.clone(),
                depth: at.depth,
                name_shared: seen.get(layer.name.as_str()).is_some_and(|&n| n > 1),
                holds: layer_holds(map, names, layer),
                hidden: !layer.visible,
                locked: layer.locked,
                offset: (layer.offset != DVec2::ZERO).then_some(layer.offset),
                parallax: (layer.parallax != DVec2::ONE).then_some(layer.parallax),
            }
        })
        .collect()
}

/// What a layer holds, in words.
fn layer_holds(map: &Map, names: &Names, layer: &Layer) -> String {
    match &layer.kind {
        LayerKind::Tile(tiles) => {
            let mut kinds = Tally::new();
            // Plain tiles of each tileset: one kind is named, several are counted.
            let mut plain: BTreeMap<TilesetSlot, Tally> = BTreeMap::new();
            // Each tile's words, found once a tile: what it shows, and whether that is a
            // terrain's.
            let mut labels: HashMap<TileKey, TileWords> = HashMap::new();
            let mut count = 0usize;
            for placed in tiles.cells() {
                count += 1;
                let Some(key) = TileKey::of(placed.cell) else {
                    continue;
                };
                let words = labels.entry(key).or_insert_with(|| {
                    let label = names.tile_label(map, key.cell());
                    let terrain = label != names.tile_text(key.cell());
                    TileWords { label, terrain }
                });
                if words.terrain {
                    kinds.add_one(words.label.clone());
                } else {
                    plain
                        .entry(key.slot)
                        .or_default()
                        .add_one(words.label.clone());
                }
            }
            for (slot, tally) in plain {
                let total: usize = tally.sorted().iter().map(|c| c.count).sum();
                match tally.sorted().as_slice() {
                    [one] => kinds.add(one.name.clone(), one.count),
                    _ => {
                        let tileset = names.tileset_name(slot).unwrap_or("tileset");
                        kinds.add(format!("{} kinds of {tileset} tiles", tally.kinds()), total);
                    }
                }
            }
            match tiles.used_bounds() {
                Some(bounds) if count > 0 => format!(
                    "tiles: {} in {}; {}",
                    plural(count, "cell", "cells"),
                    region_text(bounds),
                    counts_text(&kinds.sorted(), KINDS_SHOWN)
                ),
                _ => "tiles: empty".to_owned(),
            }
        }
        LayerKind::Object(objects) => {
            if objects.objects.is_empty() {
                return "objects: none".to_owned();
            }
            let mut kinds = Tally::new();
            for object in objects.objects.iter() {
                kinds.add_one(object_kind(map, names, object));
            }
            format!(
                "objects: {}; {}",
                objects.objects.len(),
                counts_text(&kinds.sorted(), KINDS_SHOWN + 1)
            )
        }
        LayerKind::Image(image) => match &image.image {
            Some(image) => match &image.source {
                bt_core::ImageSource::File(file) => format!("image: {}", file.source),
                bt_core::ImageSource::Embedded(embedded) => {
                    format!("image: embedded {}", embedded.format)
                }
            },
            None => "image: none".to_owned(),
        },
        LayerKind::Group(children) => {
            format!("group of {}", plural(children.len(), "layer", "layers"))
        }
    }
}

/// A tile in words, and whether the words are a terrain's.
struct TileWords {
    label: String,
    terrain: bool,
}

/// What an object is, in a word: its sprite's name, else its type, else its shape.
pub(crate) fn object_kind(map: &Map, names: &Names, object: &bt_core::MapObject) -> String {
    if let Some(sprite) = object.tile.and_then(|tile| names.sprite_of(tile)) {
        return sprite.name.clone();
    }
    let kind = object_type(map, object);
    if !kind.is_empty() {
        return kind.to_owned();
    }
    if let Some(tile) = object.tile {
        return names.tile_text(tile);
    }
    object.kind_name().to_owned()
}

/// The AutoMap part of `bt info`: a rule map's own rules (whatever rules file lists it), else
/// the rules the caller found for the map.
fn rules_info(request: &InfoRequest<'_>) -> Option<RulesInfo> {
    let map = request.map;
    if is_rule_map(map) {
        return Some(rule_map_info(request));
    }
    match &request.rules {
        Some(MapRules::Layers {
            setup,
            rules,
            notes,
            kept_in,
        }) => {
            let mut info = layers_rules_info(map, setup, borrowed(rules), notes);
            info.kept_in = kept_in.map(str::to_owned);
            Some(info)
        }
        Some(MapRules::File { path, rules }) => Some(file_rules_info(map, path, borrowed(rules))),
        None => None,
    }
}

/// The rules, or why they can't be read, borrowed.
fn borrowed<'r>(rules: &'r Result<&'r RuleSet, String>) -> Result<&'r RuleSet, &'r str> {
    match rules {
        Ok(rules) => Ok(rules),
        Err(why) => Err(why.as_str()),
    }
}

/// Whether the map's layers make it a rule map: `input…_`, `output…_`, `regions` or
/// `rule_options` layers.
pub(crate) fn is_rule_map(map: &Map) -> bool {
    let mut found = false;
    map.for_each_layer(|layer, _| {
        let name = layer.name.to_ascii_lowercase();
        let rule_layer = ((name.starts_with("input") || name.starts_with("output"))
            && name.contains('_'))
            || name == "regions"
            || name.starts_with("regions_")
            || name == "rule_options";
        found |= rule_layer && !name.starts_with("//");
    });
    found
}

/// A rule set read for the map: its counts and problems, or why it can't be read.
fn read_rules(kind: RulesKind, rules_file: String, rules: Result<&RuleSet, &str>) -> RulesInfo {
    let mut info = RulesInfo {
        kind,
        rules: rules_file,
        unreadable: None,
        rule_maps: 0,
        rule_count: 0,
        runs: Vec::new(),
        seed: None,
        reads: Vec::new(),
        writes: Vec::new(),
        notes: Vec::new(),
        kept_in: None,
    };
    match rules {
        Ok(rules) => {
            info.rule_maps = rules.rule_maps().len();
            info.rule_count = rules.rule_count();
            info.notes.extend(rules.problems().iter().map(note_of));
            for (index, rule_map) in rules.rule_maps().iter().enumerate() {
                let name = crate::rule_words::rule_map_name(rules, index);
                for problem in rule_map
                    .problems()
                    .iter()
                    .filter(|p| p.severity() == bt_automap::Severity::Error)
                {
                    info.notes.push(RuleNote {
                        kind: LineKind::Warning,
                        text: format!("{name} is not used: {}", problem.text),
                    });
                }
                if rule_map.is_used() && rule_map.rule_count() == 0 {
                    info.notes.push(RuleNote {
                        kind: LineKind::Warning,
                        text: format!("{name} has no rules"),
                    });
                }
            }
        }
        Err(why) => info.unreadable = Some(why.to_owned()),
    }
    info
}

fn note_of(problem: &Problem) -> RuleNote {
    let line = problem_line(problem);
    RuleNote {
        kind: line.kind,
        text: line.text,
    }
}

/// A rules file found for the map: what it reads and writes, and what the map lacks or has
/// empty of what it reads.
fn file_rules_info(map: &Map, path: &Path, rules: Result<&RuleSet, &str>) -> RulesInfo {
    let mut info = read_rules(RulesKind::File, rules_file_name(path), rules);
    let Ok(rules) = rules else {
        return info;
    };
    let layers = rules.rule_layers();
    // Layers an earlier rule map writes are read from what it wrote.
    info.reads = layers
        .inputs
        .iter()
        .filter(|name| !layers.made.contains(name))
        .cloned()
        .collect();
    info.writes = layers
        .tile_outputs
        .iter()
        .cloned()
        .chain(
            layers
                .object_outputs
                .iter()
                .map(|name| format!("{name} (objects)")),
        )
        .collect();
    for copy in rules.tileset_copies(map) {
        info.notes.push(note_of(&copy.problem()));
    }
    info.notes.extend(input_notes(map, rules));
    let missing_outputs: Vec<&String> = layers
        .tile_outputs
        .iter()
        .filter(|name| tile_layer_named(map, name).is_none())
        .collect();
    if !missing_outputs.is_empty() {
        info.notes.push(RuleNote {
            kind: LineKind::Fact,
            text: if missing_outputs.len() == 1 {
                format!(
                    "the map has no layer {}, which the rules write: a run makes it",
                    and_list(&missing_outputs)
                )
            } else {
                format!(
                    "the map has no layers {}, which the rules write: a run makes them",
                    and_list(&missing_outputs)
                )
            },
        });
    }
    info
}

/// What keeps the rules from doing anything on `map`: layers they read that it lacks (with a
/// near miss, since names must match exactly) or has empty, leaving out those an earlier rule
/// map writes.
fn input_notes(map: &Map, rules: &RuleSet) -> Vec<RuleNote> {
    let mut notes = Vec::new();
    for missing in rules.missing_layers(map) {
        let near = near_miss_layer(map, &missing.name).map_or_else(String::new, |near| {
            format!(" (it has {}: names must match exactly)", quoted(&near))
        });
        notes.push(RuleNote {
            kind: LineKind::Warning,
            text: format!(
                "the rules read layer {}, which the map does not have, so it reads as empty{near}",
                quoted(&missing.name)
            ),
        });
    }
    let mut written: Vec<&str> = Vec::new();
    let mut empty: Vec<&str> = Vec::new();
    for rule_map in rules.rule_maps().iter().filter(|r| r.is_used()) {
        for name in rule_map.inputs() {
            if written.contains(&name.as_str()) || empty.contains(&name.as_str()) {
                continue;
            }
            let has_tiles = tile_layer_named(map, name)
                .and_then(Layer::as_tile_layer)
                .map(|tiles| tiles.cells().next().is_some());
            if has_tiles == Some(false) {
                empty.push(name);
            }
        }
        written.extend(rule_map.tile_outputs().iter().map(String::as_str));
    }
    if !empty.is_empty() {
        notes.push(RuleNote {
            kind: LineKind::Fact,
            text: format!(
                "layer {}, which the rules read, {} empty",
                and_list(&empty.iter().map(|n| quoted(n)).collect::<Vec<_>>()),
                if empty.len() == 1 { "is" } else { "are" }
            ),
        });
    }
    notes
}

/// The map's AutoMap layers: their rules, runs and seed, what the rules read and make in the
/// map's layer names, and the layers whose painted state could not be put back.
fn layers_rules_info(
    map: &Map,
    setup: &Setup,
    rules: Result<&RuleSet, &str>,
    notes: &[Problem],
) -> RulesInfo {
    let mut info = read_rules(RulesKind::Layers, rules_file_name(&setup.rules), rules);
    info.seed = Some(setup.seed);
    let layer_name = |id: bt_core::LayerId| {
        map.layer(id)
            .map_or_else(|| format!("#{}", id.0), |layer| layer.name.clone())
    };
    for run in &setup.runs {
        let mut bindings: Vec<String> = run
            .tile_bindings()
            .iter()
            .map(|b| format!("{}={}", b.name, layer_name(b.layer)))
            .collect();
        bindings.extend(
            run.object_bindings()
                .iter()
                .map(|b| format!("{}={} (objects)", b.name, layer_name(b.layer))),
        );
        info.runs.push(if bindings.is_empty() {
            "no layers bound".to_owned()
        } else {
            bindings.join(", ")
        });
    }
    if let Ok(rules) = rules {
        let layers = rules.rule_layers();
        for run in &setup.runs {
            for name in layers
                .inputs
                .iter()
                .filter(|name| !layers.made.contains(name))
            {
                if let Some(id) = run.tile_layer(name) {
                    push_once(&mut info.reads, layer_name(id));
                }
            }
            for name in &layers.tile_outputs {
                if let Some(id) = run.tile_layer(name) {
                    push_once(&mut info.writes, layer_name(id));
                }
            }
            for name in &layers.object_outputs {
                if let Some(id) = run.object_layer(name) {
                    push_once(&mut info.writes, format!("{} (objects)", layer_name(id)));
                }
            }
        }
    }
    info.notes.extend(notes.iter().map(|note| RuleNote {
        kind: LineKind::Warning,
        text: note.text.clone(),
    }));
    info
}

fn push_once(names: &mut Vec<String>, name: String) {
    if !names.contains(&name) {
        names.push(name);
    }
}

/// A rule map's own rules: what they read and write, its problems, and the tiles that make no
/// rule.
fn rule_map_info(request: &InfoRequest<'_>) -> RulesInfo {
    let rules = RuleSet::builder()
        .custom_types(Arc::clone(&request.custom_types))
        .rule_map(PathBuf::from(&request.file), Arc::new(request.map.clone()))
        .build();
    let mut info = RulesInfo {
        kind: RulesKind::RuleMap,
        rules: request.file.clone(),
        unreadable: None,
        rule_maps: 1,
        rule_count: rules.rule_count(),
        runs: Vec::new(),
        seed: None,
        reads: Vec::new(),
        writes: Vec::new(),
        notes: Vec::new(),
        kept_in: None,
    };
    if let Some(rule_map) = rules.rule_maps().first() {
        info.reads = rule_map.inputs().to_vec();
        info.writes = rule_map
            .tile_outputs()
            .iter()
            .cloned()
            .chain(
                rule_map
                    .object_outputs()
                    .iter()
                    .map(|name| format!("{name} (objects)")),
            )
            .collect();
        info.notes.extend(rule_map.problems().iter().map(note_of));
    }
    info
}

impl Outcome for InfoOutcome {
    const SCHEMA: &'static str = "bt.info/1";

    fn report(&self) -> Report {
        let mut report = Report::new();
        let size = match self.infinite {
            Some(painted) => format!("infinite (tiles in {})", region_text(painted)),
            None => format!("{}x{} cells", self.size.x, self.size.y),
        };
        report.add_line(Line::fact(format!(
            "{}: {}, {size} of {}x{} px",
            self.file, self.orientation, self.tile_size.x, self.tile_size.y
        )));
        if let Some(layout) = &self.layout {
            report.add_line(Line::fact(layout.clone()));
        }
        match self.editor {
            Some(EditorCopy::Unsaved) => report.add_line(Line::note(
                "open in the editor, with unsaved changes: this is the editor's copy (bt save MAP \
                 writes it, bt diff MAP lists the changes)",
            )),
            Some(EditorCopy::Saved) => {
                report.add_line(Line::fact("open in the editor, with no unsaved changes"));
            }
            None => {}
        }
        if !self.class.is_empty() {
            report.add_line(Line::fact(format!("class: {}", self.class)));
        }
        if !self.properties.is_empty() {
            let mut section = Section::headed("properties:");
            section.add_lines(
                self.properties
                    .iter()
                    .map(|(key, value)| Line::fact(format!("{}={value}", quoted(key)))),
            );
            report.add_section(section);
        }
        let mut tilesets = Section::headed("tilesets:");
        if self.tilesets.is_empty() {
            tilesets.add_line(Line::fact("none"));
        }
        tilesets.add_lines(self.tilesets.iter().map(|t| Line::fact(tileset_text(t))));
        report.add_section(tilesets);
        let mut layers = Section::headed("layers (top first):");
        if self.layers.is_empty() {
            layers.add_line(Line::fact("none"));
        }
        layers.add_lines(self.layers.iter().map(layer_line));
        report.add_section(layers);
        if let Some(rules) = &self.automap {
            report.add_section(rules_section(rules));
        }
        for warning in &self.warnings {
            report.add_line(Line::warning(warning.clone()));
        }
        report
    }
}

/// `desert: 48 tiles of 32x32 in 8 columns; terrains: desert, brick`.
fn tileset_text(tileset: &TilesetInfo) -> String {
    let mut text = if tileset.collection {
        format!(
            "{}: {} (images of their own)",
            tileset.name,
            plural(tileset.tiles as usize, "sprite", "sprites")
        )
    } else {
        format!(
            "{}: {} of {}x{} px in {}",
            tileset.name,
            plural(tileset.tiles as usize, "tile", "tiles"),
            tileset.tile_size.x,
            tileset.tile_size.y,
            plural(tileset.columns as usize, "column", "columns")
        )
    };
    if !tileset.terrains.is_empty() {
        text.push_str(&format!(
            "; terrains: {}",
            crate::words::capped_list(&tileset.terrains, TERRAINS_SHOWN)
        ));
    }
    if let Some(drawn) = &tileset.drawn_over {
        text.push_str(&format!("; {drawn}"));
    }
    text
}

/// A layer's line, indented under the groups it is in.
fn layer_line(layer: &LayerInfo) -> Line {
    let mut text = "  ".repeat(layer.depth);
    text.push_str(&layer.name);
    if layer.name_shared {
        text.push_str(&format!(" (#{})", layer.id));
    }
    text.push_str(&format!(": {}", layer.holds));
    if layer.hidden {
        text.push_str(" (hidden)");
    }
    if layer.locked {
        text.push_str(" (locked)");
    }
    if let Some(offset) = layer.offset {
        text.push_str(&format!(
            " (offset {},{} px)",
            number_text(offset.x),
            number_text(offset.y)
        ));
    }
    if let Some(parallax) = layer.parallax {
        text.push_str(&format!(
            " (parallax {},{}: it scrolls at that part of the camera's speed)",
            number_text(parallax.x),
            number_text(parallax.y)
        ));
    }
    Line::fact(text)
}

/// The AutoMap section of `bt info`.
fn rules_section(rules: &RulesInfo) -> Section {
    let list = |names: &[String]| {
        if names.is_empty() {
            "nothing".to_owned()
        } else {
            and_list(names)
        }
    };
    let counts = format!(
        "{}, {}",
        plural(rules.rule_maps, "rule map", "rule maps"),
        plural(rules.rule_count, "rule", "rules")
    );
    let heading = match rules.kind {
        RulesKind::Layers => format!(
            "AutoMap layers ({}, {counts}, seed {}): the map is what the rules make of what was \
             painted, and bt edit changes what was painted (bt view MAP --painted shows it){}",
            rules.rules,
            rules.seed.unwrap_or_default(),
            rules
                .kept_in
                .as_ref()
                .map_or_else(String::new, |file| format!(
                    "; {file} keeps what was painted"
                ))
        ),
        RulesKind::File => format!(
            "AutoMap rules: {} ({counts}) read {} and write {}; the edit step automap runs them",
            rules.rules,
            list(&rules.reads),
            list(&rules.writes)
        ),
        RulesKind::RuleMap => format!(
            "AutoMap rule map: {}; on the map the rules run on, they read {} and write {}",
            plural(rules.rule_count, "rule", "rules"),
            list(&rules.reads),
            list(&rules.writes)
        ),
    };
    let mut section = Section::headed(heading);
    if let Some(why) = &rules.unreadable {
        section.add_line(Line::warning(format!("the rules can't be read: {why}")));
    }
    for (number, run) in (1..).zip(&rules.runs) {
        section.add_line(Line::fact(format!("run {number}: {run}")));
    }
    if rules.kind == RulesKind::Layers && rules.unreadable.is_none() {
        section.add_line(Line::fact(format!(
            "the rules read {} and make {}",
            list(&rules.reads),
            list(&rules.writes)
        )));
    }
    section.add_lines(
        rules
            .notes
            .iter()
            .map(|note| Line::new(note.kind, note.text.clone())),
    );
    section
}

#[cfg(test)]
mod tests {
    use crate::report::LineKind;
    use bt_core::{MapObject, Property, Shape, Value, WangSetType};
    use bt_test_data::fixtures::{Atlas, MapBuilder, TerrainSetBuilder, combination_tile};

    use super::*;
    use crate::report::{from_json, to_json};

    /// A 6×4 map of grass with a pond of water, and a layer of props.
    pub(crate) fn pond_map() -> Map {
        let set = TerrainSetBuilder::new(WangSetType::Corner)
            .name("Land")
            .terrains(&["Grass", "Water"])
            .every_combination();
        let atlas = Atlas::builder().name("ground").build_embedded().unwrap();
        let tileset = set.added_to(Tileset::clone(&atlas.tileset));
        let grass = combination_tile(2, [1, 1, 1, 1]).unwrap().0;
        let water = combination_tile(2, [2, 2, 2, 2]).unwrap().0;
        let rows: Vec<String> = (0..4)
            .map(|y| {
                (0..6)
                    .map(|x| {
                        if (2..=3).contains(&x) && (1..=2).contains(&y) {
                            water
                        } else {
                            grass
                        }
                        .to_string()
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        let spawn = MapObject {
            class: "spawn".into(),
            shape: Shape::Point,
            pos: DVec2::new(40.0, 24.0),
            ..MapObject::default()
        };
        let mut map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(6, 4))
            .tileset(Arc::new(tileset))
            .tile_layer("ground", &rows.join("\n"))
            .object_layer("design", vec![spawn])
            .build()
            .unwrap();
        map.properties.insert(
            "intent".into(),
            Property::new(Value::String("a pond".into())),
        );
        map
    }

    #[test]
    fn info_says_what_the_map_is_made_of() {
        let map = pond_map();
        let names = Names::of(&map);
        let request = InfoRequest::new(&map, &names, "pond.tmx")
            .editor(EditorCopy::Unsaved)
            .warnings(vec!["pond.tmx: tile 99 of ground is missing".into()]);
        let outcome = InfoOutcome::from_request(&request);
        assert_eq!(outcome.orientation, "orthogonal");
        assert_eq!(outcome.size, UVec2::new(6, 4));
        assert_eq!(outcome.tilesets.len(), 1);
        assert_eq!(outcome.tilesets[0].terrains, ["grass", "water"]);
        assert_eq!(outcome.layers[0].name, "design");
        assert_eq!(
            outcome.layers[1].holds,
            "tiles: 24 cells in 0,0 6x4; grass ×20, water ×4"
        );
        assert_eq!(outcome.properties["intent"], "\"a pond\"");
        assert!(outcome.automap.is_none());
        assert_eq!(
            outcome.report().text(),
            "pond.tmx: orthogonal, 6x4 cells of 16x16 px\n\
             note: open in the editor, with unsaved changes: this is the editor's copy (bt save MAP writes it, bt diff MAP lists the changes)\n\
             properties:\n  \
             intent=\"a pond\"\n\
             tilesets:\n  \
             ground: 16 tiles of 16x16 px in 4 columns; terrains: grass, water\n\
             layers (top first):\n  \
             design: objects: 1; spawn\n  \
             ground: tiles: 24 cells in 0,0 6x4; grass ×20, water ×4\n\
             warning: pond.tmx: tile 99 of ground is missing\n"
        );
        assert_eq!(
            from_json::<InfoOutcome>(&to_json(&outcome).unwrap()).unwrap(),
            outcome
        );
    }

    /// A rule map whose one rule turns tile 1 of `tiles` on layer `set` into tile 2, and a map
    /// of tile 1 on a layer `ground` beside it.
    fn replace_rules(tiles: &Arc<Tileset>) -> RuleSet {
        let rule_map = MapBuilder::new(UVec2::splat(16), UVec2::new(1, 1))
            .tileset(Arc::clone(tiles))
            .tile_layer("input_set", "1")
            .tile_layer("output_set", "2")
            .build()
            .unwrap();
        RuleSet::builder()
            .rule_map("/rules/replace.tmx", Arc::new(rule_map))
            .build()
    }

    #[test]
    fn the_rules_found_for_a_map_say_what_they_read_and_what_the_map_lacks() {
        let tiles = Atlas::builder()
            .name("ground")
            .build_embedded()
            .unwrap()
            .tileset;
        let rules = replace_rules(&tiles);
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(2, 2))
            .tileset(Arc::clone(&tiles))
            .tile_layer("Set", "1 1\n1 1")
            .build()
            .unwrap();
        let names = Names::of(&map);
        let path = PathBuf::from("/maps/rules.txt");
        let request = InfoRequest::new(&map, &names, "a.tmx").rules(MapRules::File {
            path: &path,
            rules: Ok(&rules),
        });
        let rules_info = InfoOutcome::from_request(&request).automap.unwrap();
        assert_eq!(rules_info.kind, RulesKind::File);
        assert_eq!((rules_info.rule_maps, rules_info.rule_count), (1, 1));
        assert_eq!(rules_info.reads, ["set"]);
        let text = InfoOutcome::from_request(&request).report().text();
        assert!(
            text.contains(
                "AutoMap rules: rules.txt (1 rule map, 1 rule) read set and write set; the edit step automap runs them\n  \
                 warning: the rules read layer set, which the map does not have, so it reads as empty (it has Set: names must match exactly)\n  \
                 the map has no layer set, which the rules write: a run makes it\n"
            ),
            "{text}"
        );
        // Rules that can't be read say why.
        let unreadable = InfoRequest::new(&map, &names, "a.tmx").rules(MapRules::File {
            path: &path,
            rules: Err("the rules file can't be read".into()),
        });
        assert!(
            InfoOutcome::from_request(&unreadable)
                .report()
                .text()
                .contains("  warning: the rules can't be read: the rules file can't be read\n")
        );
    }

    #[test]
    fn automap_layers_say_their_runs_and_what_the_rules_read_and_make() {
        let tiles = Atlas::builder()
            .name("ground")
            .build_embedded()
            .unwrap()
            .tileset;
        let rules = replace_rules(&tiles);
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(2, 2))
            .tileset(Arc::clone(&tiles))
            .tile_layer("ground", "2 2\n2 2")
            .build()
            .unwrap();
        let names = Names::of(&map);
        let setup = Setup {
            rules: PathBuf::from("/maps/rules.txt"),
            runs: vec![bt_automap::LayerNames::temporary().rebound(
                "set",
                false,
                Some(bt_core::LayerId(1)),
            )],
            seed: 7,
        };
        let lost = [Problem::new(
            bt_automap::ProblemKind::PaintedNotRestored,
            "what was painted on ground changed outside better-tiled, so all of it counts as painted",
        )];
        let request = InfoRequest::new(&map, &names, "a.tmx").rules(MapRules::Layers {
            setup: &setup,
            rules: Ok(&rules),
            notes: &lost,
            kept_in: None,
        });
        let outcome = InfoOutcome::from_request(&request);
        let rules_info = outcome.automap.clone().unwrap();
        assert_eq!(rules_info.runs, ["set=ground"]);
        assert_eq!(
            (rules_info.reads.as_slice(), rules_info.writes.as_slice()),
            (&["ground".to_owned()][..], &["ground".to_owned()][..])
        );
        assert!(
            outcome.report().text().ends_with(
                "AutoMap layers (rules.txt, 1 rule map, 1 rule, seed 7): the map is what the rules make of what was painted, and bt edit changes what was painted (bt view MAP --painted shows it)\n  \
                 run 1: set=ground\n  \
                 the rules read ground and make ground\n  \
                 warning: what was painted on ground changed outside better-tiled, so all of it counts as painted\n"
            ),
            "{}",
            outcome.report().text()
        );
        assert_eq!(
            from_json::<InfoOutcome>(&to_json(&outcome).unwrap()).unwrap(),
            outcome
        );
    }

    #[test]
    fn a_rule_map_says_what_its_rules_read_and_write_and_the_tiles_that_make_no_rule() {
        let tiles = Atlas::builder()
            .name("ground")
            .build_embedded()
            .unwrap()
            .tileset;
        // The input tile at 2,0 touches no output tile.
        let rule_map = MapBuilder::new(UVec2::splat(16), UVec2::new(3, 1))
            .tileset(Arc::clone(&tiles))
            .tile_layer("input_set", "1 . 1")
            .tile_layer("output_ground", "2 . .")
            .build()
            .unwrap();
        let names = Names::of(&rule_map);
        let outcome = InfoOutcome::from_request(&InfoRequest::new(&rule_map, &names, "rules.tmx"));
        let rules_info = outcome.automap.unwrap();
        assert_eq!(rules_info.kind, RulesKind::RuleMap);
        assert_eq!(rules_info.rule_count, 1);
        assert_eq!(
            (rules_info.reads.as_slice(), rules_info.writes.as_slice()),
            (&["set".to_owned()][..], &["ground".to_owned()][..])
        );
        assert!(
            rules_info
                .notes
                .iter()
                .any(|n| n.kind == LineKind::Note && n.text.contains("make no rule")),
            "{:?}",
            rules_info.notes
        );
        // The rules.txt beside a rule map lists it: the rule map is told by its own rules.
        let beside = replace_rules(&tiles);
        let path = PathBuf::from("/rules/rules.txt");
        let listed = InfoOutcome::from_request(
            &InfoRequest::new(&rule_map, &names, "rules.tmx").rules(MapRules::File {
                path: &path,
                rules: Ok(&beside),
            }),
        );
        assert_eq!(listed.automap.unwrap().kind, RulesKind::RuleMap);
    }

    #[test]
    fn tileds_examples_read_sensibly() {
        let Some(examples) = bt_test_data::test_data("reference/tiled/examples") else {
            return;
        };
        let mut loader = bt_tmx::Loader::default();
        let mut info = |file: &str| {
            let loaded = loader.load_map(&examples.join(file)).unwrap();
            let names = Names::of(&loaded.value);
            let warnings = loaded.warnings.iter().map(ToString::to_string).collect();
            InfoOutcome::from_request(
                &InfoRequest::new(&loaded.value, &names, file).warnings(warnings),
            )
        };
        let desert = info("desert.tmx");
        assert_eq!(
            desert.report().text().lines().next(),
            Some("desert.tmx: orthogonal, 40x40 cells of 32x32 px")
        );
        assert_eq!(
            desert.tilesets[0].terrains,
            ["desert", "brick", "cobblestone", "dirt"]
        );
        assert!(
            desert.layers[0]
                .holds
                .starts_with("tiles: 1600 cells in 0,0 40x40; desert ×"),
            "{}",
            desert.layers[0].holds
        );
        let sewers = info("sewers.tmx");
        assert_eq!(
            sewers
                .layers
                .iter()
                .map(|l| l.name.as_str())
                .collect::<Vec<_>>(),
            ["Top", "Bottom"]
        );
        let iso = info("isometric_grass_and_water.tmx");
        assert!(iso.layout.unwrap().contains("x=24 (lower right)"));
        let hex = info("hexagonal-mini.tmx");
        assert_eq!(hex.orientation, "hexagonal");
        assert!(
            hex.layout.unwrap().starts_with(
                "hexagons with sides of 6 px: odd rows sit half a hexagon to the right"
            )
        );
        let knight = info("sticker-knight/map/sandbox.tmx");
        assert_eq!(knight.tilesets[0].tiles, 62);
        assert!(knight.tilesets[0].collection);
        let clouds = knight
            .layers
            .iter()
            .find(|l| l.name == "parallax clouds")
            .unwrap();
        assert_eq!(
            (clouds.holds.as_str(), clouds.parallax),
            ("objects: 5; cloud ×5", Some(DVec2::splat(0.5)))
        );
        let rule_map = info("sewer_automap/rule_003.tmx");
        assert!(rule_map.report().text().contains("AutoMap rule map: 4 rules; on the map the rules run on, they read set and write Ground and Over\n"));
        assert!(
            !rule_map.warnings.is_empty(),
            "its DOCTYPE is a reading warning"
        );
    }

    #[test]
    fn grids_that_are_not_square_say_how_they_sit_on_screen() {
        let iso = MapBuilder::new(UVec2::new(64, 32), UVec2::new(10, 8))
            .orientation(Orientation::Isometric)
            .build()
            .unwrap();
        assert_eq!(
            layout_text(&iso).unwrap(),
            "on screen the grid is a diamond: 0,0 is its top corner, x runs down-right and y \
             down-left; its edges are y=0 (upper right), x=9 (lower right), y=7 (lower left) and \
             x=0 (upper left)"
        );
        let hex = MapBuilder::new(UVec2::new(14, 12), UVec2::new(4, 4))
            .orientation(Orientation::Hexagonal)
            .hex_side_length(6)
            .stagger(StaggerAxis::Y, StaggerIndex::Odd)
            .build()
            .unwrap();
        assert!(
            layout_text(&hex)
                .unwrap()
                .starts_with("hexagons with sides of 6 px: odd rows sit half a hexagon to the right; each touches 6 others")
        );
        assert_eq!(
            layout_text(
                &MapBuilder::new(UVec2::splat(8), UVec2::ONE)
                    .build()
                    .unwrap()
            ),
            None
        );
    }
}
