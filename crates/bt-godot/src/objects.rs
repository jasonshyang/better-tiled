//! Objects as nodes: tile objects as sprites, points as `Marker2D`s, shapes as `Area2D`s with a
//! collision shape, polylines as `Path2D`s, text as `Label`s, and objects with a `resPath` as
//! instances of that scene (docs/design/12-godot-export.md).
//!
//! Each object is placed as the canvas draws it (bt-core object geometry) on every grid: at its
//! projected position minus the grid's shift (Decision 1), turned by its rotation. On isometric
//! maps shapes are projected onto the diamonds, so rectangles, ellipses and capsules become
//! polygons; tile objects and text stay upright, as Tiled draws them.
//!
//! `godotNode` makes a shape's area, or a tile object's sprite, another node: a physics body, an
//! area or a plain `Node2D` (docs/design/12-godot-export.md). A shape keeps its collision shape under it; a tile
//! object's node holds its sprite and its tile's collision shapes, as Tiled draws them on the
//! object. `godotCollisionLayer` and `godotCollisionMask` give bodies and areas their bits.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use bt_core::{Cell, HAlign, MapObject, Orientation, Property, Shape, VAlign, Value};
use glam::DVec2;

use super::collision::TileShape;
use super::conventions::{
    GODOT_COLLISION_LAYER, GODOT_COLLISION_MASK, GODOT_NODE, GODOT_Z_INDEX, NO_EXPORT, RES_PATH,
};
use super::report::{Element, Note, Severity};
use super::{RULE_TILES, collision, layers, names, read, res_path, text, values};
use crate::build::Build;

/// The property names objects give a meaning to, so they aren't metadata.
const CONSUMED: [&str; 3] = [RES_PATH, NO_EXPORT, GODOT_Z_INDEX];

/// What `godotNode` can make an object's node: bodies and an area, which collide with the shapes
/// under them, or a plain node, which keeps them for their geometry only.
const NODE_TYPES: [&str; 6] = [
    "StaticBody2D",
    "AnimatableBody2D",
    "CharacterBody2D",
    "RigidBody2D",
    "Area2D",
    "Node2D",
];

/// The settings of the nodes `godotNode` applies to; on other objects they stay metadata.
const BODY_SETTINGS: [&str; 3] = [GODOT_NODE, GODOT_COLLISION_LAYER, GODOT_COLLISION_MASK];

/// Godot's `z_index` range (`RenderingServer.CANVAS_ITEM_Z_MIN` to `_MAX`).
const Z_RANGE: i64 = 4096;

/// How deep instanced scenes inheriting from scenes are followed to their root script.
const INHERITANCE: usize = 8;

/// What objects with a `resPath` leave until every node is placed, and what the report says of
/// them and of `godotNode`, grouped.
#[derive(Default)]
pub(super) struct Instances {
    /// A `godotNode` naming no node type → the objects given it.
    unknown_nodes: BTreeMap<String, Vec<u32>>,
    /// Tile objects made to collide whose tiles have no collision shapes.
    shapeless: Vec<u32>,
    /// Whether the shapes of a tile the TileSet doesn't (yet) hold were taken for an object, so
    /// that the tiles' notes have more to say.
    tallied: bool,
    /// `res://` path → what its root script exports.
    exports: HashMap<String, Exports>,
    /// Properties also set on an instance as its script's own: (node, name, value, whether the
    /// variable holds a node rather than a `NodePath`).
    direct: Vec<(usize, String, Property, bool)>,
    /// Rotated instances, placed differently from Tiled's exporter.
    rotated: Vec<u32>,
    /// (`resPath`, what is wrong with it) → the objects it was given to.
    missing: BTreeMap<(String, &'static str), Vec<u32>>,
    /// `res://` path → the properties its root script doesn't export, and the objects having them.
    undeclared: BTreeMap<String, (BTreeSet<String>, Vec<u32>)>,
    /// Text styling a Label doesn't take: the font families, which of [`STYLES`] are used, and
    /// the text objects having any.
    unstyled: (BTreeSet<String>, [bool; 5], Vec<u32>),
}

/// Tiled's text styles a Label has no setting for, as said in the note.
const STYLES: [&str; 5] = ["bold", "italic", "underline", "strikeout", "kerning off"];

/// The variables a scene's root script exports (`@export`) with their declared types, or why
/// they can't be known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Exports {
    Known(BTreeMap<String, Option<String>>),
    Unknown(&'static str),
}

impl Build<'_> {
    /// Whether object `o` is left out of the scene (`noExport`, read as on layers).
    pub(super) fn left_out(&mut self, o: &MapObject) -> bool {
        self.consumed
            .take(&o.properties, NO_EXPORT)
            .is_some_and(|p| layers::flag(&p.value))
    }

