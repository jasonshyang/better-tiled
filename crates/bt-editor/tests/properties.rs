//! The Properties panel through the editor: what a document inspects follows what was chosen
//! last (objects selected, layers chosen, Map Properties, Layer Properties, the tileset editor's
//! tiles), and its changes are undoable steps on the map or the tileset's home, typing folding
//! into one step.

#![cfg(test)]

mod common;

use bt_core::{MapObject, ObjectId, Property, TileId, TilesetSlot, Value};
use bt_edit::{SelectionMode, TileRegion};
use bt_editor::{
    ActionId, Attribute, AttributeValue, Change, DocumentId, Editor, Effect, FileRequest,
    Inspected, Intent, Step, ValueType,
};
use bt_test_data::Scratch;
use bt_test_data::fixtures::{Atlas, MapBuilder};
use bt_utils::IRect;
use glam::{DVec2, IVec2, UVec2};

use common::{apply, editor_in, write_map};

/// A map with a tile layer and an object layer of two objects, open in a new editor.
fn editor_with_objects() -> (Editor, DocumentId) {
    let object = |name: &str, x: f64| MapObject {
        name: name.to_owned(),
        pos: DVec2::new(x, 0.0),
        size: DVec2::splat(16.0),
        ..MapObject::default()
    };
    let atlas = Atlas::builder().build_embedded().unwrap();
    let map = MapBuilder::new(UVec2::splat(16), UVec2::new(4, 3))
        .tileset(atlas.tileset)
        .tile_layer("ground", "1 1 1 1\n1 1 1 1\n1 1 1 1")
        .object_layer("things", vec![object("chest", 0.0), object("door", 32.0)])
        .build()
        .unwrap();
    let mut editor = Editor::new();
    let doc = editor.open_new_map(map);
    (editor, doc)
}

fn set_property(doc: DocumentId, inspected: Inspected, change: Change) -> Intent {
    Intent::ChangeProperties {
        doc,
        inspected,
        change,
        merge: None,
    }
}

fn property(editor: &Editor, doc: DocumentId, inspected: &Inspected, name: &str) -> Value {
    let rows = editor.property_rows(doc, inspected).unwrap();
    let row = rows
        .iter()
        .find(|row| row.path == [Step::Member(name.to_owned())])
        .unwrap_or_else(|| panic!("no property {name} in {rows:?}"));
    row.value.value.clone()
}

#[test]
fn the_panel_follows_what_was_chosen_last() {
    let (mut editor, doc) = editor_with_objects();
    assert_eq!(
        editor.inspected(doc),
        Some(Inspected::Map),
        "a map starts on itself"
    );

    editor.apply(Intent::SelectObjects {
        doc,
        ids: vec![ObjectId(2), ObjectId(1)],
        mode: SelectionMode::Replace,
    });
    assert_eq!(
        editor.inspected(doc),
        Some(Inspected::Objects(vec![ObjectId(2), ObjectId(1)]))
    );
    // Selecting nothing leaves the panel on the objects, with none to show.
    editor.apply(Intent::SelectObjects {
        doc,
        ids: Vec::new(),
        mode: SelectionMode::Replace,
    });
    assert_eq!(editor.inspected(doc), Some(Inspected::Objects(Vec::new())));

    editor.apply(Intent::Run(ActionId::MapProperties));
    assert_eq!(editor.inspected(doc), Some(Inspected::Map));
    let ground = editor.map_of(doc).unwrap().layers[0].id;
    editor.apply(Intent::SelectLayer { doc, layer: ground });
    assert_eq!(editor.inspected(doc), Some(Inspected::Layers(vec![ground])));
    editor.apply(Intent::Run(ActionId::MapProperties));
    editor.apply(Intent::Run(ActionId::LayerProperties));
    assert_eq!(editor.inspected(doc), Some(Inspected::Layers(vec![ground])));

    // An object made is selected, and shown.
    editor.apply(Intent::SelectLayer {
        doc,
        layer: editor.map_of(doc).unwrap().layers[1].id,
    });
    editor.apply(Intent::InsertObject {
        doc,
        object: Box::new(MapObject {
            size: DVec2::splat(8.0),
            ..MapObject::default()
        }),
    });
    assert_eq!(
        editor.inspected(doc),
        Some(Inspected::Objects(vec![ObjectId(3)]))
    );

    // The tileset editor shows its tileset, then its selected tiles; the map's choice comes back
    // with the map.
    let slot = TilesetSlot::new(0).unwrap();
    editor.apply(Intent::EditTileset { doc, slot });
    assert_eq!(editor.inspected(doc), Some(Inspected::Tileset));
    editor.apply(Intent::SelectTilesetTiles {
        doc,
        tiles: vec![TileId(1), TileId(2)],
        current: Some(TileId(1)),
    });
    assert_eq!(
        editor.inspected(doc),
        Some(Inspected::Tiles(vec![TileId(1), TileId(2)]))
    );
    editor.apply(Intent::StopEditingTileset { doc });
    assert_eq!(editor.inspected(doc), Some(Inspected::Objects(Vec::new())));
}

