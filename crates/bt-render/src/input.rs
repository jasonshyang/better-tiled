//! What a frame draws: a map through a camera, with what the editor adds on top.

use std::collections::BTreeMap;
use std::sync::Arc;

use bt_core::{Cell, ClassUsage, Color, CustomTypes, LayerId, Map};
use bt_utils::DRect;
use glam::{DVec2, IVec2, UVec2};

use crate::vector::VectorItem;

/// Where a view looks: the world pixel at its top-left corner and its scale. World pixels are
/// Tiled's screen coordinates (the map's projected space).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Camera {
    /// The world point at the viewport's top-left corner.
    pub origin: DVec2,
    /// Device pixels per world pixel.
    pub zoom: f64,
}

impl Camera {
    /// The device pixel (from the viewport's top-left) a world point is drawn at.
    pub fn world_to_device(self, world: DVec2) -> DVec2 {
        (world - self.origin) * self.zoom
    }

    /// The world point a device pixel shows.
    pub fn device_to_world(self, device: DVec2) -> DVec2 {
        self.origin + device / self.zoom.max(f64::MIN_POSITIVE)
    }

    /// The world rectangle a viewport of `viewport_px` device pixels shows.
    pub fn visible_rect(self, viewport_px: UVec2) -> DRect {
        DRect::from_min_size(
            self.origin,
            viewport_px.as_dvec2() / self.zoom.max(f64::MIN_POSITIVE),
        )
    }

    /// The camera with its origin snapped to whole device pixels, so a texel edge lands where
    /// the overlays measure it.
    pub fn snapped(self) -> Camera {
        Camera {
            origin: (self.origin * self.zoom).round() / self.zoom.max(f64::MIN_POSITIVE),
            zoom: self.zoom,
        }
    }
}

/// A tile drawn above a layer as a preview (a brush under the pointer, a fill to come).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PreviewTile {
    /// The cell it is drawn at.
    pub at: IVec2,
    /// What it shows.
    pub cell: Cell,
    /// Its opacity, 0 to 1.
    pub opacity: f32,
}

/// The editor's view switches over the layers of one view.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerVisibility {
    highlighted: Vec<LayerId>,
    fade: Color,
    hidden: Vec<LayerId>,
}

impl Default for LayerVisibility {
    fn default() -> LayerVisibility {
        LayerVisibility {
            highlighted: Vec::new(),
            fade: Color::BLACK,
            hidden: Vec::new(),
        }
    }
}

impl LayerVisibility {
    /// Tiled's Highlight Current Layer: what is below the lowest of `layers` is faded toward
    /// `fade`, and the other layers above it are drawn at 40 % of their opacity.
    pub fn highlight(mut self, layers: Vec<LayerId>, fade: Color) -> LayerVisibility {
        self.highlighted = layers;
        self.fade = fade;
        self
    }

    /// Layers hidden in this view only, whatever their own visibility.
    pub fn hide(mut self, layers: Vec<LayerId>) -> LayerVisibility {
        self.hidden = layers;
        self
    }

    pub(crate) fn highlighted(&self) -> &[LayerId] {
        &self.highlighted
    }

    pub(crate) fn fade(&self) -> Color {
        self.fade
    }

    pub(crate) fn is_hidden(&self, layer: LayerId) -> bool {
        self.hidden.contains(&layer)
    }
}

/// How objects that are not tiles are drawn: their outline width and their colours.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectStyle {
    line_width: f32,
    class_colors: Arc<BTreeMap<String, Color>>,
    shapes: bool,
}

impl Default for ObjectStyle {
    fn default() -> ObjectStyle {
        ObjectStyle {
            line_width: 2.0,
            class_colors: Arc::new(BTreeMap::new()),
            shapes: true,
        }
    }
}

impl ObjectStyle {
    /// The width of shape outlines in device pixels (Tiled's Object Line Width; 2 by default).
    pub fn line_width(mut self, width: f32) -> ObjectStyle {
        self.line_width = width.max(0.0);
        self
    }

    /// The colour of each class that has one (from the project's custom types); an object of
    /// another class takes its layer's colour.
    pub fn class_colors(mut self, colors: Arc<BTreeMap<String, Color>>) -> ObjectStyle {
        self.class_colors = colors;
        self
    }

    /// Uses the colours of project classes that apply to objects.
    pub fn custom_types(mut self, types: &CustomTypes) -> ObjectStyle {
        self.class_colors = Arc::new(
            types
                .types
                .iter()
                .filter_map(|ty| {
                    let class = ty.as_class()?;
                    class
                        .use_as
                        .intersects(ClassUsage::OBJECT)
                        .then(|| (ty.name.clone(), class.color))
                })
                .collect(),
        );
        self
    }