    /// Object `o` as a node named `name` (names.rs) under its layer's node at `layer`.
    pub(super) fn object(&mut self, o: &MapObject, layer: &str, name: &str) {
        let map = self.map;
        self.counts.objects += 1;
        let frame = bt_core::ObjectFrame::of(&map.geometry(), o);
        // Where pixel-space vectors go on the map: not straight on isometric maps.
        let axes = Some(projection_axes(map))
            .filter(|a| !frame.is_upright() && *a != (DVec2::X, DVec2::Y));
        let scene = self.instance_scene(o);
        let zero_size = o.size.x == 0.0 || o.size.y == 0.0;
        let kind: &'static str = match &o.shape {
            // Not written: an instance has its scene's type.
            _ if scene.is_some() => "Node2D",
            _ if o.tile.is_some() => {
                let animated = o
                    .tile
                    .and_then(|c| map.tile_data(c))
                    .is_some_and(|t| !t.animation.is_empty());
                if animated {
                    "AnimatedSprite2D"
                } else {
                    "Sprite2D"
                }
            }
            Shape::Point => "Marker2D",
            Shape::Rectangle | Shape::Ellipse | Shape::Capsule if zero_size => "Marker2D",
            Shape::Polyline(_) => "Path2D",
            Shape::Text(_) => "Label",
            _ => "Area2D",
        };
        // `godotNode`: shapes with an area and tile objects only (an instance is "Node2D" here).
        let consulted = matches!(kind, "Area2D" | "Sprite2D" | "AnimatedSprite2D");
        let body = if consulted { self.godot_node(o) } else { None };
        let collides =
            body.unwrap_or(kind) == "Area2D" || body.is_some_and(|b| b.ends_with("Body2D"));
        let node = self.node(Some(layer), name, body.unwrap_or(kind));
        let path = self.node_path(node);
        {
            let uid = self.names.object(o.id.0);
            self.set_node_id(node, uid);
        }
        self.names.placed(Element::Object(o.id.0), &path);
        self.object_paths.insert(o.id.0, path);
        if !o.visible {
            self.prop(node, "visible", "false".to_owned());
        }
        if o.opacity != 1.0 && scene.is_none() {
            self.prop(
                node,
                "modulate",
                text::color(bt_core::Color::rgba(
                    255,
                    255,
                    255,
                    (o.opacity * 255.0).round().clamp(0.0, 255.0) as u8,
                )),
            );
        }
        if let Some(z) = self.z_index(o) {
            self.prop(node, "z_index", z.to_string());
        }
        let rotation = o.rotation.to_radians();
        let origin = self.placed(frame.to_screen(DVec2::ZERO));
        if let Some(scene) = scene {
            self.instance(node, o, frame, scene);
        } else if let Shape::Text(t) = &o.shape {
            // A Control: placed by its edges, turning around its top-left corner like Tiled's.
            for (key, v) in [
                ("offset_left", origin.x),
                ("offset_top", origin.y),
                ("offset_right", origin.x + o.size.x),
                ("offset_bottom", origin.y + o.size.y),
            ] {
                self.prop(node, key, text::float(v));
            }
            if rotation != 0.0 {
                self.prop(node, "rotation", text::float(rotation));
            }
            self.prop(node, "text", text::string(&t.text));
            self.prop(
                node,
                "theme_override_font_sizes/font_size",
                t.pixel_size.to_string(),
            );
            self.prop(
                node,
                "theme_override_colors/font_color",
                text::color(t.color),
            );
            let h = match t.halign {
                HAlign::Left => 0,
                HAlign::Center => 1,
                HAlign::Right => 2,
                HAlign::Justify => 3,
            };
            let v = match t.valign {
                VAlign::Top => 0,
                VAlign::Center => 1,
                VAlign::Bottom => 2,
            };
            self.prop(node, "horizontal_alignment", h.to_string());
            self.prop(node, "vertical_alignment", v.to_string());
            if t.wrap {
                self.prop(node, "autowrap_mode", "3".to_owned());
            }
            // The rest shows in the theme's font, plain: said once for all text. Tiled's default
            // family is sans-serif, as Godot's default font is.
            let styles = [t.bold, t.italic, t.underline, t.strikeout, !t.kerning];
            let family = t.font_family != "sans-serif";
            if family || styles.contains(&true) {
                let (families, used, ids) = &mut self.instances.unstyled;
                if family {
                    families.insert(t.font_family.clone());
                }
                used.iter_mut().zip(styles).for_each(|(u, s)| *u |= s);
                ids.push(o.id.0);
            }
        } else {
            self.prop(node, "position", text::vector2(origin));
            if rotation != 0.0 {
                self.prop(node, "rotation", text::float(rotation));
            }
            if collides {
                self.collision_bits(node, o);
            }
            let project = |p: DVec2| axes.map_or(p, |(x, y)| tidy(x * p.x + y * p.y));
            if let Some(cell) = o.tile {
                if body.is_some() {
                    self.tile_body(node, o, cell, collides);
                } else {
                    self.sprite(node, o, cell);
                }
            } else if kind == "Area2D" {
                // Whatever node `godotNode` made it, a shape keeps its collision shape.
                self.area_shape(node, o, axes.map(|_| &project as &dyn Fn(DVec2) -> DVec2));
            } else if let Shape::Polyline(points) = &o.shape {
                let mut data = Vec::new();
                for p in points {
                    data.extend([DVec2::ZERO, DVec2::ZERO, project(*p)]);
                }
                let curve = self.sub(
                    "Curve2D",
                    vec![
                        (
                            "_data".to_owned(),
                            format!("{{\n\"points\": {}\n}}", text::points(&data)),
                        ),
                        ("point_count".to_owned(), points.len().to_string()),
                    ],
                );
                self.prop(node, "curve", curve);
            }
        }
        self.push_node_meta(
            node,
            "id".to_owned(),
            Property::new(Value::Int(i64::from(o.id.0))),
        );
        // The body settings only where they apply; elsewhere they stay metadata.
        let used = |k: &str| {
            CONSUMED.contains(&k)
                || (consulted && k == GODOT_NODE)
                || (collides && [GODOT_COLLISION_LAYER, GODOT_COLLISION_MASK].contains(&k))
        };
        let properties = o
            .properties
            .iter()
            .filter(|(k, _)| !used(k))
            .map(|(k, p)| (k.clone(), p.clone()))
            .collect();
        self.meta(node, &o.class, &properties);
    }

    /// The node type the object's `godotNode` names; a value naming none keeps the usual node,
    /// with a note.
    fn godot_node(&mut self, o: &MapObject) -> Option<&'static str> {
        let property = self.consumed.take(&o.properties, GODOT_NODE)?;
        let value = match &property.value {
            // Empty, as a class's default often is: the usual node.
            Value::String(s) if s.is_empty() => return None,
            Value::String(s) => s.as_str(),
            _ => "",
        };
        if let Some(kind) = NODE_TYPES.into_iter().find(|k| *k == value) {
            return Some(kind);
        }
        let shown = match &property.value {
            Value::String(s) => format!("\"{s}\""),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Bool(b) => b.to_string(),
            _ => "of that type".to_owned(),
        };
        self.instances
            .unknown_nodes
            .entry(shown)
            .or_default()
            .push(o.id.0);
        None
    }

    /// A body's or area's `collision_layer` and `collision_mask`: Godot's bits, from
    /// `godotCollisionLayer` and `godotCollisionMask`. Godot's default, 1, is left out, as Godot
    /// leaves it.
    fn collision_bits(&mut self, node: usize, o: &MapObject) {
        for (name, key) in [
            (GODOT_COLLISION_LAYER, "collision_layer"),
            (GODOT_COLLISION_MASK, "collision_mask"),
        ] {
            let Some(property) = self.consumed.take(&o.properties, name) else {
                continue;
            };
            let bits = match property.value {
                Value::Int(i) => u32::try_from(i).ok(),
                Value::Float(f) if f.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&f) => {
                    Some(f as u32)
                }
                _ => None,
            };
            match bits {
                Some(1) => {}
                Some(bits) => self.prop(node, key, bits.to_string()),
                None => self.warn_once(crate::Code::PropertyType, format!("object {}: {name} takes Godot's bits, a whole number from 0 to {}; it is left out", o.id.0, u32::MAX), vec![Element::Object(o.id.0)]),
            }
        }
    }

    /// A tile object that `godotNode` made a body, an area or a plain node: its tile drawn by a
    /// sprite under it, and its tile's collision shapes beside the sprite, as Tiled draws them on
    /// the object. `collides`: a body or area, which wants shapes.
    fn tile_body(&mut self, node: usize, o: &MapObject, cell: Cell, collides: bool) {
        let parent = self.node_path(node);
        let owner = self.node_id(node);
        let shapes = self.object_shapes(o, cell);
        if collides && shapes.as_ref().is_some_and(Vec::is_empty) {
            self.instances.shapeless.push(o.id.0);
        }
        let shapes = shapes.unwrap_or_default();
        // Named like objects: a shape's name, else `Shape`, the lowest id keeping it.
        let wanted: Vec<(u32, String)> = std::iter::once((0, "Sprite".to_owned()))
            .chain(shapes.iter().map(|(s, name)| (s.object, name.clone())))
            .collect();
        let names = names::siblings(&wanted);
        let animated = self
            .map
            .tile_data(cell)
            .is_some_and(|t| !t.animation.is_empty());
        let sprite = self.node(
            Some(&parent),
            names.first().map(String::as_str).unwrap_or("Sprite"),
            if animated {
                "AnimatedSprite2D"
            } else {
                "Sprite2D"
            },
        );
        {
            let uid = self.names.helper(owner, "sprite", 0);
            self.set_node_id(sprite, uid);
        }
        self.sprite(sprite, o, cell);
        for ((shape, _), name) in shapes.iter().zip(names.iter().skip(1)) {
            let child = self.node(Some(&parent), name, "CollisionPolygon2D");
            // By the shape's id in its tile, so that a shape added in Tiled moves no other's.
            {
                let uid = self.names.helper(owner, "shape", shape.object);
                self.set_node_id(child, uid);
            }
            self.prop(child, "polygon", text::points(&shape.points));
            if shape.one_way {
                self.prop(child, "one_way_collision", "true".to_owned());
            }
            if let Some(margin) = shape.one_way_margin {
                self.prop(child, "one_way_collision_margin", text::float(margin));
            }
        }
    }

    /// The physics shapes of a tile object's tile, each with the name it wants, in the object's
    /// own coordinates: the tile's image is drawn over the object's box, flipped in it, and its
    /// shapes with it (Tiled's `paintTileCollisionShapes`). `None` when the tile isn't there
    /// (the sprite says so).
    fn object_shapes(&mut self, o: &MapObject, cell: Cell) -> Option<Vec<(TileShape, String)>> {
        let map = self.map;
        let slot = cell
            .slot()
            .map(|s| s.index())
            .filter(|s| *s < map.tilesets.len() && !self.is_rule_slot(*s))?;
        let ts = &map.tilesets.get(slot)?.tileset;
        let bt_core::TileImage { rect, .. } = ts.tile_image(cell.tile())?;
        let size = DVec2::new(rect.w.max(1) as f64, rect.h.max(1) as f64);
        // What is wrong with a tile's shapes is said once, by the TileSet when it holds the tile;
        // of a tile it doesn't hold yet, finish_objects has it said.
        let held = self
            .tileset
            .sources
            .iter()
            .flat_map(|s| s.tiles.values())
            .any(|d| (d.slot, d.id) == (slot, cell.tile().0));
        let tally = held.then(|| std::mem::take(&mut self.tileset.data.tally));
        let shapes = self.tile_shapes(ts, slot, cell.tile().0, size, DVec2::ZERO);
        match tally {
            Some(tally) => self.tileset.data.tally = tally,
            None => self.instances.tallied = true,
        }
        let collision = ts.tile(cell.tile()).map_or(&[][..], |t| {
            t.collision
                .as_ref()
                .map_or(&[], |g| g.layer.objects.as_slice())
        });
        let scale = if o.size.x > 0.0 && o.size.y > 0.0 {
            o.size / size
        } else {
            DVec2::ONE
        };
        let min = bt_core::tile_object_box(
            &map.geometry(),
            o,
            o.tile.and_then(|c| c.slot()).and_then(|s| map.tileset(s)),
        )
        .map_or(DVec2::ZERO, |r| r.min);
        let flip = |p: DVec2| {
            DVec2::new(
                if cell.flip().contains(bt_core::Flip::H) {
                    size.x - p.x
                } else {
                    p.x
                },
                if cell.flip().contains(bt_core::Flip::V) {
                    size.y - p.y
                } else {
                    p.y
                },
            )
        };
        let shapes = shapes
            .into_iter()
            .filter(|s| s.physics_layer.is_some())
            .map(|mut s| {
                s.points = s
                    .points
                    .iter()
                    .map(|p| tidy(min + flip(*p) * scale))
                    .collect();
                let name = collision
                    .iter()
                    .find(|c| c.id.0 == s.object)
                    .map(|c| text::node_name(&c.name))
                    .filter(|n| !n.is_empty());
                (s, name.unwrap_or_else(|| "Shape".to_owned()))
            })
            .collect();
        Some(shapes)
    }

    /// A point of the map's screen as the scene has it: minus the grid's shift. Projected values
    /// lose their floating-point noise (16 rather than 15.999999999999998).
    fn placed(&self, p: DVec2) -> DVec2 {
        let p = p - self.grid.shift();
        if self.map.orientation == Orientation::Isometric {
            tidy(p)
        } else {
            p
        }
    }

    /// The object's `godotZIndex`, within Godot's range.
    fn z_index(&mut self, o: &MapObject) -> Option<i64> {
        let property = self.consumed.take(&o.properties, GODOT_Z_INDEX)?;
        let z = match property.value {
            Value::Int(z) => z,
            Value::Float(z) if z.fract() == 0.0 => z as i64,
            _ => {
                self.warn_once(
                    crate::Code::PropertyType,
                    format!(
                        "object {}: {GODOT_Z_INDEX} must be a whole number; it is left out",
                        o.id.0
                    ),
                    vec![Element::Object(o.id.0)],
                );
                return None;
            }
        };
        if z.abs() > Z_RANGE {
            self.warn_once(crate::Code::ZindexClamped, format!("object {}: {GODOT_Z_INDEX} {z} is beyond Godot's -{Z_RANGE} to {Z_RANGE}; it is clamped", o.id.0), vec![Element::Object(o.id.0)]);
        }
        Some(z.clamp(-Z_RANGE, Z_RANGE))
    }

    /// The `res://` path of the scene object `o` is an instance of: its `resPath` (resolved, so
    /// also its template's, its tile's or its class's), when that is a scene in the project. A
    /// `resPath` that isn't keeps the typed node, with a warning.
    fn instance_scene(&mut self, o: &MapObject) -> Option<String> {
        let property = self.consumed.take(&o.properties, RES_PATH)?;
        let res = match &property.value {
            // Empty, as a class's default often is: no scene.
            Value::String(s) | Value::File(s) if s.is_empty() => return None,
            Value::String(s) => s.clone(),
            // A file property: the scene's res:// path, if it is in the project.
            Value::File(f) => {
                res_path(&self.project, &self.map_dir.join(f)).unwrap_or_else(|| f.clone())
            }
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Bool(b) => b.to_string(),
            _ => String::new(),
        };
        let problem =
            if !(res.starts_with("res://") && (res.ends_with(".tscn") || res.ends_with(".scn"))) {
                Some("isn't a res://….tscn or .scn path")
            } else if !self.project.join(&res["res://".len()..]).is_file() {
                Some("isn't a file in the Godot project")
            } else {
                None
            };
        if let Some(problem) = problem {
            self.instances
                .missing
                .entry((res, problem))
                .or_default()
                .push(o.id.0);
            return None;
        }
        Some(res)
    }

    /// Makes `node` an instance of the scene at `res`, placed at the centre of the object's box,
    /// turned with it; its properties are also set as the root script's own where it exports
    /// them.
    fn instance(&mut self, node: usize, o: &MapObject, frame: bt_core::ObjectFrame, res: String) {
        self.counts.instances += 1;
        let resource = self.scene.ext_resource("PackedScene", res.clone());
        if let Some(node) = self.node_state(node) {
            node.instance = Some(resource);
        }
        let bounds = bt_utils::DRect::from_points(
            bt_core::object_local_outline(
                &self.map.geometry(),
                o,
                o.tile
                    .and_then(|c| c.slot())
                    .and_then(|s| self.map.tileset(s)),
            )
            .points()
            .iter()
            .copied(),
        );
        let (min, max) = (
            bounds.map_or(DVec2::ZERO, |r| r.min),
            bounds.map_or(DVec2::ZERO, |r| r.max),
        );
        // (Turned, it is worked out on every grid.)
        let centre = tidy(self.placed(frame.to_screen((min + max) / 2.0)));
        self.prop(node, "position", text::vector2(centre));
        if o.rotation != 0.0 {
            self.prop(node, "rotation", text::float(o.rotation.to_radians()));
            self.instances.rotated.push(o.id.0);
        }
        let project = self.project.clone();
        let exports = self
            .instances
            .exports
            .entry(res.clone())
            .or_insert_with(|| scene_exports(&project, &res))
            .clone();
        let mut undeclared = Vec::new();
        for (key, property) in &o.properties {
            // The body settings don't apply to instances: they are metadata, nothing more.
            if CONSUMED.contains(&key.as_str()) || BODY_SETTINGS.contains(&key.as_str()) {
                continue;
            }
            match &exports {
                Exports::Known(vars) if vars.contains_key(key) => {
                    let node_variable = vars[key].as_deref() != Some("NodePath");
                    self.instances.direct.push((
                        node,
                        key.clone(),
                        property.clone(),
                        node_variable,
                    ));
                }
                _ => undeclared.push(key.clone()),
            }
        }
        if !undeclared.is_empty() {
            let (names, ids) = self.instances.undeclared.entry(res).or_default();
            names.extend(undeclared);
            ids.push(o.id.0);
        }
    }

    /// What objects leave until every node is placed: the properties set on instances as their
    /// script's own (an object reference needs its target's path), and the notes on instances.
    pub(super) fn finish_objects(&mut self) {
        let values = values::Values {
            project: &self.project,
            map_dir: &self.map_dir,
            object_paths: &self.object_paths,
        };
        for (node, key, property, node_variable) in std::mem::take(&mut self.instances.direct) {
            let Some(path) = self.scene.nodes.get(node).map(|n| n.path.as_str()) else {
                self.failures
                    .push(format!("instance node {node} is missing"));
                continue;
            };
            let value = values.value(&property, path);
            // A reference to an object: Godot hands an exported node variable the node itself.
            if node_variable
                && matches!(property.value, Value::Object(_))
                && value.starts_with("NodePath(")
            {
                if let Some(node) = self.scene.nodes.get_mut(node) {
                    node.node_paths.push(key.clone());
                } else {
                    self.failures.push("an instance node vanished".to_owned());
                }
            }
            // Quoted where Godot would otherwise not read the name whole (`@export var état`).
            if let Some(node) = self.scene.nodes.get_mut(node) {
                node.props.push((text::property_name(&key), value));
            } else {
                self.failures.push("an instance node vanished".to_owned());
            }
        }
        let elements = |ids: &[u32]| {
            ids.iter()
                .map(|id| Element::Object(*id))
                .collect::<Vec<_>>()
        };
        for ((res, problem), ids) in std::mem::take(&mut self.instances.missing) {
            let message = format!(
                "resPath \"{res}\" {problem}: {} {}",
                objects(&ids),
                if ids.len() == 1 {
                    "stays a plain node"
                } else {
                    "stay plain nodes"
                }
            );
            self.notes.push(Note::warning(
                crate::Code::RespathMissing,
                message,
                elements(&ids),
            ));
        }
        for (res, (names, ids)) in std::mem::take(&mut self.instances.undeclared) {
            let (pronoun, verb) = if names.len() == 1 {
                ("it", "is")
            } else {
                ("they", "are")
            };
            let names = names.into_iter().collect::<Vec<_>>().join(", ");
            let Some(exports) = self.instances.exports.get(&res) else {
                self.failures
                    .push(format!("instance exports for {res} are missing"));
                continue;
            };
            let message = match exports {
                Exports::Known(_) => format!(
                    "{res}: its root script has no @export var {names}, so on {} {pronoun} {verb} metadata only; Godot drops properties a scene doesn't declare",
                    objects(&ids)
                ),
                Exports::Unknown(why) => format!(
                    "{res}: {why}, so on {} {names} {verb} metadata only",
                    objects(&ids)
                ),
            };
            self.notes.push(Note::warning(
                crate::Code::ExportUndeclared,
                message,
                elements(&ids),
            ));
        }
        let rotated = std::mem::take(&mut self.instances.rotated);
        if !rotated.is_empty() {
            let message = if rotated.len() == 1 {
                format!(
                    "{} is turned: its scene instance goes at the centre of its turned box, turned with it; Tiled's exporter puts it at the centre of the unturned box, unturned",
                    objects(&rotated)
                )
            } else {
                format!(
                    "{} are turned: their scene instances go at the centre of their turned boxes, turned with them; Tiled's exporter puts them at the centre of the unturned boxes, unturned",
                    objects(&rotated)
                )
            };
            self.notes.push(Note {
                code: crate::Code::RespathRotated,
                severity: Severity::Info,
                message,
                elements: elements(&rotated),
            });
        }
        let (families, used, ids) = std::mem::take(&mut self.instances.unstyled);
        if !ids.is_empty() {
            let families = (!families.is_empty()).then(|| {
                format!(
                    "font family {}",
                    families.into_iter().collect::<Vec<_>>().join(", ")
                )
            });
            let left: Vec<String> = families
                .into_iter()
                .chain(
                    STYLES
                        .iter()
                        .zip(used)
                        .filter(|(_, u)| *u)
                        .map(|(s, _)| (*s).to_owned()),
                )
                .collect();
            let what = match left.split_last() {
                Some((last, rest)) if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
                _ => left.concat(),
            };
            let (their, show) = if ids.len() == 1 {
                ("its", "it shows")
            } else {
                ("their", "they show")
            };
            let message = format!(
                "{}: Godot's Label doesn't take {their} text's {what}, so {show} in the theme's font; in Godot a SystemFont can name a family and make it bold or italic",
                objects(&ids)
            );
            self.notes.push(Note {
                code: crate::Code::LeftOut,
                severity: Severity::Info,
                message,
                elements: elements(&ids),
            });
        }
        let types = format!(
            "{} or {}",
            NODE_TYPES
                .iter()
                .take(NODE_TYPES.len() - 1)
                .copied()
                .collect::<Vec<_>>()
                .join(", "),
            NODE_TYPES.last().copied().unwrap_or("Node2D")
        );
        for (value, ids) in std::mem::take(&mut self.instances.unknown_nodes) {
            let keep = if ids.len() == 1 {
                "keeps its usual node"
            } else {
                "keep their usual nodes"
            };
            self.notes.push(Note::warning(
                crate::Code::NodeType,
                format!(
                    "{GODOT_NODE} {value} isn't {types}: {} {keep}",
                    objects(&ids)
                ),
                elements(&ids),
            ));
        }
        let shapeless = std::mem::take(&mut self.instances.shapeless);
        if !shapeless.is_empty() {
            let (verb, tiles, them) = if shapeless.len() == 1 {
                ("collides", "its tile has", "it")
            } else {
                ("collide", "their tiles have", "them")
            };
            let message = format!(
                "{} {verb} with nothing: {tiles} no collision shapes; draw some in Tiled's tile collision editor, or give {them} the {GODOT_NODE} Node2D",
                objects(&shapeless)
            );
            self.notes.push(Note::warning(
                crate::Code::BodyNoShape,
                message,
                elements(&shapeless),
            ));
        }
        // What is wrong with the shapes of tiles that only tile objects took: a TileSet still to
        // be made says it with its own, once.
        if std::mem::take(&mut self.instances.tallied) {
            if self.tileset.shared.is_some()
                || self.scene.nodes.iter().any(|n| n.kind == "TileMapLayer")
            {
                self.tileset_contents();
            }
            self.tally_notes();
        }
    }

    /// A tile object: a sprite drawn over the object's box.
    fn sprite(&mut self, node: usize, o: &MapObject, cell: Cell) {
        let map = self.map;
        let Some(slot) = cell
            .slot()
            .map(|s| s.index())
            .filter(|s| *s < map.tilesets.len())
        else {
            self.warn_once(
                crate::Code::TilesetMissing,
                format!("object {}: its tile's tileset is missing", o.id.0),
                vec![Element::Object(o.id.0)],
            );
            return;
        };
        if self.is_rule_slot(slot) {
            self.warn_once(crate::Code::RuleTiles, RULE_TILES.to_owned(), Vec::new());
            return;
        }
        let Some(ts) = map.tilesets.get(slot).map(|t| &t.tileset) else {
            self.failures
                .push(format!("sprite tileset {slot} is missing"));
            return;
        };
        let Some(bt_core::TileImage { image, rect }) = ts.tile_image(cell.tile()) else {
            self.no_image(slot, cell.tile().0, true);
            return;
        };
        let size = DVec2::new(rect.w.max(1) as f64, rect.h.max(1) as f64);
        // Drawn at the object's size, or its tile's when it has none (like Tiled).
        let scale = if o.size.x > 0.0 && o.size.y > 0.0 {
            o.size / size
        } else {
            DVec2::ONE
        };
        let min = bt_core::tile_object_box(
            &map.geometry(),
            o,
            o.tile.and_then(|c| c.slot()).and_then(|s| map.tileset(s)),
        )
        .map_or(DVec2::ZERO, |r| r.min);
        if scale != DVec2::ONE {
            self.prop(node, "scale", text::vector2(scale));
        }
        let animation = ts
            .tile(cell.tile())
            .map(|t| t.animation.clone())
            .unwrap_or_default();
        if animation.is_empty() {
            let texture = self.texture(image);
            self.prop(node, "texture", texture);
        } else {
            let frames = match self.frames.get(&(slot, cell.tile().0)) {
                Some(frames) => frames.clone(),
                None => {
                    let mut list = Vec::new();
                    for frame in &animation {
                        let Some(bt_core::TileImage { image, rect }) = ts.tile_image(frame.tile)
                        else {
                            continue;
                        };
                        let texture = self.region_texture(
                            image,
                            [rect.x, rect.y, rect.w, rect.h],
                            (image.declared_size.x as i32, image.declared_size.y as i32),
                        );
                        list.push(format!(
                            "{{\n\"duration\": {},\n\"texture\": {texture}\n}}",
                            text::float(frame.duration_ms as f64)
                        ));
                    }
                    let animations = format!(
                        "[{{\n\"frames\": [{}],\n\"loop\": true,\n\"name\": &\"default\",\n\"speed\": 1000.0\n}}]",
                        list.join(", ")
                    );
                    let frames =
                        self.sub("SpriteFrames", vec![("animations".to_owned(), animations)]);
                    self.frames.insert((slot, cell.tile().0), frames.clone());
                    frames
                }
            };
            self.prop(node, "sprite_frames", frames);
            self.prop(node, "autoplay", "\"default\"".to_owned());
        }
        self.prop(node, "centered", "false".to_owned());
        self.prop(node, "offset", text::vector2(min / scale));
        if cell.flip().contains(bt_core::Flip::H) {
            self.prop(node, "flip_h", "true".to_owned());
        }
        if cell.flip().contains(bt_core::Flip::V) {
            self.prop(node, "flip_v", "true".to_owned());
        }
        let whole = rect.x == 0
            && rect.y == 0
            && rect.w == image.declared_size.x as i32
            && rect.h == image.declared_size.y as i32;
        if animation.is_empty() && !whole {
            self.prop(node, "region_enabled", "true".to_owned());
            self.prop(
                node,
                "region_rect",
                text::rect2(DVec2::new(rect.x as f64, rect.y as f64), size),
            );
        }
    }

    /// An image, or a part of one (an `AtlasTexture`, made once per part).
    fn region_texture(
        &mut self,
        image: &bt_core::ImageRef,
        rect: [i32; 4],
        image_size: (i32, i32),
    ) -> String {
        let texture = self.texture(image);
        if rect == [0, 0, image_size.0, image_size.1] {
            return texture;
        }
        if let Some(id) = self.atlas_textures.get(&(texture.clone(), rect)) {
            return id.clone();
        }
        let region = text::rect2(
            DVec2::new(rect[0] as f64, rect[1] as f64),
            DVec2::new(rect[2] as f64, rect[3] as f64),
        );
        let sub = self.sub(
            "AtlasTexture",
            vec![
                ("atlas".to_owned(), texture.clone()),
                ("region".to_owned(), region),
            ],
        );
        self.atlas_textures.insert((texture, rect), sub.clone());
        sub
    }

    /// The collision shape of an area object, in the object's own coordinates; `project` maps
    /// them onto an isometric map's diamonds, where every shape is a polygon.
    fn area_shape(&mut self, node: usize, o: &MapObject, project: Option<&dyn Fn(DVec2) -> DVec2>) {
        let parent = self.node_path(node);
        let center = o.size / 2.0;
        let before = self.scene.nodes.len();
        if let Some(project) = project {
            let local = match &o.shape {
                Shape::Ellipse => collision::ellipse(o.size),
                Shape::Polygon(points) => points.clone(),
                _ => bt_core::object_local_outline(&self.map.geometry(), o, None)
                    .points()
                    .to_vec(),
            };
            let polygon: Vec<DVec2> = local.into_iter().map(project).collect();
            let child = self.node(Some(&parent), "Shape", "CollisionPolygon2D");
            self.prop(child, "polygon", text::points(&polygon));
        }
        match &o.shape {
            _ if project.is_some() => {}
            Shape::Rectangle => {
                let shape = self.sub(
                    "RectangleShape2D",
                    vec![("size".to_owned(), text::vector2(o.size))],
                );
                let child = self.node(Some(&parent), "Shape", "CollisionShape2D");
                self.prop(child, "position", text::vector2(center));
                self.prop(child, "shape", shape);
            }
            Shape::Ellipse if (o.size.x - o.size.y).abs() < 1e-9 => {
                let shape = self.sub(
                    "CircleShape2D",
                    vec![("radius".to_owned(), text::float(o.size.x / 2.0))],
                );
                let child = self.node(Some(&parent), "Shape", "CollisionShape2D");
                self.prop(child, "position", text::vector2(center));
                self.prop(child, "shape", shape);
            }
            Shape::Ellipse => {
                let child = self.node(Some(&parent), "Shape", "CollisionPolygon2D");
                self.prop(child, "polygon", text::points(&collision::ellipse(o.size)));
            }
            Shape::Capsule => {
                let (radius, height) = (o.size.x.min(o.size.y) / 2.0, o.size.x.max(o.size.y));
                let shape = self.sub(
                    "CapsuleShape2D",
                    vec![
                        ("radius".to_owned(), text::float(radius)),
                        ("height".to_owned(), text::float(height)),
                    ],
                );
                let child = self.node(Some(&parent), "Shape", "CollisionShape2D");
                self.prop(child, "position", text::vector2(center));
                if o.size.x > o.size.y {
                    self.prop(child, "rotation", text::float(std::f64::consts::FRAC_PI_2));
                }
                self.prop(child, "shape", shape);
            }
            Shape::Polygon(points) => {
                let child = self.node(Some(&parent), "Shape", "CollisionPolygon2D");
                self.prop(child, "polygon", text::points(points));
            }
            _ => {}
        }
        // The shape, if one was added, stands for no object of its own: its id comes from its
        // area's.
        if self.scene.nodes.len() > before {
            {
                let owner = self.node_id(node);
                let uid = self.names.helper(owner, "shape", 0);
                self.set_node_id(before, uid);
            }
        }
    }
}

