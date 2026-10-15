//! Tiles' collision shapes as polygons of the TileSet's physics layers (docs/design/12-godot-export.md): each on
//! the layer its `godotPhysicsLayer` names (0 by default), one-way with `godotOneWay` and
//! `godotOneWayMargin`, and out of physics with `godotCollision = false`. Rectangles, ellipses
//! and capsules become polygons; points, polylines and text have no area and are left out, with
//! a note. A tileset with an isometric grid has its shapes projected onto it, as Tiled draws
//! them. The physics layers' bits come from the setup's `physicsLayers`.
//!
//! A shape with `godotNavigationLayer` is also walkable on that navigation layer: a
//! `NavigationPolygon` of convex parts, since Godot's navigation finds its way over those and not
//! over outlines. With `godotOcclusionLayer` it is also an `OccluderPolygon2D` of that occlusion
//! layer. Their layers' bits come from the setup's `navigationLayers` and `occlusionLayers`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use bt_core::{GridOrientation, MapGeometry, MapObject, Orientation, Properties, Shape, Tileset};
use glam::{DVec2, IVec2};

use super::conventions::{
    GODOT_COLLISION, GODOT_NAVIGATION_LAYER, GODOT_OCCLUSION_LAYER, GODOT_ONE_WAY,
    GODOT_ONE_WAY_MARGIN, GODOT_PHYSICS_LAYER,
};
use super::report::{Element, Note, Severity};
use super::tileset::{TileSetBuilder, id_runs};
use super::{read, text};
use crate::build::Build;

use bt_project::{GodotSetup, PhysicsLayerSetup};

/// The highest `godotPhysicsLayer`: the TileSet has every layer up to the highest one used.
const MAX_PHYSICS_LAYER: i64 = 255;

const NO_AREA: &str = "tile collision shapes that are points, polylines, text or tiles have no area, and Godot's physics layers take only polygons: they are left out: draw them as rectangles, ellipses or polygons";

/// A tile's collision shape as Godot takes it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct TileShape {
    /// The collision object's id in the tile's object group.
    pub object: u32,
    /// Its outline, from the point `tile_shapes` was given.
    pub points: Vec<DVec2>,
    /// Its physics layer; `None` keeps it out of physics (`godotCollision = false`).
    pub physics_layer: Option<usize>,
    pub one_way: bool,
    /// `None` leaves Godot's default (1 px).
    pub one_way_margin: Option<f64>,
    /// Its navigation layer (`godotNavigationLayer`), when it is walkable.
    pub navigation_layer: Option<usize>,
    /// Its occlusion layer (`godotOcclusionLayer`), when it casts shadows.
    pub occlusion_layer: Option<usize>,
}

impl Build<'_> {
    /// The collision shapes of tile `id` of `ts` (the tileset in `slot`), drawn `size` px, with
    /// their points from `center` (the TileSet's polygons are around the tile's centre). Takes
    /// the shapes' `godot*` settings.
    pub(super) fn tile_shapes(
        &mut self,
        ts: &Tileset,
        slot: usize,
        id: u32,
        size: DVec2,
        center: DVec2,
    ) -> Vec<TileShape> {
        let Some(data) = ts.tile(bt_core::TileId(id)) else {
            return Vec::new();
        };
        let projection = Projection::of(ts, size);
        let mut shapes = Vec::new();
        for object in data.collision.iter().flat_map(|g| g.layer.objects.iter()) {
            let (p, tile) = (&object.properties, (slot, id));
            let collides = self.setting_bool(p, GODOT_COLLISION, tile).unwrap_or(true);
            let layer = match self.setting_int(p, GODOT_PHYSICS_LAYER, tile) {
                Some(n) if (0..=MAX_PHYSICS_LAYER).contains(&n) => n as usize,
                Some(_) => {
                    let message = format!(
                        "{GODOT_PHYSICS_LAYER} takes a layer from 0 to {MAX_PHYSICS_LAYER}: shapes with another are on layer 0"
                    );
                    self.tileset
                        .data
                        .tally
                        .add(crate::Code::SettingType, message, tile);
                    0
                }
                None => 0,
            };
            let one_way = self.setting_bool(p, GODOT_ONE_WAY, tile).unwrap_or(false);
            let one_way_margin = self.setting_float(p, GODOT_ONE_WAY_MARGIN, tile);
            let (navigation, occlusion) = (
                self.area_layer(p, GODOT_NAVIGATION_LAYER, tile),
                self.area_layer(p, GODOT_OCCLUSION_LAYER, tile),
            );
            let Some(outline) = projection.outline(object) else {
                self.tileset.data.tally.add(
                    crate::Code::TileCollisionKind,
                    NO_AREA.to_owned(),
                    tile,
                );
                continue;
            };
            let points: Vec<DVec2> = outline.into_iter().map(|p| tidy(p - center)).collect();
            let navigation_layer = navigation.filter(|_| self.walkable(&points, tile));
            shapes.push(TileShape {
                object: object.id.0,
                points,
                physics_layer: collides.then_some(layer),
                one_way,
                one_way_margin,
                navigation_layer,
                occlusion_layer: occlusion,
            });
        }
        shapes
    }
}

