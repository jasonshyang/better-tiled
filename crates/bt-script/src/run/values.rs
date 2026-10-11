//! Property values as scripts write them (`10-agents.md`, §8.5): a value keeps the type the
//! property has, else the type the project's class gives it, else it reads as an object
//! reference (`#12`), a whole number, a number (with a `.`), `true`/`false`, or text.

use bt_core::{Color, ObjectId, Property, Value};

use super::error::RunFailure;

/// The value `text` gives the property `key`: of the type `like` has (the property as it is, or
/// the project class's member), else the type it reads as.
pub(super) fn typed_value(
    key: &str,
    text: &str,
    like: Option<&Property>,
) -> Result<Property, RunFailure> {
    let Some(like) = like else {
        return Ok(Property::new(inferred(text)));
    };
    let not = |what: &str| {
        RunFailure::refused(format!(
            "{key}=“{text}”: {key} is {what}, and “{text}” is not one"
        ))
    };
    let value = match &like.value {
        Value::Int(_) => Value::Int(text.parse().map_err(|_| not("a whole number"))?),
        Value::Float(_) => Value::Float(
            text.parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| not("a number"))?,
        ),
        Value::Bool(_) => Value::Bool(yes_or_no(text).ok_or_else(|| not("true or false"))?),
        Value::String(_) => Value::String(text.to_owned()),
        Value::File(_) => Value::File(text.to_owned()),
        Value::Object(_) => Value::Object(ObjectId(
            text.strip_prefix('#')
                .unwrap_or(text)
                .parse()
                .map_err(|_| not("an object reference (#ID)"))?,
        )),
        Value::Color(_) => Value::Color(Some(
            Color::from_tiled_text(text).ok_or_else(|| not("a colour (#rrggbb or #aarrggbb)"))?,
        )),
        Value::Class(_) | Value::List(_) => {
            return Err(RunFailure::refused(format!(
                "{key} holds {}, which a script can't write: set it in the editor",
                if matches!(like.value, Value::Class(_)) {
                    "a class value"
                } else {
                    "a list"
                }
            )));
        }
    };
    Ok(Property {
        value,
        custom_type: like.custom_type.clone(),
    })
}

/// What text reads as when nothing gives its type: `#12` an object reference, `3` a whole
/// number, `2.5` a number, `true`/`false` yes or no, anything else text.
fn inferred(text: &str) -> Value {
    if let Some(id) = text.strip_prefix('#').and_then(|id| id.parse::<u32>().ok()) {
        return Value::Object(ObjectId(id));
    }
    if let Ok(whole) = text.parse::<i64>() {
        return Value::Int(whole);
    }
    if text.contains('.')
        && let Some(number) = text.parse::<f64>().ok().filter(|v| v.is_finite())
    {
        return Value::Float(number);
    }
    match text {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => Value::String(text.to_owned()),
    }
}

/// `true` or `false` (also `yes`/`no`, `1`/`0`).
pub(super) fn yes_or_no(text: &str) -> Option<bool> {
    match text {
        "true" | "yes" | "1" => Some(true),
        "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_keep_their_type_else_read_as_what_they_look_like() {
        let read = |text: &str| typed_value("k", text, None).unwrap().value;
        assert_eq!(read("#12"), Value::Object(ObjectId(12)));
        assert_eq!(read("3"), Value::Int(3));
        assert_eq!(read("2.5"), Value::Float(2.5));
        assert_eq!(read("true"), Value::Bool(true));
        assert_eq!(read("thin ice"), Value::String("thin ice".into()));
        assert_eq!(
            read("1e3"),
            Value::String("1e3".into()),
            "a number has a dot"
        );
        let float = Property::new(Value::Float(1.0));
        assert_eq!(
            typed_value("speed", "3", Some(&float)).unwrap().value,
            Value::Float(3.0),
            "a float property stays a float"
        );
        let text = Property::typed(Value::String("a".into()), "Kind");
        let kept = typed_value("kind", "7", Some(&text)).unwrap();
        assert_eq!(kept.value, Value::String("7".into()));
        assert_eq!(kept.custom_type.as_deref(), Some("Kind"));
        let whole = Property::new(Value::Int(1));
        assert_eq!(
            typed_value("wave", "two", Some(&whole))
                .unwrap_err()
                .to_string(),
            "wave=“two”: wave is a whole number, and “two” is not one"
        );
    }
}
