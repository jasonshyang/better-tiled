//! Objects' built-in attributes (Tiled's `MapObjectProperties`, `reference/tiled/src/tiled/
//! propertieswidget.cpp`): id and template (read-only), name, class, visibility, opacity,
//! position, size (objects with dimensions), rotation (all but points), a tile object's flips
//! and a text object's text and how it is set. Setting one on a template instance makes the
//! instance override that field of its template, as Tiled's `ChangeMapObject` does (a size
//! only when it changes, as Tiled's `TransformMapObjects` does).

use bt_core::{Flip, Map, MapObject, ObjectId, ObjectOverrides, Shape, TextData};

use super::attribute::Attribute;
use super::attribute_check::{bool_of, check_value, integer_of, invalid, number_of, text_of};
use super::attribute_row::{AttributeRow, Reading, assign, rows_of, settable};
use super::attribute_value::AttributeValue;
use super::error::PropertyError;
use super::lookup::object_mut;

/// The objects' attributes, in the order Tiled shows them.
const ORDER: [Attribute; 25] = [
    Attribute::Id,
    Attribute::Template,
    Attribute::Name,
    Attribute::Class,
    Attribute::Visible,
    Attribute::Opacity,
    Attribute::X,
    Attribute::Y,
    Attribute::Width,
    Attribute::Height,
    Attribute::Rotation,
    Attribute::FlipHorizontally,
    Attribute::FlipVertically,
    Attribute::Text,
    Attribute::FontFamily,
    Attribute::PixelSize,
    Attribute::WordWrap,
    Attribute::TextColor,
    Attribute::Bold,
    Attribute::Italic,
    Attribute::Underline,
    Attribute::Strikeout,
    Attribute::Kerning,
    Attribute::HAlign,
    Attribute::VAlign,
];

/// The built-in attributes of the map's objects `objects` as the Properties panel shows them.
/// An object the map does not have is an error.
pub fn object_attributes(
    map: &Map,
    objects: &[ObjectId],
) -> Result<Vec<AttributeRow>, PropertyError> {
    let found = found_objects(map, objects)?;
    Ok(rows_of(&ORDER, &found, |object, attribute| {
        read(object, attribute)
    }))
}

/// Sets one built-in attribute of the map's objects `objects`: of those that have it (a size
/// only on objects with dimensions, flips only on tile objects, text settings only on text
/// objects). Template instances override their template's field. Returns whether any object
/// changed. Refused, changing nothing, when the value is of the wrong kind or out of range, no
/// object has the attribute, it is read-only, or an object is missing.
pub fn set_object_attribute(
    map: &mut Map,
    objects: &[ObjectId],
    attribute: Attribute,
    value: &AttributeValue,
) -> Result<bool, PropertyError> {
    check_value(attribute, value)?;
    let found = found_objects(map, objects)?;
    let ids: Vec<ObjectId> = settable(attribute, &found, read)?
        .into_iter()
        .map(|object| object.id)
        .collect();
    let mut changed = false;
    for id in ids {
        let object = object_mut(map, id).ok_or(PropertyError::NoSuchObject(id))?;
        changed |= write(object, attribute, value)?;
    }
    Ok(changed)
}

/// The objects with these ids.
fn found_objects<'m>(map: &'m Map, ids: &[ObjectId]) -> Result<Vec<&'m MapObject>, PropertyError> {
    ids.iter()
        .map(|id| map.object(*id).ok_or(PropertyError::NoSuchObject(*id)))
        .collect()
}

/// Whether an object has a size (Tiled's `hasDimensions`): all but points, polygons and
/// polylines.
fn has_dimensions(object: &MapObject) -> bool {
    !matches!(
        object.shape,
        Shape::Point | Shape::Polygon(_) | Shape::Polyline(_)
    )
}

