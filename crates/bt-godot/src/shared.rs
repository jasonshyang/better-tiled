//! A TileSet shared by the maps of one grid (docs/design/12-godot-export.md): one `.tres`, at the map's
//! `tilesetResPath` or else in the setup's TileSet folder (`res://tilesets/<grid>.tres`), which
//! the scenes' tile layers use instead of a TileSet of their own.
//!
//! - **One grid per file.** A tile's `texture_origin` depends on the cell, so the maps sharing a
//!   file have one orientation, tile size, stagger and hex side: the grid key, such as
//!   `ortho-16x16`, `iso-64x32`, `stag-y-odd-64x32` or `hex-y-odd-14x12-s6`. A map of another grid
//!   fails.
//! - **The file is the registry.** Each source's `resource_name` names its tileset, tile or
//!   animation (tileset.rs), so an export finds the ids the file gives them and keeps them; a new
//!   source takes the highest id + 1. Cells in every scene name these ids.
//! - **It holds every tileset ever registered**, whichever map is exported: the map's tilesets
//!   are extended with the others the file names, each made again from its own file, so
//!   exporting one map drops nothing another relies on, and gives the same bytes. Sources it can
//!   no longer make (the tileset's file is missing, or the tileset no longer has them) are kept as
//!   they were.
//! - **Its ids stay the same whichever map it is made from**: its sources, images and alternative
//!   tiles are numbered by name, flips and source id, never by what one map uses first; images
//!   from outside the project are copied by their path from their tileset.
//! - It goes through the contract like a scene (contract.rs): its header and
//!   `metadata/bt_source` name the grid's tilesets rather than one map. A missing one that other
//!   scenes use (generated, or the user's own painted with it) is refused, since a new one would
//!   number its sources afresh under their cells; [`super::export_all`] makes it again from
//!   every map.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_core::{
    CustomTypes, Map, Orientation, Properties, StaggerAxis, StaggerIndex, TilesetRef, Value,
};
use glam::IVec2;

use super::contract::{self, MARKER};
use super::conventions::TILESET_RES_PATH;
use super::grid::{FLIP_H, FLIP_V, Flipped, TRANSPOSE};
use super::plan::Kind;
use super::project_scan::Scan;
use super::read::{self, Resource};
use super::report::{Element, Note, Severity};
use super::setup::{self, Setup};
use super::tiledata::Kind as DataKind;
use super::tileset::{AtlasSource, TileSetBuilder};
use super::{Options, res_path, text};
use crate::build::Build;
use crate::resolve::resolved;
use bt_utils::paths::canonical_or_clean as normalized;

/// The shared TileSet a map's export writes, and what it has from the file already there.
pub(super) struct Shared {
    /// The `.tres`, and its `res://` path.
    pub path: PathBuf,
    pub res: String,
    /// The uid it has, or will have: what the scenes name it by.
    pub uid: String,
    /// What its header and `metadata/bt_source` say it is made from.
    pub from: String,
    /// Other scenes using it while it is missing, generated or not.
    users: Vec<PathBuf>,
    /// The file as it is, when it is this grid's shared TileSet.
    old: Old,
    /// The tilesets the file has sources of, by the name the sources are named after.
    registered: HashSet<String>,
    /// How many of the map's tilesets are its own; those after come from the file.
    own: usize,
    /// Registered tilesets that can't be read, and why.
    missing: Vec<(String, String)>,
    /// The sources it has that the export doesn't make again, kept as they were, by id.
    kept: Vec<usize>,
    /// Each tileset's place (by slot): the lowest id of its sources.
    ranks: Vec<usize>,
    /// The custom data layers of the kept sources, each with a value of its type, by source id.
    kept_layers: Vec<(usize, String, Value)>,
    /// How many physics layers the kept sources use.
    kept_physics: usize,
}

/// The shared TileSet file as it is: what an export keeps of it.
#[derive(Default)]
struct Old {
    /// Each source by id: its `resource_name` and its section.
    sources: BTreeMap<usize, (String, Section)>,
    /// The other sub-resources, by id.
    subs: HashMap<String, Section>,
    /// The `ext_resource`s by id: their type and path.
    ext: HashMap<String, (String, String)>,
    /// The custom data layers by index: their names and Variant types.
    layers: Vec<(String, i64)>,
    /// The terrain sets by index: their lines under `terrain_set_N/`.
    terrain_sets: BTreeMap<usize, Vec<(String, String)>>,
}

/// A terrain set the file has for kept sources: its place, its index in the file, and its lines
/// under `terrain_set_N/`.
pub(super) type KeptTerrainSet = (usize, usize, Vec<(String, String)>);

/// An animated tile as `animation_sources` is given it: its cell, and its lines under `x:y/`.
pub(super) type AnimatedTile = (IVec2, Vec<(String, String)>);