/// `v` without floating-point noise: each coordinate snapped to a millionth when that close.
fn tidy(v: DVec2) -> DVec2 {
    let snap = |c: f64| {
        let r = (c * 1e6).round() / 1e6;
        if (r - c).abs() < 1e-9 { r } else { c }
    };
    DVec2::new(snap(v.x), snap(v.y))
}

/// `objects 3, 7 and 9`, the first few of them.
fn objects(ids: &[u32]) -> String {
    const SHOWN: usize = 8;
    let listed: Vec<String> = ids.iter().take(SHOWN).map(u32::to_string).collect();
    match ids.len() {
        1 => format!(
            "object {}",
            listed.first().map(String::as_str).unwrap_or_default()
        ),
        n if n <= SHOWN => format!(
            "objects {} and {}",
            listed
                .iter()
                .take(n - 1)
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
            listed.last().map(String::as_str).unwrap_or_default()
        ),
        n => format!("objects {} and {} more", listed.join(", "), n - SHOWN),
    }
}

/// What the root script of the scene at `res` (in `project`) exports, read as text: its own
/// script, a built-in one, or the script of the scene it inherits from. C# scripts, binary
/// scenes and exports of a base script can't be read so.
pub(super) fn scene_exports(project: &Path, res: &str) -> Exports {
    let mut res = res.to_owned();
    for _ in 0..INHERITANCE {
        if res.ends_with(".scn") {
            return Exports::Unknown("it is a binary scene, which better-tiled can't read");
        }
        let Some(file) = res.strip_prefix("res://").map(|r| project.join(r)) else {
            return Exports::Unknown("its root script can't be found");
        };
        let Ok(text) = std::fs::read_to_string(file) else {
            return Exports::Unknown("it can't be read");
        };
        let Ok(scene) = read::parse(&text) else {
            return Exports::Unknown("it isn't a scene better-tiled can read");
        };
        let Some((root, section)) = scene.root() else {
            return Exports::Unknown("it has no root node");
        };
        let by_id = |id: &str| scene.ext_resources().find(|e| e.id == id);
        let ext = |raw: &str| read::ext_resource(raw).and_then(|id| by_id(&id));
        if let Some(raw) = section.get("script") {
            if let Some(id) = read::sub_resource(raw) {
                let source = scene
                    .sub_resources()
                    .find(|(s, _)| s.id == id)
                    .and_then(|(_, s)| s.get("script/source"))
                    .and_then(read::string);
                return source.map_or(Exports::Unknown("its built-in script can't be read"), |s| {
                    Exports::Known(gd_exports(&s))
                });
            }
            return match ext(raw) {
                Some(script) if script.path.ends_with(".gd") => match script
                    .path
                    .strip_prefix("res://")
                    .and_then(|r| std::fs::read_to_string(project.join(r)).ok())
                {
                    Some(source) => Exports::Known(gd_exports(&source)),
                    None => Exports::Unknown("its root script can't be read"),
                },
                Some(script) if script.path.ends_with(".cs") => {
                    Exports::Unknown("its root script is C#, which better-tiled doesn't read")
                }
                _ => Exports::Unknown("its root script can't be found"),
            };
        }
        // No script of its own: an inherited scene's root has its base's.
        match root.instance.as_deref().and_then(by_id) {
            Some(base) => res = base.path,
            None => return Exports::Known(BTreeMap::new()),
        }
    }
    Exports::Unknown("it inherits from scenes too deep to follow")
}

