//! The Custom Types editor through the editor: its changes are undoable steps on the project,
//! classes' members show under the things of their class, and the project's file is written
//! with the map; with no project open, the types live in an untitled project for the run.

#![cfg(test)]

mod common;

use bt_core::{Map, Project, Property, Value};
use bt_editor::{
    ActionId, Attribute, AttributeValue, Change, Dialog, Editor, Effect, Inspected, Intent, Origin,
    Step, TypeEdit, ValueType,
};
use bt_test_data::Scratch;
use bt_tmx::WriteRequest;

use common::{apply, editor_in, write_map};

fn edit(doc: bt_editor::DocumentId, edit: TypeEdit) -> Intent {
    Intent::EditCustomTypes {
        doc,
        edit,
        merge: None,
    }
}

fn names(editor: &Editor) -> Vec<String> {
    editor
        .custom_types()
        .types
        .iter()
        .map(|t| t.name.clone())
        .collect()
}

#[test]
fn types_are_undoable_steps_on_the_project_saved_with_the_map() {
    let scratch = Scratch::new("custom-types-project").unwrap();
    // Projects are looked for below the home folder, which is the scratch folder here.
    let folder = scratch.join("game");
    std::fs::create_dir_all(&folder).unwrap();
    let project_path = folder.join("game.tiled-project");
    WriteRequest::for_project(&Project::default(), &project_path)
        .write()
        .unwrap();
    let path = write_map(&folder, "level.tmx");
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path));
    let doc = editor.current().unwrap();
    assert!(editor.project_path().is_some(), "the project above the map");
    let effects = editor.apply(Intent::Run(ActionId::CustomTypesEditor));
    assert!(matches!(&effects[..], [Effect::Show(Dialog::CustomTypes)]));

    editor.apply(edit(doc, TypeEdit::AddClass));
    assert_eq!(names(&editor), ["Class0"]);
    editor.apply(Intent::Run(ActionId::Undo));
    assert!(names(&editor).is_empty());
    editor.apply(Intent::Run(ActionId::Redo));
    let id = editor.custom_types().types[0].id;
    editor.apply(edit(
        doc,
        TypeEdit::Rename {
            id,
            name: "Enemy".to_owned(),
        },
    ));
    editor.apply(edit(
        doc,
        TypeEdit::Members {
            id,
            change: Change::add("hp", ValueType::Int),
        },
    ));
    editor.apply(edit(
        doc,
        TypeEdit::Members {
            id,
            change: Change::set(
                vec![Step::Member("hp".into())],
                Property::new(Value::Int(5)),
            ),
        },
    ));
    let members = editor.member_rows(id).unwrap();
    assert_eq!(members[0].value, Property::new(Value::Int(5)));
    let refused = editor.apply(edit(
        doc,
        TypeEdit::Rename {
            id,
            name: String::new(),
        },
    ));
    assert!(matches!(&refused[..], [Effect::Notify(_)]), "{refused:?}");

    // The map of class Enemy shows its members, dimmed as inherited.
    editor.apply(Intent::SetAttribute {
        doc,
        inspected: Inspected::Map,
        attribute: Attribute::Class,
        value: AttributeValue::Text("Enemy".to_owned()),
        merge: None,
    });
    let rows = editor.property_rows(doc, &Inspected::Map).unwrap();
    assert_eq!(rows[0].inherited_from, Some(Origin::Class("Enemy".into())));
    assert!(rows[0].holders.is_empty());

    // Saving the map writes the project too.
    assert_eq!(editor.store().unsaved().len(), 2, "the map and the project");
    apply(&mut editor, Intent::Run(ActionId::Save));
    assert!(editor.store().unsaved().is_empty());
    let saved = bt_tmx::load_project(&project_path).unwrap().value;
    let enemy = saved.custom_types.value_class("Enemy").unwrap();
    assert_eq!(enemy.members["hp"], Property::new(Value::Int(5)));
}

#[test]
fn without_a_project_the_types_last_for_the_run() {
    let mut editor = Editor::new();
    let doc = editor.open_new_map(Map::default());
    editor.apply(edit(doc, TypeEdit::AddEnum));
    let id = editor.custom_types().types[0].id;
    editor.apply(edit(doc, TypeEdit::AddValue(id)));
    assert_eq!(names(&editor), ["Enum0"]);
    assert_eq!(
        editor.custom_types().enum_type("Enum0").unwrap().values,
        ["Enum0_0"]
    );
    assert_eq!(editor.project_path(), None);
    assert_eq!(
        editor.store().unsaved().len(),
        1,
        "only the map: the untitled project is never asked about"
    );
}
