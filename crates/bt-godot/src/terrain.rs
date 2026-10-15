//! Tiled's Wang sets as the TileSet's terrain sets (docs/design/12-godot-export.md): one terrain set per Wang set
//! of each tileset the TileSet holds, in the TileSet's order, each colour a terrain of it, and
//! each Wang tile's colours its peering bits.
//!
//! | Tiled | Godot |
//! |---|---|
//! | corner, edge, mixed set | `mode` 1 (corners), 2 (sides), 0 (corners and sides) |
//! | colour c | terrain c − 1, with its name and color |
//! | Wang index i (0 = top, clockwise) | peering bit `[12,15,0,3,4,7,8,11][i]` on orthogonal maps, `[14,1,2,5,6,9,10,13][i]` on isometric and staggered maps |
//! | the tile | `terrain`, its most common colour (ties to the lower one) |
//!
//! - **Hexagonal maps:** Godot's hexagonal terrains match the 6 cells a cell touches, by side.
//!   Edge and mixed sets become sets of sides, without Tiled's 2 indexes toward the cells two
//!   rows (stagger axis y) or columns (x) away; corner sets have nothing to become and are left
//!   out. Each with a warning.
//! - **A Godot tile is in one terrain set:** a tile in several Wang sets is in the first one's,
//!   with a warning that names the sets lacking it.
//! - **Terrain is on alternative 0 alone.** A flipped cell has Godot read its tile's colours
//!   unflipped, so flips that change a tile's colours get a warning. Flipped colours on the
//!   alternatives of flipped tall tiles (grid.rs) wouldn't do: Godot's terrain painting would
//!   place those alternatives unflipped, and other flipped cells use alternative 0 itself.
//! - **An animated tile's colours are on its animation source's tile,** the one its cells use,
//!   and not on the still copy in its tileset's source, so Godot's terrain painting places the
//!   animated one.
//! - **A shared TileSet's terrain sets follow its registry** (shared.rs): each tileset's at its
//!   first source's id. Sources kept from a tileset that can't be read keep the terrain sets
//!   their tiles are in, as the file has them.
//! - **Godot paints corner sets by the cells' own terrains:** a corner takes a terrain only where
//!   every cell around it has it, and a cell beside those painted keeps its own. With one terrain
//!   per tile, its terrain painting can leave seams in a corner set where Tiled's brush wouldn't;
//!   edge and mixed sets paint seamlessly (tests/terrain.rs).

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};

use bt_core::{Cell, Layer, LayerKind, Map, Orientation, StaggerAxis, WangId, WangSetType};

use super::grid::Grid;
use super::report::Element;
use super::text;
use super::tileset::TileSetBuilder;
use crate::build::Build;

/// Godot's `TileSet.CellNeighbor`s by value: the names of the peering bits.
pub(super) const NEIGHBOURS: [&str; 16] = [
    "right_side",
    "right_corner",
    "bottom_right_side",
    "bottom_right_corner",
    "bottom_side",
    "bottom_corner",
    "bottom_left_side",
    "bottom_left_corner",
    "left_side",
    "left_corner",
    "top_left_side",
    "top_left_corner",
    "top_side",
    "top_corner",
    "top_right_side",
    "top_right_corner",
];

/// The peering bit of each Wang index on square cells: top, top-right corner, right, … .
const SQUARE: [Option<usize>; 8] = [
    Some(12),
    Some(15),
    Some(0),
    Some(3),
    Some(4),
    Some(7),
    Some(8),
    Some(11),
];
/// On isometric and staggered maps, where Tiled's top faces the top-right neighbour.
const DIAMOND: [Option<usize>; 8] = [
    Some(14),
    Some(1),
    Some(2),
    Some(5),
    Some(6),
    Some(9),
    Some(10),
    Some(13),
];
/// On hexagonal maps staggered along y: the top-right, right, bottom-right, bottom-left, left
/// and top-left sides; the bottom-right and top-left corners point two rows away.
const HEX_ROWS: [Option<usize>; 8] = [
    Some(14),
    Some(0),
    Some(2),
    None,
    Some(6),
    Some(8),
    Some(10),
    None,
];
/// Along x: the top-right, bottom-right, bottom, bottom-left, top-left and top sides; the
/// top-right and bottom-left corners point two columns away.
const HEX_COLUMNS: [Option<usize>; 8] = [
    Some(14),
    None,
    Some(2),
    Some(4),
    Some(6),
    None,
    Some(10),
    Some(12),
];

