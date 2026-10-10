//! Changes of custom properties: setting a value anywhere in a property (a member of a class
//! value, an item of a list), resetting, editing lists, and the whole-property operations Add,
//! Remove, Rename, Convert To and Paste. A change applies to each target's own properties
//! separately (Tiled's `SetProperty`, `RemoveProperty`, `RenameProperty` and the paste and
//! convert commands, `reference/tiled/src/tiled/changeproperties.cpp` and
//! `propertieswidget.cpp`, with libtiled's `setPropertyMemberValue`).

use bt_core::{CustomTypes, Properties, Property, Value};

use super::convert::{convert, kept_type};
use super::error::PropertyError;
use super::step::{Step, path_text};
use super::target::Target;
use super::value_type::ValueType;

/// An edit of a list's items, by index: every target's list edits its own item at that index
/// (Tiled addresses list items by index too).
#[derive(Clone, Debug, PartialEq)]
pub enum ListEdit {
    /// Adds an item at the end.
    Push(Property),
    /// Removes the item at an index.
    Remove(usize),
    /// Moves the item at `from` so that it ends up at `to`.
    Move {
        /// Where the item is.
        from: usize,
        /// Where it goes.
        to: usize,
    },
    /// Puts a copy of the item at an index right after it.
    Duplicate(usize),
}

/// What a change does at its path.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// Sets the value there. A value of a built-in type keeps each target's type where it
    /// converts there and back unchanged (the keep-type rule); enum values are stored in their
    /// enum's form. A target without the property gets it.
    Set(Property),
    /// For a property, removes each target's own value where it hides an inherited one (for a
    /// template instance, puts the template's back), so the inherited value shows again. For a
    /// member of a class value, unsets it, so its class's default shows.
    Reset,
    /// Edits the list there.
    List(ListEdit),
    /// Adds a property of this type at its default value to the targets that have none.
    Add(ValueType),
    /// Removes the targets' own values of the property; inherited values stay.
    Remove,
    /// Renames the targets' own values of the property.
    Rename(String),
    /// Converts the targets' own values of the property to another type, where they convert.
    Convert(ValueType),
    /// Sets these properties, copied from another thing, on every target.
    Paste(Properties),
}

/// A change of custom properties: what to do, and where. The path starts with the property's
/// name and walks into class members and list items; the whole-property operations (Add,
/// Remove, Rename, Convert) take the name alone, and a paste takes no path.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    /// Where: the property's name, then members and items.
    pub path: Vec<Step>,
    /// What.
    pub op: Op,
}

impl Change {
    /// Sets the value at `path`.
    pub fn set(path: Vec<Step>, value: Property) -> Change {
        Change {
            path,
            op: Op::Set(value),
        }
    }

    /// Resets the property or member at `path`.
    pub fn reset(path: Vec<Step>) -> Change {
        Change {
            path,
            op: Op::Reset,
        }
    }

    /// Edits the list at `path`.
    pub fn list(path: Vec<Step>, edit: ListEdit) -> Change {
        Change {
            path,
            op: Op::List(edit),
        }
    }

    /// Adds the property `name` of `value_type`.
    pub fn add(name: impl Into<String>, value_type: ValueType) -> Change {
        Change {
            path: vec![Step::Member(name.into())],
            op: Op::Add(value_type),
        }
    }

    /// Removes the property `name`.
    pub fn remove(name: impl Into<String>) -> Change {
        Change {
            path: vec![Step::Member(name.into())],
            op: Op::Remove,
        }
    }

    /// Renames the property `from` to `to`.
    pub fn rename(from: impl Into<String>, to: impl Into<String>) -> Change {
        Change {
            path: vec![Step::Member(from.into())],
            op: Op::Rename(to.into()),
        }
    }

    /// Converts the property `name` to `to`.
    pub fn convert(name: impl Into<String>, to: ValueType) -> Change {
        Change {
            path: vec![Step::Member(name.into())],
            op: Op::Convert(to),
        }
    }

    /// Pastes `properties`.
    pub fn paste(properties: Properties) -> Change {
        Change {
            path: Vec::new(),
            op: Op::Paste(properties),
        }
    }

