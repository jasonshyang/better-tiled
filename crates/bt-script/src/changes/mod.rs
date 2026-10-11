//! What differs between two maps (`10-agents.md`, §9, §10.7), typed: the size and how far the old
//! cells moved, the tilesets added, the map's properties, and per layer, in draw order from the
//! top, whether it is new or removed, renamed, shown or hidden, locked, its cells (how many,
//! where, what they hold now, tiles placed and cleared, each terrain's cells before and after)
//! and its objects (added by ids with their kinds and places, removed, moved, changed). An edit
//! report and `bt diff` say a map's changes in these words.

mod cells;
mod objects;

pub use cells::{CellChanges, TerrainCells};
pub use objects::{ObjectBrief, ObjectChanged, ObjectChanges, ObjectMoved};

pub(crate) use objects::object_kind;

use std::collections::{BTreeSet, HashMap};

use bt_core::{Layer, LayerId, LayerKind, Map, MapObject, ObjectId};
use bt_utils::text::and_list;
use glam::IVec2;
use serde::{Deserialize, Serialize};

use crate::feet::{Feet, standing_cell};
use crate::grid::sorted_cells;
use crate::names::Names;
use crate::report::Line;
use crate::words::value_text;

use cells::MapsCompared;
use objects::ObjectReader;

/// What differs between two maps.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MapChanges {
    /// The size before and after, and how far the old cells moved; none when neither changed.
    pub resized: Option<Resized>,
    /// The tilesets the later map has beyond the earlier one's, by name.
    pub tilesets_added: Vec<String>,
    /// The map's properties that were set or removed.
    pub properties: Vec<PropertyChange>,
    /// The layers that changed, in draw order from the top, then the layers removed.
    pub layers: Vec<LayerChanges>,
    /// Whether the layers both maps have are in another order.
    pub reordered: bool,
}

/// A map's size before and after, and how far its old cells moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resized {
    /// The size before, in cells.
    pub before: IVec2,
    /// The size after.
    pub after: IVec2,
    /// How far the old cells moved; what else changed is said of them where they are now.
    pub shift: IVec2,
}

/// A map property set or removed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PropertyChange {
    /// The property.
    pub key: String,
    /// Its value before, as a script writes it; none when it is new.
    pub before: Option<String>,
    /// Its value after; none when it was removed.
    pub after: Option<String>,
}

/// What changed in one layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerChanges {
    /// The layer's id.
    pub id: u32,
    /// Its name (as it was, for a layer removed).
    pub name: String,
    /// What kind of layer it is: `tile layer`, `object layer`, `image layer`, `group`.
    pub kind: String,
    /// Whether the layer is new.
    pub new: bool,
    /// Whether the layer was removed.
    pub removed: bool,
    /// Its name before, when it was renamed.
    pub renamed_from: Option<String>,
    /// Whether it is shown now, when that changed.
    pub shown: Option<bool>,
    /// Whether it is locked now, when that changed.
    pub locked: Option<bool>,
    /// What changed in its cells.
    pub cells: Option<CellChanges>,
    /// What changed among its objects.
    pub objects: Option<ObjectChanges>,
}

impl MapChanges {
    /// What differs from `before` to `after`. Layers are matched by id, objects by id.
    pub fn between(before: &Map, after: &Map) -> MapChanges {
        let mut feet = Feet::new();
        MapChanges::between_with(before, after, &mut feet)
    }

    /// [`MapChanges::between`], measuring sprites' feet with `feet`.
    pub(crate) fn between_with(before: &Map, after: &Map, feet: &mut Feet) -> MapChanges {
        let names_before = Names::of(before);
        let names_after = Names::of(after);
        let maps = MapsCompared {
            before,
            after,
            names_before: &names_before,
            names_after: &names_after,
        };
        let mut reader = ObjectReader { feet };
        let resized =
            (before.width != after.width || before.height != after.height).then(|| Resized {
                before: size_of(before),
                after: size_of(after),
                shift: IVec2::ZERO,
            });
        let tilesets_added = (before.tilesets.len()..after.tilesets.len())
            .filter_map(bt_core::TilesetSlot::new)
            .filter_map(|slot| names_after.tileset_name(slot).map(str::to_owned))
            .collect();
        let old_layers = layers_top_first(before);
        let new_layers = layers_top_first(after);
        let by_id: HashMap<LayerId, &Layer> = old_layers.iter().map(|l| (l.id, *l)).collect();
        let kept: BTreeSet<LayerId> = new_layers.iter().map(|l| l.id).collect();
        let mut layers = Vec::new();
        for layer in &new_layers {
            if let Some(changes) = layer_changes(maps, &mut reader, by_id.get(&layer.id), layer) {
                layers.push(changes);
            }
        }
        for layer in old_layers.iter().filter(|l| !kept.contains(&l.id)) {
            layers.push(LayerChanges {
                id: layer.id.0,
                name: layer.name.clone(),
                kind: layer.kind_name().to_owned(),
                new: false,
                removed: true,
                renamed_from: None,
                shown: None,
                locked: None,
                cells: None,
                objects: None,
            });
        }
        let common = |layers: &[&Layer], other: &dyn Fn(LayerId) -> bool| -> Vec<LayerId> {
            layers
                .iter()
                .map(|l| l.id)
                .filter(|&id| other(id))
                .collect()
        };
        let reordered = common(&old_layers, &|id| kept.contains(&id))
            != common(&new_layers, &|id| by_id.contains_key(&id));
        MapChanges {
            resized,
            tilesets_added,
            properties: property_changes(before, after),
            layers,
            reordered,
        }
    }

