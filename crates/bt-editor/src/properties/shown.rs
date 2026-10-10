//! The rows of custom properties the Properties panel shows for its targets: every property any
//! of them has, its value or that the values differ, who sets it and who inherits it; class
//! values expanded to their members (unset ones at the class's default) and lists to their
//! items (Tiled's `VariantMapProperty`, `ClassProperty` and `VariantListProperty`,
//! `reference/tiled/src/tiled/variantmapproperty.cpp`).

use std::collections::BTreeSet;

use bt_core::{CustomTypes, Properties, Property, Value};

use super::step::Step;
use super::target::{Origin, Target};

/// A row of custom properties: a property, a member of a class value or an item of a list, as
/// the targets have it.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    /// Where it is: the property's name, then members and items.
    pub path: Vec<Step>,
    /// The value of the first target that has it (its own, inherited, or for a member its value
    /// leaves unset, the class's default), as it is kept: a class value holds the members it
    /// sets, and its member rows show the rest at their defaults. What Copy takes.
    pub value: Property,
    /// Whether the targets' values differ, with unset members counted at their defaults, or
    /// some lack it.
    pub mixed: bool,
    /// The targets, by their index, that set it themselves: as one of their own properties, or
    /// as a member set in or an item of one of their own values.
    pub holders: Vec<usize>,
    /// Where the value of the targets that inherit it comes from (the first such target's).
    pub inherited_from: Option<Origin>,
    /// A member no target sets: its class's default shows.
    pub is_default: bool,
    /// Whether Reset applies: for a property, some target's own value hides an inherited one;
    /// for a member, some target's value sets it.
    pub resettable: bool,
    /// The members of a class value or the items of a list, when the targets' values are all
    /// of one class or all lists.
    pub children: Vec<Shown>,
}

/// How a target has what a row shows.
#[derive(Copy, Clone, Debug)]
enum Holding<'s> {
    /// It sets it itself.
    Own,
    /// It inherits it.
    Inherited(&'s Origin),
    /// A member its value leaves unset: the class's default.
    Default,
}

/// A target's value at a row, and how it has it.
#[derive(Copy, Clone, Debug)]
struct Slot<'s> {
    value: &'s Property,
    holding: Holding<'s>,
}

/// The rows for `targets`: every property any of them has, own or inherited, by name.
pub(crate) fn rows<'s>(targets: &'s [Target<'s>], types: &'s CustomTypes) -> Vec<Shown> {
    let names: BTreeSet<&String> = targets.iter().flat_map(Target::names).collect();
    names
        .into_iter()
        .filter_map(|name| {
            let slots: Vec<Option<Slot<'s>>> = targets
                .iter()
                .map(|target| top_slot(target, name))
                .collect();
            let resettable = targets.iter().any(|target| {
                target.own_in(target.stored(), name).is_some()
                    && target.inherited_value(name).is_some()
            });
            row(vec![Step::Member(name.clone())], &slots, types, resettable)
        })
        .collect()
}

/// A target's value of the property `name`: its own, else what it inherits.
fn top_slot<'s>(target: &'s Target<'s>, name: &str) -> Option<Slot<'s>> {
    if let Some(value) = target.own_in(target.stored(), name) {
        return Some(Slot {
            value,
            holding: Holding::Own,
        });
    }
    target.inherited_value(name).map(|found| Slot {
        value: found.property,
        holding: Holding::Inherited(found.origin),
    })
}

