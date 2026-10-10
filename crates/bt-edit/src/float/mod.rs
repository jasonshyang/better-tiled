//! Carried selections: [`FloatingTiles`], the tiles a tile selection lifts off the map and
//! carries while it moves, mirrors and turns, so that what they covered shows again when they
//! move on. Tiled has none; its Cut and Paste is the only way to move tiles there, and moving a
//! selection is what its users ask for most (`docs/decisions.md`, "Editing choices").
//!
//! The map always holds the tiles where they are: the editor keeps the map that
//! [`FloatingTiles::lift`] leaves without them and places them on a copy of it after every
//! change (`docs/design/05-editor-core.md`, §4), so every other crate sees a normal map.
//! Positions live on the grid's own lattice (`crate::lattice`): the cells of orthogonal and
//! isometric maps, the diamond grid behind the shifted rows (columns) of staggered maps, and
//! axial coordinates on hexagonal maps, where a turn is a sixth. Where the tiles are is one
//! exact map of doubled lattice coordinates (`placement`), so shapes keep their form on every
//! grid and mirrors and turns undo exactly.

mod placement;

pub use placement::TilePivot;

use std::sync::Arc;

use bt_core::{Layer, LayerId, Map, PlacedCell};
use bt_utils::IRect;
use glam::IVec2;

use crate::axis::Axis;
use crate::clip::{ClipLayer, ClipTilesets, TileClip, erase, tile_layers_among, tiles_in};
use crate::error::EditError;
use crate::lattice::{Lattice, Motion, RotateDirection};
use crate::region::TileRegion;
use crate::report::{EraseReport, LayerChange, PasteReport};
use placement::Placement;

/// Tiles a tile selection lifted off a map, carried where the selection goes: moved along the
/// grid's lattice, mirrored and turned on screen, each tile mirrored and turned with them as a
/// brush's tiles are. A value: every change gives a new one sharing the lifted tiles, and only
/// [`FloatingTiles::place`] touches a map.
///
/// The selection travels whole, its empty cells included, so [`FloatingTiles::region`] has the
/// shape the selection had. Its empty cells cover nothing: placed over other tiles, they leave
/// them showing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FloatingTiles {
    /// The lattice of the map they were lifted from.
    lattice: Lattice,
    /// What was lifted, where it was.
    lifted: Arc<Lifted>,
    /// Where it is now.
    placement: Placement,
}

/// What a lift took, where it was.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Lifted {
    /// The selection, empty cells included.
    region: TileRegion,
    /// The middle of the selection's bounds on the lattice.
    centre: TilePivot,
    /// The layers carried, in the map's order.
    layers: Vec<CarriedLayer>,
}

/// The tiles carried off one layer, with the map's tileset slots: where they were lifted from,
/// or where they are now.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CarriedLayer {
    /// The layer.
    layer: LayerId,
    /// Its tiles in the selection.
    tiles: Vec<PlacedCell>,
}

/// What lifting a selection's tiles took and what it emptied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileLift {
    /// The tiles, carried where they were.
    pub tiles: FloatingTiles,
    /// The cells emptied, and the locked layers whose tiles stayed on the map.
    pub erased: EraseReport,
}

impl FloatingTiles {
    /// Lifts the tiles of the tile layers among `layers` inside `region` off `map`, carried
    /// where they were. A locked layer, or one in a locked group, keeps its tiles and is listed
    /// in the report, as the eraser lists it; layers of other kinds are passed over, as the
    /// clipboard passes them. A layer without tiles in the region is carried all the same, so a
    /// copy of the carried tiles has it. An id the map does not have is an error, and the map is
    /// left as it was.
    pub fn lift(
        map: &mut Map,
        layers: &[LayerId],
        region: &TileRegion,
    ) -> Result<TileLift, EditError> {
        let lattice = Lattice::of(&map.geometry());
        let mut carried = Vec::new();
        for id in tile_layers_among(map, layers)? {
            if map.effective_layer(id).is_some_and(|layer| layer.locked) {
                continue;
            }
            if let Some(cells) = map.layer(id).and_then(Layer::as_tile_layer) {
                carried.push(CarriedLayer {
                    layer: id,
                    tiles: tiles_in(cells, region),
                });
            }
        }
        let erased = erase(map, layers, region)?;
        let lifted = Lifted {
            region: region.clone(),
            centre: centre_of(lattice, region),
            layers: carried,
        };
        Ok(TileLift {
            tiles: FloatingTiles {
                lattice,
                lifted: Arc::new(lifted),
                placement: Placement::default(),
            },
            erased,
        })
    }

    /// The tiles moved by `delta` cells: the top-left of the selection's bounds moves by
    /// `delta`, and every cell with it along the lattice, as a pasted clip is anchored. On
    /// staggered and hexagonal maps a move across an odd number of rows (columns) moves the
    /// rows of each parity by different numbers of cells, which keeps the shape's form; moves
    /// add up, and a move back undoes one exactly. An empty selection moves as cell (0, 0)
    /// would.
    pub fn moved(&self, delta: IVec2) -> FloatingTiles {
        let lattice = self.lattice;
        let now = self.placement.cells(lattice);
        let cells: Vec<IVec2> = self.lifted.region.iter().map(now).collect();
        let step = IRect::bounding(cells.iter().copied())
            .and_then(|bounds| lattice.anchoring_step(&cells, bounds.min().saturating_add(delta)))
            .unwrap_or_else(|| lattice.step(IVec2::ZERO, delta));
        // Cell (0, 0) has lattice coordinates (0, 0), and a step changes them alike everywhere.
        let step = lattice.coordinates(lattice.stepped(IVec2::ZERO, step));
        self.with_placement(self.placement.then_stepped(step))
    }

