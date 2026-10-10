//! One fixture per shape of older files (`02-file-formats.md`, §9.1): what it reads as, what it
//! is reported as, and what it is written back as (`dialects/<name>.written.<ext>`, rewritten
//! by `BT_UPDATE_GOLDENS=1` and reviewed as a change of behaviour).

#![cfg(test)]

use std::path::{Path, PathBuf};

use bt_core::{
    Cell, ClassUsage, Flip, Frame, LayerDataFormat, Map, Property, TileId, TilesetSlot, Value,
    WangId, WangSetType,
};
use bt_tmx::{Loader, Warning, WarningKind, WriteRequest, load_project};
use glam::{DVec2, IVec2};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/dialects")
        .join(name)
}

/// Compares the written text with the fixture's golden, or rewrites the golden when asked.
fn check_written(name: &str, written: &str) {
    let (stem, extension) = name.rsplit_once('.').unwrap();
    let golden = fixture(&format!("{stem}.written.{extension}"));
    if std::env::var_os("BT_UPDATE_GOLDENS").is_some() {
        std::fs::write(&golden, written).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&golden).unwrap_or_else(|_| {
        panic!(
            "{} is missing; run with BT_UPDATE_GOLDENS=1 and review it",
            golden.display()
        )
    });
    assert_eq!(
        written, expected,
        "{name} is written differently from its golden"
    );
}

fn kinds(warnings: &[Warning]) -> Vec<WarningKind> {
    warnings.iter().map(|w| w.kind).collect()
}

fn read_map(name: &str) -> (Map, Vec<Warning>, Loader) {
    let mut loader = Loader::default();
    let loaded = loader.load_map(&fixture(name)).unwrap();
    (loaded.value, loaded.warnings, loader)
}

fn written_map(map: &Map, name: &str, loader: &Loader) -> String {
    WriteRequest::for_map(map, &fixture(name))
        .loader(loader)
        .render()
        .unwrap()
        .text
}

fn cell(tile: u32) -> Cell {
    Cell::new(TilesetSlot::new(0).unwrap(), TileId(tile))
}

fn tile_cell(map: &Map, x: i32, y: i32) -> Cell {
    map.layers[0]
        .as_tile_layer()
        .unwrap()
        .cell(IVec2::new(x, y))
}

