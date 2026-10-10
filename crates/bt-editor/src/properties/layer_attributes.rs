//! Layers' built-in attributes (Tiled's `LayerProperties`, `ObjectGroupProperties` and
//! `ImageLayerProperties`, `reference/tiled/src/tiled/propertieswidget.cpp`): what every layer
//! has, an object layer's colour and draw order, an image layer's image, transparent colour and
//! repetition. With several layers, an attribute only some kinds have applies to those.

use bt_core::{ImageRef, Layer, LayerId, LayerKind, Map};

use super::attribute::Attribute;
use super::attribute_check::{
    bool_of, check_value, color_of, image_of, invalid, number_of, text_of,
};
use super::attribute_row::{AttributeRow, Reading, assign, rows_of, settable};
use super::attribute_value::AttributeValue;
use super::error::PropertyError;

/// The layers' attributes, in the order Tiled shows them.
const ORDER: [Attribute; 18] = [
    Attribute::Id,
    Attribute::Name,
    Attribute::Class,
    Attribute::Visible,
    Attribute::Locked,
    Attribute::Opacity,
    Attribute::TintColor,
    Attribute::BlendMode,
    Attribute::OffsetX,
    Attribute::OffsetY,
    Attribute::ParallaxX,
    Attribute::ParallaxY,
    Attribute::Color,
    Attribute::DrawOrder,
    Attribute::Image,
    Attribute::TransparentColor,
    Attribute::RepeatX,
    Attribute::RepeatY,
];

/// The built-in attributes of the map's layers `layers` as the Properties panel shows them: id
/// (read-only), name, class, visibility, lock, opacity, tint, blend mode, offset and parallax
/// factor; an object layer's colour and draw order; an image layer's image, transparent colour
/// (read-only without an image) and repetition. A layer the map does not have is an error.
pub fn layer_attributes(map: &Map, layers: &[LayerId]) -> Result<Vec<AttributeRow>, PropertyError> {
    let found = layers
        .iter()
        .map(|id| map.layer(*id).ok_or(PropertyError::NoSuchLayer(*id)))
        .collect::<Result<Vec<&Layer>, PropertyError>>()?;
    Ok(rows_of(&ORDER, &found, |layer, attribute| {
        read(layer, attribute)
    }))
}

/// Sets one built-in attribute of the map's layers `layers`: of those that have it and can set
/// it. Returns whether any changed. Refused, changing nothing, when the value is of the wrong
/// kind or out of range, no layer has the attribute or none can set it, or a layer is missing.
pub fn set_layer_attribute(
    map: &mut Map,
    layers: &[LayerId],
    attribute: Attribute,
    value: &AttributeValue,
) -> Result<bool, PropertyError> {
    check_value(attribute, value)?;
    let found = layers
        .iter()
        .map(|id| map.layer(*id).ok_or(PropertyError::NoSuchLayer(*id)))
        .collect::<Result<Vec<&Layer>, PropertyError>>()?;
    let ids: Vec<LayerId> = settable(attribute, &found, read)?
        .into_iter()
        .map(|layer| layer.id)
        .collect();
    let mut changed = false;
    for id in ids {
        let layer = map.layer_mut(id).ok_or(PropertyError::NoSuchLayer(id))?;
        changed |= write(layer, attribute, value)?;
    }
    Ok(changed)
}

