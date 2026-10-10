//! What an RPG Maker sheet's shape and name say: how many A2 autotiles it holds, its likely tile
//! size, and whether it is another kind of sheet, which would not be put together right.

use std::fmt;
use std::path::Path;

use glam::UVec2;

/// The A2 autotiles in a sheet `width`×`height` pixels of `tile`-pixel tiles: its columns and
/// rows of 2×3-tile blocks.
pub fn autotiles(width: u32, height: u32, tile: u32) -> UVec2 {
    let across = tile.checked_mul(2).and_then(|w| width.checked_div(w));
    let down = tile.checked_mul(3).and_then(|h| height.checked_div(h));
    match (across, down) {
        (Some(columns), Some(rows)) => UVec2::new(columns, rows),
        _ => UVec2::ZERO,
    }
}

/// Whether a sheet `width`×`height` pixels is a whole number of autotiles of `tile`-pixel tiles.
fn whole_autotiles(width: u32, height: u32, tile: u32) -> bool {
    tile > 0
        && autotiles(width, height, tile) * UVec2::new(2, 3) * tile == UVec2::new(width, height)
}

/// The tile size a sheet `width`×`height` pixels most likely has: a whole A2 sheet is 16×12
/// tiles and a single autotile 2×3, of RPG Maker's 48 or 32 pixels (or another whole RPG Maker
/// sheet, which [`sheet_warning`] then tells apart); else `current` when the sheet is whole
/// autotiles of it; else the size of either shape, when it is even. `None` when nothing fits.
pub fn guess_tile(width: u32, height: u32, current: u32) -> Option<u32> {
    let shape = |columns: u32, rows: u32| {
        let tile = width / columns;
        (tile * columns == width && u64::from(tile) * u64::from(rows) == u64::from(height))
            .then_some(tile)
    };
    let shapes = [shape(16, 12), shape(2, 3)];
    let even = |t: &u32| *t > 0 && t.is_multiple_of(2);
    if let Some(&tile) = shapes.iter().flatten().find(|t| [48, 32].contains(*t)) {
        return Some(tile);
    }
    let other_sheet = |tile: u32| {
        SHEETS
            .iter()
            .filter_map(|sheet| sheet.tiles)
            .any(|tiles| UVec2::new(width, height) == tiles * tile)
    };
    if let Some(tile) = [48, 32].into_iter().find(|&t| other_sheet(t)) {
        return Some(tile);
    }
    if even(&current) && whole_autotiles(width, height, current) {
        return Some(current);
    }
    shapes.into_iter().flatten().find(even)
}

/// Why a sheet would not be put together right as A2 floors.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SheetWarning {
    /// An A1 sheet: animated water, whose frames would each become a terrain of their own and
    /// whose waterfalls would not be put together right.
    A1,
    /// An A3 sheet: roofs and building walls, autotiles of 2×2 tiles, not 2×3.
    A3,
    /// An A4 sheet: walls, rows of wall tops three tiles high and wall sides two tiles high,
    /// which do not line up with 2×3 autotiles.
    A4,
    /// An A5 sheet: plain tiles, not autotiles.
    A5,
    /// A B, C, D or E sheet: plain tiles, not autotiles.
    Plain,
    /// Not a whole number of 2×3 autotiles of `tile`-pixel tiles: the part past the last whole
    /// one is left out.
    Partial {
        /// The tile size.
        tile: u32,
    },
}

impl fmt::Display for SheetWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SheetWarning::A1 => f.write_str(
                "This looks like an A1 sheet (animated water): each animation frame becomes a \
                 terrain of its own, and its waterfalls are not put together right",
            ),
            SheetWarning::A3 => f.write_str(
                "This looks like an A3 sheet (roofs and building walls): its autotiles are 2×2 \
                 tiles, not the 2×3 of A2 floors, so they are not put together right",
            ),
            SheetWarning::A4 => f.write_str(
                "This looks like an A4 sheet (walls): its rows of wall tops (3 tiles high) and \
                 wall sides (2 tiles high) do not line up with A2's 2×3 autotiles, so only the \
                 first row comes out right",
            ),
            SheetWarning::A5 => {
                f.write_str("This looks like an A5 sheet: its tiles are plain tiles, not autotiles")
            }
            SheetWarning::Plain => {
                f.write_str("This looks like a B–E sheet: its tiles are plain tiles, not autotiles")
            }
            SheetWarning::Partial { tile } => write!(
                f,
                "The image is not a whole number of autotiles of 2×3 {tile}-pixel tiles: the part \
                 past the last whole one is left out"
            ),
        }
    }
}

/// RPG Maker's sheets other than A2, by the end of their file name (`Outside_A3.png`) and their
/// size in tiles (an A1 sheet is the size of an A2 one).
struct SheetKind {
    name: &'static str,
    tiles: Option<UVec2>,
    warning: SheetWarning,
}

const SHEETS: [SheetKind; 6] = [
    SheetKind {
        name: "A1",
        tiles: None,
        warning: SheetWarning::A1,
    },
    SheetKind {
        name: "A3",
        tiles: Some(UVec2::new(16, 8)),
        warning: SheetWarning::A3,
    },
    SheetKind {
        name: "A4",
        tiles: Some(UVec2::new(16, 15)),
        warning: SheetWarning::A4,
    },
    SheetKind {
        name: "A5",
        tiles: Some(UVec2::new(8, 16)),
        warning: SheetWarning::A5,
    },
    SheetKind {
        name: "B",
        tiles: Some(UVec2::new(16, 16)),
        warning: SheetWarning::Plain,
    },
    SheetKind {
        name: "C",
        tiles: None,
        warning: SheetWarning::Plain,
    },
];

