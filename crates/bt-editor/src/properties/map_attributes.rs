//! A map's built-in attributes (Tiled's `MapProperties`, `reference/tiled/src/tiled/
//! propertieswidget.cpp`): what the Properties panel shows for the map and how each is set.
//! Attributes that do not apply to the map's orientation, format or bounds are read-only, as
//! Tiled disables them; Infinite makes the map infinite or finite with `bt_edit`'s operations.

use bt_core::{LayerDataFormat, Map, Orientation};
use bt_edit::{make_finite, make_infinite};

use super::attribute::Attribute;
use super::attribute_check::{
    bool_of, check_value, color_of, integer_of, invalid, number_of, text_of,
};
use super::attribute_row::{AttributeRow, Reading, assign, rows_of, settable};
use super::attribute_value::AttributeValue;
use super::error::PropertyError;

/// The map's attributes, in the order Tiled shows them.
const ORDER: [Attribute; 20] = [
    Attribute::Class,
    Attribute::Orientation,
    Attribute::MapWidth,
    Attribute::MapHeight,
    Attribute::Infinite,
    Attribute::TileWidth,
    Attribute::TileHeight,
    Attribute::HexSideLength,
    Attribute::StaggerAxis,
    Attribute::StaggerIndex,
    Attribute::SkewX,
    Attribute::SkewY,
    Attribute::ParallaxOriginX,
    Attribute::ParallaxOriginY,
    Attribute::LayerDataFormat,
    Attribute::CompressionLevel,
    Attribute::ChunkWidth,
    Attribute::ChunkHeight,
    Attribute::RenderOrder,
    Attribute::BackgroundColor,
];

/// The map's built-in attributes as the Properties panel shows them: its class, orientation,
/// size (read-only), whether it is infinite, its tile size, hexagon side, stagger, skew,
/// parallax origin, layer data format, compression level, output chunk size, render order and
/// background colour.
pub fn map_attributes(map: &Map) -> Vec<AttributeRow> {
    rows_of(&ORDER, &[map], |map, attribute| read(map, attribute))
}

/// Sets one of the map's built-in attributes; returns whether the map changed. Refused when the
/// value is of the wrong kind or out of range, or the attribute is read-only for this map.
/// Turning a map finite crops it to its tiles, moving its content to start at the first cell
/// (`bt_edit::make_finite`, which reports how far): a tile selection does not follow.
pub fn set_map_attribute(
    map: &mut Map,
    attribute: Attribute,
    value: &AttributeValue,
) -> Result<bool, PropertyError> {
    check_value(attribute, value)?;
    settable(attribute, &[&*map], read)?;
    write(map, attribute, value)
}

