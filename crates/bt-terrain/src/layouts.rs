//! Autotile layouts: where each tile of a terrain sits in the sheets that tile generators,
//! engines and packs lay out (the blob sheet of Tiled's manual, Godot 3's templates, Tilesetter's
//! exports, the 3×3 islands and holes of many packs). Once the layout and the block it covers are
//! known, the whole block is labelled at once and exactly, instead of corner by corner
//! (`docs/design/07-terrain.md`, §9).
//!
//! - [`Layout`] and [`LAYOUTS`]: the layouts as text art, and the labels they give a block.
//! - [`Samples`], [`detect`] and [`places`]: the blocks of a tileset image that look like a
//!   layout.
//! - [`KnownTerrains`] and [`recognize`]: Label All, which gives blocks that look alike the same
//!   terrains.
//!
//! The Blob 7×7 arrangement is cr31's (cr31.co.uk), the sheet of Tiled's manual and of Tiled's
//! own test set (`reference/tiled/tests/wangtiles/wangblob.tsx`). The Godot 3 and Tilesetter
//! arrangements follow the templates of TileBitTools
//! (<https://github.com/dandeliondino/tile_bit_tools>, MIT, Copyright (c) 2023 dandeliondino;
//! the notice is in `THIRD_PARTY_NOTICES.md`). The two 5×5 cross islands are how the Epic RPG
//! World packs lay out their terrains, flat and raised, with or without the diagonal tiles; the
//! packs' own labels follow them.

mod detect;
mod histogram;
mod label_all;
mod samples;

use std::collections::BTreeMap;

use bt_core::{TileId, WangId, WangSetType};
use glam::UVec2;

use crate::color::TerrainColor;
use crate::lanes::kind_lanes;

pub use detect::{Fit, detect, places};
pub use histogram::Histogram;
pub use label_all::{Block, KnownTerrains, SAME_GROUND, SAME_TERRAIN, recognize};
pub use samples::{Samples, SideColors};

/// An autotile layout: how the tiles of one terrain are arranged in a block of a sheet.
#[derive(Debug)]
pub struct Layout {
    /// Its name, as a list of layouts shows it.
    pub name: &'static str,
    /// What it holds and where it comes from, in a sentence.
    pub about: &'static str,
    /// The kind of terrain set its labels are for.
    pub kind: WangSetType,
    /// The tiles, row by row, each drawn as 3×3 characters after a `|` margin: its top-left
    /// corner, top edge and top-right corner on the first line, its left edge, middle and right
    /// edge on the second, the bottom ones on the third. `#` is the terrain and `.` what it is
    /// drawn over; a space is a lane the kind does not have. Tiles are one column apart, and a
    /// tile of spaces only is a slot the layout leaves empty.
    art: &'static str,
}

/// A tile of a layout.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    /// The lanes that are the terrain: bit `i` is lane `i`, in Tiled's order (the top edge, the
    /// top-right corner, clockwise to the top-left corner).
    pub terrain: u8,
    /// Whether the middle of the tile is the terrain.
    pub middle: bool,
}

/// Where each lane is drawn in a tile's 3×3 characters: (line, column), in Tiled's lane order.
const LANE_CHARS: [[usize; 2]; 8] = [
    [0, 1],
    [0, 2],
    [1, 2],
    [2, 2],
    [2, 1],
    [2, 0],
    [1, 0],
    [0, 0],
];

