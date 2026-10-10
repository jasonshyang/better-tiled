//! The Terrain tool (T): paints the chosen terrain on corners, edges or whole tiles as Tiled's
//! Terrain Brush does, each drag one step; the preview is what gets painted, with the cells no
//! tile fits outlined in red. ⌘ paints whole tiles, Shift a line from where the last stroke
//! ended, ⌥ the half turn around the map's centre too (finite maps); a right-click takes the
//! terrain under the pointer.

use bt_core::WangSetType;
use bt_editor::{Intent, TerrainStroke, TileRegion, ToolKind};
use bt_terrain::{BrushTarget, Stroke, hit_test, line_targets};
use egui::{Modifiers, PointerButton};
use glam::DVec2;

use super::painting::{ERASE_COLOR, outline_cells, preview_cells};
use super::tool::{Overlay, Tool, ToolContext};
use crate::canvas::Gesture;

/// The Terrain tool's state.
#[derive(Clone, Debug, Default)]
pub(crate) struct TerrainTool {
    /// Where the pointer is, in the map's screen space less the layer's offset.
    pointer: Option<DVec2>,
    /// The modifiers held.
    modifiers: Modifiers,
    /// The stroke under way.
    stroke: Option<Stroke>,
}

impl TerrainTool {
    /// How the brush paints the chosen terrain here, if it can.
    fn kind(cx: &ToolContext<'_>) -> Option<WangSetType> {
        let in_use = cx.editor.terrain_in_use(cx.doc).ok()?;
        Some(in_use.terrain.brush_kind(in_use.color))
    }

    /// The target under the pointer, with whole tiles when ⌘ is held.
    fn hovered(&self, cx: &ToolContext<'_>) -> Option<BrushTarget> {
        let kind = Self::kind(cx)?;
        hit_test(cx.map, self.pointer?, self.modifiers.command, kind)
    }

    /// What a press now paints: the line from the last stroke with Shift, else the target.
    fn targets_now(&self, cx: &ToolContext<'_>) -> Vec<BrushTarget> {
        let Some(target) = self.hovered(cx) else {
            return Vec::new();
        };
        let line_from = cx
            .editor
            .tools()
            .last_terrain_target()
            .filter(|_| self.modifiers.shift);
        match (line_from, Self::kind(cx)) {
            (Some(from), Some(kind)) => line_targets(from, target, self.modifiers.command, kind),
            _ => vec![target],
        }
    }
}

/// Where a pointer is for the terrain brush: the map's screen space less the current layer's
/// offset, as Tiled's brush measures it.
fn brush_position(world: DVec2, cx: &ToolContext<'_>) -> DVec2 {
    world - cx.layer_offset
}

impl Tool for TerrainTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Terrain
    }

    fn handle(&mut self, gesture: &Gesture, cx: &mut ToolContext<'_>) {
        match gesture {
            Gesture::Hover(at) => self.pointer = Some(brush_position(at.world, cx)),
            Gesture::ModifiersChanged(modifiers) => self.modifiers = *modifiers,
            Gesture::Press(press) if press.button == PointerButton::Primary => {
                self.modifiers = press.modifiers;
                self.pointer = Some(brush_position(press.at.world, cx));
                let Some(kind) = Self::kind(cx) else {
                    // Says why it cannot paint.
                    cx.out
                        .push(Intent::TerrainStroke(TerrainStroke::Begin { doc: cx.doc }));
                    return;
                };
                let mirror = press.modifiers.alt;
                cx.out
                    .push(Intent::TerrainStroke(TerrainStroke::Begin { doc: cx.doc }));
                if press.modifiers.shift && cx.editor.tools().last_terrain_target().is_some() {
                    cx.out.push(Intent::TerrainStroke(TerrainStroke::Paint {
                        targets: self.targets_now(cx),
                        mirror,
                    }));
                    cx.out.push(Intent::TerrainStroke(TerrainStroke::End));
                    return;
                }
                let mut stroke = Stroke::new(cx.map, kind, press.modifiers.command);
                let targets = self.pointer.map(|p| stroke.press(p)).unwrap_or_default();
                cx.out.push(Intent::TerrainStroke(TerrainStroke::Paint {
                    targets,
                    mirror,
                }));
                self.stroke = Some(stroke);
            }
            Gesture::Drag(drag) if drag.button == PointerButton::Primary => {
                let position = brush_position(drag.at.world, cx);
                self.pointer = Some(position);
                if let Some(stroke) = &mut self.stroke {
                    let targets = stroke.drag_to(position);
                    if !targets.is_empty() {
                        cx.out.push(Intent::TerrainStroke(TerrainStroke::Paint {
                            targets,
                            mirror: drag.modifiers.alt,
                        }));
                    }
                }
            }
            Gesture::Release(release)
                if release.button == PointerButton::Primary && self.stroke.is_some() =>
            {
                self.stroke = None;
                cx.out.push(Intent::TerrainStroke(TerrainStroke::End));
            }
            Gesture::Press(press) if press.button == PointerButton::Secondary => {
                self.pointer = Some(brush_position(press.at.world, cx));
                if let Some(target) = self.hovered(cx) {
                    cx.out.push(Intent::CaptureTerrain {
                        doc: cx.doc,
                        target,
                    });
                }
            }
            Gesture::Cancel if self.stroke.is_some() => {
                self.stroke = None;
                cx.out.push(Intent::TerrainStroke(TerrainStroke::Cancel));
            }
            Gesture::Leave => self.pointer = None,
            _ => {}
        }
    }

    fn overlay(&self, cx: &ToolContext<'_>, out: &mut Overlay) {
        if self.stroke.is_some() {
            // The stroke paints as it goes; the map shows it.
            return;
        }
        let targets = self.targets_now(cx);
        if targets.is_empty() {
            return;
        }
        let Some(preview) = cx
            .editor
            .terrain_preview(cx.doc, &targets, self.modifiers.alt)
        else {
            return;
        };
        preview_cells(cx, &preview.cells, out);
        if !preview.invalid.is_empty() {
            let invalid = TileRegion::from_cells(preview.invalid.iter().copied());
            outline_cells(cx, &invalid, ERASE_COLOR, out);
        }
    }

    fn hint(&self, cx: &ToolContext<'_>) -> String {
        let in_use = match cx.editor.terrain_in_use(cx.doc) {
            Ok(in_use) => in_use,
            Err(reason) => return reason.to_string(),
        };
        let command = cx.command_key();
        let mut hint = format!(
            "Click or drag to paint {} · {command}: whole tiles · Shift: a line from the last stroke · ⌥: mirrored across the map · right-click takes the terrain under the pointer",
            in_use.name(in_use.color)
        );
        if self.modifiers.alt && cx.map.infinite {
            hint = "Mirrored painting needs a finite map".to_owned();
        } else if self.stroke.is_none() {
            let targets = self.targets_now(cx);
            if let Some(preview) = cx
                .editor
                .terrain_preview(cx.doc, &targets, self.modifiers.alt)
                .filter(|p| !p.invalid.is_empty())
            {
                let name = |c| in_use.name(c);
                if let Some(text) =
                    bt_terrain::missing_text(&in_use.terrain, &name, &preview.needed)
                {
                    hint = format!("Here, {text}");
                }
            }
        }
        hint
    }

    fn reset(&mut self, out: &mut Vec<Intent>) {
        if self.stroke.take().is_some() {
            out.push(Intent::TerrainStroke(TerrainStroke::End));
        }
        self.pointer = None;
    }
}