/// How many physics layers `shapes` need: up to the highest they are on.
pub(super) fn layers_used<'s>(shapes: impl IntoIterator<Item = &'s TileShape>) -> usize {
    shapes
        .into_iter()
        .filter_map(|s| s.physics_layer)
        .map(|l| l + 1)
        .max()
        .unwrap_or(0)
}

/// A tile's physics lines (`physics_layer_N/polygon_M/…`), its shapes moved by `moved`; the
/// polygons of each layer are numbered from 0. Its occluders and navigation polygons (`areas`)
/// too, naming the sub-resources `resources` writes.
pub(super) fn tile_lines(shapes: &[TileShape], moved: DVec2) -> Vec<(String, String)> {
    // Godot's own order: occluders, physics, navigation.
    let (occluders, navigation) = areas(shapes, moved);
    let mut next: Vec<usize> = Vec::new();
    let mut lines: Vec<(String, String)> = occluders.iter().map(Area::line).collect();
    for shape in shapes {
        let Some(layer) = shape.physics_layer else {
            continue;
        };
        if next.len() <= layer {
            next.resize(layer + 1, 0);
        }
        let Some(index) = next.get_mut(layer) else {
            continue;
        };
        let polygon = format!("physics_layer_{layer}/polygon_{}", *index);
        *index += 1;
        let points: Vec<DVec2> = shape.points.iter().map(|p| *p + moved).collect();
        lines.push((format!("{polygon}/points"), text::points(&points)));
        if shape.one_way {
            lines.push((format!("{polygon}/one_way"), "true".to_owned()));
        }
        if let Some(margin) = shape.one_way_margin {
            lines.push((format!("{polygon}/one_way_margin"), text::float(margin)));
        }
    }
    lines.extend(navigation.iter().map(Area::line));
    lines
}

/// The TileSet's physics layers: every one the setup lists, and as many more as the tiles use.
/// A layer the setup doesn't list has Godot's defaults (layer and mask 1). Writing a layer's
/// `collision_layer` is what makes it; the mask is written when it isn't Godot's default.
pub(super) fn tileset_lines(setup: &[PhysicsLayerSetup], used: usize) -> Vec<(String, String)> {
    let mut lines = Vec::new();
    for n in 0..used.max(setup.len()) {
        let (layer, mask) = setup.get(n).map_or((1, 1), |s| {
            (
                s.collision_layer_or_default(),
                s.collision_mask_or_default(),
            )
        });
        lines.push((
            format!("physics_layer_{n}/collision_layer"),
            layer.to_string(),
        ));
        if mask != 1 {
            lines.push((
                format!("physics_layer_{n}/collision_mask"),
                mask.to_string(),
            ));
        }
    }
    lines
}

// Navigation and occlusion.

const NOT_WALKABLE: &str = "tile collision shapes on a navigation layer whose edges cross or touch, or that have no area, can't be split into the convex polygons Godot's navigation takes: they are left out of navigation: redraw them so that their edges don't meet";

const SPLIT: &str = "tile collision shapes on a navigation layer that aren't convex are split into convex polygons, which is what Godot's navigation takes";

