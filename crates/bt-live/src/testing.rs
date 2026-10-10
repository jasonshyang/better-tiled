//! What the tests share: a sample of every request and answer, and short private folders.

use std::path::PathBuf;

use bt_utils::IRect;
use glam::IVec2;

use crate::protocol::{
    DocStatus, Done, EditRequest, Edited, KeyboardFocus, MapText, MarkSpec, PROTOCOL, Request,
    Response, Run, Status, TilesetText,
};

/// Every kind of request, each with its fields set to something other than a default.
pub(crate) fn every_request() -> Vec<Request> {
    let map = PathBuf::from("/maps/dungeon level.tmx");
    vec![
        Request::Status,
        Request::Snapshot { path: map.clone() },
        Request::Edit(EditRequest {
            prelude: "terrain floor = stone/floor".to_owned(),
            seed: Some(u64::MAX),
            dry_run: true,
            view: true,
            by: Some("terrain".to_owned()),
            runs: true,
            want_map: true,
            want_cells: true,
            ..EditRequest::new(map.clone(), "# Lay the floor\npaint floor rect 0,0 8x6\n")
        }),
        Request::Undo { path: map.clone() },
        Request::Mark {
            path: map.clone(),
            marks: vec![
                MarkSpec {
                    runs: vec![
                        Run { y: 0, x0: 1, x1: 4 },
                        Run {
                            y: 1,
                            x0: -3,
                            x1: 0,
                        },
                    ],
                    label: "door“s”".to_owned(),
                },
                MarkSpec {
                    runs: Vec::new(),
                    label: String::new(),
                },
            ],
            add: true,
            focus: true,
        },
        Request::Open { path: map.clone() },
        Request::Save { path: map },
        Request::Screenshot {
            out: PathBuf::from("/tmp/window.png"),
        },
    ]
}

/// Every kind of answer, each with its fields set to something other than a default.
pub(crate) fn every_answer() -> Vec<Response> {
    vec![
        Response::Status(Status {
            protocol: PROTOCOL,
            pid: 4242,
            focused: true,
            documents: vec![
                DocStatus {
                    path: Some(PathBuf::from("/maps/dungeon level.tmx")),
                    title: "dungeon level.tmx".to_owned(),
                    active: true,
                    dirty: true,
                    tileset: false,
                    layer: Some("Ground".to_owned()),
                    tool: "Terrain Brush".to_owned(),
                    brush: Some("terrain stone/floor".to_owned()),
                    selection: vec![Run { y: 2, x0: 2, x1: 9 }],
                    objects: vec![3, 17],
                    view: Some(IRect {
                        x: -4,
                        y: 0,
                        w: 60,
                        h: 34,
                    }),
                    mouse: Some(IVec2::new(12, 7)),
                    history: vec!["Paint".to_owned(), "bt: lay the floor".to_owned()],
                    undo_steps: 31,
                    marks: vec![MarkSpec {
                        runs: vec![Run { y: 5, x0: 5, x1: 6 }],
                        label: "spawn".to_owned(),
                    }],
                    world_tool: false,
                    keyboard: Some(KeyboardFocus::Map),
                },
                DocStatus {
                    path: None,
                    title: "untitled.tsx".to_owned(),
                    active: false,
                    dirty: false,
                    tileset: true,
                    layer: None,
                    tool: "Select Objects".to_owned(),
                    brush: None,
                    selection: Vec::new(),
                    objects: Vec::new(),
                    view: None,
                    mouse: None,
                    history: Vec::new(),
                    undo_steps: 0,
                    marks: Vec::new(),
                    world_tool: true,
                    keyboard: None,
                },
            ],
        }),
        Response::MapText(MapText {
            dirty: true,
            map: "<?xml version=\"1.0\"?>\n<map>…</map>\n".to_owned(),
            tilesets: vec![TilesetText {
                path: PathBuf::from("/maps/stone.tsx"),
                tsx: "<tileset name=\"stone\"/>".to_owned(),
            }],
        }),
        Response::Edited(Edited {
            report: serde_json::json!({
                "schema": "bt.edit/1",
                "operations": 2,
                "seed": u64::MAX,
                "ratio": 0.1,
                "lines": [{"kind": "note", "text": "painted 48 cells"}],
            }),
            area: Some(IRect {
                x: 0,
                y: 0,
                w: 8,
                h: 6,
            }),
            applied: false,
            map: Some("<map/>".to_owned()),
            cells: Some(vec![Run { y: 0, x0: 0, x1: 8 }]),
        }),
        Response::Done(Done {
            message: "undid “lay the floor” in the editor".to_owned(),
            nothing: true,
        }),
    ]
}

/// A short folder of the test's own under `/tmp`, mode `0700`, removed when dropped. Socket
/// paths have room for about a hundred bytes, which the system's temporary folder on macOS
/// nearly fills, so tests' sockets live here instead.
#[cfg(unix)]
pub(crate) struct TestFolder {
    path: PathBuf,
}

#[cfg(unix)]
impl TestFolder {
    /// A new, empty folder: `/tmp/btl-<pid>-<n>`.
    pub(crate) fn new() -> TestFolder {
        use std::os::unix::fs::DirBuilderExt;
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!("/tmp/btl-{}-{n}", std::process::id()));
        if path.exists() {
            std::fs::remove_dir_all(&path).unwrap();
        }
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        TestFolder { path }
    }

    /// The folder.
    pub(crate) fn path(&self) -> &std::path::Path {
        &self.path
    }
}

#[cfg(unix)]
impl Drop for TestFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
