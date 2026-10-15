//! Whether a command can run now, and why not; whether a toggle or a tool is on. Pure functions
//! of the editor's state.

use bt_core::{Layer, LayerKind, Map};

use super::table::{ActionId, ActionKind, ActionSpec, Context};
use crate::document::{Document, TilesetMode};
use crate::editor::Editor;
use crate::objects::ObjectHost;
use crate::settings::Snapping;
use crate::tools::ToolKind;

/// Whether a command can run now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Enabled {
    /// It can.
    Enabled,
    /// It cannot; the reason, when a key that does nothing would otherwise be a mystery.
    Disabled(Option<String>),
}

/// What commands depend on: the current document, its map and its current layer.
pub(crate) struct Situation<'e> {
    pub(crate) document: Option<&'e Document>,
    pub(crate) map: Option<&'e Map>,
    pub(crate) layer: Option<&'e Layer>,
}

impl Editor {
    /// Whether the current document's object commands act on objects now: a map's while its
    /// current layer is an object layer, or the current tile's collision shapes.
    pub(crate) fn edits_objects(&self) -> bool {
        let Some(doc) = self.documents.current() else {
            return false;
        };
        match self.object_host(doc) {
            Some(ObjectHost::Tile { .. }) => true,
            Some(ObjectHost::Map(_)) => self
                .situation()
                .layer
                .is_some_and(|l| matches!(l.kind, LayerKind::Object(_))),
            None => false,
        }
    }

