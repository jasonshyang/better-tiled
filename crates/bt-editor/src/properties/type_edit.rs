//! Changes of the project's custom types, as Tiled's Custom Types editor makes them
//! (`reference/tiled/src/tiled/propertytypeseditor.cpp` and `propertytypesmodel.cpp`): enums
//! and classes added, renamed and removed; an enum's values, how they are stored and whether
//! they are flags; a class's colour, fill, what it may be the class of, and its members, changed
//! as custom properties are. Values that use a type keep its name when it is renamed or removed,
//! as in Tiled.

use bt_core::{
    ClassType, ClassUsage, Color, CustomType, CustomTypeId, CustomTypeKind, CustomTypes,
    EnumStorage, EnumType,
};

use super::change::Change;
use super::error::PropertyError;
use super::set::PropertySet;
use super::target::Target;

/// The most values an enum of flags holds, one bit each.
const MAX_FLAGS: usize = 31;

/// A change of the project's custom types.
#[derive(Clone, Debug, PartialEq)]
pub enum TypeEdit {
    /// Adds an enum with no values, named "Enum" and the first free number from the count of
    /// enums (Tiled's `nextPropertyTypeName`).
    AddEnum,
    /// Adds a class with no members, named "Class" and the first free number.
    AddClass,
    /// Removes a type.
    Remove(CustomTypeId),
    /// Renames a type; another type may not have the name.
    Rename {
        /// The type.
        id: CustomTypeId,
        /// Its new name.
        name: String,
    },
    /// How an enum's values are stored in files.
    SetStorage {
        /// The enum.
        id: CustomTypeId,
        /// As names or as numbers.
        storage: EnumStorage,
    },
    /// Whether several of an enum's values can be set at once (at most 31 values).
    SetValuesAsFlags {
        /// The enum.
        id: CustomTypeId,
        /// Flags or not.
        flags: bool,
    },
    /// Adds a value to an enum, named after the enum and the first free number (Tiled's
    /// `nextValueText`).
    AddValue(CustomTypeId),
    /// Renames an enum's value.
    RenameValue {
        /// The enum.
        id: CustomTypeId,
        /// The value's place.
        index: usize,
        /// Its new name.
        name: String,
    },
    /// Removes an enum's value.
    RemoveValue {
        /// The enum.
        id: CustomTypeId,
        /// The value's place.
        index: usize,
    },
    /// The colour objects of a class are drawn in.
    SetColor {
        /// The class.
        id: CustomTypeId,
        /// The colour.
        color: Color,
    },
    /// Whether shapes of objects of a class are filled.
    SetDrawFill {
        /// The class.
        id: CustomTypeId,
        /// Filled or not.
        fill: bool,
    },
    /// What a class may be the class of (Tiled's "Use as").
    SetUseAs {
        /// The class.
        id: CustomTypeId,
        /// The usages.
        usage: ClassUsage,
    },
    /// Changes a class's members and their defaults, as custom properties are changed; a member
    /// may not make its class contain itself.
    Members {
        /// The class.
        id: CustomTypeId,
        /// The change.
        change: Change,
    },
}

impl TypeEdit {
    /// What the change is called in the History panel and the Edit menu.
    pub fn label(&self) -> String {
        match self {
            TypeEdit::AddEnum => "Add Enum".to_owned(),
            TypeEdit::AddClass => "Add Class".to_owned(),
            TypeEdit::Remove(_) => "Remove Type".to_owned(),
            TypeEdit::Rename { .. } => "Rename Type".to_owned(),
            TypeEdit::SetStorage { .. }
            | TypeEdit::SetValuesAsFlags { .. }
            | TypeEdit::AddValue(_)
            | TypeEdit::RenameValue { .. }
            | TypeEdit::RemoveValue { .. } => "Change Enum".to_owned(),
            TypeEdit::SetColor { .. }
            | TypeEdit::SetDrawFill { .. }
            | TypeEdit::SetUseAs { .. } => "Change Class".to_owned(),
            TypeEdit::Members { change, .. } => change.member_label(),
        }
    }