/// The variables a GDScript exports, with their declared (or plainly inferred) types: those
/// declared at its top level (not in an inner class) with an `@export…` annotation before them,
/// on their line or the lines above. Groups and categories (`@export_group`, …) annotate no
/// variable.
pub(super) fn gd_exports(source: &str) -> BTreeMap<String, Option<String>> {
    let identifier = |s: &str| {
        s.find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(s.len())
    };
    let mut names = BTreeMap::new();
    let mut exported = false;
    for line in logical_lines(source) {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            exported = false;
            continue;
        }
        let mut rest = line.as_str();
        while let Some(after) = rest.strip_prefix('@') {
            let annotation = &after[..identifier(after)];
            rest = after[annotation.len()..].trim_start();
            if rest.starts_with('(') {
                rest = after_parentheses(rest).trim_start();
            }
            exported |= annotation.starts_with("export")
                && !matches!(
                    annotation,
                    "export_group" | "export_subgroup" | "export_category"
                );
        }
        if rest.is_empty() {
            continue;
        }
        if let Some(var) = rest
            .strip_prefix("var")
            .filter(|r| r.starts_with([' ', '\t']))
            .map(str::trim_start)
            && exported
            && identifier(var) > 0
        {
            let name = &var[..identifier(var)];
            names.insert(
                name.to_owned(),
                declared_type(var[name.len()..].trim_start()),
            );
        }
        exported = false;
    }
    names
}