    /// The current document, its map and current layer.
    pub(crate) fn situation(&self) -> Situation<'_> {
        let document = self
            .documents
            .current()
            .and_then(|doc| self.documents.get(doc));
        let map = document
            .and_then(|d| self.store.editing_map(d.asset()))
            .map(AsRef::as_ref);
        let layer = map.and_then(|m| {
            let id = document?.selection().current_layer?;
            m.layer(id)
        });
        Situation {
            document,
            map,
            layer,
        }
    }

    /// Whether a command can run now, with the reason when it cannot and its key would
    /// otherwise do nothing for no visible reason.
    pub fn enabled(&self, id: ActionId) -> Enabled {
        let here = self.situation();
        if self
            .current()
            .is_some_and(|doc| self.template_of(doc).is_some())
        {
            let map_command = matches!(id.spec().context, Context::Map | Context::TileLayer);
            let insert_tool = matches!(id.spec().kind, ActionKind::Tool(tool) if !matches!(tool, ToolKind::SelectObjects | ToolKind::EditPolygons));
            let changes_base_count = matches!(
                id,
                ActionId::Cut
                    | ActionId::Delete
                    | ActionId::Paste
                    | ActionId::PasteInPlace
                    | ActionId::DuplicateObjects
                    | ActionId::MoveObjectsToLayer
                    | ActionId::SaveAs
            );
            if map_command || insert_tool || changes_base_count {
                return Enabled::Disabled(Some("The template editor edits one base object; map and insertion commands apply to its instances".into()));
            }
        }
        if let Some(reason) = context_blocker(id.spec(), &here) {
            return Enabled::Disabled(reason);
        }
        let history = here.document.map(Document::history);
        let selection = here.document.map(Document::selection);
        let editing_tileset = here.document.is_some_and(|d| d.tileset_editing().is_some());
        let on_tiles = !editing_tileset
            && here
                .layer
                .is_some_and(|l| matches!(l.kind, LayerKind::Tile(_)));
        let on_objects = self.edits_objects();
        let has_tiles = selection.is_some_and(|s| !s.tiles.is_empty());
        let has_objects = selection.is_some_and(|s| !s.objects.is_empty());
        let has_layers = selection.is_some_and(|s| !s.layers.is_empty());
        let selects_group = selection.is_some_and(|s| {
            s.layers.iter().any(|id| {
                here.map
                    .and_then(|m| m.layer(*id))
                    .is_some_and(|l| matches!(l.kind, LayerKind::Group(_)))
            })
        });
        match id {
            ActionId::WorldTool if self.current().and_then(|doc| self.world_of(doc)).is_none() => {
                Enabled::Disabled(Some("Open a world containing this map first".into()))
            }
            ActionId::Undo if history.is_none_or(|h| h.undoable().is_none()) => {
                Enabled::Disabled(Some("There is nothing to undo".to_owned()))
            }
            ActionId::Redo if history.is_none_or(|h| h.redoable().is_none()) => {
                Enabled::Disabled(Some("There is nothing to redo".to_owned()))
            }
            ActionId::NextDocument | ActionId::PreviousDocument if self.documents.len() < 2 => {
                Enabled::Disabled(None)
            }
            ActionId::SelectNone if !has_tiles && !has_objects => Enabled::Disabled(None),
            ActionId::SelectAll | ActionId::InvertSelection
                if !on_objects && (here.map.is_none() || editing_tileset) =>
            {
                Enabled::Disabled(None)
            }
            ActionId::Cut | ActionId::Copy | ActionId::Delete
                if !(on_tiles && has_tiles || on_objects && has_objects) =>
            {
                Enabled::Disabled(None)
            }
            ActionId::Cut | ActionId::Delete if on_tiles => {
                let verb = if id == ActionId::Cut { "cut" } else { "delete" };
                match here
                    .document
                    .and_then(|d| self.tiles_locked_reason(d.id(), verb))
                {
                    Some(reason) => Enabled::Disabled(Some(reason)),
                    None => Enabled::Enabled,
                }
            }
            ActionId::Paste | ActionId::PasteInPlace
                if !(on_tiles && self.has_copied_tiles()
                    || on_objects && self.has_copied_objects()) =>
            {
                Enabled::Disabled(Some(if on_objects {
                    "No objects were copied".to_owned()
                } else {
                    "Nothing was copied".to_owned()
                }))
            }
            ActionId::FlipHorizontal
            | ActionId::FlipVertical
            | ActionId::RotateLeft
            | ActionId::RotateRight
                if !(on_tiles && self.tools.brush().is_some()
                    || on_tiles && has_tiles && self.tools.tool().selects_tiles()
                    || on_objects && has_objects) =>
            {
                Enabled::Disabled(None)
            }
            ActionId::DuplicateObjects | ActionId::MoveObjectsToLayer if !has_objects => {
                Enabled::Disabled(None)
            }
            ActionId::JoinNodes | ActionId::SplitSegments
                if self.tools.tool() != ToolKind::EditPolygons
                    || selection.is_none_or(|s| s.nodes.len() < 2) =>
            {
                Enabled::Disabled(Some(
                    "Select two nodes or more with Edit Polygons first".to_owned(),
                ))
            }
            ActionId::MoveObjectsToLayer if editing_tileset => {
                Enabled::Disabled(Some("Collision shapes stay with their tile".to_owned()))
            }
            ActionId::InsertTileTool | ActionId::InsertTextTool if editing_tileset => {
                Enabled::Disabled(Some(
                    "Collision shapes are rectangles, ellipses, capsules, points and polygons"
                        .to_owned(),
                ))
            }
            ActionId::RaiseObjects
            | ActionId::LowerObjects
            | ActionId::RaiseObjectsToTop
            | ActionId::LowerObjectsToBottom => {
                if !has_objects {
                    Enabled::Disabled(None)
                } else {
                    match here.document.and_then(|d| self.restack_reason(d.id())) {
                        Some(reason) => Enabled::Disabled(Some(reason)),
                        None => Enabled::Enabled,
                    }
                }
            }
            ActionId::CropToSelection if !has_tiles => Enabled::Disabled(None),
            ActionId::AutoMap if here.document.is_some_and(|d| d.automap().is_some()) => {
                Enabled::Disabled(Some(
                    "Live AutoMap is on: the rules run after every change".to_owned(),
                ))
            }
            ActionId::ShowPainted if here.document.is_none_or(|d| d.automap().is_none()) => {
                Enabled::Disabled(Some(
                    "Live AutoMap is off: what is shown is what was painted".to_owned(),
                ))
            }
            ActionId::ResizeMap => match here.map.and_then(crate::map_commands::resize_refusal) {
                Some(reason) => Enabled::Disabled(Some(reason)),
                None => Enabled::Enabled,
            },
            ActionId::MergeLayerDown
                if !here
                    .document
                    .is_some_and(|d| self.current_layer_merges_down(d.id())) =>
            {
                Enabled::Disabled(None)
            }
            ActionId::RemoveLayers | ActionId::GroupLayers | ActionId::DuplicateLayers
                if !has_layers =>
            {
                Enabled::Disabled(None)
            }
            ActionId::UngroupLayers if !selects_group => Enabled::Disabled(None),
            ActionId::LayerViaCopy | ActionId::LayerViaCut if !(has_tiles || has_objects) => {
                Enabled::Disabled(None)
            }
            _ => Enabled::Enabled,
        }
    }

    /// Whether a toggle is on or a tool is in use; `None` for other commands.
    pub fn checked(&self, id: ActionId) -> Option<bool> {
        if let ActionKind::Tool(tool) = id.spec().kind {
            return Some(self.tools.tool() == tool);
        }
        let view = self
            .situation()
            .document
            .map(Document::view)
            .unwrap_or_default();
        match id {
            ActionId::ShowGrid => Some(view.grid),
            ActionId::HighlightCurrentLayer => Some(view.highlight_current_layer),
            ActionId::ShowPainted => Some(view.show_painted),
            ActionId::LiveAutoMap => Some(
                self.situation()
                    .document
                    .is_some_and(|d| d.automap().is_some()),
            ),
            ActionId::RandomMode => Some(self.tools.random()),
            ActionId::SnapNone => Some(self.settings.behaviour.snapping == Snapping::None),
            ActionId::SnapToGrid => Some(self.settings.behaviour.snapping == Snapping::Grid),
            ActionId::SnapToFineGrid => {
                Some(self.settings.behaviour.snapping == Snapping::FineGrid)
            }
            ActionId::SnapToPixels => Some(self.settings.behaviour.snapping == Snapping::Pixels),
            _ => None,
        }
    }
}

