//! What `bt` asks a running editor.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::run::Run;

/// One request, sent as one line of JSON on a connection of its own and tagged by `"op"`:
/// `{"op": "snapshot", "path": "/maps/a.tmx"}`. Maps are named by their canonical path
/// ([`canonical_path`](crate::canonical_path)). The answer each request gets is named on it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    /// The editor's protocol version, process and open documents: answered by a
    /// [`Status`](crate::Status). Read whatever else the line carries, so that every version
    /// answers it.
    Status,
    /// A map as the editor shows it, unsaved changes included: answered by a
    /// [`MapText`](crate::MapText).
    Snapshot {
        /// The map.
        path: PathBuf,
    },
    /// A script run on a map inside the editor and applied as one undo step: answered by an
    /// [`Edited`](crate::Edited).
    Edit(EditRequest),
    /// The last edit `bt` made of a map taken back, while it is still the latest: answered by a
    /// [`Done`](crate::Done), with `nothing` set when the editor has none to take back.
    Undo {
        /// The map.
        path: PathBuf,
    },
    /// Cells outlined on a map's canvas: answered by a [`Done`](crate::Done).
    Mark {
        /// The map.
        path: PathBuf,
        /// The marks, in order.
        marks: Vec<MarkSpec>,
        /// Whether the marks are added to the map's marks; otherwise they replace them, and an
        /// empty list clears them.
        add: bool,
        /// Whether the view moves to show the marks (on the map shown; the editor never
        /// switches the map the person is looking at).
        focus: bool,
    },
    /// A map opened in the editor, or shown when it is open: answered by a
    /// [`Done`](crate::Done).
    Open {
        /// The map.
        path: PathBuf,
    },
    /// A map saved through the editor's own save: answered by a [`Done`](crate::Done).
    Save {
        /// The map.
        path: PathBuf,
    },
    /// Remember a Godot target as one undoable step; answered by Done.
    RememberExport {
        /// The open map.
        path: PathBuf,
        /// The absolute .tscn target.
        target: PathBuf,
    },
    /// The editor's window as the person sees it, written as a PNG: answered by a
    /// [`Done`](crate::Done).
    Screenshot {
        /// The absolute path of the PNG to write.
        out: PathBuf,
    },
}

/// What [`Request::Edit`] asks: a script, with `bt edit`'s options. [`EditRequest::new`] sets
/// the map and the script and leaves every option off; set the others by name:
/// `EditRequest { dry_run: true, ..EditRequest::new(path, script) }`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditRequest {
    /// The map.
    pub path: PathBuf,
    /// The script, as `bt edit` reads it.
    pub script: String,
    /// Definitions run before the script: the text of a `--with` file, or empty.
    pub prelude: String,
    /// The script's seed (`--seed N`); none for the default seed.
    pub seed: Option<u64>,
    /// Report what the script would do and change nothing (`--dry-run`).
    pub dry_run: bool,
    /// Report the changed area as text (`--view`).
    pub view: bool,
    /// What the view is drawn by (`--by`: `terrain`, `tile` or `layer`), in `bt`'s words; none
    /// for the view's default.
    pub by: Option<String>,
    /// Add each row's runs to the view (`--runs`).
    pub runs: bool,
    /// Send the edited map back as TMX text (for `--render`).
    pub want_map: bool,
    /// Send back the cells the edit changed, or would change, as runs (for `--mark`).
    pub want_cells: bool,
}

impl EditRequest {
    /// Runs `script` on the map at `path`, with every option off: no prelude, the default seed,
    /// applied, no view, nothing sent back but the report.
    pub fn new(path: impl Into<PathBuf>, script: impl Into<String>) -> EditRequest {
        EditRequest {
            path: path.into(),
            script: script.into(),
            prelude: String::new(),
            seed: None,
            dry_run: false,
            view: false,
            by: None,
            runs: false,
            want_map: false,
            want_cells: false,
        }
    }
}

