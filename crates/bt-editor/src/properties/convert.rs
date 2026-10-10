//! Converting property values between types: Tiled's Convert To (`showContextMenu` in
//! `reference/tiled/src/tiled/propertieswidget.cpp`), which converts with Qt's `QVariant`
//! conversions between the built-in types and Tiled's own between strings and files and
//! between ints and object references; enum values convert to and from their names and their
//! index or flags. The keep-type rule for values typed over several targets is built on it.

use bt_core::{Color, CustomTypes, EnumType, ObjectId, Property, Value};

use super::value_type::ValueType;

/// The built-in types Convert To offers, in Tiled's order.
const CONVERT_TO: [ValueType; 7] = [
    ValueType::Bool,
    ValueType::Color,
    ValueType::Float,
    ValueType::File,
    ValueType::Object,
    ValueType::Int,
    ValueType::String,
];

/// What `property` can be converted to with Convert To, never its own type: for a value of a
/// built-in type, the built-in types it converts to in Tiled's order (bool, color, float, file,
/// object, int, string), then for text, the project's enums whose values it names; for an enum
/// value, the text of its names and the int of its index or flags. Class values, lists and
/// values of types the project does not have convert to nothing.
pub fn conversions(property: &Property, types: &CustomTypes) -> Vec<ValueType> {
    let candidates: Vec<ValueType> = match &property.custom_type {
        Some(name) if types.enum_type(name).is_some() => vec![ValueType::String, ValueType::Int],
        Some(_) => Vec::new(),
        None => {
            let enums = types
                .types
                .iter()
                .filter(|t| t.as_enum().is_some() && matches!(property.value, Value::String(_)))
                .map(|t| ValueType::Custom(t.name.clone()));
            CONVERT_TO.into_iter().chain(enums).collect()
        }
    };
    let own = ValueType::of(property);
    candidates
        .into_iter()
        .filter(|to| Some(to) != own.as_ref() && convert(property, to, types).is_some())
        .collect()
}

/// `property` converted to `to`, as Convert To converts it; `None` when it does not convert.
///
/// Between the built-in types the conversions are `QVariant`'s: bools to 0 and 1 or `true` and
/// `false`; numbers to bools by being non-zero and to each other (reals rounded, when they
/// fit); text to bools (all but the empty text, `0` and `false`), to numbers when it reads as
/// one, to colours when it is `#rgb`, `#rrggbb` or `#aarrggbb`, and to files as it is; colours
/// to their `#rrggbb` or `#aarrggbb` text (the empty colour to the empty text); files to their
/// text; ints to object references and back. Reals become text in their shortest form that
/// reads back the same, without an exponent. An enum value converts to the text of its names
/// (comma-separated for flags) or to the int of its index or flags, and text naming an enum's
/// values or an int within them converts to the enum, in its storage form. A value converts to
/// its own type unchanged.
pub fn convert(property: &Property, to: &ValueType, types: &CustomTypes) -> Option<Property> {
    if ValueType::of(property).as_ref() == Some(to) {
        return Some(property.clone());
    }
    if let Some(name) = &property.custom_type {
        let enumeration = types.enum_type(name)?;
        let number = enumeration.to_int(&property.value)?;
        return match to {
            ValueType::Int => Some(Property::new(Value::Int(number))),
            ValueType::String => enumeration
                .to_name(number)
                .map(|names| Property::new(Value::String(names))),
            _ => None,
        };
    }
    if let ValueType::Custom(name) = to {
        let enumeration = types.enum_type(name)?;
        return names_values(enumeration, &property.value)
            .then(|| Property::typed(enumeration.to_storage(&property.value), name.clone()));
    }
    converted_plain(&property.value, to).map(Property::new)
}

