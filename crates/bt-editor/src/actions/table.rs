//! The action table: every command declared once, with what menus and the palette show, when it
//! applies, and its default keys (Tiled's, where Tiled has them). Menus are the window's
//! arrangement of the table.

use super::chord::{Chord, Key, Modifiers};
use crate::tools::ToolKind;

/// When a command applies.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Context {
    /// Always.
    Always,
    /// With a document open, a map or a tileset.
    Document,
    /// With a map open (and not its tileset editor).
    Map,
    /// With a map open whose current layer is a tile layer.
    TileLayer,
    /// With a map open whose current layer is an object layer, or the tileset editor's
    /// collision shapes.
    ObjectLayer,
    /// With the tileset editor open: a tileset's tab, or a map's while it edits a tileset.
    Tileset,
}

impl Context {
    /// Whether commands of the two contexts can apply at once, so that one key cannot serve
    /// both: a tile layer and an object layer are never current together, nor a map and the
    /// tileset editor, which edits collision shapes with the object tools.
    pub fn overlaps(self, other: Context) -> bool {
        use Context::{Always, Document, Map, ObjectLayer, TileLayer, Tileset};
        match (self, other) {
            (Always | Document, _) | (_, Always | Document) => true,
            (Tileset, Tileset | ObjectLayer) | (ObjectLayer, Tileset) => true,
            (Tileset, _) | (_, Tileset) => false,
            (TileLayer, ObjectLayer) | (ObjectLayer, TileLayer) => false,
            (Map | TileLayer | ObjectLayer, Map | TileLayer | ObjectLayer) => true,
        }
    }
}

/// What a command is.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ActionKind {
    /// Runs and is done.
    Command,
    /// Switches something on or off (menus show a check).
    Toggle,
    /// Chooses a tool.
    Tool(ToolKind),
}

/// A command's declaration.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ActionSpec {
    /// Its id.
    pub id: ActionId,
    /// Its stable name, in settings and `.kms` files: `save_as`.
    pub name: &'static str,
    /// What menus show.
    pub label: &'static str,
    /// More words the palette matches.
    pub keywords: &'static str,
    /// When it applies.
    pub context: Context,
    /// Its default keys on Windows and Linux.
    pub keys: &'static [Chord],
    /// Its default keys on macOS.
    pub keys_mac: &'static [Chord],
    /// Command, toggle or tool.
    pub kind: ActionKind,
    /// Whether it fires again while its key is held.
    pub repeats: bool,
    /// Whether a focused text field handles its key itself.
    pub text_edit: bool,
}

impl ActionSpec {
    /// A command that always applies, without keys.
    const fn new(
        id: ActionId,
        name: &'static str,
        label: &'static str,
        keywords: &'static str,
    ) -> ActionSpec {
        ActionSpec {
            id,
            name,
            label,
            keywords,
            context: Context::Always,
            keys: &[],
            keys_mac: &[],
            kind: ActionKind::Command,
            repeats: false,
            text_edit: false,
        }
    }

    /// Applies in `context`.
    const fn on(self, context: Context) -> ActionSpec {
        ActionSpec { context, ..self }
    }

    /// Bound to `keys` on every platform.
    const fn keys(self, keys: &'static [Chord]) -> ActionSpec {
        ActionSpec {
            keys,
            keys_mac: keys,
            ..self
        }
    }

    /// Bound to `keys` on Windows and Linux and `keys_mac` on macOS.
    const fn keys_by_platform(
        self,
        keys: &'static [Chord],
        keys_mac: &'static [Chord],
    ) -> ActionSpec {
        ActionSpec {
            keys,
            keys_mac,
            ..self
        }
    }

    /// A toggle.
    const fn toggle(self) -> ActionSpec {
        ActionSpec {
            kind: ActionKind::Toggle,
            ..self
        }
    }

    /// Chooses `tool`.
    const fn tool(self, tool: ToolKind) -> ActionSpec {
        ActionSpec {
            kind: ActionKind::Tool(tool),
            ..self
        }
    }