/// A section of the file: its type and its lines, as written.
struct Section {
    kind: String,
    props: Vec<(String, String)>,
}

/// What a shared TileSet's header and marker say it is made from.
fn marker(key: &str) -> String {
    format!("the tilesets of {key} maps")
}

/// The grid key of a shared TileSet's marker.
fn marked_key(marker: &str) -> Option<&str> {
    marker
        .strip_prefix("the tilesets of ")?
        .strip_suffix(" maps")
}

/// The map's grid, as a shared TileSet's name: `ortho-16x16`, `iso-64x32`, `stag-y-odd-64x32`,
/// `hex-y-odd-14x12-s6`.
pub(super) fn grid_key(map: &Map) -> String {
    let size = format!("{}x{}", map.tile_width, map.tile_height);
    let axis = if map.stagger_axis == StaggerAxis::X {
        "x"
    } else {
        "y"
    };
    let index = if map.stagger_index == StaggerIndex::Even {
        "even"
    } else {
        "odd"
    };
    match map.orientation {
        Orientation::Orthogonal => format!("ortho-{size}"),
        Orientation::Isometric => format!("iso-{size}"),
        Orientation::Staggered => format!("stag-{axis}-{index}-{size}"),
        Orientation::Hexagonal => format!("hex-{axis}-{index}-{size}-s{}", map.hex_side_length),
        Orientation::Oblique => format!("oblique-{size}"),
    }
}

/// The base of a source's name: its tileset's, without `#<tile>` or `#anim<n>`.
fn base_of(name: &str) -> &str {
    name.rsplit_once('#').map_or(name, |(base, _)| base)
}

fn unescaped(name: &str) -> String {
    name.replace("%23", "#").replace("%25", "%")
}

/// The metadata/bt_source of a resource's `[resource]`.
fn marker_of(resource: &Resource) -> Option<String> {
    resource.resource()?.first(MARKER).and_then(read::string)
}

impl Shared {
    /// The shared TileSet of the map (resolved) at `map_path` exported to `target`, if its tile
    /// layers use one: its `tilesetResPath`, else the setup's when the setup is for this project
    /// and doesn't keep TileSets in the scenes. With `rebuilding`, a missing one is made afresh
    /// even if generated scenes use it ([`super::export_all`]), but not if the user's own do.
    pub fn of(
        map: &Map,
        map_path: &Path,
        project: &Path,
        target: &Path,
        scan: &Scan,
        options: &Options,
        rebuilding: bool,
    ) -> Result<Option<Shared>, String> {
        let key = grid_key(map);
        let Some(path) = place(map, map_path, project, &key)? else {
            return Ok(None);
        };
        let res = res_path(project, &path).unwrap_or_default();
        let from = marker(&key);
        let text = std::fs::read_to_string(&path).ok();
        let resource = text.as_deref().and_then(|t| read::parse(t).ok());
        let theirs = resource.as_ref().and_then(marker_of);
        if let Some(other) = theirs.as_deref().and_then(marked_key).filter(|k| *k != key) {
            return Err(format!(
                "{res} is the TileSet of {other} maps, and this map's grid is {key}: a tile sits on one grid only, so give the map a tilesetResPath of its own"
            ));
        }
        // Godot keeps the names and the marker when it saves the file, so it is read even then.
        let old = resource
            .as_ref()
            .filter(|_| theirs.as_deref() == Some(from.as_str()))
            .map(Old::of)
            .unwrap_or_default();
        let kept_uid = text
            .as_deref()
            .and_then(|t| read::parse_header(t).ok())
            .and_then(|r| r.uid());
        let uid = contract::uid(Kind::TileSet, &path, kept_uid, scan, options.new_uid);
        let mut users = Vec::new();
        if !path.exists() {
            // A scene of the user's painted with it counts too: its cells name the ids as well.
            // Rebuilding exports every generated one again (Godot's re-save drops the header but
            // keeps the marker), but can't renumber the user's cells.
            let is_scene = |p: &Path| p != target && p.extension().is_some_and(|e| e == "tscn");
            let users_of = scan.referencing(&res, read::text_to_uid(&uid)).filter(|e| {
                is_scene(&e.path) && !(rebuilding && (e.generated || contract::exported(&e.path)))
            });
            users = users_of.map(|e| e.path.clone()).collect();
        }
        let registered = old
            .sources
            .values()
            .map(|(name, _)| base_of(name).to_owned())
            .collect();
        let shared = Shared {
            path,
            res,
            uid,
            from,
            users,
            old,
            registered,
            own: map.tilesets.len(),
            missing: Vec::new(),
            kept: Vec::new(),
            ranks: Vec::new(),
            kept_layers: Vec::new(),
            kept_physics: 0,
        };
        Ok(Some(shared))
    }

