//! The types a property value can have: Tiled's built-in types and the project's classes and
//! enums, with the value a new property of each type starts with (libtiled's
//! `possiblePropertyValues`, `reference/tiled/src/libtiled/properties.cpp`).

use bt_core::{ClassUsage, CustomTypeKind, CustomTypes, ObjectId, Properties, Property, Value};

use super::error::PropertyError;

/// The type of a property value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ValueType {
    /// Yes or no.
    Bool,
    /// A colour, or none.
    Color,
    /// A real number.
    Float,
    /// A file.
    File,
    /// A whole number.
    Int,
    /// An object of the map, or none.
    Object,
    /// Text.
    String,
    /// A list of values of any types.
    List,
    /// A class or an enum of the project, by name.
    Custom(String),
}

impl ValueType {
    /// The built-in types, in the order Tiled offers them for a new property.
    pub const BUILT_IN: [ValueType; 8] = [
        ValueType::Bool,
        ValueType::Color,
        ValueType::Float,
        ValueType::File,
        ValueType::Int,
        ValueType::Object,
        ValueType::String,
        ValueType::List,
    ];

    /// The type of a value: the class or enum it names, else the built-in type of its value.
    /// `None` for a class value that names no class, which no type describes.
    pub fn of(property: &Property) -> Option<ValueType> {
        if let Some(name) = &property.custom_type {
            return Some(ValueType::Custom(name.clone()));
        }
        Some(match property.value {
            Value::String(_) => ValueType::String,
            Value::Int(_) => ValueType::Int,
            Value::Float(_) => ValueType::Float,
            Value::Bool(_) => ValueType::Bool,
            Value::Color(_) => ValueType::Color,
            Value::File(_) => ValueType::File,
            Value::Object(_) => ValueType::Object,
            Value::List(_) => ValueType::List,
            Value::Class(_) => return None,
        })
    }

    /// The type's name as Tiled shows it: `bool`, `float`, a class's or an enum's name.
    pub fn name(&self) -> &str {
        match self {
            ValueType::Bool => "bool",
            ValueType::Color => "color",
            ValueType::Float => "float",
            ValueType::File => "file",
            ValueType::Int => "int",
            ValueType::Object => "object",
            ValueType::String => "string",
            ValueType::List => "list",
            ValueType::Custom(name) => name,
        }
    }

    /// A new value of this type, as Add Property makes it: false, the empty colour, 0, no file,
    /// no object, the empty text or list; an enum's first value (no flags) in its storage form;
    /// a class value with no member set, which shows the class's defaults. A custom type the
    /// project does not have for property values is an error.
    pub fn default_value(&self, types: &CustomTypes) -> Result<Property, PropertyError> {
        let value = match self {
            ValueType::Bool => Value::Bool(false),
            ValueType::Color => Value::Color(None),
            ValueType::Float => Value::Float(0.0),
            ValueType::File => Value::File(String::new()),
            ValueType::Int => Value::Int(0),
            ValueType::Object => Value::Object(ObjectId(0)),
            ValueType::String => Value::String(String::new()),
            ValueType::List => Value::List(Vec::new()),
            ValueType::Custom(name) => {
                let found = types
                    .property_type(name)
                    .ok_or_else(|| PropertyError::UnknownType(name.clone()))?;
                let value = match &found.kind {
                    CustomTypeKind::Enum(enumeration) => enumeration.default_value(),
                    CustomTypeKind::Class(_) => Value::Class(Properties::new()),
                };
                return Ok(Property::typed(value, name.clone()));
            }
        };
        Ok(Property::new(value))
    }
}

/// The types a new property or list item can have, in Tiled's order: the built-in ones, then
/// the project's enums and the classes that can be property values, in the project's order.
pub fn value_types(types: &CustomTypes) -> Vec<ValueType> {
    let custom = types
        .types
        .iter()
        .filter(|t| t.usage().contains(ClassUsage::PROPERTY))
        .map(|t| ValueType::Custom(t.name.clone()));
    ValueType::BUILT_IN.into_iter().chain(custom).collect()
}

#[cfg(test)]
mod tests {
    use bt_core::{ClassType, CustomType, EnumStorage, EnumType};

    use super::*;

    fn types() -> CustomTypes {
        let mut types = CustomTypes::new();
        types.add(CustomType::enumeration(
            "Kind",
            EnumType {
                values: vec!["Small".into(), "Large".into()],
                ..EnumType::default()
            },
        ));
        types.add(CustomType::enumeration(
            "Layers",
            EnumType {
                storage: EnumStorage::Int,
                values: vec!["A".into(), "B".into()],
                values_as_flags: true,
            },
        ));
        types.add(CustomType::class("Loot", ClassType::default()));
        types.add(CustomType::class(
            "Enemy",
            ClassType {
                use_as: ClassUsage::OBJECT,
                ..ClassType::default()
            },
        ));
        types
    }

    #[test]
    fn new_values_start_empty_or_at_the_first_enum_value() {
        let types = types();
        assert_eq!(
            ValueType::Color.default_value(&types),
            Ok(Property::new(Value::Color(None)))
        );
        assert_eq!(
            ValueType::Custom("Kind".into()).default_value(&types),
            Ok(Property::typed(Value::String("Small".into()), "Kind"))
        );
        assert_eq!(
            ValueType::Custom("Layers".into()).default_value(&types),
            Ok(Property::typed(Value::Int(0), "Layers"))
        );
        assert_eq!(
            ValueType::Custom("Loot".into()).default_value(&types),
            Ok(Property::typed(Value::Class(Properties::new()), "Loot"))
        );
        assert_eq!(
            ValueType::Custom("Enemy".into()).default_value(&types),
            Err(PropertyError::UnknownType("Enemy".into())),
            "a class only for objects is not a property type"
        );
    }

    #[test]
    fn the_types_offered_follow_tileds_order() {
        let names: Vec<String> = value_types(&types())
            .iter()
            .map(|t| t.name().to_owned())
            .collect();
        assert_eq!(
            names,
            [
                "bool", "color", "float", "file", "int", "object", "string", "list", "Kind",
                "Layers", "Loot"
            ]
        );
        assert_eq!(
            ValueType::of(&Property::typed(Value::Int(1), "Layers")),
            Some(ValueType::Custom("Layers".into()))
        );
        assert_eq!(
            ValueType::of(&Property::new(Value::Class(Properties::new()))),
            None
        );
    }
}
