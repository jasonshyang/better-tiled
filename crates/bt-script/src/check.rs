//! `bt check` (`10-agents.md`, §11): problems, then facts. The problems: the map's reading
//! warnings; terrain seams per layer; objects standing outside the map; sprites on the same spot;
//! objects standing on nothing when most of their layer stands on something; objects whose type
//! the project does not define or with properties their class lacks; project-file entries whose
//! map is gone. Then what objects stand on and what zones cover, layer by layer, the few on other
//! ground than the rest listed by id. `check` states what is there; what to do about it is the
//! guide's. [`made_by`] is the same checks on a map before and after an edit, for its report.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use bt_core::{
    Cell, ClassUsage, CustomTypes, LayerId, LayerKind, Map, MapObject, Shape, TilesetSlot,
    WangSetType,
};
use bt_terrain::{Terrain, Topology, kind_lanes};
use glam::{DVec2, IVec2};
use serde::{Deserialize, Serialize};

use crate::feet::{Feet, standing_cell};
use crate::ground::GroundUnder;
use crate::names::{Names, object_type};
use crate::report::{Line, Outcome, Report, Section};
use crate::shapes::{map_cells, object_cells};
use crate::words::{capped_list, point_text};

/// How close two sprites' feet stand to count as on the same spot, in cells.
const SAME_SPOT: f64 = 0.4;

/// How many cells or objects a problem line lists before "and N more".
const LISTED: usize = 12;

/// The odd ones of a group are listed by id when they are at most a fifth of it and this many.
const MOST_ODD: usize = 6;

/// What `bt check` found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CheckOutcome {
    /// The map, as the command named it.
    pub map: String,
    /// The problems, in the order the module's doc lists their kinds.
    pub problems: Vec<Problem>,
    /// What objects stand on and what zones cover, layer by layer.
    pub layers: Vec<LayerGrounds>,
}

/// A problem `check` points out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "problem", rename_all = "snake_case")]
pub enum Problem {
    /// Reading the map warned.
    Reading {
        /// The warning.
        text: String,
    },
    /// Terrain tiles that do not fit the tile next to them, or end next to an empty cell when
    /// the set has edge tiles.
    Seams {
        /// The layer.
        layer: String,
        /// Each seam: a tile and the neighbour it does not fit.
        seams: Vec<Seam>,
    },
    /// An object stands outside the map.
    OutsideMap {
        /// The layer.
        layer: String,
        /// The object.
        object: ObjectAt,
    },
    /// Two sprites stand on the same spot.
    SameSpot {
        /// The layer.
        layer: String,
        /// The sprite standing there first.
        first: u32,
        /// The other.
        second: u32,
        /// Where.
        at: DVec2,
    },
    /// Objects stand on nothing while most of their layer stands on something.
    OverNothing {
        /// The layer.
        layer: String,
        /// The objects.
        objects: Vec<ObjectAt>,
    },
    /// An object's type is not one the project defines.
    UndefinedType {
        /// The layer.
        layer: String,
        /// The object.
        object: u32,
        /// Its type.
        kind: String,
        /// The types the project defines for objects.
        defined: Vec<String>,
    },
    /// An object has a property its class lacks.
    NotAMember {
        /// The layer.
        layer: String,
        /// The object.
        object: u32,
        /// Its class.
        class: String,
        /// The property.
        property: String,
    },
    /// A sprite's image could not be read, so where it stands is not known.
    NoFoot {
        /// The layer.
        layer: String,
        /// The object.
        object: u32,
        /// Why.
        reason: String,
    },
    /// Project-file entries name maps that are gone.
    Orphans {
        /// The project file.
        file: PathBuf,
        /// The entries' keys.
        keys: Vec<String>,
    },
}

/// A terrain tile and a neighbour it does not fit.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Seam {
    /// The tile.
    pub cell: IVec2,
    /// The neighbour.
    pub next: IVec2,
}

/// An object, named and placed for a problem line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectAt {
    /// Its id.
    pub id: u32,
    /// What it is: a sprite's name, a type, or a shape.
    pub kind: String,
    /// Where it stands, in tile units.
    pub at: DVec2,
}

/// What a layer's objects stand on and its zones cover.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerGrounds {
    /// The layer.
    pub layer: String,
    /// The sprites, and the points of each type, by what they stand on.
    pub groups: Vec<StandingGroup>,
    /// The zones (typed shapes) and what they cover.
    pub zones: Vec<ZoneCover>,
}

/// Objects of one kind on one layer, by what they stand on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StandingGroup {
    /// `sprites`, or a type of points.
    pub what: String,
    /// The grounds, the most stood on first.
    pub grounds: Vec<GroundCount>,
}

