//! Custom types: the classes and enums of a project, and the rules Tiled resolves property
//! values with. The rules follow libtiled's `propertytype.cpp` and `object.cpp`
//! (`reference/tiled/src/libtiled/`). Unlike Tiled, values stay in the form files store them
//! (an enum value as its name or number, the type's name in [`Property::custom_type`]), so a
//! file opened without its project still saves unchanged.

use crate::color::Color;
use crate::ids::{CustomTypeId, ObjectId};
use crate::object::MapObject;
use crate::property::{Properties, Property, Value};
use crate::tile::TileData;

bitflags::bitflags! {
    /// What a class may be the class of (Tiled's `useAs`).
    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
    pub struct ClassUsage: u16 {
        /// The type of custom property values.
        const PROPERTY = 0x001;
        /// Layers.
        const LAYER = 0x002;
        /// Objects.
        const OBJECT = 0x004;
        /// Maps.
        const MAP = 0x008;
        /// Tilesets.
        const TILESET = 0x010;
        /// Tiles.
        const TILE = 0x020;
        /// Terrain sets.
        const WANG_SET = 0x040;
        /// Terrains.
        const WANG_COLOR = 0x080;
        /// Projects.
        const PROJECT = 0x100;
    }
}

impl ClassUsage {
    /// The `useAs` names, in the order Tiled writes them.
    pub const NAMES: [UsageName; 9] = [
        UsageName {
            name: "property",
            usage: ClassUsage::PROPERTY,
        },
        UsageName {
            name: "map",
            usage: ClassUsage::MAP,
        },
        UsageName {
            name: "layer",
            usage: ClassUsage::LAYER,
        },
        UsageName {
            name: "object",
            usage: ClassUsage::OBJECT,
        },
        UsageName {
            name: "tile",
            usage: ClassUsage::TILE,
        },
        UsageName {
            name: "tileset",
            usage: ClassUsage::TILESET,
        },
        UsageName {
            name: "wangcolor",
            usage: ClassUsage::WANG_COLOR,
        },
        UsageName {
            name: "wangset",
            usage: ClassUsage::WANG_SET,
        },
        UsageName {
            name: "project",
            usage: ClassUsage::PROJECT,
        },
    ];

    /// The usage with the `useAs` name `name`.
    pub fn from_use_as(name: &str) -> Option<ClassUsage> {
        ClassUsage::NAMES
            .iter()
            .find(|n| n.name == name)
            .map(|n| n.usage)
    }

    /// The `useAs` names of the usages included, in Tiled's order.
    pub fn use_as_names(self) -> impl Iterator<Item = &'static str> {
        ClassUsage::NAMES
            .into_iter()
            .filter(move |n| self.contains(n.usage))
            .map(|n| n.name)
    }
}

/// A usage with its `useAs` name.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct UsageName {
    /// The name in files.
    pub name: &'static str,
    /// The usage.
    pub usage: ClassUsage,
}

/// How an enum's values are stored in files.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum EnumStorage {
    /// As names; comma-separated names for flags.
    #[default]
    String,
    /// As indexes; a bit set for flags.
    Int,
}

/// An enum: a list of named values.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnumType {
    /// How values are stored.
    pub storage: EnumStorage,
    /// The names, in order.
    pub values: Vec<String>,
    /// Whether several values can be set at once: bit `i` stands for `values[i]`.
    pub values_as_flags: bool,
}

/// The most values an enum of flags can hold.
const MAX_FLAGS: usize = 31;

impl EnumType {
    /// The integer form of a value: a name's index, or for flags the bits of comma-separated
    /// names (empty names skipped). An integer is returned as it is. `None` when a name is not
    /// one of the values.
    pub fn to_int(&self, value: &Value) -> Option<i64> {
        match value {
            Value::Int(i) => Some(*i),
            Value::String(text) if self.values_as_flags => {
                let mut flags = 0i64;
                for name in text.split(',').filter(|n| !n.is_empty()) {
                    let index = self.values.iter().position(|v| v == name)?;
                    flags |= 1i64.checked_shl(u32::try_from(index).ok()?)?;
                }
                Some(flags)
            }
            Value::String(text) => self
                .values
                .iter()
                .position(|v| v == text)
                .and_then(|i| i64::try_from(i).ok()),
            _ => None,
        }
    }