    /// Whether rectangles, ellipses, points, polygons and polylines are drawn (tile and text
    /// objects always are).
    pub fn shapes(mut self, shown: bool) -> ObjectStyle {
        self.shapes = shown;
        self
    }

    pub(crate) fn width(&self) -> f32 {
        self.line_width
    }

    pub(crate) fn class_color(&self, class: &str) -> Option<Color> {
        self.class_colors.get(class).copied()
    }

    pub(crate) fn draws_shapes(&self) -> bool {
        self.shapes
    }
}

/// One view of a frame: a map, a camera, a viewport and the editor's additions.
#[derive(Clone, Debug)]
pub struct FrameInput {
    pub(crate) map: Arc<Map>,
    pub(crate) camera: Camera,
    pub(crate) viewport_px: UVec2,
    pub(crate) time_ms: u64,
    pub(crate) layers: LayerVisibility,
    pub(crate) previews: Arc<[PreviewTile]>,
    pub(crate) preview_layer: Option<LayerId>,
    pub(crate) vectors: Arc<[VectorItem]>,
    pub(crate) objects: ObjectStyle,
    pub(crate) parallax: bool,
    pub(crate) background: Option<Color>,
    pub(crate) background_bounds: Option<DRect>,
}

impl FrameInput {
    /// A view of `map` through `camera` into a viewport of `viewport_px` device pixels, with
    /// nothing added, no animation time, no parallax and no background.
    pub fn new(map: Arc<Map>, camera: Camera, viewport_px: UVec2) -> FrameInput {
        FrameInput {
            map,
            camera,
            viewport_px,
            time_ms: 0,
            layers: LayerVisibility::default(),
            previews: Arc::from(Vec::new()),
            preview_layer: None,
            vectors: Arc::from(Vec::new()),
            objects: ObjectStyle::default(),
            parallax: false,
            background: None,
            background_bounds: None,
        }
    }

    /// The time animations are shown at.
    pub fn time_ms(mut self, time_ms: u64) -> FrameInput {
        self.time_ms = time_ms;
        self
    }

    /// The view switches over the layers.
    pub fn layers(mut self, layers: LayerVisibility) -> FrameInput {
        self.layers = layers;
        self
    }

    /// Tiles drawn above `layer` as a preview.
    pub fn previews(mut self, layer: LayerId, tiles: Arc<[PreviewTile]>) -> FrameInput {
        self.preview_layer = Some(layer);
        self.previews = tiles;
        self
    }

    /// Outlines, marks, text and the grid ([`VectorItem`]).
    pub fn vectors(mut self, items: Arc<[VectorItem]>) -> FrameInput {
        self.vectors = items;
        self
    }

    /// How shape objects are drawn.
    pub fn objects(mut self, style: ObjectStyle) -> FrameInput {
        self.objects = style;
        self
    }

    /// Whether parallax factors move layers with the view.
    pub fn parallax(mut self, on: bool) -> FrameInput {
        self.parallax = on;
        self
    }

    /// The colour under the layers (blend modes blend with it); none leaves the target as it is.
    pub fn background(mut self, color: Option<Color>) -> FrameInput {
        self.background = color;
        self
    }

    /// Limits the background to a world-space rectangle, for several maps in one viewport.
    pub fn background_bounds(mut self, bounds: DRect) -> FrameInput {
        self.background_bounds = Some(bounds);
        self
    }

    /// The map.
    pub fn map(&self) -> &Arc<Map> {
        &self.map
    }

    /// The camera.
    pub fn camera(&self) -> Camera {
        self.camera
    }

    /// The viewport's size in device pixels.
    pub fn viewport_px(&self) -> UVec2 {
        self.viewport_px
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cameras_convert_both_ways_and_snap_to_device_pixels() {
        let camera = Camera {
            origin: DVec2::new(10.3, -4.0),
            zoom: 2.0,
        };
        let world = DVec2::new(15.0, 6.0);
        let device = camera.world_to_device(world);
        assert!((camera.device_to_world(device) - world).length() < 1e-9);
        let snapped = camera.snapped();
        assert_eq!(
            snapped.origin * snapped.zoom,
            (snapped.origin * snapped.zoom).round()
        );
        assert_eq!(
            camera.visible_rect(UVec2::new(100, 50)).size(),
            DVec2::new(50.0, 25.0)
        );
    }
}
