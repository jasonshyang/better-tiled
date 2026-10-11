//! The resolved JSON against its schema, on every Tiled example map that is not oblique and on
//! maps written here for what the examples lack, and against the geometry the window draws
//! with: cell centres from `MapGeometry`, objects from `bt-core`'s object outlines. The schema
//! is checked by a validator of the keywords it uses, written here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_core::{
    Cell, CustomTypes, Map, MapGeometry, MapObject, Orientation, Outline, Shape, TileId,
    TilesetSlot, object_outline, object_pixel_outline, tile_object_box,
};
use bt_test_data::Scratch;
use bt_test_data::fixtures::{Atlas, MapBuilder};
use bt_tmx::Loader;
use bt_utils::{DRect, IRect};
use glam::{DVec2, IVec2, UVec2};
use serde_json::{Value, json};

use super::layout::json_text;
use super::request::Resolved;
use super::{RESOLVED_FORMAT, RESOLVED_SCHEMA, ResolvedError, ResolvedRequest, ResolvedWarning};
use crate::feet::{pixel_to_point, standing_cell};

// The validator.

/// The keywords the validator checks; the schema may use no others.
const CHECKED: [&str; 12] = [
    "type",
    "required",
    "properties",
    "additionalProperties",
    "patternProperties",
    "items",
    "minItems",
    "maxItems",
    "enum",
    "const",
    "pattern",
    "$ref",
];

/// What a schema may say that checks nothing.
const ANNOTATIONS: [&str; 5] = ["$schema", "$id", "title", "description", "$defs"];

/// The schema, read.
fn schema() -> Value {
    serde_json::from_str(RESOLVED_SCHEMA).unwrap()
}

/// Whether `text` matches `pattern`: the patterns the schema uses, and no others.
fn matches_pattern(pattern: &str, text: &str) -> bool {
    match pattern {
        "^#[0-9a-f]{6}([0-9a-f]{2})?$" => {
            let hex = |s: &str| {
                s.chars()
                    .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
            };
            text.strip_prefix('#')
                .is_some_and(|digits| matches!(digits.len(), 6 | 8) && hex(digits))
        }
        "^[1-9][0-9]*$" => {
            !text.is_empty() && !text.starts_with('0') && text.chars().all(|c| c.is_ascii_digit())
        }
        other => panic!("the validator does not know the pattern {other}"),
    }
}

/// Whether `value` is of the JSON Schema type `kind`.
fn is_of_type(kind: &str, value: &Value) -> bool {
    match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        "number" => value.is_number(),
        "integer" => value.as_f64().is_some_and(|n| n.fract() == 0.0),
        other => panic!("the validator does not know the type {other}"),
    }
}

/// Checks `value` against `schema` (a part of `root`) at `at`, adding what is wrong to
/// `errors`.
fn validate(value: &Value, schema: &Value, root: &Value, at: &str, errors: &mut Vec<String>) {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let name = reference
            .strip_prefix("#/$defs/")
            .unwrap_or_else(|| panic!("{at}: only #/$defs/ references are checked: {reference}"));
        let target = root["$defs"]
            .get(name)
            .unwrap_or_else(|| panic!("{at}: no definition {name}"));
        validate(value, target, root, at, errors);
    }
    if let Some(expected) = schema.get("const")
        && value != expected
    {
        errors.push(format!("{at}: {value} is not {expected}"));
    }
    if let Some(options) = schema.get("enum").and_then(Value::as_array)
        && !options.contains(value)
    {
        errors.push(format!("{at}: {value} is none of {options:?}"));
    }
    if let Some(kinds) = schema.get("type") {
        let kinds: Vec<&str> = match kinds {
            Value::String(kind) => vec![kind.as_str()],
            Value::Array(kinds) => kinds.iter().filter_map(Value::as_str).collect(),
            other => panic!("{at}: type {other}"),
        };
        if !kinds.iter().any(|kind| is_of_type(kind, value)) {
            errors.push(format!("{at}: {value} is not of type {kinds:?}"));
            return;
        }
    }
    if let Some(pattern) = schema.get("pattern").and_then(Value::as_str)
        && let Some(text) = value.as_str()
        && !matches_pattern(pattern, text)
    {
        errors.push(format!("{at}: {text} does not match {pattern}"));
    }
    if let Some(object) = value.as_object() {
        for key in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let key = key.as_str().unwrap();
            if !object.contains_key(key) {
                errors.push(format!("{at}: {key} is missing"));
            }
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        let patterns = schema.get("patternProperties").and_then(Value::as_object);
        for (key, item) in object {
            let path = format!("{at}.{key}");
            if let Some(property) = properties.and_then(|p| p.get(key)) {
                validate(item, property, root, &path, errors);
                continue;
            }
            let matching: Vec<&Value> = patterns
                .into_iter()
                .flatten()
                .filter(|(pattern, _)| matches_pattern(pattern, key))
                .map(|(_, schema)| schema)
                .collect();
            if !matching.is_empty() {
                for schema in matching {
                    validate(item, schema, root, &path, errors);
                }
                continue;
            }
            match schema.get("additionalProperties") {
                Some(Value::Bool(false)) => errors.push(format!("{path}: not in the schema")),
                Some(extra @ Value::Object(_)) => validate(item, extra, root, &path, errors),
                _ => {}
            }
        }
    }
    if let Some(items) = value.as_array() {
        if let Some(min) = schema.get("minItems").and_then(Value::as_u64)
            && (items.len() as u64) < min
        {
            errors.push(format!("{at}: fewer than {min} items"));
        }
        if let Some(max) = schema.get("maxItems").and_then(Value::as_u64)
            && (items.len() as u64) > max
        {
            errors.push(format!("{at}: more than {max} items"));
        }
        if let Some(item_schema) = schema.get("items") {
            for (index, item) in items.iter().enumerate() {
                validate(item, item_schema, root, &format!("{at}[{index}]"), errors);
            }
        }
    }
}