/// One attribute of a layer; `None` for one its kind does not have.
fn read(layer: &Layer, attribute: Attribute) -> Option<Reading> {
    let value = match attribute {
        Attribute::Id => {
            return Some(Reading::fixed(AttributeValue::Integer(i64::from(
                layer.id.0,
            ))));
        }
        Attribute::Name => AttributeValue::Text(layer.name.clone()),
        Attribute::Class => AttributeValue::Text(layer.class.clone()),
        Attribute::Visible => AttributeValue::Bool(layer.visible),
        Attribute::Locked => AttributeValue::Bool(layer.locked),
        Attribute::Opacity => AttributeValue::Number(layer.opacity),
        Attribute::TintColor => AttributeValue::Color(layer.tint),
        Attribute::BlendMode => AttributeValue::BlendMode(layer.blend_mode),
        Attribute::OffsetX => AttributeValue::Number(layer.offset.x),
        Attribute::OffsetY => AttributeValue::Number(layer.offset.y),
        Attribute::ParallaxX => AttributeValue::Number(layer.parallax.x),
        Attribute::ParallaxY => AttributeValue::Number(layer.parallax.y),
        Attribute::Color => AttributeValue::Color(layer.as_object_layer()?.color),
        Attribute::DrawOrder => AttributeValue::DrawOrder(layer.as_object_layer()?.draw_order),
        Attribute::Image => AttributeValue::Image(layer.as_image_layer()?.image.clone()),
        Attribute::TransparentColor => {
            let image = &layer.as_image_layer()?.image;
            return Some(Reading::editable_if(
                AttributeValue::Color(image.as_ref().and_then(|image| image.transparent)),
                image.is_some(),
            ));
        }
        Attribute::RepeatX => AttributeValue::Bool(layer.as_image_layer()?.repeat_x),
        Attribute::RepeatY => AttributeValue::Bool(layer.as_image_layer()?.repeat_y),
        _ => return None,
    };
    Some(Reading::editable(value))
}

/// Sets one attribute of a layer to a value already checked.
fn write(
    layer: &mut Layer,
    attribute: Attribute,
    value: &AttributeValue,
) -> Result<bool, PropertyError> {
    Ok(match (attribute, value, &mut layer.kind) {
        (Attribute::Name, _, _) => assign(&mut layer.name, text_of(attribute, value)?),
        (Attribute::Class, _, _) => assign(&mut layer.class, text_of(attribute, value)?),
        (Attribute::Visible, _, _) => assign(&mut layer.visible, bool_of(attribute, value)?),
        (Attribute::Locked, _, _) => assign(&mut layer.locked, bool_of(attribute, value)?),
        (Attribute::Opacity, _, _) => assign(&mut layer.opacity, number_of(attribute, value)?),
        (Attribute::TintColor, _, _) => assign(&mut layer.tint, color_of(attribute, value)?),
        (Attribute::BlendMode, AttributeValue::BlendMode(mode), _) => {
            assign(&mut layer.blend_mode, *mode)
        }
        (Attribute::OffsetX, _, _) => assign(&mut layer.offset.x, number_of(attribute, value)?),
        (Attribute::OffsetY, _, _) => assign(&mut layer.offset.y, number_of(attribute, value)?),
        (Attribute::ParallaxX, _, _) => assign(&mut layer.parallax.x, number_of(attribute, value)?),
        (Attribute::ParallaxY, _, _) => assign(&mut layer.parallax.y, number_of(attribute, value)?),
        (Attribute::Color, _, LayerKind::Object(objects)) => {
            assign(&mut objects.color, color_of(attribute, value)?)
        }
        (Attribute::DrawOrder, AttributeValue::DrawOrder(order), LayerKind::Object(objects)) => {
            assign(&mut objects.draw_order, *order)
        }
        (Attribute::Image, _, LayerKind::Image(image_layer)) => {
            // The image's transparent colour is the layer's, whatever image it shows.
            let transparent = image_layer
                .image
                .as_ref()
                .and_then(|image| image.transparent);
            let image = image_of(attribute, value)?.map(|image| ImageRef {
                transparent,
                ..image
            });
            assign(&mut image_layer.image, image)
        }
        (Attribute::TransparentColor, _, LayerKind::Image(image_layer)) => {
            let color = color_of(attribute, value)?;
            match &mut image_layer.image {
                Some(image) => assign(&mut image.transparent, color),
                None => return Err(PropertyError::ReadOnly(attribute)),
            }
        }
        (Attribute::RepeatX, _, LayerKind::Image(image_layer)) => {
            assign(&mut image_layer.repeat_x, bool_of(attribute, value)?)
        }
        (Attribute::RepeatY, _, LayerKind::Image(image_layer)) => {
            assign(&mut image_layer.repeat_y, bool_of(attribute, value)?)
        }
        _ => return Err(invalid(attribute, value)),
    })
}

