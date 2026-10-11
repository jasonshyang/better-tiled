//! What changed among an object layer's objects: those added (written as ranges of ids, with
//! their kinds and where they stand), those removed (kind and place), those moved, and those
//! changed otherwise (which fields and properties). An object is where it is placed by: a
//! sprite by its foot, a point by its position, a shape by its top-left corner.

use std::collections::HashMap;

use bt_core::{Map, MapObject, ObjectId, ObjectLayer, Shape};
use bt_utils::text::plural;
use glam::DVec2;
use serde::{Deserialize, Serialize};

use crate::feet::{Feet, pixel_to_point};
use crate::names::Names;
use crate::words::{id_ranges_text, number_text, point_text, quoted_word, value_text};

/// The most objects a line lists by one.
const OBJECTS_LISTED: usize = 8;

/// What changed among an object layer's objects.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ObjectChanges {
    /// The objects added, by id.
    pub added: Vec<ObjectBrief>,
    /// The objects removed, by id, as they were.
    pub removed: Vec<ObjectBrief>,
    /// The objects that moved, by id.
    pub moved: Vec<ObjectMoved>,
    /// The objects changed otherwise, by id.
    pub changed: Vec<ObjectChanged>,
}

/// An object in brief: its id, what it is and where.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectBrief {
    /// Its id.
    pub id: u32,
    /// What it is: a sprite's name (with its type in brackets when it has one), a type, or a
    /// shape (`rectangle`, `point`…).
    pub kind: String,
    /// Its name; empty for none.
    pub name: String,
    /// Where it is, in tile units: a sprite's foot, a point, a shape's top-left corner.
    pub at: DVec2,
    /// A rectangle's, ellipse's or text's size in tile units; none for sprites, points,
    /// polygons and paths.
    pub size: Option<DVec2>,
    /// Its properties, `key=value`, by key.
    pub properties: Vec<String>,
}

/// An object that moved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectMoved {
    /// Its id.
    pub id: u32,
    /// What it is.
    pub kind: String,
    /// Where it was, in tile units.
    pub from: DVec2,
    /// Where it is.
    pub to: DVec2,
}

/// An object changed in other ways than its place.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectChanged {
    /// Its id.
    pub id: u32,
    /// What it is now.
    pub kind: String,
    /// What changed, as words: `name “gate”`, `type door`, `points=5`, `points removed`,
    /// `hidden`, `rotated 45°`, `size 2x1`, `flipped`.
    pub what: Vec<String>,
}

/// Reads objects for a comparison: where each stands, through one cache of feet.
pub(crate) struct ObjectReader<'f> {
    pub(crate) feet: &'f mut Feet,
}

impl ObjectReader<'_> {
    /// The object in brief, on `map` named by `names`.
    pub(crate) fn brief(&mut self, map: &Map, names: &Names, object: &MapObject) -> ObjectBrief {
        ObjectBrief {
            id: object.id.0,
            kind: object_kind(names, object),
            name: object.name.clone(),
            at: self.place(map, object),
            size: shape_size(map, object),
            properties: object
                .properties
                .iter()
                .map(|(key, property)| format!("{key}={}", value_text(&property.value)))
                .collect(),
        }
    }

    /// Where an object is placed by, in tile units: a sprite's foot (its anchor when its foot
    /// cannot be measured, as a report says where it is all the same), a point, a shape's
    /// top-left corner.
    pub(crate) fn place(&mut self, map: &Map, object: &MapObject) -> DVec2 {
        let geometry = map.geometry();
        if object.tile.is_some() {
            return self
                .feet
                .standing_point(map, object)
                .unwrap_or_else(|_| pixel_to_point(&geometry, object.pos));
        }
        pixel_to_point(&geometry, object.pos)
    }
}

/// What an object is: a sprite's name (with its own type in brackets when it has one), else its
/// type, else its shape.
pub(crate) fn object_kind(names: &Names, object: &MapObject) -> String {
    if let Some(tile) = object.tile {
        let sprite = names.tile_text(tile);
        return if object.class.is_empty() {
            sprite
        } else {
            format!("{sprite} [{}]", object.class)
        };
    }
    if !object.class.is_empty() {
        return object.class.clone();
    }
    object.kind_name().to_owned()
}

/// A shape's size in tile units, for rectangles, ellipses, capsules and text with a size.
fn shape_size(map: &Map, object: &MapObject) -> Option<DVec2> {
    if object.tile.is_some() || object.size == DVec2::ZERO {
        return None;
    }
    match object.shape {
        Shape::Rectangle | Shape::Ellipse | Shape::Capsule | Shape::Text(_) => {
            let geometry = map.geometry();
            Some(
                pixel_to_point(&geometry, object.pos + object.size)
                    - pixel_to_point(&geometry, object.pos),
            )
        }
        Shape::Point | Shape::Polygon(_) | Shape::Polyline(_) => None,
    }
}