/// Fails unless `document` matches the schema.
fn assert_matches_schema(document: &Value, what: &str) {
    let schema = schema();
    let mut errors = Vec::new();
    validate(document, &schema, &schema, "$", &mut errors);
    assert!(
        errors.is_empty(),
        "{what} does not match the schema:\n{}",
        errors
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Walks a schema checking that it uses only the keywords the validator checks, that every
/// reference has its definition, and that every field says what it is (on itself or on the
/// definition it refers to).
fn check_schema_node(node: &Value, root: &Value, at: &str, problems: &mut Vec<String>) {
    let Some(object) = node.as_object() else {
        return;
    };
    for (key, value) in object {
        if !CHECKED.contains(&key.as_str()) && !ANNOTATIONS.contains(&key.as_str()) {
            problems.push(format!("{at}: the validator does not check {key}"));
        }
        match key.as_str() {
            "properties" | "patternProperties" | "$defs" => {
                for (name, field) in value.as_object().into_iter().flatten() {
                    let path = format!("{at}.{key}.{name}");
                    if key != "$defs" && !describes(field, root) {
                        problems.push(format!("{path}: no description"));
                    }
                    check_schema_node(field, root, &path, problems);
                }
            }
            "items" | "additionalProperties" => {
                check_schema_node(value, root, &format!("{at}.{key}"), problems);
            }
            "$ref" => {
                let name = value.as_str().and_then(|r| r.strip_prefix("#/$defs/"));
                if name.and_then(|name| root["$defs"].get(name)).is_none() {
                    problems.push(format!("{at}: {value} names no definition"));
                }
            }
            _ => {}
        }
    }
}

/// Whether a field has a description, or refers to a definition that has one.
fn describes(field: &Value, root: &Value) -> bool {
    field.get("description").is_some()
        || field
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|r| r.strip_prefix("#/$defs/"))
            .and_then(|name| root["$defs"].get(name))
            .is_some_and(|definition| definition.get("description").is_some())
}

#[test]
fn the_schema_is_checked_whole_and_says_what_every_field_is() {
    let schema = schema();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["properties"]["format"]["const"], RESOLVED_FORMAT);
    let mut problems = Vec::new();
    check_schema_node(&schema, &schema, "$", &mut problems);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn the_validator_finds_what_the_schema_forbids() {
    let schema = schema();
    let errors_of = |value: &Value, definition: &str| {
        let mut errors = Vec::new();
        validate(
            value,
            &schema["$defs"][definition],
            &schema,
            "$",
            &mut errors,
        );
        errors
    };
    assert!(errors_of(&json!("#a0b1c2"), "color").is_empty());
    assert!(errors_of(&json!("#a0b1c2ff"), "color").is_empty());
    assert_eq!(errors_of(&json!("#A0B1C2"), "color").len(), 1);
    assert_eq!(errors_of(&json!([1, 2, 3]), "point").len(), 1, "too many");
    assert_eq!(errors_of(&json!([1.5, 2]), "cell").len(), 1, "not whole");
    assert!(errors_of(&json!({"top": null, "left": "grass"}), "lanes").is_empty());
    assert_eq!(errors_of(&json!({"middle": "grass"}), "lanes").len(), 1);
    let frame = json!({"tile_id": 1});
    assert_eq!(errors_of(&frame, "frame"), ["$: duration_ms is missing"]);
    let mut errors = Vec::new();
    let tiles = json!({"1": {"tileset": "t"}, "01": {}});
    validate(
        &tiles,
        &schema["properties"]["tiles"],
        &schema,
        "$",
        &mut errors,
    );
    assert!(
        errors.iter().any(|e| e.contains("$.01: not in the schema")),
        "{errors:?}"
    );
    assert!(
        errors.iter().any(|e| e.contains("$.1: tile_id is missing")),
        "{errors:?}"
    );
}

// Reading the JSON back.

/// The JSON of `resolved`, as written and read back.
fn written(resolved: &Resolved) -> Value {
    serde_json::from_str(&json_text(&resolved.document).unwrap()).unwrap()
}

fn pair(value: &Value) -> DVec2 {
    DVec2::new(value[0].as_f64().unwrap(), value[1].as_f64().unwrap())
}

fn points(value: &Value) -> Vec<DVec2> {
    value.as_array().unwrap().iter().map(pair).collect()
}

fn assert_near(found: DVec2, expected: DVec2, what: &str) {
    assert!(
        found.distance(expected) < 1e-6,
        "{what}: {found} is not {expected}"
    );
}

/// The layer whose path is `path`.
fn layer<'d>(document: &'d Value, path: &str) -> &'d Value {
    document["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["path"] == path)
        .unwrap_or_else(|| panic!("no layer {path}"))
}

/// The objects written, by id.
fn objects_by_id(document: &Value) -> HashMap<u64, &Value> {
    document["layers"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|layer| layer["objects"].as_array().into_iter().flatten())
        .map(|object| (object["id"].as_u64().unwrap(), object))
        .collect()
}

/// The tile a key stands for.
fn tile<'d>(document: &'d Value, key: &Value) -> &'d Value {
    &document["tiles"][key.as_u64().unwrap().to_string()]
}

/// The tile in a cell of a tile layer.
fn tile_at<'d>(document: &'d Value, path: &str, cell: IVec2) -> &'d Value {
    let origin = pair(&document["map"]["origin_cell"]).as_ivec2();
    let at = cell - origin;
    tile(
        document,
        &layer(document, path)["rows"][at.y as usize][at.x as usize],
    )
}

/// The centre of `cell` as the JSON's `cell_centres` says.
fn centre_from(centres: &Value, cell: IVec2) -> DVec2 {
    let mut centre = pair(&centres["first"])
        + pair(&centres["column_step"]) * f64::from(cell.x)
        + pair(&centres["row_step"]) * f64::from(cell.y);
    if cell.y.rem_euclid(2) == 1 && centres.get("odd_rows_shift").is_some() {
        centre += pair(&centres["odd_rows_shift"]);
    }
    if cell.x.rem_euclid(2) == 1 && centres.get("odd_columns_shift").is_some() {
        centre += pair(&centres["odd_columns_shift"]);
    }
    centre
}

/// The middle of a cell's outline, as `MapGeometry` draws it.
fn outline_middle(geometry: &MapGeometry, cell: IVec2) -> DVec2 {
    let outline = DRect::from_points(geometry.tile_polygon(cell)).unwrap();
    (outline.min + outline.max) * 0.5
}

