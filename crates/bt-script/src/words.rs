//! How reports write numbers, cells, points, regions and lists, the same way in every command:
//! cells as `X,Y`, points in tile units with at most two decimals, regions as `X,Y WxH`, long
//! lists cut with "and N more" (`10-agents.md`, §2 and §9).

use bt_utils::IRect;
use glam::{DVec2, IVec2};

/// A number in tile units: at most two decimals, no trailing zeros, never `-0`.
pub fn number_text(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    let text = format!("{rounded:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".to_owned()
    } else {
        text.to_owned()
    }
}

/// A cell, `X,Y`.
pub fn cell_text(cell: IVec2) -> String {
    format!("{},{}", cell.x, cell.y)
}

/// A point in tile units, `X,Y`: `10.5,6` is the middle of the bottom edge of cell 10,5.
pub fn point_text(point: DVec2) -> String {
    format!("{},{}", number_text(point.x), number_text(point.y))
}

/// A region, `X,Y WxH`.
pub fn region_text(region: IRect) -> String {
    format!("{},{} {}x{}", region.x, region.y, region.w, region.h)
}

/// Cells separated by spaces, at most `max` of them, then how many more.
pub fn cells_text(cells: &[IVec2], max: usize) -> String {
    let mut parts: Vec<String> = cells.iter().take(max).map(|c| cell_text(*c)).collect();
    if cells.len() > max {
        parts.push(format!("and {} more", cells.len() - max));
    }
    parts.join(" ")
}

/// Items separated by commas, at most `max` of them, then how many more.
pub fn capped_list<S: AsRef<str>>(items: &[S], max: usize) -> String {
    let shown: Vec<&str> = items.iter().take(max).map(AsRef::as_ref).collect();
    let mut text = shown.join(", ");
    if items.len() > max {
        text.push_str(&format!(" and {} more", items.len() - max));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_keep_two_decimals_at_most_and_drop_trailing_zeros() {
        assert_eq!(number_text(10.5), "10.5");
        assert_eq!(number_text(6.0), "6");
        assert_eq!(number_text(1.226), "1.23");
        assert_eq!(number_text(-0.001), "0");
        assert_eq!(point_text(DVec2::new(10.5, 6.0)), "10.5,6");
    }

    #[test]
    fn regions_cells_and_lists_are_written_one_way() {
        assert_eq!(
            region_text(IRect::from_min_size(IVec2::new(1, 2), IVec2::new(3, 4))),
            "1,2 3x4"
        );
        let cells = [IVec2::new(1, 2), IVec2::new(3, 4), IVec2::new(5, 6)];
        assert_eq!(cells_text(&cells, 2), "1,2 3,4 and 1 more");
        assert_eq!(cells_text(&cells, 3), "1,2 3,4 5,6");
        assert_eq!(capped_list(&["a", "b", "c"], 2), "a, b and 1 more");
        assert_eq!(capped_list(&["a"], 2), "a");
    }
}