/// Godot's `TileSet.TerrainMode`s.
const CORNERS_AND_SIDES: u8 = 0;
const CORNERS: u8 = 1;
const SIDES: u8 = 2;

/// How a Wang set of `kind` is a terrain set on a map of `orientation` (staggered along
/// `axis`): its mode, and the peering bit each Wang index's colour goes on (`None`: left out).
/// `None` when Godot has no terrain set it could be.
pub(super) fn mapping(
    orientation: Orientation,
    axis: StaggerAxis,
    kind: WangSetType,
) -> Option<(u8, [Option<usize>; 8])> {
    let only = |table: [Option<usize>; 8], corners: bool| {
        let mut table = table;
        for (i, bit) in table.iter_mut().enumerate() {
            if (i % 2 == 1) != corners {
                *bit = None;
            }
        }
        table
    };
    let table = match orientation {
        Orientation::Hexagonal if axis == StaggerAxis::X => HEX_COLUMNS,
        Orientation::Hexagonal => HEX_ROWS,
        Orientation::Isometric | Orientation::Staggered => DIAMOND,
        Orientation::Orthogonal | Orientation::Oblique => SQUARE,
    };
    match (orientation == Orientation::Hexagonal, kind) {
        (true, WangSetType::Corner) => None,
        (true, WangSetType::Edge) => Some((SIDES, only(table, false))),
        (true, WangSetType::Mixed) => Some((SIDES, table)),
        (false, WangSetType::Corner) => Some((CORNERS, only(table, true))),
        (false, WangSetType::Edge) => Some((SIDES, only(table, false))),
        (false, WangSetType::Mixed) => Some((CORNERS_AND_SIDES, table)),
    }
}

/// The terrain lines of a Wang tile labelled `wang` in terrain set `set` of `colours` terrains,
/// its colours on `bits`: `terrain_set`, `terrain` and its peering bits by bit, as Godot writes
/// them (the terrain set first, which the bits need). `None` for a tile Tiled has in no set: one
/// with a colour beyond the set's, or with none Godot keeps.
fn wang_tile_lines(
    wang: WangId,
    bits: &[Option<usize>; 8],
    colours: usize,
    set: usize,
) -> Option<Vec<(String, String)>> {
    if wang.lanes().iter().any(|&c| usize::from(c) > colours) {
        return None;
    }
    let mut on: Vec<(usize, usize)> = wang
        .lanes()
        .iter()
        .zip(bits)
        .filter(|(c, _)| **c > 0)
        .filter_map(|(&c, bit)| Some(((*bit)?, usize::from(c))))
        .collect();
    if on.is_empty() {
        return None;
    }
    on.sort_unstable();
    let mut count = vec![0; colours + 1];
    for &(_, c) in &on {
        if let Some(count) = count.get_mut(c) {
            *count += 1;
        }
    }
    let centre = (1..=colours)
        .max_by_key(|&c| (count.get(c).copied().unwrap_or_default(), Reverse(c)))
        .unwrap_or(1);
    let mut lines = vec![
        ("terrain_set".to_owned(), set.to_string()),
        ("terrain".to_owned(), (centre - 1).to_string()),
    ];
    lines.extend(on.into_iter().filter_map(|(bit, c)| {
        Some((
            format!("terrains_peering_bit/{}", NEIGHBOURS.get(bit)?),
            (c - 1).to_string(),
        ))
    }));
    Some(lines)
}

/// The TileSet's terrain sets and its tiles' terrain lines, worked out once its sources are made
/// ([`Build::terrains`]).
#[derive(Default)]
pub(super) struct Terrains {
    /// The TileSet's lines: each terrain set's mode, then its terrains' names and colors.
    lines: Vec<(String, String)>,
    /// Each Wang tile's terrain: by slot and id.
    tiles: HashMap<(usize, u32), WangTile>,
    /// The terrain sets of a shared TileSet's kept sources: their index in the file → in the
    /// TileSet now (shared.rs renumbers the kept tiles by it).
    pub kept: HashMap<usize, usize>,
}

