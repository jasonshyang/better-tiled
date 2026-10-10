//! The canvas: a document's view of its map. The camera pans and zooms; the renderer draws the
//! map inside egui's render pass; the overlays (grid, map border, hovered cell) are vector
//! items the renderer draws with it.

mod camera;
mod gesture;
mod overlay;
mod paint;
mod view;

pub(crate) use camera::{MAX_ZOOM, MIN_ZOOM, ViewTransform};
pub(crate) use gesture::{Gesture, Pointer, PointerDrag, PointerPress};
pub(crate) use paint::CanvasGpu;
pub(crate) use view::{Canvas, CanvasFrame, CanvasRole, Hover};
