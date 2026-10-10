//! Cells, points, sizes and regions as scripts and options write them (`10-agents.md`, §2):
//! cells `X,Y` in whole numbers, points `X,Y` in tile units, sizes `WxH`, regions `X,Y WxH` or
//! `X1,Y1..X2,Y2` with both corners included. `bt`'s options read them through the same
//! functions, so a region option takes `X,Y WxH` unquoted as well as `X,Y,WxH`; a bare
//! `X,Y,W,H` is refused rather than guessed.

use bt_utils::IRect;
use glam::{DVec2, IVec2};

/// A cell, `X,Y` in whole numbers.
pub fn parse_cell(text: &str) -> Result<IVec2, String> {
    let (x, y) = text
        .split_once(',')
        .ok_or_else(|| format!("“{text}” is not a cell: write X,Y"))?;
    let whole = |v: &str| {
        v.trim()
            .parse::<i32>()
            .map_err(|_| format!("“{text}” is not a cell: X and Y are whole numbers"))
    };
    Ok(IVec2::new(whole(x)?, whole(y)?))
}

/// Whether `text` is a cell `X,Y`.
pub(crate) fn is_cell(text: &str) -> bool {
    parse_cell(text).is_ok()
}

/// A point, `X,Y` in tile units (fractions allowed).
pub fn parse_point(text: &str) -> Result<DVec2, String> {
    let (x, y) = text
        .split_once(',')
        .ok_or_else(|| format!("“{text}” is not a point: write X,Y"))?;
    let number = |v: &str| {
        v.trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("“{text}” is not a point: X and Y are numbers"))
    };
    Ok(DVec2::new(number(x)?, number(y)?))
}

/// Whether `text` is a point `X,Y`.
pub(crate) fn is_point(text: &str) -> bool {
    parse_point(text).is_ok()
}

/// A size in cells, `WxH` in positive whole numbers.
pub fn parse_size(text: &str) -> Result<IVec2, String> {
    let (w, h) = text
        .split_once(['x', '×'])
        .ok_or_else(|| format!("“{text}” is not a size: write WxH"))?;
    let positive = |v: &str| {
        v.trim()
            .parse::<i32>()
            .ok()
            .filter(|v| *v > 0)
            .ok_or_else(|| format!("“{text}” is not a size: W and H are positive whole numbers"))
    };
    Ok(IVec2::new(positive(w)?, positive(h)?))
}

/// Whether `text` is a size `WxH` (whole or not).
pub(crate) fn is_size(text: &str) -> bool {
    !text.contains(',') && parse_extent(text).is_ok()
}

/// A size in tile units, `WxH` in positive numbers (fractions allowed): an object's box.
pub fn parse_extent(text: &str) -> Result<DVec2, String> {
    let (w, h) = text
        .split_once(['x', '×'])
        .ok_or_else(|| format!("“{text}” is not a size: write WxH in tiles, e.g. 5x3.5"))?;
    let positive = |v: &str| {
        v.trim()
            .parse::<f64>()
            .ok()
            .filter(|v| *v > 0.0 && v.is_finite())
            .ok_or_else(|| format!("“{text}” is not a size: write WxH in tiles, e.g. 5x3.5"))
    };
    Ok(DVec2::new(positive(w)?, positive(h)?))
}

/// Radii in cells: `RXxRY`, or one radius for both.
pub(crate) fn parse_radii(text: &str) -> Result<DVec2, String> {
    let positive = |v: &str| {
        v.trim()
            .parse::<f64>()
            .ok()
            .filter(|v| *v > 0.0 && v.is_finite())
    };
    match text.split_once(['x', '×']) {
        Some((a, b)) => match (positive(a), positive(b)) {
            (Some(a), Some(b)) => Ok(DVec2::new(a, b)),
            _ => Err(format!("“{text}” is not radii: write RXxRY, e.g. 8x4.5")),
        },
        None => positive(text)
            .map(DVec2::splat)
            .ok_or_else(|| format!("“{text}” is not a radius: a number of cells more than 0")),
    }
}

/// A region of cells: `X,Y WxH` (also `X,Y,WxH`), `X1,Y1..X2,Y2` (both corners included) or
/// one cell `X,Y`. A bare `X,Y,W,H` is refused: which numbers are corners and which are sizes
/// would be a guess.
pub fn parse_region(text: &str) -> Result<IRect, String> {
    let text = text.trim();
    if let Some((a, b)) = text.split_once("..") {
        return Ok(IRect::from_corners(parse_cell(a)?, parse_cell(b)?));
    }
    let parts: Vec<&str> = text.split([' ', ',']).filter(|p| !p.is_empty()).collect();
    let refused =
        || format!("“{text}” is not a region: write X,Y WxH (e.g. 4,2 10x6) or X1,Y1..X2,Y2");
    match parts.as_slice() {
        [x, y, size] if size.contains(['x', '×']) => {
            let at = parse_cell(&format!("{x},{y}")).map_err(|_| refused())?;
            Ok(IRect::from_min_size(at, parse_size(size)?))
        }
        [x, y] => {
            let at = parse_cell(&format!("{x},{y}")).map_err(|_| refused())?;
            Ok(IRect::from_min_size(at, IVec2::ONE))
        }
        _ => Err(refused()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_read_in_every_written_form_and_guesses_are_refused() {
        let expected = IRect::from_min_size(IVec2::new(4, 2), IVec2::new(10, 6));
        assert_eq!(parse_region("4,2 10x6").unwrap(), expected);
        assert_eq!(parse_region("4,2,10x6").unwrap(), expected);
        assert_eq!(parse_region("4,2..13,7").unwrap(), expected);
        assert_eq!(parse_region("13,7..4,2").unwrap(), expected);
        assert_eq!(
            parse_region("3,3").unwrap(),
            IRect::from_min_size(IVec2::new(3, 3), IVec2::ONE)
        );
        assert!(
            parse_region("4,2,10,6")
                .unwrap_err()
                .contains("not a region")
        );
        assert!(parse_region("4,2 0x6").unwrap_err().contains("positive"));
    }

    #[test]
    fn cells_points_sizes_and_radii() {
        assert_eq!(parse_cell("-1,2").unwrap(), IVec2::new(-1, 2));
        assert!(parse_cell("1.5,2").is_err());
        assert_eq!(parse_point("10.5,6").unwrap(), DVec2::new(10.5, 6.0));
        assert!(parse_point("a,b").is_err());
        assert_eq!(parse_size("3×4").unwrap(), IVec2::new(3, 4));
        assert_eq!(parse_extent("5x3.5").unwrap(), DVec2::new(5.0, 3.5));
        assert_eq!(parse_radii("8x4.5").unwrap(), DVec2::new(8.0, 4.5));
        assert_eq!(parse_radii("5").unwrap(), DVec2::splat(5.0));
        assert!(parse_radii("0").is_err());
        assert!(is_size("2x3") && !is_size("2,3") && !is_size("rect"));
    }
}
