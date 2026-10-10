//! A document: a tab onto a map or a tileset, with its selection, camera, history, view
//! switches and tileset editor.

use std::fmt;

use glam::DVec2;

use super::selection::Selection;
use super::tileset_editing::TilesetEditing;
use crate::history::History;
use crate::store::AssetId;

/// A document's identity for the life of the process.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocumentId(pub u64);

impl fmt::Display for DocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "document {}", self.0)
    }
}

/// Where a document's camera looks: the world point at the view's centre and the zoom.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ViewState {
    /// The world point at the centre of the view.
    pub center: DVec2,
    /// Screen points per world pixel (1 is 100 %), the same on every display.
    pub zoom: f64,
}

/// A tab's View switches.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ViewToggles {
    /// The tile grid.
    pub grid: bool,
    /// Tiled's Highlight Current Layer.
    pub highlight_current_layer: bool,
}

impl Default for ViewToggles {
    fn default() -> ViewToggles {
        ViewToggles {
            grid: true,
            highlight_current_layer: false,
        }
    }
}

/// A tab onto a map or a tileset.
#[derive(Clone, Debug)]
pub struct Document {
    pub(crate) id: DocumentId,
    pub(crate) asset: AssetId,
    pub(crate) selection: Selection,
    pub(crate) camera: Option<ViewState>,
    pub(crate) history: History,
    pub(crate) view: ViewToggles,
    /// The tileset editor: always there in a tileset's tab, in a map's while it edits one of
    /// the map's tilesets in place of the map.
    pub(crate) tileset: Option<TilesetEditing>,
}

impl Document {
    /// Its identity.
    pub fn id(&self) -> DocumentId {
        self.id
    }

    /// The asset it shows.
    pub fn asset(&self) -> AssetId {
        self.asset
    }

    /// What is selected.
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Where its camera looks; `None` until the window first frames the map.
    pub fn camera(&self) -> Option<ViewState> {
        self.camera
    }

    /// The transactions made in it.
    pub fn history(&self) -> &History {
        &self.history
    }

    /// Its View switches.
    pub fn view(&self) -> ViewToggles {
        self.view
    }

    /// Its tileset editor, when it shows one.
    pub fn tileset_editing(&self) -> Option<&TilesetEditing> {
        self.tileset.as_ref()
    }
}
