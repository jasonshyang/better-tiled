//! Layers: the attributes every kind shares, and the kind's payload.

use glam::DVec2;

use crate::color::Color;
use crate::ids::LayerId;
use crate::image_layer::ImageLayer;
use crate::object_layer::ObjectLayer;
use crate::property::Properties;
use crate::tile_layer::TileLayer;

/// A layer of a map. Groups hold their children, so the layers of a map form a tree; a group
/// draws nothing itself, and its attributes apply to its children ([`EffectiveLayer`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    /// Unique in the map.
    pub id: LayerId,
    /// The name; empty for none.
    pub name: String,
    /// The class; empty for none.
    pub class: String,
    /// Whether the layer is shown.
    pub visible: bool,
    /// Whether the layer is locked against editing.
    pub locked: bool,
    /// Opacity, 0 to 1.
    pub opacity: f64,
    /// A colour the layer is multiplied with.
    pub tint: Option<Color>,
    /// Where the layer is drawn, in screen pixels.
    pub offset: DVec2,
    /// How fast the layer scrolls with the view; 1 is the map's speed.
    pub parallax: DVec2,
    /// How the layer is composited over what is below it.
    pub blend_mode: BlendMode,
    /// Custom properties.
    pub properties: Properties,
    /// What the layer holds.
    pub kind: LayerKind,
}

/// What a layer holds.
#[derive(Clone, Debug, PartialEq)]
pub enum LayerKind {
    /// Cells.
    Tile(TileLayer),
    /// Objects.
    Object(ObjectLayer),
    /// An image.
    Image(ImageLayer),
    /// Other layers, bottom first.
    Group(Vec<Layer>),
}

/// How a layer is composited.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum BlendMode {
    /// Source over.
    #[default]
    Normal,
    /// Added.
    Add,
    /// Multiplied.
    Multiply,
    /// Screen.
    Screen,
    /// Overlay.
    Overlay,
    /// The darker of the two.
    Darken,
    /// The lighter of the two.
    Lighten,
    /// Colour dodge.
    ColorDodge,
    /// Colour burn.
    ColorBurn,
    /// Hard light.
    HardLight,
    /// Soft light.
    SoftLight,
    /// Difference.
    Difference,
    /// Exclusion.
    Exclusion,
}

/// A layer's attributes combined with those of the groups it is in.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct EffectiveLayer {
    /// The layer and every group it is in are visible.
    pub visible: bool,
    /// The layer or a group it is in is locked.
    pub locked: bool,
    /// The product of the opacities.
    pub opacity: f64,
    /// The tints multiplied channel by channel.
    pub tint: Option<Color>,
    /// The sum of the offsets.
    pub offset: DVec2,
    /// The product of the parallax factors.
    pub parallax: DVec2,
}

impl EffectiveLayer {
    /// The attributes of a top-level layer, or of a layer under `self`.
    pub fn combined_with(self, layer: &Layer) -> EffectiveLayer {
        EffectiveLayer {
            visible: self.visible && layer.visible,
            locked: self.locked || layer.locked,
            opacity: self.opacity * layer.opacity,
            tint: match (self.tint, layer.tint) {
                (Some(a), Some(b)) => Some(multiply(a, b)),
                (a, b) => a.or(b),
            },
            offset: self.offset + layer.offset,
            parallax: self.parallax * layer.parallax,
        }
    }

    /// What applies to a top-level layer: nothing.
    pub const ROOT: EffectiveLayer = EffectiveLayer {
        visible: true,
        locked: false,
        opacity: 1.0,
        tint: None,
        offset: DVec2::ZERO,
        parallax: DVec2::ONE,
    };
}

/// Two colours multiplied channel by channel, as Tiled combines tints.
fn multiply(a: Color, b: Color) -> Color {
    let channel = |x: u8, y: u8| {
        let product = f64::from(x) * f64::from(y) / 255.0;
        // The product of two channels over 255 is a channel again.
        u8::try_from(product.round() as u32).unwrap_or(u8::MAX)
    };
    Color::rgba(
        channel(a.r, b.r),
        channel(a.g, b.g),
        channel(a.b, b.b),
        channel(a.a, b.a),
    )
}

