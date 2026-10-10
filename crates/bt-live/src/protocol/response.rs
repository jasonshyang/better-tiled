//! What a running editor answers.

use std::path::PathBuf;

use bt_utils::IRect;
use serde::{Deserialize, Serialize};

use super::run::Run;
use super::status::Status;
use crate::error::Error;

/// One answer, tagged by `"answer"`: `{"answer": "done", "message": "saved", "nothing": false}`.
/// On the wire it travels inside `{"ok": …}`; an editor that could not carry a request out
/// answers `{"error": "…"}` instead. [`Response::into_status`] and its siblings take out the
/// answer a request asks for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum Response {
    /// The editor's version, process and documents.
    Status(Status),
    /// A map as the editor shows it.
    MapText(MapText),
    /// What an edit did.
    Edited(Edited),
    /// A request carried out, in a sentence.
    Done(Done),
}

/// The answer to [`Request::Snapshot`](crate::Request::Snapshot): a map as the editor shows it,
/// with its tilesets' unsaved changes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapText {
    /// Whether the map has unsaved changes in the editor.
    pub dirty: bool,
    /// The map as TMX text, as the editor would save it.
    pub map: String,
    /// The map's external tilesets that have unsaved changes, as TSX text.
    pub tilesets: Vec<TilesetText>,
}

/// An external tileset as TSX text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TilesetText {
    /// The tileset's file, by its canonical path.
    pub path: PathBuf,
    /// The tileset as TSX text.
    pub tsx: String,
}

/// The answer to [`Request::Edit`](crate::Request::Edit).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edited {
    /// The edit's typed outcome as `bt-script` writes it (`10-agents.md`, §9): JSON this crate
    /// carries without reading, which `bt` reads back as the outcome.
    pub report: serde_json::Value,
    /// The bounding rectangle of the cells the edit changed, or would change; none when it
    /// changes none.
    pub area: Option<IRect>,
    /// Whether the edit was applied as one undo step: not for a dry run, nor for a script that
    /// changed nothing.
    pub applied: bool,
    /// The edited map as TMX text, when the request wanted it.
    pub map: Option<String>,
    /// The cells the edit changed, or would change, as runs, when the request wanted them.
    pub cells: Option<Vec<Run>>,
}

/// The answer to a request that does something: what the editor did, in a sentence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Done {
    /// A sentence for the person: what was done.
    pub message: String,
    /// Whether there was nothing to do: no edit of `bt`'s to undo (then `bt undo` falls through
    /// to the file's own history), no unsaved change to save, a map already open.
    pub nothing: bool,
}

impl Response {
    /// The answer as a [`Status`], or [`Error::Unexpected`] when it is another.
    pub fn into_status(self) -> Result<Status, Error> {
        match self {
            Response::Status(status) => Ok(status),
            other => Err(other.unexpected("a status")),
        }
    }

    /// The answer as a [`MapText`], or [`Error::Unexpected`] when it is another.
    pub fn into_map_text(self) -> Result<MapText, Error> {
        match self {
            Response::MapText(text) => Ok(text),
            other => Err(other.unexpected("a map")),
        }
    }

    /// The answer as an [`Edited`], or [`Error::Unexpected`] when it is another.
    pub fn into_edited(self) -> Result<Edited, Error> {
        match self {
            Response::Edited(edited) => Ok(edited),
            other => Err(other.unexpected("an edit's report")),
        }
    }

    /// The answer as a [`Done`], or [`Error::Unexpected`] when it is another.
    pub fn into_done(self) -> Result<Done, Error> {
        match self {
            Response::Done(done) => Ok(done),
            other => Err(other.unexpected("a sentence of what was done")),
        }
    }

    /// The error of a request that asks for `expected` and got this answer.
    fn unexpected(&self, expected: &'static str) -> Error {
        let found = match self {
            Response::Status(_) => "a status",
            Response::MapText(_) => "a map",
            Response::Edited(_) => "an edit's report",
            Response::Done(_) => "a sentence of what was done",
        };
        Error::Unexpected { expected, found }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done() -> Response {
        Response::Done(Done {
            message: "saved".to_owned(),
            nothing: false,
        })
    }

    #[test]
    fn answers_are_tagged_by_their_kind() {
        assert_eq!(
            serde_json::to_string(&done()).unwrap(),
            r#"{"answer":"done","message":"saved","nothing":false}"#
        );
        let text = Response::MapText(MapText {
            dirty: true,
            map: "<map/>".to_owned(),
            tilesets: vec![TilesetText {
                path: PathBuf::from("/maps/t.tsx"),
                tsx: "<tileset/>".to_owned(),
            }],
        });
        assert_eq!(
            serde_json::to_string(&text).unwrap(),
            r#"{"answer":"map_text","dirty":true,"map":"<map/>","tilesets":[{"path":"/maps/t.tsx","tsx":"<tileset/>"}]}"#
        );
    }

    #[test]
    fn the_answer_a_request_asks_for_is_taken_out_or_refused() {
        assert_eq!(
            done().into_done().unwrap(),
            Done {
                message: "saved".to_owned(),
                nothing: false
            }
        );
        let error = done().into_map_text().unwrap_err();
        assert_eq!(
            error.to_string(),
            "the editor answered with a sentence of what was done where a map was expected"
        );
        assert!(matches!(
            done().into_status(),
            Err(Error::Unexpected {
                expected: "a status",
                ..
            })
        ));
        assert!(done().into_edited().is_err());
    }

    #[test]
    fn answers_of_another_shape_are_refused() {
        for (text, why) in [
            (r#"{"answer":"saved"}"#, "unknown variant"),
            (
                r#"{"message":"saved","nothing":false}"#,
                "missing field `answer`",
            ),
            (
                r#"{"answer":"done","message":"saved","nothing":false,"extra":1}"#,
                "unknown field",
            ),
            (
                r#"{"answer":"done","message":"saved"}"#,
                "missing field `nothing`",
            ),
        ] {
            let error = serde_json::from_str::<Response>(text)
                .unwrap_err()
                .to_string();
            assert!(error.contains(why), "{text}: {error}");
        }
    }
}
