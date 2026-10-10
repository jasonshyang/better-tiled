//! The value of a built-in attribute.

use std::fmt;

use bt_core::{
    Alignment, BlendMode, Color, DrawOrder, FileRef, FillMode, GridOrientation, HAlign, ImageRef,
    ImageSource, LayerDataFormat, Orientation, RenderOrder, StaggerAxis, StaggerIndex,
    TileRenderSize, VAlign,
};

/// The value of a built-in attribute: the kind each attribute takes is fixed (an opacity is a
/// number, an orientation a choice of orientations).
#[derive(Clone, Debug, PartialEq)]
pub enum AttributeValue {
    /// Text: a name, a class, a text object's text or font family.
    Text(String),
    /// A whole number: an id, a size in tiles or pixels, an offset in pixels.
    Integer(i64),
    /// A real number: an opacity, a position, a rotation, a parallax factor, a probability.
    Number(f64),
    /// Yes or no.
    Bool(bool),
    /// A colour; `None` for no colour where the attribute can have none.
    Color(Option<Color>),
    /// An image (its file or bytes and the size the window read for it), or none. The
    /// transparent colour it carries is the separate Transparent Color attribute's.
    Image(Option<ImageRef>),
    /// A file: a template instance's template, or none.
    File(Option<FileRef>),
    /// A map's orientation.
    Orientation(Orientation),
    /// A map's stagger axis.
    StaggerAxis(StaggerAxis),
    /// A map's stagger index.
    StaggerIndex(StaggerIndex),
    /// A map's layer data format.
    LayerDataFormat(LayerDataFormat),
    /// A map's tile render order.
    RenderOrder(RenderOrder),
    /// A layer's blend mode.
    BlendMode(BlendMode),
    /// An object layer's draw order.
    DrawOrder(DrawOrder),
    /// A tileset's object alignment.
    Alignment(Alignment),
    /// A tileset's tile render size.
    TileRenderSize(TileRenderSize),
    /// A tileset's fill mode.
    FillMode(FillMode),
    /// A tileset's grid orientation.
    GridOrientation(GridOrientation),
    /// A terrain set's type.
    TerrainType(bt_core::WangSetType),
    /// A text object's horizontal alignment.
    HAlign(HAlign),
    /// A text object's vertical alignment.
    VAlign(VAlign),
}

