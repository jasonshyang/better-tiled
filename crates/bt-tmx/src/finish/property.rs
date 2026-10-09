//! Custom properties: values typed as the file states, or as the project's classes say for the
//! untyped members of JSON class values.

use bt_core::{Color, CustomTypes, ObjectId, Properties, Property, Value};

use crate::raw::{RawProperty, RawValue, Scalar};
use crate::warning::{WarningKind, Warnings};

/// Decodes raw properties with a project's custom types.
pub(crate) struct PropertyDecoder<'a> {
    types: &'a CustomTypes,
    warnings: &'a mut Warnings,
}

impl<'a> PropertyDecoder<'a> {
    /// A decoder using `types`, recording into `warnings`.
    pub(crate) fn new(types: &'a CustomTypes, warnings: &'a mut Warnings) -> PropertyDecoder<'a> {
        PropertyDecoder { types, warnings }
    }

    /// The properties of an element, the last of a name winning, brought into the forms the
    /// custom types prescribe.
    pub(crate) fn properties_of(&mut self, raw: Vec<RawProperty>) -> Properties {
        let mut properties = Properties::new();
        for property in raw {
            let name = property.name.clone();
            let decoded = self.property_of(property);
            properties.insert(name, decoded);
        }
        self.types.normalize_all(&mut properties);
        properties
    }

    /// One property, member or list item.
    pub(crate) fn property_of(&mut self, raw: RawProperty) -> Property {
        let custom_type = raw.custom_type.filter(|name| !name.is_empty());
        let value = self.value_of(raw.type_name.as_deref(), raw.value, custom_type.as_deref());
        Property { value, custom_type }
    }

    /// A value of the stated type; `None` types it after its own form.
    fn value_of(
        &mut self,
        type_name: Option<&str>,
        raw: RawValue,
        custom_type: Option<&str>,
    ) -> Value {
        match type_name {
            None => self.guessed(raw),
            Some("string") => Value::String(self.text_of(raw, "string")),
            Some("file") => Value::File(self.text_of(raw, "file")),
            Some("int") => Value::Int(self.scalar_of(raw, "int", Scalar::to_int).unwrap_or(0)),
            Some("float") => {
                Value::Float(self.scalar_of(raw, "float", Scalar::to_real).unwrap_or(0.0))
            }
            Some("bool") => {
                Value::Bool(self.scalar_of(raw, "bool", qvariant_bool).unwrap_or(false))
            }
            Some("object") => Value::Object(ObjectId(
                self.scalar_of(raw, "object", |s| {
                    s.to_int().and_then(|i| u32::try_from(i).ok())
                })
                .unwrap_or(0),
            )),
            Some("color") => Value::Color(self.color_of(raw)),
            Some("class") => Value::Class(match raw {
                RawValue::Members(members) => self.members_of(members, custom_type),
                RawValue::Absent => Properties::new(),
                RawValue::Scalar(scalar) if scalar.is_empty_text() => Properties::new(),
                other => {
                    self.record_mismatch(&other, "class");
                    Properties::new()
                }
            }),
            Some("list") => Value::List(self.items_of(raw)),
            Some(other) => {
                if let RawValue::Items(_) = raw {
                    // Tiled 1.12.0 to 1.12.2 wrote pasted lists with this type.
                    self.warnings.record(
                        WarningKind::LegacyShape,
                        format!("a list property of type “{other}” is read as a list"),
                    );
                    return Value::List(self.items_of(raw));
                }
                self.warnings.record(
                    WarningKind::UnknownPropertyType,
                    format!("the property type “{other}” is not one of Tiled's; its value is read as it is"),
                );
                self.guessed(raw)
            }
        }
    }

    /// A value typed after its form: text, number, boolean, members or items.
    fn guessed(&mut self, raw: RawValue) -> Value {
        match raw {
            RawValue::Scalar(scalar) => guessed_value(scalar),
            RawValue::Members(members) => Value::Class(self.members_of(members, None)),
            RawValue::Items(_) => Value::List(self.items_of(raw)),
            RawValue::Absent => Value::String(String::new()),
        }
    }

    /// The members of a class value: typed as stated, else like the member of `class` with the
    /// same name, else after their form.
    fn members_of(&mut self, members: Vec<RawProperty>, class: Option<&str>) -> Properties {
        let types = self.types;
        let definition = class.and_then(|name| types.value_class(name));
        let mut out = Properties::new();
        for member in members {
            let name = member.name.clone();
            let template = definition.and_then(|class| class.members.get(&name));
            let property = match (&member.type_name, template) {
                (None, Some(template)) => self.like(member, template),
                _ => self.property_of(member),
            };
            out.insert(name, property);
        }
        out
    }

    /// An untyped member typed like the class member `template`.
    fn like(&mut self, member: RawProperty, template: &Property) -> Property {
        let custom_type = template.custom_type.clone();
        let is_enum = custom_type
            .as_deref()
            .is_some_and(|name| self.types.enum_type(name).is_some());
        let value = if is_enum {
            // Enum values are names or numbers; normalizing brings them into their storage.
            self.guessed(member.value)
        } else {
            self.value_of(
                Some(template.value.type_name()),
                member.value,
                custom_type.as_deref(),
            )
        };
        Property { value, custom_type }
    }

    /// A list's items, each typed as stated or after its form.
    fn items_of(&mut self, raw: RawValue) -> Vec<Property> {
        match raw {
            RawValue::Items(items) => items
                .into_iter()
                .map(|item| self.property_of(item))
                .collect(),
            RawValue::Absent => Vec::new(),
            RawValue::Scalar(scalar) if scalar.is_empty_text() => Vec::new(),
            other => {
                self.record_mismatch(&other, "list");
                Vec::new()
            }
        }
    }

    fn text_of(&mut self, raw: RawValue, type_name: &str) -> String {
        match raw {
            RawValue::Scalar(scalar) => scalar.to_text(),
            RawValue::Absent => String::new(),
            other => {
                self.record_mismatch(&other, type_name);
                String::new()
            }
        }
    }

    fn scalar_of<T>(
        &mut self,
        raw: RawValue,
        type_name: &str,
        convert: impl FnOnce(&Scalar) -> Option<T>,
    ) -> Option<T> {
        match raw {
            RawValue::Scalar(scalar) if scalar.is_empty_text() => None,
            RawValue::Scalar(scalar) => {
                let converted = convert(&scalar);
                if converted.is_none() {
                    self.warnings.record(
                        WarningKind::InvalidValue,
                        format!(
                            "the {type_name} property value “{}” is not valid; the default is used",
                            scalar.to_text()
                        ),
                    );
                }
                converted
            }
            RawValue::Absent => None,
            other => {
                self.record_mismatch(&other, type_name);
                None
            }
        }
    }

    fn color_of(&mut self, raw: RawValue) -> Option<Color> {
        let text = match raw {
            RawValue::Scalar(scalar) => scalar.to_text(),
            RawValue::Absent => return None,
            other => {
                self.record_mismatch(&other, "color");
                return None;
            }
        };
        if text.trim().is_empty() {
            return None;
        }
        let color = Color::from_tiled_text(&text);
        if color.is_none() {
            self.warnings.record(
                WarningKind::InvalidValue,
                format!("the color property value “{text}” is not a colour; it is left empty"),
            );
        }
        color
    }

    fn record_mismatch(&mut self, raw: &RawValue, type_name: &str) {
        let form = match raw {
            RawValue::Scalar(_) => "a single value",
            RawValue::Members(_) => "members",
            RawValue::Items(_) => "list items",
            RawValue::Absent => "nothing",
        };
        self.warnings.record(
            WarningKind::InvalidValue,
            format!("a {type_name} property holds {form}; the default is used"),
        );
    }
}