impl Build<'_> {
    /// A shape's navigation or occlusion layer (`godotNavigationLayer`, `godotOcclusionLayer`).
    fn area_layer(
        &mut self,
        properties: &Properties,
        name: &'static str,
        tile: (usize, u32),
    ) -> Option<usize> {
        match self.setting_int(properties, name, tile)? {
            n if (0..=MAX_PHYSICS_LAYER).contains(&n) => Some(n as usize),
            _ => {
                let message = format!(
                    "{name} takes a layer from 0 to {MAX_PHYSICS_LAYER}: shapes with another are on layer 0"
                );
                self.tileset
                    .data
                    .tally
                    .add(crate::Code::SettingType, message, tile);
                Some(0)
            }
        }
    }

    /// Whether a shape's outline can be navigation polygons; noted when it can't.
    fn walkable(&mut self, points: &[DVec2], tile: (usize, u32)) -> bool {
        let walkable = convex_parts(points).is_some();
        if !walkable {
            self.tileset.data.tally.add(
                crate::Code::NavigationShape,
                NOT_WALKABLE.to_owned(),
                tile,
            );
        }
        walkable
    }

    /// The notes on the tiles' navigation and occlusion, once the TileSet's tiles are made: the
    /// shapes split into convex polygons, and the layers used that the setup has no entry for.
    pub(super) fn area_notes(&mut self) {
        let mut split: BTreeMap<usize, BTreeSet<u32>> = BTreeMap::new();
        for def in self.tileset.sources.iter().flat_map(|s| s.tiles.values()) {
            if def.polygons.iter().any(|s| {
                s.navigation_layer.is_some()
                    && convex_parts(&s.points).is_some_and(|(_, parts)| parts.len() > 1)
            }) {
                split.entry(def.slot).or_default().insert(def.id);
            }
        }
        if !split.is_empty() {
            let map = self.map;
            let places: Vec<String> = split
                .iter()
                .map(|(&slot, ids)| {
                    let ids: Vec<u32> = ids.iter().copied().collect();
                    let name = map
                        .tilesets
                        .get(slot)
                        .map_or("", |t| t.tileset.name.as_str());
                    let count = if ids.len() == 1 {
                        "tile".to_owned()
                    } else {
                        format!("{} tiles", ids.len())
                    };
                    format!("tileset {name}: {count} {}", id_runs(&ids))
                })
                .collect();
            let elements = split
                .iter()
                .flat_map(|(&slot, ids)| {
                    ids.iter()
                        .map(move |&id| Element::Tile { tileset: slot, id })
                })
                .collect();
            self.notes.push(Note {
                code: crate::Code::NavigationSplit,
                severity: Severity::Info,
                message: format!("{SPLIT} ({})", places.join("; ")),
                elements,
            });
        }

        // As for physics layers, layer 0 is never noted.
        let (navigation, occlusion) = areas_used(&self.tileset);
        let unset = [
            (
                crate::Code::NavigationLayerUnset,
                "navigation",
                "navigationLayers",
                self.setup
                    .navigation_layers
                    .as_deref()
                    .unwrap_or_default()
                    .len(),
                navigation,
                "navigation layers 1",
            ),
            (
                crate::Code::OcclusionLayerUnset,
                "occlusion",
                "occlusionLayers",
                self.setup
                    .occlusion_layers
                    .as_deref()
                    .unwrap_or_default()
                    .len(),
                occlusion,
                "light mask 1",
            ),
        ];
        for (code, kind, key, set_up, used, default) in unset {
            let set_up = set_up.max(1);
            if used <= set_up {
                continue;
            }
            let (layers, them) = match used - set_up {
                1 => (format!("{kind} layer {set_up} has"), "it"),
                _ => (format!("{kind} layers {set_up}–{} have", used - 1), "each"),
            };
            self.warn_once(code, format!("{layers} no entry in {key} of better-tiled.json's godot setup: Godot gives {them} {default}"), Vec::new());
        }
    }
}

/// A tile's occluder or navigation polygon: its line in the tile, and the sub-resource it names.
struct Area {
    key: String,
    kind: &'static str,
    /// The sub-resource's lines.
    body: String,
    /// The sub-resource's id, from what it holds: a shape on many tiles is one sub-resource, and
    /// ids don't depend on which tiles the TileSet has or in what order.
    id: String,
}