/// Fails unless the JSON of `map` puts its cells where `MapGeometry` does and its objects
/// where the canvas draws them: at their projected position moved by their layer's offset,
/// with the outline `object_outline` gives, the box `tile_object_box` gives, and points in the
/// cell bt stands them in.
fn assert_drawn_as_the_canvas_draws(map: &Map, document: &Value, what: &str) {
    let geometry = map.geometry();
    let centres = &document["map"]["cell_centres"];
    for cell in IRect::from_min_size(IVec2::splat(-4), IVec2::splat(11)).points() {
        let middle = outline_middle(&geometry, cell);
        assert_near(
            centre_from(centres, cell),
            middle,
            &format!("{what}: cell {cell}"),
        );
        assert_eq!(
            geometry.tile_at(middle),
            cell,
            "{what}: the centre of {cell} is in it"
        );
    }
    let written = objects_by_id(document);
    let isometric = map.orientation == Orientation::Isometric;
    map.for_each_layer(|layer, _| {
        let Some(objects) = layer.as_object_layer() else {
            return;
        };
        let offset = map.effective_layer(layer.id).unwrap().offset;
        let shift = geometry.screen_to_pixel(offset) - geometry.screen_to_pixel(DVec2::ZERO);
        for object in objects.objects.iter() {
            let what = format!("{what}: object #{}", object.id.0);
            let Some(json) = written.get(&u64::from(object.id.0)) else {
                panic!("{what} is not written");
            };
            let tileset = object.tile.and_then(|cell| map.tileset_of(cell));
            let screen = pair(&json["screen_pos"]);
            assert_near(screen, geometry.pixel_to_screen(object.pos) + offset, &what);
            assert_near(pair(&json["pos"]), object.pos + shift, &what);
            assert_near(geometry.pixel_to_screen(pair(&json["pos"])), screen, &what);
            match object_outline(&geometry, object, tileset) {
                Outline::Area(drawn) | Outline::Path(drawn) => {
                    let listed = points(&json["outline"]);
                    assert_eq!(listed.len(), drawn.len(), "{what}");
                    for (found, expected) in listed.iter().zip(&drawn) {
                        assert_near(*found, *expected + offset, &what);
                    }
                }
                Outline::Point(_) => assert!(json.get("outline").is_none(), "{what}: {json}"),
            }
            match object_pixel_outline(&geometry, object, tileset) {
                Outline::Area(drawn) | Outline::Path(drawn) if isometric => {
                    let listed = points(&json["pixel_outline"]);
                    for (found, expected) in listed.iter().zip(&drawn) {
                        assert_near(*found, *expected + shift, &what);
                    }
                }
                _ => assert!(json.get("pixel_outline").is_none(), "{what}: {json}"),
            }
            if let Some(image) = tile_object_box(&geometry, object, tileset) {
                let written = &json["box"];
                assert_near(pair(written), image.min, &what);
                assert_near(
                    DVec2::new(written[2].as_f64().unwrap(), written[3].as_f64().unwrap()),
                    image.size(),
                    &what,
                );
                assert!(json["tile"].is_u64() || tileset.is_none(), "{what}: {json}");
            }
            if object.tile.is_none() && matches!(object.shape, Shape::Point) {
                let stands = standing_cell(pixel_to_point(&geometry, object.pos + shift));
                assert_eq!(pair(&json["cell"]).as_ivec2(), stands, "{what}");
            }
        }
    });
}

// Tiled's examples.

/// The maps under `folder` and its folders: Tiled's XML maps and the JSON files that are maps.
fn maps_under(folder: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            maps_under(&path, out);
            continue;
        }
        let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase());
        let is_map = match extension.as_deref() {
            Some("tmx" | "tmj") => true,
            Some("json") => {
                let text = std::fs::read_to_string(&path).unwrap();
                serde_json::from_str::<Value>(&text)
                    .is_ok_and(|value| value.get("type").and_then(Value::as_str) == Some("map"))
            }
            _ => false,
        };
        if is_map {
            out.push(path);
        }
    }
}

/// The custom types of the nearest project above `map`, up to `root`.
fn project_types(map: &Path, root: &Path) -> CustomTypes {
    let mut folder = map.parent();
    while let Some(dir) = folder {
        let mut projects: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|e| e == "tiled-project"))
            .collect();
        projects.sort();
        if let Some(project) = projects.first() {
            return bt_tmx::load_project(project).unwrap().value.custom_types;
        }
        if dir == root {
            break;
        }
        folder = dir.parent();
    }
    CustomTypes::new()
}

/// A map read with `types`.
fn load(path: &Path, types: &CustomTypes) -> Map {
    let mut loader = Loader::builder()
        .custom_types(Arc::new(types.clone()))
        .build();
    loader.load_map(path).unwrap().value
}

#[test]
fn every_tiled_example_exports_matches_the_schema_and_is_drawn_as_the_canvas_draws_it() {
    let Some(root) = bt_test_data::test_data("reference/tiled/examples") else {
        return;
    };
    let root = std::fs::canonicalize(root).unwrap();
    let mut maps = Vec::new();
    maps_under(&root, &mut maps);
    assert!(maps.len() >= 20, "{maps:?}");
    let scratch = Scratch::new("bt-resolved-examples").unwrap();
    let mut orientations = Vec::new();
    for path in &maps {
        let name = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('/', "-");
        let types = project_types(path, &root);
        let map = load(path, &types);
        let out = scratch.join(format!("{name}.json"));
        let request = ResolvedRequest::new(&map, path).custom_types(&types);
        if map.orientation == Orientation::Oblique {
            assert!(matches!(
                request.resolve_for(&out),
                Err(ResolvedError::Oblique { .. })
            ));
            continue;
        }
        orientations.push(map.orientation);
        let resolved = request.resolve_for(&out).unwrap();
        let document = written(&resolved);
        assert_matches_schema(&document, &name);
        assert_drawn_as_the_canvas_draws(&map, &document, &name);
        // Every image named is there, relative to the JSON file.
        let mut images = Vec::new();
        collect_images(&document, &mut images);
        for image in images {
            assert!(
                scratch.path().join(&image).exists(),
                "{name}: {image} is not relative to the JSON file"
            );
        }
        assert_eq!(
            scratch
                .path()
                .join(document["map"]["source"].as_str().unwrap())
                .canonicalize()
                .unwrap(),
            *path,
            "{name}"
        );
    }
    for orientation in [
        Orientation::Orthogonal,
        Orientation::Isometric,
        Orientation::Staggered,
        Orientation::Hexagonal,
    ] {
        assert!(
            orientations.contains(&orientation),
            "no {orientation:?} example"
        );
    }
}

