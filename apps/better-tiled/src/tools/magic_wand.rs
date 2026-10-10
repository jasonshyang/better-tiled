//! The Magic Wand (W) and Select Same Tile (S): the cells like the one under the pointer,
//! connected to it (the wand) or anywhere on the layer (same tile), previewed as the pointer
//! moves. Dragging adds the kinds of tile it passes over; the release selects. Modifiers at the
//! press choose how it combines with the selection, as for every selection tool.

use bt_core::Cell;
use bt_editor::{Intent, SelectionMode, TileRegion, ToolKind};
use egui::PointerButton;
use glam::IVec2;

use super::selecting::{mode_at_press, region_preview, selection_hint};
use super::tool::{Overlay, Tool, ToolContext};
use crate::canvas::Gesture;

/// Which of the two tools.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Reach {
    /// Connected cells only.
    Connected,
    /// Every cell of the layer.
    Layer,
}

/// The Magic Wand or Select Same Tile.
#[derive(Clone, Debug)]
pub(crate) struct LikeTiles {
    reach: Reach,
    /// The cell the preview is of.
    at: Option<IVec2>,
    /// A press under way: its mode and the kinds of tile dragged over.
    press: Option<LikePress>,
    /// The cells the preview shows, and what they were worked out from.
    preview: Option<LikePreview>,
}

#[derive(Clone, Debug)]
struct LikePress {
    mode: SelectionMode,
    kinds: Vec<Cell>,
}

#[derive(Clone, Debug)]
struct LikePreview {
    /// The map, layer, cell and kinds it was computed for.
    key: PreviewKey,
    region: TileRegion,
}

#[derive(Clone, Debug, PartialEq)]
struct PreviewKey {
    map: usize,
    layer: Option<bt_core::LayerId>,
    at: IVec2,
    kinds: Vec<Cell>,
}

impl LikeTiles {
    /// The Magic Wand.
    pub(crate) fn magic_wand() -> LikeTiles {
        LikeTiles::new(Reach::Connected)
    }

    /// Select Same Tile.
    pub(crate) fn same_tile() -> LikeTiles {
        LikeTiles::new(Reach::Layer)
    }

    fn new(reach: Reach) -> LikeTiles {
        LikeTiles {
            reach,
            at: None,
            press: None,
            preview: None,
        }
    }

    /// Works out the preview for the pointer at `at`, unless it is up to date.
    fn update_preview(&mut self, cx: &ToolContext<'_>, at: IVec2) {
        self.at = Some(at);
        let kinds = self
            .press
            .as_ref()
            .map(|p| p.kinds.clone())
            .unwrap_or_default();
        let key = PreviewKey {
            map: std::sync::Arc::as_ptr(cx.map) as usize,
            layer: cx.current_layer().map(|l| l.id),
            at,
            kinds: kinds.clone(),
        };
        if self.preview.as_ref().is_some_and(|p| p.key == key) {
            return;
        }
        let region = match self.reach {
            Reach::Connected => cx.editor.wand_area(cx.doc, at, &kinds),
            Reach::Layer => cx.editor.same_tile_area(cx.doc, at, &kinds),
        };
        self.preview = Some(LikePreview { key, region });
    }
}

impl Tool for LikeTiles {
    fn kind(&self) -> ToolKind {
        match self.reach {
            Reach::Connected => ToolKind::MagicWand,
            Reach::Layer => ToolKind::SameTile,
        }
    }

    fn handle(&mut self, gesture: &Gesture, cx: &mut ToolContext<'_>) {
        match gesture {
            Gesture::Hover(at) => self.update_preview(cx, at.cell),
            Gesture::Press(press) if press.button == PointerButton::Primary => {
                let default = cx.editor.tools().selection_mode();
                self.press = Some(LikePress {
                    mode: mode_at_press(press.modifiers, default),
                    kinds: Vec::new(),
                });
                self.update_preview(cx, press.at.cell);
            }
            // Right-click cancels a press under way, else clears the selection.
            Gesture::Press(press)
                if press.button == PointerButton::Secondary && self.press.is_some() =>
            {
                self.press = None;
            }
            Gesture::Press(press) if press.button == PointerButton::Secondary => {
                cx.out.push(Intent::SelectTiles {
                    doc: cx.doc,
                    region: TileRegion::new(),
                    mode: SelectionMode::Replace,
                });
            }
            Gesture::Drag(drag) if drag.button == PointerButton::Primary => {
                // The kind under the pointer joins the kinds selected.
                if let (Some(press), Some(layer)) = (&mut self.press, cx.tile_layer())
                    && let Some(previous) = self.at
                    && previous != drag.at.cell
                {
                    let kind = layer.cell(previous);
                    if !press.kinds.contains(&kind) {
                        press.kinds.push(kind);
                    }
                }
                self.update_preview(cx, drag.at.cell);
            }
            Gesture::Release(release) if release.button == PointerButton::Primary => {
                let Some(press) = self.press.take() else {
                    return;
                };
                let region = self
                    .preview
                    .as_ref()
                    .map(|p| p.region.clone())
                    .unwrap_or_default();
                cx.out.push(Intent::SelectTiles {
                    doc: cx.doc,
                    region,
                    mode: press.mode,
                });
                self.preview = None;
                self.update_preview(cx, release.at.cell);
            }
            Gesture::Cancel => self.press = None,
            Gesture::Leave => {
                self.at = None;
                self.preview = None;
            }
            _ => {}
        }
    }

    fn overlay(&self, cx: &ToolContext<'_>, out: &mut Overlay) {
        if let Some(preview) = &self.preview {
            let color = cx.editor.settings().theme.selection_color.0;
            out.vectors.extend(region_preview(
                &cx.map.geometry(),
                &preview.region,
                color,
                1.0,
            ));
        }
    }

    fn hint(&self, cx: &ToolContext<'_>) -> String {
        let what = match self.reach {
            Reach::Connected => "Click to select the connected tiles like this one",
            Reach::Layer => "Click to select every tile like this one",
        };
        let count = self.preview.as_ref().map_or(0, |p| p.region.len());
        format!(
            "{} · {} · drag to add the kinds you pass over",
            selection_hint(what, cx.command_key()),
            bt_utils::text::plural(count, "tile", "tiles")
        )
    }

    fn reset(&mut self, _out: &mut Vec<Intent>) {
        self.press = None;
        self.preview = None;
        self.at = None;
    }
}