    /// What the change is called in the History panel and the Edit menu, as Tiled's commands
    /// call it.
    pub fn label(&self) -> String {
        let path = path_text(&self.path);
        match &self.op {
            Op::Set(_) | Op::List(_) => format!("Set Property '{path}'"),
            Op::Reset => format!("Reset Property '{path}'"),
            Op::Add(_) => format!("Add Property '{path}'"),
            Op::Remove => format!("Remove Property '{path}'"),
            Op::Rename(_) => "Rename Property".to_owned(),
            Op::Convert(_) => "Convert Property".to_owned(),
            Op::Paste(properties) if properties.len() == 1 => "Paste Property".to_owned(),
            Op::Paste(_) => "Paste Properties".to_owned(),
        }
    }

    /// The operation as its command names it, for refusals.
    fn operation(&self) -> &'static str {
        match &self.op {
            Op::Set(_) => "Set",
            Op::Reset => "Reset",
            Op::List(_) => "a list edit",
            Op::Add(_) => "Add",
            Op::Remove => "Remove",
            Op::Rename(_) => "Rename",
            Op::Convert(_) => "Convert",
            Op::Paste(_) => "Paste",
        }
    }
}

/// A change checked against its targets, ready to apply to each of them.
pub(crate) struct Plan<'c> {
    types: &'c CustomTypes,
    action: Action<'c>,
}

