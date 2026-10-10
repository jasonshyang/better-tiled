//! Whether a command can run now, and why not; whether a toggle or a tool is on. Pure functions
//! of the editor's state.

use bt_core::{Layer, LayerKind, Map};

use super::table::{ActionId, ActionKind, ActionSpec, Context};
use crate::document::Document;
use crate::editor::Editor;
use crate::settings::Snapping;

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
    /// The current document, its map and current layer.
    pub(crate) fn situation(&self) -> Situation<'_> {
        let document = self
            .documents
            .current()
            .and_then(|doc| self.documents.get(doc));
        let map = document
            .and_then(|d| self.store.map(d.asset()))
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
        if let Some(reason) = context_blocker(id.spec(), &here) {
            return Enabled::Disabled(reason);
        }
        if super::execute::pending(id) {
            return Enabled::Disabled(None);
        }
        let history = here.document.map(Document::history);
        let selection = here.document.map(Document::selection);
        let on_tiles = here
            .layer
            .is_some_and(|l| matches!(l.kind, LayerKind::Tile(_)));
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
            // Objects come to the clipboard with the object commands.
            ActionId::Cut | ActionId::Copy | ActionId::Delete if !(on_tiles && has_tiles) => {
                Enabled::Disabled(None)
            }
            ActionId::Cut | ActionId::Delete => {
                let verb = if id == ActionId::Cut { "cut" } else { "delete" };
                match here
                    .document
                    .and_then(|d| self.tiles_locked_reason(d.id(), verb))
                {
                    Some(reason) => Enabled::Disabled(Some(reason)),
                    None => Enabled::Enabled,
                }
            }
            ActionId::Paste | ActionId::PasteInPlace if !self.has_copied_tiles() => {
                Enabled::Disabled(Some("Nothing was copied".to_owned()))
            }
            ActionId::FlipHorizontal
            | ActionId::FlipVertical
            | ActionId::RotateLeft
            | ActionId::RotateRight
                if !(on_tiles && self.tools.brush().is_some()) =>
            {
                Enabled::Disabled(None)
            }
            ActionId::CropToSelection if !has_tiles => Enabled::Disabled(None),
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
    let holds = match spec.context {
        Context::Always => true,
        Context::Document => here.document.is_some(),
        Context::Map => here.map.is_some(),
        Context::TileLayer => matches!(kind, Some(LayerKind::Tile(_))),
        Context::ObjectLayer => matches!(kind, Some(LayerKind::Object(_))),
        // Tilesets open in tabs of their own with the tileset editor.
        Context::Tileset => false,
    };
    if holds {
        return None;
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