    /// The tiles mirrored on screen about `pivot`, each tile mirrored with them (on an
    /// isometric map a left-right mirror swaps the grid's axes). Mirroring twice about the same
    /// point gives the tiles back exactly as they were.
    pub fn flipped(&self, axis: Axis, pivot: TilePivot) -> FloatingTiles {
        self.moved_by(Motion::Mirror(axis), pivot)
    }

    /// The tiles turned on screen about `pivot`, a quarter turn or a sixth on hexagonal maps,
    /// each tile turned with them. Four quarter turns (six sixths) about the same point give the
    /// tiles back exactly as they were.
    pub fn rotated(&self, direction: RotateDirection, pivot: TilePivot) -> FloatingTiles {
        self.moved_by(Motion::Turn(direction), pivot)
    }

    /// The selection's centre: the middle of its bounds on the lattice as lifted, carried along
    /// by every move, mirror and turn since, so mirrors and turns about it keep the selection
    /// where it is. A turn that cannot keep the middle (of a block with an odd and an even side)
    /// puts the cells half a cell down the lattice's axes, all alike; turned on, they come back.
    /// An empty selection's centre is cell (0, 0)'s, carried.
    pub fn centre(&self) -> TilePivot {
        self.placement.pivot(self.lattice, self.lifted.centre)
    }

    /// Paints the tiles where they are now into the layers they came from, over what is there.
    /// A tile outside a finite layer is not painted, but it stays carried and comes back when
    /// the tiles are moved onto the map again. A layer locked since the lift (or in a group
    /// locked since) is left alone and listed in the report. Placing makes no layers and adds no
    /// tilesets: the tiles name the map's tilesets, as they did on it.
    ///
    /// A carried layer the map no longer has (or that holds no cells), or a tileset slot the
    /// map lacks, is an error, and the map is left as it was.
    pub fn place(&self, map: &mut Map) -> Result<PasteReport, EditError> {
        for carried in &self.lifted.layers {
            let id = carried.layer;
            let layer = map.layer(id).ok_or(EditError::NoSuchLayer(id))?;
            if layer.as_tile_layer().is_none() {
                return Err(EditError::NotATileLayer(id));
            }
            let mut slots = carried.tiles.iter().filter_map(|placed| placed.cell.slot());
            if let Some(missing) = slots.find(|&slot| map.tileset(slot).is_none()) {
                return Err(EditError::NoSuchTileset(missing));
            }
        }
        let mut report = PasteReport::default();
        for carried in self.layers_now() {
            let id = carried.layer;
            if map.effective_layer(id).is_some_and(|layer| layer.locked) {
                report.skipped_locked.push(id);
                continue;
            }
            let Some(cells) = map.layer_mut(id).and_then(Layer::as_tile_layer_mut) else {
                continue;
            };
            let mut changed: Option<IRect> = None;
            for placed in carried.tiles {
                if !cells.contains(placed.at) {
                    continue;
                }
                if cells.set_cell(placed.at, placed.cell)? {
                    let rect = IRect::from_min_size(placed.at, IVec2::ONE);
                    changed = Some(changed.map_or(rect, |so_far| so_far.union(rect)));
                }
            }
            if let Some(rect) = changed {
                report.changed.push(LayerChange { layer: id, rect });
            }
        }
        Ok(report)
    }

    /// The selection where it is now: the lifted selection, empty cells included, moved,
    /// mirrored and turned with the tiles, so it keeps its shape. Cells off a finite map are in
    /// it: they come back onto the map with the tiles.
    pub fn region(&self) -> TileRegion {
        let now = self.placement.cells(self.lattice);
        self.lifted.region.iter().map(now).collect()
    }

    /// Whether the tiles are back exactly where and as they were lifted, so that placing them
    /// gives back the map as it was.
    pub fn at_rest(&self) -> bool {
        self.placement.at_rest()
    }

    /// The tiles where they are now as Copy takes them, as if they were placed: a clip layer
    /// for each carried layer with its name, opacity and tint in `map`, over the bounding
    /// rectangle of the selection, the empty cells of which stay empty, with the tilesets of
    /// `map` that the tiles use. Tiles off a finite map are in it too. A carried layer or a
    /// tileset slot that `map` lacks is an error.
    pub fn clip(&self, map: &Map) -> Result<TileClip, EditError> {
        let bounds = self.region().bounds().unwrap_or_default();
        let mut tilesets = ClipTilesets::new();
        let mut layers = Vec::new();
        for carried in self.layers_now() {
            let id = carried.layer;
            let layer = map.layer(id).ok_or(EditError::NoSuchLayer(id))?;
            let mut copied = ClipLayer {
                opacity: layer.opacity,
                tint: layer.tint,
                ..ClipLayer::named(layer.name.clone())
            };
            // Row by row, as Copy meets them, so the tilesets are listed in its order.
            let mut tiles = carried.tiles;
            tiles.sort_unstable_by_key(|placed| (placed.at.y, placed.at.x));
            for placed in tiles {
                let cell = tilesets.adopt(&map.tilesets, placed.cell)?;
                copied.put(placed.at - bounds.min(), cell);
            }
            layers.push(copied);
        }
        Ok(TileClip {
            origin: bounds.min(),
            size: bounds.size(),
            layers,
            tilesets: tilesets.into_list(),
        })
    }

    /// The same tiles after a mirror or a turn about `pivot`.
    fn moved_by(&self, motion: Motion, pivot: TilePivot) -> FloatingTiles {
        self.with_placement(self.placement.then_moved(self.lattice, motion, pivot))
    }

    /// The same tiles, placed otherwise.
    fn with_placement(&self, placement: Placement) -> FloatingTiles {
        FloatingTiles {
            lattice: self.lattice,
            lifted: Arc::clone(&self.lifted),
            placement,
        }
    }

