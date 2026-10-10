//! Running commands: each makes transactions, changes the tool state, or returns effects for
//! the window. Nothing here opens a dialog or touches a file.

use bt_core::{Map, MapGeometry};
use bt_edit::{Brush, Axis, LayerType, RotateDirection};

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
        ActionId::ResizeMap
            | ActionId::OffsetMap
            | ActionId::AddExternalTileset
            | ActionId::MapProperties
            | ActionId::LayerProperties
            | ActionId::DuplicateObjects
            | ActionId::RaiseObjects
            | ActionId::LowerObjects
            | ActionId::RaiseObjectsToTop
            | ActionId::LowerObjectsToBottom
            | ActionId::MoveObjectsToLayer
    )
}

impl Editor {
    /// Mirrors or turns the brush on the current map's grid.
    fn transform_brush(&mut self, transform: impl FnOnce(&Brush, &MapGeometry) -> Brush) {
        let Some(geometry) = self.situation().map.map(Map::geometry) else {
            return;
        };
        if let Some(brush) = &self.tools.brush {
            self.tools.brush = Some(transform(brush, &geometry));
        }
    }

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
                self.tools.set_random(!self.tools.random);
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
            ActionId::Cut => on_document(self, Editor::cut_tiles),
            ActionId::Copy => on_document(self, Editor::copy_tiles),
            ActionId::Paste => self.paste_tiles(),
            ActionId::PasteInPlace => on_document(self, Editor::paste_tiles_in_place),
            ActionId::Delete => on_document(self, Editor::delete_tiles),
            ActionId::FlipHorizontal | ActionId::FlipVertical => {
                let direction = if id == ActionId::FlipHorizontal {
                    Axis::Horizontal
                } else {
                    Axis::Vertical
                };
                self.transform_brush(|brush, geometry| brush.flipped(direction, geometry));
                Vec::new()
            }
            ActionId::RotateLeft | ActionId::RotateRight => {
                let direction = if id == ActionId::RotateRight {
                    RotateDirection::Clockwise
                } else {
                    RotateDirection::CounterClockwise
                };
                self.transform_brush(|brush, geometry| brush.rotated(direction, geometry));
                Vec::new()
            }
            ActionId::CropToSelection => on_document(self, Editor::crop_to_selection),
            ActionId::Autocrop => on_document(self, Editor::autocrop_map),
            ActionId::NewTileLayer => {
                on_document(self, |editor, doc| editor.add_layer(doc, LayerType::Tile))
            }
            ActionId::NewObjectLayer => {
                on_document(self, |editor, doc| editor.add_layer(doc, LayerType::Object))
            }
            ActionId::NewImageLayer => {
                on_document(self, |editor, doc| editor.add_layer(doc, LayerType::Image))
            }
            ActionId::NewGroupLayer => {
                on_document(self, |editor, doc| editor.add_layer(doc, LayerType::Group))
            }
            ActionId::LayerViaCopy => on_document(self, |editor, doc| editor.layer_via(doc, false)),
            ActionId::LayerViaCut => on_document(self, |editor, doc| editor.layer_via(doc, true)),
            ActionId::GroupLayers => on_document(self, Editor::group_selected_layers),
            ActionId::UngroupLayers => on_document(self, Editor::ungroup_selected_layers),
            ActionId::DuplicateLayers => on_document(self, Editor::duplicate_selected_layers),
            ActionId::MergeLayerDown => on_document(self, Editor::merge_current_layer_down),
            ActionId::RemoveLayers => on_document(self, Editor::remove_selected_layers),
            ActionId::SelectPreviousLayer | ActionId::SelectNextLayer => {
                if let Some(doc) = current {
                    self.select_neighbour_layer(doc, id == ActionId::SelectNextLayer);
                }
                Vec::new()
            }
            ActionId::SelectAllLayers => {
                if let Some(doc) = current {
                    self.select_all_layers(doc);
                }
                Vec::new()
            }
            ActionId::RaiseLayers => {
                on_document(self, |editor, doc| editor.step_selected_layers(doc, true))
            }
            ActionId::LowerLayers => {
                on_document(self, |editor, doc| editor.step_selected_layers(doc, false))
            }
            ActionId::ToggleSelectedLayers => on_document(self, |editor, doc| {
                editor.toggle_selected_layers(doc, false)
            }),
            ActionId::ToggleLockSelectedLayers => {
                on_document(self, |editor, doc| editor.toggle_selected_layers(doc, true))
            }
            ActionId::ToggleOtherLayers => {
                on_document(self, |editor, doc| editor.toggle_other_layers(doc, false))
            }
            ActionId::ToggleLockOtherLayers => {
                on_document(self, |editor, doc| editor.toggle_other_layers(doc, true))
            }
            ActionId::ResizeMap
            | ActionId::OffsetMap
            | ActionId::AddExternalTileset
            | ActionId::MapProperties
            | ActionId::LayerProperties
            | ActionId::DuplicateObjects
            | ActionId::RaiseObjects
            | ActionId::LowerObjects
            | ActionId::RaiseObjectsToTop
            | ActionId::LowerObjectsToBottom
            | ActionId::MoveObjectsToLayer => Vec::new(),
        }
    }
}
