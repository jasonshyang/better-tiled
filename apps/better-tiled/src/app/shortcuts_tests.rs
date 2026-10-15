//! Shortcut imports edit the Preferences draft, and exports use the same portable scheme.

use super::super::test_window::TestWindow;
use bt_editor::{ActionId, Chord, FileRequest};
use bt_test_data::Scratch;

#[test]
fn imported_shortcuts_wait_for_ok_and_cancel_discards_them() {
    let scratch = Scratch::new("window-shortcut-import").unwrap();
    let path = scratch.join("keys.kms");
    std::fs::write(&path, r#"<mapping><shortcut id="Save"><key value="Ctrl+Alt+F7"/></shortcut><shortcut id="Open"/></mapping>"#).unwrap();
    let mut window = TestWindow::with_data(&scratch.join("data"));
    let original = window.editor().keymap().clone();
    for (index, accept) in [false, true].into_iter().enumerate() {
        window.apply(bt_editor::Intent::Run(ActionId::Preferences));
        window.click("Keyboard");
        window.click("Import Shortcuts…");
        window.answer_file_dialog(
            FileRequest::ImportShortcuts {
                dialog: index as u64 + 1,
            },
            &path,
        );
        assert_eq!(
            window.editor().keymap(),
            &original,
            "import only edits the draft"
        );
        window.click(if accept { "OK" } else { "Cancel" });
        if accept {
            assert_eq!(
                window.editor().keymap().chord(ActionId::Save),
                Chord::parse("alt+cmd+f7")
            );
            assert!(window.editor().keymap().chords(ActionId::Open).is_empty());
        } else {
            assert_eq!(window.editor().keymap(), &original);
        }
    }
}

#[test]
fn exported_shortcuts_can_be_imported_by_a_fresh_keymap() {
    let scratch = Scratch::new("window-shortcut-export").unwrap();
    let mut window = TestWindow::with_data(&scratch.join("data"));
    let expected = window.editor().keymap().clone();
    window.apply(bt_editor::Intent::Run(ActionId::Preferences));
    window.click("Keyboard");
    window.click("Export Shortcuts…");
    let path = scratch.join("keys.kms");
    window.answer_file_dialog(
        FileRequest::ExportShortcuts {
            scheme: expected.to_kms(),
        },
        &path,
    );
    let mut restored = bt_editor::Keymap::new(expected.is_mac());
    restored
        .import_kms(&std::fs::read_to_string(path).unwrap())
        .unwrap();
    for action in bt_editor::ACTIONS {
        assert_eq!(restored.chords(action.id), expected.chords(action.id));
    }
    window.click("Cancel");
}