impl Layout {
    /// The art's lines, without their margins.
    fn lines(&self) -> Vec<&'static str> {
        self.art
            .lines()
            .filter_map(|line| line.split_once('|').map(|(_, art)| art))
            .collect()
    }

    /// The layout's size in tiles: columns and rows.
    pub fn size(&self) -> UVec2 {
        let lines = self.lines();
        let width = lines.iter().map(|line| line.len()).max().unwrap_or(0);
        UVec2::new(
            u32::try_from(width.div_ceil(4)).unwrap_or(0),
            u32::try_from(lines.len() / 3).unwrap_or(0),
        )
    }

    /// The layout's tiles, row by row; `None` for a slot it leaves empty.
    pub fn slots(&self) -> Vec<Option<Slot>> {
        let lines = self.lines();
        let size = self.size();
        let mut slots = Vec::new();
        for row in 0..size.y as usize {
            for column in 0..size.x as usize {
                let char_at = |line: usize, x: usize| {
                    lines
                        .get(row * 3 + line)
                        .and_then(|l| l.as_bytes().get(column * 4 + x))
                        .copied()
                        .unwrap_or(b' ')
                };
                let middle = char_at(1, 1);
                let mut drawn = middle != b' ';
                let mut terrain = 0_u8;
                for (lane, &[line, x]) in LANE_CHARS.iter().enumerate() {
                    match char_at(line, x) {
                        b'#' => terrain |= 1 << lane,
                        b'.' => {}
                        _ => continue,
                    }
                    drawn = true;
                }
                slots.push(drawn.then_some(Slot {
                    terrain,
                    middle: middle == b'#',
                }));
            }
        }
        slots
    }

    /// How many tiles the layout has (its empty slots not counted).
    pub fn tile_count(&self) -> usize {
        self.slots().iter().flatten().count()
    }

    /// The labels the layout gives the tiles of a block whose top-left tile is at `origin`
    /// (column and row) of an atlas `columns` tiles wide: the terrain's lanes get `terrain`, the
    /// others `under` (no terrain, for a terrain drawn over nothing); the two should differ.
    /// A tile that is all one thing but for its middle (a lone blob, a lone hole) gets no label
    /// (`None`), as labels cannot tell it from a full tile; nor does a tile all of no terrain.
    /// Empty slots and slots past the atlas's columns are left out.
    pub fn labels(
        &self,
        origin: UVec2,
        columns: u32,
        terrain: TerrainColor,
        under: TerrainColor,
    ) -> BTreeMap<TileId, Option<WangId>> {
        let width = self.size().x;
        let mut labels = BTreeMap::new();
        for (index, slot) in self.slots().into_iter().enumerate() {
            let Some(slot) = slot else { continue };
            let Some(tile) = block_tile(origin, columns, width, index) else {
                continue;
            };
            labels.insert(tile, slot.label(self.kind, terrain, under));
        }
        labels
    }
}

impl Slot {
    /// This tile's label in a set of `kind`, with `terrain` on its terrain lanes and `under` on
    /// the others; `None` for a tile that is all one thing but for its middle, or all of no
    /// terrain.
    pub(crate) fn label(
        self,
        kind: WangSetType,
        terrain: TerrainColor,
        under: TerrainColor,
    ) -> Option<WangId> {
        let lanes = kind_lanes(kind);
        let mut values = [0_u8; 8];
        for &lane in lanes {
            if let Some(value) = values.get_mut(lane) {
                *value = if self.is_terrain(lane) {
                    terrain.0
                } else {
                    under.0
                };
            }
        }
        let all = |v: u8| lanes.iter().all(|&lane| values.get(lane) == Some(&v));
        let lone = (all(terrain.0) && !self.middle) || (all(under.0) && self.middle);
        (!lone && !all(0)).then_some(WangId::from_lanes(values))
    }

    /// Whether lane `lane` is the terrain.
    pub(crate) fn is_terrain(self, lane: usize) -> bool {
        lane < 8 && self.terrain & (1 << lane) != 0
    }
}

/// The atlas tile of slot `index` of a block `width` slots wide whose top-left tile is at
/// `origin` of an atlas `columns` tiles wide; `None` past the atlas's columns.
pub(crate) fn block_tile(origin: UVec2, columns: u32, width: u32, index: usize) -> Option<TileId> {
    let index = u64::try_from(index).ok()?;
    let width = u64::from(width);
    let column = u64::from(origin.x) + index.checked_rem(width)?;
    let row = u64::from(origin.y) + index.checked_div(width)?;
    if column >= u64::from(columns) {
        return None;
    }
    u32::try_from(row * u64::from(columns) + column)
        .ok()
        .map(TileId)
}

/// The 47 tiles of a blob set (cr31's), the layout RPG Maker autotiles are composed into.
pub(crate) const BLOB_7X7: Layout = Layout {
    name: "Blob 7×7",
    about: "The 47 tiles of a blob set in 7×7, three of them plain: the sheet of Tiled's manual \
            (from cr31.co.uk), a seamless picture",
    kind: WangSetType::Mixed,
    art: "
    |... ... ... ... ... ... ...
    |... .## ### ### ### ##. ...
    |... ... .## ### ##. .#. ...

    |... ... .## ### ##. .#. ...
    |.#. .## ### ### ##. .## ##.
    |.#. .#. .#. .## ##. .#. ...

    |.#. .#. .#. .## ##. .#. ...
    |.## ### ### ### ### ### ##.
    |.## ##. .#. ... .## ### ##.

    |.## ##. .#. ... .## ### ##.
    |.## ### ##. .## ### ### ##.
    |.## ### ##. .## ### ##. .#.

    |.## ### ##. .## ### ##. .#.
    |.## ### ### ### ### ### ##.
    |.#. ... .#. .## ### ##. .#.

    |.#. ... .#. .## ### ##. .#.
    |.## ### ### ### ### ##. .#.
    |... .#. .## ##. .#. ... .#.

    |... .#. .## ##. .#. ... .#.
    |... .#. .## ### ### ### ##.
    |... ... ... ... ... ... ...
    ",
};