/// The row at `path` for the targets' values there; `None` when no target has one.
fn row(
    path: Vec<Step>,
    slots: &[Option<Slot<'_>>],
    types: &CustomTypes,
    resettable: bool,
) -> Option<Shown> {
    let first = slots.iter().flatten().next()?;
    let completed = types.resolve_value(first.value);
    let mixed = slots.iter().any(|slot| match slot {
        None => true,
        Some(slot) => slot.value != first.value && types.resolve_value(slot.value) != completed,
    });
    let value = first.value.clone();
    let holders = slots
        .iter()
        .enumerate()
        .filter(|(_, slot)| {
            matches!(
                slot,
                Some(Slot {
                    holding: Holding::Own,
                    ..
                })
            )
        })
        .map(|(index, _)| index)
        .collect();
    let inherited_from = slots.iter().flatten().find_map(|slot| match slot.holding {
        Holding::Inherited(origin) => Some(origin.clone()),
        _ => None,
    });
    let is_default = slots
        .iter()
        .flatten()
        .all(|slot| matches!(slot.holding, Holding::Default));
    let children = children(&path, slots, types);
    Some(Shown {
        path,
        value,
        mixed,
        holders,
        inherited_from,
        is_default,
        resettable,
        children,
    })
}

/// The rows under a row: members when every value there is a class value of one class, items
/// when every value is a list.
fn children(path: &[Step], slots: &[Option<Slot<'_>>], types: &CustomTypes) -> Vec<Shown> {
    let mut present = slots.iter().flatten();
    let Some(first) = present.next() else {
        return Vec::new();
    };
    let within = |step: Step| {
        let mut path = path.to_vec();
        path.push(step);
        path
    };
    match &first.value.value {
        Value::Class(_) => {
            let class_name = &first.value.custom_type;
            if !present.clone().all(|slot| {
                matches!(slot.value.value, Value::Class(_)) && &slot.value.custom_type == class_name
            }) {
                return Vec::new();
            }
            let definition = class_name
                .as_deref()
                .and_then(|name| types.value_class(name))
                .map(|class| &class.members);
            let mut names: BTreeSet<&String> = definition
                .map(|members| members.keys().collect())
                .unwrap_or_default();
            for slot in slots.iter().flatten() {
                if let Value::Class(members) = &slot.value.value {
                    names.extend(members.keys());
                }
            }
            names
                .into_iter()
                .filter_map(|name| {
                    let member_slots: Vec<Option<Slot<'_>>> = slots
                        .iter()
                        .map(|slot| slot.and_then(|slot| member_slot(slot, name, definition)))
                        .collect();
                    let resettable = member_slots
                        .iter()
                        .flatten()
                        .any(|slot| !matches!(slot.holding, Holding::Default));
                    row(
                        within(Step::Member(name.clone())),
                        &member_slots,
                        types,
                        resettable,
                    )
                })
                .collect()
        }
        Value::List(_) => {
            if !present
                .clone()
                .all(|slot| matches!(slot.value.value, Value::List(_)))
            {
                return Vec::new();
            }
            let longest = slots
                .iter()
                .flatten()
                .map(|slot| match &slot.value.value {
                    Value::List(items) => items.len(),
                    _ => 0,
                })
                .max()
                .unwrap_or(0);
            (0..longest)
                .filter_map(|index| {
                    let item_slots: Vec<Option<Slot<'_>>> = slots
                        .iter()
                        .map(|slot| slot.and_then(|slot| item_slot(slot, index)))
                        .collect();
                    row(within(Step::Item(index)), &item_slots, types, false)
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

/// A class value's member `name`: as the value sets it, else the class's default.
fn member_slot<'s>(
    slot: Slot<'s>,
    name: &str,
    definition: Option<&'s Properties>,
) -> Option<Slot<'s>> {
    let Value::Class(members) = &slot.value.value else {
        return None;
    };
    if let Some(value) = members.get(name) {
        return Some(Slot {
            value,
            holding: slot.holding,
        });
    }
    definition?.get(name).map(|value| Slot {
        value,
        holding: Holding::Default,
    })
}

/// A list's item at `index`, if it has one.
fn item_slot(slot: Slot<'_>, index: usize) -> Option<Slot<'_>> {
    let Value::List(items) = &slot.value.value else {
        return None;
    };
    items.get(index).map(|value| Slot {
        value,
        holding: slot.holding,
    })
}
