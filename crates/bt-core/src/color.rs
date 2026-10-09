//! Colours.

/// A straight-alpha RGBA colour, eight bits a channel.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Alpha: 0 is transparent, 255 opaque.
    pub a: u8,
}

impl Color {
    /// Opaque white.
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    /// Opaque black.
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    /// Tiled's grey for objects of classes without a colour of their own (`#a0a0a4`).
    pub const GRAY: Color = Color::rgb(160, 160, 164);

    /// An opaque colour.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }

    /// A colour with alpha.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r, g, b, a }
    }

    /// Reads Tiled's text form: `#rrggbb`, or `#aarrggbb` with alpha first, with or without the
    /// `#`. `None` for anything else, the empty text included.
    pub fn from_tiled_text(text: &str) -> Option<Color> {
        let text = text.trim();
        let hex = text.strip_prefix('#').unwrap_or(text);
        let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        match hex.len() {
            6 => Some(Color::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Color::rgba(byte(2)?, byte(4)?, byte(6)?, byte(0)?)),
            _ => None,
        }
    }

    /// Tiled's text form: `#rrggbb` when opaque, else `#aarrggbb`.
    pub fn to_tiled_text(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.a, self.r, self.g, self.b)
        }
    }

    /// The channels in RGBA order.
    pub const fn to_array(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tileds_text_puts_alpha_first() {
        assert_eq!(
            Color::from_tiled_text("#80ff0000"),
            Some(Color::rgba(255, 0, 0, 128))
        );
        assert_eq!(
            Color::from_tiled_text(" ff0000 "),
            Some(Color::rgb(255, 0, 0))
        );
        assert_eq!(Color::from_tiled_text(""), None);
        assert_eq!(Color::from_tiled_text("#12345"), None);
        assert_eq!(Color::from_tiled_text("#gg0000"), None);
        assert_eq!(Color::from_tiled_text("#ff00€0"), None);
        assert_eq!(Color::rgba(1, 2, 3, 4).to_tiled_text(), "#04010203");
        assert_eq!(Color::rgb(160, 160, 164).to_tiled_text(), "#a0a0a4");
    }
}