/// A value typed after its form: text is a string, a number an int or a float.
fn guessed_value(scalar: Scalar) -> Value {
    match scalar {
        Scalar::Text(text) => Value::String(text),
        Scalar::Int(value) => Value::Int(value),
        Scalar::Real(value) => Value::Float(value),
        Scalar::Bool(value) => Value::Bool(value),
    }
}

/// `QVariant`'s text-to-boolean conversion: everything but empty text, `0` and `false` is
/// true.
fn qvariant_bool(scalar: &Scalar) -> Option<bool> {
    match scalar {
        Scalar::Text(text) => {
            let text = text.trim();
            Some(!(text.is_empty() || text == "0" || text.eq_ignore_ascii_case("false")))
        }
        other => other.to_bool(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use bt_core::{ClassType, CustomType, EnumStorage, EnumType};

    use super::*;

    fn types() -> CustomTypes {
        let mut types = CustomTypes::new();
        types.add(CustomType::enumeration(
            "Dir",
            EnumType {
                storage: EnumStorage::Int,
                values: vec!["Up".into(), "Down".into()],
                values_as_flags: false,
            },
        ));
        let members = [
            ("dir", Property::typed(Value::Int(0), "Dir")),
            ("speed", Property::new(Value::Float(0.0))),
            ("tint", Property::new(Value::Color(None))),
            ("script", Property::new(Value::File(String::new()))),
            ("target", Property::new(Value::Object(ObjectId(0)))),
        ];
        types.add(CustomType::class(
            "Spawn",
            ClassType {
                members: members
                    .into_iter()
                    .map(|(name, property)| (name.to_owned(), property))
                    .collect(),
                ..ClassType::default()
            },
        ));
        types
    }

    fn untyped(name: &str, value: Scalar) -> RawProperty {
        RawProperty {
            name: name.into(),
            value: RawValue::Scalar(value),
            ..RawProperty::default()
        }
    }

    #[test]
    fn json_class_members_take_their_classes_types() {
        let raw = RawProperty {
            name: "s".into(),
            type_name: Some("class".into()),
            custom_type: Some("Spawn".into()),
            value: RawValue::Members(vec![
                untyped("dir", Scalar::text("Down")),
                untyped("speed", Scalar::Int(2)),
                untyped("tint", Scalar::text("#ff102030")),
                untyped("script", Scalar::text("a.lua")),
                untyped("target", Scalar::Int(7)),
                untyped("extra", Scalar::Real(1.5)),
            ]),
        };
        let types = types();
        let mut warnings = Warnings::new(Path::new("/m.tmj"));
        let properties =
            PropertyDecoder::new(&types, &mut warnings).properties_of(vec![raw.clone()]);
        let Value::Class(members) = &properties["s"].value else {
            panic!()
        };
        assert_eq!(members["dir"], Property::typed(Value::Int(1), "Dir"));
        assert_eq!(members["speed"].value, Value::Float(2.0));
        assert_eq!(
            members["tint"].value,
            Value::Color(Some(Color::rgb(0x10, 0x20, 0x30)))
        );
        assert_eq!(members["script"].value, Value::File("a.lua".into()));
        assert_eq!(members["target"].value, Value::Object(ObjectId(7)));
        assert_eq!(members["extra"].value, Value::Float(1.5));

        let none = CustomTypes::new();
        let properties = PropertyDecoder::new(&none, &mut warnings).properties_of(vec![raw]);
        let Value::Class(members) = &properties["s"].value else {
            panic!()
        };
        assert_eq!(members["dir"], Property::new(Value::String("Down".into())));
        assert_eq!(members["speed"].value, Value::Int(2));
        assert_eq!(properties["s"].custom_type.as_deref(), Some("Spawn"));
        assert!(warnings.into_list().is_empty());
    }

    #[test]
    fn stated_types_convert_leniently() {
        let typed = |type_name: &str, value: &str| RawProperty {
            name: type_name.into(),
            type_name: Some(type_name.into()),
            value: RawValue::Scalar(Scalar::text(value)),
            ..RawProperty::default()
        };
        let types = CustomTypes::new();
        let mut warnings = Warnings::new(Path::new("/m.tmx"));
        let properties = PropertyDecoder::new(&types, &mut warnings).properties_of(vec![
            typed("int", "32.0"),
            typed("float", "1e3"),
            typed("bool", "yes"),
            typed("color", ""),
            typed("object", "x"),
            typed("std::nullptr_t", "1"),
        ]);
        assert_eq!(properties["int"].value, Value::Int(32));
        assert_eq!(properties["float"].value, Value::Float(1000.0));
        assert_eq!(properties["bool"].value, Value::Bool(true));
        assert_eq!(properties["color"].value, Value::Color(None));
        assert_eq!(properties["object"].value, Value::Object(ObjectId(0)));
        assert_eq!(
            properties["std::nullptr_t"].value,
            Value::String("1".into())
        );
        let kinds: Vec<WarningKind> = warnings.into_list().into_iter().map(|w| w.kind).collect();
        assert_eq!(
            kinds,
            [WarningKind::InvalidValue, WarningKind::UnknownPropertyType]
        );
    }
}
