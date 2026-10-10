//! Where Select Objects' handles sit (Tiled's `ObjectSelectionTool::updateHandles`): the quad
//! the selected objects resize by, on screen, and the space a drag of them is measured in. A
//! map's objects are measured in the pixels of the layer holding the first of them (its offset
//! taken off), a tile's collision shapes in the tile's own.

use bt_core::MapGeometry;
use bt_edit::{resize_quad, resize_quad_in};
use glam::DVec2;

use super::host::ObjectHost;
use crate::document::DocumentId;
use crate::editor::Editor;

/// Where the handles of the selected objects sit, in the map's screen space.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct HandleFrame {
    /// The corners of the quad the resize handles sit on: top-left, top-right, bottom-right,
    /// bottom-left.
    pub corners: [DVec2; 4],
    /// Whether the selection resizes: anything but a lone point, as Tiled shows no handles
    /// for one.
    pub resizable: bool,
}

impl HandleFrame {
    /// The middle of the quad, which rotate handles turn the objects around.
    pub fn centre(&self) -> DVec2 {
        self.corners.iter().copied().sum::<DVec2>() / 4.0
    }
}

/// The grid a drag of the selected objects is measured on, and the offset of their layer.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct SelectionSpace {
    pub(crate) geometry: MapGeometry,
    pub(crate) offset: DVec2,
}

impl SelectionSpace {
    /// A point of the map's screen space in the objects' pixels.
    pub(crate) fn to_pixel(self, world: DVec2) -> DVec2 {
        self.geometry.screen_to_pixel(world - self.offset)
    }

    /// A point of the objects' pixels in the map's screen space.
    pub(crate) fn to_world(self, pixel: DVec2) -> DVec2 {
        self.geometry.pixel_to_screen(pixel) + self.offset
    }
}

impl Editor {
    /// Where Select Objects' handles sit for the selected objects of `doc`; `None` without a
    /// selection.
    pub fn selection_handles(&self, doc: DocumentId) -> Option<HandleFrame> {
        let ids = self.documents.get(doc)?.selection().objects.clone();
        let space = self.selection_space(doc)?;
        let quad = match self.object_host(doc)? {
            ObjectHost::Tile { home, tile } => {
                let shapes = self.tile_shapes(home, tile)?;
                resize_quad_in(&shapes.shapes, &shapes.space(), &ids).ok()??
            }
            ObjectHost::Map(_) => resize_quad(self.map_of(doc)?, &ids).ok()??,
        };
        let lone_point = ids.len() == 1 && quad.iter().all(|corner| *corner == quad[0]);
        Some(HandleFrame {
            corners: quad.map(|corner| space.to_world(corner)),
            resizable: !lone_point,
        })
    }

    /// The space a drag of `doc`'s selected objects is measured in.
    pub(crate) fn selection_space(&self, doc: DocumentId) -> Option<SelectionSpace> {
        match self.object_host(doc)? {
            ObjectHost::Tile { home, tile } => Some(SelectionSpace {
                geometry: *self.tile_shapes(home, tile)?.space().geometry(),
                offset: DVec2::ZERO,
            }),
            ObjectHost::Map(_) => {
                let map = self.map_of(doc)?;
                let first = self
                    .documents
                    .get(doc)?
                    .selection()
                    .objects
                    .first()
                    .copied();
                let offset = first
                    .and_then(|id| map.object_layer_of(id))
                    .and_then(|layer| map.effective_layer(layer))
                    .map_or(DVec2::ZERO, |state| state.offset);
                Some(SelectionSpace {
                    geometry: map.geometry(),
                    offset,
                })
            }
        }
    }
}