    /// Whether the file has sources of the tileset whose sources are named after `name`.
    pub fn registered(&self, name: &str) -> bool {
        self.registered.contains(name)
    }

    /// The folder the file is in.
    pub fn folder(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new(""))
    }

    pub fn kept_physics(&self) -> usize {
        self.kept_physics
    }

    /// How many layers named `<prefix>N` (`navigation_layer_`) the kept sources' tiles use.
    pub fn kept_uses(&self, prefix: &str) -> usize {
        let keys = self
            .kept
            .iter()
            .filter_map(|id| self.old.sources.get(id))
            .flat_map(|source| &source.1.props)
            .map(|(key, _)| key);
        keys.filter_map(|key| {
            key.split('/')
                .find_map(|part| part.strip_prefix(prefix))?
                .parse::<usize>()
                .ok()
        })
        .map(|n| n + 1)
        .max()
        .unwrap_or(0)
    }

    /// The terrain sets that the kept sources of tilesets that can't be read have tiles in, from
    /// the file: each with its place (the lowest id of its tileset's kept sources), its index in
    /// the file and its lines under `terrain_set_N/`. Sources that a tileset which can be read
    /// no longer makes are in none (terrain.rs).
    pub fn kept_terrain_sets(&self) -> Vec<KeptTerrainSet> {
        let base = |id: &usize| {
            self.old
                .sources
                .get(id)
                .map(|s| base_of(&s.0))
                .unwrap_or("")
        };
        let mut first: HashMap<&str, usize> = HashMap::new();
        for id in self
            .kept
            .iter()
            .filter(|id| self.missing.iter().any(|(b, _)| b == base(id)))
        {
            let place = first.entry(base(id)).or_insert(*id);
            *place = (*place).min(*id);
        }
        let mut sets: BTreeMap<usize, usize> = BTreeMap::new();
        for id in &self.kept {
            let Some(&place) = first.get(base(id)) else {
                continue;
            };
            for (key, value) in self
                .old
                .sources
                .get(id)
                .into_iter()
                .flat_map(|s| &s.1.props)
            {
                if key.ends_with("/terrain_set")
                    && let Some(n) = read::int(value)
                        .and_then(|n| usize::try_from(n).ok())
                        .filter(|n| self.old.terrain_sets.contains_key(n))
                {
                    let at = sets.entry(n).or_insert(place);
                    *at = (*at).min(place);
                }
            }
        }
        sets.into_iter()
            .filter_map(|(n, place)| Some((place, n, self.old.terrain_sets.get(&n)?.clone())))
            .collect()
    }

    /// The animation source (by index: 0 for `#anim1`) the file has each of an atlas's animated
    /// tiles in, `heads` giving each one's cell (its first frame's) and its animation's lines; the
    /// atlas's sources are named after `name`. Tiles sharing a cell are in different sources: each
    /// such source goes to the tile whose lines it has, else to the first.
    pub fn animation_sources(&self, name: &str, heads: &[AnimatedTile]) -> Vec<Option<usize>> {
        let prefix = format!("{name}#anim");
        // Each animated tile of the file: its source, cell and lines.
        let mut had: Vec<(usize, AnimatedTile)> = Vec::new();
        for (source, section) in self.old.sources.values() {
            let Some(n) = source
                .strip_prefix(&prefix)
                .and_then(|n| n.parse::<usize>().ok())
                .filter(|n| *n > 0)
            else {
                continue;
            };
            for (key, _) in &section.props {
                let Some((x, y)) = key.strip_suffix("/0").and_then(|cell| cell.split_once(':'))
                else {
                    continue;
                };
                let (Ok(x), Ok(y)) = (x.parse(), y.parse()) else {
                    continue;
                };
                let tile = format!("{x}:{y}/");
                let lines = section
                    .props
                    .iter()
                    .filter_map(|(k, v)| Some((k.strip_prefix(&tile)?.to_owned(), v.clone())))
                    .filter(|(k, _)| k.starts_with("animation_"))
                    .collect();
                had.push((n - 1, (IVec2::new(x, y), lines)));
            }
        }
        had.sort_by_key(|(n, _)| *n);
        heads
            .iter()
            .map(|(head, lines)| {
                let at = had
                    .iter()
                    .position(|(_, (h, l))| h == head && l == lines)
                    .or_else(|| had.iter().position(|(_, (h, _))| h == head))?;
                Some(had.remove(at).0)
            })
            .collect()
    }

    /// Adds the registered tilesets the map (resolved) lacks to its tilesets, read from their
    /// files, with their source names, folders and rule flags (`names`, `dirs` and `rules` by
    /// slot, the map's own given): so the TileSet made from the map holds them too.
    pub fn extend(
        &mut self,
        map: &mut Map,
        names: &mut Vec<String>,
        dirs: &mut Vec<PathBuf>,
        rules: &mut Vec<bool>,
        project: &Path,
        custom_types: &CustomTypes,
    ) {
        let mut bases: Vec<&str> = Vec::new();
        for (name, _) in self.old.sources.values() {
            let base = base_of(name);
            // A source without a name (made in Godot) has no tileset to make it again.
            if !base.is_empty()
                && !bases.contains(&base)
                && !names.iter().any(|n| n.as_str() == base)
            {
                bases.push(base);
            }
        }
        if bases.is_empty() {
            return;
        }
        let mut loader = bt_tmx::Loader::builder()
            .custom_types(Arc::new(custom_types.clone()))
            .build();
        let mut extras = Vec::new();
        for base in bases {
            match load(base, project, &mut loader) {
                Ok((tileset, dir)) => extras.push((base.to_owned(), dir, tileset)),
                Err(why) => self.missing.push((base.to_owned(), why)),
            }
        }
        let mut next = map
            .tilesets
            .iter()
            .map(|t| t.first_gid.unwrap_or(1) + t.tileset.tile_count())
            .max()
            .unwrap_or(1);
        let mut tilesets = Vec::new();
        for (name, dir, mut tileset) in extras {
            tileset.first_gid = Some(next);
            next += tileset.tileset.tile_count().max(1);
            names.push(name);
            dirs.push(dir);
            rules.push(false);
            tilesets.push(tileset);
        }
        // Resolved as the map's own are.
        map.tilesets.extend(
            resolved(
                &Map {
                    tilesets,
                    ..Map::default()
                },
                custom_types,
            )
            .tilesets,
        );
    }
}