#[test]
fn custom_properties_change_as_undoable_steps_on_several_objects() {
    let (mut editor, doc) = editor_with_objects();
    let both = Inspected::Objects(vec![ObjectId(1), ObjectId(2)]);
    editor.apply(set_property(
        doc,
        both.clone(),
        Change::add("hp", ValueType::Int),
    ));
    editor.apply(set_property(
        doc,
        both.clone(),
        Change::set(
            vec![Step::Member("hp".into())],
            Property::new(Value::Int(7)),
        ),
    ));
    assert_eq!(property(&editor, doc, &both, "hp"), Value::Int(7));
    let one = Inspected::Objects(vec![ObjectId(2)]);
    assert_eq!(property(&editor, doc, &one, "hp"), Value::Int(7));
    let history = editor.document(doc).unwrap().history();
    assert_eq!(
        history.undoable().map(|t| t.label().to_owned()),
        Some("Set Property 'hp'".to_owned())
    );

    editor.apply(Intent::Run(ActionId::Undo));
    assert_eq!(property(&editor, doc, &both, "hp"), Value::Int(0));
    editor.apply(Intent::Run(ActionId::Undo));
    assert!(editor.property_rows(doc, &both).unwrap().is_empty());

    // A refused change says why and changes nothing.
    let effects = editor.apply(set_property(
        doc,
        both.clone(),
        Change::add("", ValueType::Int),
    ));
    assert!(matches!(&effects[..], [Effect::Notify(_)]), "{effects:?}");
}

#[test]
fn typing_in_a_field_folds_into_one_step() {
    let (mut editor, doc) = editor_with_objects();
    let chest = Inspected::Objects(vec![ObjectId(1)]);
    let name = |editor: &Editor| {
        editor
            .map_of(doc)
            .unwrap()
            .object(ObjectId(1))
            .unwrap()
            .name
            .clone()
    };
    for (text, run) in [("b", 1), ("bo", 1), ("box", 1), ("boxes", 2)] {
        editor.apply(Intent::SetAttribute {
            doc,
            inspected: chest.clone(),
            attribute: Attribute::Name,
            value: AttributeValue::Text(text.to_owned()),
            merge: Some(run),
        });
    }
    assert_eq!(name(&editor), "boxes");
    let labels: Vec<String> = editor
        .document(doc)
        .unwrap()
        .history()
        .entries()
        .iter()
        .map(|entry| entry.label.clone())
        .collect();
    assert_eq!(labels, ["Change Name", "Change Name"], "a run is one step");
    editor.apply(Intent::Run(ActionId::Undo));
    assert_eq!(name(&editor), "box");
    editor.apply(Intent::Run(ActionId::Undo));
    assert_eq!(name(&editor), "chest");

    let rows = editor.attribute_rows(doc, &chest).unwrap();
    let id = rows
        .iter()
        .find(|row| row.attribute == Attribute::Id)
        .unwrap();
    assert!(id.read_only);
    let effects = editor.apply(Intent::SetAttribute {
        doc,
        inspected: chest,
        attribute: Attribute::Id,
        value: AttributeValue::Integer(9),
        merge: None,
    });
    assert!(matches!(&effects[..], [Effect::Notify(_)]), "{effects:?}");
}

