//! Built-in attributes as the Properties panel shows them for several targets of one kind, and
//! which of the targets a value set applies to.

use super::attribute::Attribute;
use super::attribute_value::AttributeValue;
use super::error::PropertyError;

/// A built-in attribute as the Properties panel shows it for its targets.
#[derive(Clone, Debug, PartialEq)]
pub struct AttributeRow {
    /// The attribute.
    pub attribute: Attribute,
    /// The value of the first target that has it.
    pub value: AttributeValue,
    /// Whether the targets that have it differ.
    pub mixed: bool,
    /// Whether it cannot be set: an id, a map's size, or one that does not apply in the
    /// targets' state (the hex side length of an orthogonal map).
    pub read_only: bool,
    /// The targets, by their index, that have it (an image layer's image, among layers).
    pub holders: Vec<usize>,
    /// Whether every target that has it is a template instance that takes it from its
    /// template: setting it makes the instances override it.
    pub from_template: bool,
}

/// One target's attribute.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Reading {
    /// Its value.
    pub(crate) value: AttributeValue,
    /// Whether it cannot be set.
    pub(crate) read_only: bool,
    /// Whether it comes from the target's template.
    pub(crate) from_template: bool,
}

impl Reading {
    /// A value that can be set.
    pub(crate) fn editable(value: AttributeValue) -> Reading {
        Reading {
            value,
            read_only: false,
            from_template: false,
        }
    }

    /// A value that cannot be set.
    pub(crate) fn fixed(value: AttributeValue) -> Reading {
        Reading {
            value,
            read_only: true,
            from_template: false,
        }
    }

    /// A value that can be set when `editable`.
    pub(crate) fn editable_if(value: AttributeValue, editable: bool) -> Reading {
        Reading {
            value,
            read_only: !editable,
            from_template: false,
        }
    }
}

/// The rows of `targets` in `order`: each attribute some target has.
pub(crate) fn rows_of<T>(
    order: &[Attribute],
    targets: &[T],
    read: impl Fn(&T, Attribute) -> Option<Reading>,
) -> Vec<AttributeRow> {
    order
        .iter()
        .filter_map(|&attribute| {
            let readings: Vec<Option<Reading>> = targets
                .iter()
                .map(|target| read(target, attribute))
                .collect();
            let first = readings.iter().flatten().next()?;
            let holders = readings
                .iter()
                .enumerate()
                .filter(|(_, reading)| reading.is_some())
                .map(|(index, _)| index)
                .collect();
            Some(AttributeRow {
                attribute,
                value: first.value.clone(),
                mixed: readings
                    .iter()
                    .flatten()
                    .any(|reading| reading.value != first.value),
                read_only: readings.iter().flatten().all(|reading| reading.read_only),
                holders,
                from_template: readings
                    .iter()
                    .flatten()
                    .all(|reading| reading.from_template),
            })
        })
        .collect()
}

/// Which of `targets` setting `attribute` changes: those that have it and can set it. Refused
/// when none has it, or none of those that have it can set it.
pub(crate) fn settable<T: Copy>(
    attribute: Attribute,
    targets: &[T],
    read: impl Fn(T, Attribute) -> Option<Reading>,
) -> Result<Vec<T>, PropertyError> {
    let mut found = false;
    let mut settable = Vec::new();
    for &target in targets {
        let Some(reading) = read(target, attribute) else {
            continue;
        };
        found = true;
        if !reading.read_only {
            settable.push(target);
        }
    }
    if !found {
        return Err(PropertyError::NotApplicable(attribute));
    }
    if settable.is_empty() {
        return Err(PropertyError::ReadOnly(attribute));
    }
    Ok(settable)
}

/// Sets `field` to `value`; whether it changed.
pub(crate) fn assign<T: PartialEq>(field: &mut T, value: T) -> bool {
    if *field == value {
        return false;
    }
    *field = value;
    true
}
