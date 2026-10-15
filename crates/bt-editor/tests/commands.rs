//! Commands use a saved snapshot and preserve placeholder values as literal arguments.

#![cfg(test)]
mod common;

use bt_core::Command;
use bt_editor::{ActionId, Effect, Intent, PaletteTarget};
use bt_test_data::Scratch;
use common::{apply, editor_in, write_map};

#[test]
fn commands_edit_undo_save_and_expand_without_resplitting_document_values() {
    let scratch = Scratch::new("editor-project-commands").unwrap();
    let folder = scratch.join("game");
    std::fs::create_dir_all(&folder).unwrap();
    let path = write_map(&folder, "map with spaces.tmx");
    let project = folder.join("game.tiled-project");
    bt_tmx::WriteRequest::for_project(&bt_core::Project::default(), &project)
        .write()
        .unwrap();
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path.clone()));
    let doc = editor.current().unwrap();
    let layer = editor.map_of(doc).unwrap().layers[0].id;
    apply(&mut editor, Intent::SelectLayer { doc, layer });
    let command = Command {
        name: "Probe".into(),
        command: "echo".into(),
        arguments: "%mapfile \"%layername\" %projectpath %s".into(),
        shortcut: "Ctrl+Alt+F8".into(),
        ..Command::default()
    };
    apply(
        &mut editor,
        Intent::SetProjectCommands {
            doc,
            commands: vec![command.clone()],
        },
    );
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert!(editor.project_commands().is_empty());
    apply(&mut editor, Intent::Run(ActionId::Redo));
    assert_eq!(editor.project_commands(), &[command]);
    let name = "a \"quoted\"; $HOME layer";
    apply(
        &mut editor,
        Intent::RenameLayer {
            doc,
            layer,
            name: name.into(),
        },
    );
    let effects = apply(&mut editor, Intent::RunProjectCommand(0));
    let job = effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::RunCommand(job) => Some(job),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        job.process().args(),
        &[
            path.display().to_string(),
            name.to_owned(),
            folder.display().to_string(),
            "%s".into()
        ]
    );
    assert_eq!(
        bt_tmx::Loader::default()
            .load_map(&path)
            .unwrap()
            .value
            .layers[0]
            .name,
        name
    );
    assert_eq!(
        bt_tmx::load_project(&project).unwrap().value.commands.len(),
        1
    );
    assert!(!editor.has_unsaved());
    assert_eq!(
        editor.palette("Probe", 10)[0].target,
        PaletteTarget::ProjectCommand(0)
    );
    assert_eq!(
        editor.project_command_for(bt_editor::Chord::from_qt("Ctrl+Alt+F8").unwrap()),
        Some(0)
    );
}

#[test]
fn a_required_save_failure_and_unavailable_placeholders_refuse_execution() {
    let scratch = Scratch::new("editor-command-refused-save").unwrap();
    let path = write_map(scratch.path(), "map.tmx");
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path.clone()));
    let doc = editor.current().unwrap();
    let command = Command {
        name: "Probe".into(),
        command: "echo".into(),
        ..Command::default()
    };
    apply(
        &mut editor,
        Intent::SetProjectCommands {
            doc,
            commands: vec![command.clone()],
        },
    );
    let permissions = std::fs::metadata(&path).unwrap().permissions();
    let mut locked = permissions.clone();
    locked.set_readonly(true);
    std::fs::set_permissions(&path, locked).unwrap();
    let effects = apply(&mut editor, Intent::RunProjectCommand(0));
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::RunCommand(_)))
    );
    assert!(effects.iter().any(|effect| matches!(effect, Effect::Notify(notice) if notice.text.contains("required save failed"))));
    std::fs::set_permissions(&path, permissions).unwrap();
    apply(
        &mut editor,
        Intent::SetProjectCommands {
            doc,
            commands: vec![Command {
                arguments: "%objectid".into(),
                save_before_execute: false,
                ..command
            }],
        },
    );
    let effects = apply(&mut editor, Intent::RunProjectCommand(0));
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::RunCommand(_)))
    );
    assert!(effects.iter().any(|effect| matches!(effect, Effect::Notify(notice) if notice.text.contains("%objectid is unavailable"))));
}
