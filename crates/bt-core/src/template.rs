//! Object templates (`.tx` files).

use glam::DVec2;

use crate::cell::Cell;
use crate::file_format::FileFormat;
use crate::ids::{ObjectId, TilesetSlot};
use crate::map::{Map, TilesetRef};
use crate::object::{MapObject, ObjectOverrides};

/// An object template: an object without id or position that instances take their values from,
/// and the tileset its tile is in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjectTemplate {
    /// The object; `None` for a template file without one.
    pub object: Option<MapObject>,
    /// The template's tilesets; the object's tile names a slot of this list.
    pub tilesets: Vec<TilesetRef>,
    /// How the template's file was written.
    pub file: FileFormat,
}

impl ObjectTemplate {
    /// Tiled's "Save As Template" for an object of `map`: the object without its id, position
    /// and template link, with its tile's tileset as the template's only one.
    pub fn from_map_object(map: &Map, object: &MapObject) -> ObjectTemplate {
        let mut tilesets = Vec::new();
        let tile = object.tile.and_then(|cell| {
            let tileset = map.tilesets.get(cell.slot()?.index())?;
            tilesets.push(TilesetRef {
                first_gid: Some(1),
                ..tileset.clone()
            });
            Some(Cell::new(TilesetSlot::new(0)?, cell.tile()).with_flip(cell.flip()))
        });
        let base = MapObject {
            id: ObjectId(0),
            pos: DVec2::ZERO,
            template: None,
            overrides: ObjectOverrides::empty(),
            tile,
            ..object.clone()
        };
        ObjectTemplate {
            object: Some(base),
            tilesets,
            file: FileFormat {
                line_ending: map.file.line_ending,
                ..FileFormat::default()
            },
        }
    }
}