    /// Fires again while its key is held.
    const fn repeating(self) -> ActionSpec {
        ActionSpec {
            repeats: true,
            ..self
        }
    }

    /// A focused text field handles its key itself.
    const fn text_edit(self) -> ActionSpec {
        ActionSpec {
            text_edit: true,
            ..self
        }
    }
}

const fn plain(c: char) -> Chord {
    Chord::with(Modifiers::NONE, Key::Char(c))
}

const fn shift(c: char) -> Chord {
    Chord::with(Modifiers::SHIFT, Key::Char(c))
}

const fn cmd(c: char) -> Chord {
    Chord::with(Modifiers::COMMAND, Key::Char(c))
}

const fn cmd_shift(c: char) -> Chord {
    Chord::with(Modifiers::COMMAND_SHIFT, Key::Char(c))
}

const fn cmd_alt(c: char) -> Chord {
    Chord::with(Modifiers::COMMAND_ALT, Key::Char(c))
}

const fn named(key: Key) -> Chord {
    Chord::with(Modifiers::NONE, key)
}

const fn cmd_named(key: Key) -> Chord {
    Chord::with(Modifiers::COMMAND, key)
}

const fn cmd_shift_named(key: Key) -> Chord {
    Chord::with(Modifiers::COMMAND_SHIFT, key)
}

const CONTROL: Modifiers = Modifiers {
    control: true,
    ..Modifiers::NONE
};
const CONTROL_SHIFT: Modifiers = Modifiers {
    control: true,
    shift: true,
    ..Modifiers::NONE
};