/// One attribute of the map; `None` for one a map does not have.
fn read(map: &Map, attribute: Attribute) -> Option<Reading> {
    let orientation = map.orientation;
    let staggered = matches!(orientation, Orientation::Staggered | Orientation::Hexagonal);
    let compressed = matches!(
        map.file.layer_data,
        LayerDataFormat::Base64Gzip | LayerDataFormat::Base64Zlib | LayerDataFormat::Base64Zstd
    );
    let integer = |number: i64| AttributeValue::Integer(number);
    Some(match attribute {
        Attribute::Class => Reading::editable(AttributeValue::Text(map.class.clone())),
        Attribute::Orientation => Reading::editable(AttributeValue::Orientation(orientation)),
        Attribute::MapWidth => Reading::fixed(integer(i64::from(map.width))),
        Attribute::MapHeight => Reading::fixed(integer(i64::from(map.height))),
        Attribute::Infinite => Reading::editable(AttributeValue::Bool(map.infinite)),
        Attribute::TileWidth => Reading::editable(integer(i64::from(map.tile_width))),
        Attribute::TileHeight => Reading::editable(integer(i64::from(map.tile_height))),
        Attribute::HexSideLength => Reading::editable_if(
            integer(i64::from(map.hex_side_length)),
            orientation == Orientation::Hexagonal,
        ),
        Attribute::StaggerAxis => {
            Reading::editable_if(AttributeValue::StaggerAxis(map.stagger_axis), staggered)
        }
        Attribute::StaggerIndex => {
            Reading::editable_if(AttributeValue::StaggerIndex(map.stagger_index), staggered)
        }
        Attribute::SkewX => Reading::editable_if(
            integer(i64::from(map.skew.x)),
            orientation == Orientation::Oblique,
        ),
        Attribute::SkewY => Reading::editable_if(
            integer(i64::from(map.skew.y)),
            orientation == Orientation::Oblique,
        ),
        Attribute::ParallaxOriginX => {
            Reading::editable(AttributeValue::Number(map.parallax_origin.x))
        }
        Attribute::ParallaxOriginY => {
            Reading::editable(AttributeValue::Number(map.parallax_origin.y))
        }
        Attribute::LayerDataFormat => {
            Reading::editable(AttributeValue::LayerDataFormat(map.file.layer_data))
        }
        Attribute::CompressionLevel => Reading::editable_if(
            integer(i64::from(map.file.compression_level.unwrap_or(-1))),
            compressed,
        ),
        Attribute::ChunkWidth => Reading::editable_if(
            integer(i64::from(map.file.output_chunk_size.x)),
            map.infinite,
        ),
        Attribute::ChunkHeight => Reading::editable_if(
            integer(i64::from(map.file.output_chunk_size.y)),
            map.infinite,
        ),
        Attribute::RenderOrder => Reading::editable_if(
            AttributeValue::RenderOrder(map.render_order),
            matches!(orientation, Orientation::Orthogonal | Orientation::Oblique),
        ),
        Attribute::BackgroundColor => {
            Reading::editable(AttributeValue::Color(map.background_color))
        }
        _ => return None,
    })
}

/// Sets one attribute of the map to a value already checked.
fn write(
    map: &mut Map,
    attribute: Attribute,
    value: &AttributeValue,
) -> Result<bool, PropertyError> {
    Ok(match (attribute, value) {
        (Attribute::Class, _) => assign(&mut map.class, text_of(attribute, value)?),
        (Attribute::Orientation, AttributeValue::Orientation(orientation)) => {
            assign(&mut map.orientation, *orientation)
        }
        (Attribute::Infinite, _) => {
            let infinite = bool_of(attribute, value)?;
            if infinite == map.infinite {
                return Ok(false);
            }
            if infinite {
                make_infinite(map)?;
            } else {
                make_finite(map)?;
            }
            true
        }
        (Attribute::TileWidth, _) => assign(&mut map.tile_width, integer_of(attribute, value)?),
        (Attribute::TileHeight, _) => assign(&mut map.tile_height, integer_of(attribute, value)?),
        (Attribute::HexSideLength, _) => {
            assign(&mut map.hex_side_length, integer_of(attribute, value)?)
        }
        (Attribute::StaggerAxis, AttributeValue::StaggerAxis(axis)) => {
            assign(&mut map.stagger_axis, *axis)
        }
        (Attribute::StaggerIndex, AttributeValue::StaggerIndex(index)) => {
            assign(&mut map.stagger_index, *index)
        }
        (Attribute::SkewX, _) => assign(&mut map.skew.x, integer_of(attribute, value)?),
        (Attribute::SkewY, _) => assign(&mut map.skew.y, integer_of(attribute, value)?),
        (Attribute::ParallaxOriginX, _) => {
            assign(&mut map.parallax_origin.x, number_of(attribute, value)?)
        }
        (Attribute::ParallaxOriginY, _) => {
            assign(&mut map.parallax_origin.y, number_of(attribute, value)?)
        }
        (Attribute::LayerDataFormat, AttributeValue::LayerDataFormat(format)) => {
            assign(&mut map.file.layer_data, *format)
        }
        (Attribute::CompressionLevel, _) => {
            let level: i32 = integer_of(attribute, value)?;
            // zlib and gzip go to 9; Zstandard to 22.
            let most = if map.file.layer_data == LayerDataFormat::Base64Zstd {
                22
            } else {
                9
            };
            if level > most {
                return Err(PropertyError::OutOfRange {
                    attribute,
                    min: Some(-1.0),
                    max: Some(f64::from(most)),
                });
            }
            assign(
                &mut map.file.compression_level,
                (level != -1).then_some(level),
            )
        }
        (Attribute::ChunkWidth, _) => assign(
            &mut map.file.output_chunk_size.x,
            integer_of(attribute, value)?,
        ),
        (Attribute::ChunkHeight, _) => assign(
            &mut map.file.output_chunk_size.y,
            integer_of(attribute, value)?,
        ),
        (Attribute::RenderOrder, AttributeValue::RenderOrder(order)) => {
            assign(&mut map.render_order, *order)
        }
        (Attribute::BackgroundColor, _) => {
            assign(&mut map.background_color, color_of(attribute, value)?)
        }
        _ => return Err(invalid(attribute, value)),
    })
}

