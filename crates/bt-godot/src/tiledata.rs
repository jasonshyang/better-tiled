//! What the TileSet's tiles carry besides their images and shapes (docs/design/12-godot-export.md): their
//! properties as custom data layers, one per name, sorted by name as Tiled's exporter sorts
//! them, with a tile's class in a `class` layer; and their Godot settings, `godotZIndex` and
//! `godotYSortOrigin`, as the tile's `z_index` and `y_sort_origin` (Tiled's `zIndex` and
//! `ySortOrigin` on a tile stay custom data, as in Tiled).
//!
//! | Tiled | Godot custom data |
//! |---|---|
//! | bool, int, float, string | bool, int, float, String |
//! | file | String: a `res://` path inside the Godot project, else the path as Tiled wrote it |
//! | color | Color |
//! | object | int, the object's id |
//! | enum | int or String, as it is stored |
//! | class | Dictionary |
//! | list | Array |
//!
//! A layer has the type of its first tile's value (ints and floats together make a float
//! layer). Godot would quietly convert another type's value, or load it as null, so such values
//! are left out, with a note.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use bt_core::{Color, Map, Properties, Value};

use super::conventions::{GODOT_Y_SORT_ORIGIN, GODOT_Z_INDEX};
use super::report::{Element, Note};
use super::tileset::{TileSetBuilder, id_runs};
use super::{res_path, text};
use crate::build::Build;

/// Godot's `z_index` range.
const Z_INDEX: i64 = 4096;

/// What the TileSet's tiles carry, worked out once its sources are made ([`Build::tile_data`]).
#[derive(Default)]
pub(super) struct Data {
    /// The folder each tileset's files are relative to, by slot.
    pub dirs: Vec<PathBuf>,
    /// The custom data layers: name and type, by index.
    layers: Vec<(String, Kind)>,
    /// Each tile's lines under its `x:y/0/`, by slot and id.
    tiles: HashMap<(usize, u32), Vec<(String, String)>>,
    /// Notes about tiles, given out grouped once the TileSet is made.
    pub tally: Tally,
}

impl Data {
    /// The custom data layers: name and type, by index.
    pub fn layers(&self) -> &[(String, Kind)] {
        &self.layers
    }
}

/// Godot's Variant types the layers take. Never 0 (nil): its values load as null.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Bool = 1,
    Int = 2,
    Float = 3,
    String = 4,
    Color = 20,
    Dictionary = 27,
    Array = 28,
}

impl Kind {
    pub fn of(value: &Value) -> Kind {
        match value {
            Value::Bool(_) => Kind::Bool,
            Value::Int(_) | Value::Object(_) => Kind::Int,
            Value::Float(_) => Kind::Float,
            Value::String(_) | Value::File(_) => Kind::String,
            Value::Color(_) => Kind::Color,
            Value::Class(_) => Kind::Dictionary,
            Value::List(_) => Kind::Array,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Kind::Bool => "bool",
            Kind::Int => "int",
            Kind::Float => "float",
            Kind::String => "String",
            Kind::Color => "Color",
            Kind::Dictionary => "Dictionary",
            Kind::Array => "Array",
        }
    }
}

/// The custom data layers of tiles' properties (`tiles` in tileset order, then tile order): one
/// per name, sorted by name, each of the type of its first value, except that ints and floats
/// together make a float layer.
pub(super) fn layers<'v, T: IntoIterator<Item = (&'v str, &'v Value)>>(
    tiles: impl IntoIterator<Item = T>,
) -> Vec<(String, Kind)> {
    let mut seen: BTreeMap<&str, (Kind, bool)> = BTreeMap::new();
    for (name, value) in tiles.into_iter().flatten() {
        let kind = Kind::of(value);
        seen.entry(name).or_insert((kind, false)).1 |= kind == Kind::Float;
    }
    let layer = |(first, float): (Kind, bool)| {
        if float && first == Kind::Int {
            Kind::Float
        } else {
            first
        }
    };
    seen.into_iter()
        .map(|(name, kinds)| (name.to_owned(), layer(kinds)))
        .collect()
}