/// How many of a group stand on a ground (or how many cells of a zone a ground is).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroundCount {
    /// The ground: a terrain, `tiles of LAYER`, or `nothing`.
    pub ground: String,
    /// How many.
    pub count: usize,
    /// The objects, listed when they are the odd ones of their group.
    pub odd: Vec<u32>,
}

/// A zone and what it covers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ZoneCover {
    /// The zone's id.
    pub object: u32,
    /// What it is.
    pub kind: String,
    /// Its cells by ground, the most first.
    pub covers: Vec<GroundCount>,
}

impl CheckOutcome {
    /// Checks `map` (named `name`), with the reading `warnings`, the project's custom types and
    /// the project file holding the map's entry, if any.
    pub fn of(
        map: &Map,
        name: impl Into<String>,
        warnings: &[String],
        types: &CustomTypes,
        project_file: Option<&Path>,
    ) -> CheckOutcome {
        let names = Names::of(map);
        let mut problems: Vec<Problem> = warnings
            .iter()
            .map(|text| Problem::Reading { text: text.clone() })
            .collect();
        problems.extend(seams(map));
        let objects = ObjectChecks::run(map, &names, types);
        problems.extend(objects.problems);
        if let Some(file) = project_file
            && let Ok(document) = bt_project::ProjectFile::load(file)
        {
            let keys: Vec<String> = document
                .orphans(file)
                .into_iter()
                .map(str::to_owned)
                .collect();
            if !keys.is_empty() {
                problems.push(Problem::Orphans {
                    file: file.to_path_buf(),
                    keys,
                });
            }
        }
        CheckOutcome {
            map: name.into(),
            problems,
            layers: objects.layers,
        }
    }

    /// Whether `check` found a problem (it exits 5).
    pub fn has_problems(&self) -> bool {
        !self.problems.is_empty()
    }
}

impl Problem {
    /// The problem in words: where, then what.
    pub fn text(&self) -> String {
        match self {
            Problem::Reading { text } => format!("reading the map: {text}"),
            Problem::Seams { layer, seams } => {
                let shown: Vec<String> = seams
                    .iter()
                    .take(LISTED)
                    .map(|s| format!("{},{}|{},{}", s.cell.x, s.cell.y, s.next.x, s.next.y))
                    .collect();
                let more = seams.len().saturating_sub(LISTED);
                format!(
                    "{layer}: {} terrain {} (tiles that don't fit the tile next to them): {}{}",
                    seams.len(),
                    if seams.len() == 1 { "seam" } else { "seams" },
                    shown.join(" "),
                    if more > 0 {
                        format!(" and {more} more")
                    } else {
                        String::new()
                    }
                )
            }
            Problem::OutsideMap { layer, object } => format!(
                "{layer}: #{} {} stands outside the map, at {}",
                object.id,
                object.kind,
                point_text(object.at)
            ),
            Problem::SameSpot {
                layer,
                first,
                second,
                at,
            } => format!(
                "{layer}: #{first} and #{second} stand on the same spot ({})",
                point_text(*at)
            ),
            Problem::OverNothing { layer, objects } => {
                let listed: Vec<String> = objects
                    .iter()
                    .map(|o| format!("#{} {} at {}", o.id, o.kind, point_text(o.at)))
                    .collect();
                let (count, verb, them) = if objects.len() == 1 {
                    ("1 object".to_owned(), "stands", "it")
                } else {
                    (format!("{} objects", objects.len()), "stand", "them")
                };
                format!(
                    "{layer}: {count} {verb} on nothing (no ground under {them}): {}",
                    capped_list(&listed, 8)
                )
            }
            Problem::UndefinedType {
                layer,
                object,
                kind,
                defined,
            } => format!(
                "{layer}: #{object} has the type {kind}, which the project doesn't define (it has \
                 {})",
                capped_list(defined, 12)
            ),
            Problem::NotAMember {
                layer,
                object,
                class,
                property,
            } => format!(
                "{layer}: #{object} {class} has {property}, which isn't a member of {class}"
            ),
            Problem::NoFoot {
                layer,
                object,
                reason,
            } => format!("{layer}: #{object}: where it stands isn't known: {reason}"),
            Problem::Orphans { file, keys } => format!(
                "{}: {} {} maps that are gone (moved or deleted outside the editor): {}",
                file.display(),
                if keys.len() == 1 {
                    "an entry names"
                } else {
                    "entries name"
                },
                if keys.len() == 1 { "a map" } else { "the" },
                capped_list(keys, LISTED)
            ),
        }
    }
}

impl Outcome for CheckOutcome {
    const SCHEMA: &'static str = "bt.check/1";