/// The type of a variable declared with `rest` after its name: `: Type`, or `:= NodePath(…)`.
fn declared_type(rest: &str) -> Option<String> {
    if let Some(value) = rest.strip_prefix(":=").or_else(|| rest.strip_prefix('=')) {
        let value = value.trim_start();
        return (value.starts_with("NodePath(") || value.starts_with('^'))
            .then(|| "NodePath".to_owned());
    }
    let declared = rest.strip_prefix(':')?.trim_start();
    let end = declared
        .find(|c: char| c.is_whitespace() || c == '=' || c == ':')
        .unwrap_or(declared.len());
    Some(declared[..end].to_owned()).filter(|t| !t.is_empty())
}

/// What follows the parenthesis that `s` opens and the one closing it.
fn after_parentheses(s: &str) -> &str {
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return &s[i + 1..];
                }
            }
            _ => {}
        }
    }
    ""
}

/// GDScript's statements line by line: lines inside brackets or ending in `\` joined, comments
/// dropped, and strings emptied (`""`), so that nothing in them reads as code.
fn logical_lines(source: &str) -> Vec<String> {
    let chars: Vec<char> = source.chars().collect();
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut depth = 0i32;
    let mut i = 0;
    while i < chars.len() {
        let Some(&c) = chars.get(i) else { break };
        i += 1;
        match c {
            '"' | '\'' => {
                let triple = chars.get(i) == Some(&c) && chars.get(i + 1) == Some(&c);
                if triple {
                    i += 2;
                }
                while i < chars.len() {
                    if chars.get(i) == Some(&'\\') {
                        i += 2;
                    } else if chars.get(i) == Some(&c)
                        && (!triple
                            || (chars.get(i + 1) == Some(&c) && chars.get(i + 2) == Some(&c)))
                    {
                        i += if triple { 3 } else { 1 };
                        break;
                    } else {
                        i += 1;
                    }
                }
                line.push_str("\"\"");
            }
            '#' => {
                while i < chars.len() && chars.get(i) != Some(&'\n') {
                    i += 1;
                }
            }
            '(' | '[' | '{' => {
                depth += 1;
                line.push(c);
            }
            ')' | ']' | '}' => {
                depth -= 1;
                line.push(c);
            }
            '\\' if chars.get(i) == Some(&'\n') => {
                i += 1;
                line.push(' ');
            }
            '\n' if depth > 0 => line.push(' '),
            '\n' => lines.push(std::mem::take(&mut line)),
            '\r' => {}
            _ => line.push(c),
        }
    }
    lines.push(line);
    lines
}

fn projection_axes(map: &bt_core::Map) -> (DVec2, DVec2) {
    let geometry = map.geometry();
    let origin = geometry.pixel_to_screen(DVec2::ZERO);
    (
        geometry.pixel_to_screen(DVec2::X) - origin,
        geometry.pixel_to_screen(DVec2::Y) - origin,
    )
}