const GODOT_3X3_MINIMAL: Layout = Layout {
    name: "Godot 3×3 minimal",
    about: "The 47 tiles of a blob set and a lone blob in 12×4: Godot 3's “3×3 minimal” \
            autotile template",
    kind: WangSetType::Mixed,
    art: "
    |... ... ... ... ##. ... ... .## ... .#. ... ...
    |.#. .## ### ##. ### ### ### ### .## ### ### ##.
    |.#. .#. .#. .#. .#. .## ##. .#. .## ### ### ##.

    |.#. .#. .#. .#. .#. .## ##. .#. .## .## ... ##.
    |.#. .## ### ##. .## ### ### ##. .## ### ... ###
    |.#. .#. .#. .#. .## ### ### ##. .## ##. ... ##.

    |.#. .#. .#. .#. .## ### ### ##. .## ### ##. ##.
    |.#. .## ### ##. .## ### ### ##. ### ### ### ##.
    |... ... ... ... .#. .## ##. .#. .## ### .## ##.

    |... ... ... ... .#. .## ##. .#. .## ### ### ##.
    |.#. .## ### ##. ### ### ### ### .## ### ### ##.
    |... ... ... ... ##. ... ... .## ... ... .#. ...
    ",
};

const TILESETTER_BLOB: Layout = Layout {
    name: "Tilesetter blob",
    about: "The 47 tiles of a blob set in 11×5, as Tilesetter exports them",
    kind: WangSetType::Mixed,
    art: "
    |... ... ... ... ... ... ... ... ... ##.
    |.## ### ##. .#. .## ### ### ##. ### ###
    |.## ### ##. .#. .#. ##. .## .#. .#. .##

    |.## ### ##. .#. .## ### ### ##. ### .##
    |.## ### ##. .#. .## ### ### ##. ### ###
    |.## ### ##. .#. .#. ##. .## .#. .#. ##.

    |.## ### ##. .#. .#. ##. .## .#. .#. .#. .#.
    |.## ### ##. .#. .## ### ### ##. ### ### ###
    |... ... ... ... .## ### ### ##. ### .## ##.

    |... ... ... ... .#. ##. .## .#. .#. .## ##.
    |.## ### ##. .#. .## ### ### ##. ### ### ###
    |... ... ... ... ... ... ... ... ... .#. .#.

    |                .#. ##. .## .#. .#.
    |                .## ### ### ##. ###
    |                .#. ##. .## .#. .#.
    ",
};

const GODOT_2X2: Layout = Layout {
    name: "Godot 2×2",
    about: "All 16 corner tiles in 4×4: Godot 3's “2×2” autotile template",
    kind: WangSetType::Corner,
    art: "
    |. . . # # . . .
    | #   #   #   #
    |# . . # # # # #

    |# . . # # # # #
    | #   #   #   #
    |. # # # # # # .

    |. # # # # # # .
    | #   #   #   #
    |. . . . . # # .

    |. . . . . # # .
    | .   #   #   #
    |. . . # # . . .
    ",
};

const TILESETTER_WANG: Layout = Layout {
    name: "Tilesetter Wang",
    about: "15 corner tiles in 5×3, all but the plain one, as Tilesetter exports a Wang set",
    kind: WangSetType::Corner,
    art: "
    |. . . . . . # # # #
    | #   #   #   #   #
    |. # # # # . # . . #

    |. # # # # . # . . #
    | #   #   #   #   #
    |. # # # # . # # # #

    |. # # # # . # . . #
    | #   #   #   #   #
    |. . . . . . . # # .
    ",
};

