//! The tileset editor's collision view: the current tile on a canvas of its own, its collision
//! shapes edited with the object tools (Tiled's tile collision editor,
//! `reference/tiled/src/tiled/tilecollisiondock.cpp`). The canvas draws the one-cell map the
//! editor builds of the tile and its shapes, built again when the tileset changes; another
//! tile starts with the camera framing it.

use std::collections::HashMap;
use std::sync::Arc;

use bt_core::{Map, TileId, Tileset};
use bt_editor::{DocumentId, Editor, Intent, ViewToggles};
use glam::DVec2;

use crate::canvas::{Canvas, CanvasFrame, CanvasGpu, CanvasRole};
use crate::tools::ToolBox;

/// The collision views of the documents.
#[derive(Debug, Default)]
pub(crate) struct CollisionView {
    views: HashMap<DocumentId, TileCanvas>,
}

/// A document's collision view.
#[derive(Debug)]
struct TileCanvas {
    canvas: Canvas,
    /// The tile shown.
    tile: TileId,
    /// The tileset the map was built from, by identity: an edit makes a new one.
    tileset: usize,
    /// The map drawn.
    map: Arc<Map>,
}

/// What a frame of the collision view works with.
pub(crate) struct CollisionFrame<'a> {
    /// The editor.
    pub(crate) editor: &'a Editor,
    /// The document.
    pub(crate) doc: DocumentId,
    /// The tileset edited.
    pub(crate) tileset: &'a Arc<Tileset>,
    /// The tile shown.
    pub(crate) tile: TileId,
    /// The renderer; `None` in the window's tests.
    pub(crate) gpu: Option<&'a CanvasGpu>,
    /// A text field has the keyboard.
    pub(crate) typing: bool,
    /// A dialog is open.
    pub(crate) dialog_open: bool,
}

impl CollisionView {
    /// Shows the current tile's collision view in the rest of `ui`.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        frame: CollisionFrame<'_>,
        tools: &mut ToolBox,
        out: &mut Vec<Intent>,
    ) {
        let doc = frame.doc;
        let identity = Arc::as_ptr(frame.tileset) as usize;
        let shown = self.views.get(&doc);
        let same_tile = shown.is_some_and(|view| view.tile == frame.tile);
        if !(same_tile && shown.is_some_and(|view| view.tileset == identity)) {
            let Some(map) = frame.editor.collision_view(doc) else {
                ui.weak("The tile cannot be shown.");
                return;
            };
            let canvas = match self.views.remove(&doc) {
                Some(view) if same_tile => view.canvas,
                _ => Canvas::default(),
            };
            self.views.insert(
                doc,
                TileCanvas {
                    canvas,
                    tile: frame.tile,
                    tileset: identity,
                    map: Arc::new(map),
                },
            );
        }
        let Some(view) = self.views.get_mut(&doc) else {
            return;
        };
        let canvas_frame = CanvasFrame {
            doc,
            map: &view.map,
            saved_camera: None,
            toggles: ViewToggles::default(),
            selected_layers: &[],
            settings: frame.editor.settings(),
            gpu: frame.gpu,
            typing: frame.typing,
            dialog_open: frame.dialog_open,
            editor: frame.editor,
            role: CanvasRole::Collision,
        };
        view.canvas.show(ui, &canvas_frame, tools, out);
    }

    /// Where the pointer is over `doc`'s collision view, in the tile's pixels.
    pub(crate) fn pointer(&self, doc: DocumentId) -> Option<DVec2> {
        Some(self.views.get(&doc)?.canvas.hover()?.pixel)
    }

    /// `doc`'s collision view's canvas.
    pub(crate) fn canvas(&self, doc: DocumentId) -> Option<&Canvas> {
        Some(&self.views.get(&doc)?.canvas)
    }

    /// Where the middle of the tile is on screen in `doc`'s collision view (for the window's
    /// tests).
    #[cfg(test)]
    pub(crate) fn world_on_screen(&self, doc: DocumentId, world: DVec2) -> Option<egui::Pos2> {
        Some(
            self.views
                .get(&doc)?
                .canvas
                .last_view()?
                .world_to_screen(world),
        )
    }

    /// Forgets the views of documents that are closed.
    pub(crate) fn retain_documents(&mut self, open: &[DocumentId]) {
        self.views.retain(|doc, _| open.contains(doc));
    }
}