    fn report(&self) -> Report {
        let mut report = Report::new();
        let mut problems = Section::headed(format!(
            "{}: {}",
            self.map,
            match self.problems.len() {
                0 => "no problems found".to_owned(),
                1 => "1 problem".to_owned(),
                n => format!("{n} problems"),
            }
        ))
        .limited(60);
        problems.add_lines(self.problems.iter().map(|p| Line::problem(p.text())));
        report.add_section(problems);
        let mut grounds = Section::headed(
            "what objects stand on, and what zones cover (odd ones out listed by id):",
        );
        for layer in &self.layers {
            for group in &layer.groups {
                grounds.add_line(Line::fact(format!(
                    "{} {}: {}",
                    layer.layer,
                    group.what,
                    counts_text(&group.grounds)
                )));
            }
            for zone in &layer.zones {
                grounds.add_line(Line::fact(format!(
                    "{} {} #{} covers {}",
                    layer.layer,
                    zone.kind,
                    zone.object,
                    if zone.covers.is_empty() {
                        "no cells".to_owned()
                    } else {
                        counts_text(&zone.covers)
                    }
                )));
            }
        }
        if !grounds.lines().is_empty() {
            report.add_section(grounds);
        }
        report
    }
}

/// `water ×12 (#4 #9), grass ×3`.
fn counts_text(counts: &[GroundCount]) -> String {
    counts
        .iter()
        .map(|count| {
            let ids = if count.odd.is_empty() {
                String::new()
            } else {
                let ids: Vec<String> = count.odd.iter().map(|id| format!("#{id}")).collect();
                format!(" ({})", ids.join(" "))
            };
            format!("{} ×{}{ids}", count.ground, count.count)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Terrain seams, layer by layer: a tile whose lanes disagree with the tile across its right or
/// bottom edge (the neighbours the terrain fill sees on every grid), or a terrain ending next to
/// an empty cell when its set has edge tiles.
pub fn seams(map: &Map) -> Vec<Problem> {
    let topology = Topology::of(map);
    let bounds = map_cells(map);
    let mut sets: HashMap<SetKey, Option<PreparedSet>> = HashMap::new();
    let mut problems = Vec::new();
    map.for_each_layer(|layer, _| {
        let Some(tiles) = layer.as_tile_layer() else {
            return;
        };
        let mut cells: Vec<IVec2> = tiles.cells().map(|placed| placed.at).collect();
        cells.sort_by_key(|c| (c.y, c.x));
        let mut found = Vec::new();
        for cell in cells {
            let tile = tiles.cell(cell);
            let Some(key) = SetKey::of(map, tile) else {
                continue;
            };
            let Some(set) = sets
                .entry(key)
                .or_insert_with(|| PreparedSet::of(map, key))
                .as_ref()
            else {
                continue;
            };
            let here = set.terrain.wang_of_cell(tile).lanes();
            let around = topology.around(cell);
            // The right and bottom edges: each pair of cells once.
            for edge in [2usize, 4] {
                let Some(&next) = around.get(edge) else {
                    continue;
                };
                if !map.infinite && !bounds.contains(next) {
                    continue;
                }
                let neighbour = tiles.cell(next);
                let relevant = |lane: usize| kind_lanes(set.kind).contains(&lane);
                let facing = facing_lanes(edge);
                let bad = if set.holds(map, neighbour) {
                    let there = set.terrain.wang_of_cell(neighbour).lanes();
                    facing.iter().any(|pair| {
                        relevant(pair.here) && here.get(pair.here) != there.get(pair.there)
                    })
                } else {
                    neighbour.is_empty()
                        && set.has_edge_tiles
                        && facing
                            .iter()
                            .any(|pair| relevant(pair.here) && here.get(pair.here) != Some(&0))
                };
                if bad {
                    found.push(Seam { cell, next });
                }
            }
        }
        if !found.is_empty() {
            problems.push(Problem::Seams {
                layer: layer.name.clone(),
                seams: found,
            });
        }
    });
    problems
}

/// A lane of a tile and the lane of its neighbour that must hold the same terrain.
struct LanePair {
    here: usize,
    there: usize,
}

/// The lanes facing across edge lane `edge`: the edge itself and its two corners.
fn facing_lanes(edge: usize) -> [LanePair; 3] {
    [
        LanePair {
            here: edge,
            there: (edge + 4) % 8,
        },
        LanePair {
            here: (edge + 7) % 8,
            there: (edge + 5) % 8,
        },
        LanePair {
            here: (edge + 1) % 8,
            there: (edge + 3) % 8,
        },
    ]
}

/// A terrain set of a map's tileset.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct SetKey {
    slot: TilesetSlot,
    set: usize,
}

impl SetKey {
    /// The first set of its tileset that labels `cell`'s tile.
    fn of(map: &Map, cell: Cell) -> Option<SetKey> {
        let slot = cell.slot()?;
        let tileset = &map.tilesets.get(slot.index())?.tileset;
        let set = tileset
            .wang_sets
            .iter()
            .position(|set| set.tiles.contains_key(&cell.tile()))?;
        Some(SetKey { slot, set })
    }
}

/// A terrain set prepared once for the seam check.
struct PreparedSet {
    key: SetKey,
    terrain: Terrain,
    kind: WangSetType,
    has_edge_tiles: bool,
}

impl PreparedSet {
    fn of(map: &Map, key: SetKey) -> Option<PreparedSet> {
        let set = map
            .tilesets
            .get(key.slot.index())?
            .tileset
            .wang_sets
            .get(key.set)?;
        let lanes = kind_lanes(set.kind);
        let has_edge_tiles = set
            .tiles
            .values()
            .any(|id| lanes.iter().any(|&lane| id.lanes().get(lane) == Some(&0)));
        Some(PreparedSet {
            key,
            terrain: Terrain::new(map, key.slot, key.set).ok()?,
            kind: set.kind,
            has_edge_tiles,
        })
    }

    /// Whether `cell` is a tile of this set.
    fn holds(&self, map: &Map, cell: Cell) -> bool {
        cell.slot() == Some(self.key.slot)
            && map
                .tilesets
                .get(self.key.slot.index())
                .and_then(|t| t.tileset.wang_sets.get(self.key.set))
                .is_some_and(|set| set.tiles.contains_key(&cell.tile()))
    }
}

/// The object checks, run once over every object layer.
struct ObjectChecks {
    problems: Vec<Problem>,
    layers: Vec<LayerGrounds>,
}

impl ObjectChecks {
    fn run(map: &Map, names: &Names, types: &CustomTypes) -> ObjectChecks {
        let mut feet = Feet::new();
        let mut ground = GroundUnder::new(map, names);
        let bounds = map_cells(map);
        let object_types: Vec<String> = types
            .types
            .iter()
            .filter(|t| {
                t.as_class()
                    .is_some_and(|c| c.use_as.intersects(ClassUsage::OBJECT))
            })
            .map(|t| t.name.clone())
            .collect();
        let mut checks = ObjectChecks {
            problems: Vec::new(),
            layers: Vec::new(),
        };
        let mut layers: Vec<(LayerId, String, Vec<MapObject>)> = Vec::new();
        map.for_each_layer(|layer, _| {
            if let LayerKind::Object(objects) = &layer.kind {
                layers.push((layer.id, layer.name.clone(), objects.objects.to_vec()));
            }
        });
        for (layer_id, layer, objects) in layers {
            let mut feet_here: Vec<FootAt> = Vec::new();
            let mut groups: BTreeMap<String, BTreeMap<String, Vec<u32>>> = BTreeMap::new();
            let mut floating: Vec<ObjectAt> = Vec::new();
            let mut grounded = 0usize;
            let mut zones = Vec::new();
            for object in &objects {
                let kind = object_kind(map, names, object);
                let at = match feet.standing_point(map, object) {
                    Ok(at) => at,
                    Err(error) => {
                        checks.problems.push(Problem::NoFoot {
                            layer: layer.clone(),
                            object: object.id.0,
                            reason: error.to_string(),
                        });
                        continue;
                    }
                };
                let cell = standing_cell(at);
                if !map.infinite && !bounds.contains(cell) {
                    checks.problems.push(Problem::OutsideMap {
                        layer: layer.clone(),
                        object: ObjectAt {
                            id: object.id.0,
                            kind,
                            at,
                        },
                    });
                    continue;
                }
                let is_point = matches!(object.shape, Shape::Point);
                let is_text = matches!(object.shape, Shape::Text(_));
                if object.tile.is_none() && !is_point && !is_text {
                    zones.push(zone_cover(
                        map,
                        &mut ground,
                        &mut feet,
                        layer_id,
                        object,
                        kind.clone(),
                    ));
                }
                if object.tile.is_some() || is_point {
                    let group = if object.tile.is_some() {
                        "sprites".to_owned()
                    } else {
                        object_type(map, object).to_owned()
                    };
                    let stands = ground.stands_on(layer_id, cell).text();
                    if stands == "nothing" {
                        floating.push(ObjectAt {
                            id: object.id.0,
                            kind: kind.clone(),
                            at,
                        });
                    } else {
                        grounded += 1;
                    }
                    groups
                        .entry(group)
                        .or_default()
                        .entry(stands)
                        .or_default()
                        .push(object.id.0);
                }
                if object.tile.is_some() {
                    if let Some(other) = feet_here.iter().find(|f| f.at.distance(at) < SAME_SPOT) {
                        checks.problems.push(Problem::SameSpot {
                            layer: layer.clone(),
                            first: other.id,
                            second: object.id.0,
                            at,
                        });
                    }
                    feet_here.push(FootAt {
                        id: object.id.0,
                        at,
                    });
                }
                checks.check_type(&layer, object, types, &object_types);
            }
            // Most of the layer stands on something: the ones over nothing are out of place (a
            // layer of clouds, all over nothing, is fine).
            if !floating.is_empty() && grounded > floating.len() {
                checks.problems.push(Problem::OverNothing {
                    layer: layer.clone(),
                    objects: floating,
                });
            }
            let groups = groups
                .into_iter()
                .map(|(what, grounds)| StandingGroup {
                    what,
                    grounds: grounds_counted(grounds),
                })
                .collect::<Vec<_>>();
            if !groups.is_empty() || !zones.is_empty() {
                checks.layers.push(LayerGrounds {
                    layer,
                    groups,
                    zones,
                });
            }
        }
        checks
    }

    /// An object's type the project does not define, or properties its class lacks.
    fn check_type(
        &mut self,
        layer: &str,
        object: &MapObject,
        types: &CustomTypes,
        object_types: &[String],
    ) {
        if object_types.is_empty() || object.tile.is_some() || object.class.is_empty() {
            return;
        }
        match types.class_for(&object.class, ClassUsage::OBJECT) {
            None => self.problems.push(Problem::UndefinedType {
                layer: layer.to_owned(),
                object: object.id.0,
                kind: object.class.clone(),
                defined: object_types.to_vec(),
            }),
            Some(class) => {
                for key in object
                    .properties
                    .keys()
                    .filter(|key| !class.members.contains_key(*key))
                {
                    self.problems.push(Problem::NotAMember {
                        layer: layer.to_owned(),
                        object: object.id.0,
                        class: object.class.clone(),
                        property: key.clone(),
                    });
                }
            }
        }
    }
}

/// A sprite's foot, for the same-spot check.
struct FootAt {
    id: u32,
    at: DVec2,
}

/// The grounds a group stands on, the most first, the odd ones listed by id.
fn grounds_counted(grounds: BTreeMap<String, Vec<u32>>) -> Vec<GroundCount> {
    let total: usize = grounds.values().map(Vec::len).sum();
    let mut counts: Vec<GroundCount> = grounds
        .into_iter()
        .map(|(ground, ids)| {
            let odd = ids.len() * 5 <= total && ids.len() <= MOST_ODD;
            GroundCount {
                ground,
                count: ids.len(),
                odd: if odd { ids } else { Vec::new() },
            }
        })
        .collect();
    counts.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.ground.cmp(&b.ground)));
    counts
}