    /// Applies the change to `types`; whether they changed. Refused, changing nothing, when the
    /// type is missing or of the other kind, a name is empty or taken, an enum of flags would
    /// hold too many values, or a member would make its class contain itself.
    pub fn apply_to(&self, types: &mut CustomTypes) -> Result<bool, PropertyError> {
        match self {
            TypeEdit::AddEnum => {
                let name = next_type_name(types, "Enum", |t| t.as_enum().is_some());
                types.add(CustomType::enumeration(name, EnumType::default()));
                Ok(true)
            }
            TypeEdit::AddClass => {
                let name = next_type_name(types, "Class", |t| t.as_class().is_some());
                types.add(CustomType::class(name, ClassType::default()));
                Ok(true)
            }
            TypeEdit::Remove(id) => {
                let before = types.types.len();
                types.types.retain(|t| t.id != *id);
                if types.types.len() == before {
                    return Err(PropertyError::NoSuchType(*id));
                }
                Ok(true)
            }
            TypeEdit::Rename { id, name } => rename_type(types, *id, name),
            TypeEdit::SetStorage { id, storage } => {
                let enumeration = enum_mut(types, *id)?;
                Ok(assign(&mut enumeration.storage, *storage))
            }
            TypeEdit::SetValuesAsFlags { id, flags } => {
                let enumeration = enum_mut(types, *id)?;
                if *flags && enumeration.values.len() > MAX_FLAGS {
                    return Err(PropertyError::TooManyFlags);
                }
                Ok(assign(&mut enumeration.values_as_flags, *flags))
            }
            TypeEdit::AddValue(id) => {
                let name = type_of(types, *id)?.name.clone();
                let enumeration = enum_mut(types, *id)?;
                if enumeration.values_as_flags && enumeration.values.len() >= MAX_FLAGS {
                    return Err(PropertyError::TooManyFlags);
                }
                let value = next_value_name(enumeration, &name);
                enumeration.values.push(value);
                Ok(true)
            }
            TypeEdit::RenameValue { id, index, name } => {
                let enumeration = enum_mut(types, *id)?;
                let name = name.trim();
                if name.is_empty() {
                    return Err(PropertyError::EmptyValueName);
                }
                let taken = enumeration
                    .values
                    .iter()
                    .enumerate()
                    .any(|(other, value)| other != *index && value == name);
                if taken {
                    return Err(PropertyError::ValueNameTaken(name.to_owned()));
                }
                let value = enumeration
                    .values
                    .get_mut(*index)
                    .ok_or(PropertyError::NoSuchValue(*index))?;
                Ok(assign(value, name.to_owned()))
            }
            TypeEdit::RemoveValue { id, index } => {
                let enumeration = enum_mut(types, *id)?;
                if *index >= enumeration.values.len() {
                    return Err(PropertyError::NoSuchValue(*index));
                }
                enumeration.values.remove(*index);
                Ok(true)
            }
            TypeEdit::SetColor { id, color } => {
                Ok(assign(&mut class_mut(types, *id)?.color, *color))
            }
            TypeEdit::SetDrawFill { id, fill } => {
                Ok(assign(&mut class_mut(types, *id)?.draw_fill, *fill))
            }
            TypeEdit::SetUseAs { id, usage } => {
                Ok(assign(&mut class_mut(types, *id)?.use_as, *usage))
            }
            TypeEdit::Members { id, change } => change_members(types, *id, change),
        }
    }
}

/// The type with this id.
fn type_of(types: &CustomTypes, id: CustomTypeId) -> Result<&CustomType, PropertyError> {
    types.find_by_id(id).ok_or(PropertyError::NoSuchType(id))
}

/// The enum with this id, to change.
fn enum_mut(types: &mut CustomTypes, id: CustomTypeId) -> Result<&mut EnumType, PropertyError> {
    let found = types
        .types
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or(PropertyError::NoSuchType(id))?;
    match &mut found.kind {
        CustomTypeKind::Enum(enumeration) => Ok(enumeration),
        CustomTypeKind::Class(_) => Err(PropertyError::NotAnEnum(found.name.clone())),
    }
}

