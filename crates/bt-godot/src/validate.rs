//! Rejects values Godot cannot represent before any output is planned.
use bt_core::{Map, MapObject, Properties, Shape, TilesetImage, Value};

pub(crate) fn map(map: &Map) -> Result<(), String> {
    properties(&map.properties)?;
    if !map.parallax_origin.is_finite() {
        return Err("map parallax origin must be finite".into());
    }
    let mut failure = None;
    map.for_each_layer(|layer, _| {
        let result = (|| {
            if !layer.offset.is_finite()
                || !layer.parallax.is_finite()
                || !layer.opacity.is_finite()
            {
                return Err(format!(
                    "layer {}: offset, parallax and opacity must be finite",
                    layer.id
                ));
            }
            properties(&layer.properties)?;
            if let bt_core::LayerKind::Object(objects) = &layer.kind {
                for object in &*objects.objects {
                    check_object(object)?;
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            failure = Some(error);
        }
    });
    if let Some(error) = failure {
        return Err(error);
    }
    for reference in &map.tilesets {
        let tileset = &reference.tileset;
        if let TilesetImage::Atlas {
            columns,
            tile_count,
            ..
        } = &tileset.image
            && *tile_count > 0
            && (*columns == 0 || tileset.tile_width == 0 || tileset.tile_height == 0)
        {
            return Err(format!(
                "tileset {}: atlas tiles need positive dimensions and columns",
                tileset.name
            ));
        }
        properties(&tileset.properties)?;
        for tile in tileset.tiles.values() {
            if !tile.probability.is_finite() || tile.probability < 0.0 {
                return Err(format!(
                    "tileset {}: tile probability must be finite and nonnegative",
                    tileset.name
                ));
            }
            properties(&tile.properties)?;
            if let Some(collision) = &tile.collision {
                for object in &*collision.layer.objects {
                    check_object(object)?;
                }
            }
        }
        for set in &tileset.wang_sets {
            for color in &set.colors {
                if !color.probability.is_finite() {
                    return Err(format!(
                        "terrain {}: probability must be finite",
                        color.name
                    ));
                }
            }
        }
    }
    Ok(())
}
fn check_object(object: &MapObject) -> Result<(), String> {
    if !object.pos.is_finite()
        || !object.size.is_finite()
        || !object.rotation.is_finite()
        || !object.opacity.is_finite()
    {
        return Err(format!(
            "object {}: coordinates, size, rotation and opacity must be finite",
            object.id
        ));
    }
    if let Shape::Polygon(points) | Shape::Polyline(points) = &object.shape
        && points.iter().any(|p| !p.is_finite())
    {
        return Err(format!(
            "object {}: polygon points must be finite",
            object.id
        ));
    }
    properties(&object.properties)
}
fn properties(properties: &Properties) -> Result<(), String> {
    for (name, property) in properties {
        check_value(&property.value).map_err(|e| format!("property {name}: {e}"))?;
    }
    Ok(())
}
fn check_value(value: &Value) -> Result<(), String> {
    match value {
        Value::Float(number) if !number.is_finite() => Err("float must be finite".into()),
        Value::Class(members) => properties(members),
        Value::List(items) => {
            for item in items {
                check_value(&item.value)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