/// What a value of a built-in type typed over `current` becomes: `current`'s type when the
/// typed value converts there and back unchanged, else the value as typed. So a `7` typed over
/// an int and a text sets the int 7 and the text `"7"`, and a name typed over an enum value
/// that has it sets the enum value. Values of custom types, class values and lists are set as
/// they are.
pub(crate) fn kept_type(typed: &Property, current: &Property, types: &CustomTypes) -> Property {
    if typed.custom_type.is_some() {
        return typed.clone();
    }
    let (Some(from), Some(to)) = (ValueType::of(typed), ValueType::of(current)) else {
        return typed.clone();
    };
    let enum_target = match &to {
        ValueType::Custom(name) => types.enum_type(name).is_some(),
        _ => true,
    };
    if from == to || from == ValueType::List || to == ValueType::List || !enum_target {
        return typed.clone();
    }
    match convert(typed, &to, types) {
        Some(converted) if convert(&converted, &from, types).as_ref() == Some(typed) => converted,
        _ => typed.clone(),
    }
}

/// Whether `value` names values of `enumeration`: text of its names, or an int of an index it
/// has (or, for flags, of bits it has).
fn names_values(enumeration: &EnumType, value: &Value) -> bool {
    match value {
        Value::String(_) => enumeration.to_int(value).is_some(),
        Value::Int(number) if enumeration.values_as_flags => {
            let count = u32::try_from(enumeration.values.len().min(31)).unwrap_or(31);
            (0..(1i64 << count)).contains(number)
        }
        Value::Int(number) => enumeration.to_name(*number).is_some(),
        _ => false,
    }
}

/// A value of a built-in type converted to another built-in type, as `QVariant` converts it.
fn converted_plain(value: &Value, to: &ValueType) -> Option<Value> {
    match (value, to) {
        (Value::Bool(b), ValueType::Int) => Some(Value::Int(i64::from(*b))),
        (Value::Bool(b), ValueType::Float) => Some(Value::Float(f64::from(u8::from(*b)))),
        (Value::Bool(b), ValueType::String) => Some(Value::String(b.to_string())),
        (Value::Int(i), ValueType::Bool) => Some(Value::Bool(*i != 0)),
        // Beyond 2^53 the nearest real, as Qt converts it.
        (Value::Int(i), ValueType::Float) => Some(Value::Float(*i as f64)),
        (Value::Int(i), ValueType::Object) => {
            u32::try_from(*i).ok().map(|id| Value::Object(ObjectId(id)))
        }
        (Value::Int(i), ValueType::String) => Some(Value::String(i.to_string())),
        (Value::Float(f), ValueType::Bool) => Some(Value::Bool(*f != 0.0)),
        (Value::Float(f), ValueType::Int) => rounded(*f).map(Value::Int),
        (Value::Float(f), ValueType::String) => Some(Value::String(f.to_string())),
        (Value::String(s), ValueType::Bool) => Some(Value::Bool(text_truth(s))),
        (Value::String(s), ValueType::Color) => color_of_text(s).map(|c| Value::Color(Some(c))),
        (Value::String(s), ValueType::Float) => s
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|f| f.is_finite())
            .map(Value::Float),
        (Value::String(s), ValueType::File) => Some(Value::File(s.clone())),
        (Value::String(s), ValueType::Int) => s.trim().parse::<i64>().ok().map(Value::Int),
        (Value::Color(c), ValueType::String) => Some(Value::String(
            c.map(Color::to_tiled_text).unwrap_or_default(),
        )),
        (Value::File(f), ValueType::String) => Some(Value::String(f.clone())),
        (Value::Object(id), ValueType::Int) => Some(Value::Int(i64::from(id.0))),
        _ => None,
    }
}

/// `QVariant`'s text to bool: true unless empty, `0` or `false` in any case.
fn text_truth(text: &str) -> bool {
    !(text.is_empty() || text == "0" || text.eq_ignore_ascii_case("false"))
}

/// A real rounded to a whole number (halves away from zero, as `qRound64`), when it is finite
/// and fits.
fn rounded(real: f64) -> Option<i64> {
    const LIMIT: f64 = 9.2e18;
    let whole = real.round();
    // Within ±9.2e18 the cast cannot overflow.
    (whole.is_finite() && whole.abs() < LIMIT).then_some(whole as i64)
}

