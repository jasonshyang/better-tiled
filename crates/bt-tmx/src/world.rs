//! Worlds (`.world`): maps laid out side by side, listed or found by patterns of file names.

use std::path::Path;

use bt_core::{CustomTypes, World, WorldMap, WorldPattern};
use bt_utils::IRect;
use glam::IVec2;
use serde_json::{Map as JsonObject, Value as Json};

use crate::error::Error;
use crate::finish::PropertyDecoder;
use crate::json::{JsonReader, properties_json, qt_layout};
use crate::loader::Loaded;
use crate::paths::{file_ref, written_reference};
use crate::unfinish::raw_properties;
use crate::warning::{WarningKind, Warnings};

/// Reads a world from its text; `path` is where it is (map files resolve against its folder).
/// Its properties are decoded with `types`.
pub(crate) fn parse_world(
    text: &str,
    path: &Path,
    types: &CustomTypes,
) -> Result<Loaded<World>, Error> {
    let value = crate::json::parse_document(text, path)?;
    let object = value
        .as_object()
        .ok_or_else(|| Error::format(path, "not a Tiled world: not a JSON object"))?;
    if let Some(kind) = object
        .get("type")
        .and_then(Json::as_str)
        .filter(|k| *k != "world")
    {
        return Err(Error::format(
            path,
            format!("not a Tiled world: its type is “{kind}”"),
        ));
    }
    let mut warnings = Warnings::new(path);
    let dir = path.parent().unwrap_or(Path::new(""));
    let mut reader = WorldReader {
        json: JsonReader {
            warnings: &mut warnings,
        },
    };
    let world = reader.world_of(object, dir, types);
    Ok(Loaded {
        value: world,
        warnings: warnings.into_list(),
    })
}

struct WorldReader<'w> {
    json: JsonReader<'w>,
}

impl WorldReader<'_> {
    fn world_of(
        &mut self,
        object: &JsonObject<String, Json>,
        dir: &Path,
        types: &CustomTypes,
    ) -> World {
        self.json.check_keys(
            object,
            "the world",
            &[
                "type",
                "maps",
                "patterns",
                "properties",
                "onlyShowAdjacentMaps",
                "gridWidth",
                "gridHeight",
            ],
        );
        let mut world = World::default();
        for map in self.json.objects(object, "maps") {
            self.json.check_keys(
                map,
                "a world map",
                &["fileName", "x", "y", "width", "height"],
            );
            let name = self.json.text_of(map, "fileName");
            world.maps.push(WorldMap {
                file: file_ref(dir, &name),
                rect: IRect {
                    x: self.int(map, "x", 0),
                    y: self.int(map, "y", 0),
                    w: self.int(map, "width", 0),
                    h: self.int(map, "height", 0),
                },
            });
        }
        for pattern in self.json.objects(object, "patterns") {
            self.json.check_keys(
                pattern,
                "a world pattern",
                &[
                    "regexp",
                    "multiplierX",
                    "multiplierY",
                    "offsetX",
                    "offsetY",
                    "mapWidth",
                    "mapHeight",
                ],
            );
            let regexp = self.json.text_of(pattern, "regexp");
            let multiplier = IVec2::new(
                self.int(pattern, "multiplierX", 1),
                self.int(pattern, "multiplierY", 1),
            );
            let found = WorldPattern {
                multiplier,
                offset: IVec2::new(
                    self.int(pattern, "offsetX", 0),
                    self.int(pattern, "offsetY", 0),
                ),
                map_size: IVec2::new(
                    self.int(pattern, "mapWidth", multiplier.x.abs()),
                    self.int(pattern, "mapHeight", multiplier.y.abs()),
                ),
                regexp,
            };
            if let Some(problem) = world_pattern_problem(&found) {
                self.json.warnings.record(
                    WarningKind::InvalidPattern,
                    format!(
                        "the world pattern “{}” {problem}; it is kept but finds no maps",
                        found.regexp
                    ),
                );
            }
            world.patterns.push(found);
        }
        let raw = self.json.properties_of(object);
        world.properties = PropertyDecoder::new(types, self.json.warnings).properties_of(raw);
        world.only_show_adjacent_maps = self
            .json
            .scalar(object, "onlyShowAdjacentMaps")
            .and_then(|v| v.to_bool())
            .unwrap_or(false);
        world.grid_size = IVec2::new(
            self.int(object, "gridWidth", 0),
            self.int(object, "gridHeight", 0),
        );
        world
    }

    fn int(&mut self, object: &JsonObject<String, Json>, key: &str, default: i32) -> i32 {
        let Some(value) = self.json.scalar(object, key) else {
            return default;
        };
        match value.to_int().and_then(|v| i32::try_from(v).ok()) {
            Some(int) => int,
            None => {
                self.json.warnings.record(
                    WarningKind::InvalidValue,
                    format!(
                        "the world's “{key}” is “{}”, not a whole number; {default} is used",
                        value.to_text()
                    ),
                );
                default
            }
        }
    }
}

/// Why a pattern finds no maps, if it does not: Tiled requires two capture groups (column and
/// row), multipliers that are not zero and a positive map size.
pub fn world_pattern_problem(pattern: &WorldPattern) -> Option<String> {
    match regex::Regex::new(&pattern.regexp) {
        Err(error) => Some(format!("does not compile ({error})")),
        Ok(regex) if regex.captures_len() != 3 => Some(format!(
            "has {} capture groups, not the 2 for column and row",
            regex.captures_len() - 1
        )),
        Ok(_) if pattern.multiplier.x == 0 || pattern.multiplier.y == 0 => {
            Some("has a multiplier of 0".to_owned())
        }
        Ok(_) if pattern.map_size.x <= 0 || pattern.map_size.y <= 0 => {
            Some("has an empty map size".to_owned())
        }
        Ok(_) => None,
    }
}

