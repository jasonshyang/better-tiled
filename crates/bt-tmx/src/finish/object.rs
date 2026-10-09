//! Objects: of object layers, as tiles' collision shapes, and in templates; template instances
//! take the template's values for what they do not state.

use std::sync::Arc;

use bt_core::{
    Cell, MapObject, ObjectId, ObjectOverrides, Shape, TextData, TileId, TilesetRef, TilesetSlot,
};
use glam::DVec2;

use super::context::{Finisher, text_of};
use super::map::GidSpace;
use crate::names;
use crate::raw::{RawObject, RawPoint, RawShape, RawText, Scalar};
use crate::warning::{Target, WarningKind};

/// Where an object is.
pub(crate) enum ObjectPlace<'s> {
    /// In a map's object layer: global ids name tiles of the map's tilesets, and a template's
    /// tileset joins them.
    Map(&'s mut GidSpace),
    /// A tile's collision shape: no tiles.
    Collision,
    /// A template's object: global ids name tiles of the template's tilesets.
    Template(&'s mut GidSpace),
}

/// Cells and tile objects whose global ids no tileset owns, reported once per layer.
#[derive(Default)]
pub(crate) struct UnknownGids {
    /// How many.
    pub(crate) count: usize,
    /// The first one.
    pub(crate) first: u32,
}

impl UnknownGids {
    /// Counts a global id no tileset owns.
    pub(crate) fn record_gid(&mut self, gid: u32) {
        if self.count == 0 {
            self.first = gid;
        }
        self.count += 1;
    }
}

impl Finisher<'_> {
    /// An object as stated, on top of its template's values when it is a template instance.
    pub(crate) fn object_of(
        &mut self,
        raw: RawObject,
        place: &mut ObjectPlace<'_>,
        unknown: &mut UnknownGids,
    ) -> MapObject {
        let id = ObjectId(
            self.u32_of(&raw.id, format_args!("an object id"))
                .unwrap_or(0),
        );
        let target = Some(Target::Object(id));
        let template_text = text_of(&raw.template);
        let template = if template_text.trim().is_empty() {
            None
        } else if matches!(place, ObjectPlace::Template(_)) {
            self.warnings.record_about(
                WarningKind::InvalidValue,
                target.clone(),
                "a template's object refers to another template; the reference is dropped",
            );
            None
        } else {
            Some(self.file_ref(&template_text))
        };
        let base = match &template {
            Some(file) => self.template_base(file, place, id),
            None => MapObject::default(),
        };
        let instance = template.is_some();
        let mut overrides = ObjectOverrides::empty();
        let mut object = base;
        object.id = id;
        object.pos = DVec2::new(
            self.real_of(&raw.x, format_args!("object {id}'s x"))
                .unwrap_or(0.0),
            self.real_of(&raw.y, format_args!("object {id}'s y"))
                .unwrap_or(0.0),
        );
        let name = text_of(&raw.name);
        if !name.is_empty() {
            object.name = name;
            overrides |= ObjectOverrides::NAME;
        }
        let class = match text_of(&raw.class) {
            class if !class.is_empty() => class,
            _ => text_of(&raw.class_as_type),
        };
        if !class.is_empty() {
            object.class = class;
            overrides |= ObjectOverrides::CLASS;
        }
        let size = DVec2::new(
            self.real_of(&raw.width, format_args!("object {id}'s width"))
                .unwrap_or(0.0),
            self.real_of(&raw.height, format_args!("object {id}'s height"))
                .unwrap_or(0.0),
        );
        if size.x > 0.0 && size.y > 0.0 {
            overrides |= ObjectOverrides::SIZE;
        }
        if !instance || overrides.contains(ObjectOverrides::SIZE) {
            object.size = size;
        }
        if let Some(rotation) = self.real_of(&raw.rotation, format_args!("object {id}'s rotation"))
        {
            object.rotation = rotation;
            overrides |= ObjectOverrides::ROTATION;
        }
        if let Some(opacity) = self.real_of(&raw.opacity, format_args!("object {id}'s opacity")) {
            object.opacity = opacity;
            overrides |= ObjectOverrides::OPACITY;
        }
        if let Some(visible) = self.bool_of(&raw.visible, format_args!("object {id}'s visibility"))
        {
            object.visible = visible;
            overrides |= ObjectOverrides::VISIBLE;
        }
        let gid = match &raw.gid {
            Some(value) if !value.is_empty_text() => value.to_gid().or_else(|| {
                self.warnings.record_about(
                    WarningKind::InvalidValue,
                    target.clone(),
                    format!("object {id}'s gid “{}” is not a tile id", value.to_text()),
                );
                None
            }),
            _ => None,
        }
        .filter(|gid| *gid != 0);
        if let Some(gid) = gid {
            match place {
                ObjectPlace::Map(space) | ObjectPlace::Template(space) => {
                    match space.cell_of(gid) {
                        Ok(cell) => object.tile = (!cell.is_empty()).then_some(cell),
                        Err(_) => {
                            unknown.record_gid(gid);
                            object.tile = None;
                        }
                    }
                    overrides |= ObjectOverrides::TILE;
                }
                ObjectPlace::Collision => self.warnings.record_about(
                    WarningKind::InvalidValue,
                    target.clone(),
                    "a collision shape cannot draw a tile; its gid is dropped",
                ),
            }
        }
        if let Some(shape) = raw.shape {
            overrides |= if matches!(shape, RawShape::Text(_)) {
                ObjectOverrides::TEXT
            } else {
                ObjectOverrides::SHAPE
            };
            object.shape = self.shape_of(shape, id);
        }
        let own = self.properties_of(raw.properties);
        object.properties.extend(own);
        object.template = template;
        object.overrides = if instance {
            overrides
        } else {
            ObjectOverrides::empty()
        };
        // Tile objects without a size take their tile's (files before Tiled 0.12).
        if let (Some(cell), ObjectPlace::Map(space) | ObjectPlace::Template(space)) =
            (object.tile, place)
            && (object.size.x == 0.0 || object.size.y == 0.0)
            && let Some(tileset) = cell
                .slot()
                .and_then(|slot| space.tilesets.get(slot.index()))
        {
            let tile_size = tileset.tileset.tile_pixel_size(cell.tile()).as_dvec2();
            if object.size.x == 0.0 {
                object.size.x = tile_size.x;
            }
            if object.size.y == 0.0 {
                object.size.y = tile_size.y;
            }
        }
        object
    }

    /// What an instance of the template at `file` takes: the template's object, its tile moved
    /// into the map's tilesets (none for a collision shape). A template that cannot be read
    /// gives an empty object and a warning; the reference is kept.
    fn template_base(
        &mut self,
        file: &bt_core::FileRef,
        place: &mut ObjectPlace<'_>,
        id: ObjectId,
    ) -> MapObject {
        let loaded = match self.loader.load_template_cached(&file.path) {
            Ok(loaded) => loaded,
            Err(error) => {
                self.warnings.record_about(
                    WarningKind::MissingFile,
                    Some(Target::Object(id)),
                    format!("the template “{}” cannot be read ({error}); object {id} keeps only its own values", file.source),
                );
                return MapObject::default();
            }
        };
        self.warnings.record_all(loaded.warnings);
        let template = loaded.value;
        let Some(mut base) = template.object.clone() else {
            return MapObject::default();
        };
        base.id = ObjectId(0);
        base.pos = DVec2::ZERO;
        base.template = None;
        base.overrides = ObjectOverrides::empty();
        base.tile = match (base.tile, place) {
            (Some(cell), ObjectPlace::Map(space)) => cell
                .slot()
                .and_then(|slot| template.tilesets.get(slot.index()))
                .and_then(|tileset| space.slot_for_template_tileset(tileset))
                .map(|slot| {
                    space.record_tile_use(slot, cell.tile());
                    Cell::new(slot, cell.tile()).with_flip(cell.flip())
                }),
            (Some(_), ObjectPlace::Collision) => {
                self.warnings.record_about(
                    WarningKind::InvalidValue,
                    Some(Target::Object(id)),
                    format!("the template “{}” draws a tile, which a collision shape cannot; the tile is dropped", file.source),
                );
                None
            }
            (Some(_), ObjectPlace::Template(_)) | (None, _) => None,
        };
        base
    }

    fn shape_of(&mut self, raw: RawShape, id: ObjectId) -> Shape {
        match raw {
            RawShape::Ellipse => Shape::Ellipse,
            RawShape::Point => Shape::Point,
            RawShape::Capsule => Shape::Capsule,
            RawShape::Polygon(points) => Shape::Polygon(self.points_of(points, id)),
            RawShape::Polyline(points) => Shape::Polyline(self.points_of(points, id)),
            RawShape::Text(text) => Shape::Text(self.text_data_of(*text, id)),
        }
    }

    fn points_of(&mut self, raw: Vec<RawPoint>, id: ObjectId) -> Vec<DVec2> {
        let mut points = Vec::with_capacity(raw.len());
        for point in raw {
            let x = self.real_of(&point.x, format_args!("a point of object {id}"));
            let y = self.real_of(&point.y, format_args!("a point of object {id}"));
            if let (Some(x), Some(y)) = (x, y) {
                points.push(DVec2::new(x, y));
            }
        }
        points
    }

    fn text_data_of(&mut self, raw: RawText, id: ObjectId) -> TextData {
        let defaults = TextData::default();
        let what = format!("object {id}'s text");
        let flag = |finisher: &mut Self, value: &Option<Scalar>, default: bool| {
            finisher
                .bool_of(value, format_args!("{what}'s style"))
                .unwrap_or(default)
        };
        TextData {
            font_family: raw
                .font_family
                .as_ref()
                .map(Scalar::to_text)
                .filter(|family| !family.is_empty())
                .unwrap_or(defaults.font_family),
            pixel_size: self
                .u32_of(&raw.pixel_size, format_args!("{what}'s pixel size"))
                .filter(|size| *size > 0)
                .unwrap_or(defaults.pixel_size),
            wrap: flag(self, &raw.wrap, defaults.wrap),
            color: self
                .color_of(&raw.color, format_args!("{what}'s colour"))
                .unwrap_or(defaults.color),
            bold: flag(self, &raw.bold, defaults.bold),
            italic: flag(self, &raw.italic, defaults.italic),
            underline: flag(self, &raw.underline, defaults.underline),
            strikeout: flag(self, &raw.strikeout, defaults.strikeout),
            kerning: flag(self, &raw.kerning, defaults.kerning),
            halign: self
                .named_of(
                    &raw.halign,
                    format_args!("{what}'s horizontal alignment"),
                    names::halign_from_name,
                )
                .unwrap_or(defaults.halign),
            valign: self
                .named_of(
                    &raw.valign,
                    format_args!("{what}'s vertical alignment"),
                    names::valign_from_name,
                )
                .unwrap_or(defaults.valign),
            text: raw.text,
        }
    }

    /// The collision shapes of a tile, with ids as stated.
    pub(crate) fn collision_objects_of(
        &mut self,
        raw: Vec<RawObject>,
        tileset: &str,
        tile: TileId,
    ) -> Vec<MapObject> {
        // Collision shapes name no tiles, so nothing is counted here.
        let mut unknown = UnknownGids::default();
        let _ = (tileset, tile);
        raw.into_iter()
            .map(|object| self.object_of(object, &mut ObjectPlace::Collision, &mut unknown))
            .collect()
    }
}

impl GidSpace {
    /// The slot of a template's tileset in a map: the map's own when it has the same tileset or
    /// file, else the tileset added with no global ids.
    pub(crate) fn slot_for_template_tileset(
        &mut self,
        tileset: &TilesetRef,
    ) -> Option<TilesetSlot> {
        let existing = self.tilesets.iter().position(|own| {
            Arc::ptr_eq(&own.tileset, &tileset.tileset)
                || (own.file.is_some()
                    && own.file.as_ref().map(|f| &f.path) == tileset.file.as_ref().map(|f| &f.path))
        });
        if let Some(index) = existing {
            return TilesetSlot::new(index);
        }
        self.add_tileset(
            TilesetRef {
                first_gid: None,
                ..tileset.clone()
            },
            false,
            true,
        )
    }
}
