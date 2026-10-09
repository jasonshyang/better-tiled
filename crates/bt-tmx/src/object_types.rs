//! Legacy object types (`objecttypes.xml`, `objecttypes.json`, before Tiled 1.8): each type is
//! a class usable for objects and tiles, with its colour and member defaults. Read only.

use std::path::Path;

use bt_core::{ClassType, ClassUsage, Color, CustomType, CustomTypes};
use roxmltree::Node;

use crate::error::Error;
use crate::finish::PropertyDecoder;
use crate::format::Format;
use crate::json::JsonReader;
use crate::loader::{Loaded, absolute};
use crate::raw::{RawProperty, RawValue, Scalar};
use crate::text::read_text;
use crate::warning::{WarningKind, Warnings};

/// Reads an object types file (XML or JSON, told apart by the contents).
pub fn load_object_types(path: &Path) -> Result<Loaded<CustomTypes>, Error> {
    let path = absolute(path)?;
    let text = read_text(&path)?;
    parse_object_types(&text, &path)
}

/// Reads object types from text; `path` is where they are.
pub fn parse_object_types(text: &str, path: &Path) -> Result<Loaded<CustomTypes>, Error> {
    let mut warnings = Warnings::new(path);
    let entries = match Format::of_file(text, path) {
        Format::Xml => {
            let document = crate::xml::parse_document(text, path)?;
            let root = document.root_element();
            if !root.has_tag_name("objecttypes") {
                return Err(Error::format(
                    path,
                    format!(
                        "not object types: the root element is <{}>",
                        root.tag_name().name()
                    ),
                ));
            }
            xml_entries(root)
        }
        Format::Json | Format::JavaScript => {
            let value = crate::json::parse_document(text, path)?;
            let array = value
                .as_array()
                .ok_or_else(|| Error::format(path, "not object types: not a JSON array"))?;
            let mut reader = JsonReader {
                warnings: &mut warnings,
            };
            array
                .iter()
                .filter_map(|entry| entry.as_object())
                .map(|entry| ObjectTypeEntry {
                    name: reader.text_of(entry, "name"),
                    color: reader.text_of(entry, "color"),
                    members: reader.properties_of(entry),
                })
                .collect()
        }
    };
    let mut types = CustomTypes::new();
    for entry in entries {
        let members =
            PropertyDecoder::new(&CustomTypes::new(), &mut warnings).properties_of(entry.members);
        let color = Color::from_tiled_text(&entry.color).unwrap_or_else(|| {
            if !entry.color.is_empty() {
                warnings.record(
                    WarningKind::InvalidValue,
                    format!(
                        "object type “{}” has the colour “{}”, which is not one",
                        entry.name, entry.color
                    ),
                );
            }
            ClassType::default().color
        });
        types.add(CustomType::class(
            entry.name,
            ClassType {
                members,
                color,
                draw_fill: true,
                use_as: ClassUsage::OBJECT | ClassUsage::TILE,
            },
        ));
    }
    Ok(Loaded {
        value: types,
        warnings: warnings.into_list(),
    })
}

/// An object type as written.
struct ObjectTypeEntry {
    name: String,
    color: String,
    members: Vec<RawProperty>,
}

/// The `<objecttype>`s: `<property name type propertytype default>` members.
fn xml_entries(root: Node) -> Vec<ObjectTypeEntry> {
    root.children()
        .filter(|node| node.has_tag_name("objecttype"))
        .map(|node| ObjectTypeEntry {
            name: node.attribute("name").unwrap_or("").to_owned(),
            color: node.attribute("color").unwrap_or("").to_owned(),
            members: node
                .children()
                .filter(|child| child.has_tag_name("property"))
                .map(|property| RawProperty {
                    name: property.attribute("name").unwrap_or("").to_owned(),
                    type_name: property.attribute("type").map(str::to_owned),
                    custom_type: property.attribute("propertytype").map(str::to_owned),
                    value: RawValue::Scalar(Scalar::text(
                        property.attribute("default").unwrap_or(""),
                    )),
                })
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use bt_core::{Property, Value};

    use super::*;

    #[test]
    fn both_forms_become_classes_for_objects_and_tiles() {
        let xml = r##"<?xml version="1.0"?>
<objecttypes>
 <objecttype name="Spawn" color="#ff0000">
  <property name="hp" type="int" default="3"/>
  <property name="note" type="string"/>
 </objecttype>
</objecttypes>"##;
        let json = r##"[{"name": "Spawn", "color": "#ff0000", "properties": [
            {"name": "hp", "type": "int", "value": 3}, {"name": "note", "type": "string", "value": ""}]}]"##;
        for (text, file) in [(xml, "/o/objecttypes.xml"), (json, "/o/objecttypes.json")] {
            let types = parse_object_types(text, Path::new(file)).unwrap().value;
            let class = types.class_for("Spawn", ClassUsage::TILE).unwrap();
            assert_eq!(class.color, Color::rgb(255, 0, 0));
            assert_eq!(class.members["hp"], Property::new(Value::Int(3)));
            assert_eq!(
                class.members["note"],
                Property::new(Value::String(String::new()))
            );
            assert!(types.class_for("Spawn", ClassUsage::MAP).is_none());
        }
    }
}