/// `QColor::fromString` for the forms Tiled writes and reads: `#rgb`, `#rrggbb` and
/// `#aarrggbb`. Colour names are not read.
fn color_of_text(text: &str) -> Option<Color> {
    let hex = text.strip_prefix('#')?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    match hex.len() {
        3 => {
            let channel = |i: usize| -> Option<u8> {
                let digit = u8::from_str_radix(hex.get(i..=i)?, 16).ok()?;
                Some(digit * 17)
            };
            Some(Color::rgb(channel(0)?, channel(1)?, channel(2)?))
        }
        6 | 8 => Color::from_tiled_text(text),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{ClassType, CustomType, EnumStorage, Properties};

    use super::*;

    fn plain(value: Value) -> Property {
        Property::new(value)
    }

    fn types() -> CustomTypes {
        let mut types = CustomTypes::new();
        types.add(CustomType::enumeration(
            "Body",
            EnumType {
                values: vec!["Static".into(), "Dynamic".into()],
                ..EnumType::default()
            },
        ));
        types.add(CustomType::enumeration(
            "Layers",
            EnumType {
                storage: EnumStorage::Int,
                values: vec!["A".into(), "B".into(), "C".into()],
                values_as_flags: true,
            },
        ));
        types.add(CustomType::class("Loot", ClassType::default()));
        types
    }

    fn converted(value: Value, to: ValueType) -> Option<Value> {
        convert(&plain(value), &to, &CustomTypes::new()).map(|p| p.value)
    }

    #[test]
    fn bools_convert_to_numbers_and_text() {
        assert_eq!(
            converted(Value::Bool(true), ValueType::Int),
            Some(Value::Int(1))
        );
        assert_eq!(
            converted(Value::Bool(false), ValueType::Float),
            Some(Value::Float(0.0))
        );
        assert_eq!(
            converted(Value::Bool(true), ValueType::String),
            Some(Value::String("true".into()))
        );
        assert_eq!(converted(Value::Bool(true), ValueType::Color), None);
        assert_eq!(converted(Value::Bool(true), ValueType::File), None);
        assert_eq!(converted(Value::Bool(true), ValueType::Object), None);
    }

    #[test]
    fn ints_convert_to_bools_reals_objects_and_text() {
        assert_eq!(
            converted(Value::Int(0), ValueType::Bool),
            Some(Value::Bool(false))
        );
        assert_eq!(
            converted(Value::Int(-3), ValueType::Bool),
            Some(Value::Bool(true))
        );
        assert_eq!(
            converted(Value::Int(7), ValueType::Float),
            Some(Value::Float(7.0))
        );
        assert_eq!(
            converted(Value::Int(7), ValueType::Object),
            Some(Value::Object(ObjectId(7)))
        );
        assert_eq!(
            converted(Value::Int(-1), ValueType::Object),
            None,
            "no object has a negative id"
        );
        assert_eq!(
            converted(Value::Int(-12), ValueType::String),
            Some(Value::String("-12".into()))
        );
        assert_eq!(converted(Value::Int(7), ValueType::Color), None);
        assert_eq!(converted(Value::Int(7), ValueType::File), None);
    }

    #[test]
    fn reals_convert_rounded_and_to_their_shortest_text() {
        assert_eq!(
            converted(Value::Float(2.5), ValueType::Int),
            Some(Value::Int(3))
        );
        assert_eq!(
            converted(Value::Float(-2.5), ValueType::Int),
            Some(Value::Int(-3)),
            "halves away from zero, as qRound64"
        );
        assert_eq!(converted(Value::Float(1e30), ValueType::Int), None);
        assert_eq!(
            converted(Value::Float(0.0), ValueType::Bool),
            Some(Value::Bool(false))
        );
        assert_eq!(
            converted(Value::Float(0.1), ValueType::String),
            Some(Value::String("0.1".into()))
        );
        assert_eq!(
            converted(Value::Float(100000.0), ValueType::String),
            Some(Value::String("100000".into()))
        );
        assert_eq!(converted(Value::Float(1.0), ValueType::Object), None);
    }

    #[test]
    fn text_converts_where_it_reads_as_the_type() {
        let text = |s: &str| Value::String(s.into());
        assert_eq!(
            converted(text(" 42 "), ValueType::Int),
            Some(Value::Int(42))
        );
        assert_eq!(converted(text("4.5"), ValueType::Int), None);
        assert_eq!(
            converted(text("4.5"), ValueType::Float),
            Some(Value::Float(4.5))
        );
        assert_eq!(converted(text("inf"), ValueType::Float), None);
        assert_eq!(converted(text("abc"), ValueType::Float), None);
        assert_eq!(
            converted(text(""), ValueType::Bool),
            Some(Value::Bool(false))
        );
        assert_eq!(
            converted(text("FALSE"), ValueType::Bool),
            Some(Value::Bool(false))
        );
        assert_eq!(
            converted(text("0"), ValueType::Bool),
            Some(Value::Bool(false))
        );
        assert_eq!(
            converted(text("no"), ValueType::Bool),
            Some(Value::Bool(true))
        );
        assert_eq!(
            converted(text("#80ff0000"), ValueType::Color),
            Some(Value::Color(Some(Color::rgba(255, 0, 0, 128))))
        );
        assert_eq!(
            converted(text("#f80"), ValueType::Color),
            Some(Value::Color(Some(Color::rgb(255, 136, 0))))
        );
        assert_eq!(converted(text("ff0000"), ValueType::Color), None);
        assert_eq!(converted(text("red"), ValueType::Color), None);
        assert_eq!(converted(text(""), ValueType::Color), None);
        assert_eq!(
            converted(text("../a.png"), ValueType::File),
            Some(Value::File("../a.png".into()))
        );
        assert_eq!(converted(text("3"), ValueType::Object), None);
    }

    #[test]
    fn colours_files_and_objects_convert_to_text_or_ints_only() {
        assert_eq!(
            converted(Value::Color(Some(Color::rgb(1, 2, 3))), ValueType::String),
            Some(Value::String("#010203".into()))
        );
        assert_eq!(
            converted(Value::Color(None), ValueType::String),
            Some(Value::String(String::new()))
        );
        assert_eq!(converted(Value::Color(None), ValueType::Int), None);
        assert_eq!(
            converted(Value::File("a.png".into()), ValueType::String),
            Some(Value::String("a.png".into()))
        );
        assert_eq!(converted(Value::File("a.png".into()), ValueType::Int), None);
        assert_eq!(
            converted(Value::Object(ObjectId(9)), ValueType::Int),
            Some(Value::Int(9))
        );
        assert_eq!(
            converted(Value::Object(ObjectId(9)), ValueType::String),
            None
        );
        assert_eq!(converted(Value::List(Vec::new()), ValueType::String), None);
        assert_eq!(
            converted(Value::Class(Properties::new()), ValueType::String),
            None
        );
    }

    #[test]
    fn conversions_list_what_converts_in_tileds_order() {
        let types = types();
        let names = |property: Property| -> Vec<String> {
            conversions(&property, &types)
                .iter()
                .map(|t| t.name().to_owned())
                .collect()
        };
        assert_eq!(
            names(plain(Value::Int(1))),
            ["bool", "float", "object", "string"]
        );
        assert_eq!(names(plain(Value::Bool(true))), ["float", "int", "string"]);
        assert_eq!(names(plain(Value::Float(1.5))), ["bool", "int", "string"]);
        assert_eq!(
            names(plain(Value::String("1".into()))),
            ["bool", "float", "file", "int"]
        );
        assert_eq!(
            names(plain(Value::String("Dynamic".into()))),
            ["bool", "file", "Body"],
            "text naming an enum's value converts to the enum"
        );
        assert_eq!(names(plain(Value::Color(None))), ["string"]);
        assert_eq!(names(plain(Value::Object(ObjectId(2)))), ["int"]);
        assert_eq!(names(plain(Value::List(Vec::new()))), Vec::<String>::new());
        assert_eq!(
            names(Property::typed(Value::Class(Properties::new()), "Loot")),
            Vec::<String>::new()
        );
        assert_eq!(
            names(Property::typed(Value::String("Dynamic".into()), "Body")),
            ["string", "int"]
        );
        assert_eq!(
            names(Property::typed(Value::String("x".into()), "Missing")),
            Vec::<String>::new()
        );
    }

    #[test]
    fn enum_values_convert_to_and_from_their_names_and_flags() {
        let types = types();
        let dynamic = Property::typed(Value::String("Dynamic".into()), "Body");
        assert_eq!(
            convert(&dynamic, &ValueType::Int, &types),
            Some(plain(Value::Int(1)))
        );
        assert_eq!(
            convert(&dynamic, &ValueType::String, &types),
            Some(plain(Value::String("Dynamic".into())))
        );
        assert_eq!(convert(&dynamic, &ValueType::Float, &types), None);
        let flags = Property::typed(Value::Int(5), "Layers");
        assert_eq!(
            convert(&flags, &ValueType::String, &types),
            Some(plain(Value::String("A,C".into())))
        );
        assert_eq!(
            convert(
                &plain(Value::String("B,C".into())),
                &ValueType::Custom("Layers".into()),
                &types
            ),
            Some(Property::typed(Value::Int(6), "Layers")),
            "into the enum's storage form"
        );
        assert_eq!(
            convert(
                &plain(Value::Int(0)),
                &ValueType::Custom("Body".into()),
                &types
            ),
            Some(Property::typed(Value::String("Static".into()), "Body"))
        );
        assert_eq!(
            convert(
                &plain(Value::Int(2)),
                &ValueType::Custom("Body".into()),
                &types
            ),
            None,
            "Body has no third value"
        );
        assert_eq!(
            convert(
                &plain(Value::Int(8)),
                &ValueType::Custom("Layers".into()),
                &types
            ),
            None,
            "Layers has no fourth flag"
        );
        assert_eq!(
            convert(
                &plain(Value::String("Static".into())),
                &ValueType::Custom("Loot".into()),
                &types
            ),
            None,
            "nothing converts to a class"
        );
    }

    #[test]
    fn typed_values_keep_each_targets_type_when_they_convert_back_unchanged() {
        let types = types();
        let seven = plain(Value::String("7".into()));
        assert_eq!(
            kept_type(&seven, &plain(Value::Int(3)), &types),
            plain(Value::Int(7))
        );
        assert_eq!(
            kept_type(&seven, &plain(Value::String("x".into())), &types),
            seven
        );
        assert_eq!(
            kept_type(
                &plain(Value::Int(7)),
                &plain(Value::String("x".into())),
                &types
            ),
            plain(Value::String("7".into()))
        );
        assert_eq!(
            kept_type(&seven, &plain(Value::Float(0.5)), &types),
            plain(Value::Float(7.0))
        );
        let leading_zero = plain(Value::String("007".into()));
        assert_eq!(
            kept_type(&leading_zero, &plain(Value::Int(3)), &types),
            leading_zero,
            "007 would come back as 7: kept as text"
        );
        assert_eq!(
            kept_type(&plain(Value::Float(7.5)), &plain(Value::Int(3)), &types),
            plain(Value::Float(7.5)),
            "7.5 would come back as 8"
        );
        assert_eq!(
            kept_type(
                &plain(Value::String("Dynamic".into())),
                &Property::typed(Value::String("Static".into()), "Body"),
                &types
            ),
            Property::typed(Value::String("Dynamic".into()), "Body")
        );
        assert_eq!(
            kept_type(
                &plain(Value::String("Moving".into())),
                &Property::typed(Value::String("Static".into()), "Body"),
                &types
            ),
            plain(Value::String("Moving".into()))
        );
        let list = plain(Value::List(Vec::new()));
        assert_eq!(kept_type(&seven, &list, &types), seven);
    }
}