/// The world's maps: those it lists, then those its patterns find among the file names of its
/// folder (sorted), with the places their captured column and row give them.
pub fn resolve_world_maps(world: &World, world_path: &Path) -> Vec<WorldMap> {
    let mut maps = world.maps.clone();
    let dir = world_path.parent().unwrap_or(Path::new(""));
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
                .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    for pattern in &world.patterns {
        if world_pattern_problem(pattern).is_some() {
            continue;
        }
        let Ok(regex) = regex::Regex::new(&pattern.regexp) else {
            continue;
        };
        for name in &names {
            let Some(captures) = regex.captures(name) else {
                continue;
            };
            let number = |i: usize| {
                captures
                    .get(i)
                    .and_then(|m| m.as_str().parse::<i32>().ok())
                    .unwrap_or(0)
            };
            maps.push(WorldMap {
                file: file_ref(dir, name),
                rect: pattern.map_rect(number(1), number(2)),
            });
        }
    }
    maps
}

/// A world's text, laid out as Tiled writes it (`QJsonDocument`): maps, patterns and properties
/// when there are any, defaults left out.
pub(crate) fn world_text(world: &World, path: &Path) -> String {
    let dir = path.parent().unwrap_or(Path::new(""));
    let mut document = JsonObject::new();
    if !world.maps.is_empty() {
        let maps = world
            .maps
            .iter()
            .map(|map| {
                let mut entry = JsonObject::new();
                entry.insert(
                    "fileName".into(),
                    Json::String(written_reference(dir, &map.file)),
                );
                entry.insert("x".into(), Json::from(map.rect.x));
                entry.insert("y".into(), Json::from(map.rect.y));
                entry.insert("width".into(), Json::from(map.rect.w));
                entry.insert("height".into(), Json::from(map.rect.h));
                Json::Object(entry)
            })
            .collect();
        document.insert("maps".into(), Json::Array(maps));
    }
    if !world.patterns.is_empty() {
        let patterns = world
            .patterns
            .iter()
            .map(|pattern| {
                let mut entry = JsonObject::new();
                entry.insert("regexp".into(), Json::String(pattern.regexp.clone()));
                let mut unless = |key: &str, value: i32, default: i32| {
                    if value != default {
                        entry.insert(key.into(), Json::from(value));
                    }
                };
                unless("multiplierX", pattern.multiplier.x, 1);
                unless("multiplierY", pattern.multiplier.y, 1);
                unless("offsetX", pattern.offset.x, 0);
                unless("offsetY", pattern.offset.y, 0);
                unless("mapWidth", pattern.map_size.x, pattern.multiplier.x.abs());
                unless("mapHeight", pattern.map_size.y, pattern.multiplier.y.abs());
                Json::Object(entry)
            })
            .collect();
        document.insert("patterns".into(), Json::Array(patterns));
    }
    if !world.properties.is_empty() {
        document.insert(
            "properties".into(),
            properties_json(&raw_properties(&world.properties), "propertytype"),
        );
    }
    document.insert("type".into(), Json::String("world".into()));
    document.insert(
        "onlyShowAdjacentMaps".into(),
        Json::Bool(world.only_show_adjacent_maps),
    );
    if world.grid_size != IVec2::ZERO {
        document.insert("gridWidth".into(), Json::from(world.grid_size.x));
        document.insert("gridHeight".into(), Json::from(world.grid_size.y));
    }
    qt_layout(&Json::Object(document))
}

#[cfg(test)]
mod tests {
    use bt_test_data::Scratch;

    use super::*;

    #[test]
    fn worlds_read_and_write_as_tiled_does() {
        let text = r#"{
    "maps": [
        {
            "fileName": "map/a.tmx",
            "height": 992,
            "width": 2560,
            "x": 1824,
            "y": 0
        }
    ],
    "onlyShowAdjacentMaps": false,
    "patterns": [
        {
            "multiplierX": 6400,
            "multiplierY": 3200,
            "regexp": "ow-p0*(\\d+)-n0*(\\d+)\\.tmx"
        },
        {
            "regexp": "broken(\\d+"
        }
    ],
    "type": "world"
}
"#;
        let scratch = Scratch::new("bt-tmx-world").unwrap();
        let path = scratch.join("w.world");
        let loaded = parse_world(text, &path, &CustomTypes::new()).unwrap();
        assert_eq!(loaded.value.maps[0].file.path, scratch.join("map/a.tmx"));
        assert_eq!(loaded.value.patterns[0].map_size, IVec2::new(6400, 3200));
        assert_eq!(loaded.warnings.len(), 1);
        assert_eq!(loaded.warnings[0].kind, WarningKind::InvalidPattern);
        assert_eq!(world_text(&loaded.value, &path), text);

        std::fs::write(scratch.join("ow-p02-n003.tmx"), "").unwrap();
        let maps = resolve_world_maps(&loaded.value, &path);
        assert_eq!(maps.len(), 2);
        assert_eq!(
            maps[1].rect,
            IRect {
                x: 12800,
                y: 9600,
                w: 6400,
                h: 3200
            }
        );
    }
}
