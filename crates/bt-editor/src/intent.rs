//! What the window tells the editor: commands, picks, answers, loads that finished, and edits
//! the panels ask for. Intents are applied in order at the end of a frame.

use std::path::PathBuf;

use bt_core::{Cell, Frame, LayerId, MapObject, ObjectId, PlacedCell, TileId, TilesetSlot};
use bt_edit::{FillShape, LayerLocation, SelectionMode, TileRegion};
use bt_utils::IRect;
use glam::{DVec2, IVec2};

use crate::actions::{ActionId, PaletteTarget};
use crate::document::{DocumentId, TilesetMode, ViewState};
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
    /// Edit Tileset: show the tileset in a slot of the map in place of the map.
    EditTileset {
        /// The map's document.
        doc: DocumentId,
        /// The tileset's slot.
        slot: TilesetSlot,
    },
    /// Back to the map from its tileset editor.
    StopEditingTileset {
        /// The map's document.
        doc: DocumentId,
    },
    /// Select tiles in the tileset editor.
    SelectTilesetTiles {
        /// The document.
        doc: DocumentId,
        /// The tiles.
        tiles: Vec<TileId>,
        /// The one the collision and animation editors show.
        current: Option<TileId>,
    },
    /// Show another of the tileset editor's modes.
    SetTilesetMode {
        /// The document.
        doc: DocumentId,
        /// The mode.
        mode: TilesetMode,
    },
    /// Set a tile's animation, as one step.
    SetTileAnimation {
        /// The document.
        doc: DocumentId,
        /// The tile.
        tile: TileId,
        /// Its frames; none for a still tile.
        frames: Vec<Frame>,
    },
    /// Select layers, one of them the layer tools act on (the Layers panel's Shift and ⌘
    /// clicks): `current` is selected whether `layers` names it or not.
    SelectLayers {
        /// The document.
        doc: DocumentId,
        /// The layer tools act on.
        current: LayerId,
        /// The selected layers, in the order given.
        layers: Vec<LayerId>,
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
    /// Select objects: `ids` combined with the selected ones as `mode` says. When the selected
    /// objects all lie on one object layer, it becomes current, as in Tiled.
    SelectObjects {
        /// The document.
        doc: DocumentId,
        /// The objects.
        ids: Vec<ObjectId>,
        /// How they combine with the selection.
        mode: SelectionMode,
    },
    /// Show or hide an object (the Objects panel's eye), as one step.
    SetObjectVisible {
        /// The document.
        doc: DocumentId,
        /// The object.
        object: ObjectId,
        /// Shown or hidden.
        visible: bool,
    },
    /// Centre the view on an object (a double-click in the Objects panel).
    CenterOnObject {
        /// The document.
        doc: DocumentId,
        /// The object.
        object: ObjectId,
    },
    /// Move objects to a place in an object layer's list, from theirs or other layers (the
    /// Objects panel's drag and drop), as one step.
    ReorderObjects {
        /// The document.
        doc: DocumentId,
        /// The objects.
        objects: Vec<ObjectId>,
        /// The object layer they go to.
        layer: LayerId,
        /// Their place in its list before the move, counted from the bottom.
        index: usize,
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
    /// Drag layers to a place in the tree (the Layers panel's drag and drop), as one step; a
    /// layer inside a group dragged with it stays in its group.
    MoveLayers {
        /// The document.
        doc: DocumentId,
        /// The layers.
        layers: Vec<LayerId>,
        /// Where they go: a group (`None` for the map's top level) and a place among its layers
        /// before the move, counted from the bottom.
        to: LayerLocation,
    },
    /// Set layers' opacity (the Layers panel's slider). The changes of one drag of the slider
    /// fold into one step, which `done` ends.
    SetLayerOpacity {
        /// The document.
        doc: DocumentId,
        /// The layers.
        layers: Vec<LayerId>,
        /// From 0 (transparent) to 1 (opaque).
        opacity: f64,
        /// The drag is over: the next change is a step of its own.
        done: bool,
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