/// One attribute of an object; `None` for one its shape does not have.
fn read(object: &MapObject, attribute: Attribute) -> Option<Reading> {
    let text = match &object.shape {
        Shape::Text(text) => Some(text),
        _ => None,
    };
    let flip = object.tile.map(|cell| cell.flip());
    let value = match attribute {
        Attribute::Id => {
            return Some(Reading::fixed(AttributeValue::Integer(i64::from(
                object.id.0,
            ))));
        }
        Attribute::Template => {
            return Some(Reading::fixed(AttributeValue::File(
                object.template.clone(),
            )));
        }
        Attribute::Name => AttributeValue::Text(object.name.clone()),
        Attribute::Class => AttributeValue::Text(object.class.clone()),
        Attribute::Visible => AttributeValue::Bool(object.visible),
        Attribute::Opacity => AttributeValue::Number(object.opacity),
        Attribute::X => AttributeValue::Number(object.pos.x),
        Attribute::Y => AttributeValue::Number(object.pos.y),
        Attribute::Width if has_dimensions(object) => AttributeValue::Number(object.size.x),
        Attribute::Height if has_dimensions(object) => AttributeValue::Number(object.size.y),
        Attribute::Rotation if object.shape != Shape::Point => {
            AttributeValue::Number(object.rotation)
        }
        Attribute::FlipHorizontally => AttributeValue::Bool(flip?.contains(Flip::H)),
        Attribute::FlipVertically => AttributeValue::Bool(flip?.contains(Flip::V)),
        Attribute::Text => AttributeValue::Text(text?.text.clone()),
        Attribute::FontFamily => AttributeValue::Text(text?.font_family.clone()),
        Attribute::PixelSize => AttributeValue::Integer(i64::from(text?.pixel_size)),
        Attribute::WordWrap => AttributeValue::Bool(text?.wrap),
        Attribute::TextColor => AttributeValue::Color(Some(text?.color)),
        Attribute::Bold => AttributeValue::Bool(text?.bold),
        Attribute::Italic => AttributeValue::Bool(text?.italic),
        Attribute::Underline => AttributeValue::Bool(text?.underline),
        Attribute::Strikeout => AttributeValue::Bool(text?.strikeout),
        Attribute::Kerning => AttributeValue::Bool(text?.kerning),
        Attribute::HAlign => AttributeValue::HAlign(text?.halign),
        Attribute::VAlign => AttributeValue::VAlign(text?.valign),
        _ => return None,
    };
    let from_template = object.template.is_some()
        && attribute
            .template_override()
            .is_some_and(|field| !object.overrides.contains(field));
    Some(Reading {
        from_template,
        ..Reading::editable(value)
    })
}

/// Sets one attribute of an object to a value already checked, and marks a template instance
/// as overriding the field.
fn write(
    object: &mut MapObject,
    attribute: Attribute,
    value: &AttributeValue,
) -> Result<bool, PropertyError> {
    let changed = match attribute {
        Attribute::Name => assign(&mut object.name, text_of(attribute, value)?),
        Attribute::Class => assign(&mut object.class, text_of(attribute, value)?),
        Attribute::Visible => assign(&mut object.visible, bool_of(attribute, value)?),
        Attribute::Opacity => assign(&mut object.opacity, number_of(attribute, value)?),
        Attribute::X => assign(&mut object.pos.x, number_of(attribute, value)?),
        Attribute::Y => assign(&mut object.pos.y, number_of(attribute, value)?),
        Attribute::Width | Attribute::Height => {
            let size = number_of(attribute, value)?;
            let field = if attribute == Attribute::Width {
                &mut object.size.x
            } else {
                &mut object.size.y
            };
            // A size overrides the template only when it changes.
            let changed = assign(field, size);
            if changed {
                mark_override(object, ObjectOverrides::SIZE);
            }
            return Ok(changed);
        }
        Attribute::Rotation => assign(&mut object.rotation, number_of(attribute, value)?),
        Attribute::FlipHorizontally | Attribute::FlipVertically => {
            let flag = if attribute == Attribute::FlipHorizontally {
                Flip::H
            } else {
                Flip::V
            };
            let on = bool_of(attribute, value)?;
            let Some(cell) = object.tile else {
                return Err(invalid(attribute, value));
            };
            let mut flip = cell.flip();
            flip.set(flag, on);
            assign(&mut object.tile, Some(cell.with_flip(flip)))
        }
        _ => {
            let Shape::Text(text) = &mut object.shape else {
                return Err(invalid(attribute, value));
            };
            write_text(text, attribute, value)?
        }
    };
    let marked = attribute
        .template_override()
        .is_some_and(|field| mark_override(object, field));
    Ok(changed || marked)
}