/// The class with this id, to change.
fn class_mut(types: &mut CustomTypes, id: CustomTypeId) -> Result<&mut ClassType, PropertyError> {
    let found = types
        .types
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or(PropertyError::NoSuchType(id))?;
    match &mut found.kind {
        CustomTypeKind::Class(class) => Ok(class),
        CustomTypeKind::Enum(_) => Err(PropertyError::NotAClass(found.name.clone())),
    }
}

/// Renames a type to `name` (trimmed), which no other type may have.
fn rename_type(
    types: &mut CustomTypes,
    id: CustomTypeId,
    name: &str,
) -> Result<bool, PropertyError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(PropertyError::EmptyTypeName);
    }
    if types.types.iter().any(|t| t.id != id && t.name == name) {
        return Err(PropertyError::TypeNameTaken(name.to_owned()));
    }
    let found = types
        .types
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or(PropertyError::NoSuchType(id))?;
    Ok(assign(&mut found.name, name.to_owned()))
}

/// Changes a class's members as custom properties are changed, refusing a member that makes
/// the class contain itself.
fn change_members(
    types: &mut CustomTypes,
    id: CustomTypeId,
    change: &Change,
) -> Result<bool, PropertyError> {
    let original = types.clone();
    let set = PropertySet::of(vec![Target::members(&original, id)?], &original);
    let mut changed = types.clone();
    if !set.apply_to_types(&mut changed, change)? {
        return Ok(false);
    }
    let circular_before = original.circular_members();
    if let Some(circular) = changed
        .circular_members()
        .into_iter()
        .find(|circular| !circular_before.contains(circular))
    {
        return Err(PropertyError::Circular(circular));
    }
    *types = changed;
    Ok(true)
}

/// `base` and the first number, from the count of types of its kind, that no type's name has
/// (Tiled's `nextPropertyTypeName`).
fn next_type_name(
    types: &CustomTypes,
    base: &str,
    of_kind: impl Fn(&CustomType) -> bool,
) -> String {
    let mut number = types.types.iter().filter(|t| of_kind(t)).count();
    loop {
        let name = format!("{base}{number}");
        if !types.types.iter().any(|t| t.name == name) {
            return name;
        }
        number += 1;
    }
}

/// The enum's name, `_` and the first number, from the count of its values, that no value has
/// (Tiled's `nextValueText`).
fn next_value_name(enumeration: &EnumType, type_name: &str) -> String {
    let base = if type_name.is_empty() {
        String::new()
    } else {
        format!("{type_name}_")
    };
    let mut number = enumeration.values.len();
    loop {
        let name = format!("{base}{number}");
        if !enumeration.values.contains(&name) {
            return name;
        }
        number += 1;
    }
}

/// Sets `field` to `value`; whether it changed.
fn assign<T: PartialEq>(field: &mut T, value: T) -> bool {
    if *field == value {
        return false;
    }
    *field = value;
    true
}

#[cfg(test)]
mod tests {
    use bt_core::{Property, Value};

    use super::*;
    use crate::properties::{Step, ValueType};

    fn id_of(types: &CustomTypes, name: &str) -> CustomTypeId {
        types.types.iter().find(|t| t.name == name).unwrap().id
    }

    #[test]
    fn types_are_added_named_renamed_and_removed_as_tileds_editor_does() {
        let mut types = CustomTypes::new();
        TypeEdit::AddEnum.apply_to(&mut types).unwrap();
        TypeEdit::AddEnum.apply_to(&mut types).unwrap();
        TypeEdit::AddClass.apply_to(&mut types).unwrap();
        let names: Vec<&str> = types.types.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Enum0", "Enum1", "Class0"]);

        let class = id_of(&types, "Class0");
        let rename = |name: &str| TypeEdit::Rename {
            id: class,
            name: name.to_owned(),
        };
        assert_eq!(rename(" Enemy ").apply_to(&mut types), Ok(true));
        assert_eq!(
            rename("Enum0").apply_to(&mut types),
            Err(PropertyError::TypeNameTaken("Enum0".into()))
        );
        assert_eq!(
            rename("  ").apply_to(&mut types),
            Err(PropertyError::EmptyTypeName)
        );
        assert_eq!(rename("Enemy").apply_to(&mut types), Ok(false));
        assert_eq!(TypeEdit::Remove(class).apply_to(&mut types), Ok(true));
        assert_eq!(
            TypeEdit::Remove(class).apply_to(&mut types),
            Err(PropertyError::NoSuchType(class))
        );
        assert_eq!(TypeEdit::AddClass.label(), "Add Class");
    }

