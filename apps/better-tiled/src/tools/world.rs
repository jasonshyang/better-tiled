//! World movement and resize previews. A release records one map-history transaction; Escape
//! drops the preview. Resize follows Tiled's WorldMoveMapTool projected-bounds calculation.

use super::tool::{Overlay, Tool, ToolContext};
use crate::canvas::Gesture;
use bt_core::{Color, WorldMap};
use bt_editor::{Intent, ResizeHandle, ToolKind};
use bt_render::{TextItem, VectorItem};
use bt_utils::{DRect, IRect};
use egui::PointerButton;
use glam::{DVec2, IVec2, UVec2};

#[derive(Default)]
pub(crate) struct WorldTool {
    drag: Option<Drag>,
}

struct Drag {
    entry: WorldMap,
    start: DVec2,
    handle: Option<ResizeHandle>,
    preview: IRect,
    size: UVec2,
    offset: IVec2,
}

fn rect(entry: &WorldMap, origin: DVec2) -> DRect {
    DRect::from_min_size(
        entry.rect.min().as_dvec2() - origin,
        entry.rect.size().as_dvec2(),
    )
}
fn quad(area: DRect) -> [DVec2; 4] {
    [
        area.min,
        DVec2::new(area.max.x, area.min.y),
        area.max,
        DVec2::new(area.min.x, area.max.y),
    ]
}
fn step(cx: &ToolContext<'_>, entry: &WorldMap) -> DVec2 {
    if let Some(grid) = cx
        .editor
        .world_of(cx.doc)
        .and_then(|id| cx.editor.store().world(id))
        .map(|world| world.grid_size)
        && grid.x > 0
        && grid.y > 0
    {
        return grid.as_dvec2();
    }
    let map = cx
        .editor
        .store()
        .by_path(&entry.file.path)
        .and_then(|id| cx.editor.store().map(id))
        .unwrap_or(cx.map);
    DVec2::new(
        f64::from(map.tile_width.max(1)),
        f64::from(map.tile_height.max(1)),
    )
}
fn current_entry(cx: &ToolContext<'_>) -> Option<WorldMap> {
    let path = cx.editor.path_of(cx.doc)?;
    cx.editor
        .world_maps(cx.editor.world_of(cx.doc)?)
        .into_iter()
        .find(|entry| entry.file.path == path)
}

impl WorldTool {
    fn update(&mut self, cx: &ToolContext<'_>, at: DVec2, free: bool) {
        let Some(drag) = &mut self.drag else { return };
        let mut delta = at - drag.start;
        let snap = step(cx, &drag.entry);
        if let Some(handle) = drag.handle {
            if !free {
                delta = (delta / snap).round() * snap;
            }
            let map = cx.map;
            let geometry = map.geometry();
            let size = IVec2::new(map.width as i32, map.height as i32);
            let bounds = geometry.map_bounding_rect();
            let column = (geometry
                .bounding_rect(IRect::from_min_size(IVec2::ZERO, size + IVec2::X))
                .w
                - bounds.w)
                .max(1);
            let row = (geometry
                .bounding_rect(IRect::from_min_size(IVec2::ZERO, size + IVec2::Y))
                .h
                - bounds.h)
                .max(1);
            let direction = handle.on(quad(DRect::from_min_size(-DVec2::ONE, DVec2::splat(2.0))));
            let width = (f64::from(size.x) + (delta.x * direction.x / f64::from(column)).round())
                .max(1.0) as i32;
            let height = (f64::from(size.y) + (delta.y * direction.y / f64::from(row)).round())
                .max(1.0) as i32;
            drag.size = UVec2::new(width as u32, height as u32);
            drag.offset = IVec2::new(
                if direction.x < 0.0 { width - size.x } else { 0 },
                if direction.y < 0.0 {
                    height - size.y
                } else {
                    0
                },
            );
            let resized =
                geometry.bounding_rect(IRect::from_min_size(-drag.offset, drag.size.as_ivec2()));
            drag.preview = IRect::from_min_size(
                drag.entry.rect.min() + resized.min() - bounds.min(),
                resized.size(),
            );
        } else {
            let position = drag.entry.rect.min().as_dvec2() + delta;
            let position = if free {
                position
            } else {
                (position / snap).round() * snap
            };
            drag.preview =
                IRect::from_min_size(position.round().as_ivec2(), drag.entry.rect.size());
        }
    }
}

