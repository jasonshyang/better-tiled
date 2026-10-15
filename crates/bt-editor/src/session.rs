//! What the next start restores: the open files and which is current, each file's camera and
//! current layer (for the last hundred files), the recent files, and the window's panel layout,
//! which the editor keeps without reading.

use std::path::{Path, PathBuf};

use bt_core::LayerId;
use serde::{Deserialize, Serialize};

use crate::document::ViewState;

/// How many files' view states are remembered.
const VIEW_STATES: usize = 100;
/// How many recent files are remembered.
const RECENT_FILES: usize = 20;

/// The session, as `session.json` holds it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    /// Explicit project reopened before its documents.
    pub project: Option<PathBuf>,
    /// Document sets remembered while switching between projects.
    pub projects: Vec<ProjectSession>,
    /// The loaded worlds, reopened before their maps.
    pub worlds: Vec<PathBuf>,
    /// The nine quick stamps, as Tiled stamp JSON with the owning path used for references.
    pub quick_stamps: Vec<SessionStamp>,
    /// The files open, in tab order.
    pub open_files: Vec<PathBuf>,
    /// The current file.
    pub current: Option<PathBuf>,
    /// Files opened lately, the latest first.
    pub recent_files: Vec<PathBuf>,
    /// Each file's camera and current layer, the latest first.
    pub view_states: Vec<FileViewState>,
    /// The window's panel layout, kept as the window wrote it.
    pub layout: serde_json::Value,
}

/// The document set last shown for one project.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectSession {
    /// Project filename.
    pub path: PathBuf,
    /// Open documents, excluding the project itself.
    pub open_files: Vec<PathBuf>,
    /// Active document.
    pub current: Option<PathBuf>,
    /// Loaded worlds.
    pub worlds: Vec<PathBuf>,
}

/// A quick stamp kept across sessions, with the location its relative references resolve from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionStamp {
    /// The quick-stamp slot (0–8).
    pub slot: usize,
    /// The virtual stamp file used when writing the JSON.
    pub path: PathBuf,
    /// A Tiled `.stamp` document.
    pub text: String,
}

/// A file's camera and current layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileViewState {
    /// The file.
    pub path: PathBuf,
    /// The world point at the view's centre.
    pub center_x: f64,
    /// The world point at the view's centre.
    pub center_y: f64,
    /// The zoom.
    pub zoom: f64,
    /// The current layer's id.
    pub current_layer: Option<u32>,
}

impl Session {
    /// The view state remembered for a file.
    pub fn view_state(&self, path: &Path) -> Option<&FileViewState> {
        self.view_states.iter().find(|state| state.path == path)
    }

    /// Remembers a file's camera and current layer, the latest first.
    pub(crate) fn remember_view(
        &mut self,
        path: &Path,
        camera: Option<ViewState>,
        layer: Option<LayerId>,
    ) {
        let Some(camera) = camera else { return };
        self.view_states.retain(|state| state.path != path);
        self.view_states.insert(
            0,
            FileViewState {
                path: path.to_path_buf(),
                center_x: camera.center.x,
                center_y: camera.center.y,
                zoom: camera.zoom,
                current_layer: layer.map(|id| id.0),
            },
        );
        self.view_states.truncate(VIEW_STATES);
    }

    /// Puts a file at the top of the recent files.
    pub(crate) fn remember_recent(&mut self, path: &Path) {
        self.recent_files.retain(|recent| recent != path);
        self.recent_files.insert(0, path.to_path_buf());
        self.recent_files.truncate(RECENT_FILES);
    }

    /// The session in `text` (`session.json`).
    pub fn parse(text: &str) -> Result<Session, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// The session as `session.json` holds it.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec2;

    use super::*;

    #[test]
    fn sessions_remember_the_latest_first_and_round_trip() {
        let mut session = Session::default();
        let camera = Some(ViewState {
            center: DVec2::new(10.0, 20.0),
            zoom: 2.0,
        });
        session.remember_view(Path::new("/a.tmx"), camera, Some(LayerId(3)));
        session.remember_view(Path::new("/b.tmx"), camera, None);
        session.remember_view(Path::new("/a.tmx"), camera, Some(LayerId(4)));
        assert_eq!(session.view_states.len(), 2);
        assert_eq!(session.view_states[0].current_layer, Some(4));
        for i in 0..30 {
            session.remember_recent(Path::new(&format!("/{i}.tmx")));
        }
        assert_eq!(session.recent_files.len(), RECENT_FILES);
        assert_eq!(session.recent_files[0], PathBuf::from("/29.tmx"));
        session.layout = serde_json::json!({"left": ["layers"]});
        assert_eq!(
            Session::parse(&session.to_json().unwrap()).unwrap(),
            session
        );
        assert_eq!(Session::parse("{}").unwrap(), Session::default());
    }
}