/// Sets one of a text object's settings to a value already checked.
fn write_text(
    text: &mut TextData,
    attribute: Attribute,
    value: &AttributeValue,
) -> Result<bool, PropertyError> {
    Ok(match (attribute, value) {
        (Attribute::Text, _) => assign(&mut text.text, text_of(attribute, value)?),
        (Attribute::FontFamily, _) => assign(&mut text.font_family, text_of(attribute, value)?),
        (Attribute::PixelSize, _) => assign(&mut text.pixel_size, integer_of(attribute, value)?),
        (Attribute::WordWrap, _) => assign(&mut text.wrap, bool_of(attribute, value)?),
        (Attribute::TextColor, AttributeValue::Color(Some(color))) => {
            assign(&mut text.color, *color)
        }
        (Attribute::Bold, _) => assign(&mut text.bold, bool_of(attribute, value)?),
        (Attribute::Italic, _) => assign(&mut text.italic, bool_of(attribute, value)?),
        (Attribute::Underline, _) => assign(&mut text.underline, bool_of(attribute, value)?),
        (Attribute::Strikeout, _) => assign(&mut text.strikeout, bool_of(attribute, value)?),
        (Attribute::Kerning, _) => assign(&mut text.kerning, bool_of(attribute, value)?),
        (Attribute::HAlign, AttributeValue::HAlign(align)) => assign(&mut text.halign, *align),
        (Attribute::VAlign, AttributeValue::VAlign(align)) => assign(&mut text.valign, *align),
        _ => return Err(invalid(attribute, value)),
    })
}

/// Marks a template instance as overriding `field` of its template; whether that is new.
/// Objects that are not template instances have nothing to override.
fn mark_override(object: &mut MapObject, field: ObjectOverrides) -> bool {
    if object.template.is_none() || object.overrides.contains(field) {
        return false;
    }
    object.overrides |= field;
    true
}

#[cfg(test)]
mod tests {
    use bt_core::{Cell, Color, FileRef, HAlign, TileId, TilesetSlot, VAlign};
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::{DVec2, UVec2};

    use super::*;

    /// A rectangle (1), a point (2), a tile object (3), a text object (4) and a template
    /// instance of a rectangle template (5).
    fn map() -> Map {
        let atlas = Atlas::builder().build_embedded().unwrap();
        let tile = Cell::new(TilesetSlot::new(0).unwrap(), TileId(2));
        let objects = vec![
            MapObject {
                name: "box".into(),
                pos: DVec2::new(8.0, 8.0),
                size: DVec2::new(16.0, 16.0),
                ..MapObject::default()
            },
            MapObject {
                shape: Shape::Point,
                pos: DVec2::new(3.0, 4.0),
                ..MapObject::default()
            },
            MapObject {
                tile: Some(tile),
                size: DVec2::new(16.0, 16.0),
                ..MapObject::default()
            },
            MapObject {
                shape: Shape::Text(TextData {
                    text: "Hello".into(),
                    ..TextData::default()
                }),
                size: DVec2::new(64.0, 16.0),
                ..MapObject::default()
            },
            MapObject {
                name: "door".into(),
                size: DVec2::new(32.0, 32.0),
                template: Some(FileRef::new("door.tx", "/door.tx")),
                ..MapObject::default()
            },
        ];
        MapBuilder::new(UVec2::new(16, 16), UVec2::new(8, 8))
            .tileset(atlas.tileset)
            .object_layer("things", objects)
            .build()
            .unwrap()
    }

    fn ids(list: &[u32]) -> Vec<ObjectId> {
        list.iter().map(|id| ObjectId(*id)).collect()
    }

    fn row(rows: &[AttributeRow], attribute: Attribute) -> Option<&AttributeRow> {
        rows.iter().find(|row| row.attribute == attribute)
    }