    /// The name form of an integer: its value's name, or for flags the comma-separated names of
    /// the bits set (only the first 31 values can be flags). `None` when there is no such value.
    pub fn to_name(&self, value: i64) -> Option<String> {
        if self.values_as_flags {
            let names: Vec<&str> = self
                .values
                .iter()
                .take(MAX_FLAGS)
                .enumerate()
                .filter(|(i, _)| value & (1i64 << i) != 0)
                .map(|(_, name)| name.as_str())
                .collect();
            return Some(names.join(","));
        }
        usize::try_from(value)
            .ok()
            .and_then(|i| self.values.get(i))
            .cloned()
    }

    /// A value in this enum's storage form. A value that cannot be converted is returned as it
    /// is, as Tiled keeps it, rather than lost.
    pub fn to_storage(&self, value: &Value) -> Value {
        match (self.storage, value) {
            (EnumStorage::String, Value::Int(i)) => self
                .to_name(*i)
                .map_or_else(|| value.clone(), Value::String),
            (EnumStorage::Int, Value::String(_)) => {
                self.to_int(value).map_or_else(|| value.clone(), Value::Int)
            }
            _ => value.clone(),
        }
    }

    /// The default value (the first value, or no flags) in storage form.
    pub fn default_value(&self) -> Value {
        self.to_storage(&Value::Int(0))
    }
}

/// A class: named members with default values.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassType {
    /// The members and their defaults. A member of a custom type names it in
    /// [`Property::custom_type`]; a class-valued member holds only the members it changes.
    pub members: Properties,
    /// The colour objects of this class are drawn in.
    pub color: Color,
    /// Whether shapes of objects of this class are filled.
    pub draw_fill: bool,
    /// What the class may be the class of.
    pub use_as: ClassUsage,
}

impl Default for ClassType {
    fn default() -> ClassType {
        ClassType {
            members: Properties::new(),
            color: Color::GRAY,
            draw_fill: true,
            use_as: ClassUsage::all(),
        }
    }
}

/// A class or an enum.
#[derive(Clone, Debug, PartialEq)]
pub enum CustomTypeKind {
    /// A class.
    Class(ClassType),
    /// An enum.
    Enum(EnumType),
}

/// A named class or enum of a project.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomType {
    /// Unique in the project.
    pub id: CustomTypeId,
    /// The name; with the usage, what files refer to the type by.
    pub name: String,
    /// Class or enum.
    pub kind: CustomTypeKind,
}

impl CustomType {
    /// A class, without an id yet ([`CustomTypes::add`] gives it one).
    pub fn class(name: impl Into<String>, class: ClassType) -> CustomType {
        CustomType {
            id: CustomTypeId(0),
            name: name.into(),
            kind: CustomTypeKind::Class(class),
        }
    }

    /// An enum, without an id yet.
    pub fn enumeration(name: impl Into<String>, enumeration: EnumType) -> CustomType {
        CustomType {
            id: CustomTypeId(0),
            name: name.into(),
            kind: CustomTypeKind::Enum(enumeration),
        }
    }

    /// The class, for a class.
    pub fn as_class(&self) -> Option<&ClassType> {
        match &self.kind {
            CustomTypeKind::Class(class) => Some(class),
            CustomTypeKind::Enum(_) => None,
        }
    }

    /// The enum, for an enum.
    pub fn as_enum(&self) -> Option<&EnumType> {
        match &self.kind {
            CustomTypeKind::Enum(enumeration) => Some(enumeration),
            CustomTypeKind::Class(_) => None,
        }
    }

    /// What the type can be used for: a class's usage; an enum is a property value type.
    pub fn usage(&self) -> ClassUsage {
        match &self.kind {
            CustomTypeKind::Class(class) => class.use_as,
            CustomTypeKind::Enum(_) => ClassUsage::PROPERTY,
        }
    }
}

/// A class member that makes its class contain itself, directly or through other classes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CircularMember {
    /// The class.
    pub class: String,
    /// The member.
    pub member: String,
}