#[test]
fn tiles_change_on_their_tilesets_home_and_keep_no_empty_data() {
    let (mut editor, doc) = editor_with_objects();
    editor.apply(Intent::EditTileset {
        doc,
        slot: TilesetSlot::new(0).unwrap(),
    });
    let tiles = Inspected::Tiles(vec![TileId(3)]);
    editor.apply(set_property(
        doc,
        tiles.clone(),
        Change::add("solid", ValueType::Bool),
    ));
    let tileset = |editor: &Editor| editor.edited_tileset(doc).unwrap().clone();
    assert!(tileset(&editor).tile(TileId(3)).is_some());
    assert_eq!(
        editor.store().unsaved().len(),
        1,
        "the map holds the tileset"
    );
    editor.apply(set_property(doc, tiles.clone(), Change::remove("solid")));
    assert!(
        tileset(&editor).tile(TileId(3)).is_none(),
        "an atlas keeps no data a tile does not need"
    );

    editor.apply(Intent::SetAttribute {
        doc,
        inspected: Inspected::Tileset,
        attribute: Attribute::Name,
        value: AttributeValue::Text("terrain".to_owned()),
        merge: None,
    });
    assert_eq!(tileset(&editor).name, "terrain");
    let map = editor.map_of(doc).unwrap();
    assert_eq!(map.tilesets[0].tileset.name, "terrain");
}

#[test]
fn a_map_made_finite_is_cropped_and_its_tile_selection_cleared() {
    let map = MapBuilder::new(UVec2::splat(16), UVec2::new(4, 4))
        .infinite()
        .tile_layer_at("ground", IVec2::new(5, 5), "1 1\n1 1")
        .tileset(Atlas::builder().build_embedded().unwrap().tileset)
        .build()
        .unwrap();
    let mut editor = Editor::new();
    let doc = editor.open_new_map(map);
    editor.apply(Intent::SelectTiles {
        doc,
        region: TileRegion::from_rect(IRect::from_corners(IVec2::new(5, 5), IVec2::new(6, 6))),
        mode: SelectionMode::Replace,
    });
    assert!(!editor.document(doc).unwrap().selection().tiles.is_empty());
    editor.apply(Intent::SetAttribute {
        doc,
        inspected: Inspected::Map,
        attribute: Attribute::Infinite,
        value: AttributeValue::Bool(false),
        merge: None,
    });
    let map = editor.map_of(doc).unwrap();
    assert!(!map.infinite);
    assert_eq!((map.width, map.height), (2, 2));
    assert!(editor.document(doc).unwrap().selection().tiles.is_empty());
}

#[test]
fn a_chosen_file_is_kept_relative_to_the_map() {
    let scratch = Scratch::new("properties-file").unwrap();
    let path = write_map(scratch.path(), "level.tmx");
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path));
    let doc = editor.current().unwrap();
    let map = Inspected::Map;
    editor.apply(set_property(
        doc,
        map.clone(),
        Change::add("script", ValueType::File),
    ));
    let request = FileRequest::PropertyFile {
        doc,
        inspected: map.clone(),
        path: vec![Step::Member("script".into())],
    };
    editor.apply(Intent::FilePicked {
        request,
        path: Some(scratch.join("scripts").join("door.lua")),
    });
    assert_eq!(
        property(&editor, doc, &map, "script"),
        Value::File("scripts/door.lua".to_owned())
    );
}
