//! Ids for new layers and objects, and the object references among copies (Tiled's
//! `ObjectReferencesHelper`, `reference/tiled/src/tiled/objectreferenceshelper.cpp`).

use std::collections::BTreeMap;

use bt_core::{LayerId, Map, ObjectId, Properties, Property, Value};

use crate::error::EditError;

/// Takes the map's next layer id.
pub(crate) fn take_layer_id(map: &mut Map) -> Result<LayerId, EditError> {
    let id = map.next_layer_id.max(1);
    map.next_layer_id = id.checked_add(1).ok_or(EditError::OutOfIds)?;
    Ok(LayerId(id))
}

/// Takes the map's next object id.
pub(crate) fn take_object_id(map: &mut Map) -> Result<ObjectId, EditError> {
    let id = map.next_object_id.max(1);
    map.next_object_id = id.checked_add(1).ok_or(EditError::OutOfIds)?;
    Ok(ObjectId(id))
}

/// The ids copies of objects were given. A reference from one copy to an object that was copied
/// with it follows the copy; a reference to anything else stays as it was.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IdRemap {
    copies: BTreeMap<ObjectId, ObjectId>,
}

impl IdRemap {
    /// No copies yet.
    pub fn new() -> IdRemap {
        IdRemap::default()
    }

    /// Records that `copy` is the copy of `original`.
    pub fn record(&mut self, original: ObjectId, copy: ObjectId) {
        self.copies.insert(original, copy);
    }

    /// The copy of `original`, if it was copied.
    pub fn copy_of(&self, original: ObjectId) -> Option<ObjectId> {
        self.copies.get(&original).copied()
    }

    /// Points the object references in `properties` (class members and list items included) at
    /// the copies.
    pub fn rewire_properties(&self, properties: &mut Properties) {
        for property in properties.values_mut() {
            self.rewire_property(property);
        }
    }

    fn rewire_property(&self, property: &mut Property) {
        match &mut property.value {
            Value::Object(id) => {
                if let Some(copy) = self.copy_of(*id) {
                    *id = copy;
                }
            }
            Value::Class(members) => self.rewire_properties(members),
            Value::List(items) => {
                for item in items {
                    self.rewire_property(item);
                }
            }
            Value::String(_)
            | Value::Int(_)
            | Value::Float(_)
            | Value::Bool(_)
            | Value::Color(_)
            | Value::File(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_taken_in_order_and_never_wrap() {
        let mut map = Map {
            next_layer_id: 4,
            next_object_id: 0,
            ..Map::default()
        };
        assert_eq!(take_layer_id(&mut map), Ok(LayerId(4)));
        assert_eq!(map.next_layer_id, 5);
        assert_eq!(take_object_id(&mut map), Ok(ObjectId(1)), "ids start at 1");
        map.next_object_id = u32::MAX;
        assert_eq!(take_object_id(&mut map), Err(EditError::OutOfIds));
    }

    #[test]
    fn references_among_copies_follow_them() {
        let mut remap = IdRemap::new();
        remap.record(ObjectId(3), ObjectId(30));
        let mut properties = Properties::new();
        properties.insert("target".into(), Property::new(Value::Object(ObjectId(3))));
        properties.insert("other".into(), Property::new(Value::Object(ObjectId(4))));
        let mut members = Properties::new();
        members.insert("next".into(), Property::new(Value::Object(ObjectId(3))));
        properties.insert(
            "door".into(),
            Property::typed(Value::Class(members), "Door"),
        );
        properties.insert(
            "path".into(),
            Property::new(Value::List(vec![Property::new(Value::Object(ObjectId(3)))])),
        );
        remap.rewire_properties(&mut properties);
        assert_eq!(properties["target"].value, Value::Object(ObjectId(30)));
        assert_eq!(
            properties["other"].value,
            Value::Object(ObjectId(4)),
            "not copied"
        );
        let Value::Class(members) = &properties["door"].value else {
            panic!("a class");
        };
        assert_eq!(members["next"].value, Value::Object(ObjectId(30)));
        let Value::List(items) = &properties["path"].value else {
            panic!("a list");
        };
        assert_eq!(items[0].value, Value::Object(ObjectId(30)));
    }
}