/// Where the map's shared TileSet goes, if it has one.
fn place(map: &Map, map_path: &Path, project: &Path, key: &str) -> Result<Option<PathBuf>, String> {
    if let Some(property) = map.properties.get(TILESET_RES_PATH) {
        let res = match &property.value {
            Value::String(s) => s.clone(),
            Value::File(f) if f.is_empty() => String::new(),
            Value::File(f) => {
                res_path(project, &map_path.parent().unwrap_or(Path::new("")).join(f))
                    .unwrap_or_else(|| f.clone())
            }
            other => {
                return Err(format!(
                    "{TILESET_RES_PATH} is a res://….tres path, and this one is {}",
                    other.type_name()
                ));
            }
        };
        // An empty one is none, as in Tiled.
        if !res.is_empty() {
            let file = res.rsplit('/').next().unwrap_or_default();
            return match setup::in_project(project, &res)
                .filter(|_| file.len() > ".tres".len() && file.ends_with(".tres"))
            {
                Some(path) => Ok(Some(path)),
                None => Err(format!(
                    "{TILESET_RES_PATH} is a res://….tres path inside the Godot project, and this one is {}",
                    text::string(&res)
                )),
            };
        }
    }
    let Some(setup) = Setup::of(map_path)? else {
        return Ok(None);
    };
    let same = setup
        .project()
        .and_then(|p| p.canonicalize().ok())
        .zip(project.canonicalize().ok())
        .is_some_and(|(a, b)| a == b);
    if !same || setup.godot.tile_sets.as_deref() == Some("embedded") {
        return Ok(None);
    }
    let folder = setup
        .godot
        .tile_set_folder
        .as_deref()
        .unwrap_or(setup::TILE_SETS);
    let dir = setup::in_project(project, folder).ok_or_else(|| format!("the godot setup's tileSetFolder is a res:// folder inside the Godot project, and this one is {}", text::string(folder)))?;
    Ok(Some(dir.join(format!("{key}.tres"))))
}

/// The registered tileset whose sources are named after `base` (tileset.rs `source_names`), from
/// its file or the map it is in, with the folder its files are relative to.
fn load(
    base: &str,
    project: &Path,
    loader: &mut bt_tmx::Loader,
) -> Result<(TilesetRef, PathBuf), String> {
    // The second of two of one name in a map is `~2`.
    let (name, nth) = match base.rsplit_once('~') {
        Some((name, n)) if n.parse::<usize>().is_ok_and(|n| n > 1) => {
            (name, n.parse().unwrap_or(1))
        }
        _ => (base, 1),
    };
    match name.split_once("::") {
        Some((map_file, tileset)) => {
            let map_path = normalized(&project.join(unescaped(map_file)));
            let loaded = loader.load_map(&map_path).map_err(|e| e.to_string())?;
            let tileset = unescaped(tileset);
            let found = loaded
                .value
                .tilesets
                .into_iter()
                .filter(|t| t.file.is_none() && t.tileset.name == tileset)
                .nth(nth - 1);
            let found = found
                .ok_or_else(|| format!("{} has no tileset named {tileset}", map_path.display()))?;
            Ok((
                found,
                map_path
                    .parent()
                    .map_or_else(PathBuf::new, Path::to_path_buf),
            ))
        }
        None => {
            let file = normalized(&project.join(unescaped(name)));
            let tileset = loader.load_tileset(&file).map_err(|e| e.to_string())?;
            let dir = file.parent().map_or_else(PathBuf::new, Path::to_path_buf);
            Ok((
                TilesetRef {
                    first_gid: Some(1),
                    file: Some(bt_core::FileRef::new(
                        file.to_string_lossy().into_owned(),
                        &file,
                    )),
                    tileset: tileset.value,
                },
                dir,
            ))
        }
    }
}

