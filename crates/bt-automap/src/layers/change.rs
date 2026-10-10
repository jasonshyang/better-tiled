//! What changed between two versions of a painted map, as AutoMap layers' update needs to know
//! it (`08-automap.md`, §10): nothing the rules see, the cells of tile layers inside a rectangle
//! (a region run then recomputes the result there), or more (a full run).

use std::collections::BTreeSet;
use std::sync::Arc;

use bt_core::{CHUNK_SIZE, Chunk, ChunkPos, Layer, LayerKind, Map, TileLayer, TilesetRef};
use bt_utils::IRect;
use glam::IVec2;

/// What changed between two versions of a painted map.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MapChange {
    /// Nothing.
    Nothing,
    /// Only cells of tile layers, all inside this rectangle.
    Cells(IRect),
    /// More than cells: the layers themselves (added, removed, moved, renamed, locked, …),
    /// objects, images, tilesets or the map's own attributes.
    Everything,
}

impl MapChange {
    /// What changed from `before` to `after`.
    pub fn between(before: &Map, after: &Map) -> MapChange {
        if std::ptr::eq(before, after) {
            return MapChange::Nothing;
        }
        if !same_tilesets(&before.tilesets, &after.tilesets) || shell(before) != shell(after) {
            return MapChange::Everything;
        }
        let mut old_layers = Vec::new();
        before.for_each_layer(|layer, _| old_layers.push(layer));
        let mut new_layers = Vec::new();
        after.for_each_layer(|layer, _| new_layers.push(layer));
        if old_layers.len() != new_layers.len() {
            return MapChange::Everything;
        }
        let mut changed: Option<IRect> = None;
        for (old, new) in old_layers.into_iter().zip(new_layers) {
            if without_contents(old) != without_contents(new) {
                return MapChange::Everything;
            }
            match (&old.kind, &new.kind) {
                (LayerKind::Tile(a), LayerKind::Tile(b)) => {
                    if a.size() != b.size() || a.is_infinite() != b.is_infinite() {
                        return MapChange::Everything;
                    }
                    if let Some(cells) = changed_cells(a, b) {
                        changed = Some(changed.map_or(cells, |c| c.union(cells)));
                    }
                }
                (LayerKind::Object(a), LayerKind::Object(b)) => {
                    let same = a.color == b.color
                        && a.draw_order == b.draw_order
                        && (Arc::ptr_eq(&a.objects, &b.objects) || a.objects == b.objects);
                    if !same {
                        return MapChange::Everything;
                    }
                }
                (LayerKind::Group(_), LayerKind::Group(_)) => {}
                (a, b) if a == b => {}
                _ => return MapChange::Everything,
            }
        }
        changed.map_or(MapChange::Nothing, MapChange::Cells)
    }
}

/// Whether two maps use the same tilesets: the same files at the same places, each the same
/// value (shared values compared by address).
fn same_tilesets(a: &[TilesetRef], b: &[TilesetRef]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.file == y.file
                && x.first_gid == y.first_gid
                && (Arc::ptr_eq(&x.tileset, &y.tileset) || x.tileset == y.tileset)
        })
}

/// The map without its layers and tilesets, to compare what else it holds.
fn shell(map: &Map) -> Map {
    Map {
        layers: Vec::new(),
        tilesets: Vec::new(),
        ..map.clone()
    }
}

/// The layer without what it holds, to compare its own attributes.
fn without_contents(layer: &Layer) -> Layer {
    Layer {
        kind: LayerKind::Group(Vec::new()),
        ..layer.clone()
    }
}

/// The smallest rectangle holding every cell that differs between two tile layers of the same
/// size; chunks they share are skipped by address.
fn changed_cells(a: &TileLayer, b: &TileLayer) -> Option<IRect> {
    let positions: BTreeSet<ChunkPos> = a.chunks().chain(b.chunks()).map(|c| c.pos).collect();
    let mut changed: Option<IRect> = None;
    for pos in positions {
        let (x, y) = (a.chunk(pos), b.chunk(pos));
        if let (Some(x), Some(y)) = (x, y)
            && (Arc::ptr_eq(x, y) || x == y)
        {
            continue;
        }
        if let Some(cells) = differing_cells(pos, x, y) {
            changed = Some(changed.map_or(cells, |c| c.union(cells)));
        }
    }
    changed
}

/// The cells of the chunk at `pos` that differ between two versions of it (none is empty).
fn differing_cells(pos: ChunkPos, a: Option<&Arc<Chunk>>, b: Option<&Arc<Chunk>>) -> Option<IRect> {
    let origin = pos.rect();
    let size = CHUNK_SIZE;
    let cell = |chunk: Option<&Arc<Chunk>>, p: IVec2| {
        chunk.map_or(bt_core::Cell::EMPTY, |chunk| chunk.cell(p))
    };
    IRect::bounding(
        (0..size)
            .flat_map(|dy| (0..size).map(move |dx| IVec2::new(dx, dy)))
            .filter_map(|d| {
                let p = IVec2::new(origin.x, origin.y) + d;
                (cell(a, p) != cell(b, p)).then_some(p)
            }),
    )
}

#[cfg(test)]
mod tests {
    use bt_core::{Cell, LayerId, TileId, TilesetSlot};
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::UVec2;

    use super::*;

    fn map() -> Map {
        let atlas = Atlas::builder().build_embedded().unwrap().tileset;
        MapBuilder::new(UVec2::splat(16), UVec2::new(40, 40))
            .tileset(atlas)
            .tile_layer("ground", &vec![". ".repeat(40); 40].join("\n"))
            .build()
            .unwrap()
    }

    fn paint(map: &mut Map, at: IVec2) {
        let tiles = map
            .layer_mut(LayerId(1))
            .and_then(Layer::as_tile_layer_mut)
            .unwrap();
        tiles
            .set_cell(at, Cell::new(TilesetSlot::new(0).unwrap(), TileId(3)))
            .unwrap();
    }

    #[test]
    fn painted_cells_change_a_rectangle_and_anything_else_everything() {
        let before = map();
        assert_eq!(
            MapChange::between(&before, &before.clone()),
            MapChange::Nothing
        );
        let mut after = before.clone();
        paint(&mut after, IVec2::new(3, 4));
        paint(&mut after, IVec2::new(20, 30));
        assert_eq!(
            MapChange::between(&before, &after),
            MapChange::Cells(IRect {
                x: 3,
                y: 4,
                w: 18,
                h: 27
            })
        );
        let mut renamed = before.clone();
        renamed.layers[0].name = "other".to_owned();
        assert_eq!(MapChange::between(&before, &renamed), MapChange::Everything);
        let mut locked = before.clone();
        locked.layers[0].locked = true;
        assert_eq!(MapChange::between(&before, &locked), MapChange::Everything);
        let mut resized = before.clone();
        resized.width = 41;
        assert_eq!(MapChange::between(&before, &resized), MapChange::Everything);
    }
}
