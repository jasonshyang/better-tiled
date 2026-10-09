//! Objects as files state them.

use super::property::RawProperty;
use super::scalar::Scalar;

/// An `<object>`: of an object layer, a tile's collision shape, or a template's.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawObject {
    /// The id.
    pub(crate) id: Option<Scalar>,
    /// The name.
    pub(crate) name: Option<Scalar>,
    /// The class.
    pub(crate) class: Option<Scalar>,
    /// The class, spelled `type`.
    pub(crate) class_as_type: Option<Scalar>,
    /// The tile, as a global id.
    pub(crate) gid: Option<Scalar>,
    /// The x position.
    pub(crate) x: Option<Scalar>,
    /// The y position.
    pub(crate) y: Option<Scalar>,
    /// The width.
    pub(crate) width: Option<Scalar>,
    /// The height.
    pub(crate) height: Option<Scalar>,
    /// The rotation in degrees.
    pub(crate) rotation: Option<Scalar>,
    /// The opacity.
    pub(crate) opacity: Option<Scalar>,
    /// The visibility.
    pub(crate) visible: Option<Scalar>,
    /// The template file of a template instance.
    pub(crate) template: Option<Scalar>,
    /// Custom properties.
    pub(crate) properties: Vec<RawProperty>,
    /// The shape, when not a rectangle.
    pub(crate) shape: Option<RawShape>,
}

/// An object's shape element.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RawShape {
    /// `<ellipse/>`.
    Ellipse,
    /// `<point/>`.
    Point,
    /// `<capsule/>`.
    Capsule,
    /// `<polygon points>`.
    Polygon(Vec<RawPoint>),
    /// `<polyline points>`.
    Polyline(Vec<RawPoint>),
    /// `<text>`.
    Text(Box<RawText>),
}

/// A point, or a tileset's tile offset.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawPoint {
    /// x.
    pub(crate) x: Option<Scalar>,
    /// y.
    pub(crate) y: Option<Scalar>,
}

/// A text object's `<text>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawText {
    /// The font family.
    pub(crate) font_family: Option<Scalar>,
    /// The font size in pixels.
    pub(crate) pixel_size: Option<Scalar>,
    /// Word wrap.
    pub(crate) wrap: Option<Scalar>,
    /// The colour.
    pub(crate) color: Option<Scalar>,
    /// Bold.
    pub(crate) bold: Option<Scalar>,
    /// Italic.
    pub(crate) italic: Option<Scalar>,
    /// Underline.
    pub(crate) underline: Option<Scalar>,
    /// Strike-out.
    pub(crate) strikeout: Option<Scalar>,
    /// Kerning.
    pub(crate) kerning: Option<Scalar>,
    /// Horizontal alignment.
    pub(crate) halign: Option<Scalar>,
    /// Vertical alignment.
    pub(crate) valign: Option<Scalar>,
    /// The text.
    pub(crate) text: String,
}