impl Old {
    fn of(resource: &Resource) -> Old {
        let mut old = Old::default();
        for ext in resource.ext_resources() {
            old.ext.insert(ext.id, (ext.class, ext.path));
        }
        for (sub, section) in resource.sub_resources() {
            let props = section
                .props
                .iter()
                .map(|(k, v)| (k.to_string(), (*v).to_owned()))
                .collect();
            old.subs.insert(
                sub.id,
                Section {
                    kind: sub.class,
                    props,
                },
            );
        }
        let Some(main) = resource.resource() else {
            return old;
        };
        let mut layers: BTreeMap<usize, (Option<String>, Option<i64>)> = BTreeMap::new();
        for (key, value) in &main.props {
            if let Some(id) = key
                .strip_prefix("sources/")
                .and_then(|n| n.parse::<usize>().ok())
                && let Some(sub) = read::sub_resource(value).and_then(|id| old.subs.remove(&id))
            {
                let name = sub
                    .props
                    .iter()
                    .find(|(k, _)| k == "resource_name")
                    .and_then(|(_, v)| read::string(v))
                    .unwrap_or_default();
                old.sources.insert(id, (name, sub));
            } else if let Some((n, what)) = key
                .strip_prefix("custom_data_layer_")
                .and_then(|k| k.split_once('/'))
                && let Ok(n) = n.parse::<usize>()
            {
                let layer = layers.entry(n).or_default();
                match what {
                    "name" => layer.0 = read::string(value),
                    "type" => layer.1 = read::int(value),
                    _ => {}
                }
            } else if let Some((n, what)) = key
                .strip_prefix("terrain_set_")
                .and_then(|k| k.split_once('/'))
                && let Ok(n) = n.parse::<usize>()
            {
                old.terrain_sets
                    .entry(n)
                    .or_default()
                    .push((what.to_owned(), (*value).to_owned()));
            }
        }
        let count = layers.keys().next_back().map_or(0, |n| n + 1);
        old.layers = (0..count)
            .map(|n| {
                layers
                    .remove(&n)
                    .map(|(name, kind)| (name.unwrap_or_default(), kind.unwrap_or(0)))
                    .unwrap_or_default()
            })
            .collect();
        old
    }
}

/// A value of a custom data layer's Variant type, which works out the layers as one of its tiles'
/// would.
fn sample(kind: i64) -> Option<Value> {
    Some(match kind {
        1 => Value::Bool(false),
        2 => Value::Int(0),
        3 => Value::Float(0.0),
        4 => Value::String(String::new()),
        20 => Value::Color(None),
        27 => Value::Class(Properties::new()),
        28 => Value::List(Vec::new()),
        _ => return None,
    })
}

impl TileSetBuilder {
    /// The tiles' properties (`own`, those of `tiles`) in the order the custom data layers are
    /// worked out in: by tileset in the TileSet's order, with a shared TileSet's kept sources
    /// in their places.
    pub(super) fn in_order<'v>(
        &'v self,
        tiles: &[(usize, u32)],
        own: &'v [BTreeMap<&'v str, Value>],
    ) -> Vec<Vec<(&'v str, &'v Value)>> {
        let rank = |slot: usize| {
            self.shared
                .as_ref()
                .map_or(slot, |s| s.ranks.get(slot).copied().unwrap_or(usize::MAX))
        };
        let mut ordered: Vec<(usize, Vec<(&str, &Value)>)> = tiles
            .iter()
            .zip(own)
            .map(|(&(slot, _), own)| {
                (
                    rank(slot),
                    own.iter().map(|(name, value)| (*name, value)).collect(),
                )
            })
            .collect();
        if let Some(shared) = &self.shared {
            ordered.extend(
                shared
                    .kept_layers
                    .iter()
                    .map(|(id, name, value)| (*id, vec![(name.as_str(), value)])),
            );
        }
        // Stable: tiles keep their order within their tileset.
        ordered.sort_by_key(|(rank, _)| *rank);
        ordered.into_iter().map(|(_, own)| own).collect()
    }
}