/// Every `image` field in a JSON value.
fn collect_images(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(fields) => {
            for (key, field) in fields {
                match (key.as_str(), field) {
                    ("image", Value::String(image)) => out.push(image.clone()),
                    _ => collect_images(field, out),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| collect_images(item, out)),
        _ => {}
    }
}

#[test]
fn the_examples_grids_cells_and_templates_are_written_out() {
    let Some(root) = bt_test_data::test_data("reference/tiled/examples") else {
        return;
    };
    let root = std::fs::canonicalize(root).unwrap();
    let scratch = Scratch::new("bt-resolved-example-grids").unwrap();
    let resolve = |relative: &str| {
        let path = root.join(relative);
        let types = project_types(&path, &root);
        let map = load(&path, &types);
        let resolved = ResolvedRequest::new(&map, &path)
            .custom_types(&types)
            .resolve_for(&scratch.join("map.json"))
            .unwrap();
        (map, written(&resolved))
    };
    // A staggered infinite map: its used area from its origin cell, its rows shifted.
    let (map, staggered) = resolve("isometric_staggered_grass_and_water.tmx");
    let info = &staggered["map"];
    assert_eq!(info["orientation"], "staggered");
    assert_eq!(
        (
            info["stagger_axis"].as_str(),
            info["stagger_index"].as_str()
        ),
        (Some("y"), Some("odd"))
    );
    assert!(info.get("hex_side_length").is_none());
    assert_eq!(info["infinite"], true);
    let origin = pair(&info["origin_cell"]).as_ivec2();
    let size = IVec2::new(
        info["columns"].as_i64().unwrap() as i32,
        info["rows"].as_i64().unwrap() as i32,
    );
    let mut used: Option<IRect> = None;
    map.for_each_layer(|layer, _| {
        if let Some(bounds) = layer.as_tile_layer().and_then(|t| t.used_bounds()) {
            used = Some(used.map_or(bounds, |u| u.union(bounds)));
        }
    });
    assert_eq!(IRect::from_min_size(origin, size), used.unwrap());
    let screen = map.geometry().bounding_rect(used.unwrap());
    assert_eq!(
        info["screen_rect"],
        json!([screen.x, screen.y, screen.w, screen.h])
    );
    assert!(info["cell_centres"].get("odd_rows_shift").is_some());
    let rows = layer(&staggered, &map.layers[0].name)["rows"]
        .as_array()
        .unwrap();
    assert_eq!(rows.len(), size.y as usize);
    assert!(
        rows.iter()
            .all(|row| row.as_array().unwrap().len() == size.x as usize)
    );
    // A hexagonal map staggered along x.
    let (_, hexagonal) = resolve("test_hexagonal_tile_60x60x30.tmx");
    let info = &hexagonal["map"];
    assert_eq!(
        (
            info["orientation"].as_str(),
            info["hex_side_length"].as_i64()
        ),
        (Some("hexagonal"), Some(30))
    );
    assert_eq!(info["stagger_axis"], "x");
    assert!(info["cell_centres"].get("odd_columns_shift").is_some());
    assert_eq!(
        info["screen_rect"][0], 0,
        "a finite map's cells start at 0,0"
    );
    // An isometric map: cell 0,0 is the diamond's top corner, half a tile below it its centre.
    let (_, isometric) = resolve("isometric_grass_and_water.tmx");
    let info = &isometric["map"];
    assert_eq!(info["cell_centres"]["first"], json!([25 * 32, 16]));
    assert_eq!(info["cell_centres"]["column_step"], json!([32, 16]));
    assert_eq!(info["cell_centres"]["row_step"], json!([-32, 16]));
    // Templates detached: the hero is written whole, with what its template gives it.
    let (_, sandbox) = resolve("sticker-knight/map/sandbox.tmx");
    let objects = objects_by_id(&sandbox);
    let hero = objects[&58];
    assert_eq!(
        (
            hero["name"].as_str(),
            hero["type"].as_str(),
            hero["kind"].as_str()
        ),
        (Some("hero"), Some("hero"), Some("tile"))
    );
    assert!(
        hero["template"]
            .as_str()
            .unwrap()
            .ends_with("sticker-knight/map/templates/hero.tx")
    );
    assert_eq!(hero["size"], json!([128, 160]));
    assert!(
        tile(&sandbox, &hero["tile"])["image"]
            .as_str()
            .unwrap()
            .ends_with(".png")
    );
    assert!(objects.values().all(|o| o.get("gid").is_none()));
}

// Maps written here, for what the examples lack.

/// A folder with a tileset file in `art/` (an atlas of 8 tiles 16 pixels square, tile 1 a
/// door with a class, a file property, an animation and a collision shape, tiles 0 and 2 in
/// the terrain set "Land" of grass and water), a template in `maps/`, and `out/` for JSON.
fn fixture(name: &str) -> Scratch {
    let scratch = Scratch::new(name).unwrap();
    for folder in ["art", "maps", "out"] {
        std::fs::create_dir_all(scratch.join(folder)).unwrap();
    }
    Atlas::builder()
        .name("tiles")
        .tile_size(16, 16)
        .grid(4, 2)
        .build_in(&scratch.join("art"))
        .unwrap();
    std::fs::write(scratch.join("art/tiles.tsx"), TILESET).unwrap();
    std::fs::write(scratch.join("maps/chest.tx"), TEMPLATE).unwrap();
    scratch
}

const TILESET: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.11.0" name="Tiles" tilewidth="16" tileheight="16" tilecount="8" columns="4">
 <image source="tiles.png" width="64" height="32"/>
 <tile id="1" type="Door">
  <properties>
   <property name="locked" type="bool" value="true"/>
   <property name="script" type="file" value="scripts/door.lua"/>
  </properties>
  <objectgroup draworder="index" id="2">
   <object id="1" x="2" y="3" width="10" height="12"/>
  </objectgroup>
  <animation>
   <frame tileid="1" duration="100"/>
   <frame tileid="2" duration="200"/>
  </animation>
 </tile>
 <wangsets>
  <wangset name="Land" type="corner" tile="-1">
   <wangcolor name="Grass" color="#00ff00" tile="-1" probability="1"/>
   <wangcolor name="Water" color="#0000ff" tile="-1" probability="1"/>
   <wangtile tileid="0" wangid="0,1,0,1,0,1,0,1"/>
   <wangtile tileid="2" wangid="0,2,0,1,0,1,0,1"/>
  </wangset>
 </wangsets>
</tileset>
"##;

const TEMPLATE: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<template>
 <tileset firstgid="1" source="../art/tiles.tsx"/>
 <object name="chest" type="Chest" gid="4" width="16" height="16">
  <properties>
   <property name="loot" type="int" value="10"/>
  </properties>
 </object>
</template>
"##;

/// A map of every kind of object, turned and not, in a layer moved by (-2, 6) in a group moved
/// by (3, 4), with flipped cells, a template instance, an image layer and a `file` property.
/// `{grid}` is the map's grid and `{more}` more layers.
const SHAPES: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" tiledversion="1.11.0" {grid} renderorder="right-down" width="6" height="5" infinite="0" nextlayerid="9" nextobjectid="40">
 <properties>
  <property name="music" type="file" value="music/theme.ogg"/>
 </properties>
 <tileset firstgid="1" source="../art/tiles.tsx"/>
 <layer id="1" name="ground" width="6" height="5">
  <data encoding="csv">
1,2,3,0,0,0,
2147483651,1073741825,536870913,0,0,0,
1,1,1,1,1,1,
1,1,1,1,1,1,
1,1,1,1,1,8
</data>
 </layer>
 <group id="2" name="things" offsetx="3" offsety="4" opacity="0.5">
  <objectgroup id="3" name="objects" offsetx="-2" offsety="6" color="#ff0000">
   <object id="1" name="box" x="10" y="20" width="40" height="30" rotation="30"/>
   <object id="2" name="oval" x="50" y="40" width="24" height="16" rotation="45">
    <ellipse/>
   </object>
   <object id="3" name="spot" x="70" y="9">
    <point/>
   </object>
   <object id="4" name="pill" x="5" y="60" width="30" height="12" rotation="-20">
    <capsule/>
   </object>
   <object id="5" name="shard" x="100" y="30" rotation="15">
    <polygon points="0,0 20,-10 30,15"/>
   </object>
   <object id="6" name="trail" x="120" y="50">
    <polyline points="0,0 15,5 25,-8"/>
   </object>
   <object id="7" name="sign" x="8" y="80" width="60" height="20" rotation="10">
    <text wrap="1" color="#80102030" halign="center">hi</text>
   </object>
   <object id="8" name="door" gid="2" x="40" y="100" width="32" height="32" rotation="90"/>
   <object id="9" name="mirrored" gid="2147483651" x="80" y="64" width="16" height="16"/>
   <object id="10" name="marker" x="33" y="33"/>
   <object id="11" template="chest.tx" x="64" y="64">
    <properties>
     <property name="opened" type="bool" value="true"/>
    </properties>
   </object>
   <object id="12" name="far" x="-17" y="-5">
    <point/>
   </object>
  </objectgroup>
 </group>
 <imagelayer id="4" name="sky" offsetx="5" offsety="7" parallaxx="0.5" repeatx="1">
  <image source="../art/tiles.png" width="64" height="32"/>
 </imagelayer>{more}
</map>
"##;

/// The grids [`SHAPES`] is drawn on: odd sizes, both stagger axes and indices, and hexagons of
/// several proportions.
const GRIDS: [&str; 8] = [
    r#"orientation="orthogonal" tilewidth="32" tileheight="16""#,
    r#"orientation="isometric" tilewidth="64" tileheight="32""#,
    r#"orientation="isometric" tilewidth="63" tileheight="31""#,
    r#"orientation="staggered" tilewidth="64" tileheight="32" staggeraxis="y" staggerindex="odd""#,
    r#"orientation="staggered" tilewidth="63" tileheight="31" staggeraxis="x" staggerindex="even""#,
    r#"orientation="hexagonal" tilewidth="14" tileheight="12" staggeraxis="y" staggerindex="odd" hexsidelength="6""#,
    r#"orientation="hexagonal" tilewidth="14" tileheight="14" staggeraxis="y" staggerindex="even" hexsidelength="7""#,
    r#"orientation="hexagonal" tilewidth="15" tileheight="13" staggeraxis="x" staggerindex="even" hexsidelength="7""#,
];

/// [`SHAPES`] on `grid` with `more` layers, written as `maps/<name>.tmx` in `scratch`.
fn shapes_map(scratch: &Scratch, name: &str, grid: &str, more: &str) -> PathBuf {
    let path = scratch.join(format!("maps/{name}.tmx"));
    std::fs::write(
        &path,
        SHAPES.replace("{grid}", grid).replace("{more}", more),
    )
    .unwrap();
    path
}

/// The resolved JSON of the map at `path`, for a file in `out/`.
fn resolve_file(scratch: &Scratch, path: &Path, types: &CustomTypes) -> (Map, Resolved) {
    let map = load(path, types);
    let resolved = ResolvedRequest::new(&map, path)
        .custom_types(types)
        .resolve_for(&scratch.join("out/map.json"))
        .unwrap();
    (map, resolved)
}

#[test]
fn objects_are_where_the_canvas_draws_them_and_points_at_centres_are_in_their_cells() {
    let scratch = fixture("bt-resolved-grids");
    for (index, grid) in GRIDS.iter().enumerate() {
        // Points at the centres of the cells around the origin, on a layer of their own.
        let plain = load(
            &shapes_map(&scratch, &format!("plain{index}"), grid, ""),
            &CustomTypes::new(),
        );
        let geometry = plain.geometry();
        let cells: Vec<IVec2> = IRect::from_min_size(IVec2::splat(-3), IVec2::splat(7))
            .points()
            .collect();
        let mut more = String::from("\n <objectgroup id=\"8\" name=\"centres\">");
        for (k, cell) in cells.iter().enumerate() {
            let at = geometry.screen_to_pixel(outline_middle(&geometry, *cell));
            more.push_str(&format!(
                "\n  <object id=\"{}\" x=\"{}\" y=\"{}\">\n   <point/>\n  </object>",
                100 + k,
                at.x,
                at.y
            ));
        }
        more.push_str("\n </objectgroup>");
        let path = shapes_map(&scratch, &format!("grid{index}"), grid, &more);
        let (map, resolved) = resolve_file(&scratch, &path, &CustomTypes::new());
        let document = written(&resolved);
        assert_matches_schema(&document, grid);
        assert_drawn_as_the_canvas_draws(&map, &document, grid);
        let centres = layer(&document, "centres")["objects"].as_array().unwrap();
        for (k, cell) in cells.iter().enumerate() {
            assert_eq!(
                pair(&centres[k]["cell"]).as_ivec2(),
                *cell,
                "{grid}: the point at the centre of {cell}"
            );
        }
        // Tile and text objects stand upright; shapes are projected on isometric maps.
        let objects = objects_by_id(&document);
        assert_eq!(objects[&7]["box"], json!([0, 0, 60, 20]), "{grid}");
        assert_eq!(objects[&8]["size"], json!([32, 32]), "{grid}");
        assert!(objects[&3].get("outline").is_none() && objects[&10].get("outline").is_none());
        assert_eq!(
            objects[&6]["outline"].as_array().unwrap().len(),
            3,
            "an open line"
        );
        assert_eq!(
            objects[&2]["outline"].as_array().unwrap().len(),
            64,
            "an ellipse"
        );
        let isometric = map.orientation == Orientation::Isometric;
        assert_eq!(
            objects[&1].get("pixel_outline").is_some(),
            isometric,
            "{grid}"
        );
    }
}

#[test]
fn cells_and_tiles_have_keys_flips_as_booleans_and_their_terrains_and_classes_by_name() {
    let scratch = fixture("bt-resolved-tiles");
    let path = shapes_map(&scratch, "level", GRIDS[0], "");
    let (_, resolved) = resolve_file(&scratch, &path, &CustomTypes::new());
    assert!(resolved.warnings.is_empty(), "{:?}", resolved.warnings);
    let document = written(&resolved);
    assert_matches_schema(&document, "level");
    assert_eq!(document["format"], RESOLVED_FORMAT);
    let ground = layer(&document, "ground");
    assert_eq!(ground["filled_cells"], 24);
    assert_eq!(ground["rows"][0][3], 0, "an empty cell");
    // The same tile mirrored another way is another key.
    let plain = tile_at(&document, "ground", IVec2::new(2, 0));
    let mirrored = tile_at(&document, "ground", IVec2::new(0, 1));
    assert_ne!(ground["rows"][0][2], ground["rows"][1][0]);
    assert_eq!(
        (plain["tileset"].as_str(), plain["tile_id"].as_u64()),
        (Some("tiles"), Some(2))
    );
    assert_eq!(
        (mirrored["tileset"].as_str(), mirrored["tile_id"].as_u64()),
        (Some("tiles"), Some(2))
    );
    let flips = |tile: &Value| {
        ["flip_h", "flip_v", "flip_d", "rotated_hex_120"].map(|flip| tile[flip].as_bool().unwrap())
    };
    assert_eq!(flips(plain), [false; 4]);
    assert_eq!(flips(mirrored), [true, false, false, false]);
    assert_eq!(
        flips(tile_at(&document, "ground", IVec2::new(1, 1))),
        [false, true, false, false]
    );
    assert_eq!(
        flips(tile_at(&document, "ground", IVec2::new(2, 1))),
        [false, false, true, false]
    );
    // Terrains by name, lanes as drawn: mirrored, the water moves to the top-left.
    assert_eq!(plain["terrain"], "grass", "three corners of four");
    let lanes = |tile: &Value| tile["terrains"][0]["lanes"].clone();
    assert_eq!(plain["terrains"][0]["set"], "Land");
    assert_eq!(
        lanes(plain),
        json!({"top-left": "grass", "top-right": "water", "bottom-left": "grass", "bottom-right": "grass"})
    );
    assert_eq!(lanes(mirrored)["top-left"], "water");
    assert_eq!(lanes(mirrored)["top-right"], "grass");
    let unlabelled = tile_at(&document, "ground", IVec2::new(5, 4));
    assert!(unlabelled.get("terrain").is_none() && unlabelled.get("terrains").is_none());
    // A tile's place, class, properties (its file relative to the JSON), animation and shapes.
    let door = tile_at(&document, "ground", IVec2::new(1, 0));
    assert_eq!(door["image"], "../art/tiles.png");
    assert_eq!(door["source_rect"], json!([16, 0, 16, 16]));
    assert_eq!(door["atlas"], json!([1, 0]));
    assert_eq!(door["type"], "Door");
    assert_eq!(
        door["properties"],
        json!({"locked": true, "script": "../art/scripts/door.lua"})
    );
    assert_eq!(
        door["animation"],
        json!([
            {"tile_id": 1, "image": "../art/tiles.png", "source_rect": [16, 0, 16, 16], "atlas": [1, 0], "duration_ms": 100},
            {"tile_id": 2, "image": "../art/tiles.png", "source_rect": [32, 0, 16, 16], "atlas": [2, 0], "duration_ms": 200}
        ])
    );
    assert_eq!(door["collision"][0]["kind"], "rectangle");
    assert_eq!(door["collision"][0]["size"], json!([10, 12]));
    assert!(door["collision"][0].get("screen_pos").is_none());
    // The tileset, the map's file property relative to the JSON file.
    assert_eq!(
        document["tilesets"],
        json!([{
            "name": "tiles", "tiled_name": "Tiles", "kind": "atlas", "image": "../art/tiles.png",
            "image_size": [64, 32], "tile_size": [16, 16], "tile_count": 8, "columns": 4, "rows": 2,
            "margin": 0, "spacing": 0, "tile_offset": [0, 0], "type": "", "properties": {}
        }])
    );
    assert_eq!(
        document["map"]["properties"]["music"],
        "../maps/music/theme.ogg"
    );
    assert_eq!(document["map"]["source"], "../maps/level.tmx");
    // Tile objects name their tile unflipped and say their flips.
    let objects = objects_by_id(&document);
    let mirrored_object = objects[&9];
    assert_eq!(
        (
            mirrored_object["flip_h"].as_bool(),
            mirrored_object["flip_v"].as_bool()
        ),
        (Some(true), Some(false))
    );
    assert_eq!(flips(tile(&document, &mirrored_object["tile"])), [false; 4]);
    assert_eq!(
        objects[&8]["type"], "Door",
        "a tile object's type is its tile's class"
    );
    assert_eq!(
        objects[&8]["properties"]["locked"], true,
        "and it has its tile's properties"
    );
    // Layers with their groups' settings.
    let things = layer(&document, "things/objects");
    assert_eq!(
        (things["opacity"].as_f64(), things["offset"].clone()),
        (Some(0.5), json!([1, 10]))
    );
    assert_eq!(things["color"], "#ff0000");
    assert_eq!(layer(&document, "things")["kind"], "group");
    let sky = layer(&document, "sky");
    assert_eq!(
        (sky["image"].as_str(), sky["parallax"].clone()),
        (Some("../art/tiles.png"), json!([0.5, 1]))
    );
    assert_eq!(
        (sky["repeat_x"].as_bool(), sky["image_size"].clone()),
        (Some(true), json!([64, 32]))
    );
    let text = &objects[&7]["text"];
    assert_eq!(
        (
            text["string"].as_str(),
            text["color"].as_str(),
            text["halign"].as_str()
        ),
        (Some("hi"), Some("#10203080"), Some("center"))
    );
}

#[test]
fn templates_are_detached_and_classes_resolved_through_the_projects_types() {
    let scratch = fixture("bt-resolved-templates");
    let path = shapes_map(&scratch, "level", GRIDS[0], "");
    let types = bt_tmx::parse_project(PROJECT, &scratch.join("game.tiled-project"))
        .unwrap()
        .value
        .custom_types;
    let (_, resolved) = resolve_file(&scratch, &path, &types);
    let document = written(&resolved);
    assert_matches_schema(&document, "level");
    let chest = objects_by_id(&document)[&11];
    assert_eq!(chest["template"], "../maps/chest.tx");
    assert_eq!(
        (
            chest["name"].as_str(),
            chest["type"].as_str(),
            chest["kind"].as_str()
        ),
        (Some("chest"), Some("Chest"), Some("tile"))
    );
    assert_eq!(chest["properties"]["loot"], 10, "from the template");
    assert_eq!(chest["properties"]["opened"], true, "its own");
    assert_eq!(chest["properties"]["capacity"], 3, "its class's member");
    assert_eq!(tile(&document, &chest["tile"])["tile_id"], 3);
    assert_eq!(
        document["map"]["properties"]["music"],
        "../maps/music/theme.ogg"
    );
    let door = tile_at(&document, "ground", IVec2::new(1, 0));
    assert_eq!(
        door["properties"]["material"], "oak",
        "the tile's class's member"
    );
}

const PROJECT: &str = r##"{
    "propertyTypes": [
        {"id": 1, "name": "Chest", "type": "class", "useAs": ["object"],
         "members": [{"name": "capacity", "type": "int", "value": 3}]},
        {"id": 2, "name": "Door", "type": "class", "useAs": ["tile", "object"],
         "members": [{"name": "material", "type": "string", "value": "oak"}]}
    ]
}"##;