/// Why a sheet named `file_name`, `width`×`height` pixels, would not be put together right as A2
/// floors of `tile`-pixel tiles: another kind of RPG Maker sheet (by the end of its name, such as
/// `World_A1`, else by its size), or not a whole number of autotiles. `None` when it would.
pub fn sheet_warning(file_name: &str, width: u32, height: u32, tile: u32) -> Option<SheetWarning> {
    let stem = Path::new(file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_uppercase())
        .unwrap_or_default();
    // The name ends in the kind after a separator: `Outside_A2`, `World-e`, not `TILE`.
    let named = |kind: &str| {
        stem.strip_suffix(kind)
            .is_some_and(|rest| !rest.is_empty() && !rest.ends_with(char::is_alphanumeric))
    };
    if named("A2") {
        return partial(width, height, tile);
    }
    let by_name = SHEETS
        .iter()
        .find(|s| named(s.name) || (s.name == "C" && (named("D") || named("E"))));
    let by_size = || {
        let whole = tile > 0 && width.is_multiple_of(tile) && height.is_multiple_of(tile);
        let size = size_in_tiles(width, height, tile);
        SHEETS.iter().find(|s| whole && s.tiles == size)
    };
    match by_name.or_else(by_size) {
        Some(kind) => Some(kind.warning),
        None => partial(width, height, tile),
    }
}

/// The sheet's size in tiles, when the tile size is not 0.
fn size_in_tiles(width: u32, height: u32, tile: u32) -> Option<UVec2> {
    Some(UVec2::new(
        width.checked_div(tile)?,
        height.checked_div(tile)?,
    ))
}

/// The warning for a sheet that is not a whole number of 2×3-tile autotiles.
fn partial(width: u32, height: u32, tile: u32) -> Option<SheetWarning> {
    (tile > 0 && !whole_autotiles(width, height, tile)).then_some(SheetWarning::Partial { tile })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_sizes_of_whole_sheets_and_single_autotiles() {
        assert_eq!(
            guess_tile(768, 576, 32),
            Some(48),
            "an RPG Maker MV A2 sheet"
        );
        assert_eq!(guess_tile(512, 384, 48), Some(32), "VX Ace");
        assert_eq!(guess_tile(96, 144, 16), Some(48), "one autotile");
        assert_eq!(
            guess_tile(128, 96, 32),
            Some(32),
            "two autotiles of the map's tile size, not 16×12 tiles of 8 pixels"
        );
        assert_eq!(
            guess_tile(160, 120, 7),
            Some(10),
            "a sheet of 16×12 tiles of another size"
        );
        assert_eq!(guess_tile(100, 100, 32), None);
        assert_eq!(
            guess_tile(768, 384, 32),
            Some(48),
            "an MV A3 sheet, which is then said to be one"
        );
        assert_eq!(guess_tile(512, 480, 48), Some(32), "a VX Ace A4 sheet");
        assert_eq!(autotiles(768, 576, 48), UVec2::new(8, 4));
        assert_eq!(autotiles(768, 576, 0), UVec2::ZERO);
    }

    #[test]
    fn other_rpg_maker_sheets_are_told_apart_by_name_or_size() {
        let warn = |name: &str, w: u32, h: u32, tile: u32| sheet_warning(name, w, h, tile);
        assert_eq!(warn("Outside_A2.png", 768, 576, 48), None);
        assert_eq!(
            warn("floors.png", 768, 576, 48),
            None,
            "A1 and A2 sheets are the same size"
        );
        assert_eq!(warn("World_A1.png", 768, 576, 48), Some(SheetWarning::A1));
        assert_eq!(warn("Outside_A3.png", 768, 384, 48), Some(SheetWarning::A3));
        assert_eq!(
            warn("roofs.png", 768, 384, 48),
            Some(SheetWarning::A3),
            "by its size"
        );
        assert_eq!(warn("Inside_A4.png", 768, 720, 48), Some(SheetWarning::A4));
        assert_eq!(warn("walls.png", 512, 480, 32), Some(SheetWarning::A4));
        assert_eq!(warn("Dungeon_A5.png", 384, 768, 48), Some(SheetWarning::A5));
        assert_eq!(
            warn("Inside_C.png", 768, 768, 48),
            Some(SheetWarning::Plain)
        );
        assert_eq!(
            warn("World-e.png", 768, 768, 48),
            Some(SheetWarning::Plain),
            "any separator, any case"
        );
        assert_eq!(
            warn("Outside_B.png", 768, 768, 48),
            Some(SheetWarning::Plain)
        );
        assert_eq!(
            warn("TILE.png", 96, 144, 48),
            None,
            "a name ending in E, not _E"
        );
        assert_eq!(
            warn("Outside_A2.png", 800, 576, 48),
            Some(SheetWarning::Partial { tile: 48 })
        );
        assert_eq!(
            SheetWarning::Partial { tile: 48 }.to_string(),
            "The image is not a whole number of autotiles of 2×3 48-pixel tiles: the part past \
             the last whole one is left out"
        );
    }
}
