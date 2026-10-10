//! What values each built-in attribute takes, checked before anything is set: the kind of value,
//! the range of numbers (Tiled's minimums: tiles and grid cells at least a pixel, output chunks
//! at least 4 tiles, probabilities and sizes not negative, opacities from 0 to 1), whether a
//! colour may be none or translucent. Nothing out of range is clamped.

use bt_core::{Color, ImageRef};

use super::attribute::Attribute;
use super::attribute_value::AttributeValue;
use super::error::PropertyError;

/// What values an attribute takes.
enum Takes {
    /// Text.
    Text,
    /// Whole numbers in a range.
    Integer {
        /// The least.
        min: i64,
        /// The greatest.
        max: i64,
    },
    /// Finite real numbers, in a range where there is one.
    Number {
        /// The least, if any.
        min: Option<f64>,
        /// The greatest, if any.
        max: Option<f64>,
    },
    /// Yes or no.
    Bool,
    /// A colour.
    Color {
        /// Whether it may be none.
        optional: bool,
        /// Whether it may be translucent.
        alpha: bool,
    },
    /// An image, or none.
    Image,
    /// A file, or none.
    File,
    /// One of the attribute's choices.
    Choice,
}

/// The least `i32`: offsets in files fit in one.
const I32_MIN: i64 = i32::MIN as i64;
/// The greatest `i32`: sizes in files fit in one.
const I32_MAX: i64 = i32::MAX as i64;

/// What values `attribute` takes.
fn takes(attribute: Attribute) -> Takes {
    let integer = |min, max| Takes::Integer { min, max };
    let finite = Takes::Number {
        min: None,
        max: None,
    };
    let not_negative = Takes::Number {
        min: Some(0.0),
        max: None,
    };
    match attribute {
        Attribute::Name | Attribute::Class | Attribute::Text | Attribute::FontFamily => Takes::Text,
        Attribute::Id
        | Attribute::MapWidth
        | Attribute::MapHeight
        | Attribute::Margin
        | Attribute::Spacing => integer(i64::MIN, i64::MAX),
        Attribute::TileWidth
        | Attribute::TileHeight
        | Attribute::PixelSize
        | Attribute::GridWidth
        | Attribute::GridHeight => integer(1, I32_MAX),
        Attribute::HexSideLength => integer(0, I32_MAX),
        Attribute::SkewX | Attribute::SkewY | Attribute::TileOffsetX | Attribute::TileOffsetY => {
            integer(I32_MIN, I32_MAX)
        }
        Attribute::CompressionLevel => integer(-1, 22),
        Attribute::ChunkWidth | Attribute::ChunkHeight => integer(4, I32_MAX),
        Attribute::Opacity => Takes::Number {
            min: Some(0.0),
            max: Some(1.0),
        },
        Attribute::Probability | Attribute::Width | Attribute::Height => not_negative,
        Attribute::ParallaxOriginX
        | Attribute::ParallaxOriginY
        | Attribute::OffsetX
        | Attribute::OffsetY
        | Attribute::ParallaxX
        | Attribute::ParallaxY
        | Attribute::X
        | Attribute::Y
        | Attribute::Rotation => finite,
        Attribute::Infinite
        | Attribute::Visible
        | Attribute::Locked
        | Attribute::RepeatX
        | Attribute::RepeatY
        | Attribute::FlipHorizontally
        | Attribute::FlipVertically
        | Attribute::WordWrap
        | Attribute::Bold
        | Attribute::Italic
        | Attribute::Underline
        | Attribute::Strikeout
        | Attribute::Kerning
        | Attribute::AllowFlipHorizontally
        | Attribute::AllowFlipVertically
        | Attribute::AllowRotate
        | Attribute::PreferUntransformed => Takes::Bool,
        Attribute::BackgroundColor | Attribute::TintColor | Attribute::Color => Takes::Color {
            optional: true,
            alpha: true,
        },
        Attribute::TransparentColor => Takes::Color {
            optional: true,
            alpha: false,
        },
        Attribute::TextColor => Takes::Color {
            optional: false,
            alpha: true,
        },
        Attribute::Image => Takes::Image,
        Attribute::Template => Takes::File,
        Attribute::Orientation
        | Attribute::StaggerAxis
        | Attribute::StaggerIndex
        | Attribute::LayerDataFormat
        | Attribute::RenderOrder
        | Attribute::BlendMode
        | Attribute::DrawOrder
        | Attribute::ObjectAlignment
        | Attribute::TileRenderSize
        | Attribute::FillMode
        | Attribute::GridOrientation
        | Attribute::HAlign
        | Attribute::VAlign => Takes::Choice,
    }
}

