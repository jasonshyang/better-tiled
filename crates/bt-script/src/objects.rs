//! `bt objects` (`10-agents.md`, §10.1): one line per object, as a script would make it: its id,
//! what it is and where (a sprite by its foot and the box its image covers, a point where it is,
//! a shape by its geometry in tile units), its layer, then its name, type and properties as
//! `key=value` words. Filtered by type, layer, region (where it stands) and name.

use std::collections::{BTreeMap, HashSet};

use bt_core::{Flip, LayerId, Map, MapObject, ObjectId, Shape, object_pixel_outline};
use bt_utils::IRect;
use bt_utils::text::{plural, slug};
use glam::{DVec2, IVec2};
use serde::{Deserialize, Serialize};

use crate::feet::{Feet, pixel_to_point, standing_cell};
use crate::layer_tree::layers_top_first;
use crate::names::{Names, object_type};
use crate::property_text::{assignments_text, properties_text, quoted};
use crate::report::{Line, Outcome, Report, Section};
use crate::words::{number_text, point_text, region_text};

/// The objects `bt objects` lists before "and N more".
const OBJECTS_SHOWN: usize = 400;

/// One object, in tile units.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectInfo {
    /// Its id.
    pub id: u32,
    /// Its layer's name.
    pub layer: String,
    /// What it is: `sprite`, `tile`, `point`, `rect`, `ellipse`, `capsule`, `polygon`, `path`
    /// or `text`.
    pub kind: String,
    /// A sprite's name, or a tile object's tile as `TILESET:ID`.
    pub tile: Option<String>,
    /// Its name; empty for none.
    pub name: String,
    /// Its type: its class, else its tile's.
    pub object_type: String,
    /// Where it stands: a sprite's foot, a point's position, the middle of a shape; `None` when
    /// a sprite's foot cannot be measured.
    pub stands: Option<DVec2>,
    /// The cell it stands in.
    pub cell: Option<IVec2>,
    /// Why its foot cannot be measured, when it cannot.
    pub unmeasured: Option<String>,
    /// The box it covers on the map, in tile units: a shape's, a sprite's image's.
    pub bounds: ObjectBox,
    /// A polygon's or path's points.
    pub points: Vec<DVec2>,
    /// A text object's text.
    pub text: Option<String>,
    /// Its rotation in degrees, clockwise.
    pub rotation: f64,
    /// A tile object's flips: `h`, `v` or `hv`; empty for none.
    pub flipped: String,
    /// A tile object drawn at another size than its image's: the image's size in cells.
    pub stretched_from: Option<DVec2>,
    /// Whether it is hidden.
    pub hidden: bool,
    /// Whether the map's AutoMap rules placed it (it cannot be edited).
    pub placed_by_rules: bool,
    /// Its properties, their values as scripts write them.
    pub properties: BTreeMap<String, String>,
}

/// A box in tile units: its top-left and its size.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectBox {
    /// The top-left corner.
    pub at: DVec2,
    /// The width and height.
    pub size: DVec2,
}

impl ObjectBox {
    /// Whether the box covers any of cell `cell`.
    pub(crate) fn overlaps_cell(&self, cell: IVec2) -> bool {
        let min = cell.as_dvec2();
        let max = min + DVec2::ONE;
        let end = self.at + self.size;
        self.at.x < max.x && end.x > min.x && self.at.y < max.y && end.y > min.y
    }
}