impl ObjectChanges {
    /// What changed from `before` (none for a new layer) to `after`; `None` when nothing did.
    pub(crate) fn between(
        maps: super::cells::MapsCompared<'_>,
        reader: &mut ObjectReader<'_>,
        before: Option<&ObjectLayer>,
        after: &ObjectLayer,
    ) -> Option<ObjectChanges> {
        let old: HashMap<ObjectId, &MapObject> = before
            .map(|layer| layer.objects.iter().map(|o| (o.id, o)).collect())
            .unwrap_or_default();
        let mut changes = ObjectChanges::default();
        for object in after.objects.iter() {
            let Some(&was) = old.get(&object.id) else {
                changes
                    .added
                    .push(reader.brief(maps.after, maps.names_after, object));
                continue;
            };
            if was == object {
                continue;
            }
            let from = reader.place(maps.before, was);
            let to = reader.place(maps.after, object);
            let kind = object_kind(maps.names_after, object);
            if was.pos != object.pos {
                changes.moved.push(ObjectMoved {
                    id: object.id.0,
                    kind: kind.clone(),
                    from,
                    to,
                });
            }
            let what = what_changed(maps.after, was, object);
            if !what.is_empty() {
                changes.changed.push(ObjectChanged {
                    id: object.id.0,
                    kind,
                    what,
                });
            }
        }
        if let Some(before) = before {
            let now: std::collections::HashSet<ObjectId> =
                after.objects.iter().map(|o| o.id).collect();
            for object in before.objects.iter().filter(|o| !now.contains(&o.id)) {
                changes
                    .removed
                    .push(reader.brief(maps.before, maps.names_before, object));
            }
        }
        changes.added.sort_by_key(|o| o.id);
        changes.removed.sort_by_key(|o| o.id);
        changes.moved.sort_by_key(|o| o.id);
        changes.changed.sort_by_key(|o| o.id);
        let nothing = changes.added.is_empty()
            && changes.removed.is_empty()
            && changes.moved.is_empty()
            && changes.changed.is_empty();
        (!nothing).then_some(changes)
    }

    /// The changes in words, each a line to put after the layer's name.
    pub(crate) fn texts(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.added.is_empty() {
            let ids: Vec<u32> = self.added.iter().map(|o| o.id).collect();
            lines.push(format!(
                "+{} {}: {}",
                plural(self.added.len(), "object", "objects"),
                id_ranges_text(&ids),
                listed(self.added.iter().map(brief_text).collect(), "; ")
            ));
        }
        if !self.removed.is_empty() {
            lines.push(format!(
                "-{}: {}",
                plural(self.removed.len(), "object", "objects"),
                listed(
                    self.removed
                        .iter()
                        .map(|o| format!("#{} {} at {}", o.id, o.kind, point_text(o.at)))
                        .collect(),
                    ", "
                )
            ));
        }
        if !self.moved.is_empty() {
            lines.push(format!(
                "moved {}: {}",
                plural(self.moved.len(), "object", "objects"),
                listed(
                    self.moved
                        .iter()
                        .map(|o| format!(
                            "#{} {} {} → {}",
                            o.id,
                            o.kind,
                            point_text(o.from),
                            point_text(o.to)
                        ))
                        .collect(),
                    ", "
                )
            ));
        }
        if !self.changed.is_empty() {
            lines.push(format!(
                "changed {}: {}",
                plural(self.changed.len(), "object", "objects"),
                listed(
                    self.changed
                        .iter()
                        .map(|o| format!("#{} {} {}", o.id, o.kind, o.what.join(" ")))
                        .collect(),
                    "; "
                )
            ));
        }
        lines
    }
}

/// An object in words: `#4 tree at 3.5,6`, `#2 hazard “ice” at 1,1 3x2 kind="thin ice"`.
fn brief_text(object: &ObjectBrief) -> String {
    let mut text = format!("#{} {}", object.id, object.kind);
    if !object.name.is_empty() {
        text.push_str(&format!(" “{}”", object.name));
    }
    text.push_str(&format!(" at {}", point_text(object.at)));
    if let Some(size) = object.size {
        text.push_str(&format!(" {}x{}", number_text(size.x), number_text(size.y)));
    }
    for property in &object.properties {
        text.push(' ');
        text.push_str(property);
    }
    text
}

/// Items joined by `separator`, the first few of them, then how many more.
fn listed(items: Vec<String>, separator: &str) -> String {
    let more = items.len().saturating_sub(OBJECTS_LISTED);
    let mut text = items
        .into_iter()
        .take(OBJECTS_LISTED)
        .collect::<Vec<_>>()
        .join(separator);
    if more > 0 {
        text.push_str(&format!("{separator}and {more} more"));
    }
    text
}

/// What changed in an object apart from its place, as words.
fn what_changed(map: &Map, was: &MapObject, now: &MapObject) -> Vec<String> {
    let mut what = Vec::new();
    if was.name != now.name {
        what.push(format!("name {}", quoted_word(&now.name)));
    }
    if was.class != now.class {
        what.push(format!("type {}", quoted_word(&now.class)));
    }
    if was.visible != now.visible {
        what.push(if now.visible { "shown" } else { "hidden" }.to_owned());
    }
    if was.rotation != now.rotation {
        what.push(format!("rotated {}°", number_text(now.rotation)));
    }
    if was.size != now.size {
        let tile = DVec2::new(f64::from(map.tile_width), f64::from(map.tile_height));
        let size = now.size / tile.max(DVec2::ONE);
        what.push(format!(
            "size {}x{}",
            number_text(size.x),
            number_text(size.y)
        ));
    }
    match (was.tile, now.tile) {
        (Some(a), Some(b)) if a.same_tile(b) && a.flip() != b.flip() => {
            what.push("flipped".to_owned());
        }
        (a, b) if a != b => what.push("tile".to_owned()),
        _ => {}
    }
    if was.shape != now.shape && was.tile.is_none() {
        what.push("shape".to_owned());
    }
    for (key, property) in &now.properties {
        if was.properties.get(key) != Some(property) {
            what.push(format!("{key}={}", value_text(&property.value)));
        }
    }
    for key in was.properties.keys() {
        if !now.properties.contains_key(key) {
            what.push(format!("{key} removed"));
        }
    }
    let other = was.opacity != now.opacity || was.template != now.template;
    if what.is_empty() && other {
        what.push("changed".to_owned());
    }
    what
}
