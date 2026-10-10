//! What an editor says about itself (`11-live-link.md`, §5.1): its version and process, and
//! for each open document what the person is doing with it, so an agent can tell before it
//! interrupts.

use std::path::PathBuf;

use bt_utils::IRect;
use glam::IVec2;
use serde::{Deserialize, Serialize};

use super::PROTOCOL;
use super::request::MarkSpec;
use super::run::Run;

/// The answer to [`Request::Status`](crate::Request::Status).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    /// The protocol version the editor speaks: [`PROTOCOL`] for this build's.
    pub protocol: u32,
    /// The editor's process id.
    pub pid: u32,
    /// Whether the editor's window has the keyboard focus: the person is working in it.
    pub focused: bool,
    /// The documents open in the editor, in the order of their tabs.
    pub documents: Vec<DocStatus>,
}

impl Status {
    /// The status of the editor this code runs in: this build's protocol and this process.
    pub fn of_this_process(focused: bool, documents: Vec<DocStatus>) -> Status {
        Status {
            protocol: PROTOCOL,
            pid: std::process::id(),
            focused,
            documents,
        }
    }
}

/// A document open in the editor, as `bt editor` shows it. Names are `bt`'s own: a terrain, a
/// tile, an `NxM` stamp, an eraser size. What is in view, under the pointer and on the keyboard
/// is reported for the document shown only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocStatus {
    /// The document's file, by its canonical path; none for a map never saved.
    pub path: Option<PathBuf>,
    /// The title of its tab.
    pub title: String,
    /// Whether it is the document shown.
    pub active: bool,
    /// Whether it has unsaved changes.
    pub dirty: bool,
    /// Whether it is a tileset opened on its own, rather than a map.
    pub tileset: bool,
    /// The current layer's name; none when there is no current layer.
    pub layer: Option<String>,
    /// The current tool's name.
    pub tool: String,
    /// What the tool paints with; none for a tool that paints nothing.
    pub brush: Option<String>,
    /// The selected cells, as runs.
    pub selection: Vec<Run>,
    /// The selected objects' ids.
    pub objects: Vec<u32>,
    /// The cells in view (the document shown only).
    pub view: Option<IRect>,
    /// The cell under the pointer (the document shown only).
    pub mouse: Option<IVec2>,
    /// The labels of the last six undo steps, the latest last.
    pub history: Vec<String>,
    /// How many steps can be undone.
    pub undo_steps: usize,
    /// The marks `bt mark` put on the canvas, in order.
    pub marks: Vec<MarkSpec>,
    /// Whether the World tool is on: the person is arranging the maps of a world (the document
    /// shown only).
    pub world_tool: bool,
    /// What has the keyboard (the document shown only; none for the others).
    pub keyboard: Option<KeyboardFocus>,
}

/// What has the keyboard in the editor's window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyboardFocus {
    /// The map: keys go to the tools and the shortcuts.
    Map,
    /// A text field: the person is typing.
    TextField,
    /// A canvas of the tileset editor (collision shapes, animation frames).
    TilesetCanvas,
    /// A dialog or the command palette is open.
    Dialog,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_names_this_process_and_protocol() {
        let status = Status::of_this_process(true, Vec::new());
        assert_eq!(status.protocol, PROTOCOL);
        assert_eq!(status.pid, std::process::id());
        assert!(status.focused);
    }

    #[test]
    fn rectangles_points_and_focus_are_written_plainly() {
        let doc = DocStatus {
            path: None,
            title: "untitled.tmx".to_owned(),
            active: true,
            dirty: true,
            tileset: false,
            layer: None,
            tool: "Stamp Brush".to_owned(),
            brush: None,
            selection: Vec::new(),
            objects: Vec::new(),
            view: Some(IRect {
                x: -2,
                y: 0,
                w: 40,
                h: 30,
            }),
            mouse: Some(IVec2::new(5, -1)),
            history: Vec::new(),
            undo_steps: 0,
            marks: Vec::new(),
            world_tool: false,
            keyboard: Some(KeyboardFocus::TextField),
        };
        let json: serde_json::Value = serde_json::to_value(&doc).unwrap();
        assert_eq!(
            json["view"],
            serde_json::json!({"x": -2, "y": 0, "w": 40, "h": 30})
        );
        assert_eq!(json["mouse"], serde_json::json!([5, -1]));
        assert_eq!(json["keyboard"], "text_field");
        assert_eq!(json["path"], serde_json::Value::Null);
    }
}
