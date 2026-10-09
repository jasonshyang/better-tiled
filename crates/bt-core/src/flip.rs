//! How a cell's tile is turned and mirrored.

bitflags::bitflags! {
    /// The transformation of a cell's tile, with Tiled's in-memory meaning
    /// (`reference/tiled/src/libtiled/tilelayer.h`). On square grids the tile is mirrored along
    /// the anti-diagonal (`D`), then left-right (`H`), then top-bottom (`V`). On hexagonal grids
    /// it is mirrored by `H` and `V`, then turned 60° clockwise for `D` and 120° for `HEX120`.
    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
    pub struct Flip: u8 {
        /// Mirrored left-right.
        const H = 0b0001;
        /// Mirrored top-bottom.
        const V = 0b0010;
        /// Mirrored along the anti-diagonal (a transpose); on hexagonal grids, turned 60°.
        const D = 0b0100;
        /// On hexagonal grids, turned 120°.
        const HEX120 = 0b1000;
    }
}

impl Flip {
    /// The tile as it is drawn, then mirrored left-right.
    pub fn flipped_horizontally(self) -> Flip {
        self ^ Flip::H
    }

    /// The tile as it is drawn, then mirrored top-bottom.
    pub fn flipped_vertically(self) -> Flip {
        self ^ Flip::V
    }

    /// The tile as it is drawn, then turned a quarter clockwise (Tiled's `Cell::rotate`).
    pub fn rotated_cw(self) -> Flip {
        // A clockwise quarter turn is a transpose followed by a left-right mirror; composed after
        // the drawn transform it gives this table.
        Flip::from_parts(
            !self.contains(Flip::V),
            self.contains(Flip::H),
            !self.contains(Flip::D),
        ) | (self & Flip::HEX120)
    }

    /// The tile as it is drawn, then turned a quarter counter-clockwise.
    pub fn rotated_ccw(self) -> Flip {
        Flip::from_parts(
            self.contains(Flip::V),
            !self.contains(Flip::H),
            !self.contains(Flip::D),
        ) | (self & Flip::HEX120)
    }

    /// On a hexagonal grid: the tile as it is drawn, then turned 60° clockwise (Tiled's
    /// `TileLayer::rotateHexagonal`).
    pub fn rotated_hex_cw(self) -> Flip {
        // Tiled's table has 0 at index 11; its own comment and the drawing say 6.
        const TABLE: [u8; 16] = [2, 12, 1, 14, 6, 8, 5, 10, 10, 4, 9, 6, 14, 0, 13, 2];
        self.remapped_hex(&TABLE)
    }

    /// On a hexagonal grid: the tile as it is drawn, then turned 60° counter-clockwise.
    pub fn rotated_hex_ccw(self) -> Flip {
        const TABLE: [u8; 16] = [13, 2, 0, 1, 9, 6, 4, 5, 5, 10, 8, 9, 1, 14, 12, 13];
        self.remapped_hex(&TABLE)
    }

    /// On a hexagonal grid, where a mirror also changes the turn: the tile as it is drawn, then
    /// mirrored left-right (Tiled's `TileLayer::flipHexagonal`).
    pub fn flipped_hex_horizontally(self) -> Flip {
        const TABLE: [u8; 16] = [8, 6, 5, 4, 12, 2, 1, 0, 0, 14, 13, 12, 4, 10, 9, 8];
        self.remapped_hex(&TABLE)
    }

    /// On a hexagonal grid: the tile as it is drawn, then mirrored top-bottom.
    pub fn flipped_hex_vertically(self) -> Flip {
        const TABLE: [u8; 16] = [4, 10, 9, 8, 0, 14, 13, 12, 12, 2, 1, 0, 8, 6, 5, 4];
        self.remapped_hex(&TABLE)
    }

    /// The flags from mirrors and a transpose, without hexagonal turns.
    fn from_parts(h: bool, v: bool, d: bool) -> Flip {
        let mut flip = Flip::empty();
        flip.set(Flip::H, h);
        flip.set(Flip::V, v);
        flip.set(Flip::D, d);
        flip
    }