impl ObjectInfo {
    /// `object` of the layer called `layer`, in tile units; `feet` measures sprites' feet.
    pub(crate) fn from_object(
        map: &Map,
        names: &Names,
        feet: &mut Feet,
        layer: &str,
        object: &MapObject,
    ) -> ObjectInfo {
        let geometry = map.geometry();
        let tileset = object.tile.and_then(|tile| map.tileset_of(tile));
        let pixels = object_pixel_outline(&geometry, object, tileset).bounds();
        let min = pixel_to_point(&geometry, pixels.min);
        let max = pixel_to_point(&geometry, pixels.max);
        let (stands, unmeasured) = match feet.standing_point(map, object) {
            Ok(point) => (Some(point), None),
            Err(error) => (None, Some(error.to_string())),
        };
        let to_point = |p: DVec2| pixel_to_point(&geometry, object.pos + p);
        let (kind, points, text) = match &object.shape {
            _ if object.tile.is_some() => {
                let sprite = object.tile.is_some_and(|t| names.sprite_of(t).is_some());
                (if sprite { "sprite" } else { "tile" }, Vec::new(), None)
            }
            Shape::Point => ("point", Vec::new(), None),
            Shape::Rectangle => ("rect", Vec::new(), None),
            Shape::Ellipse => ("ellipse", Vec::new(), None),
            Shape::Capsule => ("capsule", Vec::new(), None),
            Shape::Polygon(points) => (
                "polygon",
                points.iter().map(|p| to_point(*p)).collect(),
                None,
            ),
            Shape::Polyline(points) => {
                ("path", points.iter().map(|p| to_point(*p)).collect(), None)
            }
            Shape::Text(data) => ("text", Vec::new(), Some(data.text.clone())),
        };
        let stretched_from = object.tile.and_then(|tile| {
            let tileset = map.tileset_of(tile)?;
            let natural = tileset.tile_pixel_size(tile.tile()).as_dvec2();
            let cell = map.tile_size().as_dvec2();
            let differs = (object.size - natural).abs().max_element() > 0.5;
            (differs && natural.min_element() > 0.0 && cell.min_element() > 0.0)
                .then(|| natural / cell)
        });
        ObjectInfo {
            id: object.id.0,
            layer: layer.to_owned(),
            kind: kind.to_owned(),
            tile: object.tile.map(|tile| names.tile_text(tile)),
            name: object.name.clone(),
            object_type: object_type(map, object).to_owned(),
            cell: stands.map(standing_cell),
            stands,
            unmeasured,
            bounds: ObjectBox {
                at: min,
                size: max - min,
            },
            points,
            text,
            rotation: object.rotation,
            flipped: object
                .tile
                .map_or_else(String::new, |tile| flips_text(tile.flip())),
            stretched_from,
            hidden: !object.visible,
            placed_by_rules: false,
            properties: properties_text(&object.properties),
        }
    }

    /// The object as one line, as a script would make it.
    pub(crate) fn line_text(&self) -> String {
        let mut text = format!("#{} ", self.id);
        let at = |point: Option<DVec2>| point.map_or_else(|| "?".to_owned(), point_text);
        let geometry = format!(
            "{} {}x{}",
            point_text(self.bounds.at),
            number_text(self.bounds.size.x),
            number_text(self.bounds.size.y)
        );
        match self.kind.as_str() {
            "sprite" | "tile" => {
                let tile = self.tile.as_deref().unwrap_or("?");
                if self.kind == "sprite" {
                    text.push_str(&format!("sprite {tile}"));
                } else {
                    text.push_str(&format!("tile {tile}"));
                }
                text.push_str(&format!(" foot {} box {geometry}", at(self.stands)));
                if let Some(size) = self.stretched_from {
                    text.push_str(&format!(
                        " (its image is {}x{} cells, stretched)",
                        number_text(size.x),
                        number_text(size.y)
                    ));
                }
                if !self.flipped.is_empty() {
                    text.push_str(&format!(" flipped {}", self.flipped));
                }
            }
            "point" => text.push_str(&format!("point at {}", at(self.stands))),
            "polygon" | "path" => {
                let points: Vec<String> = self.points.iter().map(|p| point_text(*p)).collect();
                text.push_str(&format!("{} {}", self.kind, points.join(" ")));
            }
            "text" => text.push_str(&format!(
                "text {} box {geometry}",
                quoted(self.text.as_deref().unwrap_or(""))
            )),
            kind => text.push_str(&format!("{kind} {geometry}")),
        }
        if self.rotation != 0.0 {
            text.push_str(&format!(" rotated {}°", number_text(self.rotation)));
        }
        text.push_str(&format!(" on {}", self.layer));
        if self.hidden {
            text.push_str(" (hidden)");
        }
        if self.placed_by_rules {
            text.push_str(" (placed by the AutoMap rules)");
        }
        let mut fields: BTreeMap<String, String> = BTreeMap::new();
        if !self.name.is_empty() {
            fields.insert("name".into(), quoted(&self.name));
        }
        if !self.object_type.is_empty() {
            fields.insert("type".into(), quoted(&self.object_type));
        }
        let mut words = assignments_text(&fields);
        let properties = assignments_text(&self.properties);
        if !properties.is_empty() {
            if !words.is_empty() {
                words.push(' ');
            }
            words.push_str(&properties);
        }
        if !words.is_empty() {
            text.push_str(&format!(": {words}"));
        }
        if let Some(why) = &self.unmeasured {
            text.push_str(&format!(" (its foot can't be measured: {why})"));
        }
        text
    }
}

