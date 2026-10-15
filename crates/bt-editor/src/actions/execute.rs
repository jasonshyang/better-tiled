//! Running commands: each makes transactions, changes the tool state, or returns effects for
//! the window. Nothing here opens a dialog or touches a file.

use bt_core::{Map, MapGeometry};
use bt_edit::{Axis, Brush, LayerType, RotateDirection};

use super::enabled::Enabled;
use super::table::{ActionId, ActionKind};
use crate::automap::LiveEdit;
use crate::document::{DocumentId, Inspecting, Reorient};
use crate::editor::{Editor, warn};
use crate::effect::{Dialog, Effect, FileRequest, ViewRequest};
use crate::notice::Notice;
use crate::objects::{NodeCommand, Restack};
use crate::settings::Snapping;

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

    /// Whether X, Y and Z mirror and turn the selected tiles rather than the brush: a tool that
    /// selects tiles is current and tiles are selected.
    fn reorients_tiles(&self) -> bool {
        self.tools.tool().selects_tiles()
            && self
                .situation()
                .document
                .is_some_and(|d| !d.selection().tiles.is_empty())
    }

    /// Whether Delete takes the selected nodes rather than the objects: Edit Polygons is the
    /// tool and nodes are selected.
    fn edits_nodes(&self) -> bool {
        self.tools.tool() == crate::tools::ToolKind::EditPolygons
            && self
                .situation()
                .document
                .is_some_and(|d| !d.selection().nodes.is_empty())
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
            if tool == crate::ToolKind::InsertTemplate && self.chosen_template().is_none() {
                return self
                    .current()
                    .map(|doc| Effect::PickFile(FileRequest::ObjectTemplate(doc)))
                    .into_iter()
                    .collect();
            }
            self.tools.set_tool(tool);
            return Vec::new();
        }
        let current = self.documents.current();
        let on_document =
            |editor: &mut Editor, f: &dyn Fn(&mut Editor, DocumentId) -> Vec<Effect>| {
                current.map(|doc| f(editor, doc)).unwrap_or_default()
            };
        // Whether the current layer holds objects, so the shared commands act on them.
        let objects = self.edits_objects();
        match id {
            ActionId::NewMap => vec![Effect::Show(Dialog::NewMap)],
            ActionId::NewTileset => {
                on_document(self, &|_, doc| vec![Effect::Show(Dialog::NewTileset(doc))])
            }
            ActionId::ChooseTemplate => on_document(self, &|_, doc| {
                vec![Effect::PickFile(FileRequest::ObjectTemplate(doc))]
            }),
            ActionId::SaveAsTemplate => on_document(self, &|editor, doc| {
                editor
                    .document(doc)
                    .and_then(|doc| doc.selection().objects.first())
                    .map(|object| {
                        Effect::PickFile(FileRequest::SaveAsTemplate {
                            doc,
                            object: *object,
                        })
                    })
                    .into_iter()
                    .collect()
            }),
            ActionId::EditTemplate => on_document(self, &Editor::edit_instance_template),
            ActionId::ResetTemplate | ActionId::DetachTemplate | ActionId::ReplaceWithTemplate => {
                on_document(self, &|editor, doc| editor.edit_template_instances(doc, id))
            }
            ActionId::NewProject => vec![Effect::PickFile(FileRequest::NewProject)],
            ActionId::CloseProject => self.begin_project_switch(crate::files::ProjectSwitch::Close),
            ActionId::ProjectProperties => self
                .show_project_document()
                .map(|doc| Effect::Show(Dialog::ProjectProperties(doc)))
                .into_iter()
                .collect(),
            ActionId::SaveProjectAs => self
                .project_asset()
                .map(|project| Effect::PickFile(FileRequest::ProjectAs(project)))
                .into_iter()
                .collect(),
            ActionId::AddProjectFolder => self
                .project_asset()
                .map(|project| Effect::PickFile(FileRequest::ProjectFolder(project)))
                .into_iter()
                .collect(),
            ActionId::RefreshProject => self.refresh_project_files(),
            ActionId::NewWorld => on_document(self, &|_, doc| {
                vec![Effect::PickFile(FileRequest::NewWorld(doc))]
            }),
            ActionId::WorldProperties => on_document(self, &|_, doc| {
                vec![Effect::Show(Dialog::WorldProperties(doc))]
            }),
            ActionId::AddMapToWorld => on_document(self, &|editor, doc| {
                editor
                    .world_of(doc)
                    .map(|world| Effect::PickFile(FileRequest::WorldMap { doc, world }))
                    .into_iter()
                    .collect()
            }),
            ActionId::UnloadWorld => on_document(self, &|editor, doc| {
                editor
                    .world_of(doc)
                    .map(|world| editor.unload_world(world))
                    .unwrap_or_default()
            }),
            ActionId::Open => vec![Effect::PickFile(FileRequest::Open)],
            ActionId::Save => on_document(self, &Editor::save),
            ActionId::SaveAs => on_document(self, &|editor, doc| vec![editor.save_as_request(doc)]),
            ActionId::Export
            | ActionId::ExportAs
            | ActionId::GodotSetup
            | ActionId::VerifyGodot => current
                .map(|doc| {
                    let operation = match id {
                        ActionId::ExportAs => crate::GodotOperation::ExportAs,
                        ActionId::GodotSetup => crate::GodotOperation::Setup,
                        ActionId::VerifyGodot => crate::GodotOperation::Verify,
                        _ => crate::GodotOperation::Export,
                    };
                    vec![Effect::Godot { doc, operation }]
                })
                .unwrap_or_default(),
            ActionId::ExportAsImage => {
                on_document(self, &|_, doc| vec![Effect::Show(Dialog::ExportImage(doc))])
            }
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
            ActionId::Close => on_document(self, &Editor::close_document),
            ActionId::Preferences => vec![Effect::Show(Dialog::Preferences)],
            ActionId::Quit => self.quit(),
            ActionId::Undo => on_document(self, &|editor, doc| {
                editor.undo(doc).err().map(warn).into_iter().collect()
            }),
            ActionId::Redo => on_document(self, &|editor, doc| {
                editor.redo(doc).err().map(warn).into_iter().collect()
            }),
            ActionId::SelectAll => on_document(self, &Editor::select_all),
            ActionId::SelectNone => on_document(self, &Editor::select_none),
            ActionId::InvertSelection => on_document(self, &Editor::invert_selection),
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
            ActionId::ShowPainted => {
                self.toggle_view(|view| view.show_painted = !view.show_painted);
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
            ActionId::CustomTypesEditor => vec![Effect::Show(Dialog::CustomTypes)],
            ActionId::ShowConsole => vec![Effect::Show(Dialog::Console)],
            ActionId::EditCommands => current
                .map(|doc| Effect::Show(Dialog::EditCommands(doc)))
                .into_iter()
                .collect(),
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
            | ActionId::TerrainTool
            | ActionId::EraserTool
            | ActionId::BucketFillTool
            | ActionId::ShapeFillTool
            | ActionId::PickerTool
            | ActionId::RectSelectTool
            | ActionId::MagicWandTool
            | ActionId::SameTileTool
            | ActionId::WorldTool
            | ActionId::OffsetLayersTool
            | ActionId::ExplainTool
            | ActionId::SelectObjectsTool
            | ActionId::InsertRectangleTool
            | ActionId::InsertEllipseTool
            | ActionId::InsertCapsuleTool
            | ActionId::InsertPointTool
            | ActionId::InsertPolygonTool
            | ActionId::InsertTileTool
            | ActionId::InsertTextTool
            | ActionId::InsertTemplateTool
            | ActionId::EditPolygonsTool => Vec::new(),
            ActionId::Cut if objects => on_document(self, &Editor::cut_selected_objects),
            ActionId::Cut => on_document(self, &Editor::cut_tiles),
            ActionId::Copy if objects => on_document(self, &Editor::copy_selected_objects),
            ActionId::Copy => on_document(self, &Editor::copy_tiles),
            ActionId::Paste if objects => current
                .map(|doc| vec![Effect::PasteAtPointer(doc)])
                .unwrap_or_default(),
            ActionId::Paste => self.paste_tiles(),
            ActionId::PasteInPlace if objects => {
                on_document(self, &|editor, doc| editor.paste_copied_objects(doc, None))
            }
            ActionId::PasteInPlace => on_document(self, &Editor::paste_tiles_in_place),
            ActionId::Delete if objects && self.edits_nodes() => {
                on_document(self, &|editor, doc| {
                    editor.change_selected_nodes(doc, NodeCommand::Delete)
                })
            }
            ActionId::Delete if objects => on_document(self, &Editor::delete_objects),
            ActionId::JoinNodes => on_document(self, &|editor, doc| {
                editor.change_selected_nodes(doc, NodeCommand::Join)
            }),
            ActionId::SplitSegments => on_document(self, &|editor, doc| {
                editor.change_selected_nodes(doc, NodeCommand::Split)
            }),
            ActionId::Delete => on_document(self, &Editor::delete_tiles),
            ActionId::FlipHorizontal | ActionId::FlipVertical => {
                let axis = if id == ActionId::FlipHorizontal {
                    Axis::Horizontal
                } else {
                    Axis::Vertical
                };
                if objects {
                    return on_document(self, &|editor, doc| {
                        editor.flip_selected_objects(doc, axis)
                    });
                }
                if self.reorients_tiles() {
                    return on_document(self, &|editor, doc| {
                        editor.reorient_selected_tiles(doc, Reorient::Flip(axis))
                    });
                }
                self.transform_brush(|brush, geometry| brush.flipped(axis, geometry));
                Vec::new()
            }
            ActionId::RotateLeft | ActionId::RotateRight => {
                let clockwise = id == ActionId::RotateRight;
                if objects {
                    let degrees = if clockwise { 90.0 } else { -90.0 };
                    return on_document(self, &|editor, doc| {
                        editor.rotate_selected_objects(doc, degrees)
                    });
                }
                let direction = if clockwise {
                    RotateDirection::Clockwise
                } else {
                    RotateDirection::CounterClockwise
                };
                if self.reorients_tiles() {
                    return on_document(self, &|editor, doc| {
                        editor.reorient_selected_tiles(doc, Reorient::Rotate(direction))
                    });
                }
                self.transform_brush(|brush, geometry| brush.rotated(direction, geometry));
                Vec::new()
            }
            ActionId::DuplicateObjects => on_document(self, &Editor::duplicate_selected_objects),
            ActionId::RaiseObjects => on_document(self, &|editor, doc| {
                editor.restack_objects(doc, Restack::Raise)
            }),
            ActionId::LowerObjects => on_document(self, &|editor, doc| {
                editor.restack_objects(doc, Restack::Lower)
            }),
            ActionId::RaiseObjectsToTop => on_document(self, &|editor, doc| {
                editor.restack_objects(doc, Restack::ToTop)
            }),
            ActionId::LowerObjectsToBottom => on_document(self, &|editor, doc| {
                editor.restack_objects(doc, Restack::ToBottom)
            }),
            ActionId::MoveObjectsToLayer => current
                .map(|doc| vec![Effect::Show(Dialog::MoveObjectsToLayer(doc))])
                .unwrap_or_default(),
            ActionId::CropToSelection => on_document(self, &Editor::crop_to_selection),
            ActionId::Autocrop => on_document(self, &Editor::autocrop_map),
            ActionId::AutoMap => on_document(self, &Editor::automap),
            ActionId::LiveAutoMap => on_document(self, &|editor, doc| {
                let on = editor.document(doc).is_some_and(|d| d.automap().is_some());
                let edit = if on { LiveEdit::Stop } else { LiveEdit::Start };
                editor.edit_live(doc, edit)
            }),
            ActionId::NewTileLayer => {
                on_document(self, &|editor, doc| editor.add_layer(doc, LayerType::Tile))
            }
            ActionId::NewObjectLayer => on_document(self, &|editor, doc| {
                editor.add_layer(doc, LayerType::Object)
            }),
            ActionId::NewImageLayer => {
                on_document(self, &|editor, doc| editor.add_layer(doc, LayerType::Image))
            }
            ActionId::NewGroupLayer => {
                on_document(self, &|editor, doc| editor.add_layer(doc, LayerType::Group))
            }
            ActionId::LayerViaCopy => {
                on_document(self, &|editor, doc| editor.layer_via(doc, false))
            }
            ActionId::LayerViaCut => on_document(self, &|editor, doc| editor.layer_via(doc, true)),
            ActionId::GroupLayers => on_document(self, &Editor::group_selected_layers),
            ActionId::UngroupLayers => on_document(self, &Editor::ungroup_selected_layers),
            ActionId::DuplicateLayers => on_document(self, &Editor::duplicate_selected_layers),
            ActionId::MergeLayerDown => on_document(self, &Editor::merge_current_layer_down),
            ActionId::RemoveLayers => on_document(self, &Editor::remove_selected_layers),
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
                on_document(self, &|editor, doc| editor.step_selected_layers(doc, true))
            }
            ActionId::LowerLayers => {
                on_document(self, &|editor, doc| editor.step_selected_layers(doc, false))
            }
            ActionId::ToggleSelectedLayers => on_document(self, &|editor, doc| {
                editor.toggle_selected_layers(doc, false)
            }),
            ActionId::ToggleLockSelectedLayers => on_document(self, &|editor, doc| {
                editor.toggle_selected_layers(doc, true)
            }),
            ActionId::ToggleOtherLayers => {
                on_document(self, &|editor, doc| editor.toggle_other_layers(doc, false))
            }
            ActionId::ToggleLockOtherLayers => {
                on_document(self, &|editor, doc| editor.toggle_other_layers(doc, true))
            }
            ActionId::ResizeMap => {
                on_document(self, &|_, doc| vec![Effect::Show(Dialog::ResizeMap(doc))])
            }
            ActionId::OffsetMap => {
                on_document(self, &|_, doc| vec![Effect::Show(Dialog::OffsetMap(doc))])
            }
            ActionId::AddExternalTileset => on_document(self, &|_, doc| {
                vec![Effect::PickFile(FileRequest::ExternalTileset(doc))]
            }),
            ActionId::MapProperties => {
                if let Some(doc) = current {
                    self.set_inspecting(doc, Inspecting::Map);
                }
                Vec::new()
            }
            ActionId::LayerProperties => {
                if let Some(doc) = current {
                    self.set_inspecting(doc, Inspecting::Layers);
                }
                Vec::new()
            }
        }
    }
}
