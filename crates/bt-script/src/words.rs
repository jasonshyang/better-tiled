//! How reports write numbers, cells, points, regions and lists, the same way in every command:
//! cells as `X,Y`, points in tile units with at most two decimals, regions as `X,Y WxH`, long
//! lists cut with "and N more" (`10-agents.md`, §2 and §9). Rectangles go into JSON the one way
//! here too ([`RectJson`]).

use bt_core::Value;
use bt_utils::IRect;
use glam::{DVec2, IVec2};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// How a rectangle of cells is written in JSON: `{"x", "y", "w", "h"}`.
#[derive(Serialize, Deserialize)]
#[serde(remote = "IRect")]
pub(crate) struct RectJson {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

/// How a rectangle that may be missing is written in JSON: as [`RectJson`], or `null`.
pub(crate) mod rect_option_json {
    use super::*;

    /// A rectangle written as [`RectJson`] writes it.
    #[derive(Serialize, Deserialize)]
    struct Written(#[serde(with = "RectJson")] IRect);

    /// Writes `rect`, or `null`.
    pub(crate) fn serialize<S: Serializer>(
        rect: &Option<IRect>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        rect.map(Written).serialize(serializer)
    }

    /// Reads a rectangle, or `null`.
    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<IRect>, D::Error> {
        Ok(Option::<Written>::deserialize(deserializer)?.map(|written| written.0))
    }
}

/// A word as a script writes it: as it is, or in quotes when it is empty or holds spaces, quotes,
/// `=` or `#`.
pub fn quoted_word(word: &str) -> String {
    let plain = !word.is_empty()
        && !word
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '"' | '=' | '#'));
    if plain {
        word.to_owned()
    } else {
        format!("\"{}\"", word.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// A property value as `key=value` lists and scripts write it: text quoted when it needs to be,
/// numbers and yes/no as they are, an object reference as `#ID`, a colour as Tiled writes it.
pub fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) | Value::File(text) => quoted_word(text),
        Value::Int(number) => number.to_string(),
        Value::Float(number) => format!("{number}"),
        Value::Bool(yes) => yes.to_string(),
        Value::Color(Some(color)) => color.to_tiled_text(),
        Value::Color(None) => "\"\"".to_owned(),
        Value::Object(id) => format!("#{id}"),
        Value::Class(members) => {
            let inner: Vec<String> = members
                .iter()
                .map(|(key, member)| format!("{key}={}", value_text(&member.value)))
                .collect();
            format!("{{{}}}", inner.join(" "))
        }
        Value::List(items) => {
            let inner: Vec<String> = items.iter().map(|item| value_text(&item.value)).collect();
            format!("[{}]", inner.join(", "))
        }
    }
}

/// Ids as `#3-5 #9`: runs of consecutive ids joined, in order.
pub fn id_ranges_text(ids: &[u32]) -> String {
    let mut sorted = ids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut parts: Vec<String> = Vec::new();
    let mut run: Option<IdRun> = None;
    for id in sorted {
        run = match run {
            Some(open) if open.last.checked_add(1) == Some(id) => Some(IdRun {
                first: open.first,
                last: id,
            }),
            Some(open) => {
                parts.push(open.text());
                Some(IdRun {
                    first: id,
                    last: id,
                })
            }
            None => Some(IdRun {
                first: id,
                last: id,
            }),
        };
    }
    parts.extend(run.map(IdRun::text));
    parts.join(" ")
}

/// Consecutive ids.
#[derive(Clone, Copy)]
struct IdRun {
    first: u32,
    last: u32,
}

impl IdRun {
    fn text(self) -> String {
        if self.first == self.last {
            format!("#{}", self.first)
        } else {
            format!("#{}-{}", self.first, self.last)
        }
    }
}

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

    #[test]
    fn values_words_and_ids_read_as_scripts_write_them() {
        assert_eq!(
            value_text(&Value::String("thin ice".into())),
            "\"thin ice\""
        );
        assert_eq!(value_text(&Value::String("golem".into())), "golem");
        assert_eq!(value_text(&Value::Int(3)), "3");
        assert_eq!(value_text(&Value::Float(2.5)), "2.5");
        assert_eq!(value_text(&Value::Object(bt_core::ObjectId(12))), "#12");
        assert_eq!(quoted_word(""), "\"\"");
        assert_eq!(id_ranges_text(&[9, 3, 4, 5]), "#3-5 #9");
        assert_eq!(id_ranges_text(&[]), "");
    }

    #[test]
    fn rectangles_go_into_json_by_their_fields() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct Holder {
            #[serde(with = "RectJson")]
            rect: IRect,
            #[serde(with = "rect_option_json")]
            maybe: Option<IRect>,
        }
        let holder = Holder {
            rect: IRect::from_min_size(IVec2::new(1, 2), IVec2::new(3, 4)),
            maybe: None,
        };
        let json = serde_json::to_string(&holder).unwrap();
        assert_eq!(json, r#"{"rect":{"x":1,"y":2,"w":3,"h":4},"maybe":null}"#);
        assert_eq!(serde_json::from_str::<Holder>(&json).unwrap(), holder);
    }
}