#[cfg(test)]
mod tests {
    use bt_core::{Color, LayerKind, StaggerAxis};
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::UVec2;

    use super::*;

    fn map() -> Map {
        let atlas = Atlas::builder().build_embedded().unwrap();
        MapBuilder::new(UVec2::new(16, 16), UVec2::new(4, 3))
            .tileset(atlas.tileset)
            .tile_layer("ground", ". 1 .\n. . 2")
            .build()
            .unwrap()
    }

    fn row(map: &Map, attribute: Attribute) -> AttributeRow {
        map_attributes(map)
            .into_iter()
            .find(|row| row.attribute == attribute)
            .unwrap()
    }

    #[test]
    fn the_map_shows_tileds_attributes_with_what_does_not_apply_read_only() {
        let map = map();
        let rows = map_attributes(&map);
        assert_eq!(rows.len(), 20);
        assert_eq!(rows[0].attribute, Attribute::Class);
        assert_eq!(
            row(&map, Attribute::MapWidth).value,
            AttributeValue::Integer(4)
        );
        assert!(row(&map, Attribute::MapWidth).read_only);
        assert!(row(&map, Attribute::HexSideLength).read_only, "orthogonal");
        assert!(row(&map, Attribute::StaggerAxis).read_only);
        assert!(row(&map, Attribute::SkewX).read_only);
        assert!(!row(&map, Attribute::RenderOrder).read_only);
        assert!(row(&map, Attribute::ChunkWidth).read_only, "finite");
        assert!(!row(&map, Attribute::CompressionLevel).read_only, "zlib");
        assert_eq!(
            row(&map, Attribute::CompressionLevel).value,
            AttributeValue::Integer(-1)
        );
        assert!(!row(&map, Attribute::TileWidth).mixed);
    }

    #[test]
    fn attributes_set_and_say_whether_they_changed() {
        let mut map = map();
        let set = |map: &mut Map, attribute, value| set_map_attribute(map, attribute, &value);
        assert_eq!(
            set(&mut map, Attribute::TileWidth, AttributeValue::Integer(32)),
            Ok(true)
        );
        assert_eq!(map.tile_width, 32);
        assert_eq!(
            set(&mut map, Attribute::TileWidth, AttributeValue::Integer(32)),
            Ok(false)
        );
        assert_eq!(
            set(
                &mut map,
                Attribute::BackgroundColor,
                AttributeValue::Color(Some(Color::rgb(1, 2, 3)))
            ),
            Ok(true)
        );
        assert_eq!(map.background_color, Some(Color::rgb(1, 2, 3)));
        assert_eq!(
            set(
                &mut map,
                Attribute::HexSideLength,
                AttributeValue::Integer(8)
            ),
            Err(PropertyError::ReadOnly(Attribute::HexSideLength))
        );
        assert_eq!(
            set(
                &mut map,
                Attribute::Orientation,
                AttributeValue::Orientation(Orientation::Hexagonal)
            ),
            Ok(true)
        );
        assert_eq!(
            set(
                &mut map,
                Attribute::HexSideLength,
                AttributeValue::Integer(8)
            ),
            Ok(true)
        );
        assert_eq!(
            set(
                &mut map,
                Attribute::StaggerAxis,
                AttributeValue::StaggerAxis(StaggerAxis::X)
            ),
            Ok(true)
        );
        assert_eq!(map.stagger_axis, StaggerAxis::X);
        assert_eq!(
            set(&mut map, Attribute::MapWidth, AttributeValue::Integer(10)),
            Err(PropertyError::ReadOnly(Attribute::MapWidth))
        );
        assert_eq!(
            set(&mut map, Attribute::Opacity, AttributeValue::Number(0.5)),
            Err(PropertyError::NotApplicable(Attribute::Opacity))
        );
    }

