//! The live link end to end (`11-live-link.md`, §13): `bt`'s client on a thread asks the window
//! while it runs its frames, through a socket of its own.

use std::path::{Path, PathBuf};

use bt_core::{FileRef, TilesetRef};
use bt_live::{KeyboardFocus, MarkSpec, Request, Response, Run, runs_from_cells};
use bt_test_data::Scratch;
use bt_test_data::fixtures::{Atlas, MapBuilder};
use bt_tmx::WriteRequest;
use glam::{IVec2, UVec2};

use crate::test_window::{LiveFolder, TestWindow};

/// A map of `name` with an external tileset, written in `folder`.
fn write_map(folder: &Path, name: &str) -> PathBuf {
    let atlas = Atlas::builder().name("tiles").build_in(folder).unwrap();
    let tileset_path = folder.join("tiles.tsx");
    WriteRequest::for_tileset(&atlas.tileset, &tileset_path)
        .write()
        .unwrap();
    let mut map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(4, 3))
        .tileset(atlas.tileset.clone())
        .tile_layer("ground", "0 1 2 3\n0 1 2 3\n0 1 2 3")
        .build()
        .unwrap();
    map.tilesets[0] = TilesetRef {
        file: Some(FileRef::new("tiles.tsx", &tileset_path)),
        first_gid: Some(1),
        tileset: atlas.tileset.clone(),
    };
    let path = folder.join(name);
    WriteRequest::for_map(&map, &path).write().unwrap();
    std::fs::canonicalize(path).unwrap()
}

fn done(response: Result<Response, String>) -> bt_live::Done {
    response.unwrap().into_done().unwrap()
}

fn mark(label: &str, cells: &[(i32, i32)]) -> MarkSpec {
    MarkSpec {
        runs: runs_from_cells(cells.iter().map(|&(x, y)| IVec2::new(x, y))),
        label: label.to_owned(),
    }
}