/// Refuses a value `attribute` cannot take: one of another kind, a number out of its range, no
/// colour or a translucent one where it needs one or an opaque one, a choice it does not offer.
pub(crate) fn check_value(
    attribute: Attribute,
    value: &AttributeValue,
) -> Result<(), PropertyError> {
    let invalid = || PropertyError::InvalidValue {
        attribute,
        value: value.clone(),
    };
    match (takes(attribute), value) {
        (Takes::Text, AttributeValue::Text(_))
        | (Takes::Bool, AttributeValue::Bool(_))
        | (Takes::Image, AttributeValue::Image(_))
        | (Takes::File, AttributeValue::File(_)) => Ok(()),
        (Takes::Integer { min, max }, AttributeValue::Integer(number)) => {
            if (min..=max).contains(number) {
                Ok(())
            } else {
                // Bounds within i32, or never reached: exact as reals.
                Err(PropertyError::OutOfRange {
                    attribute,
                    min: Some(min as f64),
                    max: Some(max as f64),
                })
            }
        }
        (Takes::Number { min, max }, AttributeValue::Number(number)) => {
            let fits = number.is_finite()
                && min.is_none_or(|min| *number >= min)
                && max.is_none_or(|max| *number <= max);
            if fits {
                Ok(())
            } else {
                Err(PropertyError::OutOfRange {
                    attribute,
                    min,
                    max,
                })
            }
        }
        (Takes::Color { optional, alpha }, AttributeValue::Color(color)) => match color {
            None if !optional => Err(invalid()),
            Some(color) if !alpha && color.a != u8::MAX => Err(invalid()),
            _ => Ok(()),
        },
        (Takes::Choice, value) if attribute.choices().contains(value) => Ok(()),
        _ => Err(invalid()),
    }
}

/// The text of a text value.
pub(crate) fn text_of(
    attribute: Attribute,
    value: &AttributeValue,
) -> Result<String, PropertyError> {
    match value {
        AttributeValue::Text(text) => Ok(text.clone()),
        _ => Err(invalid(attribute, value)),
    }
}

/// The number of a whole-number value, as the field's type.
pub(crate) fn integer_of<T: TryFrom<i64>>(
    attribute: Attribute,
    value: &AttributeValue,
) -> Result<T, PropertyError> {
    match value {
        AttributeValue::Integer(number) => {
            T::try_from(*number).map_err(|_| PropertyError::OutOfRange {
                attribute,
                min: None,
                max: None,
            })
        }
        _ => Err(invalid(attribute, value)),
    }
}

/// The number of a real-number value.
pub(crate) fn number_of(
    attribute: Attribute,
    value: &AttributeValue,
) -> Result<f64, PropertyError> {
    match value {
        AttributeValue::Number(number) => Ok(*number),
        _ => Err(invalid(attribute, value)),
    }
}

/// The yes or no of a bool value.
pub(crate) fn bool_of(attribute: Attribute, value: &AttributeValue) -> Result<bool, PropertyError> {
    match value {
        AttributeValue::Bool(value) => Ok(*value),
        _ => Err(invalid(attribute, value)),
    }
}

/// The colour of a colour value.
pub(crate) fn color_of(
    attribute: Attribute,
    value: &AttributeValue,
) -> Result<Option<Color>, PropertyError> {
    match value {
        AttributeValue::Color(color) => Ok(*color),
        _ => Err(invalid(attribute, value)),
    }
}

/// The image of an image value.
pub(crate) fn image_of(
    attribute: Attribute,
    value: &AttributeValue,
) -> Result<Option<ImageRef>, PropertyError> {
    match value {
        AttributeValue::Image(image) => Ok(image.clone()),
        _ => Err(invalid(attribute, value)),
    }
}

/// The refusal of a value of the wrong kind.
pub(crate) fn invalid(attribute: Attribute, value: &AttributeValue) -> PropertyError {
    PropertyError::InvalidValue {
        attribute,
        value: value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use bt_core::LayerDataFormat;

    use super::*;

    #[test]
    fn values_out_of_range_or_of_the_wrong_kind_are_refused() {
        assert_eq!(
            check_value(Attribute::Opacity, &AttributeValue::Number(1.5)),
            Err(PropertyError::OutOfRange {
                attribute: Attribute::Opacity,
                min: Some(0.0),
                max: Some(1.0)
            })
        );
        assert!(check_value(Attribute::Opacity, &AttributeValue::Number(f64::NAN)).is_err());
        assert!(check_value(Attribute::Opacity, &AttributeValue::Number(0.25)).is_ok());
        assert!(check_value(Attribute::X, &AttributeValue::Number(f64::INFINITY)).is_err());
        assert!(check_value(Attribute::X, &AttributeValue::Number(-1e9)).is_ok());
        assert!(check_value(Attribute::TileWidth, &AttributeValue::Integer(0)).is_err());
        assert!(check_value(Attribute::ChunkWidth, &AttributeValue::Integer(3)).is_err());
        assert!(check_value(Attribute::Width, &AttributeValue::Number(-1.0)).is_err());
        assert_eq!(
            check_value(Attribute::Opacity, &AttributeValue::Integer(1)),
            Err(PropertyError::InvalidValue {
                attribute: Attribute::Opacity,
                value: AttributeValue::Integer(1)
            })
        );
        assert!(check_value(Attribute::TextColor, &AttributeValue::Color(None)).is_err());
        assert!(
            check_value(
                Attribute::TransparentColor,
                &AttributeValue::Color(Some(Color::rgba(1, 2, 3, 4)))
            )
            .is_err()
        );
        assert!(
            check_value(
                Attribute::LayerDataFormat,
                &AttributeValue::LayerDataFormat(LayerDataFormat::Xml)
            )
            .is_err(),
            "XML is never written"
        );
        assert!(
            check_value(
                Attribute::LayerDataFormat,
                &AttributeValue::LayerDataFormat(LayerDataFormat::Csv)
            )
            .is_ok()
        );
        assert!(
            check_value(
                Attribute::Orientation,
                &AttributeValue::LayerDataFormat(LayerDataFormat::Csv)
            )
            .is_err()
        );
    }
}
