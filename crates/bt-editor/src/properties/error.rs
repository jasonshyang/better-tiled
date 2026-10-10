//! Why a change of custom properties or of a built-in attribute was refused.

use bt_core::{CircularMember, CustomTypeId, LayerId, ObjectId, TileId};

use super::attribute::Attribute;
use super::attribute_value::AttributeValue;
use super::target_id::TargetId;

/// What stopped a change of custom properties or of a built-in attribute. Whatever was given to
/// change is left as it was.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum PropertyError {
    /// The map has no layer with this id.
    #[error("the map has no layer {0}")]
    NoSuchLayer(LayerId),
    /// The map has no object with this id.
    #[error("the map has no object {0}")]
    NoSuchObject(ObjectId),
    /// The tileset has no tile with this id.
    #[error("the tileset has no tile {0}")]
    NoSuchTile(TileId),
    /// The tileset has no terrain set at this index.
    #[error("the tileset has no terrain set {0}")]
    NoSuchTerrainSet(usize),
    /// The terrain set has no terrain with this index.
    #[error("terrain set {set} has no terrain {color}")]
    NoSuchTerrain {
        /// The set.
        set: usize,
        /// The terrain.
        color: u8,
    },
    /// A target of a tileset was given a map to change.
    #[error("{0} is not part of a map")]
    NotInMap(TargetId),
    /// A target of a map was given a tileset to change.
    #[error("{0} is not part of a tileset")]
    NotInTileset(TargetId),
    /// A target that is not a class's members was given custom types to change.
    #[error("{0} is not part of the custom types")]
    NotInTypes(TargetId),
    /// The project has no custom type with this id.
    #[error("the project has no type {0}")]
    NoSuchType(CustomTypeId),
    /// The type is not a class.
    #[error("“{0}” is not a class")]
    NotAClass(String),
    /// The type is not an enum.
    #[error("“{0}” is not an enum")]
    NotAnEnum(String),
    /// A custom type needs a name.
    #[error("a type needs a name")]
    EmptyTypeName,
    /// Another custom type has the name (Tiled's message).
    #[error("the name “{0}” is already in use")]
    TypeNameTaken(String),
    /// An enum's value needs a name.
    #[error("an enum value needs a name")]
    EmptyValueName,
    /// The enum has the value already.
    #[error("the enum has a value “{0}” already")]
    ValueNameTaken(String),
    /// The enum has no value at this index.
    #[error("the enum has no value {0}")]
    NoSuchValue(usize),
    /// An enum of flags holds at most 31 values, one bit each.
    #[error("an enum of flags holds at most 31 values")]
    TooManyFlags,
    /// A member would make its class contain itself.
    #[error("class “{}” would contain itself through its member “{}”", .0.class, .0.member)]
    Circular(CircularMember),
    /// A property needs a name.
    #[error("a property needs a name")]
    EmptyName,
    /// A rename would replace a property a target has.
    #[error("there is a property “{0}” already")]
    NameTaken(String),
    /// The project has no class or enum of this name that property values can have.
    #[error("the project has no type “{0}” for property values")]
    UnknownType(String),
    /// The path does not suit the operation: a whole-property operation given a member, a reset
    /// given a list's item, a paste given a path, a path that does not start with a property.
    #[error("{operation} does not apply at “{path}”")]
    PathMismatch {
        /// The operation, as its command says it.
        operation: &'static str,
        /// The path, as Tiled writes it.
        path: String,
    },
    /// The attribute cannot be changed: an id, the size of a map, or one that does not apply in
    /// the current state (the hex side length of an orthogonal map).
    #[error("{0} cannot be changed here")]
    ReadOnly(Attribute),
    /// None of the targets has the attribute.
    #[error("{0} does not apply to what is selected")]
    NotApplicable(Attribute),
    /// A value of the wrong kind for the attribute, or one it cannot take (an empty text colour,
    /// a layer data format that is never written).
    #[error("{attribute} cannot be {value}")]
    InvalidValue {
        /// The attribute.
        attribute: Attribute,
        /// The value refused.
        value: AttributeValue,
    },
    /// A number outside the attribute's range.
    #[error("{attribute} must be {}", range_text(*.min, *.max))]
    OutOfRange {
        /// The attribute.
        attribute: Attribute,
        /// The least value it takes, if it has one.
        min: Option<f64>,
        /// The greatest value it takes, if it has one.
        max: Option<f64>,
    },
    /// The map refused the change (making it finite or infinite).
    #[error(transparent)]
    Operation(#[from] bt_edit::EditError),
}

/// A range in words: "between 0 and 1", "a finite number of at least 0", "a finite number".
fn range_text(min: Option<f64>, max: Option<f64>) -> String {
    match (min, max) {
        (Some(min), Some(max)) => format!("between {min} and {max}"),
        (Some(min), None) => format!("a finite number of at least {min}"),
        (None, Some(max)) => format!("a finite number of at most {max}"),
        (None, None) => "a finite number".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_read_as_words() {
        let error = PropertyError::OutOfRange {
            attribute: Attribute::Opacity,
            min: Some(0.0),
            max: Some(1.0),
        };
        assert_eq!(error.to_string(), "Opacity must be between 0 and 1");
        let error = PropertyError::OutOfRange {
            attribute: Attribute::X,
            min: None,
            max: None,
        };
        assert_eq!(error.to_string(), "X must be a finite number");
    }
}