impl Area {
    fn new(key: String, kind: &'static str, body: String) -> Area {
        let hash = read::uid_to_text(read::fnv1a64(body.as_bytes()) >> 1);
        let id = format!("{kind}_{}", hash.trim_start_matches("uid://"));
        Area {
            key,
            kind,
            body,
            id,
        }
    }

    fn line(&self) -> (String, String) {
        (self.key.clone(), format!("SubResource(\"{}\")", self.id))
    }
}

/// A tile's occluders (`occlusion_layer_N/polygon_M/polygon`, numbered per layer) and navigation
/// polygons (`navigation_layer_N/polygon`, one per layer, holding the convex parts of every shape
/// on it), its shapes moved by `moved`.
fn areas(shapes: &[TileShape], moved: DVec2) -> (Vec<Area>, Vec<Area>) {
    let mut occluders = Vec::new();
    let mut next: BTreeMap<usize, usize> = BTreeMap::new();
    for shape in shapes {
        let Some(layer) = shape.occlusion_layer else {
            continue;
        };
        let n = next.entry(layer).or_default();
        let points: Vec<DVec2> = shape.points.iter().map(|p| *p + moved).collect();
        occluders.push(Area::new(
            format!("occlusion_layer_{layer}/polygon_{n}/polygon"),
            "OccluderPolygon2D",
            format!("polygon = {}\n", text::points(&points)),
        ));
        *n += 1;
    }

    // Each layer's vertices, convex parts (by index into them) and outlines.
    type Walkable = (Vec<DVec2>, Vec<Vec<usize>>, Vec<Vec<DVec2>>);
    let mut layers: BTreeMap<usize, Walkable> = BTreeMap::new();
    for shape in shapes {
        let Some(layer) = shape.navigation_layer else {
            continue;
        };
        let Some((outline, parts)) = convex_parts(&shape.points) else {
            continue;
        };
        let (vertices, polygons, outlines) = layers.entry(layer).or_default();
        let outline: Vec<DVec2> = outline.iter().map(|p| *p + moved).collect();
        polygons.extend(
            parts
                .into_iter()
                .map(|part| part.into_iter().map(|i| i + vertices.len()).collect()),
        );
        vertices.extend(&outline);
        outlines.push(outline);
    }
    let navigation = layers
        .into_iter()
        .map(|(layer, (vertices, polygons, outlines))| {
            let polygons: Vec<String> = polygons.iter().map(|p| format!("PackedInt32Array({})", p.iter().map(usize::to_string).collect::<Vec<_>>().join(", "))).collect();
            let outlines: Vec<String> = outlines.iter().map(|o| text::points(o)).collect();
            let body = format!(
                "vertices = {}\npolygons = Array[PackedInt32Array]([{}])\noutlines = Array[PackedVector2Array]([{}])\n",
                text::points(&vertices),
                polygons.join(", "),
                outlines.join(", ")
            );
            Area::new(format!("navigation_layer_{layer}/polygon"), "NavigationPolygon", body)
        })
        .collect();
    (occluders, navigation)
}

/// The sub-resources that the occluders and navigation polygons of every alternative of every
/// tile name, each once, by id: they go before the atlas sources, which Godot reads them into.
/// `kept` (types and lines by id) are a shared TileSet's kept sources' (shared.rs), among them:
/// an id is its contents', so one both have is written once.
pub(super) fn resources(
    tileset: &TileSetBuilder,
    kept: BTreeMap<String, (String, String)>,
) -> String {
    let mut found = kept;
    for def in tileset.sources.iter().flat_map(|s| s.tiles.values()) {
        // Alternative 0, then the flipped ones, moved as grid.rs `flipped_lines` moves them.
        let moves = std::iter::once(DVec2::ZERO).chain(
            def.flipped
                .iter()
                .map(|f| (def.origin - f.origin).as_dvec2()),
        );
        for moved in moves {
            let (occluders, navigation) = areas(&def.polygons, moved);
            found.extend(
                occluders
                    .into_iter()
                    .chain(navigation)
                    .map(|area| (area.id, (area.kind.to_owned(), area.body))),
            );
        }
    }
    let mut out = String::new();
    for (id, (kind, body)) in &found {
        let _ = write!(out, "\n[sub_resource type=\"{kind}\" id=\"{id}\"]\n{body}");
    }
    out
}