struct WangTile {
    /// The source its lines go in: the one its cells use.
    source: usize,
    /// Its Wang set, by index in its tileset.
    wang_set: usize,
    lines: Vec<(String, String)>,
}

/// The TileSet's own lines (`terrain_set_N/…`), written before its sources: Godot keeps a tile's
/// peering bits only for terrains it already has.
pub(super) fn tileset_lines(
    _map: &Map,
    _grid: &Grid,
    tileset: &TileSetBuilder,
) -> Vec<(String, String)> {
    tileset.terrain.lines.clone()
}

/// The lines of tile `id` of the tileset in `slot`, in the source whose id is `source`, under its
/// `x:y/0/` (`terrain_set`, `terrain`, `terrains_peering_bit/…`).
pub(super) fn tile_lines(
    _map: &Map,
    _grid: &Grid,
    tileset: &TileSetBuilder,
    source: usize,
    slot: usize,
    id: u32,
) -> Vec<(String, String)> {
    tileset
        .terrain
        .tiles
        .get(&(slot, id))
        .filter(|t| t.source == source)
        .map(|t| t.lines.clone())
        .unwrap_or_default()
}

/// Where a terrain set comes from.
enum Origin {
    /// A Wang set: its tileset's slot, and its index there.
    Wang(usize, usize),
    /// A shared TileSet's file, for its kept sources: the set's index there, and its lines.
    Kept(usize, Vec<(String, String)>),
}