/// Declares [`ActionId`], [`ACTIONS`] in the same order, and [`ActionId::spec`], from one list,
/// so the three cannot disagree.
macro_rules! action_table {
    ($($(#[doc = $doc:literal])+ $id:ident => $spec:expr,)+) => {
        /// A command.
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum ActionId {
            $($(#[doc = $doc])+ $id,)+
        }

        /// Every command, in the order of [`ActionId`].
        pub const ACTIONS: &[ActionSpec] = &[$($spec,)+];

        impl ActionId {
            /// Its declaration.
            pub fn spec(self) -> &'static ActionSpec {
                match self {
                    $(ActionId::$id => {
                        const SPEC: ActionSpec = $spec;
                        &SPEC
                    })+
                }
            }
        }
    };
}

use Context::{Document, Map, ObjectLayer, TileLayer};

action_table! {
    /// Make a new map.
    NewMap => ActionSpec::new(ActionId::NewMap, "new_map", "New Map…", "file create")
        .keys(&[cmd('n')]),
    /// Make a new tileset.
    NewTileset => ActionSpec::new(ActionId::NewTileset, "new_tileset", "New Tileset…", "file create tiles image")
        .on(Map),
    /// Open a file.
    Open => ActionSpec::new(ActionId::Open, "open", "Open…", "file load").keys(&[cmd('o')]),
    /// Save the current document.
    Save => ActionSpec::new(ActionId::Save, "save", "Save", "file write")
        .on(Document)
        .keys(&[cmd('s')]),
    /// Save the current document to another file.
    SaveAs => ActionSpec::new(ActionId::SaveAs, "save_as", "Save As…", "file write rename")
        .on(Document)
        .keys(&[cmd_shift('s')]),
    /// Save everything unsaved.
    SaveAll => ActionSpec::new(ActionId::SaveAll, "save_all", "Save All", "file write every")
        .keys(&[cmd_alt('s')]),
    /// Close the current document.
    Close => ActionSpec::new(ActionId::Close, "close", "Close", "tab document")
        .on(Document)
        .keys(&[cmd('w')]),
    /// The preferences.
    Preferences => ActionSpec::new(ActionId::Preferences, "preferences", "Preferences…", "settings options keyboard shortcuts")
        .keys_by_platform(&[], &[cmd(',')]),
    /// Quit.
    Quit => ActionSpec::new(ActionId::Quit, "quit", "Quit", "exit").keys(&[cmd('q')]),
    /// Undo in the current document.
    Undo => ActionSpec::new(ActionId::Undo, "undo", "Undo", "history back")
        .on(Document)
        .keys(&[cmd('z')])
        .repeating()
        .text_edit(),
    /// Redo in the current document.
    Redo => ActionSpec::new(ActionId::Redo, "redo", "Redo", "history forward")
        .on(Document)
        .keys_by_platform(&[cmd('y'), cmd_shift('z')], &[cmd_shift('z')])
        .repeating()
        .text_edit(),
    /// Cut the selected tiles or objects.
    Cut => ActionSpec::new(ActionId::Cut, "cut", "Cut", "clipboard")
        .on(Map)
        .keys(&[cmd('x')])
        .text_edit(),
    /// Copy the selected tiles or objects.
    Copy => ActionSpec::new(ActionId::Copy, "copy", "Copy", "clipboard")
        .on(Map)
        .keys(&[cmd('c')])
        .text_edit(),
    /// Take what was copied as the brush, or paste objects under the pointer.
    Paste => ActionSpec::new(ActionId::Paste, "paste", "Paste", "clipboard")
        .on(Map)
        .keys(&[cmd('v')])
        .text_edit(),
    /// Paste where it was copied from.
    PasteInPlace => ActionSpec::new(ActionId::PasteInPlace, "paste_in_place", "Paste in Place", "clipboard same position")
        .on(Map)
        .keys(&[cmd_shift('v')]),
    /// Delete the selected tiles or objects.
    Delete => ActionSpec::new(ActionId::Delete, "delete", "Delete", "remove erase clear")
        .on(Map)
        .keys(&[named(Key::Delete), named(Key::Backspace)])
        .text_edit(),
    /// Select every tile of the layer, or every object.
    SelectAll => ActionSpec::new(ActionId::SelectAll, "select_all", "Select All", "selection everything")
        .on(Map)
        .keys(&[cmd('a')])
        .text_edit(),
    /// Select nothing.
    SelectNone => ActionSpec::new(ActionId::SelectNone, "select_none", "Select None", "selection clear deselect")
        .on(Map)
        .keys(&[cmd_shift('a')]),
    /// Select the tiles or objects of the current layer that are not selected.
    InvertSelection => ActionSpec::new(ActionId::InvertSelection, "invert_selection", "Invert Selection", "selection inverse")
        .on(Map)
        .keys(&[cmd('i')]),
    /// Find a command by name.
    SearchActions => ActionSpec::new(ActionId::SearchActions, "search_actions", "Search Actions…", "palette command find")
        .keys(&[cmd_shift('p')]),
    /// Zoom in.
    ZoomIn => ActionSpec::new(ActionId::ZoomIn, "zoom_in", "Zoom In", "view bigger")
        .on(Document)
        .keys(&[cmd('=')])
        .repeating(),
    /// Zoom out.
    ZoomOut => ActionSpec::new(ActionId::ZoomOut, "zoom_out", "Zoom Out", "view smaller")
        .on(Document)
        .keys(&[cmd('-')])
        .repeating(),
    /// Zoom to 100 %.
    ZoomNormal => ActionSpec::new(ActionId::ZoomNormal, "zoom_normal", "Normal Size", "view 100 actual pixels")
        .on(Document)
        .keys(&[cmd('0')]),
    /// Fit the map in the view.
    FitInView => ActionSpec::new(ActionId::FitInView, "fit_in_view", "Fit Map in View", "view whole")
        .on(Document)
        .keys(&[cmd('/')]),
    /// Show or hide the tile grid.
    ShowGrid => ActionSpec::new(ActionId::ShowGrid, "show_grid", "Show Grid", "view tiles lines")
        .on(Document)
        .keys(&[cmd('g')])
        .toggle(),
    /// Tiled's Highlight Current Layer.
    HighlightCurrentLayer => ActionSpec::new(ActionId::HighlightCurrentLayer, "highlight_current_layer", "Highlight Current Layer", "view layer fade")
        .on(Document)
        .keys(&[plain('h')])
        .toggle(),
    /// Objects go where they are put.
    SnapNone => ActionSpec::new(ActionId::SnapNone, "snap_none", "No Snapping", "snapping free")
        .on(Map)
        .toggle(),
    /// Objects snap to the tile grid.
    SnapToGrid => ActionSpec::new(ActionId::SnapToGrid, "snap_to_grid", "Snap to Grid", "snapping tiles")
        .on(Map)
        .toggle(),
    /// Objects snap to the fine grid.
    SnapToFineGrid => ActionSpec::new(ActionId::SnapToFineGrid, "snap_to_fine_grid", "Snap to Fine Grid", "snapping subdivisions")
        .on(Map)
        .toggle(),
    /// Objects snap to whole pixels.
    SnapToPixels => ActionSpec::new(ActionId::SnapToPixels, "snap_to_pixels", "Snap to Pixels", "snapping whole")
        .on(Map)
        .toggle(),
    /// The next tab.
    NextDocument => ActionSpec::new(ActionId::NextDocument, "next_document", "Next Tab", "document switch")
        .on(Document)
        .keys_by_platform(
            &[cmd_named(Key::Tab)],
            &[Chord::with(CONTROL, Key::Tab), cmd_shift(']')],
        ),
    /// The previous tab.
    PreviousDocument => ActionSpec::new(ActionId::PreviousDocument, "previous_document", "Previous Tab", "document switch")
        .on(Document)
        .keys_by_platform(
            &[cmd_shift_named(Key::Tab)],
            &[Chord::with(CONTROL_SHIFT, Key::Tab), cmd_shift('[')],
        ),
    /// Change the map's size.
    ResizeMap => ActionSpec::new(ActionId::ResizeMap, "resize_map", "Resize Map…", "size grow shrink canvas")
        .on(Map),
    /// Shift the map's contents.
    OffsetMap => ActionSpec::new(ActionId::OffsetMap, "offset_map", "Offset Map…", "move shift wrap")
        .on(Map),
    /// Cut the map down to the selection.
    CropToSelection => ActionSpec::new(ActionId::CropToSelection, "crop_to_selection", "Crop to Selection", "resize size")
        .on(Map),
    /// Cut the map down to what its tiles use.
    Autocrop => ActionSpec::new(ActionId::Autocrop, "autocrop", "Autocrop", "resize size trim")
        .on(Map),
    /// Add a tileset file to the map.
    AddExternalTileset => ActionSpec::new(ActionId::AddExternalTileset, "add_external_tileset", "Add External Tileset…", "tsx file")
        .on(Map),
    /// The map's properties.
    MapProperties => ActionSpec::new(ActionId::MapProperties, "map_properties", "Map Properties…", "attributes custom")
        .on(Map),
    /// Add a tile layer.
    NewTileLayer => ActionSpec::new(ActionId::NewTileLayer, "new_tile_layer", "New Tile Layer", "add layer")
        .on(Map),
    /// Add an object layer.
    NewObjectLayer => ActionSpec::new(ActionId::NewObjectLayer, "new_object_layer", "New Object Layer", "add layer group objects")
        .on(Map),
    /// Add an image layer.
    NewImageLayer => ActionSpec::new(ActionId::NewImageLayer, "new_image_layer", "New Image Layer", "add layer picture")
        .on(Map),
    /// Add a group layer.
    NewGroupLayer => ActionSpec::new(ActionId::NewGroupLayer, "new_group_layer", "New Group Layer", "add layer folder")
        .on(Map),
    /// Copy the selection into a new layer.
    LayerViaCopy => ActionSpec::new(ActionId::LayerViaCopy, "layer_via_copy", "Layer via Copy", "new layer selection")
        .on(Map)
        .keys(&[cmd('j')]),
    /// Move the selection into a new layer.
    LayerViaCut => ActionSpec::new(ActionId::LayerViaCut, "layer_via_cut", "Layer via Cut", "new layer selection")
        .on(Map)
        .keys(&[cmd_shift('j')]),
    /// Put the selected layers in a group.
    GroupLayers => ActionSpec::new(ActionId::GroupLayers, "group_layers", "Group Layers", "folder")
        .on(Map),
    /// Take the selected groups' layers out of them.
    UngroupLayers => ActionSpec::new(ActionId::UngroupLayers, "ungroup_layers", "Ungroup Layers", "folder")
        .on(Map),
    /// Copy the selected layers.
    DuplicateLayers => ActionSpec::new(ActionId::DuplicateLayers, "duplicate_layers", "Duplicate Layers", "copy clone")
        .on(Map)
        .keys(&[cmd_shift('d')]),
    /// Merge the current layer into the one below.
    MergeLayerDown => ActionSpec::new(ActionId::MergeLayerDown, "merge_layer_down", "Merge Layer Down", "combine flatten")
        .on(Map),
    /// Remove the selected layers.
    RemoveLayers => ActionSpec::new(ActionId::RemoveLayers, "remove_layers", "Remove Layers", "delete")
        .on(Map),
    /// Make the layer below current.
    SelectPreviousLayer => ActionSpec::new(ActionId::SelectPreviousLayer, "select_previous_layer", "Select Previous Layer", "below down")
        .on(Map)
        .keys(&[cmd_named(Key::PageDown)])
        .repeating(),
    /// Make the layer above current.
    SelectNextLayer => ActionSpec::new(ActionId::SelectNextLayer, "select_next_layer", "Select Next Layer", "above up")
        .on(Map)
        .keys(&[cmd_named(Key::PageUp)])
        .repeating(),
    /// Select every layer.
    SelectAllLayers => ActionSpec::new(ActionId::SelectAllLayers, "select_all_layers", "Select All Layers", "every")
        .on(Map)
        .keys(&[cmd_alt('a')]),
    /// Move the selected layers up.
    RaiseLayers => ActionSpec::new(ActionId::RaiseLayers, "raise_layers", "Raise Layers", "move up order")
        .on(Map)
        .keys(&[cmd_shift_named(Key::Up)])
        .repeating(),
    /// Move the selected layers down.
    LowerLayers => ActionSpec::new(ActionId::LowerLayers, "lower_layers", "Lower Layers", "move down order")
        .on(Map)
        .keys(&[cmd_shift_named(Key::Down)])
        .repeating(),
    /// Show or hide the selected layers.
    ToggleSelectedLayers => ActionSpec::new(ActionId::ToggleSelectedLayers, "toggle_selected_layers", "Show/Hide Selected Layers", "visibility visible")
        .on(Map)
        .keys_by_platform(&[cmd('h')], &[]),
    /// Lock or unlock the selected layers.
    ToggleLockSelectedLayers => ActionSpec::new(ActionId::ToggleLockSelectedLayers, "toggle_lock_selected_layers", "Lock/Unlock Selected Layers", "locked")
        .on(Map)
        .keys(&[cmd('l')]),
    /// Show or hide every other layer.
    ToggleOtherLayers => ActionSpec::new(ActionId::ToggleOtherLayers, "toggle_other_layers", "Show/Hide Other Layers", "visibility visible solo")
        .on(Map)
        .keys(&[cmd_shift('h')]),
    /// Lock or unlock every other layer.
    ToggleLockOtherLayers => ActionSpec::new(ActionId::ToggleLockOtherLayers, "toggle_lock_other_layers", "Lock/Unlock Other Layers", "locked")
        .on(Map)
        .keys(&[cmd_shift('l')]),
    /// The current layer's properties.
    LayerProperties => ActionSpec::new(ActionId::LayerProperties, "layer_properties", "Layer Properties…", "attributes custom")
        .on(Map),
    /// Copy the selected objects.
    DuplicateObjects => ActionSpec::new(ActionId::DuplicateObjects, "duplicate_objects", "Duplicate Objects", "copy clone")
        .on(ObjectLayer)
        .keys(&[cmd('d')]),
    /// Mirror the brush, the carried tiles or the selected objects left to right.
    FlipHorizontal => ActionSpec::new(ActionId::FlipHorizontal, "flip_horizontal", "Flip Horizontally", "mirror")
        .on(Map)
        .keys(&[plain('x')]),
    /// Mirror them top to bottom.
    FlipVertical => ActionSpec::new(ActionId::FlipVertical, "flip_vertical", "Flip Vertically", "mirror")
        .on(Map)
        .keys(&[plain('y')]),
    /// Turn them counterclockwise.
    RotateLeft => ActionSpec::new(ActionId::RotateLeft, "rotate_left", "Rotate Left", "turn counterclockwise")
        .on(Map)
        .keys(&[shift('z')]),
    /// Turn them clockwise.
    RotateRight => ActionSpec::new(ActionId::RotateRight, "rotate_right", "Rotate Right", "turn clockwise")
        .on(Map)
        .keys(&[plain('z')]),
    /// Move the selected objects up a step.
    RaiseObjects => ActionSpec::new(ActionId::RaiseObjects, "raise_objects", "Raise Objects", "order up front")
        .on(ObjectLayer)
        .keys(&[named(Key::PageUp)]),
    /// Move the selected objects down a step.
    LowerObjects => ActionSpec::new(ActionId::LowerObjects, "lower_objects", "Lower Objects", "order down back")
        .on(ObjectLayer)
        .keys(&[named(Key::PageDown)]),
    /// Move the selected objects to the top.
    RaiseObjectsToTop => ActionSpec::new(ActionId::RaiseObjectsToTop, "raise_objects_to_top", "Raise Objects to Top", "order front")
        .on(ObjectLayer)
        .keys(&[named(Key::Home)]),
    /// Move the selected objects to the bottom.
    LowerObjectsToBottom => ActionSpec::new(ActionId::LowerObjectsToBottom, "lower_objects_to_bottom", "Lower Objects to Bottom", "order back")
        .on(ObjectLayer)
        .keys(&[named(Key::End)]),
    /// Move the selected objects to another layer.
    MoveObjectsToLayer => ActionSpec::new(ActionId::MoveObjectsToLayer, "move_objects_to_layer", "Move Objects to Layer…", "group")
        .on(ObjectLayer),
    /// Paint a random tile of the brush in each cell.
    RandomMode => ActionSpec::new(ActionId::RandomMode, "random_mode", "Random Mode", "brush stamp variation")
        .on(TileLayer)
        .keys(&[plain('d')])
        .toggle(),
    /// Make the brush bigger.
    BrushSizeUp => ActionSpec::new(ActionId::BrushSizeUp, "brush_size_up", "Bigger Brush", "size grow")
        .on(TileLayer)
        .keys(&[plain(']')])
        .repeating(),
    /// Make the brush smaller.
    BrushSizeDown => ActionSpec::new(ActionId::BrushSizeDown, "brush_size_down", "Smaller Brush", "size shrink")
        .on(TileLayer)
        .keys(&[plain('[')])
        .repeating(),
    /// The Brush.
    BrushTool => ActionSpec::new(ActionId::BrushTool, "brush_tool", "Brush", "stamp paint tool")
        .on(TileLayer)
        .keys(&[plain('b')])
        .tool(ToolKind::Brush),
    /// The Eraser.
    EraserTool => ActionSpec::new(ActionId::EraserTool, "eraser_tool", "Eraser", "erase delete tool")
        .on(TileLayer)
        .keys(&[plain('e')])
        .tool(ToolKind::Eraser),
    /// Bucket Fill.
    BucketFillTool => ActionSpec::new(ActionId::BucketFillTool, "bucket_fill_tool", "Bucket Fill", "flood paint tool")
        .on(TileLayer)
        .keys(&[plain('f')])
        .tool(ToolKind::BucketFill),
    /// Shape Fill.
    ShapeFillTool => ActionSpec::new(ActionId::ShapeFillTool, "shape_fill_tool", "Shape Fill", "rectangle ellipse circle paint tool")
        .on(TileLayer)
        .keys(&[plain('p')])
        .tool(ToolKind::ShapeFill),
    /// The Picker.
    PickerTool => ActionSpec::new(ActionId::PickerTool, "picker_tool", "Picker", "eyedropper pick tile tool")
        .on(TileLayer)
        .keys(&[plain('i')])
        .tool(ToolKind::Picker),
    /// Rectangular Select.
    RectSelectTool => ActionSpec::new(ActionId::RectSelectTool, "rect_select_tool", "Rectangular Select", "selection marquee tool")
        .on(TileLayer)
        .keys(&[plain('r')])
        .tool(ToolKind::RectSelect),
    /// The Magic Wand.
    MagicWandTool => ActionSpec::new(ActionId::MagicWandTool, "magic_wand_tool", "Magic Wand", "selection connected tool")
        .on(TileLayer)
        .keys(&[plain('w')])
        .tool(ToolKind::MagicWand),
    /// Select Same Tile.
    SameTileTool => ActionSpec::new(ActionId::SameTileTool, "same_tile_tool", "Select Same Tile", "selection tool")
        .on(TileLayer)
        .keys(&[plain('s')])
        .tool(ToolKind::SameTile),
    /// Offset Layers.
    OffsetLayersTool => ActionSpec::new(ActionId::OffsetLayersTool, "offset_layers_tool", "Offset Layers", "move layer tool")
        .on(Map)
        .keys(&[plain('m')])
        .tool(ToolKind::OffsetLayers),
    /// Select Objects.
    SelectObjectsTool => ActionSpec::new(ActionId::SelectObjectsTool, "select_objects_tool", "Select Objects", "move resize rotate tool")
        .on(ObjectLayer)
        .keys(&[plain('s')])
        .tool(ToolKind::SelectObjects),
    /// Insert Rectangle.
    InsertRectangleTool => ActionSpec::new(ActionId::InsertRectangleTool, "insert_rectangle_tool", "Insert Rectangle", "object box tool")
        .on(ObjectLayer)
        .keys(&[plain('r')])
        .tool(ToolKind::InsertRectangle),
    /// Insert Ellipse.
    InsertEllipseTool => ActionSpec::new(ActionId::InsertEllipseTool, "insert_ellipse_tool", "Insert Ellipse", "object circle tool")
        .on(ObjectLayer)
        .keys(&[plain('c')])
        .tool(ToolKind::InsertEllipse),
    /// Insert Capsule.
    InsertCapsuleTool => ActionSpec::new(ActionId::InsertCapsuleTool, "insert_capsule_tool", "Insert Capsule", "object pill tool")
        .on(ObjectLayer)
        .keys(&[shift('c')])
        .tool(ToolKind::InsertCapsule),
    /// Insert Point.
    InsertPointTool => ActionSpec::new(ActionId::InsertPointTool, "insert_point_tool", "Insert Point", "object marker tool")
        .on(ObjectLayer)
        .keys(&[plain('i')])
        .tool(ToolKind::InsertPoint),
    /// Insert Polygon.
    InsertPolygonTool => ActionSpec::new(ActionId::InsertPolygonTool, "insert_polygon_tool", "Insert Polygon", "object polyline line shape tool")
        .on(ObjectLayer)
        .keys(&[plain('p')])
        .tool(ToolKind::InsertPolygon),
    /// Insert Tile.
    InsertTileTool => ActionSpec::new(ActionId::InsertTileTool, "insert_tile_tool", "Insert Tile", "object image sprite tool")
        .on(ObjectLayer)
        .keys(&[plain('t')])
        .tool(ToolKind::InsertTile),
    /// Insert Text.
    InsertTextTool => ActionSpec::new(ActionId::InsertTextTool, "insert_text_tool", "Insert Text", "object label tool")
        .on(ObjectLayer)
        .keys(&[plain('e')])
        .tool(ToolKind::InsertText),
    /// Edit Polygons.
    EditPolygonsTool => ActionSpec::new(ActionId::EditPolygonsTool, "edit_polygons_tool", "Edit Polygons", "object points nodes tool")
        .on(ObjectLayer)
        .keys(&[plain('o')])
        .tool(ToolKind::EditPolygons),
}

/// The command that chooses `tool`.
pub fn tool_action(tool: ToolKind) -> ActionId {
    match tool {
        ToolKind::Brush => ActionId::BrushTool,
        ToolKind::Eraser => ActionId::EraserTool,
        ToolKind::BucketFill => ActionId::BucketFillTool,
        ToolKind::ShapeFill => ActionId::ShapeFillTool,
        ToolKind::Picker => ActionId::PickerTool,
        ToolKind::RectSelect => ActionId::RectSelectTool,
        ToolKind::MagicWand => ActionId::MagicWandTool,
        ToolKind::SameTile => ActionId::SameTileTool,
        ToolKind::OffsetLayers => ActionId::OffsetLayersTool,
        ToolKind::SelectObjects => ActionId::SelectObjectsTool,
        ToolKind::InsertRectangle => ActionId::InsertRectangleTool,
        ToolKind::InsertEllipse => ActionId::InsertEllipseTool,
        ToolKind::InsertCapsule => ActionId::InsertCapsuleTool,
        ToolKind::InsertPoint => ActionId::InsertPointTool,
        ToolKind::InsertPolygon => ActionId::InsertPolygonTool,
        ToolKind::InsertTile => ActionId::InsertTileTool,
        ToolKind::InsertText => ActionId::InsertTextTool,
        ToolKind::EditPolygons => ActionId::EditPolygonsTool,
    }
}

impl ActionId {
    /// The command with a stable name.
    pub fn from_name(name: &str) -> Option<ActionId> {
        ACTIONS
            .iter()
            .find(|spec| spec.name == name)
            .map(|spec| spec.id)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_spec_names_its_own_command_once() {
        for spec in ACTIONS {
            assert_eq!(spec.id.spec(), spec, "{:?} declares another id", spec.id);
            assert_eq!(ActionId::from_name(spec.name), Some(spec.id));
            if let ActionKind::Tool(tool) = spec.kind {
                assert_eq!(tool_action(tool), spec.id);
            }
        }
        let names: HashSet<&str> = ACTIONS.iter().map(|s| s.name).collect();
        assert_eq!(names.len(), ACTIONS.len());
    }

    #[test]
    fn no_key_serves_two_commands_that_can_apply_at_once() {
        for mac in [false, true] {
            for (i, a) in ACTIONS.iter().enumerate() {
                for b in ACTIONS.iter().skip(i + 1) {
                    if !a.context.overlaps(b.context) {
                        continue;
                    }
                    let (keys_a, keys_b) = if mac {
                        (a.keys_mac, b.keys_mac)
                    } else {
                        (a.keys, b.keys)
                    };
                    for chord in keys_a {
                        assert!(
                            !keys_b.contains(chord),
                            "{} is bound to both {} and {} (mac: {mac})",
                            chord.label(mac),
                            a.name,
                            b.name
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn tile_and_object_layers_share_tool_keys_but_never_overlap() {
        assert!(!Context::TileLayer.overlaps(Context::ObjectLayer));
        assert!(Context::Map.overlaps(Context::TileLayer));
        assert!(!Context::Tileset.overlaps(Context::Map));
        assert!(!Context::Tileset.overlaps(Context::TileLayer));
        assert!(Context::Tileset.overlaps(Context::ObjectLayer));
        assert!(Context::Document.overlaps(Context::Tileset));
        assert_eq!(
            ActionId::SameTileTool.spec().keys,
            ActionId::SelectObjectsTool.spec().keys
        );
    }
}