#[test]
fn bt_sees_the_open_maps_their_tool_and_their_unsaved_changes() {
    let scratch = Scratch::new("window-live-status").unwrap();
    let live = LiveFolder::new("status");
    let map = write_map(scratch.path(), "a.tmx");
    let mut window = TestWindow::with_live_link(&scratch.join("data"), &live);
    assert!(window.is_listening());
    window.drop_file(&map);

    let status = window
        .ask_bt(&live, Request::Status)
        .unwrap()
        .into_status()
        .unwrap();
    assert_eq!(status.pid, std::process::id());
    let [document] = status.documents.as_slice() else {
        panic!("one document: {:?}", status.documents)
    };
    assert_eq!(document.path.as_deref(), Some(map.as_path()));
    assert_eq!(document.title, "a.tmx");
    assert!(document.active && !document.dirty && !document.tileset);
    assert_eq!(document.layer.as_deref(), Some("ground"));
    assert_eq!(document.keyboard, Some(KeyboardFocus::Map));
    assert!(document.history.is_empty());

    // An unsaved change shows in the status and in the snapshot.
    window.press(egui::Modifiers::NONE, egui::Key::E);
    window.click_cell(
        IVec2::new(1, 1),
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    let status = window
        .ask_bt(&live, Request::Status)
        .unwrap()
        .into_status()
        .unwrap();
    let document = &status.documents[0];
    assert!(document.dirty);
    assert_eq!(document.history.len(), 1);
    assert_eq!(document.undo_steps, 1);
    let text = window
        .ask_bt(&live, Request::Snapshot { path: map.clone() })
        .unwrap()
        .into_map_text()
        .unwrap();
    assert!(text.dirty);
    let mut loader = bt_tmx::Loader::builder().build();
    let shown = loader.parse_map(&text.map, &map).unwrap().value;
    let ground = shown.layers[0].as_tile_layer().unwrap();
    assert!(ground.cell(IVec2::new(1, 1)).is_empty(), "the erased cell");
}

#[test]
fn marks_replace_add_move_and_clear_and_never_switch_the_map_shown() {
    let scratch = Scratch::new("window-live-marks").unwrap();
    let live = LiveFolder::new("marks");
    let a = write_map(scratch.path(), "a.tmx");
    let b = write_map(scratch.path(), "b.tmx");
    let mut window = TestWindow::with_live_link(&scratch.join("data"), &live);
    window.drop_file(&a);
    window.drop_file(&b);
    let shown = window.editor().current().unwrap();
    let doc_a = window.editor().document_at(&a).unwrap();
    assert_ne!(shown, doc_a, "b is shown");

    let marked = done(window.ask_bt(
        &live,
        Request::Mark {
            path: b.clone(),
            marks: vec![mark("bridge", &[(1, 1), (2, 1)])],
            add: false,
            focus: true,
        },
    ));
    assert!(
        marked.message.starts_with("1 mark on b.tmx"),
        "{}",
        marked.message
    );
    let labels = |window: &TestWindow, path: &Path| -> Vec<String> {
        let doc = window.editor().document_at(path).unwrap();
        window
            .editor()
            .marks(doc)
            .iter()
            .map(|m| m.label.clone())
            .collect()
    };
    done(window.ask_bt(
        &live,
        Request::Mark {
            path: b.clone(),
            marks: vec![mark("ford", &[(0, 2)])],
            add: true,
            focus: false,
        },
    ));
    assert_eq!(labels(&window, &b), ["bridge", "ford"]);

    // Marking a label again moves that mark.
    done(window.ask_bt(
        &live,
        Request::Mark {
            path: b.clone(),
            marks: vec![mark("bridge", &[(3, 0)])],
            add: true,
            focus: false,
        },
    ));
    let doc_b = window.editor().document_at(&b).unwrap();
    let bridge = window.editor().marks(doc_b)[0].clone();
    assert_eq!(bridge.cells, [IVec2::new(3, 0)]);

    // Marks on a map the editor is not showing wait there.
    let waiting = done(window.ask_bt(
        &live,
        Request::Mark {
            path: a.clone(),
            marks: vec![mark("gate", &[(0, 0)])],
            add: false,
            focus: true,
        },
    ));
    assert!(
        waiting.message.contains("they wait there"),
        "{}",
        waiting.message
    );
    assert_eq!(
        window.editor().current(),
        Some(shown),
        "bt never switches the map shown"
    );
    assert_eq!(labels(&window, &a), ["gate"]);

    let cleared = done(window.ask_bt(
        &live,
        Request::Mark {
            path: b.clone(),
            marks: Vec::new(),
            add: false,
            focus: false,
        },
    ));
    assert!(
        cleared.message.starts_with("cleared"),
        "{}",
        cleared.message
    );
    assert!(labels(&window, &b).is_empty());
    let status = window
        .ask_bt(&live, Request::Status)
        .unwrap()
        .into_status()
        .unwrap();
    let a_status = status
        .documents
        .iter()
        .find(|d| d.path.as_deref() == Some(a.as_path()))
        .unwrap();
    assert_eq!(a_status.marks[0].runs, vec![Run { y: 0, x0: 0, x1: 1 }]);
}

#[test]
fn bt_opens_saves_and_finds_nothing_to_undo_in_the_editor() {
    let scratch = Scratch::new("window-live-open-save").unwrap();
    let live = LiveFolder::new("open-save");
    let map = write_map(scratch.path(), "a.tmx");
    let mut window = TestWindow::with_live_link(&scratch.join("data"), &live);

    let opened = done(window.ask_bt(&live, Request::Open { path: map.clone() }));
    assert!(
        opened.message.starts_with("opened a.tmx"),
        "{}",
        opened.message
    );
    assert!(window.editor().document_at(&map).is_some());
    let again = done(window.ask_bt(&live, Request::Open { path: map.clone() }));
    assert!(again.message.contains("is open"), "{}", again.message);

    let nothing = done(window.ask_bt(&live, Request::Undo { path: map.clone() }));
    assert!(
        nothing.nothing,
        "no bt edit to take back: bt undo falls through to the file"
    );

    window.press(egui::Modifiers::NONE, egui::Key::E);
    window.click_cell(
        IVec2::new(0, 0),
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    let before = std::fs::read_to_string(&map).unwrap();
    let saved = done(window.ask_bt(&live, Request::Save { path: map.clone() }));
    assert_eq!(saved.message, "saved a.tmx");
    assert_ne!(
        std::fs::read_to_string(&map).unwrap(),
        before,
        "the file got the change"
    );
    let doc = window.editor().document_at(&map).unwrap();
    let asset = window.editor().document(doc).unwrap().asset();
    assert!(!window.editor().store().get(asset).unwrap().is_dirty());

    let missing = window.ask_bt(
        &live,
        Request::Save {
            path: scratch.join("b.tmx"),
        },
    );
    assert!(missing.unwrap_err().contains("not open in the editor"));
}

#[test]
fn the_preference_stops_and_starts_the_live_link() {
    let scratch = Scratch::new("window-live-preference").unwrap();
    let live = LiveFolder::new("preference");
    let mut window = TestWindow::with_live_link(&scratch.join("data"), &live);
    assert!(window.is_listening());
    let mut settings = window.editor().settings().clone();
    settings.general.live_link = false;
    window.apply(bt_editor::Intent::ChangeSettings(Box::new(
        settings.clone(),
    )));
    assert!(!window.is_listening());
    let socket = live.path().join(format!("{}.sock", std::process::id()));
    assert!(!socket.exists(), "a stopped link leaves no socket");
    settings.general.live_link = true;
    window.apply(bt_editor::Intent::ChangeSettings(Box::new(settings)));
    assert!(window.is_listening());
    assert!(socket.exists());
}