/// The custom types of a project, in file order. A type is identified by its name and usage:
/// an enum and a class used only for objects may share a name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomTypes {
    /// The types.
    pub types: Vec<CustomType>,
}

impl CustomTypes {
    /// No types.
    pub fn new() -> CustomTypes {
        CustomTypes::default()
    }

    /// Whether there are no types.
    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    /// Adds a type, giving it the next free id when its id is 0, and returns its id.
    pub fn add(&mut self, mut custom_type: CustomType) -> CustomTypeId {
        if custom_type.id == CustomTypeId(0) {
            let highest = self.types.iter().map(|t| t.id.0).max().unwrap_or(0);
            custom_type.id = CustomTypeId(highest.saturating_add(1));
        }
        let id = custom_type.id;
        self.types.push(custom_type);
        id
    }

    /// The type named `name` usable for any of `usage` (Tiled's `findTypeByName`).
    pub fn find(&self, name: &str, usage: ClassUsage) -> Option<&CustomType> {
        if name.is_empty() {
            return None;
        }
        self.types
            .iter()
            .find(|t| t.name == name && t.usage().intersects(usage))
    }

    /// The type with this id.
    pub fn find_by_id(&self, id: CustomTypeId) -> Option<&CustomType> {
        self.types.iter().find(|t| t.id == id)
    }

    /// The type of property values with this `propertytype`: an enum, or a class usable as a
    /// property value.
    pub fn property_type(&self, name: &str) -> Option<&CustomType> {
        self.find(name, ClassUsage::PROPERTY)
    }

    /// The enum of property values with this `propertytype`.
    pub fn enum_type(&self, name: &str) -> Option<&EnumType> {
        self.property_type(name).and_then(CustomType::as_enum)
    }

    /// The class of property values with this `propertytype`.
    pub fn value_class(&self, name: &str) -> Option<&ClassType> {
        self.property_type(name).and_then(CustomType::as_class)
    }

    /// The class named `name` that can be the class of data of `usage` (Tiled's
    /// `findClassFor`).
    pub fn class_for(&self, name: &str, usage: ClassUsage) -> Option<&ClassType> {
        if name.is_empty() {
            return None;
        }
        self.types.iter().find_map(|t| {
            t.as_class()
                .filter(|c| t.name == name && c.use_as.intersects(usage))
        })
    }

    /// Merges `other` in as Tiled's `PropertyTypes::merge` does: a type replaces the one with its
    /// name and an overlapping usage, keeping that one's id; the others are added with new ids.
    pub fn merge(&mut self, other: CustomTypes) {
        for mut custom_type in other.types {
            let existing = self
                .types
                .iter_mut()
                .find(|t| t.name == custom_type.name && t.usage().intersects(custom_type.usage()));
            match existing {
                Some(existing) => {
                    custom_type.id = existing.id;
                    *existing = custom_type;
                }
                None => {
                    custom_type.id = CustomTypeId(0);
                    self.add(custom_type);
                }
            }
        }
    }

    /// A value completed with its class's member defaults, as Tiled's "Resolve object types and
    /// properties" export does: every member a class value leaves unset gets the class's default,
    /// class-valued members are completed in turn, list items one by one. Values of unknown types
    /// stay as they are, and a member referring back to a class being completed is not expanded.
    pub fn resolve_value(&self, property: &Property) -> Property {
        let mut resolved = property.clone();
        self.complete(&mut resolved, &mut Vec::new());
        resolved
    }

    /// Every member of a property-value class with its default, class-valued members completed;
    /// `None` when there is no such class.
    pub fn class_defaults(&self, name: &str) -> Option<Properties> {
        self.value_class(name)?;
        let mut value = Property::typed(Value::Class(Properties::new()), name);
        self.complete(&mut value, &mut Vec::new());
        match value.value {
            Value::Class(members) => Some(members),
            _ => None,
        }
    }