/// What a plan does to each target.
enum Action<'c> {
    /// Sets the value at `rest` in the property `name`.
    Set {
        name: &'c str,
        rest: &'c [Step],
        value: &'c Property,
        /// What a target without the property starts from: an empty value of the type the
        /// others have, for edits inside it.
        start: Option<Property>,
    },
    /// Resets the property `name`, or the member at `rest` in it.
    Reset { name: &'c str, rest: &'c [Step] },
    /// Edits the list at `rest` in the property `name`.
    List {
        name: &'c str,
        rest: &'c [Step],
        edit: &'c ListEdit,
        start: Option<Property>,
    },
    /// Adds the property `name` with `value`.
    Add { name: String, value: Property },
    /// Removes the property `name`.
    Remove { name: &'c str },
    /// Renames the property `from`.
    Rename { from: &'c str, to: String },
    /// Converts the property `name`.
    Convert { name: &'c str, to: &'c ValueType },
    /// Sets the properties.
    Paste(&'c Properties),
}

impl<'c> Plan<'c> {
    /// The plan for `change` on `targets`: refused when the path does not suit the operation,
    /// a name is empty, a type is unknown, or a rename would replace a target's property.
    pub(crate) fn new(
        change: &'c Change,
        targets: &[Target<'_>],
        types: &'c CustomTypes,
    ) -> Result<Plan<'c>, PropertyError> {
        let mismatch = || PropertyError::PathMismatch {
            operation: change.operation(),
            path: path_text(&change.path),
        };
        if let Op::Paste(properties) = &change.op {
            if !change.path.is_empty() {
                return Err(mismatch());
            }
            return Ok(Plan {
                types,
                action: Action::Paste(properties),
            });
        }
        let Some((Step::Member(name), rest)) = change.path.split_first() else {
            return Err(mismatch());
        };
        let whole = |action: Action<'c>| {
            if rest.is_empty() {
                Ok(action)
            } else {
                Err(mismatch())
            }
        };
        let action = match &change.op {
            Op::Set(value) => Action::Set {
                name,
                rest,
                value,
                start: start_value(targets, name),
            },
            Op::Reset => {
                if matches!(rest.last(), Some(Step::Item(_))) {
                    return Err(mismatch());
                }
                Action::Reset { name, rest }
            }
            Op::List(edit) => Action::List {
                name,
                rest,
                edit,
                start: start_value(targets, name),
            },
            Op::Add(value_type) => {
                let trimmed = name.trim();
                if trimmed.is_empty() {
                    return Err(PropertyError::EmptyName);
                }
                whole(Action::Add {
                    name: trimmed.to_owned(),
                    value: value_type.default_value(types)?,
                })?
            }
            Op::Remove => whole(Action::Remove { name })?,
            Op::Rename(to) => {
                let to = to.trim();
                if to.is_empty() {
                    return Err(PropertyError::EmptyName);
                }
                let taken = to != name
                    && targets.iter().any(|target| {
                        target.own_in(target.stored(), name).is_some()
                            && target.own_in(target.stored(), to).is_some()
                    });
                if taken {
                    return Err(PropertyError::NameTaken(to.to_owned()));
                }
                whole(Action::Rename {
                    from: name,
                    to: to.to_owned(),
                })?
            }
            Op::Convert(to) => whole(Action::Convert { name, to })?,
            Op::Paste(_) => return Err(mismatch()),
        };
        Ok(Plan { types, action })
    }

    /// The properties `target` keeps after the change, from `stored` (what it keeps now); `None`
    /// when the change leaves them as they are (a target without the value converted, an item
    /// its list does not have).
    pub(crate) fn applied_to(
        &self,
        target: &Target<'_>,
        stored: &Properties,
    ) -> Option<Properties> {
        let types = self.types;
        let current = |name: &str| -> Option<Property> {
            target
                .own_in(stored, name)
                .or_else(|| target.inherited_value(name).map(|found| found.property))
                .cloned()
        };
        match &self.action {
            Action::Set {
                name,
                rest,
                value,
                start,
            } => {
                let mut top = match (current(name), rest.is_empty()) {
                    (Some(current), true) => kept_type(value, &current, types),
                    (None, true) => (*value).clone(),
                    (Some(current), false) => current,
                    (None, false) => start.clone()?,
                };
                if !rest.is_empty() && !set_at(&mut top, rest, value, types) {
                    return None;
                }
                types.normalize(&mut top);
                with_value(stored, name, top)
            }
            Action::Reset { name, rest } => {
                if rest.is_empty() {
                    target.own_in(stored, name)?;
                    return match target.template_value(name) {
                        Some(template) => with_value(stored, name, template.clone()),
                        None if target.inherited_value(name).is_some() => without(stored, name),
                        None => None,
                    };
                }
                let mut top = current(name)?;
                unset_at(&mut top, rest).then(|| with_value(stored, name, top))?
            }
            Action::List {
                name,
                rest,
                edit,
                start,
            } => {
                let mut top = current(name).or_else(|| start.clone())?;
                let Value::List(items) = &mut place_at(&mut top, rest, types)?.value else {
                    return None;
                };
                edited_list(items, edit, types).then(|| with_value(stored, name, top))?
            }
            Action::Add { name, value } => {
                if current(name).is_some() {
                    return None;
                }
                with_value(stored, name, value.clone())
            }
            Action::Remove { name } => {
                target.own_in(stored, name)?;
                match target.template_value(name) {
                    Some(template) => with_value(stored, name, template.clone()),
                    None => without(stored, name),
                }
            }
            Action::Rename { from, to } => {
                if from == to {
                    return None;
                }
                let value = target.own_in(stored, from)?.clone();
                let mut renamed = stored.clone();
                match target.template_value(from) {
                    Some(template) => renamed.insert((*from).to_owned(), template.clone()),
                    None => renamed.remove(*from),
                };
                renamed.insert(to.clone(), value);
                (renamed != *stored).then_some(renamed)
            }
            Action::Convert { name, to } => {
                let own = target.own_in(stored, name)?;
                with_value(stored, name, convert(own, to, types)?)
            }
            Action::Paste(properties) => {
                let mut pasted = stored.clone();
                pasted.extend(
                    properties
                        .iter()
                        .map(|(name, property)| (name.clone(), property.clone())),
                );
                (pasted != *stored).then_some(pasted)
            }
        }
    }
}

/// What a target without the property `name` starts from for edits inside it: an empty class
/// value of the class the first target that has it has, or an empty list.
fn start_value(targets: &[Target<'_>], name: &str) -> Option<Property> {
    let first = targets.iter().find_map(|target| {
        target
            .own_in(target.stored(), name)
            .or_else(|| target.inherited_value(name).map(|found| found.property))
    })?;
    let value = match &first.value {
        Value::Class(_) => Value::Class(Properties::new()),
        Value::List(_) => Value::List(Vec::new()),
        _ => return None,
    };
    Some(Property {
        value,
        custom_type: first.custom_type.clone(),
    })
}

/// `stored` with `name` set to `value`; `None` when it has that value already.
fn with_value(stored: &Properties, name: &str, value: Property) -> Option<Properties> {
    if stored.get(name) == Some(&value) {
        return None;
    }
    let mut changed = stored.clone();
    changed.insert(name.to_owned(), value);
    Some(changed)
}

/// `stored` without `name`; `None` when it has none.
fn without(stored: &Properties, name: &str) -> Option<Properties> {
    stored.get(name)?;
    let mut changed = stored.clone();
    changed.remove(name);
    Some(changed)
}

/// The default of the member `name` of a class value, by its class.
fn member_default(value: &Property, name: &str, types: &CustomTypes) -> Option<Property> {
    let class = types.value_class(value.custom_type.as_deref()?)?;
    class.members.get(name).cloned()
}

/// Sets `new` at `path` in `value` (keeping the type of what is there, by the keep-type rule);
/// members not set yet start from their class's default, and a member the class does not have
/// is set as it is. Whether the path fits the value.
fn set_at(value: &mut Property, path: &[Step], new: &Property, types: &CustomTypes) -> bool {
    let Some((step, rest)) = path.split_first() else {
        *value = kept_type(new, value, types);
        return true;
    };
    match step {
        Step::Member(name) => {
            let default = member_default(value, name, types);
            let Value::Class(members) = &mut value.value else {
                return false;
            };
            if let Some(member) = members.get_mut(name) {
                return set_at(member, rest, new, types);
            }
            let member = match default {
                Some(mut member) => {
                    if !set_at(&mut member, rest, new, types) {
                        return false;
                    }
                    member
                }
                None if rest.is_empty() => new.clone(),
                None => return false,
            };
            members.insert(name.clone(), member);
            true
        }
        Step::Item(index) => {
            let Value::List(items) = &mut value.value else {
                return false;
            };
            items
                .get_mut(*index)
                .is_some_and(|item| set_at(item, rest, new, types))
        }
    }
}

/// The value at `path` in `value`, to change it; members not set yet are set to their class's
/// default first. `None` when the path does not fit.
fn place_at<'v>(
    value: &'v mut Property,
    path: &[Step],
    types: &CustomTypes,
) -> Option<&'v mut Property> {
    let Some((step, rest)) = path.split_first() else {
        return Some(value);
    };
    let next = match step {
        Step::Member(name) => {
            let default = member_default(value, name, types);
            let Value::Class(members) = &mut value.value else {
                return None;
            };
            if !members.contains_key(name) {
                members.insert(name.clone(), default?);
            }
            members.get_mut(name)?
        }
        Step::Item(index) => {
            let Value::List(items) = &mut value.value else {
                return None;
            };
            items.get_mut(*index)?
        }
    };
    place_at(next, rest, types)
}

