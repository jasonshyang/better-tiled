//! What the window tells the editor: commands, picks, answers, loads that finished, and edits
//! the panels ask for. Intents are applied in order at the end of a frame.

use std::path::PathBuf;

use bt_core::{Cell, LayerId, MapObject, ObjectId, PlacedCell};
use bt_edit::{FillShape, SelectionMode, TileRegion};
use bt_utils::IRect;
use glam::{DVec2, IVec2};

use crate::actions::{ActionId, PaletteTarget};
use crate::document::{DocumentId, ViewState};
use crate::effect::{Choice, DecisionId, FileRequest};
use crate::files::{Loaded, NewMap, NewTileset};
use crate::objects::ObjectStroke;
use crate::recovery::Orphan;
use crate::settings::Settings;
use crate::tools::Stroke;
use crate::watch::Observation;

/// A request to the editor.
#[derive(Debug)]
pub enum Intent {
    /// Run a command.
    Run(ActionId),
    /// Run an entry of the command palette.
    RunPalette(PaletteTarget),
    /// Make a new map and open it.
    CreateMap(NewMap),
    /// Add a new tileset to a map.
    CreateTileset {
        /// The map's document.
        doc: DocumentId,
        /// The tileset.
        tileset: NewTileset,
    },
    /// Open a file (or focus its tab).
    Open(PathBuf),
    /// A load the editor asked for finished.
    Loaded(Box<Loaded>),
    /// The answer to a file request; `None` when it was cancelled.
    FilePicked {
        /// What was asked.
        request: FileRequest,
        /// The file chosen.
        path: Option<PathBuf>,
    },
    /// The answer to a question.
    Decided {
        /// The question.
        decision: DecisionId,
        /// The answer.
        choice: Choice,
    },
    /// Make a tab current.
    SelectDocument(DocumentId),
    /// Close a tab (asking about unsaved work).
    CloseDocument(DocumentId),
    /// Make a layer current and the only one selected.
    SelectLayer {
        /// The document.
        doc: DocumentId,
        /// The layer.
        layer: LayerId,
    },
    /// Select tiles: `region` combined with the selection as `mode` says.
    SelectTiles {
        /// The document.
        doc: DocumentId,
        /// The cells.
        region: TileRegion,
        /// How they combine with what is selected.
        mode: SelectionMode,
    },
    /// Tiles picked in the Tiles panel become the brush: cells of the map's tilesets at their
    /// places in the tileset.
    PickTiles {
        /// The document whose map's tilesets they are.
        doc: DocumentId,
        /// The tiles.
        cells: Vec<PlacedCell>,
    },
    /// A step of the brush's or the eraser's stroke.
    Stroke(Stroke),
    /// Select objects: `ids` combined with the selected ones as `mode` says.
    SelectObjects {
        /// The document.
        doc: DocumentId,
        /// The objects.
        ids: Vec<ObjectId>,
        /// How they combine with the selection.
        mode: SelectionMode,
    },
    /// Insert an object into the current object layer (its id is given then).
    InsertObject {
        /// The document.
        doc: DocumentId,
        /// The object, in pixel space.
        object: Box<MapObject>,
    },
    /// A step of a drag of the selected objects.
    ObjectStroke(ObjectStroke),
    /// Paste the copied objects into the current object layer, centred on a point (pixel
    /// space) or where they were copied from: the window's answer to `Effect::PasteAtPointer`.
    PasteObjects {
        /// The document.
        doc: DocumentId,
        /// Where the pointer is over the map, if it is.
        at: Option<DVec2>,
    },
    /// Move the selected objects to another object layer.
    MoveObjectsToLayer {
        /// The document.
        doc: DocumentId,
        /// The layer.
        layer: LayerId,
    },
    /// Bucket Fill from a cell.
    Fill {
        /// The document.
        doc: DocumentId,
        /// Where the fill starts.
        start: IVec2,
        /// More kinds of tile the fill spreads over (those dragged over).
        extra: Vec<Cell>,
        /// Fill the selection instead (Shift).
        whole_selection: bool,
    },
    /// Shape Fill over cells.
    ShapeFill {
        /// The document.
        doc: DocumentId,
        /// The cells.
        region: TileRegion,
    },
    /// The Picker: the tile shown at a cell becomes the brush.
    PickFromMap {
        /// The document.
        doc: DocumentId,
        /// The cell.
        at: IVec2,
    },
    /// The Brush's right-drag: the tiles in a rectangle become the brush.
    Capture {
        /// The document.
        doc: DocumentId,
        /// The cells.
        rect: IRect,
        /// Erase them too (Shift at release).
        cut: bool,
    },
    /// What Shape Fill paints (the toolbar's).
    SetFillShape(FillShape),
    /// The selection tools' default mode (the toolbar's).
    SetSelectionMode(SelectionMode),
    /// Show or hide a layer.
    SetLayerVisible {
        /// The document.
        doc: DocumentId,
        /// The layer.
        layer: LayerId,
        /// Shown or hidden.
        visible: bool,
    },
    /// Lock or unlock a layer.
    SetLayerLocked {
        /// The document.
        doc: DocumentId,
        /// The layer.
        layer: LayerId,
        /// Locked or not.
        locked: bool,
    },
    /// Rename a layer.
    RenameLayer {
        /// The document.
        doc: DocumentId,
        /// The layer.
        layer: LayerId,
        /// The new name.
        name: String,
    },
    /// The window moved a document's camera.
    SetCamera {
        /// The document.
        doc: DocumentId,
        /// Where it looks now.
        view: ViewState,
    },
    /// Snapshots of an earlier run that did not end cleanly, found at start.
    Recoverable(Vec<Orphan>),
    /// What the watched files hold now (`Editor::watched`).
    Observed(Vec<Observation>),
    /// The preferences changed (the Preferences dialog's OK).
    ChangeSettings(Box<Settings>),
    /// Time passed: the recovery schedule runs.
    Tick {
        /// Milliseconds since some fixed point (monotonic).
        now_ms: u64,
    },
}
