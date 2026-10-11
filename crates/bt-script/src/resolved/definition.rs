//! An object as Tiled defines it, without where it is drawn: its id, name, type and kind, its
//! position, size, rotation, points and text, and its properties resolved. Objects of object
//! layers and tiles' collision shapes share this; an object of a layer adds where it is drawn.

use bt_core::{HAlign, MapObject, Shape, TextData, TileData, VAlign};
use glam::DVec2;

use super::document::{ObjectEntry, TextEntry};
use super::grid::tidy;
use super::resolving::Resolving;
use super::values::{color_text, object_properties};
use crate::names::object_type;

/// `object` with its position moved by `shift` (its layer's offset, in map pixels; nothing for
/// a collision shape), its type its class else its tile's, and its properties resolved with
/// those of `tile`, the tile it shows. A tile object's size and where anything is drawn are the
/// caller's.
pub(crate) fn definition(
    resolving: &Resolving,
    object: &MapObject,
    tile: Option<&TileData>,
    shift: DVec2,
) -> ObjectEntry {
    let boxed = object.tile.is_none()
        && matches!(
            object.shape,
            Shape::Rectangle | Shape::Ellipse | Shape::Capsule | Shape::Text(_)
        );
    let points = match &object.shape {
        Shape::Polygon(points) | Shape::Polyline(points) if object.tile.is_none() => {
            Some(points.clone())
        }
        _ => None,
    };
    let text = match &object.shape {
        Shape::Text(data) if object.tile.is_none() => Some(text_entry(data)),
        _ => None,
    };
    ObjectEntry {
        id: object.id.0,
        name: object.name.clone(),
        object_type: object_type(resolving.map, object).to_owned(),
        kind: object.kind_name(),
        template: None,
        visible: object.visible,
        opacity: object.opacity,
        pos: tidy(object.pos + shift),
        size: boxed.then_some(object.size),
        rotation: object.rotation,
        points,
        drawn: None,
        shows: None,
        text,
        properties: object_properties(resolving.types, object, tile),
    }
}

/// A text object's text and how it is set.
fn text_entry(data: &TextData) -> TextEntry {
    TextEntry {
        string: data.text.clone(),
        font_family: data.font_family.clone(),
        pixel_size: data.pixel_size,
        color: color_text(data.color),
        wrap: data.wrap,
        bold: data.bold,
        italic: data.italic,
        underline: data.underline,
        strikeout: data.strikeout,
        kerning: data.kerning,
        halign: match data.halign {
            HAlign::Left => "left",
            HAlign::Center => "center",
            HAlign::Right => "right",
            HAlign::Justify => "justify",
        },
        valign: match data.valign {
            VAlign::Top => "top",
            VAlign::Center => "center",
            VAlign::Bottom => "bottom",
        },
    }
}