/// What a zone's cells are made of.
fn zone_cover(
    map: &Map,
    ground: &mut GroundUnder<'_>,
    feet: &mut Feet,
    layer: LayerId,
    object: &MapObject,
    kind: String,
) -> ZoneCover {
    let mut covers: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for cell in object_cells(map, object, feet).unwrap_or_default() {
        covers
            .entry(ground.stands_on(layer, cell).text())
            .or_default()
            .push(object.id.0);
    }
    let mut counts: Vec<GroundCount> = covers
        .into_iter()
        .map(|(ground, cells)| GroundCount {
            ground,
            count: cells.len(),
            odd: Vec::new(),
        })
        .collect();
    counts.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.ground.cmp(&b.ground)));
    ZoneCover {
        object: object.id.0,
        kind,
        covers: counts,
    }
}

/// What an object is, in words: a sprite by its name, else its type, else its shape.
fn object_kind(map: &Map, names: &Names, object: &MapObject) -> String {
    if let Some(tile) = object.tile {
        return names
            .sprite_of(tile)
            .map_or_else(|| names.tile_text(tile), |sprite| sprite.name.clone());
    }
    let kind = object_type(map, object);
    if !kind.is_empty() {
        return kind.to_owned();
    }
    match object.shape {
        Shape::Rectangle => "rectangle",
        Shape::Ellipse => "ellipse",
        Shape::Point => "point",
        Shape::Polygon(_) => "polygon",
        Shape::Polyline(_) => "path",
        Shape::Text(_) => "text",
        _ => "object",
    }
    .to_owned()
}

