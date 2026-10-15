//! A value moving to another folder (Save As, embedding a tileset in a map): `file` properties
//! are written relative to their file, so they are rewritten for the new folder. References to
//! tilesets, images and templates need nothing: they keep their resolved paths.

use std::path::Path;

use bt_core::{Layer, LayerKind, Map, Properties, Tileset, Value};

use crate::paths::rebased_file_value;

/// Rewrites the `file` properties of a map and of the tilesets embedded in it, written relative
/// to `from_dir`, relative to `to_dir`.
pub fn rebase_file_references(map: &mut Map, from_dir: &Path, to_dir: &Path) {
    if from_dir == to_dir {
        return;
    }
    rebase_properties(&mut map.properties, from_dir, to_dir);
    rebase_layers(&mut map.layers, from_dir, to_dir);
    for tileset in &mut map.tilesets {
        if tileset.file.is_none() {
            rebase_tileset(
                std::sync::Arc::make_mut(&mut tileset.tileset),
                from_dir,
                to_dir,
            );
        }
    }
}

/// Rewrites project properties and file-valued class defaults for a new project filename.
pub fn rebase_project_references(project: &mut bt_core::Project, from_dir: &Path, to_dir: &Path) {
    rebase_properties(&mut project.properties, from_dir, to_dir);
    for custom in &mut project.custom_types.types {
        if let bt_core::CustomTypeKind::Class(class) = &mut custom.kind {
            rebase_properties(&mut class.members, from_dir, to_dir);
        }
    }
}

/// Rewrites a tileset's `file` properties (its own, its tiles', their collision shapes', its
/// terrain sets' and terrains').
pub(crate) fn rebase_tileset(tileset: &mut Tileset, from_dir: &Path, to_dir: &Path) {
    rebase_properties(&mut tileset.properties, from_dir, to_dir);
    for tile in tileset.tiles.values_mut() {
        rebase_properties(&mut tile.properties, from_dir, to_dir);
        if let Some(group) = &mut tile.collision {
            for object in std::sync::Arc::make_mut(&mut group.layer.objects) {
                rebase_properties(&mut object.properties, from_dir, to_dir);
            }
        }
    }
    for set in &mut tileset.wang_sets {
        rebase_properties(&mut set.properties, from_dir, to_dir);
        for color in &mut set.colors {
            rebase_properties(&mut color.properties, from_dir, to_dir);
        }
    }
}

fn rebase_layers(layers: &mut [Layer], from_dir: &Path, to_dir: &Path) {
    for layer in layers {
        rebase_properties(&mut layer.properties, from_dir, to_dir);
        match &mut layer.kind {
            LayerKind::Object(objects) => {
                for object in std::sync::Arc::make_mut(&mut objects.objects) {
                    rebase_properties(&mut object.properties, from_dir, to_dir);
                }
            }
            LayerKind::Group(children) => rebase_layers(children, from_dir, to_dir),
            LayerKind::Tile(_) | LayerKind::Image(_) => {}
        }
    }
}

/// Rewrites nested file values in one property collection for a new owning folder.
pub fn rebase_properties(properties: &mut Properties, from_dir: &Path, to_dir: &Path) {
    for property in properties.values_mut() {
        rebase_value(&mut property.value, from_dir, to_dir);
    }
}

fn rebase_value(value: &mut Value, from_dir: &Path, to_dir: &Path) {
    match value {
        Value::File(file) => {
            if let Some(rebased) = rebased_file_value(file, from_dir, to_dir) {
                *file = rebased;
            }
        }
        Value::Class(members) => rebase_properties(members, from_dir, to_dir),
        Value::List(items) => {
            for item in items {
                rebase_value(&mut item.value, from_dir, to_dir);
            }
        }
        Value::String(_)
        | Value::Int(_)
        | Value::Float(_)
        | Value::Bool(_)
        | Value::Color(_)
        | Value::Object(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use bt_core::Property;

    use super::*;

    #[test]
    fn file_properties_follow_the_map() {
        let mut map = Map::default();
        map.properties
            .insert("script".into(), Property::new(Value::File("a.lua".into())));
        map.properties.insert(
            "nested".into(),
            Property::new(Value::List(vec![Property::new(Value::File(
                "../b.png".into(),
            ))])),
        );
        rebase_file_references(&mut map, Path::new("/p/maps"), Path::new("/p"));
        assert_eq!(
            map.properties["script"].value,
            Value::File("maps/a.lua".into())
        );
        let Value::List(items) = &map.properties["nested"].value else {
            panic!()
        };
        // "../b.png" from /p/maps is /p/b.png, which is "b.png" from /p.
        assert_eq!(items[0].value, Value::File("b.png".into()));
    }
}