/// `value` as Godot text in a layer of type `kind`, or `None` when it is of another type. Ints
/// in a float layer are written as floats; files become what `file` makes of them.
pub(super) fn in_layer(
    value: &Value,
    kind: Kind,
    file: &mut dyn FnMut(&str) -> String,
) -> Option<String> {
    match (kind, value) {
        (Kind::Float, Value::Int(i)) => Some(text::float(*i as f64)),
        (Kind::Float, Value::Object(id)) => Some(text::float(f64::from(id.0))),
        _ if Kind::of(value) == kind => Some(variant(value, file)),
        _ => None,
    }
}

/// A value as Godot text, typed as the module's table says.
fn variant(value: &Value, file: &mut dyn FnMut(&str) -> String) -> String {
    match value {
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Object(id) => id.to_string(),
        Value::Float(f) => text::float(*f),
        Value::String(s) => text::string(s),
        Value::File(f) => text::string(&file(f)),
        Value::Color(c) => text::color(c.unwrap_or(Color::rgba(0, 0, 0, 0))),
        Value::Class(members) => {
            let members: Vec<String> = members
                .iter()
                .map(|(k, p)| format!("{}: {}", text::string(k), variant(&p.value, file)))
                .collect();
            format!("{{{}}}", members.join(", "))
        }
        Value::List(items) => format!(
            "[{}]",
            items
                .iter()
                .map(|p| variant(&p.value, file))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// The folder each of the map's tilesets has its files relative to, by slot: its file's, or the
/// map's for a tileset inside the map. Takes the map as loaded: the resolved map has its
/// tilesets embedded.
pub(super) fn tileset_dirs(map: &Map, map_path: &Path) -> Vec<PathBuf> {
    let map_dir = map_path.parent().unwrap_or(Path::new(""));
    map.tilesets
        .iter()
        .map(|t| {
            t.file
                .as_ref()
                .map(|file| file.path.as_path())
                .and_then(Path::parent)
                .map_or_else(|| map_dir.to_path_buf(), |dir| map_dir.join(dir))
        })
        .collect()
}

/// The TileSet's own lines (`custom_data_layer_N/…`), written before its sources: Godot reads a
/// tile's custom data only for layers it already has.
pub(super) fn tileset_lines(_map: &Map, tileset: &TileSetBuilder) -> Vec<(String, String)> {
    let mut lines = Vec::new();
    for (n, (name, kind)) in tileset.data.layers.iter().enumerate() {
        lines.push((format!("custom_data_layer_{n}/name"), text::string(name)));
        lines.push((
            format!("custom_data_layer_{n}/type"),
            (*kind as i32).to_string(),
        ));
    }
    lines
}

/// The lines of tile `id` of the tileset in `slot`, under its `x:y/0/` and those of its flipped
/// alternatives: `z_index`, `y_sort_origin` and `custom_data_N`.
pub(super) fn tile_lines(
    _map: &Map,
    tileset: &TileSetBuilder,
    slot: usize,
    id: u32,
) -> Vec<(String, String)> {
    tileset
        .data
        .tiles
        .get(&(slot, id))
        .cloned()
        .unwrap_or_default()
}

impl Build<'_> {
    /// Works out the custom data layers and each tile's lines from the tiles the TileSet's
    /// sources hold, then gives out the notes gathered while making them.
    pub(super) fn tile_data(&mut self) {
        let map = self.map;
        let mut tiles: Vec<(usize, u32)> = self
            .tileset
            .sources
            .iter()
            .flat_map(|s| s.tiles.values())
            .map(|d| (d.slot, d.id))
            .collect();
        tiles.sort_unstable();
        tiles.dedup();

        // Each tile's settings, and the properties that are custom data: the godot* settings
        // aren't, and the tile's class is unless the tile has a property of that name.
        let mut settings: Vec<Vec<(String, String)>> = Vec::new();
        let mut properties: Vec<BTreeMap<&str, Value>> = Vec::new();
        for &(slot, id) in &tiles {
            let Some(data) = map
                .tilesets
                .get(slot)
                .and_then(|t| t.tileset.tile(bt_core::TileId(id)))
            else {
                settings.push(Vec::new());
                properties.push(BTreeMap::new());
                continue;
            };
            settings.push(self.tile_settings(&data.properties, (slot, id)));
            let mut own: BTreeMap<&str, Value> = data
                .properties
                .iter()
                .filter(|(name, _)| ![GODOT_Z_INDEX, GODOT_Y_SORT_ORIGIN].contains(&name.as_str()))
                .map(|(name, p)| (name.as_str(), p.value.clone()))
                .collect();
            if !data.class.is_empty() {
                if own.contains_key("class") {
                    self.tileset.data.tally.add(crate::Code::CustomDataClass, "tiles with a property named \"class\" have it in the custom data layer \"class\", not their class: rename the property to keep both".to_owned(), (slot, id));
                } else {
                    own.insert("class", Value::String(data.class.clone()));
                }
            }
            properties.push(own);
        }

        let layers = layers(self.tileset.in_order(&tiles, &properties));
        let index: HashMap<&str, (usize, Kind)> = layers
            .iter()
            .enumerate()
            .map(|(n, (name, kind))| (name.as_str(), (n, *kind)))
            .collect();
        let project = self.project.clone();
        for ((&tile, mut lines), own) in tiles.iter().zip(settings).zip(&properties) {
            let dir = self
                .tileset
                .data
                .dirs
                .get(tile.0)
                .cloned()
                .unwrap_or_else(|| self.map_dir.clone());
            for (&name, value) in own {
                let Some(&(n, kind)) = index.get(name) else {
                    self.failures
                        .push(format!("custom data layer {name} is missing"));
                    continue;
                };
                let mut outside = false;
                let mut file =
                    |f: &str| match res_path(&project, &dir.join(f)).filter(|_| !f.is_empty()) {
                        Some(res) => res,
                        None => {
                            outside |= !f.is_empty();
                            f.to_owned()
                        }
                    };
                match in_layer(value, kind, &mut file) {
                    Some(text) => lines.push((format!("custom_data_{n}"), text)),
                    None => {
                        let message = format!(
                            "custom data layer \"{name}\" is {} layer, so its {} values are left out: give \"{name}\" one type on every tile",
                            article(kind.name()),
                            value.type_name()
                        );
                        self.tileset
                            .data
                            .tally
                            .add(crate::Code::CustomDataType, message, tile);
                    }
                }
                if outside {
                    let message = format!(
                        "custom data layer \"{name}\" has files outside the Godot project as Tiled writes them, not as res:// paths: move the files into the project"
                    );
                    self.tileset
                        .data
                        .tally
                        .add(crate::Code::CustomDataFile, message, tile);
                }
            }
            if !lines.is_empty() {
                self.tileset.data.tiles.insert(tile, lines);
            }
        }
        self.counts.custom_data_layers = layers.len();
        self.tileset.data.layers = layers;

        let set_up = self
            .setup
            .physics_layers
            .as_deref()
            .unwrap_or_default()
            .len()
            .max(1);
        if self.tileset.physics > set_up {
            let (layers, them) = match self.tileset.physics - set_up {
                1 => (format!("physics layer {set_up} has"), "it"),
                _ => (
                    format!("physics layers {set_up}–{} have", self.tileset.physics - 1),
                    "each",
                ),
            };
            let message = format!(
                "{layers} no entry in physicsLayers of better-tiled.json's godot setup: Godot gives {them} collision layer 1 and mask 1"
            );
            self.warn_once(crate::Code::PhysicsLayerUnset, message, Vec::new());
        }
        self.tally_notes();
    }

    /// A tile's own lines: `z_index` (`godotZIndex`, within Godot's range) and `y_sort_origin`
    /// (`godotYSortOrigin`).
    fn tile_settings(
        &mut self,
        properties: &Properties,
        tile: (usize, u32),
    ) -> Vec<(String, String)> {
        let mut lines = Vec::new();
        if let Some(z) = self.setting_int(properties, GODOT_Z_INDEX, tile) {
            if z.abs() > Z_INDEX {
                self.tileset.data.tally.add(crate::Code::ZindexClamped, format!("{GODOT_Z_INDEX} is outside Godot's range, -{Z_INDEX} to {Z_INDEX}, and is clamped"), tile);
            }
            lines.push(("z_index".to_owned(), z.clamp(-Z_INDEX, Z_INDEX).to_string()));
        }
        if let Some(y) = self.setting_int(properties, GODOT_Y_SORT_ORIGIN, tile) {
            lines.push((
                "y_sort_origin".to_owned(),
                y.clamp(i32::MIN.into(), i32::MAX.into()).to_string(),
            ));
        }
        lines
    }

    /// The whole number a `godot*` setting of a tile (or of one of its collision shapes) gives,
    /// taking it; a value of another type is left out, with a note.
    pub(super) fn setting_int(
        &mut self,
        properties: &Properties,
        name: &'static str,
        tile: (usize, u32),
    ) -> Option<i64> {
        match self.consumed.take(properties, name)?.value {
            Value::Int(i) => Some(i),
            Value::Float(f) if f.fract() == 0.0 && f.abs() < 1e15 => Some(f as i64),
            _ => self.wrong_type(name, "a whole number", tile),
        }
    }

    /// [`Build::setting_int`] for a number.
    pub(super) fn setting_float(
        &mut self,
        properties: &Properties,
        name: &'static str,
        tile: (usize, u32),
    ) -> Option<f64> {
        match self.consumed.take(properties, name)?.value {
            Value::Int(i) => Some(i as f64),
            Value::Float(f) if f.is_finite() => Some(f),
            _ => self.wrong_type(name, "a number", tile),
        }
    }

    /// [`Build::setting_int`] for a bool.
    pub(super) fn setting_bool(
        &mut self,
        properties: &Properties,
        name: &'static str,
        tile: (usize, u32),
    ) -> Option<bool> {
        match self.consumed.take(properties, name)?.value {
            Value::Bool(b) => Some(b),
            _ => self.wrong_type(name, "a bool", tile),
        }
    }

    fn wrong_type<T>(&mut self, name: &str, takes: &str, tile: (usize, u32)) -> Option<T> {
        self.tileset.data.tally.add(
            crate::Code::SettingType,
            format!("{name} takes {takes}: other values are left out"),
            tile,
        );
        None
    }

    /// The notes gathered about tiles, one per kind, each naming its tiles.
    pub(super) fn tally_notes(&mut self) {
        let map = self.map;
        for Tallied {
            code,
            message,
            tiles,
        } in std::mem::take(&mut self.tileset.data.tally.0)
        {
            let mut by_slot: BTreeMap<usize, Vec<u32>> = BTreeMap::new();
            for &(slot, id) in &tiles {
                by_slot.entry(slot).or_default().push(id);
            }
            let places: Vec<String> = by_slot
                .iter_mut()
                .map(|(&slot, ids)| {
                    ids.sort_unstable();
                    let name = map
                        .tilesets
                        .get(slot)
                        .map_or("", |t| t.tileset.name.as_str());
                    let count = if ids.len() == 1 {
                        "tile".to_owned()
                    } else {
                        format!("{} tiles", ids.len())
                    };
                    format!("tileset {name}: {count} {}", id_runs(ids))
                })
                .collect();
            let elements = tiles
                .iter()
                .map(|&(slot, id)| Element::Tile { tileset: slot, id })
                .collect();
            self.notes.push(Note::warning(
                code,
                format!("{message} ({})", places.join("; ")),
                elements,
            ));
        }
    }
}

/// "a bool", "an int".
fn article(name: &str) -> String {
    let an = name.starts_with(|c: char| "aeiouAEIOU".contains(c));
    format!("{} {name}", if an { "an" } else { "a" })
}

/// Notes about tiles, one per code and message, each with the tiles it is about.
#[derive(Default)]
pub(super) struct Tally(Vec<Tallied>);

struct Tallied {
    code: crate::Code,
    message: String,
    /// Slot and id.
    tiles: Vec<(usize, u32)>,
}

impl Tally {
    pub fn add(&mut self, code: crate::Code, message: String, tile: (usize, u32)) {
        match self
            .0
            .iter_mut()
            .find(|t| t.code == code && t.message == message)
        {
            Some(t) if t.tiles.contains(&tile) => {}
            Some(t) => t.tiles.push(tile),
            None => self.0.push(Tallied {
                code,
                message,
                tiles: vec![tile],
            }),
        }
    }
}
