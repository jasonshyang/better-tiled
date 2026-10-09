//! Text objects.

use crate::color::Color;

/// The text of a text object and how it is set, with Tiled's defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct TextData {
    /// The text.
    pub text: String,
    /// The font family (`sans-serif`).
    pub font_family: String,
    /// The font size in pixels (16).
    pub pixel_size: u32,
    /// Whether lines wrap at the object's width.
    pub wrap: bool,
    /// The colour (black).
    pub color: Color,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Underlined.
    pub underline: bool,
    /// Struck out.
    pub strikeout: bool,
    /// Whether kerning is used (yes).
    pub kerning: bool,
    /// Horizontal alignment (left).
    pub halign: HAlign,
    /// Vertical alignment (top).
    pub valign: VAlign,
}

impl Default for TextData {
    fn default() -> TextData {
        TextData {
            text: String::new(),
            font_family: "sans-serif".to_owned(),
            pixel_size: 16,
            wrap: false,
            color: Color::BLACK,
            bold: false,
            italic: false,
            underline: false,
            strikeout: false,
            kerning: true,
            halign: HAlign::Left,
            valign: VAlign::Top,
        }
    }
}

/// Horizontal alignment of text.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum HAlign {
    /// Against the left edge.
    #[default]
    Left,
    /// Centred.
    Center,
    /// Against the right edge.
    Right,
    /// Stretched to both edges.
    Justify,
}

/// Vertical alignment of text.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum VAlign {
    /// Against the top edge.
    #[default]
    Top,
    /// Centred.
    Center,
    /// Against the bottom edge.
    Bottom,
}