/// Unsets the member at the end of `path` in `value`, so its class's default shows; a class
/// value inside another that is left with no member set is unset too (libtiled's
/// `setClassPropertyValue`). Whether anything was unset.
fn unset_at(value: &mut Property, path: &[Step]) -> bool {
    match (path, &mut value.value) {
        ([Step::Member(name)], Value::Class(members)) => members.remove(name).is_some(),
        ([Step::Member(name), rest @ ..], Value::Class(members)) => {
            let Some(member) = members.get_mut(name) else {
                return false;
            };
            let unset = unset_at(member, rest);
            if unset && matches!(&member.value, Value::Class(inner) if inner.is_empty()) {
                members.remove(name);
            }
            unset
        }
        ([Step::Item(index), rest @ ..], Value::List(items)) if !rest.is_empty() => items
            .get_mut(*index)
            .is_some_and(|item| unset_at(item, rest)),
        _ => false,
    }
}

/// Applies a list edit; whether the list changed (an index it does not have changes nothing).
fn edited_list(items: &mut Vec<Property>, edit: &ListEdit, types: &CustomTypes) -> bool {
    match edit {
        ListEdit::Push(item) => {
            let mut item = item.clone();
            types.normalize(&mut item);
            items.push(item);
            true
        }
        ListEdit::Remove(index) => {
            if *index >= items.len() {
                return false;
            }
            items.remove(*index);
            true
        }
        ListEdit::Move { from, to } => {
            if from == to || *from >= items.len() || *to >= items.len() {
                return false;
            }
            let item = items.remove(*from);
            items.insert(*to, item);
            true
        }
        ListEdit::Duplicate(index) => match items.get(*index).cloned() {
            Some(copy) => {
                items.insert(*index + 1, copy);
                true
            }
            None => false,
        },
    }
}