    /// The properties of data of `usage` and `class` as Tiled resolves them: the class's members
    /// (when `class` names a class for `usage`) overridden by the data's own, class values
    /// completed. For everything but objects ([`CustomTypes::object_properties`]).
    pub fn resolved_properties(
        &self,
        class: &str,
        usage: ClassUsage,
        own: &Properties,
    ) -> Properties {
        let mut properties = self
            .class_for(class, usage)
            .map(|c| c.members.clone())
            .unwrap_or_default();
        properties.extend(
            own.iter()
                .map(|(name, property)| (name.clone(), property.clone())),
        );
        self.completed_all(properties)
    }

    /// The properties of an object as Tiled resolves them: its class's members, then its tile's
    /// properties, then its template's, then its own; class values completed. The class is the
    /// object's own, else its template's, else its tile's. Loaded template instances already
    /// carry the template's class and properties, so `template` may be `None` for them.
    pub fn object_properties(
        &self,
        object: &MapObject,
        tile: Option<&TileData>,
        template: Option<&MapObject>,
    ) -> Properties {
        let class = [
            Some(object.class.as_str()),
            template.map(|t| t.class.as_str()),
            tile.map(|t| t.class.as_str()),
        ]
        .into_iter()
        .flatten()
        .find(|c| !c.is_empty())
        .unwrap_or("");
        let mut properties = self
            .class_for(class, ClassUsage::OBJECT)
            .map(|c| c.members.clone())
            .unwrap_or_default();
        let layers = [
            tile.map(|t| &t.properties),
            template.map(|t| &t.properties),
            Some(&object.properties),
        ];
        for layer in layers.into_iter().flatten() {
            properties.extend(
                layer
                    .iter()
                    .map(|(name, property)| (name.clone(), property.clone())),
            );
        }
        self.completed_all(properties)
    }

    fn completed_all(&self, mut properties: Properties) -> Properties {
        let mut stack = Vec::new();
        for property in properties.values_mut() {
            self.complete(property, &mut stack);
        }
        properties
    }

    /// Fills in the unset members of class values, recursively; `stack` holds the classes being
    /// completed, so a class containing itself stops.
    fn complete<'a>(&'a self, property: &mut Property, stack: &mut Vec<&'a str>) {
        match &mut property.value {
            Value::List(items) => {
                for item in items {
                    self.complete(item, stack);
                }
            }
            Value::Class(members) => {
                let Some(found) = property
                    .custom_type
                    .as_deref()
                    .and_then(|name| self.property_type(name))
                else {
                    return;
                };
                let Some(class) = found.as_class() else {
                    return;
                };
                if stack.contains(&found.name.as_str()) {
                    return;
                }
                stack.push(&found.name);
                for (name, default) in &class.members {
                    members
                        .entry(name.clone())
                        .or_insert_with(|| default.clone());
                }
                for member in members.values_mut() {
                    self.complete(member, stack);
                }
                stack.pop();
            }
            _ => {}
        }
    }

    /// Brings a value into the form its type prescribes, as Tiled does when it loads a file with
    /// its project: enum values into their storage form, class members into the type of the
    /// class's member (a member without a custom type takes the member's). Unknown types and
    /// members are left alone.
    pub fn normalize(&self, property: &mut Property) {
        match &mut property.value {
            Value::List(items) => {
                for item in items {
                    self.normalize(item);
                }
            }
            Value::Class(members) => {
                let class = property
                    .custom_type
                    .as_deref()
                    .and_then(|name| self.value_class(name));
                for (name, member) in members.iter_mut() {
                    if let Some(definition) = class.and_then(|c| c.members.get(name)) {
                        if member.custom_type.is_none() {
                            member.custom_type.clone_from(&definition.custom_type);
                        }
                        let enum_member = member
                            .custom_type
                            .as_deref()
                            .is_some_and(|t| self.enum_type(t).is_some());
                        if !enum_member
                            && let Some(converted) =
                                converted_like(&member.value, &definition.value)
                        {
                            member.value = converted;
                        }
                    }
                    self.normalize(member);
                }
            }
            _ => {
                if let Some(enumeration) = property
                    .custom_type
                    .as_deref()
                    .and_then(|name| self.enum_type(name))
                {
                    property.value = enumeration.to_storage(&property.value);
                }
            }
        }
    }

    /// [`CustomTypes::normalize`] for each property.
    pub fn normalize_all(&self, properties: &mut Properties) {
        if self.types.is_empty() {
            return;
        }
        for property in properties.values_mut() {
            self.normalize(property);
        }
    }

    /// The class members that make a class contain itself; Tiled drops them when it loads a
    /// project.
    pub fn circular_members(&self) -> Vec<CircularMember> {
        let mut found = Vec::new();
        for custom_type in &self.types {
            let Some(class) = custom_type.as_class() else {
                continue;
            };
            for (member_name, member) in &class.members {
                if self.refers_to(member, &custom_type.name, &mut Vec::new()) {
                    found.push(CircularMember {
                        class: custom_type.name.clone(),
                        member: member_name.clone(),
                    });
                }
            }
        }
        found
    }

    /// Whether a value (a class value or a list) contains the class `target`.
    fn refers_to<'a>(
        &'a self,
        property: &Property,
        target: &str,
        visited: &mut Vec<&'a str>,
    ) -> bool {
        match &property.value {
            Value::List(items) => items
                .iter()
                .any(|item| self.refers_to(item, target, visited)),
            Value::Class(_) => {
                let Some(found) = property
                    .custom_type
                    .as_deref()
                    .and_then(|name| self.property_type(name))
                else {
                    return false;
                };
                let Some(class) = found.as_class() else {
                    return false;
                };
                if found.name == target {
                    return true;
                }
                if visited.contains(&found.name.as_str()) {
                    return false;
                }
                visited.push(&found.name);
                class
                    .members
                    .values()
                    .any(|member| self.refers_to(member, target, visited))
            }
            _ => false,
        }
    }
}