    /// Whether nothing differs.
    pub fn is_empty(&self) -> bool {
        self.resized.is_none()
            && self.tilesets_added.is_empty()
            && self.properties.is_empty()
            && self.layers.is_empty()
            && !self.reordered
    }

    /// The changes as lines, in the order they are listed.
    pub(crate) fn lines(&self) -> Vec<Line> {
        let mut lines = Vec::new();
        if let Some(resized) = &self.resized {
            let mut text = format!(
                "map size {}x{} → {}x{}",
                resized.before.x, resized.before.y, resized.after.x, resized.after.y
            );
            if resized.shift != IVec2::ZERO {
                text.push_str(&format!(
                    ", the old cells moved by {},{} (what follows compares with them there)",
                    resized.shift.x, resized.shift.y
                ));
            }
            lines.push(Line::fact(text));
        }
        if !self.tilesets_added.is_empty() {
            lines.push(Line::fact(format!(
                "tilesets added: {}",
                and_list(&self.tilesets_added)
            )));
        }
        for property in &self.properties {
            let text = match (&property.before, &property.after) {
                (None, Some(now)) => format!("map property {} = {now} (new)", property.key),
                (Some(was), Some(now)) => {
                    format!("map property {} = {now} (was {was})", property.key)
                }
                (_, None) => format!("map property {} removed", property.key),
            };
            lines.push(Line::fact(text));
        }
        for layer in &self.layers {
            lines.extend(layer.texts().into_iter().map(Line::fact));
        }
        if self.reordered {
            lines.push(Line::fact("layer order changed"));
        }
        lines
    }
}

impl LayerChanges {
    /// The layer's changes in words, each a line starting with its name.
    fn texts(&self) -> Vec<String> {
        let name = &self.name;
        if self.removed {
            return vec![format!("{name}: {} removed", self.kind)];
        }
        let mut texts = Vec::new();
        if self.new {
            texts.push(format!("{name}: new {}", self.kind));
        }
        if let Some(old) = &self.renamed_from {
            texts.push(format!("{name}: renamed from {old}"));
        }
        if let Some(shown) = self.shown {
            texts.push(format!(
                "{name}: {}",
                if shown { "shown" } else { "hidden" }
            ));
        }
        if let Some(locked) = self.locked {
            texts.push(format!(
                "{name}: {}",
                if locked { "locked" } else { "unlocked" }
            ));
        }
        if let Some(cells) = &self.cells {
            texts.push(format!("{name}: {}", cells.text()));
        }
        if let Some(objects) = &self.objects {
            texts.extend(
                objects
                    .texts()
                    .into_iter()
                    .map(|text| format!("{name}: {text}")),
            );
        }
        texts
    }
}

/// What changed in `layer` of the later map, which was `old` in the earlier one (none when it is
/// new); `None` when nothing did.
fn layer_changes(
    maps: MapsCompared<'_>,
    reader: &mut ObjectReader<'_>,
    old: Option<&&Layer>,
    layer: &Layer,
) -> Option<LayerChanges> {
    let old = old.copied();
    let cells = match &layer.kind {
        LayerKind::Tile(tiles) => {
            CellChanges::between(maps, old.and_then(|l| l.as_tile_layer()), tiles)
        }
        _ => None,
    };
    let objects = match &layer.kind {
        LayerKind::Object(objects) => {
            ObjectChanges::between(maps, reader, old.and_then(|l| l.as_object_layer()), objects)
        }
        _ => None,
    };
    let changes = LayerChanges {
        id: layer.id.0,
        name: layer.name.clone(),
        kind: layer.kind_name().to_owned(),
        new: old.is_none(),
        removed: false,
        renamed_from: old.filter(|o| o.name != layer.name).map(|o| o.name.clone()),
        shown: old
            .filter(|o| o.visible != layer.visible)
            .map(|_| layer.visible),
        locked: old
            .filter(|o| o.locked != layer.locked)
            .map(|_| layer.locked),
        cells,
        objects,
    };
    let changed = changes.new
        || changes.renamed_from.is_some()
        || changes.shown.is_some()
        || changes.locked.is_some()
        || changes.cells.is_some()
        || changes.objects.is_some();
    changed.then_some(changes)
}