#[test]
fn a_type_attribute_is_a_class_and_class_wins() {
    let (map, warnings, loader) = read_map("type-attribute.tmx");
    assert_eq!(
        map.tilesets[0].tileset.tile(TileId(1)).unwrap().class,
        "Wall"
    );
    let objects = &map.layers[0].as_object_layer().unwrap().objects;
    assert_eq!(
        (objects[0].class.as_str(), objects[1].class.as_str()),
        ("Chest", "Door")
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    check_written(
        "type-attribute.tmx",
        &written_map(&map, "type-attribute.tmx", &loader),
    );
}

#[test]
fn tile_elements_are_cells_written_as_csv() {
    let (map, warnings, loader) = read_map("xml-tile-data.tmx");
    assert_eq!(tile_cell(&map, 0, 0), cell(0));
    assert!(tile_cell(&map, 1, 0).is_empty());
    assert_eq!(tile_cell(&map, 0, 1), cell(2).with_flip(Flip::H));
    assert_eq!(tile_cell(&map, 1, 1), cell(4));
    assert_eq!(map.file.layer_data, LayerDataFormat::Xml);
    assert_eq!(kinds(&warnings), [WarningKind::LegacyShape]);
    check_written(
        "xml-tile-data.tmx",
        &written_map(&map, "xml-tile-data.tmx", &loader),
    );
}

#[test]
fn gzip_data_is_written_back_as_it_was() {
    let (map, warnings, loader) = read_map("gzip-data.tmx");
    assert_eq!(map.file.layer_data, LayerDataFormat::Base64Gzip);
    assert_eq!(tile_cell(&map, 1, 1), cell(2));
    assert!(warnings.is_empty(), "{warnings:?}");
    let original = std::fs::read_to_string(fixture("gzip-data.tmx")).unwrap();
    assert_eq!(written_map(&map, "gzip-data.tmx", &loader), original);
}

#[test]
fn a_doctype_is_accepted_and_dropped() {
    let (map, warnings, loader) = read_map("doctype.tmx");
    assert_eq!(tile_cell(&map, 1, 0), cell(1));
    assert_eq!(kinds(&warnings), [WarningKind::LegacyShape]);
    check_written("doctype.tmx", &written_map(&map, "doctype.tmx", &loader));
}

#[test]
fn a_layer_position_is_reported_and_dropped() {
    let (map, warnings, loader) = read_map("layer-position.tmx");
    assert_eq!(map.layers[0].offset, DVec2::ZERO);
    assert_eq!(kinds(&warnings), [WarningKind::UnknownKey]);
    check_written(
        "layer-position.tmx",
        &written_map(&map, "layer-position.tmx", &loader),
    );
}

#[test]
fn an_image_layer_position_is_its_offset() {
    let (map, warnings, loader) = read_map("image-layer-position.tmx");
    assert_eq!(map.layers[0].offset, DVec2::new(12.0, -8.0));
    assert_eq!(kinds(&warnings), [WarningKind::LegacyShape]);
    check_written(
        "image-layer-position.tmx",
        &written_map(&map, "image-layer-position.tmx", &loader),
    );
}

#[test]
fn a_tile_object_without_a_size_takes_its_tiles() {
    let (map, warnings, loader) = read_map("tile-object-size.tmx");
    let object = &map.layers[0].as_object_layer().unwrap().objects[0];
    assert_eq!(
        (object.tile, object.size),
        (Some(cell(1)), DVec2::new(32.0, 32.0))
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    check_written(
        "tile-object-size.tmx",
        &written_map(&map, "tile-object-size.tmx", &loader),
    );
}

fn read_tileset(name: &str) -> (std::sync::Arc<bt_core::Tileset>, Vec<Warning>) {
    let loaded = Loader::default().load_tileset(&fixture(name)).unwrap();
    (loaded.value, loaded.warnings)
}

fn written_tileset(tileset: &bt_core::Tileset, name: &str) -> String {
    WriteRequest::for_tileset(tileset, &fixture(name))
        .render()
        .unwrap()
        .text
}

#[test]
fn terrain_types_become_a_corner_terrain_set() {
    let (tileset, warnings) = read_tileset("terrain-types.tsx");
    let set = &tileset.wang_sets[0];
    assert_eq!(
        (set.name.as_str(), set.kind),
        ("Terrains", WangSetType::Corner)
    );
    assert_eq!(set.colors[0].tile, Some(TileId(0)));
    assert_eq!(set.colors[1].tile, None);
    assert_eq!(
        set.colors[1].properties["deep"],
        Property::new(Value::Bool(true))
    );
    assert_eq!(
        set.tiles[&TileId(0)],
        WangId::from_lanes([0, 1, 0, 1, 0, 1, 0, 1])
    );
    assert_eq!(
        set.tiles[&TileId(1)],
        WangId::from_lanes([0, 2, 0, 2, 0, 0, 0, 1])
    );
    assert_eq!(tileset.tile(TileId(2)).unwrap().probability, 0.5);
    assert_eq!(kinds(&warnings), [WarningKind::LegacyShape]);
    check_written(
        "terrain-types.tsx",
        &written_tileset(&tileset, "terrain-types.tsx"),
    );
}

#[test]
fn hexadecimal_terrain_ids_and_separate_colours_merge() {
    let (tileset, warnings) = read_tileset("hex-wangid.tsx");
    let set = &tileset.wang_sets[0];
    let names: Vec<&str> = set.colors.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["", "road"]);
    assert_eq!(set.kind, WangSetType::Mixed);
    assert_eq!(
        set.tiles[&TileId(0)],
        WangId::from_lanes([0, 1, 0, 1, 0, 1, 0, 1])
    );
    assert_eq!(
        set.tiles[&TileId(1)],
        WangId::from_lanes([2, 1, 2, 1, 2, 1, 2, 1])
    );
    assert_eq!(
        set.tiles[&TileId(2)],
        WangId::from_lanes([2, 2, 0, 0, 0, 0, 0, 0])
    );
    assert_eq!(kinds(&warnings), [WarningKind::LegacyShape]);
    check_written(
        "hex-wangid.tsx",
        &written_tileset(&tileset, "hex-wangid.tsx"),
    );
}

#[test]
fn json_corner_and_edge_colours_merge() {
    let (tileset, warnings) = read_tileset("corner-edge-colors.tsj");
    let set = &tileset.wang_sets[0];
    let names: Vec<&str> = set.colors.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        ["road", "dirt"],
        "Tiled reads edge colours before corner colours"
    );
    assert_eq!(
        set.tiles[&TileId(0)],
        WangId::from_lanes([1, 2, 1, 2, 1, 2, 1, 2])
    );
    assert_eq!(kinds(&warnings), [WarningKind::LegacyShape]);
    check_written(
        "corner-edge-colors.tsj",
        &written_tileset(&tileset, "corner-edge-colors.tsj"),
    );
}