/// How many navigation and occlusion layers the TileSet's tiles use, a shared TileSet's kept
/// sources included.
fn areas_used(tileset: &TileSetBuilder) -> (usize, usize) {
    let shapes = || {
        tileset
            .sources
            .iter()
            .flat_map(|s| s.tiles.values())
            .flat_map(|t| &t.polygons)
    };
    let kept = |prefix: &str| tileset.shared.as_ref().map_or(0, |s| s.kept_uses(prefix));
    let navigation = shapes()
        .filter_map(|s| s.navigation_layer)
        .map(|l| l + 1)
        .max()
        .unwrap_or(0)
        .max(kept("navigation_layer_"));
    let occlusion = shapes()
        .filter_map(|s| s.occlusion_layer)
        .map(|l| l + 1)
        .max()
        .unwrap_or(0)
        .max(kept("occlusion_layer_"));
    (navigation, occlusion)
}

/// The TileSet's occlusion layers (`occlusion_layer_N/light_mask`) and navigation layers
/// (`navigation_layer_N/layers`): every one the setup lists, and as many more as the tiles use,
/// with Godot's default (1) where the setup has no entry. Writing the line is what makes the layer.
pub(super) fn area_lines(setup: &GodotSetup, tileset: &TileSetBuilder) -> Vec<(String, String)> {
    let (navigation, occlusion) = areas_used(tileset);
    let mut lines = Vec::new();
    for n in 0..occlusion.max(setup.occlusion_layers.as_deref().unwrap_or_default().len()) {
        let mask = setup
            .occlusion_layers
            .as_deref()
            .unwrap_or_default()
            .get(n)
            .map_or(1, |l| l.light_mask_or_default());
        lines.push((format!("occlusion_layer_{n}/light_mask"), mask.to_string()));
    }
    for n in 0..navigation.max(setup.navigation_layers.as_deref().unwrap_or_default().len()) {
        let layers = setup
            .navigation_layers
            .as_deref()
            .unwrap_or_default()
            .get(n)
            .map_or(1, |l| l.layers_or_default());
        lines.push((format!("navigation_layer_{n}/layers"), layers.to_string()));
    }
    lines
}

/// An outline as Godot's navigation takes it: without repeated points or points along a straight
/// edge, turning clockwise on screen as Godot's own tiles do, and split into convex parts (by
/// index into those points). The parts are an ear-clipped triangulation merged across its
/// diagonals while they stay convex (Hertel-Mehlhorn: at most four times the fewest). `None` for
/// an outline whose edges cross or touch, or that has no area.
pub(super) fn convex_parts(outline: &[DVec2]) -> Option<(Vec<DVec2>, Vec<Vec<usize>>)> {
    let (min, max) = outline
        .iter()
        .fold((DVec2::INFINITY, DVec2::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(*p), hi.max(*p))
        });
    let span = (max - min).max_element();
    if !span.is_finite() || span <= 0.0 {
        return None;
    }
    // Turns (twice a triangle's area) this small are rounding noise.
    let eps = span * span * 1e-9;
    let mut ring: Vec<DVec2> = Vec::new();
    for &p in outline {
        if ring.last().is_none_or(|q| q.distance(p) > span * 1e-9) {
            ring.push(p);
        }
    }
    while ring.len() > 1 && ring.first()?.distance(*ring.last()?) <= span * 1e-9 {
        ring.pop();
    }
    // A point along a straight edge, or at the tip of a spike without width, turns nothing.
    loop {
        let n = ring.len();
        if n < 3 {
            return None;
        }
        let Some(i) = (0..n)
            .find(|&i| ring_corner(&ring, i).is_some_and(|[a, b, c]| turn(a, b, c).abs() <= eps))
        else {
            break;
        };
        ring.remove(i);
    }
    let n = ring.len();
    let area: f64 = bt_core::closed_segments(&ring)
        .map(|edge| edge.from.perp_dot(edge.to))
        .sum::<f64>()
        / 2.0;
    if area.abs() <= eps {
        return None;
    }
    if area < 0.0 {
        ring.reverse();
    }
    if !simple(&ring, eps) {
        return None;
    }
    let convex = |part: &[usize]| {
        let Some(points) = part
            .iter()
            .map(|&i| ring.get(i).copied())
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        (0..points.len())
            .all(|i| ring_corner(&points, i).is_some_and(|[a, b, c]| turn(a, b, c) >= -eps))
    };
    let whole: Vec<usize> = (0..n).collect();
    if convex(&whole) {
        return Some((ring, vec![whole]));
    }
    let mut parts = ear_clip(&ring, eps)?;
    'merge: loop {
        for a in 0..parts.len() {
            for b in a + 1..parts.len() {
                if let Some(merged) = joined(parts.get(a)?, parts.get(b)?).filter(|m| convex(m)) {
                    *parts.get_mut(a)? = merged;
                    parts.remove(b);
                    continue 'merge;
                }
            }
        }
        break;
    }
    Some((ring, parts))
}

