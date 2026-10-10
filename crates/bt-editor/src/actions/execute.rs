//! Running commands: each makes transactions, changes the tool state, or returns effects for
//! the window. Nothing here opens a dialog or touches a file.

use super::enabled::Enabled;
use super::table::{ActionId, ActionKind};
use crate::document::DocumentId;
use crate::editor::{Editor, warn};
use crate::effect::{Dialog, Effect, FileRequest, ViewRequest};
use crate::notice::Notice;
use crate::settings::Snapping;

/// The commands whose operations are still being built in this milestone: disabled, and
/// running them does nothing. The list empties as they arrive.
pub(crate) fn pending(id: ActionId) -> bool {
    matches!(
        id,
        ActionId::Cut
            | ActionId::Copy
            | ActionId::Paste
            | ActionId::PasteInPlace
            | ActionId::Delete
            | ActionId::ResizeMap
            | ActionId::OffsetMap
            | ActionId::CropToSelection
            | ActionId::Autocrop
            | ActionId::AddExternalTileset
            | ActionId::MapProperties
            | ActionId::NewTileLayer
            | ActionId::NewObjectLayer
            | ActionId::NewImageLayer
            | ActionId::NewGroupLayer
            | ActionId::LayerViaCopy
            | ActionId::LayerViaCut
            | ActionId::GroupLayers
            | ActionId::UngroupLayers
            | ActionId::DuplicateLayers
            | ActionId::MergeLayerDown
            | ActionId::RemoveLayers
            | ActionId::SelectPreviousLayer
            | ActionId::SelectNextLayer
            | ActionId::SelectAllLayers
            | ActionId::RaiseLayers
            | ActionId::LowerLayers
            | ActionId::ToggleSelectedLayers
            | ActionId::ToggleLockSelectedLayers
            | ActionId::ToggleOtherLayers
            | ActionId::ToggleLockOtherLayers
            | ActionId::LayerProperties
            | ActionId::DuplicateObjects
            | ActionId::FlipHorizontal
            | ActionId::FlipVertical
            | ActionId::RotateLeft
            | ActionId::RotateRight
            | ActionId::RaiseObjects
            | ActionId::LowerObjects
            | ActionId::RaiseObjectsToTop
            | ActionId::LowerObjectsToBottom
            | ActionId::MoveObjectsToLayer
    )
}

impl Editor {
    /// Chooses where objects snap; the preference is kept.
    fn set_snapping(&mut self, snapping: Snapping) -> Vec<Effect> {
        let mut settings = self.settings.clone();
        settings.behaviour.snapping = snapping;
        self.change_settings(settings)
    }