/// `value` converted to the type of `like` where that is a plain conversion (numbers, text,
/// booleans, colours, files, object references); `None` when the types already match or no
/// conversion exists (the value is then kept as it is).
fn converted_like(value: &Value, like: &Value) -> Option<Value> {
    if std::mem::discriminant(value) == std::mem::discriminant(like) {
        return None;
    }
    let text = |v: &Value| match v {
        Value::String(s) | Value::File(s) => Some(s.trim().to_owned()),
        _ => None,
    };
    let number = |v: &Value| match v {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        Value::Object(id) => Some(f64::from(id.0)),
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        _ => text(v).and_then(|s| s.parse::<f64>().ok()),
    };
    match like {
        Value::String(_) => match value {
            Value::File(s) => Some(Value::String(s.clone())),
            Value::Int(i) => Some(Value::String(i.to_string())),
            Value::Float(f) => Some(Value::String(f.to_string())),
            Value::Bool(b) => Some(Value::String(b.to_string())),
            _ => None,
        },
        Value::Int(_) => number(value).and_then(rounded_i64).map(Value::Int),
        Value::Float(_) => number(value).map(Value::Float),
        Value::Bool(_) => match value {
            Value::String(s) => {
                let t = s.trim();
                Some(Value::Bool(
                    !(t.is_empty() || t == "0" || t.eq_ignore_ascii_case("false")),
                ))
            }
            _ => number(value).map(|f| Value::Bool(f != 0.0)),
        },
        Value::Color(_) => text(value).map(|s| Value::Color(Color::from_tiled_text(&s))),
        Value::File(_) => match value {
            Value::String(s) => Some(Value::File(s.clone())),
            _ => None,
        },
        Value::Object(_) => number(value)
            .and_then(rounded_i64)
            .and_then(|i| u32::try_from(i).ok())
            .map(|id| Value::Object(ObjectId(id))),
        Value::Class(_) | Value::List(_) => None,
    }
}