/// How much the way from `a` through `b` to `c` turns at `b`, clockwise on screen: twice the
/// triangle's area, negative when it turns the other way.
fn turn(a: DVec2, b: DVec2, c: DVec2) -> f64 {
    (b - a).perp_dot(c - b)
}

/// Whether a ring's edges meet only where neighbours join.
fn simple(ring: &[DVec2], eps: f64) -> bool {
    let edges: Vec<_> = bt_core::closed_segments(ring).collect();
    edges.iter().enumerate().all(|(i, a)| {
        edges
            .iter()
            .enumerate()
            .skip(i + 2)
            .filter(|(j, _)| i > 0 || *j + 1 < edges.len())
            .all(|(_, b)| !meet((a.from, a.to), (b.from, b.to), eps))
    })
}

/// Whether two segments cross or touch.
fn meet((a, b): (DVec2, DVec2), (c, d): (DVec2, DVec2), eps: f64) -> bool {
    let (ta, tb, tc, td) = (turn(c, d, a), turn(c, d, b), turn(a, b, c), turn(a, b, d));
    let apart = |x: f64, y: f64| x > eps && y < -eps || x < -eps && y > eps;
    if apart(ta, tb) && apart(tc, td) {
        return true;
    }
    // A point on the other segment.
    let slack = DVec2::splat(eps.sqrt());
    let on = |p: DVec2, q: DVec2, r: DVec2, t: f64| {
        t.abs() <= eps && r.cmpge(p.min(q) - slack).all() && r.cmple(p.max(q) + slack).all()
    };
    on(c, d, a, ta) || on(c, d, b, tb) || on(a, b, c, tc) || on(a, b, d, td)
}

/// A simple ring turning clockwise cut into triangles (by index), ear by ear.
fn ear_clip(ring: &[DVec2], eps: f64) -> Option<Vec<Vec<usize>>> {
    let inside = |p: DVec2, a: DVec2, b: DVec2, c: DVec2| {
        turn(a, b, p) >= -eps && turn(b, c, p) >= -eps && turn(c, a, p) >= -eps
    };
    let mut left: Vec<usize> = (0..ring.len()).collect();
    let mut triangles = Vec::new();
    while left.len() > 3 {
        let m = left.len();
        let corner = |i: usize| -> Option<[usize; 3]> {
            Some([
                *left.get((i + m - 1) % m)?,
                *left.get(i)?,
                *left.get((i + 1) % m)?,
            ])
        };
        let ear = (0..m).find(|&i| {
            let Some([a, b, c]) = corner(i) else {
                return false;
            };
            let Some([pa, pb, pc]) = indexed_triangle(ring, [a, b, c]) else {
                return false;
            };
            turn(pa, pb, pc) > eps
                && left.iter().all(|&k| {
                    [a, b, c].contains(&k) || ring.get(k).is_some_and(|p| !inside(*p, pa, pb, pc))
                })
        });
        match ear {
            Some(i) => {
                let [a, b, c] = corner(i)?;
                triangles.push(vec![a, b, c]);
                left.remove(i);
            }
            // Only a point that what is left of the ring goes straight through can be dropped.
            None => {
                let i = (0..m).find(|&i| {
                    corner(i)
                        .and_then(|ids| indexed_triangle(ring, ids))
                        .is_some_and(|[a, b, c]| turn(a, b, c).abs() <= eps)
                })?;
                left.remove(i);
            }
        }
    }
    if let [a, b, c] = left.as_slice()
        && indexed_triangle(ring, [*a, *b, *c]).is_some_and(|[a, b, c]| turn(a, b, c) > eps)
    {
        triangles.push(left);
    }
    Some(triangles)
}