impl fmt::Display for AttributeValue {
    /// The value in words: text in quotes, numbers as numbers, colours in Tiled's text form,
    /// files and images by the path written, choices by the names Tiled's combo boxes give
    /// them.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            AttributeValue::Text(text) => return write!(f, "“{text}”"),
            AttributeValue::Integer(number) => return write!(f, "{number}"),
            AttributeValue::Number(number) => return write!(f, "{number}"),
            AttributeValue::Bool(value) => return write!(f, "{value}"),
            AttributeValue::Color(Some(color)) => return f.write_str(&color.to_tiled_text()),
            AttributeValue::Color(None) => "no colour",
            AttributeValue::Image(Some(image)) => match &image.source {
                ImageSource::File(file) => return write!(f, "“{}”", file.source),
                ImageSource::Embedded(_) => "an embedded image",
            },
            AttributeValue::Image(None) => "no image",
            AttributeValue::File(Some(file)) => return write!(f, "“{}”", file.source),
            AttributeValue::File(None) => "no file",
            AttributeValue::Orientation(orientation) => match orientation {
                Orientation::Orthogonal => "Orthogonal",
                Orientation::Isometric => "Isometric",
                Orientation::Staggered => "Isometric (Staggered)",
                Orientation::Hexagonal => "Hexagonal (Staggered)",
                Orientation::Oblique => "Oblique",
            },
            AttributeValue::StaggerAxis(axis) => match axis {
                StaggerAxis::X => "X",
                StaggerAxis::Y => "Y",
            },
            AttributeValue::StaggerIndex(index) => match index {
                StaggerIndex::Odd => "Odd",
                StaggerIndex::Even => "Even",
            },
            AttributeValue::LayerDataFormat(format) => match format {
                LayerDataFormat::Xml => "XML (deprecated)",
                LayerDataFormat::Csv => "CSV",
                LayerDataFormat::Base64 => "Base64 (uncompressed)",
                LayerDataFormat::Base64Gzip => "Base64 (gzip compressed)",
                LayerDataFormat::Base64Zlib => "Base64 (zlib compressed)",
                LayerDataFormat::Base64Zstd => "Base64 (Zstandard compressed)",
            },
            AttributeValue::RenderOrder(order) => match order {
                RenderOrder::RightDown => "Right Down",
                RenderOrder::RightUp => "Right Up",
                RenderOrder::LeftDown => "Left Down",
                RenderOrder::LeftUp => "Left Up",
            },
            AttributeValue::BlendMode(mode) => match mode {
                BlendMode::Normal => "Normal",
                BlendMode::Add => "Add",
                BlendMode::Multiply => "Multiply",
                BlendMode::Screen => "Screen",
                BlendMode::Overlay => "Overlay",
                BlendMode::Darken => "Darken",
                BlendMode::Lighten => "Lighten",
                BlendMode::ColorDodge => "Color Dodge",
                BlendMode::ColorBurn => "Color Burn",
                BlendMode::HardLight => "Hard Light",
                BlendMode::SoftLight => "Soft Light",
                BlendMode::Difference => "Difference",
                BlendMode::Exclusion => "Exclusion",
            },
            AttributeValue::DrawOrder(order) => match order {
                DrawOrder::TopDown => "Top Down",
                DrawOrder::Index => "Manual",
            },
            AttributeValue::Alignment(alignment) => match alignment {
                Alignment::Unspecified => "Unspecified",
                Alignment::TopLeft => "Top Left",
                Alignment::Top => "Top",
                Alignment::TopRight => "Top Right",
                Alignment::Left => "Left",
                Alignment::Center => "Center",
                Alignment::Right => "Right",
                Alignment::BottomLeft => "Bottom Left",
                Alignment::Bottom => "Bottom",
                Alignment::BottomRight => "Bottom Right",
            },
            AttributeValue::TileRenderSize(size) => match size {
                TileRenderSize::Tile => "Tile Size",
                TileRenderSize::Grid => "Map Grid Size",
            },
            AttributeValue::FillMode(mode) => match mode {
                FillMode::Stretch => "Stretch",
                FillMode::PreserveAspectFit => "Preserve Aspect Ratio",
            },
            AttributeValue::GridOrientation(orientation) => match orientation {
                GridOrientation::Orthogonal => "Orthogonal",
                GridOrientation::Isometric => "Isometric",
            },
            AttributeValue::TerrainType(kind) => match kind {
                bt_core::WangSetType::Corner => "Corner",
                bt_core::WangSetType::Edge => "Edge",
                bt_core::WangSetType::Mixed => "Mixed",
            },
            AttributeValue::HAlign(align) => match align {
                HAlign::Left => "Left",
                HAlign::Center => "Center",
                HAlign::Right => "Right",
                HAlign::Justify => "Justify",
            },
            AttributeValue::VAlign(align) => match align {
                VAlign::Top => "Top",
                VAlign::Center => "Center",
                VAlign::Bottom => "Bottom",
            },
        };
        f.write_str(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_read_as_words() {
        assert_eq!(AttributeValue::Text("a".into()).to_string(), "“a”");
        assert_eq!(AttributeValue::Number(0.5).to_string(), "0.5");
        assert_eq!(AttributeValue::Color(None).to_string(), "no colour");
        assert_eq!(
            AttributeValue::Color(Some(Color::rgb(255, 0, 0))).to_string(),
            "#ff0000"
        );
        assert_eq!(
            AttributeValue::DrawOrder(DrawOrder::Index).to_string(),
            "Manual"
        );
    }
}