/// What an edit did that `check` would point out (`10-agents.md`, §11): the seams the map did
/// not have before, and the objects standing in changed cells whose ground changed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MadeProblems {
    /// Problems the map has now and did not have before.
    pub problems: Vec<Problem>,
    /// Objects that stood on one ground and stand on another now.
    pub moved: Vec<GroundChange>,
}

/// Objects whose ground changed from one to another.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroundChange {
    /// What they stood on.
    pub from: String,
    /// What they stand on now.
    pub to: String,
    /// The objects.
    pub objects: Vec<ObjectAt>,
}

impl MadeProblems {
    /// Whether the edit made nothing `check` would point out.
    pub fn is_empty(&self) -> bool {
        self.problems.is_empty() && self.moved.is_empty()
    }

    /// The problems as lines of a report: problems, then the objects whose ground changed.
    pub fn lines(&self) -> Vec<Line> {
        let mut lines: Vec<Line> = self
            .problems
            .iter()
            .map(|problem| Line::problem(format!("new: {}", problem.text())))
            .collect();
        for change in &self.moved {
            let listed: Vec<String> = change
                .objects
                .iter()
                .map(|o| format!("#{} {}", o.id, o.kind))
                .collect();
            let (count, verb) = if change.objects.len() == 1 {
                ("1 object that stood on".to_owned(), "stands")
            } else {
                (
                    format!("{} objects that stood on", change.objects.len()),
                    "stand",
                )
            };
            lines.push(Line::problem(format!(
                "{count} {} {verb} on {} now: {}",
                change.from,
                change.to,
                capped_list(&listed, 8)
            )));
        }
        lines
    }
}