/// A tile's flips as letters: `h`, `v`, `hv`; empty for none.
pub(crate) fn flips_text(flip: Flip) -> String {
    [
        (Flip::H, 'h'),
        (Flip::V, 'v'),
        (Flip::D, 'd'),
        (Flip::HEX120, 'x'),
    ]
    .into_iter()
    .filter(|(f, _)| flip.contains(*f))
    .map(|(_, letter)| letter)
    .collect()
}

/// What `bt objects` lists: every object, or those of a type, a layer, a region or a name.
pub struct ObjectsRequest<'a> {
    map: &'a Map,
    names: &'a Names,
    of_type: Option<String>,
    layer: Option<LayerId>,
    within: Option<IRect>,
    named: Option<String>,
    placed_by_rules: HashSet<ObjectId>,
}

impl<'a> ObjectsRequest<'a> {
    /// Every object of `map`.
    pub fn new(map: &'a Map, names: &'a Names) -> ObjectsRequest<'a> {
        ObjectsRequest {
            map,
            names,
            of_type: None,
            layer: None,
            within: None,
            named: None,
            placed_by_rules: HashSet::new(),
        }
    }

    /// Only objects of type `kind` (or sprites of that name).
    pub fn of_type(mut self, kind: impl Into<String>) -> ObjectsRequest<'a> {
        self.of_type = Some(kind.into());
        self
    }

    /// Only objects of the layer `layer`.
    pub fn on_layer(mut self, layer: LayerId) -> ObjectsRequest<'a> {
        self.layer = Some(layer);
        self
    }

    /// Only objects standing in `region`.
    pub fn within(mut self, region: IRect) -> ObjectsRequest<'a> {
        self.within = Some(region);
        self
    }

    /// Only objects called `name`.
    pub fn named(mut self, name: impl Into<String>) -> ObjectsRequest<'a> {
        self.named = Some(name.into());
        self
    }

    /// The objects the map's AutoMap rules placed, which are said to be.
    pub fn placed_by_rules(
        mut self,
        objects: impl IntoIterator<Item = ObjectId>,
    ) -> ObjectsRequest<'a> {
        self.placed_by_rules = objects.into_iter().collect();
        self
    }
}

/// `bt objects MAP`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectsOutcome {
    /// The filters, as words: `of type spawn`, `on layer design`.
    pub filters: Vec<String>,
    /// The objects, layer by layer from the top.
    pub objects: Vec<ObjectInfo>,
}

impl ObjectsOutcome {
    /// The objects `request` asks for.
    pub fn from_request(request: &ObjectsRequest<'_>) -> ObjectsOutcome {
        let map = request.map;
        let wanted_type = request.of_type.as_deref().map(slug);
        let wanted_name = request.named.as_deref().map(slug);
        let mut feet = Feet::new();
        let mut objects = Vec::new();
        for at in layers_top_first(map) {
            let Some(layer) = at.layer.as_object_layer() else {
                continue;
            };
            if request.layer.is_some_and(|id| id != at.layer.id) {
                continue;
            }
            for object in layer.objects.iter() {
                if let Some(kind) = &wanted_type {
                    let sprite = object
                        .tile
                        .and_then(|tile| request.names.sprite_of(tile))
                        .map(|s| s.name.as_str());
                    if slug(object_type(map, object)) != *kind && sprite != Some(kind.as_str()) {
                        continue;
                    }
                }
                if wanted_name
                    .as_ref()
                    .is_some_and(|name| slug(&object.name) != *name)
                {
                    continue;
                }
                let mut info =
                    ObjectInfo::from_object(map, request.names, &mut feet, &at.layer.name, object);
                if let Some(region) = request.within
                    && !info.cell.is_some_and(|cell| region.contains(cell))
                {
                    continue;
                }
                info.placed_by_rules = request.placed_by_rules.contains(&object.id);
                objects.push(info);
            }
        }
        let mut filters = Vec::new();
        if let Some(kind) = &request.of_type {
            filters.push(format!("of type {kind}"));
        }
        if let Some(layer) = request.layer.and_then(|id| map.layer(id)) {
            filters.push(format!("on layer {}", layer.name));
        }
        if let Some(region) = request.within {
            filters.push(format!("standing in {}", region_text(region)));
        }
        if let Some(name) = &request.named {
            filters.push(format!("called {}", quoted(name)));
        }
        ObjectsOutcome { filters, objects }
    }
}

impl Outcome for ObjectsOutcome {
    const SCHEMA: &'static str = "bt.objects/1";