/// The map properties set or removed.
fn property_changes(before: &Map, after: &Map) -> Vec<PropertyChange> {
    let mut changes = Vec::new();
    for (key, property) in &after.properties {
        let was = before.properties.get(key);
        if was != Some(property) {
            changes.push(PropertyChange {
                key: key.clone(),
                before: was.map(|p| value_text(&p.value)),
                after: Some(value_text(&property.value)),
            });
        }
    }
    for (key, property) in &before.properties {
        if !after.properties.contains_key(key) {
            changes.push(PropertyChange {
                key: key.clone(),
                before: Some(value_text(&property.value)),
                after: None,
            });
        }
    }
    changes
}

/// Every layer of a map, in draw order from the top.
fn layers_top_first(map: &Map) -> Vec<&Layer> {
    let mut layers = Vec::new();
    map.for_each_layer(|layer, _| layers.push(layer));
    layers.reverse();
    layers
}

/// A map's size in cells.
fn size_of(map: &Map) -> IVec2 {
    IVec2::new(
        i32::try_from(map.width).unwrap_or(i32::MAX),
        i32::try_from(map.height).unwrap_or(i32::MAX),
    )
}

/// The cells that differ between two maps, row by row: the cells of tile layers that changed,
/// and where objects that were added, removed, moved or changed stand (before and after).
pub(crate) fn changed_cells(before: &Map, after: &Map, feet: &mut Feet) -> Vec<IVec2> {
    let mut old_layers: HashMap<LayerId, &Layer> = HashMap::new();
    before.for_each_layer(|layer, _| {
        old_layers.insert(layer.id, layer);
    });
    let mut cells: Vec<IVec2> = Vec::new();
    let mut stands = |map: &Map, object: &MapObject, cells: &mut Vec<IVec2>| {
        if let Ok(point) = feet.standing_point(map, object) {
            cells.push(standing_cell(point));
        }
    };
    let mut seen: BTreeSet<LayerId> = BTreeSet::new();
    let mut new_layers = Vec::new();
    after.for_each_layer(|layer, _| new_layers.push(layer));
    for layer in new_layers {
        seen.insert(layer.id);
        let old = old_layers.get(&layer.id);
        match &layer.kind {
            LayerKind::Tile(tiles) => {
                let was = old.and_then(|l| l.as_tile_layer());
                cells.extend(cells::changed_cells(was, tiles).into_iter().map(|c| c.at));
            }
            LayerKind::Object(objects) => {
                let was: HashMap<ObjectId, &MapObject> = old
                    .and_then(|l| l.as_object_layer())
                    .map(|o| o.objects.iter().map(|o| (o.id, o)).collect())
                    .unwrap_or_default();
                for object in objects.objects.iter() {
                    match was.get(&object.id) {
                        None => stands(after, object, &mut cells),
                        Some(&old) if old != object => {
                            stands(before, old, &mut cells);
                            stands(after, object, &mut cells);
                        }
                        Some(_) => {}
                    }
                }
                let now: BTreeSet<ObjectId> = objects.objects.iter().map(|o| o.id).collect();
                for old in was.values().filter(|o| !now.contains(&o.id)) {
                    stands(before, old, &mut cells);
                }
            }
            LayerKind::Image(_) | LayerKind::Group(_) => {}
        }
    }
    for (id, layer) in &old_layers {
        if seen.contains(id) {
            continue;
        }
        if let Some(tiles) = layer.as_tile_layer() {
            cells.extend(tiles.cells().map(|placed| placed.at));
        }
        if let Some(objects) = layer.as_object_layer() {
            for object in objects.objects.iter() {
                stands(before, object, &mut cells);
            }
        }
    }
    sorted_cells(cells)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{Cell, Property, Shape, TileId, Tileset, TilesetSlot, Value, WangSetType};
    use bt_test_data::fixtures::{Atlas, MapBuilder, TerrainSetBuilder, combination_tile};
    use bt_utils::IRect;
    use glam::{DVec2, UVec2};

    use super::*;

    /// A 6×4 map of grass with a tile layer `ground`, an object layer `things` holding a spawn
    /// point at 1.5,1.5 and a zone, and a tile layer `deco`.
    fn grass_map() -> Map {
        let set = TerrainSetBuilder::new(WangSetType::Corner)
            .name("Land")
            .terrains(&["Grass", "Water"])
            .every_combination();
        let atlas = Atlas::builder().name("ground").build_embedded().unwrap();
        let tileset = set.added_to(Tileset::clone(&atlas.tileset));
        let row = "0 0 0 0 0 0";
        let grid = [row; 4].join("\n");
        let spawn = MapObject {
            class: "spawn".into(),
            shape: Shape::Point,
            pos: DVec2::new(24.0, 24.0),
            ..MapObject::default()
        };
        let zone = MapObject {
            class: "zone".into(),
            pos: DVec2::new(0.0, 0.0),
            size: DVec2::new(32.0, 16.0),
            ..MapObject::default()
        };
        MapBuilder::new(UVec2::new(16, 16), UVec2::new(6, 4))
            .tileset(Arc::new(tileset))
            .tile_layer("ground", &grid)
            .object_layer("things", vec![spawn, zone])
            .tile_layer("deco", "")
            .build()
            .unwrap()
    }

    fn tile(id: u32) -> Cell {
        Cell::new(TilesetSlot::new(0).unwrap(), TileId(id))
    }

    /// Water at the corners `water` says, on the ground layer of `map`, as a terrain brush
    /// would leave it: each cell's tile has the terrains of its four corners.
    fn paint_water(map: &mut Map, water: impl Fn(i32, i32) -> bool) {
        let corner = |x: i32, y: i32| if water(x, y) { 2 } else { 1 };
        let layer = map
            .layer_mut(LayerId(1))
            .unwrap()
            .as_tile_layer_mut()
            .unwrap();
        for y in 0..4 {
            for x in 0..6 {
                let id = combination_tile(
                    2,
                    [
                        corner(x + 1, y),
                        corner(x + 1, y + 1),
                        corner(x, y + 1),
                        corner(x, y),
                    ],
                )
                .unwrap();
                layer.set_cell(IVec2::new(x, y), tile(id.0)).unwrap();
            }
        }
    }

    #[test]
    fn cells_say_what_they_became_and_each_terrains_cells_before_and_after() {
        let before = grass_map();
        let mut after = before.clone();
        // Water on the corners 2..=3 across and 1..=2 down: cell 2,1 is wholly water, the
        // eight around it are edges.
        paint_water(&mut after, |x, y| {
            (2..=3).contains(&x) && (1..=2).contains(&y)
        });
        let changes = MapChanges::between(&before, &after);
        let ground = &changes.layers[0];
        assert_eq!(ground.name, "ground");
        let cells = ground.cells.as_ref().unwrap();
        assert_eq!(cells.count, 9);
        assert_eq!(
            cells.bounds,
            IRect::from_min_size(IVec2::new(1, 0), IVec2::new(3, 3))
        );
        assert_eq!(cells.became[0].what, "grass/water edges");
        assert_eq!(cells.became[0].cells, 8);
        assert_eq!(cells.became[1].what, "water");
        assert_eq!((cells.placed, cells.cleared), (0, 0));
        assert_eq!(
            cells.terrains,
            [
                TerrainCells {
                    terrain: "grass".into(),
                    before: 24,
                    after: 19
                },
                TerrainCells {
                    terrain: "water".into(),
                    before: 0,
                    after: 1
                }
            ],
            "a cell counts as the terrain covering more than half of it"
        );
        assert_eq!(
            cells.text(),
            "9 cells changed in 1,0 3x3: grass/water edges ×8, water ×1; grass 24 → 19 cells, water 0 → 1 cells"
        );
        let changed = changed_cells(&before, &after, &mut Feet::new());
        assert_eq!(changed.len(), 9);
        assert_eq!(changed[0], IVec2::new(1, 0));
    }

    #[test]
    fn tiles_placed_and_cleared_are_counted() {
        let before = grass_map();
        let mut after = before.clone();
        let deco = after
            .layer_mut(LayerId(3))
            .unwrap()
            .as_tile_layer_mut()
            .unwrap();
        deco.set_cell(IVec2::new(0, 0), tile(20)).unwrap();
        deco.set_cell(IVec2::new(1, 0), tile(20)).unwrap();
        let ground = after
            .layer_mut(LayerId(1))
            .unwrap()
            .as_tile_layer_mut()
            .unwrap();
        ground.set_cell(IVec2::new(5, 3), Cell::EMPTY).unwrap();
        let changes = MapChanges::between(&before, &after);
        let lines: Vec<String> = changes.lines().into_iter().map(|l| l.text).collect();
        assert_eq!(
            lines,
            [
                "deco: 2 cells changed in 0,0 2x1: ground:20 ×2",
                "ground: 1 cell changed in 5,3 1x1: nothing ×1; grass 24 → 23 cells"
            ]
        );
        let deco = &changes.layers[0].cells.as_ref().unwrap();
        assert_eq!((deco.placed, deco.cleared), (2, 0));
        let ground = &changes.layers[1].cells.as_ref().unwrap();
        assert_eq!((ground.placed, ground.cleared), (0, 1));
    }

    #[test]
    fn objects_added_removed_moved_and_changed() {
        let before = grass_map();
        let mut after = before.clone();
        let things = after
            .layer_mut(LayerId(2))
            .unwrap()
            .as_object_layer_mut()
            .unwrap();
        let list = Arc::make_mut(&mut things.objects);
        // The spawn moves two cells right and gets a wave; the zone goes; three points come.
        list[0].pos.x += 32.0;
        list[0]
            .properties
            .insert("wave".into(), Property::new(Value::Int(2)));
        list.remove(1);
        for (id, x) in [(5, 0.5), (6, 1.5), (8, 2.5)] {
            list.push(MapObject {
                id: ObjectId(id),
                class: "coin".into(),
                shape: Shape::Point,
                pos: DVec2::new(x * 16.0, 48.0),
                ..MapObject::default()
            });
        }
        let changes = MapChanges::between(&before, &after);
        let objects = changes.layers[0].objects.as_ref().unwrap();
        assert_eq!(objects.added.len(), 3);
        assert_eq!(objects.added[0].at, DVec2::new(0.5, 3.0));
        assert_eq!(objects.removed[0].kind, "zone");
        assert_eq!(objects.removed[0].size, Some(DVec2::new(2.0, 1.0)));
        assert_eq!(
            objects.moved,
            [ObjectMoved {
                id: 1,
                kind: "spawn".into(),
                from: DVec2::new(1.5, 1.5),
                to: DVec2::new(3.5, 1.5)
            }]
        );
        assert_eq!(objects.changed[0].what, ["wave=2"]);
        let lines: Vec<String> = changes.lines().into_iter().map(|l| l.text).collect();
        assert_eq!(
            lines,
            [
                "things: +3 objects #5-6 #8: #5 coin at 0.5,3; #6 coin at 1.5,3; #8 coin at 2.5,3",
                "things: -1 object: #2 zone at 0,0",
                "things: moved 1 object: #1 spawn 1.5,1.5 → 3.5,1.5",
                "things: changed 1 object: #1 spawn wave=2",
            ]
        );
        let cells = changed_cells(&before, &after, &mut Feet::new());
        assert!(cells.contains(&IVec2::new(1, 1)) && cells.contains(&IVec2::new(3, 1)));
        assert!(
            cells.contains(&IVec2::new(0, 2)),
            "a point on a bottom edge stands above it"
        );
    }

    #[test]
    fn layers_and_the_map_say_what_changed_about_them() {
        let before = grass_map();
        let mut after = before.clone();
        let deco = after.layer_mut(LayerId(3)).unwrap();
        deco.name = "flowers".into();
        deco.visible = false;
        deco.locked = true;
        after.layers.retain(|l| l.id != LayerId(2));
        after.properties.insert(
            "title".into(),
            Property::new(Value::String("Ice Cave".into())),
        );
        bt_edit::resize_map(&mut after, UVec2::new(8, 4), IVec2::ZERO, false).unwrap();
        let changes = MapChanges::between(&before, &after);
        let lines: Vec<String> = changes.lines().into_iter().map(|l| l.text).collect();
        assert_eq!(
            lines,
            [
                "map size 6x4 → 8x4",
                "map property title = \"Ice Cave\" (new)",
                "flowers: renamed from deco",
                "flowers: hidden",
                "flowers: locked",
                "things: object layer removed",
            ]
        );
        assert!(!changes.reordered);
        assert!(MapChanges::between(&before, &before).is_empty());
    }

    #[test]
    fn changes_round_trip_through_json() {
        let before = grass_map();
        let mut after = before.clone();
        paint_water(&mut after, |x, y| x >= 3 && y >= 2);
        let changes = MapChanges::between(&before, &after);
        let json = serde_json::to_string(&changes).unwrap();
        assert_eq!(serde_json::from_str::<MapChanges>(&json).unwrap(), changes);
    }
}