    #[test]
    fn values_out_of_range_are_refused_and_change_nothing() {
        let mut map = map();
        let before = map.clone();
        assert_eq!(
            set_map_attribute(&mut map, Attribute::TileHeight, &AttributeValue::Integer(0)),
            Err(PropertyError::OutOfRange {
                attribute: Attribute::TileHeight,
                min: Some(1.0),
                max: Some(f64::from(i32::MAX))
            })
        );
        assert_eq!(
            set_map_attribute(
                &mut map,
                Attribute::ParallaxOriginX,
                &AttributeValue::Number(f64::NAN)
            ),
            Err(PropertyError::OutOfRange {
                attribute: Attribute::ParallaxOriginX,
                min: None,
                max: None
            })
        );
        assert_eq!(
            set_map_attribute(
                &mut map,
                Attribute::CompressionLevel,
                &AttributeValue::Integer(12)
            ),
            Err(PropertyError::OutOfRange {
                attribute: Attribute::CompressionLevel,
                min: Some(-1.0),
                max: Some(9.0)
            }),
            "zlib goes to 9"
        );
        assert_eq!(
            set_map_attribute(
                &mut map,
                Attribute::LayerDataFormat,
                &AttributeValue::LayerDataFormat(LayerDataFormat::Xml)
            ),
            Err(PropertyError::InvalidValue {
                attribute: Attribute::LayerDataFormat,
                value: AttributeValue::LayerDataFormat(LayerDataFormat::Xml)
            })
        );
        assert_eq!(map, before);
        assert_eq!(
            set_map_attribute(
                &mut map,
                Attribute::CompressionLevel,
                &AttributeValue::Integer(6)
            ),
            Ok(true)
        );
        assert_eq!(map.file.compression_level, Some(6));
        assert_eq!(
            set_map_attribute(
                &mut map,
                Attribute::CompressionLevel,
                &AttributeValue::Integer(-1)
            ),
            Ok(true)
        );
        assert_eq!(
            map.file.compression_level, None,
            "-1 is the format's default"
        );
    }

    #[test]
    fn infinite_makes_the_map_infinite_and_finite_again() {
        let mut map = map();
        assert_eq!(
            set_map_attribute(&mut map, Attribute::Infinite, &AttributeValue::Bool(true)),
            Ok(true)
        );
        assert!(map.infinite);
        assert!(
            map.validate().is_empty(),
            "every tile layer lost its bounds"
        );
        assert!(!row(&map, Attribute::ChunkWidth).read_only);
        assert_eq!(
            set_map_attribute(
                &mut map,
                Attribute::ChunkWidth,
                &AttributeValue::Integer(32)
            ),
            Ok(true)
        );
        assert_eq!(
            set_map_attribute(&mut map, Attribute::Infinite, &AttributeValue::Bool(false)),
            Ok(true)
        );
        assert!(!map.infinite);
        assert!(map.validate().is_empty());
        assert_eq!(
            (map.width, map.height),
            (2, 2),
            "cropped to its tiles, as making a map finite does"
        );
        let LayerKind::Tile(tiles) = &map.layers[0].kind else {
            panic!()
        };
        assert!(!tiles.is_infinite());
    }
}
