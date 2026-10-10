//! The built-in attributes the Properties panel shows above the custom properties, named as
//! Tiled's Properties panel names them (`reference/tiled/src/tiled/propertieswidget.cpp`).
//! Tiled's pairs (a size, an offset, a position) are two attributes here, one per component,
//! so that each can differ among several targets and be set alone, as Tiled sets only the
//! component that changed.

use std::fmt;

use bt_core::{
    Alignment, BlendMode, DrawOrder, FillMode, GridOrientation, HAlign, LayerDataFormat,
    ObjectOverrides, Orientation, RenderOrder, StaggerAxis, StaggerIndex, TileRenderSize, VAlign,
};

use super::attribute_value::AttributeValue;

/// A built-in attribute of a map, a layer, an object, a tileset or a tile.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Attribute {
    /// The id of a layer, an object or a tile; read-only.
    Id,
    /// The template of a template instance; read-only.
    Template,
    /// The name of a layer, an object or a tileset.
    Name,
    /// The class.
    Class,
    /// The map's grid shape.
    Orientation,
    /// The map's width in tiles; read-only (Resize Map changes it).
    MapWidth,
    /// The map's height in tiles; read-only.
    MapHeight,
    /// Whether the map's tile layers have no bounds.
    Infinite,
    /// The width of the map's grid cell, or (read-only) of an atlas's tiles.
    TileWidth,
    /// The height of the map's grid cell, or (read-only) of an atlas's tiles.
    TileHeight,
    /// The length of a hexagon's straight side, on hexagonal maps.
    HexSideLength,
    /// Which axis is staggered, on staggered and hexagonal maps.
    StaggerAxis,
    /// Which rows or columns are shifted, on staggered and hexagonal maps.
    StaggerIndex,
    /// How far a row shifts right, on oblique maps.
    SkewX,
    /// How far a column shifts down, on oblique maps.
    SkewY,
    /// The point parallax scrolls around, across.
    ParallaxOriginX,
    /// The point parallax scrolls around, down.
    ParallaxOriginY,
    /// How the map's file writes tile layer data.
    LayerDataFormat,
    /// How hard compressed layer data is compressed; -1 for the format's default.
    CompressionLevel,
    /// The width of the chunks an infinite map's file is written in.
    ChunkWidth,
    /// The height of the chunks an infinite map's file is written in.
    ChunkHeight,
    /// The order tiles are drawn in, on orthogonal and oblique maps.
    RenderOrder,
    /// The colour behind a map or behind a tileset's tiles.
    BackgroundColor,
    /// Whether a layer or an object is shown.
    Visible,
    /// Whether a layer is locked against editing.
    Locked,
    /// The opacity of a layer or an object, 0 to 1.
    Opacity,
    /// The colour a layer is multiplied with.
    TintColor,
    /// How a layer is composited.
    BlendMode,
    /// Where a layer is drawn, across.
    OffsetX,
    /// Where a layer is drawn, down.
    OffsetY,
    /// How fast a layer scrolls across with the view.
    ParallaxX,
    /// How fast a layer scrolls down with the view.
    ParallaxY,
    /// The colour of an object layer's objects.
    Color,
    /// The order an object layer draws its objects in.
    DrawOrder,
    /// The image of an image layer, of a collection's tile, or (read-only) of an atlas.
    Image,
    /// The colour drawn as transparent in an image layer's image, or (read-only) an atlas's.
    TransparentColor,
    /// Whether an image layer's image repeats across.
    RepeatX,
    /// Whether an image layer's image repeats down.
    RepeatY,
    /// Where an object is, across.
    X,
    /// Where an object is, down.
    Y,
    /// An object's width.
    Width,
    /// An object's height.
    Height,
    /// An object's rotation in degrees, clockwise.
    Rotation,
    /// Whether a tile object's tile is mirrored left-right.
    FlipHorizontally,
    /// Whether a tile object's tile is mirrored top-bottom.
    FlipVertically,
    /// A text object's text.
    Text,
    /// A text object's font family.
    FontFamily,
    /// A text object's font size in pixels.
    PixelSize,
    /// Whether a text object's lines wrap.
    WordWrap,
    /// A text object's colour.
    TextColor,
    /// Whether a text object's text is bold.
    Bold,
    /// Whether a text object's text is italic.
    Italic,
    /// Whether a text object's text is underlined.
    Underline,
    /// Whether a text object's text is struck out.
    Strikeout,
    /// Whether a text object's text is kerned.
    Kerning,
    /// How a text object's text is aligned across.
    HAlign,
    /// How a text object's text is aligned down.
    VAlign,
    /// Where tile objects of a tileset are anchored.
    ObjectAlignment,
    /// Where a tileset's tiles are drawn relative to their cell, across.
    TileOffsetX,
    /// Where a tileset's tiles are drawn relative to their cell, down.
    TileOffsetY,
    /// The size a tileset's tiles are drawn at on tile layers.
    TileRenderSize,
    /// How a tileset's tiles fill another size.
    FillMode,
    /// The shape of the cells of a tileset's grid.
    GridOrientation,
    /// The width of a tileset's grid cells.
    GridWidth,
    /// The height of a tileset's grid cells.
    GridHeight,
    /// Whether terrain filling may mirror a tileset's tiles left-right.
    AllowFlipHorizontally,
    /// Whether terrain filling may mirror a tileset's tiles top-bottom.
    AllowFlipVertically,
    /// Whether terrain filling may turn a tileset's tiles.
    AllowRotate,
    /// Whether terrain filling uses a variant only where no plain tile fits.
    PreferUntransformed,
    /// The pixels around an atlas's tiles; read-only.
    Margin,
    /// The pixels between an atlas's tiles; read-only.
    Spacing,
    /// A tile's relative weight for random painting and terrain filling.
    Probability,
}