    /// Each carried layer with its tiles where they are now, mirrored and turned as they are.
    fn layers_now(&self) -> impl Iterator<Item = CarriedLayer> + '_ {
        let now = self.placement.cells(self.lattice);
        let drawn = self.placement.flips(self.lattice);
        self.lifted.layers.iter().map(move |carried| CarriedLayer {
            layer: carried.layer,
            tiles: carried
                .tiles
                .iter()
                .map(|placed| PlacedCell {
                    at: now(placed.at),
                    cell: placed.cell.with_flip(drawn(placed.cell.flip())),
                })
                .collect(),
        })
    }
}

/// The middle of `region`'s bounds on the lattice, doubled; cell (0, 0)'s centre when the region
/// is empty.
fn centre_of(lattice: Lattice, region: &TileRegion) -> TilePivot {
    let bounds = IRect::bounding(region.iter().map(|cell| lattice.coordinates(cell)));
    // The last coordinates are one before the bounds' end.
    let doubled = bounds.map_or(IVec2::ZERO, |bounds| {
        bounds
            .min()
            .saturating_add(bounds.max())
            .saturating_sub(IVec2::ONE)
    });
    TilePivot { doubled }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use bt_core::{Cell, Color, Flip, MapGeometry, Orientation, Tileset, TilesetSlot};
    use bt_test_data::Fuzz;
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use bt_utils::rng::Rng;
    use glam::UVec2;

    use super::*;
    use crate::clip::copy;
    use crate::testutil::{assert_valid, drawn, every_grid, group, map_grid, tile, tiles, trimmed};

    /// A tileset of 64 tiles.
    fn atlas() -> Arc<Tileset> {
        Atlas::builder()
            .grid(8, 8)
            .build_embedded()
            .unwrap()
            .tileset
    }

    /// A builder of maps of `geometry`'s grid, `size` cells, with `tileset`.
    fn builder(
        tileset: &Arc<Tileset>,
        geometry: &MapGeometry,
        size: UVec2,
        infinite: bool,
    ) -> MapBuilder {
        let tile = IVec2::new(geometry.tile_width, geometry.tile_height).as_uvec2();
        let builder = MapBuilder::new(tile, size)
            .orientation(geometry.orientation)
            .stagger(geometry.stagger_axis, geometry.stagger_index)
            .hex_side_length(geometry.hex_side_length)
            .skew(geometry.skew)
            .tileset(Arc::clone(tileset));
        if infinite {
            builder.infinite()
        } else {
            builder
        }
    }

    const GROUND: &str = "
        0 1 2 3 4 5
        6 7 8 9 10 11
        12 13 14 15 16 17
        18 19 20 21 22 23
        24 25 26 27 28 29
    ";

    const WALLS: &str = "
        . 40 . . 41h .
        . . 42v . . .
        43d . . . 44hv .
        . . . . . .
        . 45 . . . 46
    ";

    const TOP: &str = "
        . . . . . .
        . 50 51 . . .
        . 52 53x . . .
        . . . . . 54
        . . . . . .
    ";

    /// The selection the tests carry.
    const SELECTION: &str = "
        ......
        .###..
        .##...
        ..####
        ......
    ";

    /// A 6 × 5 map of `geometry`'s grid, every tile in it a different one: ground (1) full, a
    /// group (2) holding walls (3), an object layer (4), and top (5); some tiles are mirrored
    /// and turned.
    fn scene(tileset: &Arc<Tileset>, geometry: &MapGeometry, infinite: bool) -> Map {
        let size = UVec2::new(6, 5);
        let walls = Layer {
            // Numbered when the map is built.
            id: LayerId(0),
            ..builder(tileset, geometry, size, infinite)
                .tile_layer("walls", WALLS)
                .build()
                .unwrap()
                .layers
                .remove(0)
        };
        builder(tileset, geometry, size, infinite)
            .tile_layer("ground", GROUND)
            .layer(group("group", vec![walls]))
            .object_layer("things", Vec::new())
            .tile_layer("top", TOP)
            .build()
            .unwrap()
    }

    /// The tile layers of [`scene`].
    const SCENE_TILES: [LayerId; 3] = [LayerId(1), LayerId(3), LayerId(5)];

    /// The orthogonal grid.
    fn square() -> MapGeometry {
        MapGeometry::new(Orientation::Orthogonal, IVec2::splat(16), IVec2::new(6, 5))
    }

    /// A map without the tiles lifted off it, and the tiles.
    struct Lifting {
        base: Map,
        tiles: FloatingTiles,
    }

    /// Lifts `layers` in `region` off a copy of `map`.
    fn lifted(map: &Map, layers: &[LayerId], region: &TileRegion) -> Lifting {
        let mut base = map.clone();
        let lift = FloatingTiles::lift(&mut base, layers, region).unwrap();
        Lifting {
            base,
            tiles: lift.tiles,
        }
    }

    /// `tiles` placed on a copy of `base`, which must stay valid.
    fn placed(base: &Map, tiles: &FloatingTiles) -> Map {
        let mut map = base.clone();
        tiles.place(&mut map).unwrap();
        assert_valid(&map);
        map
    }

    /// The carried tiles now, by tile, all layers together (every tile a different one).
    fn by_tile(tiles: &FloatingTiles) -> BTreeMap<u32, PlacedCell> {
        tiles
            .layers_now()
            .flat_map(|layer| layer.tiles)
            .map(|placed| (placed.cell.tile().0, placed))
            .collect()
    }

    /// Asserts that two sets of tiles stand to each other alike: the same tiles, with the same
    /// lattice steps between them.
    fn assert_same_shape(
        lattice: Lattice,
        a: &BTreeMap<u32, PlacedCell>,
        b: &BTreeMap<u32, PlacedCell>,
        context: &str,
    ) {
        let tiles_a: Vec<&u32> = a.keys().collect();
        let tiles_b: Vec<&u32> = b.keys().collect();
        assert_eq!(tiles_a, tiles_b, "{context}");
        let Some(first_a) = a.values().next() else {
            return;
        };
        let Some(first_b) = b.values().next() else {
            return;
        };
        for (tile, placed) in a {
            assert_eq!(
                lattice.step(first_a.at, placed.at),
                lattice.step(first_b.at, b[tile].at),
                "tile {tile}, {context}"
            );
        }
    }

    #[test]
    fn lifting_takes_the_tiles_off_and_placing_them_at_rest_puts_them_back() {
        let tileset = atlas();
        let region = drawn(SELECTION);
        for geometry in every_grid() {
            for infinite in [false, true] {
                let context = format!("{geometry:?}, infinite {infinite}");
                let map = scene(&tileset, &geometry, infinite);
                let mut base = map.clone();
                // The group and the object layer are passed over, as the clipboard passes them.
                let ids = [LayerId(1), LayerId(2), LayerId(3), LayerId(4), LayerId(5)];
                let lift = FloatingTiles::lift(&mut base, &ids, &region).unwrap();
                let clip = lift.tiles.clip(&base).unwrap();
                let names: Vec<&str> = clip.layers.iter().map(|l| l.name.as_str()).collect();
                assert_eq!(names, ["ground", "walls", "top"], "{context}");
                for id in SCENE_TILES {
                    for at in IRect::from_min_size(IVec2::ZERO, IVec2::new(6, 5)).points() {
                        let expected = if region.contains(at) {
                            Cell::EMPTY
                        } else {
                            tiles(&map, id).cell(at)
                        };
                        assert_eq!(
                            tiles(&base, id).cell(at),
                            expected,
                            "{at} on {id}, {context}"
                        );
                    }
                }
                assert_eq!(lift.erased.changed.len(), 3, "{context}");
                assert!(lift.erased.skipped_locked.is_empty());
                assert_valid(&base);
                let carried = lift.tiles;
                assert!(carried.at_rest());
                assert_eq!(carried.region(), region);
                let mut back = base.clone();
                let report = carried.place(&mut back).unwrap();
                assert_eq!(back, map, "{context}");
                assert_eq!(report.changed, lift.erased.changed, "{context}");
                assert!(report.created_layers.is_empty() && report.added_tilesets.is_empty());
            }
        }
    }

    #[test]
    fn locked_layers_and_layers_in_locked_groups_keep_their_tiles() {
        let tileset = atlas();
        let mut map = scene(&tileset, &square(), false);
        map.layer_mut(LayerId(2)).unwrap().locked = true;
        map.layer_mut(LayerId(5)).unwrap().locked = true;
        let region = drawn(SELECTION);
        let ids = [LayerId(1), LayerId(3), LayerId(4), LayerId(5)];
        let mut base = map.clone();
        let lift = FloatingTiles::lift(&mut base, &ids, &region).unwrap();
        assert_eq!(lift.erased.skipped_locked, [LayerId(3), LayerId(5)]);
        let changed: Vec<LayerId> = lift.erased.changed.iter().map(|c| c.layer).collect();
        assert_eq!(changed, [LayerId(1)]);
        for id in [LayerId(3), LayerId(5)] {
            assert_eq!(map_grid(&base, id), map_grid(&map, id));
        }
        // Only the ground is carried, and a copy has that layer alone.
        let clip = lift.tiles.clip(&base).unwrap();
        let names: Vec<&str> = clip.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["ground"]);
        // Moved a cell right: the last tile of the fourth row is off the map and not painted.
        let mut moved = base.clone();
        let report = lift.tiles.moved(IVec2::X).place(&mut moved).unwrap();
        assert!(report.skipped_locked.is_empty());
        assert_eq!(
            map_grid(&moved, LayerId(1)),
            trimmed(
                "
                0 1 2 3 4 5
                6 . 7 8 9 11
                12 . 13 14 16 17
                18 19 . 20 21 22
                24 25 26 27 28 29
                "
            )
        );
        for id in [LayerId(3), LayerId(5)] {
            assert_eq!(map_grid(&moved, id), map_grid(&map, id));
        }
        // A layer locked after the lift is left alone and reported.
        let mut locked = base.clone();
        locked.layer_mut(LayerId(1)).unwrap().locked = true;
        let report = lift.tiles.place(&mut locked).unwrap();
        assert_eq!(report.skipped_locked, [LayerId(1)]);
        assert!(report.changed.is_empty());
        assert_eq!(map_grid(&locked, LayerId(1)), map_grid(&base, LayerId(1)));
        // So is a layer whose group was locked after the lift.
        let unlocked = scene(&tileset, &square(), false);
        let Lifting { base, tiles: both } = lifted(&unlocked, &[LayerId(1), LayerId(3)], &region);
        let mut in_locked_group = base.clone();
        in_locked_group.layer_mut(LayerId(2)).unwrap().locked = true;
        let report = both.place(&mut in_locked_group).unwrap();
        assert_eq!(report.skipped_locked, [LayerId(3)]);
        let changed: Vec<LayerId> = report.changed.iter().map(|c| c.layer).collect();
        assert_eq!(changed, [LayerId(1)]);
        assert_eq!(
            map_grid(&in_locked_group, LayerId(3)),
            map_grid(&base, LayerId(3))
        );
        assert_eq!(
            map_grid(&in_locked_group, LayerId(1)),
            map_grid(&unlocked, LayerId(1))
        );
        // An id the map does not have is an error, and nothing is lifted.
        let mut untouched = map.clone();
        assert_eq!(
            FloatingTiles::lift(&mut untouched, &[LayerId(1), LayerId(9)], &region),
            Err(EditError::NoSuchLayer(LayerId(9)))
        );
        assert_eq!(untouched, map);
    }

    #[test]
    fn placing_or_copying_needs_the_layers_and_tilesets_the_tiles_came_from() {
        let tileset = atlas();
        let map = scene(&tileset, &square(), false);
        let Lifting {
            base,
            tiles: carried,
        } = lifted(&map, &[LayerId(1)], &drawn(SELECTION));
        let mut without_layer = base.clone();
        without_layer.layers.remove(0);
        let before = without_layer.clone();
        assert_eq!(
            carried.place(&mut without_layer),
            Err(EditError::NoSuchLayer(LayerId(1)))
        );
        assert_eq!(without_layer, before);
        assert_eq!(
            carried.clip(&without_layer),
            Err(EditError::NoSuchLayer(LayerId(1)))
        );
        let mut without_tileset = base.clone();
        without_tileset.tilesets.clear();
        let before = without_tileset.clone();
        let missing = Err(EditError::NoSuchTileset(TilesetSlot::new(0).unwrap()));
        assert_eq!(carried.place(&mut without_tileset), missing);
        assert_eq!(without_tileset, before);
    }

    #[test]
    fn whole_turns_and_double_mirrors_give_back_what_was_lifted() {
        let tileset = atlas();
        let region = drawn(SELECTION);
        for geometry in every_grid() {
            let map = scene(&tileset, &geometry, false);
            let Lifting { base, tiles: start } = lifted(&map, &SCENE_TILES, &region);
            let circle = Lattice::of(&geometry).turns_in_circle();
            // About the selection's centre, and about a point between cells away from it.
            let elsewhere = TilePivot {
                doubled: IVec2::new(7, -2),
            };
            for pivot in [start.centre(), elsewhere] {
                for direction in [
                    RotateDirection::Clockwise,
                    RotateDirection::CounterClockwise,
                ] {
                    let mut turned = start.clone();
                    for count in 1..=circle {
                        turned = turned.rotated(direction, pivot);
                        assert_eq!(turned.at_rest(), count == circle, "{geometry:?}");
                        placed(&base, &turned);
                    }
                    assert_eq!(turned, start, "{direction:?} on {geometry:?}");
                }
                for direction in [Axis::Horizontal, Axis::Vertical] {
                    let once = start.flipped(direction, pivot);
                    assert!(!once.at_rest());
                    placed(&base, &once);
                    assert_eq!(once.flipped(direction, pivot), start, "{geometry:?}");
                }
                let there = start.rotated(RotateDirection::Clockwise, pivot);
                let back = there.rotated(RotateDirection::CounterClockwise, pivot);
                assert_eq!(back, start, "{geometry:?}");
            }
            // Turning about the centre keeps it where it is.
            let turned = start.rotated(RotateDirection::Clockwise, start.centre());
            assert_eq!(turned.centre(), start.centre());
            let back = turned.rotated(RotateDirection::CounterClockwise, turned.centre());
            assert_eq!(placed(&base, &back), map, "{geometry:?}");
        }
    }

    #[test]
    fn moves_add_up_and_a_move_back_undoes_one() {
        let tileset = atlas();
        let region = drawn(SELECTION);
        let corner = region.bounds().unwrap().min();
        let deltas = [
            IVec2::new(1, 0),
            IVec2::new(0, 1),
            IVec2::new(-3, 5),
            IVec2::new(2, -1),
            IVec2::new(-1, -1),
        ];
        for geometry in every_grid() {
            let map = scene(&tileset, &geometry, false);
            let Lifting { tiles: start, .. } = lifted(&map, &SCENE_TILES, &region);
            assert_eq!(start.moved(IVec2::ZERO), start);
            let turned = start.rotated(RotateDirection::Clockwise, start.centre());
            for a in deltas {
                let there = start.moved(a);
                assert!(!there.at_rest());
                let bounds = there.region().bounds().unwrap();
                assert_eq!(
                    bounds.min(),
                    corner + a,
                    "the corner moves by {a} on {geometry:?}"
                );
                assert_eq!(there.moved(-a), start, "{a} and back on {geometry:?}");
                for b in deltas {
                    assert_eq!(
                        there.moved(b),
                        start.moved(a + b),
                        "{a} then {b} on {geometry:?}"
                    );
                }
                assert_eq!(turned.moved(a).moved(-a), turned, "{geometry:?}");
            }
        }
    }

    #[test]
    fn tiles_moved_off_a_finite_map_stay_carried_and_come_back() {
        let tileset = atlas();
        let region = drawn(SELECTION);
        for geometry in every_grid() {
            let map = scene(&tileset, &geometry, false);
            let Lifting { base, tiles: start } = lifted(&map, &SCENE_TILES, &region);
            let away = start.moved(IVec2::new(20, -15));
            let mut off = base.clone();
            let report = away.place(&mut off).unwrap();
            assert!(report.changed.is_empty(), "{geometry:?}");
            assert_eq!(off, base);
            assert_eq!(away.region().len(), region.len());
            let back = away.moved(IVec2::new(-20, 15));
            assert!(back.at_rest(), "{geometry:?}");
            assert_eq!(placed(&base, &back), map, "{geometry:?}");
        }
        // Half off and back, on a square grid: the tiles off the map come back with the rest.
        let map = scene(&tileset, &square(), false);
        let Lifting { base, tiles: start } = lifted(&map, &[LayerId(1)], &region);
        let half_off = start.moved(IVec2::new(3, 0));
        assert_eq!(
            map_grid(&placed(&base, &half_off), LayerId(1)),
            trimmed(
                "
                0 1 2 3 4 5
                6 . . . 7 8
                12 . . 15 13 14
                18 19 . . . 20
                24 25 26 27 28 29
                "
            )
        );
        let off_cells = half_off.region().iter().filter(|at| at.x >= 6).count();
        assert_eq!(off_cells, 4, "the selection keeps the cells off the map");
        assert_eq!(placed(&base, &half_off.moved(IVec2::new(-3, 0))), map);
    }

    #[test]
    fn the_selection_follows_the_tiles_with_its_empty_cells() {
        let tileset = atlas();
        let square_map = builder(&tileset, &square(), UVec2::new(5, 4), false)
            .tile_layer("ground", ". . . . .\n. 1 2 . .\n. 3 . . .\n. . . . .")
            .build()
            .unwrap();
        let block = drawn(".....\n.##..\n.##..");
        let Lifting { base, tiles: start } = lifted(&square_map, &[LayerId(1)], &block);
        let moved = start.moved(IVec2::new(2, 1));
        assert_eq!(moved.region(), block.translated(IVec2::new(2, 1)));
        assert_eq!(
            map_grid(&placed(&base, &moved), LayerId(1)),
            trimmed(". . . . .\n. . . . .\n. . . 1 2\n. . . 3 .")
        );
        // Turned right about its centre, as Tiled turns a stamp; the empty cell turns too.
        let turned = moved.rotated(RotateDirection::Clockwise, moved.centre());
        assert_eq!(turned.region(), moved.region());
        assert_eq!(
            map_grid(&placed(&base, &turned), LayerId(1)),
            trimmed(". . . . .\n. . . . .\n. . . 3hd 1hd\n. . . . 2hd")
        );
        // On an isometric map a left-right mirror swaps the grid's axes, the selection's too.
        let iso = MapGeometry::new(Orientation::Isometric, IVec2::new(32, 16), IVec2::splat(4));
        let iso_map = builder(&tileset, &iso, UVec2::splat(4), false)
            .tile_layer("ground", ". . . .\n. 1 2 3\n. . . .\n. . . .")
            .build()
            .unwrap();
        let row = drawn("....\n.###");
        let Lifting { base, tiles: start } = lifted(&iso_map, &[LayerId(1)], &row);
        let mirrored = start.flipped(Axis::Horizontal, start.centre());
        assert_eq!(mirrored.region(), drawn("..#.\n..#.\n..#."));
        assert_eq!(
            map_grid(&placed(&base, &mirrored), LayerId(1)),
            trimmed(". . 1h .\n. . 2h .\n. . 3h .\n. . . .")
        );
    }

    #[test]
    fn a_copy_while_carrying_is_the_copy_of_the_tiles_placed() {
        // An infinite map whose tiles all lie in the selection: once they are lifted the map
        // holds nothing, so placing them shows them alone.
        let tileset = atlas();
        let region = drawn(SELECTION);
        for geometry in every_grid() {
            let mut map = builder(&tileset, &geometry, UVec2::new(6, 5), true)
                .tile_layer(
                    "ground",
                    ". . . . . .\n. 1 2 3 . .\n. 4 5h . . .\n. . 6 7v 8 9d\n. . . . . .",
                )
                .tile_layer(
                    "top",
                    ". . . . . .\n. . 10 . . .\n. 11 . . . .\n. . . . 12x .\n. . . . . .",
                )
                .build()
                .unwrap();
            map.layer_mut(LayerId(1)).unwrap().opacity = 0.5;
            map.layer_mut(LayerId(2)).unwrap().tint = Some(Color::rgb(1, 2, 3));
            let ids = [LayerId(1), LayerId(2)];
            let Lifting { base, tiles: start } = lifted(&map, &ids, &region);
            assert_eq!(
                start.clip(&base).unwrap(),
                copy(&map, &ids, &region).unwrap()
            );
            let moved = start.moved(IVec2::new(3, -2));
            let turned = moved.rotated(RotateDirection::Clockwise, moved.centre());
            let carried = turned.flipped(Axis::Vertical, turned.centre());
            let shown = placed(&base, &carried);
            assert_eq!(
                carried.clip(&base).unwrap(),
                copy(&shown, &ids, &carried.region()).unwrap(),
                "{geometry:?}"
            );
        }
    }

    #[test]
    fn shapes_keep_their_form_when_moved_across_odd_rows_and_columns() {
        let tileset = atlas();
        let region = drawn(SELECTION);
        let corner = region.bounds().unwrap().min();
        for geometry in every_grid() {
            let lattice = Lattice::of(&geometry);
            if lattice.stagger().is_none() {
                continue;
            }
            let map = scene(&tileset, &geometry, true);
            let Lifting { tiles: start, .. } = lifted(&map, &SCENE_TILES, &region);
            let before = by_tile(&start);
            for delta in [
                IVec2::new(0, 1),
                IVec2::new(1, 0),
                IVec2::new(1, 1),
                IVec2::new(-1, 3),
                IVec2::new(3, -1),
                IVec2::new(2, 2),
            ] {
                let moved = start.moved(delta);
                let context = format!("{delta} on {geometry:?}");
                let after = by_tile(&moved);
                assert_same_shape(lattice, &before, &after, &context);
                // The selection moves with them, empty cells included, by the same step.
                let first = before.keys().next().unwrap();
                let step = lattice.step(before[first].at, after[first].at);
                let expected: TileRegion =
                    region.iter().map(|at| lattice.stepped(at, step)).collect();
                assert_eq!(moved.region(), expected, "{context}");
                assert_eq!(
                    expected.bounds().unwrap().min(),
                    corner + delta,
                    "{context}"
                );
            }
        }
    }

    #[test]
    fn a_carried_tile_is_mirrored_and_turned_as_a_brushs_tile_is() {
        let tileset = atlas();
        let at = IVec2::new(2, 2);
        for geometry in every_grid() {
            let lattice = Lattice::of(&geometry);
            let hexagonal = geometry.orientation == Orientation::Hexagonal;
            for bits in 0..16u8 {
                let flip = Flip::from_bits_truncate(bits);
                if hexagonal && flip.contains(Flip::D | Flip::HEX120) {
                    // Not a form the hexagonal tables give.
                    continue;
                }
                let cell = tile(7).with_flip(flip);
                let mut map = builder(&tileset, &geometry, UVec2::splat(5), false)
                    .tile_layer("ground", ". .")
                    .build()
                    .unwrap();
                let cells = map
                    .layer_mut(LayerId(1))
                    .unwrap()
                    .as_tile_layer_mut()
                    .unwrap();
                cells.set_cell(at, cell).unwrap();
                let Lifting { base, tiles: start } =
                    lifted(&map, &[LayerId(1)], &TileRegion::from_cells([at]));
                let mut one = ClipLayer::named("ground");
                one.put(IVec2::ZERO, cell);
                let clip = TileClip {
                    origin: at,
                    size: IVec2::ONE,
                    layers: vec![one],
                    tilesets: Vec::new(),
                };
                for motion in [
                    Motion::Mirror(Axis::Horizontal),
                    Motion::Mirror(Axis::Vertical),
                    Motion::Turn(RotateDirection::Clockwise),
                    Motion::Turn(RotateDirection::CounterClockwise),
                ] {
                    let carried = match motion {
                        Motion::Mirror(direction) => start.flipped(direction, start.centre()),
                        Motion::Turn(direction) => start.rotated(direction, start.centre()),
                    };
                    let brush = match motion {
                        Motion::Mirror(direction) => clip.flipped(direction, &geometry),
                        Motion::Turn(direction) => clip.rotated(direction, &geometry),
                    };
                    let drawn = tiles(&placed(&base, &carried), LayerId(1)).cell(at);
                    let context = format!("{flip:?}, {motion:?} on {geometry:?}");
                    assert_eq!(
                        drawn,
                        tile(7).with_flip(lattice.moved_flip(flip, motion)),
                        "{context}"
                    );
                    assert_eq!(drawn, brush.layers[0].cells.cell(IVec2::ZERO), "{context}");
                }
            }
        }
    }

    /// A change the random test makes to carried tiles.
    #[derive(Copy, Clone, Debug)]
    enum Change {
        Move {
            delta: IVec2,
        },
        Mirror {
            direction: Axis,
            pivot: TilePivot,
        },
        Turn {
            direction: RotateDirection,
            pivot: TilePivot,
        },
    }

    impl Change {
        /// A change at random, about the selection's centre two times in three.
        fn random(tiles: &FloatingTiles, rng: &mut Rng) -> Change {
            let mut number = |below: u64, from: i32| rng.below(below).unwrap() as i32 + from;
            let delta = IVec2::new(number(9, -4), number(9, -4));
            let somewhere = TilePivot {
                doubled: IVec2::new(number(25, -8), number(25, -8)),
            };
            let pivot = if number(3, 0) == 0 {
                somewhere
            } else {
                tiles.centre()
            };
            let either = number(2, 0) == 0;
            match number(3, 0) {
                0 => Change::Move { delta },
                1 => Change::Mirror {
                    direction: if either {
                        Axis::Horizontal
                    } else {
                        Axis::Vertical
                    },
                    pivot,
                },
                _ => Change::Turn {
                    direction: if either {
                        RotateDirection::Clockwise
                    } else {
                        RotateDirection::CounterClockwise
                    },
                    pivot,
                },
            }
        }

        fn applied(self, tiles: &FloatingTiles) -> FloatingTiles {
            match self {
                Change::Move { delta } => tiles.moved(delta),
                Change::Mirror { direction, pivot } => tiles.flipped(direction, pivot),
                Change::Turn { direction, pivot } => tiles.rotated(direction, pivot),
            }
        }

        /// What the brush does to its clip for the same change; where it goes is the
        /// placement's.
        fn applied_to_clip(self, clip: &TileClip, geometry: &MapGeometry) -> TileClip {
            match self {
                Change::Move { .. } => clip.clone(),
                Change::Mirror { direction, .. } => clip.flipped(direction, geometry),
                Change::Turn { direction, .. } => clip.rotated(direction, geometry),
            }
        }

        fn undoing(self) -> Change {
            match self {
                Change::Move { delta } => Change::Move { delta: -delta },
                Change::Mirror { .. } => self,
                Change::Turn { direction, pivot } => Change::Turn {
                    direction: match direction {
                        RotateDirection::Clockwise => RotateDirection::CounterClockwise,
                        RotateDirection::CounterClockwise => RotateDirection::Clockwise,
                    },
                    pivot,
                },
            }
        }
    }

    /// An 8 × 8 map of `geometry`'s grid with two tile layers, each cell holding a tile two
    /// times in three, every tile of a layer a different one, mirrored and turned at random (in
    /// the forms the hexagonal tables give, on hexagonal maps).
    fn random_map(
        tileset: &Arc<Tileset>,
        geometry: &MapGeometry,
        infinite: bool,
        rng: &mut Rng,
    ) -> Map {
        let hexagonal = geometry.orientation == Orientation::Hexagonal;
        let mut map = builder(tileset, geometry, UVec2::splat(8), infinite)
            .tile_layer("ground", ". .")
            .tile_layer("decor", ". .")
            .build()
            .unwrap();
        for id in [LayerId(1), LayerId(2)] {
            let cells = map.layer_mut(id).unwrap().as_tile_layer_mut().unwrap();
            let area = IRect::from_min_size(IVec2::ZERO, IVec2::splat(8));
            for (index, at) in (0u32..).zip(area.points()) {
                if rng.below(3) == Some(0) {
                    continue;
                }
                let mut flip = Flip::from_bits_truncate(rng.below(16).unwrap() as u8);
                if hexagonal && flip.contains(Flip::D | Flip::HEX120) {
                    flip.remove(Flip::HEX120);
                }
                cells.set_cell(at, tile(index).with_flip(flip)).unwrap();
            }
        }
        map
    }

    /// One or two rectangles of cells, some of them maybe off the map.
    fn random_region(rng: &mut Rng) -> TileRegion {
        let mut number = |below: u64, from: i32| rng.below(below).unwrap() as i32 + from;
        let count = number(2, 1);
        let rects: Vec<IRect> = (0..count)
            .map(|_| IRect {
                x: number(9, -1),
                y: number(9, -1),
                w: number(5, 1),
                h: number(5, 1),
            })
            .collect();
        TileRegion::from_rects(rects)
    }

    /// The tiles of a clip's layers by tile, each layer its own, where they are on the map.
    fn clip_by_tile(clip: &TileClip) -> Vec<BTreeMap<u32, PlacedCell>> {
        clip.layers
            .iter()
            .map(|layer| {
                layer
                    .cells
                    .cells()
                    .map(|placed| {
                        let at = clip.origin + placed.at;
                        let cell = placed.cell;
                        (cell.tile().0, PlacedCell { at, cell })
                    })
                    .collect()
            })
            .collect()
    }

    /// The carried tiles by tile, each layer its own.
    fn carried_by_tile(tiles: &FloatingTiles) -> Vec<BTreeMap<u32, PlacedCell>> {
        tiles
            .layers_now()
            .map(|layer| {
                layer
                    .tiles
                    .into_iter()
                    .map(|placed| (placed.cell.tile().0, placed))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn random_moves_mirrors_and_turns_keep_the_map_valid_and_every_tile() {
        let fuzz = Fuzz::from_env(0x0F10_A7ED, 30);
        let tileset = atlas();
        let ids = [LayerId(1), LayerId(2)];
        for case in 0..fuzz.cases {
            let mut rng = Rng::new(fuzz.case_seed(case));
            for geometry in every_grid() {
                for infinite in [false, true] {
                    let lattice = Lattice::of(&geometry);
                    let map = random_map(&tileset, &geometry, infinite, &mut rng);
                    let region = random_region(&mut rng);
                    // The brush's mirrors and turns of the same tiles, to compare with.
                    let mut brush = copy(&map, &ids, &region).unwrap();
                    let Lifting { base, tiles: start } = lifted(&map, &ids, &region);
                    let mut carried = start.clone();
                    let mut changes = Vec::new();
                    for _ in 0..1 + rng.below(8).unwrap() {
                        let change = Change::random(&carried, &mut rng);
                        carried = change.applied(&carried);
                        brush = change.applied_to_clip(&brush, &geometry);
                        changes.push(change);
                        let context = format!(
                            "case {case} ({}), {changes:?} on {geometry:?}, infinite {infinite}",
                            fuzz.reproduce()
                        );
                        let mut shown = base.clone();
                        let report = carried.place(&mut shown).unwrap();
                        assert_valid(&shown);
                        let selection = carried.region();
                        assert_eq!(selection.len(), region.len(), "{context}");
                        let layers = carried_by_tile(&carried);
                        let brushed = clip_by_tile(&brush);
                        for ((id, now), clipped) in ids.iter().zip(&layers).zip(&brushed) {
                            // Every tile is carried, no two in one cell, all in the selection.
                            let cells: TileRegion = now.values().map(|placed| placed.at).collect();
                            assert_eq!(cells.len(), now.len(), "{context}");
                            assert!(cells.is_subset(&selection), "{context}");
                            // As the brush mirrors and turns them, wherever they are.
                            assert_same_shape(lattice, clipped, now, &context);
                            for (tile, placed) in now {
                                assert_eq!(placed.cell, clipped[tile].cell, "{tile}, {context}");
                            }
                            // The map shows them where they are, over what was there, and
                            // nothing else changed.
                            let layer = tiles(&shown, *id);
                            let under = tiles(&base, *id);
                            let painted: HashMap<IVec2, Cell> = now
                                .values()
                                .filter(|placed| layer.contains(placed.at))
                                .map(|placed| (placed.at, placed.cell))
                                .collect();
                            let seen: TileRegion = layer
                                .cells()
                                .chain(under.cells())
                                .map(|placed| placed.at)
                                .chain(painted.keys().copied())
                                .collect();
                            let rect = report
                                .changed
                                .iter()
                                .find(|change| change.layer == *id)
                                .map_or(IRect::default(), |change| change.rect);
                            for at in seen.iter() {
                                let expected = painted.get(&at).copied().unwrap_or(under.cell(at));
                                assert_eq!(layer.cell(at), expected, "{at}, {context}");
                                if layer.cell(at) != under.cell(at) {
                                    assert!(rect.contains(at), "{at} is reported, {context}");
                                }
                            }
                        }
                    }
                    for change in changes.iter().rev() {
                        carried = change.undoing().applied(&carried);
                    }
                    assert_eq!(carried, start, "case {case} on {geometry:?}");
                    assert!(carried.at_rest());
                    assert_eq!(placed(&base, &carried), map, "case {case} on {geometry:?}");
                }
            }
        }
    }
}