    /// Runs a command; a disabled one only says why, when it has a reason.
    pub(crate) fn run(&mut self, id: ActionId) -> Vec<Effect> {
        if let Enabled::Disabled(reason) = self.enabled(id) {
            return reason
                .map(|r| Effect::Notify(Notice::info(r)))
                .into_iter()
                .collect();
        }
        if let ActionKind::Tool(tool) = id.spec().kind {
            self.tools.set_tool(tool);
            return Vec::new();
        }
        let current = self.documents.current();
        let on_document = |editor: &mut Editor, f: fn(&mut Editor, DocumentId) -> Vec<Effect>| {
            current.map(|doc| f(editor, doc)).unwrap_or_default()
        };
        match id {
            ActionId::NewMap => vec![Effect::Show(Dialog::NewMap)],
            ActionId::NewTileset => {
                on_document(self, |_, doc| vec![Effect::Show(Dialog::NewTileset(doc))])
            }
            ActionId::Open => vec![Effect::PickFile(FileRequest::Open)],
            ActionId::Save => on_document(self, Editor::save),
            ActionId::SaveAs => on_document(self, |editor, doc| vec![editor.save_as_request(doc)]),
            ActionId::SaveAll => {
                let mut effects = self.save_all();
                let untitled = self
                    .documents
                    .in_order()
                    .find(|d| {
                        self.store
                            .get(d.asset())
                            .is_some_and(|a| a.is_dirty() && a.path().is_none())
                    })
                    .map(|d| d.id());
                if let Some(doc) = untitled {
                    effects.push(self.save_as_request(doc));
                }
                effects
            }
            ActionId::Close => on_document(self, Editor::close_document),
            ActionId::Preferences => vec![Effect::Show(Dialog::Preferences)],
            ActionId::Quit => self.quit(),
            ActionId::Undo => on_document(self, |editor, doc| {
                editor.undo(doc).err().map(warn).into_iter().collect()
            }),
            ActionId::Redo => on_document(self, |editor, doc| {
                editor.redo(doc).err().map(warn).into_iter().collect()
            }),
            ActionId::SelectAll => on_document(self, Editor::select_all),
            ActionId::SelectNone => on_document(self, Editor::select_none),
            ActionId::InvertSelection => on_document(self, Editor::invert_selection),
            ActionId::SearchActions => vec![Effect::Show(Dialog::SearchActions)],
            ActionId::ZoomIn => vec![Effect::View(ViewRequest::ZoomIn)],
            ActionId::ZoomOut => vec![Effect::View(ViewRequest::ZoomOut)],
            ActionId::ZoomNormal => vec![Effect::View(ViewRequest::ZoomNormal)],
            ActionId::FitInView => vec![Effect::View(ViewRequest::Fit)],
            ActionId::ShowGrid => {
                self.toggle_view(|view| view.grid = !view.grid);
                Vec::new()
            }
            ActionId::HighlightCurrentLayer => {
                self.toggle_view(|view| {
                    view.highlight_current_layer = !view.highlight_current_layer
                });
                Vec::new()
            }
            ActionId::NextDocument | ActionId::PreviousDocument => {
                let step = if id == ActionId::NextDocument { 1 } else { -1 };
                if let Some(doc) = self.documents.neighbour(step) {
                    self.documents.set_current(doc);
                    self.session_changed();
                }
                Vec::new()
            }
            ActionId::SnapNone => self.set_snapping(Snapping::None),
            ActionId::SnapToGrid => self.set_snapping(Snapping::Grid),
            ActionId::SnapToFineGrid => self.set_snapping(Snapping::FineGrid),
            ActionId::SnapToPixels => self.set_snapping(Snapping::Pixels),
            ActionId::RandomMode => {
                self.tools.random = !self.tools.random;
                Vec::new()
            }
            ActionId::BrushSizeUp => {
                self.tools.set_brush_size(self.tools.brush_size() + 1);
                Vec::new()
            }
            ActionId::BrushSizeDown => {
                self.tools
                    .set_brush_size(self.tools.brush_size().saturating_sub(1));
                Vec::new()
            }
            ActionId::BrushTool
            | ActionId::EraserTool
            | ActionId::BucketFillTool
            | ActionId::ShapeFillTool
            | ActionId::PickerTool
            | ActionId::RectSelectTool
            | ActionId::MagicWandTool
            | ActionId::SameTileTool
            | ActionId::OffsetLayersTool
            | ActionId::SelectObjectsTool
            | ActionId::InsertRectangleTool
            | ActionId::InsertEllipseTool
            | ActionId::InsertCapsuleTool
            | ActionId::InsertPointTool
            | ActionId::InsertPolygonTool
            | ActionId::InsertTileTool
            | ActionId::InsertTextTool
            | ActionId::EditPolygonsTool => Vec::new(),
            ActionId::Cut
            | ActionId::Copy
            | ActionId::Paste
            | ActionId::PasteInPlace
            | ActionId::Delete
            | ActionId::ResizeMap
            | ActionId::OffsetMap
            | ActionId::CropToSelection
            | ActionId::Autocrop
            | ActionId::AddExternalTileset
            | ActionId::MapProperties
            | ActionId::NewTileLayer
            | ActionId::NewObjectLayer
            | ActionId::NewImageLayer
            | ActionId::NewGroupLayer
            | ActionId::LayerViaCopy
            | ActionId::LayerViaCut
            | ActionId::GroupLayers
            | ActionId::UngroupLayers
            | ActionId::DuplicateLayers
            | ActionId::MergeLayerDown
            | ActionId::RemoveLayers
            | ActionId::SelectPreviousLayer
            | ActionId::SelectNextLayer
            | ActionId::SelectAllLayers
            | ActionId::RaiseLayers
            | ActionId::LowerLayers
            | ActionId::ToggleSelectedLayers
            | ActionId::ToggleLockSelectedLayers
            | ActionId::ToggleOtherLayers
            | ActionId::ToggleLockOtherLayers
            | ActionId::LayerProperties
            | ActionId::DuplicateObjects
            | ActionId::FlipHorizontal
            | ActionId::FlipVertical
            | ActionId::RotateLeft
            | ActionId::RotateRight
            | ActionId::RaiseObjects
            | ActionId::LowerObjects
            | ActionId::RaiseObjectsToTop
            | ActionId::LowerObjectsToBottom
            | ActionId::MoveObjectsToLayer => Vec::new(),
        }
    }
}