/// [`MadeProblems`] of an edit from `before` to `after`, which changed `changed` cells.
pub fn made_by(before: &Map, after: &Map, changed: &[IVec2]) -> MadeProblems {
    let old: HashSet<Seam> = seams(before)
        .into_iter()
        .flat_map(|problem| match problem {
            Problem::Seams { seams, .. } => seams,
            _ => Vec::new(),
        })
        .collect();
    let problems: Vec<Problem> = seams(after)
        .into_iter()
        .filter_map(|problem| match problem {
            Problem::Seams { layer, seams } => {
                let new: Vec<Seam> = seams.into_iter().filter(|s| !old.contains(s)).collect();
                (!new.is_empty()).then_some(Problem::Seams { layer, seams: new })
            }
            other => Some(other),
        })
        .collect();
    let changed: HashSet<IVec2> = changed.iter().copied().collect();
    let (old_names, names) = (Names::of(before), Names::of(after));
    let mut old_ground = GroundUnder::new(before, &old_names);
    let mut ground = GroundUnder::new(after, &names);
    let mut feet = Feet::new();
    let mut was: BTreeMap<u32, (LayerId, MapObject)> = BTreeMap::new();
    before.for_each_layer(|layer, _| {
        if let LayerKind::Object(objects) = &layer.kind {
            for object in objects.objects.iter() {
                was.insert(object.id.0, (layer.id, object.clone()));
            }
        }
    });
    let mut moved: BTreeMap<(String, String), Vec<ObjectAt>> = BTreeMap::new();
    let mut now: Vec<(LayerId, MapObject)> = Vec::new();
    after.for_each_layer(|layer, _| {
        if let LayerKind::Object(objects) = &layer.kind {
            now.extend(objects.objects.iter().map(|o| (layer.id, o.clone())));
        }
    });
    for (layer, object) in now {
        if object.tile.is_none() && !matches!(object.shape, Shape::Point) {
            continue;
        }
        let Some((old_layer, old_object)) = was.get(&object.id.0) else {
            continue;
        };
        let (Ok(at), Ok(old_at)) = (
            feet.standing_point(after, &object),
            feet.standing_point(before, old_object),
        ) else {
            continue;
        };
        let cell = standing_cell(at);
        if !changed.contains(&cell) {
            continue;
        }
        let from = old_ground
            .stands_on(*old_layer, standing_cell(old_at))
            .text();
        let to = ground.stands_on(layer, cell).text();
        if from != to {
            moved.entry((from, to)).or_default().push(ObjectAt {
                id: object.id.0,
                kind: object_kind(after, &names, &object),
                at,
            });
        }
    }
    MadeProblems {
        problems,
        moved: moved
            .into_iter()
            .map(|((from, to), objects)| GroundChange { from, to, objects })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{ClassType, CustomType, ObjectId, Property, TileId, Tileset, TilesetSlot, Value};
    use bt_test_data::fixtures::{Atlas, MapBuilder, TerrainSetBuilder, combination_tile};
    use glam::UVec2;

    use super::*;

    /// A 6×4 map of grass with water on the corners `water` says, each tile with its corners'
    /// terrains, the ground layer empty where `bare` says, and `objects` on a layer above.
    fn terrain_map(
        water: impl Fn(i32, i32) -> bool,
        bare: impl Fn(i32, i32) -> bool,
        objects: Vec<MapObject>,
    ) -> Map {
        let set = TerrainSetBuilder::new(WangSetType::Corner)
            .name("Land")
            .terrains(&["Grass", "Water"])
            .every_combination();
        let atlas = Atlas::builder().name("ground").build_embedded().unwrap();
        let tileset = set.added_to(Tileset::clone(&atlas.tileset));
        let corner = |x: i32, y: i32| if water(x, y) { 2 } else { 1 };
        let rows: Vec<String> = (0..4)
            .map(|y| {
                (0..6)
                    .map(|x| {
                        if bare(x, y) {
                            return ".".to_owned();
                        }
                        combination_tile(
                            2,
                            [
                                corner(x + 1, y),
                                corner(x + 1, y + 1),
                                corner(x, y + 1),
                                corner(x, y),
                            ],
                        )
                        .unwrap()
                        .0
                        .to_string()
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        MapBuilder::new(UVec2::new(16, 16), UVec2::new(6, 4))
            .tileset(Arc::new(tileset))
            .tile_layer("ground", &rows.join("\n"))
            .object_layer("things", objects)
            .build()
            .unwrap()
    }

    fn point(id: u32, class: &str, x: f64, y: f64) -> MapObject {
        MapObject {
            id: ObjectId(id),
            class: class.into(),
            shape: Shape::Point,
            pos: DVec2::new(x * 16.0, y * 16.0),
            ..MapObject::default()
        }
    }

    fn check(map: &Map, types: &CustomTypes) -> CheckOutcome {
        CheckOutcome::of(map, "a.tmx", &[], types, None)
    }

    fn no(_: i32, _: i32) -> bool {
        false
    }

    #[test]
    fn tiles_painted_by_their_corners_have_no_seams_and_a_wrong_tile_makes_them() {
        let pond = |x: i32, y: i32| (2..=3).contains(&x) && (1..=2).contains(&y);
        let map = terrain_map(pond, no, Vec::new());
        assert!(seams(&map).is_empty());

        // An all-water tile in the grass at 4,3: it fits none of the four tiles around it.
        let mut wrong = map.clone();
        let water = combination_tile(2, [2, 2, 2, 2]).unwrap();
        wrong
            .layer_mut(bt_core::LayerId(1))
            .and_then(|layer| layer.as_tile_layer_mut())
            .unwrap()
            .set_cell(
                IVec2::new(4, 3),
                Cell::new(TilesetSlot::new(0).unwrap(), TileId(water.0)),
            )
            .unwrap();
        let found = seams(&wrong);
        let [Problem::Seams { layer, seams }] = found.as_slice() else {
            panic!("one layer of seams: {found:?}")
        };
        assert_eq!(layer, "ground");
        assert!(seams.contains(&Seam {
            cell: IVec2::new(3, 3),
            next: IVec2::new(4, 3)
        }));
        assert!(seams.contains(&Seam {
            cell: IVec2::new(4, 3),
            next: IVec2::new(5, 3)
        }));
        assert!(seams.contains(&Seam {
            cell: IVec2::new(4, 2),
            next: IVec2::new(4, 3)
        }));
        let outcome = check(&wrong, &CustomTypes::default());
        assert!(outcome.has_problems());
        assert!(
            outcome
                .report()
                .text()
                .starts_with("a.tmx: 1 problem\n  problem: ground:")
        );
    }

    #[test]
    fn objects_outside_the_map_or_over_nothing_among_grounded_ones_are_problems() {
        let objects = vec![
            point(1, "tree", 0.5, 1.5),
            point(2, "tree", 1.5, 1.5),
            point(3, "tree", 0.5, 2.5),
            point(4, "tree", 5.5, 1.5),
            point(5, "tree", 9.5, 1.5),
        ];
        let map = terrain_map(no, |x, _| x >= 4, objects);
        let outcome = check(&map, &CustomTypes::default());
        assert_eq!(
            outcome.problems,
            [
                Problem::OutsideMap {
                    layer: "things".into(),
                    object: ObjectAt {
                        id: 5,
                        kind: "tree".into(),
                        at: DVec2::new(9.5, 1.5)
                    },
                },
                Problem::OverNothing {
                    layer: "things".into(),
                    objects: vec![ObjectAt {
                        id: 4,
                        kind: "tree".into(),
                        at: DVec2::new(5.5, 1.5)
                    }],
                },
            ]
        );
        assert_eq!(
            outcome.problems[1].text(),
            "things: 1 object stands on nothing (no ground under it): #4 tree at 5.5,1.5"
        );

        // A layer all over nothing (clouds) is fine.
        let clouds = vec![point(1, "cloud", 4.5, 0.5), point(2, "cloud", 5.5, 1.5)];
        let map = terrain_map(no, |x, _| x >= 4, clouds);
        assert!(!check(&map, &CustomTypes::default()).has_problems());
    }

    #[test]
    fn types_the_project_lacks_and_properties_their_class_lacks_are_problems() {
        let mut types = CustomTypes::default();
        let mut members = bt_core::Properties::new();
        members.insert("wave".into(), Property::new(Value::Int(1)));
        types.add(CustomType::class(
            "spawn",
            ClassType {
                members,
                use_as: ClassUsage::OBJECT,
                ..ClassType::default()
            },
        ));
        let mut good = point(1, "spawn", 0.5, 0.5);
        good.properties
            .insert("wave".into(), Property::new(Value::Int(2)));
        let mut extra = point(2, "spawn", 1.5, 0.5);
        extra
            .properties
            .insert("speed".into(), Property::new(Value::Int(3)));
        let boss = point(3, "boss", 2.5, 0.5);
        let map = terrain_map(no, no, vec![good, extra, boss]);
        let outcome = check(&map, &types);
        assert_eq!(
            outcome.problems,
            [
                Problem::NotAMember {
                    layer: "things".into(),
                    object: 2,
                    class: "spawn".into(),
                    property: "speed".into(),
                },
                Problem::UndefinedType {
                    layer: "things".into(),
                    object: 3,
                    kind: "boss".into(),
                    defined: vec!["spawn".into()],
                },
            ]
        );
        // Without a project defining types, types are not checked.
        assert!(!check(&map, &CustomTypes::default()).has_problems());
    }

    #[test]
    fn objects_are_grouped_by_what_they_stand_on_with_the_odd_ones_by_id() {
        let lake = |x: i32, y: i32| x >= 4 && y >= 2;
        let mut objects: Vec<MapObject> = (0..6)
            .map(|n| {
                point(
                    n + 1,
                    "tree",
                    0.5 + f64::from(n % 3),
                    0.5 + f64::from(n / 3),
                )
            })
            .collect();
        objects.push(point(7, "tree", 4.5, 3.5));
        objects.push(MapObject {
            id: ObjectId(8),
            class: "zone".into(),
            pos: DVec2::new(0.0, 0.0),
            size: DVec2::new(32.0, 16.0),
            ..MapObject::default()
        });
        let map = terrain_map(lake, no, objects);
        let outcome = check(&map, &CustomTypes::default());
        assert!(!outcome.has_problems());
        let [things] = outcome.layers.as_slice() else {
            panic!("{:?}", outcome.layers)
        };
        assert_eq!(
            things.groups,
            [StandingGroup {
                what: "tree".into(),
                grounds: vec![
                    GroundCount {
                        ground: "grass".into(),
                        count: 6,
                        odd: Vec::new()
                    },
                    GroundCount {
                        ground: "water".into(),
                        count: 1,
                        odd: vec![7]
                    },
                ],
            }]
        );
        assert_eq!(things.zones[0].covers[0].ground, "grass");
        assert_eq!(things.zones[0].covers[0].count, 2);
        let text = outcome.report().text();
        assert!(
            text.contains("things tree: grass ×6, water ×1 (#7)"),
            "{text}"
        );
        assert!(text.contains("things zone #8 covers grass ×2"), "{text}");
        let json = crate::report::to_json(&outcome).unwrap();
        assert_eq!(
            crate::report::from_json::<CheckOutcome>(&json).unwrap(),
            outcome
        );
    }

    #[test]
    fn an_edit_says_the_seams_it_made_and_the_objects_whose_ground_changed() {
        let before = terrain_map(no, no, vec![point(1, "tree", 1.5, 1.5)]);
        let pond = |x: i32, y: i32| (1..=2).contains(&x) && (1..=2).contains(&y);
        let after = terrain_map(pond, no, vec![point(1, "tree", 1.5, 1.5)]);
        let changed: Vec<IVec2> = (0..3)
            .flat_map(|y| (0..3).map(move |x| IVec2::new(x, y)))
            .collect();
        let made = made_by(&before, &after, &changed);
        assert!(
            made.problems.is_empty(),
            "the pond fits: {:?}",
            made.problems
        );
        assert_eq!(
            made.moved,
            [GroundChange {
                from: "grass".into(),
                to: "water".into(),
                objects: vec![ObjectAt {
                    id: 1,
                    kind: "tree".into(),
                    at: DVec2::new(1.5, 1.5)
                }],
            }]
        );
        assert_eq!(
            made.lines()[0].text,
            "1 object that stood on grass stands on water now: #1 tree"
        );
        assert!(made_by(&before, &before, &changed).is_empty());
    }
}
