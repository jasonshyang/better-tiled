//! What every tool is: a state machine over the canvas's gestures that previews what it would
//! do through overlays, says what it does in the status bar, and changes the map only by
//! sending intents.

use std::sync::Arc;

use bt_core::{Layer, LayerId, Map, TileLayer};
use bt_editor::{DocumentId, Editor, Intent, Selection, ToolKind};
use bt_render::{PreviewTile, VectorItem};

use crate::canvas::Gesture;

/// What a tool sees of the editor while it works, and where its intents go.
pub(crate) struct ToolContext<'a> {
    /// The editor.
    pub(crate) editor: &'a Editor,
    /// The document of the canvas.
    pub(crate) doc: DocumentId,
    /// Its map.
    pub(crate) map: &'a Arc<Map>,
    /// The intents the tool sends.
    pub(crate) out: &'a mut Vec<Intent>,
}

impl ToolContext<'_> {
    /// The document's selection.
    pub(crate) fn selection(&self) -> Option<&Selection> {
        self.editor.document(self.doc).map(|d| d.selection())
    }

    /// The current layer.
    pub(crate) fn current_layer(&self) -> Option<&Layer> {
        let id = self.selection()?.current_layer?;
        self.map.layer(id)
    }

    /// The current layer's cells, when it is a tile layer.
    pub(crate) fn tile_layer(&self) -> Option<&TileLayer> {
        self.current_layer()?.as_tile_layer()
    }

    /// How the status bar names the Command key: ⌘ on macOS, Ctrl elsewhere.
    pub(crate) fn command_key(&self) -> &'static str {
        if self.editor.keymap().is_mac() {
            "⌘"
        } else {
            "Ctrl"
        }
    }
}

/// What a tool draws over the map.
#[derive(Clone, Debug, Default)]
pub(crate) struct Overlay {
    /// Outlines, rectangles and fills, in the map's pixels on screen.
    pub(crate) vectors: Vec<VectorItem>,
    /// The layer previewed tiles are drawn above.
    pub(crate) preview_layer: Option<LayerId>,
    /// Tiles drawn as a preview.
    pub(crate) previews: Vec<PreviewTile>,
}

/// A tool of the canvas.
pub(crate) trait Tool {
    /// Which tool it is.
    fn kind(&self) -> ToolKind;

    /// Takes in a gesture; edits go to `cx.out` as intents.
    fn handle(&mut self, gesture: &Gesture, cx: &mut ToolContext<'_>);

    /// What it shows over the map now.
    fn overlay(&self, cx: &ToolContext<'_>, out: &mut Overlay);

    /// What the status bar says it does, with the meaning of the modifiers held.
    fn hint(&self, cx: &ToolContext<'_>) -> String;

    /// Stops whatever it was in the middle of (another tool or document became current); a
    /// stroke under way is finished through `out`.
    fn reset(&mut self, out: &mut Vec<Intent>);
}