#[test]
fn an_infinite_map_is_written_from_the_origin_of_the_area_it_uses() {
    let scratch = fixture("bt-resolved-infinite");
    let path = scratch.join("maps/endless.tmx");
    std::fs::write(&path, INFINITE).unwrap();
    let (map, resolved) = resolve_file(&scratch, &path, &CustomTypes::new());
    let document = written(&resolved);
    assert_matches_schema(&document, "endless");
    let info = &document["map"];
    assert_eq!(info["origin_cell"], json!([-18, -1]));
    assert_eq!(
        (info["columns"].as_i64(), info["rows"].as_i64()),
        (Some(20), Some(3))
    );
    assert_eq!(info["screen_rect"], json!([-288, -16, 320, 48]));
    assert_eq!(
        tile_at(&document, "ground", IVec2::new(-18, -1))["tile_id"],
        0
    );
    assert_eq!(tile_at(&document, "ground", IVec2::new(1, 1))["tile_id"], 1);
    assert_eq!(layer(&document, "ground")["rows"][1], json!(vec![0; 20]));
    assert_drawn_as_the_canvas_draws(&map, &document, "endless");
}

const INFINITE: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" tiledversion="1.11.0" orientation="orthogonal" renderorder="right-down" width="4" height="4" tilewidth="16" tileheight="16" infinite="1" nextlayerid="2" nextobjectid="1">
 <tileset firstgid="1" source="../art/tiles.tsx"/>
 <layer id="1" name="ground" width="4" height="4">
  <data encoding="csv">
   <chunk x="-32" y="-16" width="16" height="16">
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,1,0
</chunk>
   <chunk x="0" y="0" width="16" height="16">
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,2,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
</chunk>
  </data>
 </layer>