impl Attribute {
    /// The name the Properties panel shows.
    pub fn label(self) -> &'static str {
        match self {
            Attribute::Id => "ID",
            Attribute::Template => "Template",
            Attribute::Name => "Name",
            Attribute::Class => "Class",
            Attribute::Orientation => "Orientation",
            Attribute::MapWidth => "Map Width",
            Attribute::MapHeight => "Map Height",
            Attribute::Infinite => "Infinite",
            Attribute::TileWidth => "Tile Width",
            Attribute::TileHeight => "Tile Height",
            Attribute::HexSideLength => "Hex Side Length",
            Attribute::StaggerAxis => "Stagger Axis",
            Attribute::StaggerIndex => "Stagger Index",
            Attribute::SkewX => "Skew X",
            Attribute::SkewY => "Skew Y",
            Attribute::ParallaxOriginX => "Parallax Origin X",
            Attribute::ParallaxOriginY => "Parallax Origin Y",
            Attribute::LayerDataFormat => "Layer Data Format",
            Attribute::CompressionLevel => "Compression Level",
            Attribute::ChunkWidth => "Output Chunk Width",
            Attribute::ChunkHeight => "Output Chunk Height",
            Attribute::RenderOrder => "Tile Render Order",
            Attribute::BackgroundColor => "Background Color",
            Attribute::Visible => "Visible",
            Attribute::Locked => "Locked",
            Attribute::Opacity => "Opacity",
            Attribute::TintColor => "Tint Color",
            Attribute::BlendMode => "Blend Mode",
            Attribute::OffsetX => "Offset X",
            Attribute::OffsetY => "Offset Y",
            Attribute::ParallaxX => "Parallax Factor X",
            Attribute::ParallaxY => "Parallax Factor Y",
            Attribute::Color => "Color",
            Attribute::DrawOrder => "Draw Order",
            Attribute::Image => "Image",
            Attribute::TransparentColor => "Transparent Color",
            Attribute::RepeatX => "Repeat X",
            Attribute::RepeatY => "Repeat Y",
            Attribute::X => "X",
            Attribute::Y => "Y",
            Attribute::Width => "Width",
            Attribute::Height => "Height",
            Attribute::Rotation => "Rotation",
            Attribute::FlipHorizontally => "Flip Horizontally",
            Attribute::FlipVertically => "Flip Vertically",
            Attribute::Text => "Text",
            Attribute::FontFamily => "Font Family",
            Attribute::PixelSize => "Pixel Size",
            Attribute::WordWrap => "Word Wrap",
            Attribute::TextColor => "Text Color",
            Attribute::Bold => "Bold",
            Attribute::Italic => "Italic",
            Attribute::Underline => "Underline",
            Attribute::Strikeout => "Strikeout",
            Attribute::Kerning => "Kerning",
            Attribute::HAlign => "Horizontal Alignment",
            Attribute::VAlign => "Vertical Alignment",
            Attribute::ObjectAlignment => "Object Alignment",
            Attribute::TileOffsetX => "Drawing Offset X",
            Attribute::TileOffsetY => "Drawing Offset Y",
            Attribute::TileRenderSize => "Tile Render Size",
            Attribute::FillMode => "Fill Mode",
            Attribute::GridOrientation => "Grid Orientation",
            Attribute::GridWidth => "Grid Width",
            Attribute::GridHeight => "Grid Height",
            Attribute::AllowFlipHorizontally => "Allow Flip Horizontally",
            Attribute::AllowFlipVertically => "Allow Flip Vertically",
            Attribute::AllowRotate => "Allow Rotate",
            Attribute::PreferUntransformed => "Prefer Untransformed",
            Attribute::Margin => "Margin",
            Attribute::Spacing => "Spacing",
            Attribute::Probability => "Probability",
        }
    }

    /// The values a choice offers, in the order Tiled's combo boxes list them; nothing for an
    /// attribute that is not a choice. A map's layer data format offers no XML, which is read
    /// but never written.
    pub fn choices(self) -> Vec<AttributeValue> {
        match self {
            Attribute::Orientation => [
                Orientation::Orthogonal,
                Orientation::Isometric,
                Orientation::Oblique,
                Orientation::Staggered,
                Orientation::Hexagonal,
            ]
            .map(AttributeValue::Orientation)
            .to_vec(),
            Attribute::StaggerAxis => [StaggerAxis::X, StaggerAxis::Y]
                .map(AttributeValue::StaggerAxis)
                .to_vec(),
            Attribute::StaggerIndex => [StaggerIndex::Odd, StaggerIndex::Even]
                .map(AttributeValue::StaggerIndex)
                .to_vec(),
            Attribute::LayerDataFormat => [
                LayerDataFormat::Csv,
                LayerDataFormat::Base64,
                LayerDataFormat::Base64Gzip,
                LayerDataFormat::Base64Zlib,
                LayerDataFormat::Base64Zstd,
            ]
            .map(AttributeValue::LayerDataFormat)
            .to_vec(),
            Attribute::RenderOrder => [
                RenderOrder::RightDown,
                RenderOrder::RightUp,
                RenderOrder::LeftDown,
                RenderOrder::LeftUp,
            ]
            .map(AttributeValue::RenderOrder)
            .to_vec(),
            Attribute::BlendMode => [
                BlendMode::Normal,
                BlendMode::Add,
                BlendMode::Multiply,
                BlendMode::Screen,
                BlendMode::Overlay,
                BlendMode::Darken,
                BlendMode::Lighten,
                BlendMode::ColorDodge,
                BlendMode::ColorBurn,
                BlendMode::HardLight,
                BlendMode::SoftLight,
                BlendMode::Difference,
                BlendMode::Exclusion,
            ]
            .map(AttributeValue::BlendMode)
            .to_vec(),
            Attribute::DrawOrder => [DrawOrder::TopDown, DrawOrder::Index]
                .map(AttributeValue::DrawOrder)
                .to_vec(),
            Attribute::ObjectAlignment => [
                Alignment::Unspecified,
                Alignment::TopLeft,
                Alignment::Top,
                Alignment::TopRight,
                Alignment::Left,
                Alignment::Center,
                Alignment::Right,
                Alignment::BottomLeft,
                Alignment::Bottom,
                Alignment::BottomRight,
            ]
            .map(AttributeValue::Alignment)
            .to_vec(),
            Attribute::TileRenderSize => [TileRenderSize::Tile, TileRenderSize::Grid]
                .map(AttributeValue::TileRenderSize)
                .to_vec(),
            Attribute::FillMode => [FillMode::Stretch, FillMode::PreserveAspectFit]
                .map(AttributeValue::FillMode)
                .to_vec(),
            Attribute::GridOrientation => [GridOrientation::Orthogonal, GridOrientation::Isometric]
                .map(AttributeValue::GridOrientation)
                .to_vec(),
            Attribute::HAlign => [HAlign::Left, HAlign::Center, HAlign::Right, HAlign::Justify]
                .map(AttributeValue::HAlign)
                .to_vec(),
            Attribute::VAlign => [VAlign::Top, VAlign::Center, VAlign::Bottom]
                .map(AttributeValue::VAlign)
                .to_vec(),
            _ => Vec::new(),
        }
    }

    /// The template field a template instance overrides when this is set on it (Tiled's
    /// `MapObject::Property`); `None` for what instances never take from their template (the
    /// position) and for what objects do not have.
    pub(crate) fn template_override(self) -> Option<ObjectOverrides> {
        match self {
            Attribute::Name => Some(ObjectOverrides::NAME),
            Attribute::Class => Some(ObjectOverrides::CLASS),
            Attribute::Width | Attribute::Height => Some(ObjectOverrides::SIZE),
            Attribute::Rotation => Some(ObjectOverrides::ROTATION),
            Attribute::Opacity => Some(ObjectOverrides::OPACITY),
            Attribute::Visible => Some(ObjectOverrides::VISIBLE),
            Attribute::FlipHorizontally | Attribute::FlipVertically => Some(ObjectOverrides::TILE),
            Attribute::Text
            | Attribute::FontFamily
            | Attribute::PixelSize
            | Attribute::WordWrap
            | Attribute::TextColor
            | Attribute::Bold
            | Attribute::Italic
            | Attribute::Underline
            | Attribute::Strikeout
            | Attribute::Kerning
            | Attribute::HAlign
            | Attribute::VAlign => Some(ObjectOverrides::TEXT),
            _ => None,
        }
    }
}

impl fmt::Display for Attribute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_follow_tileds_combo_boxes() {
        let labels: Vec<String> = Attribute::Orientation
            .choices()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            labels,
            [
                "Orthogonal",
                "Isometric",
                "Oblique",
                "Isometric (Staggered)",
                "Hexagonal (Staggered)"
            ]
        );
        assert!(
            !Attribute::LayerDataFormat
                .choices()
                .contains(&AttributeValue::LayerDataFormat(LayerDataFormat::Xml))
        );
        assert_eq!(Attribute::BlendMode.choices().len(), 13);
        assert!(Attribute::Opacity.choices().is_empty());
        assert_eq!(Attribute::ParallaxX.to_string(), "Parallax Factor X");
    }
}