    fn report(&self) -> Report {
        let mut report = Report::new();
        let which = if self.filters.is_empty() {
            String::new()
        } else {
            format!(" {}", self.filters.join(", "))
        };
        if self.objects.is_empty() {
            report.add_line(Line::fact(format!("no objects{which}")));
            return report;
        }
        let mut section = Section::headed(format!(
            "{}{which}, layer by layer from the top:",
            plural(self.objects.len(), "object", "objects")
        ))
        .limited(OBJECTS_SHOWN);
        section.add_lines(self.objects.iter().map(|o| Line::fact(o.line_text())));
        report.add_section(section);
        report
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{Cell, Property, TileId, TilesetSlot, Value};
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::UVec2;

    use super::*;
    use crate::report::{from_json, to_json};

    /// A 6×4 map with a tile object of a drawn atlas, a spawn point and a zone.
    pub(crate) fn object_map() -> Map {
        let atlas = Atlas::builder().name("things").build_embedded().unwrap();
        let tile = Cell::new(TilesetSlot::new(0).unwrap(), TileId(5)).with_flip(Flip::H);
        let mut spawn = MapObject {
            class: "spawn".into(),
            shape: Shape::Point,
            pos: DVec2::new(40.0, 24.0),
            ..MapObject::default()
        };
        spawn
            .properties
            .insert("wave".into(), Property::new(Value::Int(1)));
        let zone = MapObject {
            name: "the pond".into(),
            class: "zone".into(),
            pos: DVec2::new(32.0, 16.0),
            size: DVec2::new(48.0, 32.0),
            ..MapObject::default()
        };
        let crate_object = MapObject {
            tile: Some(tile),
            pos: DVec2::new(16.0, 32.0),
            size: DVec2::new(16.0, 16.0),
            ..MapObject::default()
        };
        MapBuilder::new(UVec2::new(16, 16), UVec2::new(6, 4))
            .tileset(Arc::clone(&atlas.tileset))
            .object_layer("props", vec![crate_object])
            .object_layer("design", vec![spawn, zone])
            .build()
            .unwrap()
    }

    #[test]
    fn objects_read_as_scripts_make_them() {
        let map = object_map();
        let names = Names::of(&map);
        let outcome = ObjectsOutcome::from_request(&ObjectsRequest::new(&map, &names));
        assert_eq!(outcome.objects.len(), 3);
        let spawn = &outcome.objects[0];
        assert_eq!((spawn.id, spawn.kind.as_str()), (2, "point"));
        assert_eq!(spawn.stands, Some(DVec2::new(2.5, 1.5)));
        assert_eq!(spawn.cell, Some(IVec2::new(2, 1)));
        let zone = &outcome.objects[1];
        assert_eq!(
            zone.bounds,
            ObjectBox {
                at: DVec2::new(2.0, 1.0),
                size: DVec2::new(3.0, 2.0)
            }
        );
        let tile = &outcome.objects[2];
        assert_eq!(tile.kind, "tile");
        assert_eq!(tile.tile.as_deref(), Some("things:5"));
        assert_eq!(tile.flipped, "h");
        assert_eq!(
            outcome.report().text(),
            "3 objects, layer by layer from the top:\n  \
             #2 point at 2.5,1.5 on design: type=spawn wave=1\n  \
             #3 rect 2,1 3x2 on design: name=\"the pond\" type=zone\n  \
             #1 tile things:5 foot 1.5,2 box 1,1 1x1 flipped h on props\n"
        );
        assert_eq!(
            from_json::<ObjectsOutcome>(&to_json(&outcome).unwrap()).unwrap(),
            outcome
        );
    }

    #[test]
    fn objects_are_filtered_by_type_layer_region_and_name() {
        let map = object_map();
        let names = Names::of(&map);
        let only = |request: ObjectsRequest<'_>| -> Vec<u32> {
            ObjectsOutcome::from_request(&request)
                .objects
                .iter()
                .map(|o| o.id)
                .collect()
        };
        assert_eq!(
            only(ObjectsRequest::new(&map, &names).of_type("Spawn")),
            [2]
        );
        assert_eq!(
            only(ObjectsRequest::new(&map, &names).on_layer(LayerId(1))),
            [1]
        );
        assert_eq!(
            only(
                ObjectsRequest::new(&map, &names)
                    .within(IRect::from_min_size(IVec2::new(2, 1), IVec2::new(2, 2)))
            ),
            [2, 3]
        );
        assert_eq!(
            only(ObjectsRequest::new(&map, &names).named("The Pond")),
            [3]
        );
        let none = ObjectsOutcome::from_request(&ObjectsRequest::new(&map, &names).of_type("exit"));
        assert_eq!(none.report().text(), "no objects of type exit\n");
    }
}