const ISLAND_3X3: Layout = Layout {
    name: "3×3 island",
    about: "Outer corners and sides around a full tile: the terrain as a square island, as many \
            tilesets draw it",
    kind: WangSetType::Corner,
    art: "
    |. . . . . .
    | #   #   #
    |. # # # # .

    |. # # # # .
    | #   #   #
    |. # # # # .

    |. # # # # .
    | #   #   #
    |. . . . . .
    ",
};

const HOLE_3X3: Layout = Layout {
    name: "3×3 hole",
    about: "Inner corners and sides around a plain tile: a square hole in the terrain",
    kind: WangSetType::Corner,
    art: "
    |# # # # # #
    | #   #   #
    |# . . . . #

    |# . . . . #
    | #   .   #
    |# . . . . #

    |# . . . . #
    | #   #   #
    |# # # # # #
    ",
};

const HOLE_2X2: Layout = Layout {
    name: "2×2 hole",
    about: "The four inner corners: a small hole in the terrain, next to a 3×3 island in many \
            tilesets",
    kind: WangSetType::Corner,
    art: "
    |# # # #
    | #   #
    |# . . #

    |# . . #
    | #   #
    |# # # #
    ",
};

const CROSS_ISLAND_DIAGONALS: Layout = Layout {
    name: "5×5 cross island, diagonals",
    about: "All 15 corner tiles: an island with a bump on each side and the two diagonal tiles in \
            its top corners, as the Epic RPG World packs draw most of their terrains",
    kind: WangSetType::Corner,
    art: "
    |. # . . . . . . # .
    | #   #   #   #   #
    |# . . # # # # . . #

    |. . . # # # # . . .
    | #   #   #   #   #
    |. # # # # # # # # .

    |. # # # # # # # # .
    | #   #   #   #   #
    |. # # # # # # # # .

    |. # # # # # # # # .
    | #   #   #   #   #
    |. . . # # # # . . .

    |    . # # # # .
    |     #   #   #
    |    . . . . . .
    ",
};

const CROSS_ISLAND: Layout = Layout {
    name: "5×5 cross island",
    about: "13 corner tiles, all but the diagonal ones: an island with a bump on each side, as the \
            Epic RPG World packs draw some of their terrains",
    kind: WangSetType::Corner,
    art: "
    |    . . . . . .
    |     #   #   #
    |    . # # # # .

    |. . . # # # # . . .
    | #   #   #   #   #
    |. # # # # # # # # .

    |. # # # # # # # # .
    | #   #   #   #   #
    |. # # # # # # # # .

    |. # # # # # # # # .
    | #   #   #   #   #
    |. . . # # # # . . .

    |    . # # # # .
    |     #   #   #
    |    . . . . . .
    ",
};

const GODOT_3X3_SIDES: Layout = Layout {
    name: "Godot 3×3 sides",
    about: "All 16 edge tiles in 4×4: Godot 3's “3×3 (16 tiles)” autotile template, for paths, \
            roads and fences",
    kind: WangSetType::Edge,
    art: "
    | .   .   .   .
    |.#. .## ### ##.
    | #   #   #   #

    | #   #   #   #
    |.#. .## ### ##.
    | #   #   #   #

    | #   #   #   #
    |.#. .## ### ##.
    | .   .   .   .

    | .   .   .   .
    |.#. .## ### ##.
    | .   .   .   .
    ",
};