#[cfg(test)]
mod tests {
    use bt_core::{BlendMode, Color, DrawOrder, FileRef, ImageLayer, ObjectLayer};
    use bt_test_data::fixtures::MapBuilder;
    use glam::{DVec2, UVec2};

    use super::*;

    fn image(path: &str) -> ImageRef {
        ImageRef::file(FileRef::new(path, format!("/{path}")), UVec2::new(64, 32))
    }

    /// A tile layer (1), an object layer (2) and an image layer (3).
    fn map() -> Map {
        MapBuilder::new(UVec2::new(16, 16), UVec2::new(4, 4))
            .tile_layer("ground", ".")
            .layer(Layer {
                opacity: 0.5,
                ..Layer::new(
                    LayerId(0),
                    "things",
                    LayerKind::Object(ObjectLayer::default()),
                )
            })
            .image_layer(
                "sky",
                ImageLayer {
                    image: Some(ImageRef {
                        transparent: Some(Color::rgb(255, 0, 255)),
                        ..image("sky.png")
                    }),
                    ..ImageLayer::default()
                },
            )
            .build()
            .unwrap()
    }

    fn row(rows: &[AttributeRow], attribute: Attribute) -> Option<&AttributeRow> {
        rows.iter().find(|row| row.attribute == attribute)
    }

    #[test]
    fn several_layers_show_what_they_share_and_what_some_have() {
        let map = map();
        let rows = layer_attributes(&map, &[LayerId(1), LayerId(2), LayerId(3)]).unwrap();
        let opacity = row(&rows, Attribute::Opacity).unwrap();
        assert!(opacity.mixed, "one layer is half opaque");
        assert_eq!(
            opacity.value,
            AttributeValue::Number(1.0),
            "the first layer's"
        );
        assert_eq!(opacity.holders, [0, 1, 2]);
        let visible = row(&rows, Attribute::Visible).unwrap();
        assert!(!visible.mixed);
        let draw_order = row(&rows, Attribute::DrawOrder).unwrap();
        assert_eq!(draw_order.holders, [1], "only the object layer has one");
        assert!(!draw_order.mixed);
        let repeat = row(&rows, Attribute::RepeatX).unwrap();
        assert_eq!(repeat.holders, [2]);
        assert!(row(&rows, Attribute::Id).unwrap().read_only);
        assert!(row(&rows, Attribute::Id).unwrap().mixed);
        assert_eq!(
            layer_attributes(&map, &[LayerId(1), LayerId(9)]).unwrap_err(),
            PropertyError::NoSuchLayer(LayerId(9))
        );
    }

    #[test]
    fn a_value_set_on_several_layers_applies_to_those_that_have_it() {
        let mut map = map();
        let all = [LayerId(1), LayerId(2), LayerId(3)];
        assert_eq!(
            set_layer_attribute(
                &mut map,
                &all,
                Attribute::Opacity,
                &AttributeValue::Number(0.25)
            ),
            Ok(true)
        );
        for id in all {
            assert_eq!(map.layer(id).unwrap().opacity, 0.25);
        }
        assert_eq!(
            set_layer_attribute(
                &mut map,
                &all,
                Attribute::DrawOrder,
                &AttributeValue::DrawOrder(DrawOrder::Index)
            ),
            Ok(true)
        );
        assert_eq!(
            map.layer(LayerId(2))
                .unwrap()
                .as_object_layer()
                .unwrap()
                .draw_order,
            DrawOrder::Index
        );
        assert_eq!(
            set_layer_attribute(
                &mut map,
                &[LayerId(1)],
                Attribute::RepeatX,
                &AttributeValue::Bool(true)
            ),
            Err(PropertyError::NotApplicable(Attribute::RepeatX))
        );
        assert_eq!(
            set_layer_attribute(
                &mut map,
                &all,
                Attribute::BlendMode,
                &AttributeValue::BlendMode(BlendMode::Multiply)
            ),
            Ok(true)
        );
        assert_eq!(
            set_layer_attribute(
                &mut map,
                &all,
                Attribute::BlendMode,
                &AttributeValue::BlendMode(BlendMode::Multiply)
            ),
            Ok(false),
            "nothing changes the second time"
        );
        assert_eq!(
            set_layer_attribute(
                &mut map,
                &all,
                Attribute::OffsetY,
                &AttributeValue::Number(-8.0)
            ),
            Ok(true)
        );
        assert_eq!(map.layer(LayerId(3)).unwrap().offset, DVec2::new(0.0, -8.0));
    }