impl Build<'_> {
    /// Gives the TileSet's sources their ids: their place, or for a shared TileSet the id its
    /// file has for their name (new ones after the highest), keeping the file's sources the
    /// export doesn't make. A shared TileSet's tiles also get the alternatives of every flip that
    /// moves them, numbered by the flip, whichever flips this map uses.
    pub(super) fn number_sources(&mut self) {
        let Some(shared) = &mut self.tileset.shared else {
            for (i, source) in self.tileset.sources.iter_mut().enumerate() {
                source.id = i;
            }
            return;
        };
        let ids: HashMap<&str, usize> = shared
            .old
            .sources
            .iter()
            .map(|(&id, (name, _))| (name.as_str(), id))
            .collect();
        let mut next = shared.old.sources.keys().next_back().map_or(0, |id| id + 1);
        let mut made = HashSet::new();
        for source in &mut self.tileset.sources {
            source.id = ids.get(source.name.as_str()).copied().unwrap_or_else(|| {
                next += 1;
                next - 1
            });
            made.insert(source.name.clone());
        }
        shared.kept = shared
            .old
            .sources
            .iter()
            .filter(|(_, (name, _))| !made.contains(name))
            .map(|(&id, _)| id)
            .collect();
        shared.ranks = vec![usize::MAX; self.map.tilesets.len()];
        for source in &self.tileset.sources {
            for def in source.tiles.values() {
                if let Some(rank) = shared.ranks.get_mut(def.slot) {
                    *rank = (*rank).min(source.id);
                } else {
                    self.failures.push(format!(
                        "shared source has missing tileset slot {}",
                        def.slot
                    ));
                }
            }
        }
        // What the kept sources use of the TileSet's layers.
        for &id in &shared.kept {
            let mut seen = BTreeSet::new();
            for (key, _) in shared
                .old
                .sources
                .get(&id)
                .into_iter()
                .flat_map(|s| &s.1.props)
            {
                if let Some(n) = key
                    .rsplit_once("/custom_data_")
                    .and_then(|(_, n)| n.parse::<usize>().ok())
                    && let Some((name, kind)) = shared.old.layers.get(n)
                    && let Some(value) = sample(*kind)
                    && seen.insert(name.clone())
                {
                    shared.kept_layers.push((id, name.clone(), value));
                }
                if let Some(n) = key
                    .split('/')
                    .find_map(|part| part.strip_prefix("physics_layer_"))
                    .and_then(|n| n.parse::<usize>().ok())
                {
                    shared.kept_physics = shared.kept_physics.max(n + 1);
                }
            }
        }
        let notes = kept_notes(shared);
        self.notes.extend(notes);
        self.flip_all();
    }

    /// A shared TileSet's alternatives for every flip that would move a tile, whichever flips the
    /// map uses (grid.rs gives them to cells): each numbered by its flip, mirrored sideways 1, up
    /// and down 2, transposed 4, and their sums.
    fn flip_all(&mut self) {
        // Tiled's hexagonal maps turn tiles rather than transpose them, and Godot keeps only mirrors.
        let flips: &[u16] = if self.map.orientation == Orientation::Hexagonal {
            &[1, 2, 3]
        } else {
            &[1, 2, 3, 4, 5, 6, 7]
        };
        let (map, grid) = (self.map, &self.grid);
        for source in &mut self.tileset.sources {
            for def in source.tiles.values_mut() {
                let Some(offset) = map.tilesets.get(def.slot).map(|t| t.tileset.tile_offset) else {
                    self.failures
                        .push(format!("shared tile has missing slot {}", def.slot));
                    continue;
                };
                for &n in flips {
                    let flags = [(1, FLIP_H), (2, FLIP_V), (4, TRANSPOSE)]
                        .iter()
                        .filter(|(bit, _)| n & bit != 0)
                        .map(|(_, flag)| flag)
                        .sum();
                    let (origin, _) = grid.texture_origin(source.region, offset, flags);
                    if origin != def.origin {
                        def.flipped.push(Flipped {
                            flags,
                            origin,
                            id: n,
                        });
                    }
                }
            }
        }
    }

    /// Leaves out what the notes of the TileSet's making (from `from` on) say about the tilesets
    /// a shared TileSet has from other maps: their maps' exports say it.
    pub(super) fn own_notes(&mut self, from: usize) {
        let Some(own) = self.tileset.shared.as_ref().map(|s| s.own) else {
            return;
        };
        let mine = |e: &Element| !matches!(e, Element::Tileset(slot) | Element::Tile { tileset: slot, id: _ } if *slot >= own);
        for mut note in self.notes.split_off(from) {
            let about = !note.elements.is_empty();
            note.elements.retain(mine);
            if !about || !note.elements.is_empty() {
                self.notes.push(note);
            }
        }
    }

    /// The shared TileSet's file, what it says it is made from, and the scenes that use it while
    /// it is missing: `None` without one, or with nothing to put in a new one.
    pub(super) fn shared_tileset(&mut self) -> Option<(PathBuf, Vec<u8>, String, Vec<PathBuf>)> {
        self.tileset.shared.as_ref()?;
        self.tileset_contents();
        let shared = self.tileset.shared.as_ref()?;
        if self.tileset.sources.is_empty() && shared.kept.is_empty() && !shared.path.exists() {
            return None;
        }
        let layers = self.tileset.data.layers();
        let mut keeper = Keeper {
            old: &shared.old,
            ext: Ext::default(),
            subs: BTreeMap::new(),
        };
        let mut sources = String::new();
        let mut ids: Vec<(usize, Option<&AtlasSource>)> = self
            .tileset
            .sources
            .iter()
            .map(|s| (s.id, Some(s)))
            .chain(shared.kept.iter().map(|&id| (id, None)))
            .collect();
        ids.sort_by_key(|(id, _)| *id);
        for &(id, source) in &ids {
            match source {
                Some(source) => {
                    let texture = keeper.ext.id("Texture2D", &source.texture);
                    self.source_section(&mut sources, source, &texture);
                }
                None => keeper.source(&mut sources, id, layers, &self.tileset.terrain.kept),
            }
        }
        let mut out = "[gd_resource type=\"TileSet\" format=3]\n".to_owned();
        for (n, (kind, path)) in keeper.ext.files.iter().enumerate() {
            let _ = write!(
                out,
                "\n[ext_resource type=\"{kind}\" path={} id=\"{}\"]",
                text::string(path),
                n + 1
            );
            // The kept sources' images too stay in use, as copied then.
            if kind == "Texture2D" {
                self.images.keep(path, &shared.res);
            }
        }
        if !keeper.ext.files.is_empty() {
            out.push('\n');
        }
        out.push_str(&super::collision::resources(&self.tileset, keeper.subs));
        out.push_str(&sources);
        out.push_str("\n[resource]\n");
        for (key, value) in self.tileset_lines() {
            let _ = writeln!(out, "{key} = {value}");
        }
        for (id, _) in &ids {
            let _ = writeln!(
                out,
                "sources/{id} = SubResource(\"TileSetAtlasSource_{id}\")"
            );
        }
        let _ = writeln!(out, "{MARKER} = {}", text::string(&shared.from));
        Some((
            shared.path.clone(),
            out.into_bytes(),
            shared.from.clone(),
            shared.users.clone(),
        ))
    }

    /// Gives the scenes the map instances (`resPath`) the uid they have, by which Godot still
    /// finds them once they moved.
    pub(super) fn instance_uids(&mut self, scan: &Scan) {
        for ext in self
            .scene
            .ext
            .iter_mut()
            .filter(|e| e.kind == "PackedScene" && e.uid.is_none())
        {
            let entry = scan.entries.iter().find(|e| {
                e.uid.is_some()
                    && res_path(&scan.project, &e.path).as_deref() == Some(ext.path.as_str())
            });
            ext.uid = entry.and_then(|e| e.uid).map(read::uid_to_text);
        }
    }
}