impl Tool for WorldTool {
    fn kind(&self) -> ToolKind {
        ToolKind::World
    }
    fn handle(&mut self, gesture: &Gesture, cx: &mut ToolContext<'_>) {
        let Some(world) = cx.editor.world_of(cx.doc) else {
            return;
        };
        let Some(origin) = cx.editor.world_origin(cx.doc) else {
            return;
        };
        match gesture {
            Gesture::Press(press) if press.button == PointerButton::Primary => {
                let current = current_entry(cx);
                let handle = current
                    .as_ref()
                    .filter(|_| !cx.map.infinite)
                    .and_then(|entry| {
                        ResizeHandle::ALL.into_iter().find(|handle| {
                            handle
                                .on(quad(rect(entry, origin)))
                                .distance(press.at.world)
                                <= cx.world_length(7.0)
                        })
                    });
                let entry = if handle.is_some() {
                    current
                } else {
                    cx.editor
                        .world_maps(world)
                        .into_iter()
                        .rev()
                        .find(|entry| rect(entry, origin).contains(press.at.world))
                };
                self.drag = entry.map(|entry| Drag {
                    preview: entry.rect,
                    entry,
                    start: press.at.world,
                    handle,
                    size: UVec2::new(cx.map.width, cx.map.height),
                    offset: IVec2::ZERO,
                });
            }
            Gesture::Drag(drag) if drag.button == PointerButton::Primary => {
                self.update(cx, drag.at.world, drag.modifiers.command)
            }
            Gesture::Release(release) if release.button == PointerButton::Primary => {
                self.update(cx, release.at.world, release.modifiers.command);
                let Some(drag) = self.drag.take() else { return };
                if !release.moved {
                    if Some(&drag.entry.file.path) != cx.editor.path_of(cx.doc).as_ref() {
                        cx.out.push(Intent::Open(drag.entry.file.path));
                    }
                } else if drag.handle.is_some() {
                    cx.out.push(Intent::ResizeMap {
                        doc: cx.doc,
                        size: drag.size,
                        offset: drag.offset,
                        remove_objects_outside: false,
                    });
                } else {
                    cx.out.push(Intent::MoveWorldMap {
                        doc: cx.doc,
                        world,
                        path: drag.entry.file.path,
                        position: drag.preview.min(),
                    });
                }
            }
            Gesture::Cancel => self.drag = None,
            _ => {}
        }
    }
    fn overlay(&self, cx: &ToolContext<'_>, out: &mut Overlay) {
        let Some(origin) = cx.editor.world_origin(cx.doc) else {
            return;
        };
        if let Some(entry) = current_entry(cx).filter(|_| !cx.map.infinite) {
            for handle in ResizeHandle::ALL {
                let at = handle.on(quad(rect(&entry, origin)));
                out.vectors.push(
                    VectorItem::rect(
                        DRect::from_min_size(at, DVec2::ZERO)
                            .expanded(DVec2::splat(cx.world_length(3.5))),
                    )
                    .fill(Color::WHITE)
                    .stroke(Color::rgba(80, 185, 240, 255), 1.0),
                );
            }
        }
        if let Some(drag) = &self.drag {
            let area = DRect::from_min_size(
                drag.preview.min().as_dvec2() - origin,
                drag.preview.size().as_dvec2(),
            );
            let color = Color::rgba(80, 185, 240, 255);
            out.vectors.push(
                VectorItem::rect(area)
                    .fill(Color::rgba(80, 185, 240, 25))
                    .stroke(color, 2.0),
            );
            out.vectors.push(VectorItem::text(
                TextItem::new(
                    format!(
                        "{}, {} · {} × {}",
                        drag.preview.x, drag.preview.y, drag.size.x, drag.size.y
                    ),
                    area.min,
                )
                .screen_size(13.0)
                .color(color),
            ));
        }
    }
    fn hint(&self, cx: &ToolContext<'_>) -> String {
        format!(
            "Click a map to open it · drag to move · drag this map's handles to resize · {} moves freely · Esc cancels",
            cx.command_key()
        )
    }
    fn reset(&mut self, _out: &mut Vec<Intent>) {
        self.drag = None;
    }
}