impl Layer {
    /// A visible, unlocked, opaque layer at no offset with nothing but `kind`.
    pub fn new(id: LayerId, name: impl Into<String>, kind: LayerKind) -> Layer {
        Layer {
            id,
            name: name.into(),
            class: String::new(),
            visible: true,
            locked: false,
            opacity: 1.0,
            tint: None,
            offset: DVec2::ZERO,
            parallax: DVec2::ONE,
            blend_mode: BlendMode::Normal,
            properties: Properties::new(),
            kind,
        }
    }

    /// The cells, for a tile layer.
    pub fn as_tile_layer(&self) -> Option<&TileLayer> {
        match &self.kind {
            LayerKind::Tile(tiles) => Some(tiles),
            _ => None,
        }
    }

    /// The cells, for a tile layer, to change.
    pub fn as_tile_layer_mut(&mut self) -> Option<&mut TileLayer> {
        match &mut self.kind {
            LayerKind::Tile(tiles) => Some(tiles),
            _ => None,
        }
    }

    /// The objects, for an object layer.
    pub fn as_object_layer(&self) -> Option<&ObjectLayer> {
        match &self.kind {
            LayerKind::Object(objects) => Some(objects),
            _ => None,
        }
    }

    /// The objects, for an object layer, to change.
    pub fn as_object_layer_mut(&mut self) -> Option<&mut ObjectLayer> {
        match &mut self.kind {
            LayerKind::Object(objects) => Some(objects),
            _ => None,
        }
    }

    /// The image, for an image layer.
    pub fn as_image_layer(&self) -> Option<&ImageLayer> {
        match &self.kind {
            LayerKind::Image(image) => Some(image),
            _ => None,
        }
    }

    /// The children of a group; none for other layers.
    pub fn children(&self) -> &[Layer] {
        match &self.kind {
            LayerKind::Group(children) => children,
            _ => &[],
        }
    }

    /// The children of a group, to change; `None` for other layers.
    pub fn children_mut(&mut self) -> Option<&mut Vec<Layer>> {
        match &mut self.kind {
            LayerKind::Group(children) => Some(children),
            _ => None,
        }
    }

    /// What kind of layer it is, in words.
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            LayerKind::Tile(_) => "tile layer",
            LayerKind::Object(_) => "object layer",
            LayerKind::Image(_) => "image layer",
            LayerKind::Group(_) => "group",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_attributes_combine() {
        let group = Layer {
            visible: true,
            locked: true,
            opacity: 0.5,
            tint: Some(Color::rgb(255, 128, 0)),
            offset: DVec2::new(3.0, 4.0),
            parallax: DVec2::new(0.5, 1.0),
            ..Layer::new(LayerId(1), "group", LayerKind::Group(Vec::new()))
        };
        let child = Layer {
            visible: false,
            opacity: 0.5,
            tint: Some(Color::rgb(255, 255, 255)),
            offset: DVec2::new(1.0, 1.0),
            parallax: DVec2::new(0.5, 2.0),
            ..Layer::new(LayerId(2), "child", LayerKind::Group(Vec::new()))
        };
        let effective = EffectiveLayer::ROOT
            .combined_with(&group)
            .combined_with(&child);
        assert!(!effective.visible && effective.locked);
        assert_eq!(effective.opacity, 0.25);
        assert_eq!(effective.tint, Some(Color::rgb(255, 128, 0)));
        assert_eq!(
            (effective.offset, effective.parallax),
            (DVec2::new(4.0, 5.0), DVec2::new(0.25, 2.0))
        );
        assert_eq!(
            multiply(
                Color::rgba(128, 255, 0, 255),
                Color::rgba(128, 128, 255, 128)
            ),
            Color::rgba(64, 128, 0, 128)
        );
    }
}