    #[test]
    fn enums_get_values_storage_and_flags() {
        let mut types = CustomTypes::new();
        TypeEdit::AddEnum.apply_to(&mut types).unwrap();
        let id = id_of(&types, "Enum0");
        TypeEdit::AddValue(id).apply_to(&mut types).unwrap();
        TypeEdit::AddValue(id).apply_to(&mut types).unwrap();
        let values = |types: &CustomTypes| types.enum_type("Enum0").unwrap().values.clone();
        assert_eq!(values(&types), ["Enum0_0", "Enum0_1"]);
        let rename = |index: usize, name: &str| TypeEdit::RenameValue {
            id,
            index,
            name: name.to_owned(),
        };
        rename(0, "Small").apply_to(&mut types).unwrap();
        assert_eq!(
            rename(1, "Small").apply_to(&mut types),
            Err(PropertyError::ValueNameTaken("Small".into()))
        );
        assert_eq!(
            rename(5, "Huge").apply_to(&mut types),
            Err(PropertyError::NoSuchValue(5))
        );
        TypeEdit::RemoveValue { id, index: 1 }
            .apply_to(&mut types)
            .unwrap();
        assert_eq!(values(&types), ["Small"]);
        TypeEdit::SetStorage {
            id,
            storage: EnumStorage::Int,
        }
        .apply_to(&mut types)
        .unwrap();
        TypeEdit::SetValuesAsFlags { id, flags: true }
            .apply_to(&mut types)
            .unwrap();
        let enumeration = types.enum_type("Enum0").unwrap();
        assert_eq!(enumeration.storage, EnumStorage::Int);
        assert!(enumeration.values_as_flags);
        for _ in 1..MAX_FLAGS {
            TypeEdit::AddValue(id).apply_to(&mut types).unwrap();
        }
        assert_eq!(
            TypeEdit::AddValue(id).apply_to(&mut types),
            Err(PropertyError::TooManyFlags)
        );
        assert_eq!(
            TypeEdit::SetColor {
                id,
                color: Color::WHITE
            }
            .apply_to(&mut types),
            Err(PropertyError::NotAClass("Enum0".into()))
        );
    }

    #[test]
    fn class_members_change_as_properties_and_never_contain_their_class() {
        let mut types = CustomTypes::new();
        TypeEdit::AddClass.apply_to(&mut types).unwrap();
        TypeEdit::AddClass.apply_to(&mut types).unwrap();
        let outer = id_of(&types, "Class0");
        let inner = id_of(&types, "Class1");
        let members = |id, change| TypeEdit::Members { id, change };
        members(outer, Change::add("hp", ValueType::Int))
            .apply_to(&mut types)
            .unwrap();
        let set = Change::set(
            vec![Step::Member("hp".into())],
            Property::new(Value::Int(10)),
        );
        assert_eq!(members(outer, set.clone()).label(), "Set Member 'hp'");
        members(outer, set).apply_to(&mut types).unwrap();
        let class = types.value_class("Class0").unwrap();
        assert_eq!(class.members["hp"], Property::new(Value::Int(10)));

        members(
            outer,
            Change::add("loot", ValueType::Custom("Class1".into())),
        )
        .apply_to(&mut types)
        .unwrap();
        let refused = members(
            inner,
            Change::add("back", ValueType::Custom("Class0".into())),
        )
        .apply_to(&mut types);
        assert!(
            matches!(refused, Err(PropertyError::Circular(_))),
            "{refused:?}"
        );
        assert!(types.value_class("Class1").unwrap().members.is_empty());

        TypeEdit::SetUseAs {
            id: outer,
            usage: ClassUsage::OBJECT,
        }
        .apply_to(&mut types)
        .unwrap();
        assert_eq!(types.find_by_id(outer).unwrap().usage(), ClassUsage::OBJECT);
    }
}