/// The layouts: blob sets first, then corner sets, then edge sets, each the larger first.
pub const LAYOUTS: &[Layout] = &[
    BLOB_7X7,
    GODOT_3X3_MINIMAL,
    TILESETTER_BLOB,
    CROSS_ISLAND_DIAGONALS,
    CROSS_ISLAND,
    GODOT_2X2,
    TILESETTER_WANG,
    ISLAND_3X3,
    HOLE_3X3,
    HOLE_2X2,
    GODOT_3X3_SIDES,
];

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    pub(super) fn layout(name: &str) -> (usize, &'static Layout) {
        LAYOUTS
            .iter()
            .enumerate()
            .find(|(_, l)| l.name == name)
            .unwrap()
    }

    #[test]
    fn layouts_hold_every_tile_of_their_set_once() {
        for l in LAYOUTS {
            let slots = l.slots();
            let size = l.size();
            assert_eq!(slots.len(), (size.x * size.y) as usize, "{}", l.name);
            let mask: u8 = kind_lanes(l.kind).iter().map(|&i| 1_u8 << i).sum();
            let distinct: BTreeSet<u8> = slots.iter().flatten().map(|s| s.terrain).collect();
            assert!(
                distinct.iter().all(|t| t & !mask == 0),
                "{}: only lanes of its kind",
                l.name
            );
            let expected = match l.name {
                "Blob 7×7" | "Godot 3×3 minimal" | "Tilesetter blob" => 47,
                "Godot 2×2" | "Godot 3×3 sides" => 16,
                "Tilesetter Wang" | "5×5 cross island, diagonals" => 15,
                "5×5 cross island" => 13,
                "3×3 island" | "3×3 hole" => 9,
                "2×2 hole" => 4,
                other => panic!("a layout this test does not know: {other}"),
            };
            assert_eq!(distinct.len(), expected, "{}", l.name);
            if l.kind == WangSetType::Mixed {
                // A blob: a corner is the terrain only between two edges of it.
                for t in &distinct {
                    let on = |lane: usize| t & (1 << lane) != 0;
                    for corner in [1, 3, 5, 7] {
                        assert!(
                            !on(corner) || (on(corner - 1) && on((corner + 1) % 8)),
                            "{}: {t:08b}",
                            l.name
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn layouts_have_their_sizes_and_tile_counts() {
        let counts: Vec<(&str, [u32; 2], usize)> = LAYOUTS
            .iter()
            .map(|l| (l.name, l.size().to_array(), l.tile_count()))
            .collect();
        assert_eq!(
            counts,
            [
                ("Blob 7×7", [7, 7], 49),
                ("Godot 3×3 minimal", [12, 4], 48),
                ("Tilesetter blob", [11, 5], 47),
                ("5×5 cross island, diagonals", [5, 5], 23),
                ("5×5 cross island", [5, 5], 21),
                ("Godot 2×2", [4, 4], 16),
                ("Tilesetter Wang", [5, 3], 15),
                ("3×3 island", [3, 3], 9),
                ("3×3 hole", [3, 3], 9),
                ("2×2 hole", [2, 2], 4),
                ("Godot 3×3 sides", [4, 4], 16),
            ]
        );
        assert_eq!(LAYOUTS[0].name, BLOB_7X7.name, "the blob comes first");
    }

    #[test]
    fn lone_blobs_and_nothing_get_no_labels() {
        let (_, minimal) = layout("Godot 3×3 minimal");
        let c = TerrainColor;
        let tile = |labels: &BTreeMap<TileId, Option<WangId>>, id: u32| labels[&TileId(id)];
        let labels = minimal.labels(UVec2::ZERO, 12, c(1), c(0));
        assert_eq!(
            tile(&labels, 3 * 12),
            None,
            "a lone blob looks like nothing to the labels"
        );
        assert_eq!(tile(&labels, 12 + 10), None, "nothing over nothing");
        let labels = minimal.labels(UVec2::ZERO, 12, c(1), c(2));
        assert_eq!(
            tile(&labels, 12 + 10),
            Some(WangId::from_lanes([2; 8])),
            "the plain tile of the other terrain"
        );
        assert_eq!(tile(&labels, 3 * 12), None);
        assert_eq!(
            tile(&labels, 12 + 11),
            Some(WangId::from_lanes([1, 2, 1, 2, 1, 1, 1, 1])),
            "all but the right corners"
        );
        // The other way around: a lone hole in the terrain is not a full tile either, and the
        // block's tiles are numbered from its origin.
        let (_, hole) = layout("3×3 hole");
        let swapped = hole.labels(UVec2::new(2, 1), 10, c(0), c(1));
        assert_eq!(swapped.len(), 9);
        assert_eq!(
            tile(&swapped, 2 * 10 + 3),
            Some(WangId::from_lanes([0, 1, 0, 1, 0, 1, 0, 1])),
            "the middle, all terrain 1"
        );
        assert_eq!(
            tile(&swapped, 10 + 2),
            Some(WangId::from_lanes([0, 0, 0, 1, 0, 0, 0, 0])),
            "the top-left tile"
        );
    }

    #[test]
    fn blocks_past_the_atlas_edge_lose_the_tiles_outside() {
        let (_, island) = layout("3×3 island");
        let labels = island.labels(UVec2::new(3, 0), 5, TerrainColor(1), TerrainColor(2));
        let tiles: Vec<u32> = labels.keys().map(|t| t.0).collect();
        assert_eq!(
            tiles,
            [3, 4, 8, 9, 13, 14],
            "the third column is past the atlas"
        );
        assert_eq!(block_tile(UVec2::ZERO, 4, 0, 0), None, "no width");
    }
}