impl Build<'_> {
    /// Works out the terrain sets and each Wang tile's lines from the tilesets the TileSet's
    /// sources hold, once they are numbered.
    pub(super) fn terrains(&mut self) {
        let map = self.map;
        // Each tileset's place, its first source: by slot in a scene's own TileSet, by the
        // registry in a shared one. And the source each tile's cells use: an animated tile's
        // animation source, which comes after its tileset's own.
        let mut first: BTreeMap<usize, usize> = BTreeMap::new();
        let mut home: HashMap<(usize, u32), usize> = HashMap::new();
        for source in &self.tileset.sources {
            for def in source.tiles.values() {
                let place = first.entry(def.slot).or_insert(source.id);
                *place = (*place).min(source.id);
                home.insert((def.slot, def.id), source.id);
            }
        }
        let mut sets: Vec<((usize, usize), Origin)> = first
            .iter()
            .flat_map(|(&slot, &place)| {
                (0..map
                    .tilesets
                    .get(slot)
                    .map_or(0, |t| t.tileset.wang_sets.len()))
                    .map(move |n| ((place, n), Origin::Wang(slot, n)))
            })
            .collect();
        if let Some(shared) = &self.tileset.shared {
            sets.extend(
                shared
                    .kept_terrain_sets()
                    .into_iter()
                    .map(|(place, old, lines)| ((place, old), Origin::Kept(old, lines))),
            );
        }
        sets.sort_by_key(|(place, _)| *place);

        let mut terrain = Terrains::default();
        let mut index = 0;
        // Tiles already in an earlier Wang set's terrain set, by tileset and by the set lacking them.
        let mut several: BTreeMap<usize, BTreeMap<usize, Vec<u32>>> = BTreeMap::new();
        for (_, from) in sets {
            match from {
                Origin::Kept(old, lines) => {
                    terrain.lines.extend(
                        lines
                            .into_iter()
                            .map(|(what, value)| (format!("terrain_set_{index}/{what}"), value)),
                    );
                    terrain.kept.insert(old, index);
                }
                Origin::Wang(slot, n) => {
                    let Some(ts) = map.tilesets.get(slot).map(|t| &t.tileset) else {
                        self.failures
                            .push(format!("terrain slot {slot} is missing"));
                        continue;
                    };
                    let Some(set) = ts.wang_sets.get(n) else {
                        self.failures.push(format!("terrain set {n} is missing"));
                        continue;
                    };
                    let Some((mode, bits)) = mapping(map.orientation, map.stagger_axis, set.kind)
                    else {
                        let message = format!(
                            "tileset {}: Wang set {} is left out: Tiled's corners on a hexagonal map aren't a hexagon's, so Godot has no terrain set for them",
                            ts.name, set.name
                        );
                        self.warn_once(
                            crate::Code::WangHexCorners,
                            message,
                            vec![Element::Tileset(slot)],
                        );
                        continue;
                    };
                    if map.orientation == Orientation::Hexagonal {
                        self.hex_note(slot, n);
                    }
                    terrain
                        .lines
                        .push((format!("terrain_set_{index}/mode"), mode.to_string()));
                    for (m, colour) in set.colors.iter().enumerate() {
                        terrain.lines.push((
                            format!("terrain_set_{index}/terrain_{m}/name"),
                            text::string(&colour.name),
                        ));
                        terrain.lines.push((
                            format!("terrain_set_{index}/terrain_{m}/color"),
                            text::color(colour.color),
                        ));
                    }
                    for (&bt_core::TileId(id), &wang) in &set.tiles {
                        let Some(&source) = home.get(&(slot, id)) else {
                            continue;
                        };
                        let Some(lines) = wang_tile_lines(wang, &bits, set.colors.len(), index)
                        else {
                            continue;
                        };
                        if terrain.tiles.contains_key(&(slot, id)) {
                            several
                                .entry(slot)
                                .or_default()
                                .entry(n)
                                .or_default()
                                .push(id);
                            continue;
                        }
                        terrain.tiles.insert(
                            (slot, id),
                            WangTile {
                                source,
                                wang_set: n,
                                lines,
                            },
                        );
                    }
                }
            }
            index += 1;
        }
        for (slot, lacking) in several {
            self.several_sets_note(slot, &lacking);
        }
        self.counts.terrain_sets = index;
        self.tileset.terrain = terrain;
        self.flipped_terrain_cells();
    }

    /// The warning for a tileset's tiles in more than one Wang set: each is in the first one's
    /// terrain set alone, and the note names the sets that lack them (`lacking`: by Wang set,
    /// the tiles it lacks).
    fn several_sets_note(&mut self, slot: usize, lacking: &BTreeMap<usize, Vec<u32>>) {
        let Some(ts) = self.map.tilesets.get(slot).map(|t| &t.tileset) else {
            self.failures
                .push(format!("terrain slot {slot} is missing"));
            return;
        };
        let mut ids: Vec<u32> = lacking.values().flatten().copied().collect();
        ids.sort_unstable();
        ids.dedup();
        let names: Vec<&str> = lacking
            .keys()
            .filter_map(|&n| ts.wang_sets.get(n).map(|s| s.name.as_str()))
            .collect();
        let sets = match names.as_slice() {
            [one] => format!("the terrain set of {one} lacks"),
            [first @ .., last] if names.len() <= 3 => {
                format!("the terrain sets of {} and {last} lack", first.join(", "))
            }
            _ => format!(
                "the terrain sets of {} and {} more lack",
                names.iter().take(3).copied().collect::<Vec<_>>().join(", "),
                names.len() - 3
            ),
        };
        let message = match (ids.as_slice(), names.len()) {
            ([id], _) => format!(
                "tileset {}: tile {id} is in more than one Wang set, and a Godot tile is in one terrain set: it's in the first one's alone, so {sets} it",
                ts.name
            ),
            (_, 1) => format!(
                "tileset {}: {} tiles are in more than one Wang set, and a Godot tile is in one terrain set: each is in the first one's alone, so {sets} them ({})",
                ts.name,
                ids.len(),
                super::tileset::id_runs(&ids)
            ),
            _ => format!(
                "tileset {}: {} tiles are in more than one Wang set, and a Godot tile is in one terrain set: each is in the first one's alone, so {sets} some of them ({})",
                ts.name,
                ids.len(),
                super::tileset::id_runs(&ids)
            ),
        };
        let elements = ids
            .iter()
            .map(|&id| Element::Tile { tileset: slot, id })
            .collect();
        self.notes.push(super::report::Note::warning(
            crate::Code::WangSeveralSets,
            message,
            elements,
        ));
    }

    /// The warning for a Wang set that becomes a terrain set of sides on a hexagonal map.
    fn hex_note(&mut self, slot: usize, n: usize) {
        let Some(ts) = self.map.tilesets.get(slot).map(|t| &t.tileset) else {
            self.failures
                .push(format!("terrain slot {slot} is missing"));
            return;
        };
        let Some(set) = ts.wang_sets.get(n) else {
            self.failures.push(format!("terrain set {n} is missing"));
            return;
        };
        let (away, sides) = if self.map.stagger_axis == StaggerAxis::X {
            ("columns", "top and bottom")
        } else {
            ("rows", "left and right")
        };
        let message = match set.kind {
            WangSetType::Edge => format!(
                "tileset {}: Wang set {} becomes a terrain set of a cell's 6 sides, as Godot's hexagonal terrains are, and Tiled's edges are 4 of them: its tiles have no terrain on the {sides} sides",
                ts.name, set.name
            ),
            _ => format!(
                "tileset {}: Wang set {} becomes a terrain set of a cell's 6 sides, as Godot's hexagonal terrains are: its colours toward the cells two {away} away, which Tiled matches too, are left out",
                ts.name, set.name
            ),
        };
        self.warn_once(
            crate::Code::WangHexSides,
            message,
            vec![Element::Tileset(slot)],
        );
    }

    /// One warning per Wang set whose tiles the map's layers flip so that their colours change:
    /// Godot's terrains know a tile's colours unflipped only.
    fn flipped_terrain_cells(&mut self) {
        let mut cells = Vec::new();
        flipped_cells(&self.map.layers, &mut cells);
        let mut counts: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        for cell in cells {
            let Some(slot) = cell.slot() else { continue };
            let Some(tile) = self
                .tileset
                .terrain
                .tiles
                .get(&(slot.index(), cell.tile().0))
            else {
                continue;
            };
            let key = (slot.index(), tile.wang_set);
            let Some(set) = self
                .map
                .tilesets
                .get(slot.index())
                .and_then(|t| t.tileset.wang_sets.get(tile.wang_set))
            else {
                continue;
            };
            let Some(wang) = set.tiles.get(&cell.tile()) else {
                continue;
            };
            if wang_changes(*wang, cell.flip()) {
                *counts.entry(key).or_default() += 1;
            }
        }
        for ((slot, n), count) in counts {
            let Some(ts) = self.map.tilesets.get(slot).map(|t| &t.tileset) else {
                self.failures
                    .push(format!("terrain slot {slot} is missing"));
                continue;
            };
            let (cells, have, them) = match count {
                1 => (
                    "1 flipped cell".to_owned(),
                    "has other colours than its tile",
                    "the cell by the tile's",
                ),
                _ => (
                    format!("{count} flipped cells"),
                    "have other colours than their tiles",
                    "the cells by the tiles'",
                ),
            };
            let message = format!(
                "tileset {}: {cells} of Wang set {} {have}, and Godot's terrains don't flip: its terrain painting matches {them} own colours",
                ts.name,
                ts.wang_sets
                    .get(n)
                    .map(|s| s.name.as_str())
                    .unwrap_or("missing set")
            );
            self.warn_once(
                crate::Code::WangFlipped,
                message,
                vec![Element::Tileset(slot)],
            );
        }
    }
}

/// The flipped cells of the tile layers the scene has (a `noExport` layer's left out).
fn flipped_cells(layers: &[Layer], cells: &mut Vec<Cell>) {
    for layer in layers.iter().filter(|l| !super::layers::no_export(l)) {
        match &layer.kind {
            LayerKind::Tile(tiles) => cells.extend(
                tiles
                    .cells()
                    .map(|p| p.cell)
                    .filter(|c| !c.flip().is_empty() && !c.is_empty()),
            ),
            LayerKind::Group(children) => flipped_cells(children, cells),
            _ => {}
        }
    }
}

fn wang_changes(wang: WangId, flip: bt_core::Flip) -> bool {
    let lanes = wang.lanes();
    let points = [
        (0, -1),
        (1, -1),
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
    ];
    points.iter().enumerate().any(|(i, &(mut x, mut y))| {
        if flip.contains(bt_core::Flip::D) {
            (x, y) = (y, x);
        }
        if flip.contains(bt_core::Flip::H) {
            x = -x;
        }
        if flip.contains(bt_core::Flip::V) {
            y = -y;
        }
        points
            .iter()
            .position(|p| *p == (x, y))
            .is_some_and(|j| lanes.get(i) != lanes.get(j))
    })
}