/// The colours the editor draws marks in, as RGBA, given in turn: bright, and unlike the
/// selection's blue. `bt render --marks` draws them in the same.
pub const MARK_PALETTE: [[u8; 4]; 5] = [
    [255, 64, 160, 255],
    [255, 150, 0, 255],
    [40, 200, 90, 255],
    [230, 200, 0, 255],
    [170, 80, 255, 255],
];

/// Cells `bt mark` outlines, with a label.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkSpec {
    /// The cells, as runs.
    pub runs: Vec<Run>,
    /// The label shown with the cells; marking a label again moves that mark. Empty for none.
    pub label: String,
    /// Which of [`MARK_PALETTE`]'s colours the editor drew it in, in a status; none in a
    /// request, where the editor chooses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_edit_request_starts_with_every_option_off() {
        let edit = EditRequest {
            seed: Some(7),
            dry_run: true,
            ..EditRequest::new("/maps/a.tmx", "paint grass rect 0,0 4x4")
        };
        assert_eq!(edit.path, PathBuf::from("/maps/a.tmx"));
        assert_eq!(edit.script, "paint grass rect 0,0 4x4");
        assert_eq!(edit.prelude, "");
        assert_eq!(edit.seed, Some(7));
        assert!(edit.dry_run);
        assert!(!edit.view && !edit.runs && !edit.want_map && !edit.want_cells);
        assert_eq!(edit.by, None);
    }

    #[test]
    fn requests_are_tagged_by_their_operation() {
        let json = |request: &Request| serde_json::to_string(request).unwrap();
        assert_eq!(json(&Request::Status), r#"{"op":"status"}"#);
        assert_eq!(
            json(&Request::Snapshot {
                path: PathBuf::from("/maps/a.tmx")
            }),
            r#"{"op":"snapshot","path":"/maps/a.tmx"}"#
        );
        assert_eq!(
            json(&Request::Edit(EditRequest::new("/maps/a.tmx", "fill x"))),
            r#"{"op":"edit","path":"/maps/a.tmx","script":"fill x","prelude":"","seed":null,"dry_run":false,"view":false,"by":null,"runs":false,"want_map":false,"want_cells":false}"#
        );
        assert_eq!(
            json(&Request::Mark {
                path: PathBuf::from("/maps/a.tmx"),
                marks: vec![MarkSpec {
                    runs: vec![Run { y: 2, x0: 0, x1: 3 }],
                    label: "door".to_owned(),
                    color: None,
                }],
                add: true,
                focus: false
            }),
            r#"{"op":"mark","path":"/maps/a.tmx","marks":[{"runs":[[2,0,3]],"label":"door"}],"add":true,"focus":false}"#
        );
        assert_eq!(
            json(&Request::Screenshot {
                out: PathBuf::from("/tmp/shot.png")
            }),
            r#"{"op":"screenshot","out":"/tmp/shot.png"}"#
        );
    }

    #[test]
    fn requests_of_another_shape_are_refused() {
        for (text, why) in [
            (
                r#"{"op":"future_operation","path":"/a.tmx"}"#,
                "unknown variant",
            ),
            (r#"{"path":"/a.tmx"}"#, "missing field `op`"),
            (r#"{"op":"snapshot"}"#, "missing field `path`"),
            (
                r#"{"op":"snapshot","path":"/a.tmx","tilesets":true}"#,
                "unknown field",
            ),
            (
                r#"{"op":"edit","path":"/a.tmx","script":"x","prelude":"","seed":null,"dry_run":false,"view":false,"by":null,"runs":false,"want_map":false,"want_cells":false,"render":true}"#,
                "unknown field",
            ),
            (
                r#"{"op":"edit","path":"/a.tmx","script":"x"}"#,
                "missing field `prelude`",
            ),
        ] {
            let error = serde_json::from_str::<Request>(text)
                .unwrap_err()
                .to_string();
            assert!(error.contains(why), "{text}: {error}");
        }
    }

    #[test]
    fn every_version_answers_a_status_whatever_else_it_carries() {
        let later = r#"{"op":"status","since":"2027-01-01","detail":true}"#;
        assert_eq!(
            serde_json::from_str::<Request>(later).unwrap(),
            Request::Status
        );
    }
}