/// Why a command's context does not hold: `None` when it holds, `Some(None)` when it does not
/// and needs no explaining (no document is open), `Some(Some(reason))` when a layer of the other
/// family is current, so a key pressed there says why it does nothing.
fn context_blocker(spec: &ActionSpec, here: &Situation<'_>) -> Option<Option<String>> {
    let kind = here.layer.map(|l| &l.kind);
    let editing = here.document.and_then(|d| d.tileset_editing());
    let on_map = here.map.is_some() && editing.is_none();
    let holds = match spec.context {
        Context::Always => true,
        Context::Document => here.document.is_some(),
        Context::Map => on_map,
        Context::TileLayer => on_map && matches!(kind, Some(LayerKind::Tile(_))),
        Context::ObjectLayer => {
            on_map && matches!(kind, Some(LayerKind::Object(_)))
                || editing.is_some_and(|e| e.mode() == TilesetMode::Collision)
        }
        Context::Tileset => editing.is_some(),
    };
    if holds {
        return None;
    }
    if editing.is_some() && matches!(spec.context, Context::Map | Context::TileLayer) {
        return Some(Some(format!(
            "{} works on the map: the tileset editor is open",
            spec.label.trim_end_matches('…')
        )));
    }
    let reason = match (spec.context, here.layer) {
        (Context::TileLayer | Context::ObjectLayer, Some(layer)) => {
            let wanted = if spec.context == Context::TileLayer {
                "tile layers"
            } else {
                "object layers"
            };
            Some(format!(
                "{} works on {wanted}: “{}” is {}",
                spec.label.trim_end_matches('…'),
                layer.name,
                kind_noun(&layer.kind)
            ))
        }
        _ => None,
    };
    Some(reason)
}

/// A layer kind with its article: "a tile layer".
fn kind_noun(kind: &LayerKind) -> &'static str {
    match kind {
        LayerKind::Tile(_) => "a tile layer",
        LayerKind::Object(_) => "an object layer",
        LayerKind::Image(_) => "an image layer",
        LayerKind::Group(_) => "a group",
    }
}
