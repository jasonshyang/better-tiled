//! Objects back into the raw tree: everything for plain objects, only what an instance
//! overrides for template instances.

use bt_core::{Map, MapObject, ObjectOverrides, ObjectTemplate, Properties, Shape, TextData};

use super::gids::FirstGids;
use super::map::Unfinisher;
use super::property::raw_properties;
use crate::names;
use crate::raw::{RawObject, RawPoint, RawShape, RawTemplate, RawText, RawTileset, Scalar};

/// Where an object is written.
#[derive(Copy, Clone)]
pub(crate) enum Place<'m> {
    /// In an object layer of the map.
    Map(&'m Map),
    /// A tile's collision shape.
    Collision,
    /// A template's object: no id and no position.
    TemplateBase,
}

impl Unfinisher<'_> {
    /// An object.
    pub(crate) fn object_of(&mut self, object: &MapObject, place: Place<'_>) -> RawObject {
        let base = match (&object.template, place) {
            (Some(template), Place::Map(map)) => self.instance_base(template, Some(map)),
            (Some(template), Place::Collision) => self.instance_base(template, None),
            _ => None,
        };
        let instance = object.template.is_some();
        let own = |flag: ObjectOverrides, differs: &dyn Fn(&MapObject) -> bool| {
            !instance || object.overrides.contains(flag) || base.as_ref().is_some_and(differs)
        };
        let class = Some(Scalar::text(&object.class))
            .filter(|_| own(ObjectOverrides::CLASS, &|b| b.class != object.class));
        let template_base = matches!(place, Place::TemplateBase);
        RawObject {
            id: (!template_base).then_some(Scalar::int(object.id.0)),
            template: object
                .template
                .as_ref()
                .map(|t| Scalar::Text(self.reference(t))),
            name: own(ObjectOverrides::NAME, &|b| b.name != object.name)
                .then_some(Scalar::text(&object.name)),
            class: if self.class_as_type {
                None
            } else {
                class.clone()
            },
            class_as_type: if self.class_as_type { class } else { None },
            gid: if own(ObjectOverrides::TILE, &|b| b.tile != object.tile) {
                object.tile.map(|cell| Scalar::int(self.gids.gid_of(cell)))
            } else {
                None
            },
            x: (!template_base).then_some(Scalar::Real(object.pos.x)),
            y: (!template_base).then_some(Scalar::Real(object.pos.y)),
            width: own(ObjectOverrides::SIZE, &|b| b.size != object.size)
                .then_some(Scalar::Real(object.size.x)),
            height: own(ObjectOverrides::SIZE, &|b| b.size != object.size)
                .then_some(Scalar::Real(object.size.y)),
            rotation: own(ObjectOverrides::ROTATION, &|b| {
                b.rotation != object.rotation
            })
            .then_some(Scalar::Real(object.rotation)),
            opacity: own(ObjectOverrides::OPACITY, &|b| b.opacity != object.opacity)
                .then_some(Scalar::Real(object.opacity)),
            visible: own(ObjectOverrides::VISIBLE, &|b| b.visible != object.visible)
                .then_some(Scalar::Bool(object.visible)),
            properties: raw_properties(&own_properties(object, base.as_ref())),
            shape: if own(ObjectOverrides::SHAPE | ObjectOverrides::TEXT, &|b| {
                b.shape != object.shape
            }) {
                shape_of(&object.shape)
            } else {
                None
            },
        }
    }

    /// A template file: the tileset of its object's tile, with first gid 1, and the object.
    pub(crate) fn template_of(&mut self, template: &ObjectTemplate) -> RawTemplate {
        let slot = template
            .object
            .as_ref()
            .and_then(|object| object.tile)
            .and_then(|cell| cell.slot());
        self.gids = FirstGids::only(
            slot.map_or(usize::MAX, |s| s.index()),
            template.tilesets.len(),
        );
        let tilesets = slot
            .and_then(|slot| template.tilesets.get(slot.index()))
            .map(|tileset| {
                let mut raw = match &tileset.file {
                    Some(file) => RawTileset {
                        source: Some(Scalar::Text(self.reference(file))),
                        ..RawTileset::default()
                    },
                    None => self.tileset_contents_of(&tileset.tileset),
                };
                raw.first_gid = Some(Scalar::Int(1));
                raw
            })
            .into_iter()
            .collect();
        RawTemplate {
            tilesets,
            object: template
                .object
                .as_ref()
                .map(|object| self.object_of(object, Place::TemplateBase)),
        }
    }
}

/// The properties an object writes: an instance's own, those the template does not have the same.
fn own_properties(object: &MapObject, base: Option<&MapObject>) -> Properties {
    match base {
        Some(base) => object
            .properties
            .iter()
            .filter(|(name, property)| base.properties.get(*name) != Some(*property))
            .map(|(name, property)| (name.clone(), property.clone()))
            .collect(),
        None => object.properties.clone(),
    }
}

/// The shape element of a shape; none for a rectangle.
fn shape_of(shape: &Shape) -> Option<RawShape> {
    let points = |points: &[glam::DVec2]| {
        points
            .iter()
            .map(|p| RawPoint {
                x: Some(Scalar::Real(p.x)),
                y: Some(Scalar::Real(p.y)),
            })
            .collect()
    };
    match shape {
        Shape::Rectangle => None,
        Shape::Ellipse => Some(RawShape::Ellipse),
        Shape::Point => Some(RawShape::Point),
        Shape::Capsule => Some(RawShape::Capsule),
        Shape::Polygon(p) => Some(RawShape::Polygon(points(p))),
        Shape::Polyline(p) => Some(RawShape::Polyline(points(p))),
        Shape::Text(text) => Some(RawShape::Text(Box::new(text_of(text)))),
    }
}

fn text_of(text: &TextData) -> RawText {
    RawText {
        font_family: Some(Scalar::text(&text.font_family)),
        pixel_size: Some(Scalar::int(text.pixel_size)),
        wrap: Some(Scalar::Bool(text.wrap)),
        color: Some(Scalar::Text(text.color.to_tiled_text())),
        bold: Some(Scalar::Bool(text.bold)),
        italic: Some(Scalar::Bool(text.italic)),
        underline: Some(Scalar::Bool(text.underline)),
        strikeout: Some(Scalar::Bool(text.strikeout)),
        kerning: Some(Scalar::Bool(text.kerning)),
        halign: Some(Scalar::text(names::halign_name(text.halign))),
        valign: Some(Scalar::text(names::valign_name(text.valign))),
        text: text.text.clone(),
    }
}