    #[test]
    fn out_of_range_values_are_refused_without_changing_any_layer() {
        let mut map = map();
        let before = map.clone();
        let all = [LayerId(1), LayerId(2), LayerId(3)];
        assert_eq!(
            set_layer_attribute(
                &mut map,
                &all,
                Attribute::Opacity,
                &AttributeValue::Number(-0.1)
            ),
            Err(PropertyError::OutOfRange {
                attribute: Attribute::Opacity,
                min: Some(0.0),
                max: Some(1.0)
            })
        );
        assert_eq!(
            set_layer_attribute(&mut map, &all, Attribute::Id, &AttributeValue::Integer(7)),
            Err(PropertyError::ReadOnly(Attribute::Id))
        );
        assert_eq!(
            set_layer_attribute(
                &mut map,
                &all,
                Attribute::ParallaxX,
                &AttributeValue::Number(f64::INFINITY)
            ),
            Err(PropertyError::OutOfRange {
                attribute: Attribute::ParallaxX,
                min: None,
                max: None
            })
        );
        assert_eq!(
            set_layer_attribute(&mut map, &all, Attribute::Name, &AttributeValue::Bool(true)),
            Err(PropertyError::InvalidValue {
                attribute: Attribute::Name,
                value: AttributeValue::Bool(true)
            })
        );
        assert_eq!(map, before);
    }

    #[test]
    fn an_image_layers_new_image_keeps_its_transparent_colour() {
        let mut map = map();
        let sky = [LayerId(3)];
        assert_eq!(
            set_layer_attribute(
                &mut map,
                &sky,
                Attribute::Image,
                &AttributeValue::Image(Some(image("clouds.png")))
            ),
            Ok(true)
        );
        let layer = map.layer(LayerId(3)).unwrap().as_image_layer().unwrap();
        let image = layer.image.as_ref().unwrap();
        assert_eq!(image.path().unwrap().to_str(), Some("/clouds.png"));
        assert_eq!(image.transparent, Some(Color::rgb(255, 0, 255)));
        assert_eq!(
            set_layer_attribute(
                &mut map,
                &sky,
                Attribute::TransparentColor,
                &AttributeValue::Color(Some(Color::rgba(0, 0, 0, 128)))
            ),
            Err(PropertyError::InvalidValue {
                attribute: Attribute::TransparentColor,
                value: AttributeValue::Color(Some(Color::rgba(0, 0, 0, 128)))
            }),
            "a transparent colour is opaque"
        );
        assert_eq!(
            set_layer_attribute(
                &mut map,
                &sky,
                Attribute::Image,
                &AttributeValue::Image(None)
            ),
            Ok(true)
        );
        let rows = layer_attributes(&map, &sky).unwrap();
        assert!(row(&rows, Attribute::TransparentColor).unwrap().read_only);
        assert_eq!(
            set_layer_attribute(
                &mut map,
                &sky,
                Attribute::TransparentColor,
                &AttributeValue::Color(None)
            ),
            Err(PropertyError::ReadOnly(Attribute::TransparentColor))
        );
    }
}
