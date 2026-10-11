//! The objects of object layers, with where they are drawn: placed through `bt-core`'s object
//! geometry (`ObjectFrame`, `object_outline`, `object_pixel_outline`, `tile_object_box`), the
//! functions the window draws objects with, so the JSON puts every object where the canvas
//! shows it, on every grid, moved by its layer's offset.

use bt_core::{
    Flip, MapObject, ObjectFrame, Orientation, Outline, Shape, object_outline,
    object_pixel_outline, tile_object_box,
};
use glam::DVec2;

use super::definition::definition;
use super::document::{Bounds, Drawn, ObjectEntry, ShownTile};
use super::error::ResolvedWarning;
use super::grid::tidy;
use super::palette::{Palette, Unkeyed};
use super::resolving::Resolving;
use crate::feet::{pixel_to_point, standing_cell};

/// `object` of a layer drawn moved by `offset` (screen pixels, its groups' included); `None`
/// for an object showing one of AutoMap's rule tiles, which is left out.
pub(crate) fn object_entry(
    resolving: &mut Resolving,
    palette: &mut Palette,
    object: &MapObject,
    offset: DVec2,
) -> Option<ObjectEntry> {
    let map = resolving.map;
    let geometry = resolving.geometry;
    let shows = match object.tile {
        Some(cell) => match palette.key_of(resolving, cell.with_flip(Flip::empty())) {
            Ok(key) => Some(ShownTile {
                tile: key,
                flip_h: cell.flip().contains(Flip::H),
                flip_v: cell.flip().contains(Flip::V),
            }),
            Err(Unkeyed::RuleTile(slot)) => {
                resolving.record_rule_object(slot);
                return None;
            }
            Err(Unkeyed::NoTileset) => {
                resolving.record_warning(ResolvedWarning::ObjectWithoutTileset { id: object.id.0 });
                None
            }
        },
        None => None,
    };
    let tileset = object.tile.and_then(|cell| map.tileset_of(cell));
    let tile_data = object.tile.and_then(|cell| map.tile_data(cell));
    // The offset is drawn on screen; in map pixels it is what moves a position there.
    let shift = geometry.screen_to_pixel(offset) - geometry.screen_to_pixel(DVec2::ZERO);
    let frame = ObjectFrame::of(&geometry, object);
    let tile_box = tile_object_box(&geometry, object, tileset);
    let upright_box = match (&object.shape, tile_box) {
        (_, Some(image)) => Some(Bounds {
            left: image.min.x,
            top: image.min.y,
            width: image.size().x,
            height: image.size().y,
        }),
        (Shape::Text(_), None) => Some(Bounds {
            left: 0.0,
            top: 0.0,
            width: object.size.x,
            height: object.size.y,
        }),
        _ => None,
    };
    let lines = |outline: Outline, by: DVec2| match outline {
        Outline::Area(points) | Outline::Path(points) => {
            Some(points.into_iter().map(|p| tidy(p + by)).collect())
        }
        Outline::Point(_) => None,
    };
    let isometric = map.orientation == Orientation::Isometric;
    let point = object.tile.is_none() && matches!(object.shape, Shape::Point);
    let drawn = Drawn {
        screen_pos: tidy(frame.to_screen(DVec2::ZERO) + offset),
        outline: lines(object_outline(&geometry, object, tileset), offset),
        pixel_outline: if isometric {
            lines(object_pixel_outline(&geometry, object, tileset), shift)
        } else {
            None
        },
        upright_box,
        cell: point.then(|| standing_cell(pixel_to_point(&geometry, object.pos + shift))),
    };
    let mut entry = definition(resolving, object, tile_data, shift);
    if let Some(image) = tile_box {
        entry.size = Some(image.size());
    }
    entry.template = object
        .template
        .as_ref()
        .map(|template| resolving.relative(&template.path));
    entry.drawn = Some(drawn);
    entry.shows = shows;
    Some(entry)
}
