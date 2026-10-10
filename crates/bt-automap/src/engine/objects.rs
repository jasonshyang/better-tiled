//! Objects rules place and objects `DeleteTiles` removes. A placed object is the rule map's
//! object moved by the difference between where the rule's output box lands on the map and
//! where it is in the rule map, in the map's pixel coordinates (Tiled's `copyMapRegion`), so on
//! staggered and hexagonal maps the move follows the map's own geometry. Objects placed by one
//! output of one match are a group: at the end of the run each gets a new id and references
//! between them are rewired to the new ids (Tiled's `ObjectReferencesHelper`).

use std::collections::HashMap;

use bt_core::{Map, MapObject, ObjectId, Properties, Property, Value};
use glam::{DVec2, IVec2};

/// An object a rule placed, before it has its id.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlacedObject {
    /// The object layer of the run it goes to.
    pub(crate) target: usize,
    /// The object, with the rule map's id and the run's tile numbering.
    pub(crate) object: MapObject,
}

/// What one output of one match placed.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PlacedGroup {
    pub(crate) objects: Vec<PlacedObject>,
}

/// How far a rule's objects move when the rule matches `delta` cells from where it is in the
/// rule map: Tiled's `tileToPixelCoords(outputTopLeft + delta) - tileToPixelCoords(outputTopLeft)`
/// on the map.
pub(crate) fn pixel_offset(map: &Map, output_origin: IVec2, delta: IVec2) -> DVec2 {
    let geometry = map.geometry();
    geometry.tile_to_pixel((output_origin + delta).as_dvec2())
        - geometry.tile_to_pixel(output_origin.as_dvec2())
}

/// Gives every object of `group` the next id of the map and points the references between them
/// at the new ids; returns the ids given.
pub(crate) fn assign_ids(group: &mut PlacedGroup, next_object_id: &mut u32) -> Vec<ObjectId> {
    let mut renamed: HashMap<ObjectId, ObjectId> = HashMap::new();
    let mut given = Vec::with_capacity(group.objects.len());
    for placed in &mut group.objects {
        let new = ObjectId(*next_object_id);
        *next_object_id = next_object_id.saturating_add(1);
        renamed.entry(placed.object.id).or_insert(new);
        placed.object.id = new;
        given.push(new);
    }
    for placed in &mut group.objects {
        rewire(&mut placed.object.properties, &renamed);
    }
    given
}

/// Points object references in `properties` (in class values and lists too) at their new ids.
fn rewire(properties: &mut Properties, renamed: &HashMap<ObjectId, ObjectId>) {
    for property in properties.values_mut() {
        rewire_value(property, renamed);
    }
}

fn rewire_value(property: &mut Property, renamed: &HashMap<ObjectId, ObjectId>) {
    match &mut property.value {
        Value::Object(id) => {
            if let Some(new) = renamed.get(id) {
                *id = *new;
            }
        }
        Value::Class(members) => rewire(members, renamed),
        Value::List(items) => {
            for item in items {
                rewire_value(item, renamed);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{Orientation, StaggerAxis, StaggerIndex};

    use super::*;

    fn map(orientation: Orientation) -> Map {
        Map {
            orientation,
            width: 10,
            height: 10,
            tile_width: 32,
            tile_height: 16,
            stagger_axis: StaggerAxis::Y,
            stagger_index: StaggerIndex::Odd,
            hex_side_length: 8,
            ..Map::default()
        }
    }

    #[test]
    fn objects_move_by_the_map_geometry() {
        let ortho = pixel_offset(
            &map(Orientation::Orthogonal),
            IVec2::new(1, 1),
            IVec2::new(2, 3),
        );
        assert_eq!(ortho, DVec2::new(64.0, 48.0));
        // Isometric pixel coordinates are in units of the tile height on both axes.
        let iso = pixel_offset(
            &map(Orientation::Isometric),
            IVec2::new(1, 1),
            IVec2::new(2, 3),
        );
        assert_eq!(iso, DVec2::new(32.0, 48.0));
        // On a staggered map a move by one row shifts by half a tile, depending on the row.
        let staggered = map(Orientation::Staggered);
        let from_even = pixel_offset(&staggered, IVec2::new(0, 0), IVec2::new(0, 1));
        let from_odd = pixel_offset(&staggered, IVec2::new(0, 1), IVec2::new(0, 1));
        assert_ne!(from_even, from_odd);
    }

    #[test]
    fn placed_objects_get_new_ids_and_their_references_follow() {
        let mut door = MapObject {
            id: ObjectId(4),
            ..MapObject::default()
        };
        door.properties.insert(
            "opens".into(),
            Property::new(Value::List(vec![Property::new(Value::Object(ObjectId(5)))])),
        );
        door.properties.insert(
            "elsewhere".into(),
            Property::new(Value::Object(ObjectId(9))),
        );
        let mut lever = MapObject {
            id: ObjectId(5),
            ..MapObject::default()
        };
        let mut members = Properties::new();
        members.insert("door".into(), Property::new(Value::Object(ObjectId(4))));
        lever
            .properties
            .insert("link".into(), Property::new(Value::Class(members)));
        let mut group = PlacedGroup {
            objects: vec![
                PlacedObject {
                    target: 0,
                    object: door,
                },
                PlacedObject {
                    target: 0,
                    object: lever,
                },
            ],
        };
        let mut next = 20;
        assert_eq!(
            assign_ids(&mut group, &mut next),
            [ObjectId(20), ObjectId(21)]
        );
        assert_eq!(next, 22);
        let door = &group.objects[0].object;
        assert_eq!(
            door.properties["opens"].value,
            Value::List(vec![Property::new(Value::Object(ObjectId(21)))])
        );
        assert_eq!(
            door.properties["elsewhere"].value,
            Value::Object(ObjectId(9)),
            "a reference outside the group stays"
        );
        let Value::Class(link) = &group.objects[1].object.properties["link"].value else {
            panic!("a class value");
        };
        assert_eq!(link["door"].value, Value::Object(ObjectId(20)));
    }
}