#[test]
fn collision_group_properties_move_onto_the_tile() {
    let (tileset, warnings) = read_tileset("collision-properties.tsx");
    let tile = tileset.tile(TileId(1)).unwrap();
    assert_eq!(
        tile.properties["kind"],
        Property::new(Value::String("tile".into()))
    );
    assert_eq!(tile.properties["solid"], Property::new(Value::Bool(true)));
    assert_eq!(tile.collision.as_ref().unwrap().layer.objects.len(), 1);
    assert_eq!(kinds(&warnings), [WarningKind::LegacyShape]);
    check_written(
        "collision-properties.tsx",
        &written_tileset(&tileset, "collision-properties.tsx"),
    );
}

#[test]
fn mana_world_animation_properties_become_frames() {
    let (tileset, warnings) = read_tileset("mana-world-animation.tsx");
    let tile = tileset.tile(TileId(0)).unwrap();
    assert_eq!(
        tile.animation,
        [
            Frame {
                tile: TileId(0),
                duration_ms: 100
            },
            Frame {
                tile: TileId(3),
                duration_ms: 250
            }
        ]
    );
    assert_eq!(
        tile.properties.len(),
        4,
        "the properties stay, as Tiled keeps them"
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    check_written(
        "mana-world-animation.tsx",
        &written_tileset(&tileset, "mana-world-animation.tsx"),
    );
}

#[test]
fn json_properties_as_an_object_are_typed_by_their_types() {
    let (map, warnings, loader) = read_map("json-property-object.tmj");
    assert_eq!(map.properties["hp"].value, Value::Int(12));
    assert_eq!(map.properties["name"].value, Value::String("hero".into()));
    assert_eq!(map.properties["speed"].value, Value::Float(1.5));
    assert_eq!(map.properties["visible"].value, Value::Bool(true));
    assert_eq!(kinds(&warnings), [WarningKind::LegacyShape]);
    check_written(
        "json-property-object.tmj",
        &written_map(&map, "json-property-object.tmj", &loader),
    );
}

#[test]
fn json_tiles_keyed_by_id_become_a_list() {
    let (tileset, warnings) = read_tileset("json-tiles-object.tsj");
    let ids: Vec<u32> = tileset.tiles.keys().map(|id| id.0).collect();
    assert_eq!(ids, [1, 3, 4]);
    assert_eq!(tileset.tile(TileId(1)).unwrap().probability, 0.5);
    assert_eq!(
        tileset.tile(TileId(1)).unwrap().properties["hp"].value,
        Value::Int(7)
    );
    assert_eq!(
        tileset.tile(TileId(3)).unwrap().properties["door"].value,
        Value::Bool(true)
    );
    assert_eq!(tileset.tile(TileId(4)).unwrap().animation.len(), 1);
    assert_eq!(
        kinds(&warnings),
        [WarningKind::LegacyShape, WarningKind::LegacyShape]
    );
    check_written(
        "json-tiles-object.tsj",
        &written_tileset(&tileset, "json-tiles-object.tsj"),
    );
}

#[test]
fn a_numeric_version_and_no_type_are_accepted() {
    let (map, warnings, loader) = read_map("json-numeric-version.tmj");
    assert_eq!(map.file.version, "1.2");
    assert_eq!(tile_cell(&map, 1, 0), cell(1));
    assert!(warnings.is_empty(), "{warnings:?}");
    check_written(
        "json-numeric-version.tmj",
        &written_map(&map, "json-numeric-version.tmj", &loader),
    );
}

#[test]
fn a_javascript_map_comes_back_as_it_was() {
    let (map, warnings, loader) = read_map("javascript.js");
    assert_eq!(tile_cell(&map, 0, 0), cell(0));
    assert!(warnings.is_empty(), "{warnings:?}");
    let original = std::fs::read_to_string(fixture("javascript.js")).unwrap();
    assert_eq!(written_map(&map, "javascript.js", &loader), original);
}

#[test]
fn a_nullptr_typed_list_is_a_list() {
    let (map, warnings, loader) = read_map("nullptr-list.tmx");
    assert_eq!(
        map.properties["pasted"].value,
        Value::List(vec![
            Property::new(Value::Int(1)),
            Property::new(Value::String("two".into()))
        ])
    );
    assert_eq!(kinds(&warnings), [WarningKind::LegacyShape]);
    check_written(
        "nullptr-list.tmx",
        &written_map(&map, "nullptr-list.tmx", &loader),
    );
}

#[test]
fn negative_gids_and_integers_as_floats_are_read() {
    let (map, warnings, loader) = read_map("negative-gids.tmx");
    assert_eq!((map.width, map.tile_width), (2, 32));
    assert_eq!(tile_cell(&map, 0, 0), cell(0).with_flip(Flip::H));
    let object = &map.layers[1].as_object_layer().unwrap().objects[0];
    assert_eq!(object.tile, Some(cell(1).with_flip(Flip::H | Flip::V)));
    assert_eq!(kinds(&warnings), [WarningKind::LegacyShape]);
    check_written(
        "negative-gids.tmx",
        &written_map(&map, "negative-gids.tmx", &loader),
    );
}

#[test]
fn an_export_target_of_dot_is_none() {
    let (tileset, warnings) = read_tileset("export-dot.tsx");
    assert!(tileset.file.export.is_empty());
    assert!(warnings.is_empty(), "{warnings:?}");
    check_written(
        "export-dot.tsx",
        &written_tileset(&tileset, "export-dot.tsx"),
    );
}

#[test]
fn a_projects_object_types_file_becomes_classes() {
    let loaded = load_project(&fixture("object-types-file.tiled-project")).unwrap();
    let project = loaded.value;
    let chest = project
        .custom_types
        .class_for("Chest", ClassUsage::TILE)
        .unwrap();
    assert_eq!(chest.members["gold"], Property::new(Value::Int(10)));
    assert_eq!(project.object_types_file, None);
    assert_eq!(kinds(&loaded.warnings), [WarningKind::LegacyShape]);
    let path = fixture("object-types-file.tiled-project");
    check_written(
        "object-types-file.tiled-project",
        &WriteRequest::for_project(&project, &path)
            .render()
            .unwrap()
            .text,
    );
}

#[test]
fn capitalised_command_keys_are_read() {
    let project = load_project(&fixture("capitalised-commands.tiled-project"))
        .unwrap()
        .value;
    let command = &project.commands[0];
    assert_eq!(
        (command.name.as_str(), command.command.as_str()),
        ("Run", "game")
    );
    assert_eq!(
        (
            command.arguments.as_str(),
            command.working_directory.as_str()
        ),
        ("%mapfile", ".")
    );
    assert!(command.enabled && command.save_before_execute && !command.show_output);
    assert_eq!(command.shortcut, "F5");
    let path = fixture("capitalised-commands.tiled-project");
    check_written(
        "capitalised-commands.tiled-project",
        &WriteRequest::for_project(&project, &path)
            .render()
            .unwrap()
            .text,
    );
}