/// The notes on the sources a shared TileSet keeps as they were: a warning for each tileset whose
/// file is missing, and one note for those whose tilesets no longer have them.
fn kept_notes(shared: &Shared) -> Vec<Note> {
    let mut by_base: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for &id in &shared.kept {
        by_base
            .entry(
                shared
                    .old
                    .sources
                    .get(&id)
                    .map(|s| base_of(&s.0))
                    .unwrap_or(""),
            )
            .or_default()
            .push(id);
    }
    let mut notes = Vec::new();
    let mut gone = Vec::new();
    for (base, ids) in by_base {
        match shared.missing.iter().find(|(b, _)| b == base) {
            Some((_, why)) => {
                let kept = if ids.len() == 1 {
                    "the source".to_owned()
                } else {
                    format!("the {} sources", ids.len())
                };
                let message = format!(
                    "{} keeps {kept} of {base} as made before, since that tileset can't be read ({why}): restore it to update them",
                    shared.res
                );
                notes.push(Note::warning(
                    crate::Code::SharedTilesetMissing,
                    message,
                    Vec::new(),
                ));
            }
            None => gone.extend(ids.iter().map(|id| {
                shared
                    .old
                    .sources
                    .get(id)
                    .map(|s| s.0.clone())
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| format!("source {id}"))
            })),
        }
    }
    if !gone.is_empty() {
        let message = format!(
            "{} keeps {} as made before, for the scenes exported then, though no tileset makes them now: to drop them, delete the file and export every map again",
            shared.res,
            gone.join(", ")
        );
        notes.push(Note {
            code: crate::Code::SharedSourceKept,
            severity: Severity::Info,
            message,
            elements: Vec::new(),
        });
    }
    notes
}

/// The files a shared TileSet uses, numbered in the order its sources first use them.
#[derive(Default)]
struct Ext {
    files: Vec<(String, String)>,
    ids: HashMap<String, usize>,
}