/// Two parts that share an edge (`u`→`v` in one, `v`→`u` in the other) as one, around both.
fn joined(a: &[usize], b: &[usize]) -> Option<Vec<usize>> {
    let na = a.len();
    let nb = b.len();
    for (i, &u) in a.iter().enumerate() {
        let v = *a.get((i + 1) % na)?;
        if let Some(j) = b
            .iter()
            .enumerate()
            .position(|(j, &p)| p == v && b.get((j + 1) % nb) == Some(&u))
        {
            let mut merged: Vec<usize> = a.iter().cycle().skip(i + 1).take(na).copied().collect();
            merged.extend(
                b.iter()
                    .cycle()
                    .skip(j + 2)
                    .take(nb.saturating_sub(2))
                    .copied(),
            );
            return Some(merged);
        }
    }
    None
}

/// Where a tileset's collision shapes go on its tiles' images: on the image itself, or, for a
/// tileset with an isometric grid, projected onto a one-cell isometric map of the grid's size
/// whose diamond is at the bottom of the image (Tiled's `paintTileCollisionShapes`).
struct Projection {
    geometry: MapGeometry,
    lift: DVec2,
}

impl Projection {
    fn of(ts: &Tileset, size: DVec2) -> Projection {
        let iso = ts.grid.filter(|g| {
            g.orientation == GridOrientation::Isometric && g.size.x > 0 && g.size.y > 0
        });
        let (geometry, lift) = match iso {
            Some(g) => (
                MapGeometry::new(
                    Orientation::Isometric,
                    IVec2::new(g.size.x as i32, g.size.y as i32),
                    IVec2::ONE,
                ),
                DVec2::new(0.0, size.y - g.size.y as f64),
            ),
            None => (
                MapGeometry::new(Orientation::Orthogonal, IVec2::ONE, IVec2::ONE),
                DVec2::ZERO,
            ),
        };
        Projection { geometry, lift }
    }

    /// A shape's outline on the tile's image, rotated around its position as Tiled rotates it
    /// (on the screen, after projection); `None` for a shape without an area.
    fn outline(&self, shape: &MapObject) -> Option<Vec<DVec2>> {
        if shape.tile.is_some()
            || matches!(
                shape.shape,
                Shape::Point | Shape::Polyline(_) | Shape::Text(_)
            )
        {
            return None;
        }
        let outline = bt_core::object_outline(&self.geometry, shape, None);
        Some(outline.points().iter().map(|p| *p + self.lift).collect())
    }
}

/// A point without the noise of rotating it (31.999999999999996 is 32).
fn tidy(p: DVec2) -> DVec2 {
    (p * 1e6).round() / 1e6
}

/// An ellipse filling `size`, as a polygon of 24 points.
pub(super) fn ellipse(size: DVec2) -> Vec<DVec2> {
    let center = size / 2.0;
    (0..24)
        .map(|i| {
            let a = i as f64 / 24.0 * std::f64::consts::TAU;
            let p = center + DVec2::new(a.cos() * center.x, a.sin() * center.y);
            DVec2::new(
                (p.x * 1000.0).round() / 1000.0,
                (p.y * 1000.0).round() / 1000.0,
            )
        })
        .collect()
}

fn ring_corner(ring: &[DVec2], i: usize) -> Option<[DVec2; 3]> {
    let n = ring.len();
    if n == 0 {
        return None;
    }
    Some([
        *ring.get((i + n - 1) % n)?,
        *ring.get(i)?,
        *ring.get((i + 1) % n)?,
    ])
}
fn indexed_triangle(ring: &[DVec2], ids: [usize; 3]) -> Option<[DVec2; 3]> {
    let [a, b, c] = ids;
    Some([*ring.get(a)?, *ring.get(b)?, *ring.get(c)?])
}
