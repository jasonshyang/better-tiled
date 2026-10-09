//! A single value as a file states it.

use crate::numbers::{
    gid_of_integer, integer_of_real, parse_bool, parse_gid, parse_int, parse_real, real_shortest,
};

/// A value as a file states it: XML attributes are text; JSON values are text, numbers or
/// booleans. Values going out carry the type their printer formats them by.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Scalar {
    /// Text.
    Text(String),
    /// A whole number.
    Int(i64),
    /// A real number.
    Real(f64),
    /// A boolean.
    Bool(bool),
}

impl Scalar {
    /// Text.
    pub(crate) fn text(value: impl Into<String>) -> Scalar {
        Scalar::Text(value.into())
    }

    /// A whole number.
    pub(crate) fn int(value: impl Into<i64>) -> Scalar {
        Scalar::Int(value.into())
    }

    /// The value as text: numbers in their shortest form, booleans as `true`/`false`, as
    /// `QVariant::toString` gives them.
    pub(crate) fn to_text(&self) -> String {
        match self {
            Scalar::Text(text) => text.clone(),
            Scalar::Int(value) => value.to_string(),
            Scalar::Real(value) => real_shortest(*value),
            Scalar::Bool(value) => value.to_string(),
        }
    }

    /// The value as an integer, leniently: text in integer or float notation, a real rounded,
    /// a boolean as 0 or 1.
    pub(crate) fn to_int(&self) -> Option<i64> {
        match self {
            Scalar::Text(text) => parse_int(text),
            Scalar::Int(value) => Some(*value),
            Scalar::Real(value) => integer_of_real(*value),
            Scalar::Bool(value) => Some(i64::from(*value)),
        }
    }

    /// The value as a real.
    pub(crate) fn to_real(&self) -> Option<f64> {
        match self {
            Scalar::Text(text) => parse_real(text),
            // Integers in files are far below 2^53.
            Scalar::Int(value) => Some(*value as f64),
            Scalar::Real(value) => Some(*value),
            Scalar::Bool(value) => Some(f64::from(u8::from(*value))),
        }
    }

    /// The value as a boolean: `true`/`false`, or a number that is not zero.
    pub(crate) fn to_bool(&self) -> Option<bool> {
        match self {
            Scalar::Text(text) => parse_bool(text),
            Scalar::Int(value) => Some(*value != 0),
            Scalar::Real(value) => Some(*value != 0.0),
            Scalar::Bool(value) => Some(*value),
        }
    }

    /// The value as a global tile id, unsigned or in its signed 32-bit form.
    pub(crate) fn to_gid(&self) -> Option<u32> {
        match self {
            Scalar::Text(text) => parse_gid(text),
            Scalar::Int(value) => gid_of_integer(*value),
            Scalar::Real(value) => integer_of_real(*value).and_then(gid_of_integer),
            Scalar::Bool(_) => None,
        }
    }

    /// Whether the value is empty text.
    pub(crate) fn is_empty_text(&self) -> bool {
        matches!(self, Scalar::Text(text) if text.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars_convert_like_qvariants() {
        assert_eq!(Scalar::text("32.0").to_int(), Some(32));
        assert_eq!(Scalar::Real(2.5).to_int(), Some(3));
        assert_eq!(Scalar::Bool(true).to_int(), Some(1));
        assert_eq!(Scalar::Int(0).to_bool(), Some(false));
        assert_eq!(Scalar::text("true").to_bool(), Some(true));
        assert_eq!(Scalar::Real(1.1).to_text(), "1.1");
        assert_eq!(Scalar::Int(-2_147_479_356).to_gid(), Some(2_147_487_940));
        assert_eq!(Scalar::text("x").to_real(), None);
        assert!(Scalar::text(" ").is_empty_text());
    }
}