    #[test]
    fn several_objects_show_what_their_shapes_have() {
        let map = map();
        let rows = object_attributes(&map, &ids(&[1, 2, 3, 4, 5])).unwrap();
        assert!(row(&rows, Attribute::Id).unwrap().read_only);
        let template = row(&rows, Attribute::Template).unwrap();
        assert!(template.read_only && template.mixed);
        assert_eq!(row(&rows, Attribute::Width).unwrap().holders, [0, 2, 3, 4]);
        assert_eq!(
            row(&rows, Attribute::Rotation).unwrap().holders,
            [0, 2, 3, 4],
            "points do not turn"
        );
        assert_eq!(
            row(&rows, Attribute::FlipHorizontally).unwrap().holders,
            [2]
        );
        assert_eq!(row(&rows, Attribute::Text).unwrap().holders, [3]);
        let name = row(&rows, Attribute::Name).unwrap();
        assert!(name.mixed);
        assert!(!name.from_template, "only one of them is an instance");
        let only_instance = object_attributes(&map, &ids(&[5])).unwrap();
        assert!(row(&only_instance, Attribute::Name).unwrap().from_template);
        assert!(!row(&only_instance, Attribute::X).unwrap().from_template);
        assert_eq!(
            object_attributes(&map, &ids(&[9])).unwrap_err(),
            PropertyError::NoSuchObject(ObjectId(9))
        );
    }

    #[test]
    fn a_component_set_on_several_objects_applies_to_each() {
        let mut map = map();
        let all = ids(&[1, 2, 3, 4, 5]);
        let mut set = |attribute, value| set_object_attribute(&mut map, &all, attribute, &value);
        assert_eq!(set(Attribute::X, AttributeValue::Number(40.0)), Ok(true));
        assert_eq!(
            set(Attribute::Width, AttributeValue::Number(10.0)),
            Ok(true)
        );
        assert_eq!(
            set(Attribute::Rotation, AttributeValue::Number(45.0)),
            Ok(true)
        );
        for id in &all {
            let object = map.object(*id).unwrap();
            assert_eq!(object.pos.x, 40.0, "x set");
            assert_ne!(object.pos.y, 40.0, "y kept");
        }
        assert_eq!(map.object(ObjectId(1)).unwrap().size.x, 10.0);
        assert_eq!(
            map.object(ObjectId(2)).unwrap().size.x,
            0.0,
            "a point has no size"
        );
        assert_eq!(map.object(ObjectId(2)).unwrap().rotation, 0.0);
        assert_eq!(map.object(ObjectId(3)).unwrap().rotation, 45.0);
    }

    #[test]
    fn setting_an_attribute_of_a_template_instance_overrides_the_template() {
        let mut map = map();
        let both = ids(&[1, 5]);
        let mut set = |attribute, value| set_object_attribute(&mut map, &both, attribute, &value);
        assert_eq!(
            set(Attribute::Name, AttributeValue::Text("door".into())),
            Ok(true),
            "the plain object's name changes, the instance overrides its template's"
        );
        assert_eq!(
            set(Attribute::Width, AttributeValue::Number(32.0)),
            Ok(true),
            "the plain object's width changes"
        );
        assert_eq!(
            set(Attribute::Height, AttributeValue::Number(8.0)),
            Ok(true)
        );
        assert_eq!(
            set(Attribute::Opacity, AttributeValue::Number(0.5)),
            Ok(true)
        );
        assert_eq!(set(Attribute::Y, AttributeValue::Number(1.0)), Ok(true));
        let instance = map.object(ObjectId(5)).unwrap();
        assert_eq!(
            instance.overrides,
            ObjectOverrides::NAME | ObjectOverrides::SIZE | ObjectOverrides::OPACITY,
            "the name marked though it was already this, as Tiled does; the size only once it \
             changed; the position is the instance's own anyway"
        );
        assert!(
            map.object(ObjectId(1)).unwrap().overrides.is_empty(),
            "a plain object overrides nothing"
        );
        assert!(map.validate().is_empty());
        let mut map = self::map();
        assert_eq!(
            set_object_attribute(
                &mut map,
                &ids(&[5]),
                Attribute::Width,
                &AttributeValue::Number(32.0)
            ),
            Ok(false),
            "the same size neither changes nor overrides"
        );
        assert!(map.object(ObjectId(5)).unwrap().overrides.is_empty());
        assert_eq!(
            set_object_attribute(
                &mut map,
                &ids(&[5]),
                Attribute::Visible,
                &AttributeValue::Bool(true)
            ),
            Ok(true),
            "the same visibility overrides"
        );
        assert_eq!(
            map.object(ObjectId(5)).unwrap().overrides,
            ObjectOverrides::VISIBLE
        );
    }