impl Ext {
    /// `ExtResource("…")` of the file at `path`.
    fn id(&mut self, kind: &str, path: &str) -> String {
        let n = *self.ids.entry(path.to_owned()).or_insert_with(|| {
            self.files.push((kind.to_owned(), path.to_owned()));
            self.files.len()
        });
        format!("ExtResource(\"{n}\")")
    }
}

/// Writes the sources a shared TileSet keeps from its file: their lines as they were, with the
/// file's numbers of their images and custom data layers made the new file's.
struct Keeper<'o> {
    old: &'o Old,
    ext: Ext,
    /// The sub-resources the kept sources use, by their ids in the file: their types and lines.
    /// They keep their ids, which are those of their contents (collision.rs), and go with the
    /// TileSet's own in one list by id, so the file stays as it was.
    subs: BTreeMap<String, (String, String)>,
}

impl Keeper<'_> {
    /// The section of the kept source `id`; a custom data value whose layer the TileSet no longer
    /// has, or has with another type, is left out, and so is the terrain of a tile whose terrain
    /// set the TileSet no longer has (`terrain_sets`: the file's index → the TileSet's).
    fn source(
        &mut self,
        out: &mut String,
        id: usize,
        layers: &[(String, DataKind)],
        terrain_sets: &HashMap<usize, usize>,
    ) {
        let old = self.old;
        let Some(section) = old.sources.get(&id).map(|s| &s.1) else {
            return;
        };
        let _ = write!(
            out,
            "\n[sub_resource type=\"{}\" id=\"TileSetAtlasSource_{id}\"]\n",
            section.kind
        );
        let mut out_of_sets = HashSet::new();
        for (key, value) in &section.props {
            // A tile's terrain set comes before its terrain and peering bits, as Godot writes them.
            if let Some((tile, what)) = key.split_once("/terrain").filter(|(_, what)| {
                ["_set", ""].contains(what) || what.starts_with("s_peering_bit/")
            }) {
                if what == "_set" {
                    match read::int(value)
                        .and_then(|n| usize::try_from(n).ok())
                        .and_then(|n| terrain_sets.get(&n))
                    {
                        Some(n) => {
                            let _ = writeln!(out, "{key} = {n}");
                        }
                        None => {
                            out_of_sets.insert(tile);
                        }
                    }
                    continue;
                }
                if out_of_sets.contains(tile) {
                    continue;
                }
            }
            let key = match key
                .rsplit_once("/custom_data_")
                .map(|(tile, n)| (tile, n.parse::<usize>()))
            {
                Some((tile, Ok(n))) => {
                    let Some((name, kind)) = old.layers.get(n) else {
                        continue;
                    };
                    let Some(n) = layers
                        .iter()
                        .position(|(l, k)| l == name && *k as i64 == *kind)
                    else {
                        continue;
                    };
                    format!("{tile}/custom_data_{n}")
                }
                _ => key.clone(),
            };
            let value = self.value(value);
            let _ = writeln!(out, "{key} = {value}");
        }
    }

    /// `value` with each `ExtResource("…")` in it renumbered, and each `SubResource("…")` kept.
    fn value(&mut self, value: &str) -> String {
        let mut out = String::new();
        let mut rest = value;
        loop {
            let next = ["ExtResource(\"", "SubResource(\""]
                .into_iter()
                .filter_map(|p| rest.find(p).map(|at| (at, p)))
                .min();
            let Some((at, prefix)) = next else { break };
            let start = at + prefix.len();
            let Some(len) = rest[start..].find('"') else {
                break;
            };
            out.push_str(&rest[..start]);
            let id = &rest[start..start + len];
            let old = self.old;
            let new = if prefix.starts_with("Ext") {
                match old.ext.get(id) {
                    Some((kind, path)) => self
                        .ext
                        .id(kind, path)
                        .trim_start_matches("ExtResource(\"")
                        .trim_end_matches("\")")
                        .to_owned(),
                    None => id.to_owned(),
                }
            } else {
                self.sub(id)
            };
            out.push_str(&new);
            rest = &rest[start + len..];
        }
        out.push_str(rest);
        out
    }

    /// The id of the file's sub-resource `id`, which stays: it is kept the first time.
    fn sub(&mut self, id: &str) -> String {
        let old = self.old;
        let Some(section) = old.subs.get(id) else {
            return id.to_owned();
        };
        if self.subs.contains_key(id) {
            return id.to_owned();
        }
        // Taken before its lines are read, which might name it.
        self.subs
            .insert(id.to_owned(), (section.kind.clone(), String::new()));
        let mut lines = String::new();
        for (key, value) in &section.props {
            let value = self.value(value);
            let _ = writeln!(lines, "{key} = {value}");
        }
        self.subs
            .insert(id.to_owned(), (section.kind.clone(), lines));
        id.to_owned()
    }
}