    /// Looks the flags up in one of Tiled's hexagonal tables, which are indexed by
    /// `H << 3 | V << 2 | D << 1 | HEX120` and hold flags in the same layout.
    fn remapped_hex(self, table: &[u8; 16]) -> Flip {
        let bit = |flag: Flip, shift: u8| u8::from(self.contains(flag)) << shift;
        let index = bit(Flip::H, 3) | bit(Flip::V, 2) | bit(Flip::D, 1) | bit(Flip::HEX120, 0);
        // The index has four bits, so the lookup always succeeds.
        let mapped = table.get(usize::from(index)).copied().unwrap_or(index);
        let mut flip = Flip::from_parts(mapped & 8 != 0, mapped & 4 != 0, mapped & 2 != 0);
        flip.set(Flip::HEX120, mapped & 1 != 0);
        flip
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(mask: u8) -> Flip {
        Flip::from_parts(mask & 4 != 0, mask & 2 != 0, mask & 1 != 0)
    }

    fn square_mask(f: Flip) -> u8 {
        (u8::from(f.contains(Flip::H)) << 2)
            | (u8::from(f.contains(Flip::V)) << 1)
            | u8::from(f.contains(Flip::D))
    }

    /// Tiled's `Cell::rotate` tables (`reference/tiled/src/libtiled/tilelayer.cpp`), indexed by
    /// `H << 2 | V << 1 | D`.
    #[test]
    fn quarter_turns_match_tileds_tables() {
        const LEFT: [u8; 8] = [3, 2, 7, 6, 1, 0, 5, 4];
        const RIGHT: [u8; 8] = [5, 4, 1, 0, 7, 6, 3, 2];
        for mask in 0..8u8 {
            assert_eq!(
                square_mask(square(mask).rotated_ccw()),
                LEFT[usize::from(mask)],
                "left {mask}"
            );
            assert_eq!(
                square_mask(square(mask).rotated_cw()),
                RIGHT[usize::from(mask)],
                "right {mask}"
            );
        }
    }

    #[test]
    fn four_quarter_turns_and_six_sixth_turns_are_the_identity() {
        for bits in 0..16u8 {
            let f = Flip::from_bits_truncate(bits);
            assert_eq!(f.rotated_cw().rotated_cw().rotated_cw().rotated_cw(), f);
            assert_eq!(f.rotated_cw().rotated_ccw(), f);
            let mut turned = f;
            for _ in 0..6 {
                turned = turned.rotated_hex_cw();
            }
            let canonical = f.rotated_hex_cw().rotated_hex_ccw();
            assert_eq!(
                turned.rotated_hex_cw().rotated_hex_ccw(),
                canonical,
                "{bits}"
            );
        }
    }

    /// The transform a hexagonal cell is drawn with: mirrored by H and V, then turned clockwise
    /// by 60° for D and 120° for HEX120. Rows of a 2×2 matrix.
    fn hex_transform(f: Flip) -> [[f64; 2]; 2] {
        let degrees = 60.0 * f64::from(u8::from(f.contains(Flip::D)))
            + 120.0 * f64::from(u8::from(f.contains(Flip::HEX120)));
        let (s, c) = degrees.to_radians().sin_cos();
        let sx = if f.contains(Flip::H) { -1.0 } else { 1.0 };
        let sy = if f.contains(Flip::V) { -1.0 } else { 1.0 };
        [[c * sx, -s * sy], [s * sx, c * sy]]
    }

    fn multiply(a: [[f64; 2]; 2], b: [[f64; 2]; 2]) -> [[f64; 2]; 2] {
        let e = |i: usize, j: usize| a[i][0] * b[0][j] + a[i][1] * b[1][j];
        [[e(0, 0), e(0, 1)], [e(1, 0), e(1, 1)]]
    }

    fn same(a: [[f64; 2]; 2], b: [[f64; 2]; 2]) -> bool {
        (0..2).all(|i| (0..2).all(|j| (a[i][j] - b[i][j]).abs() < 1e-9))
    }

    #[test]
    fn hexagonal_tables_agree_with_what_the_flags_mean_when_drawn() {
        let turn = |degrees: f64| {
            let (s, c) = degrees.to_radians().sin_cos();
            [[c, -s], [s, c]]
        };
        for bits in 0..16u8 {
            let f = Flip::from_bits_truncate(bits);
            let drawn = hex_transform(f);
            assert!(
                same(
                    multiply(turn(60.0), drawn),
                    hex_transform(f.rotated_hex_cw())
                ),
                "cw {bits}"
            );
            assert!(
                same(
                    multiply(turn(-60.0), drawn),
                    hex_transform(f.rotated_hex_ccw())
                ),
                "ccw {bits}"
            );
            assert!(
                same(
                    multiply([[-1.0, 0.0], [0.0, 1.0]], drawn),
                    hex_transform(f.flipped_hex_horizontally())
                ),
                "h {bits}"
            );
            assert!(
                same(
                    multiply([[1.0, 0.0], [0.0, -1.0]], drawn),
                    hex_transform(f.flipped_hex_vertically())
                ),
                "v {bits}"
            );
        }
    }
}