    #[test]
    fn tile_objects_flip_and_text_objects_set_their_text() {
        let mut map = map();
        assert_eq!(
            set_object_attribute(
                &mut map,
                &ids(&[3]),
                Attribute::FlipVertically,
                &AttributeValue::Bool(true)
            ),
            Ok(true)
        );
        let cell = map.object(ObjectId(3)).unwrap().tile.unwrap();
        assert_eq!(cell.flip(), Flip::V);
        assert_eq!(cell.tile(), TileId(2), "the same tile");
        let text = ids(&[4]);
        let mut set = |attribute, value| set_object_attribute(&mut map, &text, attribute, &value);
        assert_eq!(
            set(Attribute::Text, AttributeValue::Text("Bye".into())),
            Ok(true)
        );
        assert_eq!(
            set(Attribute::PixelSize, AttributeValue::Integer(24)),
            Ok(true)
        );
        assert_eq!(set(Attribute::Bold, AttributeValue::Bool(true)), Ok(true));
        assert_eq!(
            set(Attribute::HAlign, AttributeValue::HAlign(HAlign::Center)),
            Ok(true)
        );
        assert_eq!(
            set(Attribute::VAlign, AttributeValue::VAlign(VAlign::Bottom)),
            Ok(true)
        );
        assert_eq!(
            set(
                Attribute::TextColor,
                AttributeValue::Color(Some(Color::rgb(9, 9, 9)))
            ),
            Ok(true)
        );
        assert_eq!(
            set(Attribute::PixelSize, AttributeValue::Integer(0)),
            Err(PropertyError::OutOfRange {
                attribute: Attribute::PixelSize,
                min: Some(1.0),
                max: Some(f64::from(i32::MAX))
            })
        );
        assert_eq!(
            set(Attribute::TextColor, AttributeValue::Color(None)),
            Err(PropertyError::InvalidValue {
                attribute: Attribute::TextColor,
                value: AttributeValue::Color(None)
            })
        );
        let Shape::Text(data) = &map.object(ObjectId(4)).unwrap().shape else {
            panic!()
        };
        assert_eq!(data.text, "Bye");
        assert_eq!(data.pixel_size, 24);
        assert!(data.bold);
        assert_eq!(data.halign, HAlign::Center);
        assert_eq!(data.valign, VAlign::Bottom);
        assert_eq!(data.color, Color::rgb(9, 9, 9));
    }

    #[test]
    fn refused_values_change_no_object() {
        let mut map = map();
        let before = map.clone();
        let all = ids(&[1, 2, 3, 4, 5]);
        assert_eq!(
            set_object_attribute(
                &mut map,
                &all,
                Attribute::Opacity,
                &AttributeValue::Number(2.0)
            ),
            Err(PropertyError::OutOfRange {
                attribute: Attribute::Opacity,
                min: Some(0.0),
                max: Some(1.0)
            })
        );
        assert_eq!(
            set_object_attribute(
                &mut map,
                &all,
                Attribute::Width,
                &AttributeValue::Number(-3.0)
            ),
            Err(PropertyError::OutOfRange {
                attribute: Attribute::Width,
                min: Some(0.0),
                max: None
            })
        );
        assert_eq!(
            set_object_attribute(&mut map, &all, Attribute::Id, &AttributeValue::Integer(1)),
            Err(PropertyError::ReadOnly(Attribute::Id))
        );
        assert_eq!(
            set_object_attribute(
                &mut map,
                &ids(&[2]),
                Attribute::Width,
                &AttributeValue::Number(1.0)
            ),
            Err(PropertyError::NotApplicable(Attribute::Width))
        );
        assert_eq!(
            set_object_attribute(
                &mut map,
                &ids(&[1, 7]),
                Attribute::X,
                &AttributeValue::Number(1.0)
            ),
            Err(PropertyError::NoSuchObject(ObjectId(7)))
        );
        assert_eq!(map, before);
    }
}