</map>
"##;

#[test]
fn automaps_rule_tiles_are_left_out_with_a_warning() {
    let scratch = fixture("bt-resolved-rule-tiles");
    let path = scratch.join("maps/ruled.tmx");
    std::fs::write(&path, RULED).unwrap();
    let (_, resolved) = resolve_file(&scratch, &path, &CustomTypes::new());
    assert_eq!(
        resolved.warnings,
        [ResolvedWarning::RuleTilesLeftOut {
            tileset: "AutoMap Rules".into(),
            cells: 1,
            objects: 1,
        }]
    );
    assert_eq!(
        resolved.warnings[0].to_string(),
        "the map has AutoMap's rule tiles (tileset “AutoMap Rules”), which mean something only \
         in rule maps: they are left out (1 cell, 1 object)"
    );
    let document = written(&resolved);
    assert_matches_schema(&document, "ruled");
    let names: Vec<&str> = document["tilesets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["tiles"]);
    assert_eq!(
        layer(&document, "ground")["rows"],
        json!([[1, 0]]),
        "the rule tile's cell is empty"
    );
    assert_eq!(layer(&document, "ground")["filled_cells"], 1);
    let objects: Vec<&str> = layer(&document, "things")["objects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap())
        .collect();
    assert_eq!(objects, ["plain"]);
    assert_eq!(document["tiles"].as_object().unwrap().len(), 1);
}

const RULED: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" tiledversion="1.11.0" orientation="orthogonal" renderorder="right-down" width="2" height="1" tilewidth="16" tileheight="16" infinite="0" nextlayerid="3" nextobjectid="3">
 <tileset firstgid="1" source="../art/tiles.tsx"/>
 <tileset firstgid="9" source=":/automap-tiles.tsx"/>
 <layer id="1" name="ground" width="2" height="1">
  <data encoding="csv">
1,12
</data>
 </layer>
 <objectgroup id="2" name="things">
  <object id="1" name="rule" gid="10" x="0" y="16" width="16" height="16"/>
  <object id="2" name="plain" gid="1" x="16" y="16" width="16" height="16"/>
 </objectgroup>
</map>
"##;

#[test]
fn what_the_json_cannot_point_to_is_said_once() {
    let atlas = Atlas::builder()
        .name("tiles")
        .grid(2, 2)
        .build_embedded()
        .unwrap();
    let lost = MapObject {
        tile: Some(Cell::new(TilesetSlot::new(3).unwrap(), TileId(0))),
        size: DVec2::splat(16.0),
        ..MapObject::default()
    };
    let map = MapBuilder::new(UVec2::splat(16), UVec2::new(4, 1))
        .tileset(atlas.tileset.clone())
        .tile_layer("ground", "0 9 1@2 9")
        .object_layer("things", vec![lost])
        .build()
        .unwrap();
    let scratch = Scratch::new("bt-resolved-warnings").unwrap();
    let resolved = ResolvedRequest::new(&map, &scratch.join("level.tmx"))
        .resolve_for(&scratch.join("level.json"))
        .unwrap();
    assert_eq!(
        resolved.warnings,
        [
            ResolvedWarning::EmbeddedImage {
                what: "tileset “tiles”".into()
            },
            ResolvedWarning::CellsWithoutTileset {
                layer: "ground".into(),
                cells: 1
            },
            ResolvedWarning::ObjectWithoutTileset { id: 1 },
            ResolvedWarning::NoImage {
                tileset: "tiles".into(),
                tiles: vec![9]
            },
        ]
    );
    let texts: Vec<String> = resolved.warnings.iter().map(ToString::to_string).collect();
    assert_eq!(
        texts,
        [
            "tileset “tiles”: its image is embedded in the map file, and the resolved JSON names \
             only image files",
            "layer “ground”: 1 cell of a tileset the map does not have, written empty",
            "object #1 shows a tile of a tileset the map does not have: written without its tile",
            "tileset “tiles” has no image for tile 9: written without one",
        ]
    );
    let document = written(&resolved);
    assert_matches_schema(&document, "warnings");
    assert_eq!(layer(&document, "ground")["rows"], json!([[1, 2, 0, 2]]));
    let first = tile(&document, &json!(1));
    assert!(first.get("image").is_none() && first["source_rect"] == json!([0, 0, 16, 16]));
    assert!(tile(&document, &json!(2)).get("source_rect").is_none());
    assert!(document["tilesets"][0].get("image").is_none());
    let object = objects_by_id(&document)[&1];
    assert!(object.get("tile").is_none() && object["kind"] == "tile");
    assert_eq!(object["box"], json!([0, -16, 16, 16]));
}

#[test]
fn oblique_maps_are_refused_and_nothing_is_written() {
    let scratch = Scratch::new("bt-resolved-oblique").unwrap();
    let map = MapBuilder::new(UVec2::splat(16), UVec2::new(3, 2))
        .orientation(Orientation::Oblique)
        .skew(IVec2::new(8, 0))
        .build()
        .unwrap();
    let out = scratch.join("oblique.json");
    let error = ResolvedRequest::new(&map, &scratch.join("oblique.tmx"))
        .write(&out)
        .unwrap_err();
    assert!(matches!(error, ResolvedError::Oblique { .. }));
    assert!(
        error.to_string().ends_with(
            "oblique.tmx is an oblique map: the resolved JSON is written for orthogonal, \
             isometric, staggered and hexagonal maps"
        ),
        "{error}"
    );
    assert!(!out.exists());
}

#[cfg(unix)]
#[test]
fn a_read_only_target_is_refused_and_left_as_it_was() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = fixture("bt-resolved-read-only");
    let path = shapes_map(&scratch, "level", GRIDS[0], "");
    let map = load(&path, &CustomTypes::new());
    let out = scratch.join("out/level.json");
    std::fs::write(&out, "{}").unwrap();
    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o444)).unwrap();
    let error = ResolvedRequest::new(&map, &path).write(&out).unwrap_err();
    assert!(
        matches!(&error, ResolvedError::Write(write) if write.refused().is_some()),
        "{error:?}"
    );
    assert!(
        error
            .to_string()
            .ends_with("level.json: the file is read-only"),
        "{error}"
    );
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "{}");
    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o644)).unwrap();
}