/// `f` rounded to a whole number, when it is finite and fits.
fn rounded_i64(f: f64) -> Option<i64> {
    const LIMIT: f64 = 9.2e18;
    let rounded = f.round();
    // Within ±9.2e18 the cast is exact up to f64's precision and cannot overflow.
    (rounded.is_finite() && rounded.abs() < LIMIT).then_some(rounded as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class_value(members: &[(&str, Property)], custom_type: &str) -> Property {
        let members = members
            .iter()
            .map(|(n, p)| ((*n).to_owned(), p.clone()))
            .collect();
        Property::typed(Value::Class(members), custom_type)
    }

    fn members(list: &[(&str, Property)]) -> Properties {
        list.iter()
            .map(|(n, p)| ((*n).to_owned(), p.clone()))
            .collect()
    }

    fn enumeration(storage: EnumStorage, flags: bool) -> EnumType {
        EnumType {
            storage,
            values: vec!["A".into(), "B".into(), "C".into()],
            values_as_flags: flags,
        }
    }

    /// The types of Tiled's `examples.tiled-project`: Body, Fixture and Filter.
    fn physics() -> CustomTypes {
        let mut types = CustomTypes::new();
        types.add(CustomType::enumeration(
            "BodyType",
            EnumType {
                values: vec!["Static".into(), "Dynamic".into()],
                ..Default::default()
            },
        ));
        types.add(CustomType::enumeration(
            "CollisionCategories",
            EnumType {
                storage: EnumStorage::Int,
                values: (0..5).map(|i| i.to_string()).collect(),
                values_as_flags: true,
            },
        ));
        let category = |v| Property::typed(Value::Int(v), "CollisionCategories");
        let filter = ClassType {
            members: members(&[
                ("categoryBits", category(0)),
                ("groupIndex", Property::new(Value::Int(0))),
                ("maskBits", category(0)),
            ]),
            ..ClassType::default()
        };
        types.add(CustomType::class("Filter", filter));
        let fixture = ClassType {
            members: members(&[
                ("density", Property::new(Value::Float(0.0))),
                (
                    "filter",
                    class_value(
                        &[("categoryBits", category(1)), ("maskBits", category(1))],
                        "Filter",
                    ),
                ),
                ("friction", Property::new(Value::Float(1.0))),
            ]),
            use_as: ClassUsage::PROPERTY | ClassUsage::OBJECT | ClassUsage::TILE,
            ..ClassType::default()
        };
        types.add(CustomType::class("Fixture", fixture));
        let body = ClassType {
            members: members(&[
                ("fixture", class_value(&[], "Fixture")),
                (
                    "type",
                    Property::typed(Value::String("Static".into()), "BodyType"),
                ),
            ]),
            ..ClassType::default()
        };
        types.add(CustomType::class("Body", body));
        types
    }

    #[test]
    fn usage_names_follow_tileds_order() {
        let usage = ClassUsage::OBJECT | ClassUsage::TILE;
        assert_eq!(usage.use_as_names().collect::<Vec<_>>(), ["object", "tile"]);
        assert_eq!(
            ClassUsage::from_use_as("wangcolor"),
            Some(ClassUsage::WANG_COLOR)
        );
        assert_eq!(ClassUsage::all().use_as_names().count(), 9);
    }

    #[test]
    fn enum_values_convert_between_names_and_numbers() {
        let plain = enumeration(EnumStorage::String, false);
        assert_eq!(plain.to_int(&Value::String("B".into())), Some(1));
        assert_eq!(plain.to_int(&Value::String("D".into())), None);
        assert_eq!(plain.to_name(2).as_deref(), Some("C"));
        assert_eq!(plain.to_name(3), None);
        assert_eq!(plain.to_storage(&Value::Int(1)), Value::String("B".into()));
        assert_eq!(
            plain.to_storage(&Value::Int(7)),
            Value::Int(7),
            "out of range: kept"
        );
        assert_eq!(plain.default_value(), Value::String("A".into()));

        let flags = enumeration(EnumStorage::String, true);
        assert_eq!(flags.to_int(&Value::String("B,C".into())), Some(6));
        assert_eq!(flags.to_int(&Value::String(",A,,C".into())), Some(5));
        assert_eq!(flags.to_int(&Value::String("A,X".into())), None);
        assert_eq!(flags.to_name(6).as_deref(), Some("B,C"));
        assert_eq!(flags.default_value(), Value::String(String::new()));

        let int_flags = enumeration(EnumStorage::Int, true);
        assert_eq!(
            int_flags.to_storage(&Value::String("A,C".into())),
            Value::Int(5)
        );
        assert_eq!(
            int_flags.to_storage(&Value::String("nope".into())),
            Value::String("nope".into())
        );
        assert_eq!(
            enumeration(EnumStorage::Int, false).to_storage(&Value::String("C".into())),
            Value::Int(2)
        );
    }

    #[test]
    fn lookups_respect_usage_and_ids_are_given_in_order() {
        let mut types = physics();
        types.add(CustomType::class(
            "BodyType",
            ClassType {
                use_as: ClassUsage::OBJECT,
                ..ClassType::default()
            },
        ));
        assert!(types.property_type("BodyType").unwrap().as_enum().is_some());
        assert!(types.class_for("BodyType", ClassUsage::OBJECT).is_some());
        assert!(types.class_for("Fixture", ClassUsage::MAP).is_none());
        assert!(types.class_for("Fixture", ClassUsage::TILE).is_some());
        assert!(types.find("", ClassUsage::all()).is_none());
        let ids: Vec<u32> = types.types.iter().map(|t| t.id.0).collect();
        assert_eq!(ids, [1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn class_defaults_are_completed_recursively() {
        let types = physics();
        let body = types.class_defaults("Body").unwrap();
        assert_eq!(
            body["type"],
            Property::typed(Value::String("Static".into()), "BodyType")
        );
        let Value::Class(fixture) = &body["fixture"].value else {
            panic!()
        };
        assert_eq!(fixture["friction"].value, Value::Float(1.0));
        let Value::Class(filter) = &fixture["filter"].value else {
            panic!()
        };
        // Fixture's defaults for its filter win over Filter's own.
        assert_eq!(filter["categoryBits"].value, Value::Int(1));
        assert_eq!(filter["groupIndex"].value, Value::Int(0));
        assert_eq!(types.class_defaults("BodyType"), None);

        let value = class_value(
            &[(
                "filter",
                class_value(&[("groupIndex", Property::new(Value::Int(-2)))], "Filter"),
            )],
            "Fixture",
        );
        let resolved = types.resolve_value(&value);
        let Value::Class(members) = &resolved.value else {
            panic!()
        };
        let Value::Class(filter) = &members["filter"].value else {
            panic!()
        };
        assert_eq!(
            (&filter["groupIndex"].value, &filter["categoryBits"].value),
            (&Value::Int(-2), &Value::Int(0))
        );
        assert_eq!(members["density"].value, Value::Float(0.0));
    }

    #[test]
    fn classes_containing_themselves_are_found_and_not_expanded() {
        let mut types = CustomTypes::new();
        types.add(CustomType::class(
            "A",
            ClassType {
                members: members(&[
                    ("b", class_value(&[], "B")),
                    ("n", Property::new(Value::Int(1))),
                ]),
                ..ClassType::default()
            },
        ));
        types.add(CustomType::class(
            "B",
            ClassType {
                members: members(&[("a", class_value(&[], "A"))]),
                ..ClassType::default()
            },
        ));
        types.add(CustomType::class(
            "Self",
            ClassType {
                members: members(&[(
                    "me",
                    Property::new(Value::List(vec![class_value(&[], "Self")])),
                )]),
                ..ClassType::default()
            },
        ));
        let circular: Vec<(String, String)> = types
            .circular_members()
            .into_iter()
            .map(|c| (c.class, c.member))
            .collect();
        assert_eq!(
            circular,
            [
                ("A".into(), "b".into()),
                ("B".into(), "a".into()),
                ("Self".into(), "me".into())
            ]
        );
        let a = types.class_defaults("A").unwrap();
        let Value::Class(b) = &a["b"].value else {
            panic!()
        };
        assert_eq!(
            b["a"].value,
            Value::Class(Properties::new()),
            "B's member refers back to A: not expanded"
        );
        assert!(physics().circular_members().is_empty());
    }

    #[test]
    fn object_properties_stack_class_tile_template_and_own() {
        let mut types = physics();
        types.add(CustomType::class(
            "Enemy",
            ClassType {
                members: members(&[
                    ("hp", Property::new(Value::Int(10))),
                    ("speed", Property::new(Value::Float(1.5))),
                ]),
                use_as: ClassUsage::OBJECT | ClassUsage::TILE,
                ..ClassType::default()
            },
        ));
        let tile = TileData {
            class: "Enemy".into(),
            properties: members(&[
                ("hp", Property::new(Value::Int(20))),
                ("art", Property::new(Value::File("a.png".into()))),
            ]),
            ..TileData::default()
        };
        let template = MapObject {
            properties: members(&[("hp", Property::new(Value::Int(30)))]),
            ..MapObject::default()
        };
        let object = MapObject {
            properties: members(&[
                ("speed", Property::new(Value::Float(3.0))),
                ("body", class_value(&[], "Fixture")),
            ]),
            ..MapObject::default()
        };
        let properties = types.object_properties(&object, Some(&tile), Some(&template));
        assert_eq!(properties["hp"].value, Value::Int(30));
        assert_eq!(properties["speed"].value, Value::Float(3.0));
        assert_eq!(properties["art"].value, Value::File("a.png".into()));
        let Value::Class(body) = &properties["body"].value else {
            panic!()
        };
        assert_eq!(
            body["friction"].value,
            Value::Float(1.0),
            "class values are completed"
        );

        let with_class = MapObject {
            class: "Filter".into(),
            ..object.clone()
        };
        let properties = types.object_properties(&with_class, Some(&tile), None);
        assert_eq!(
            (&properties["hp"].value, &properties["groupIndex"].value),
            (&Value::Int(20), &Value::Int(0))
        );
        assert_eq!(
            types
                .resolved_properties("Filter", ClassUsage::LAYER, &Properties::new())
                .len(),
            3
        );
        assert!(
            types
                .resolved_properties("Fixture", ClassUsage::LAYER, &Properties::new())
                .is_empty()
        );
    }

    #[test]
    fn normalizing_follows_the_types() {
        let types = physics();
        let mut category = Property::typed(Value::String("1,3".into()), "CollisionCategories");
        types.normalize(&mut category);
        assert_eq!(category.value, Value::Int(10));
        let mut body_type = Property::typed(Value::Int(1), "BodyType");
        types.normalize(&mut body_type);
        assert_eq!(body_type.value, Value::String("Dynamic".into()));

        let mut fixture = class_value(
            &[
                ("density", Property::new(Value::String("2.5".into()))),
                (
                    "filter",
                    class_value(
                        &[("maskBits", Property::new(Value::String("0,2".into())))],
                        "Filter",
                    ),
                ),
                ("unknown", Property::new(Value::String("x".into()))),
            ],
            "Fixture",
        );
        types.normalize(&mut fixture);
        let Value::Class(members) = &fixture.value else {
            panic!()
        };
        assert_eq!(members["density"].value, Value::Float(2.5));
        assert_eq!(members["unknown"].value, Value::String("x".into()));
        let Value::Class(filter) = &members["filter"].value else {
            panic!()
        };
        assert_eq!(
            filter["maskBits"],
            Property::typed(Value::Int(5), "CollisionCategories")
        );

        let mut unknown = Property::typed(Value::Int(3), "Nope");
        types.normalize(&mut unknown);
        assert_eq!(unknown, Property::typed(Value::Int(3), "Nope"));
        assert_eq!(
            converted_like(&Value::String("1e30".into()), &Value::Int(0)),
            None,
            "too large to be an int"
        );
    }

    #[test]
    fn merging_replaces_by_name_and_usage() {
        let mut types = physics();
        let mut other = CustomTypes::new();
        other.add(CustomType::class(
            "Fixture",
            ClassType {
                color: Color::BLACK,
                ..ClassType::default()
            },
        ));
        other.add(CustomType::enumeration("Fresh", EnumType::default()));
        types.merge(other);
        let fixture = types.find("Fixture", ClassUsage::all()).unwrap();
        assert_eq!(
            (fixture.id, fixture.as_class().unwrap().color),
            (CustomTypeId(4), Color::BLACK)
        );
        assert_eq!(
            types.find("Fresh", ClassUsage::PROPERTY).unwrap().id,
            CustomTypeId(6)
        );
    }
}