/// `value` with every number a real number: `16` and `16.0` are the same number in JSON.
fn numbers_as_reals(value: &Value) -> Value {
    match value {
        Value::Number(number) => json!(number.as_f64().unwrap()),
        Value::Array(items) => Value::Array(items.iter().map(numbers_as_reals).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, field)| (key.clone(), numbers_as_reals(field)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[test]
fn the_file_written_is_the_document_laid_out() {
    let scratch = fixture("bt-resolved-write");
    let path = shapes_map(&scratch, "level", GRIDS[1], "");
    let map = load(&path, &CustomTypes::new());
    let out = scratch.join("out/level.json");
    let warnings = ResolvedRequest::new(&map, &path).write(&out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let text = std::fs::read_to_string(&out).unwrap();
    let resolved = ResolvedRequest::new(&map, &path).resolve_for(&out).unwrap();
    assert_eq!(text, json_text(&resolved.document).unwrap());
    let read: Value = serde_json::from_str(&text).unwrap();
    let document = serde_json::to_value(&resolved.document).unwrap();
    assert_eq!(
        numbers_as_reals(&read),
        numbers_as_reals(&document),
        "the same values"
    );
    assert_matches_schema(&read, "level.json");
    assert!(
        text.contains("\n   \"rows\": [\n    [1, 2, 3, 0, 0, 0],\n"),
        "{text}"
    );
}
