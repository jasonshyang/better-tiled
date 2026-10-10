//! The shape language as written (`10-agents.md`, §6): a shape, then what narrows it, written in
//! scripts and on the command line (`bt areas MAP SHAPE`, `bt mark`, `bt render --highlight`).
//! These are syntax only; `crate::shapes` turns them into cells on a map, resolving the names
//! they hold.

use bt_utils::IRect;
use glam::{DVec2, IVec2};

use super::cursor::Cursor;
use super::refs::{LayerRef, Named, ObjectRef, layer_ref_of, named_items, object_ref_of};
use super::tokens::words_of;
use super::values::{is_cell, is_point, parse_radii};

/// A set of cells as written: a shape and what narrows it. The narrowing applies in the order
/// of the fields: `minus`, `on`, `inside`, `avoid`, `near`, `containing`.
#[derive(Clone, Debug, PartialEq)]
pub struct Area {
    /// The shape whose cells are narrowed.
    pub shape: Shape,
    /// `minus SHAPE`: cells taken out, any number of times.
    pub minus: Vec<Shape>,
    /// `on G,…`: keep the cells one of the grounds covers wholly.
    pub on: Vec<Named>,
    /// `inside G,…`: keep the cells that one of the grounds covers wholly, together with every
    /// cell touching them, so edges painted around the area stay on the ground.
    pub inside: Vec<Named>,
    /// `avoid G,… [margin N]`: drop the cells within the margin of a ground's touch; a margin of
    /// 0 (the default) drops the cells a ground touches.
    pub avoid: Vec<Nearness>,
    /// `near G,… [margin N]`: keep the cells within the margin of a ground (1 by default).
    pub near: Vec<Nearness>,
    /// `containing X,Y …`: keep the connected parts that hold one of these cells.
    pub containing: Vec<IVec2>,
}

impl Area {
    /// The cells of `shape`, not narrowed.
    pub fn of(shape: Shape) -> Area {
        Area {
            shape,
            minus: Vec::new(),
            on: Vec::new(),
            inside: Vec::new(),
            avoid: Vec::new(),
            near: Vec::new(),
            containing: Vec::new(),
        }
    }
}

/// Grounds and a distance from them in cells, counted over touching cells: what `avoid` and
/// `near` take.
#[derive(Clone, Debug, PartialEq)]
pub struct Nearness {
    /// The grounds.
    pub grounds: Vec<Named>,
    /// The distance in cells.
    pub margin: u32,
}

/// A shape: the cells a narrowing starts from, or takes out with `minus`.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// `X,Y` or `cells X,Y …`.
    Cells(Vec<IVec2>),
    /// `rect X,Y WxH` or `rect X1,Y1..X2,Y2` (corners inclusive).
    Rect(IRect),
    /// `all`: the map's cells; on an infinite map, the area painted so far, or its saved size
    /// while it is empty.
    All,
    /// `ellipse CX,CY RXxRY [rough R]` or `circle CX,CY R [rough R]`: the centre in tile units,
    /// the radii in cells; `rough` moves the radius by up to that part of itself with smooth
    /// noise around the ellipse, drawn from the operation's seed.
    Ellipse {
        /// The centre, in tile units.
        center: DVec2,
        /// The radii, in cells.
        radii: DVec2,
        /// How rough the outline is, as a part of the radius.
        rough: Option<f64>,
    },
    /// `poly X,Y …`: the cells whose centres are inside, by the even-odd rule.
    Polygon(Vec<DVec2>),
    /// `path X,Y … [width W]`: the cells whose centres are within half the width of the line,
    /// plus a line of cells along it that share sides, so a thin path never hangs together by
    /// corners only.
    Path {
        /// The points, in tile units.
        points: Vec<DVec2>,
        /// The width, in cells (1 by default).
        width: f64,
    },
    /// `where G,…`: the cells one of the grounds covers wholly.
    Where(Vec<Named>),
    /// `under LAYER,…`: the cells the shown objects of those layers cover.
    Under(Vec<LayerRef>),
    /// `@NAME`, `@#ID`, `@selected`: the cells inside an object (its shape, a sprite's image,
    /// the standing cell of a point).
    Object(ObjectRef),
    /// `selection`: the cells selected in the editor.
    Selection,
}

/// What the shape words are, for messages.
const SHAPES: &str = "X,Y, rect, ellipse, circle, poly, path, cells, all, where GROUND, under \
                      LAYER, selection or @OBJECT";

/// An area written on its own, as `bt areas`, `bt mark` and `--highlight` take it.
pub fn parse_area(text: &str) -> Result<Area, String> {
    let words = words_of(text)?;
    let mut cursor = Cursor::new(&words);
    take_area(&mut cursor, |_, _| Ok(false))
}

/// Reads a shape.
pub(crate) fn take_shape(cursor: &mut Cursor<'_>) -> Result<Shape, String> {
    let Some(word) = cursor.peek() else {
        return Err(format!("expected where: {SHAPES}"));
    };
    if let Some(object) = word.strip_prefix('@') {
        cursor.take();
        return Ok(Shape::Object(object_ref_of(object)));
    }
    if word.contains("..") || is_cell(word) {
        let region = cursor.take_region()?;
        return Ok(if region.size() == IVec2::ONE {
            Shape::Cells(vec![region.min()])
        } else {
            Shape::Rect(region)
        });
    }
    if is_point(word) {
        return Err(format!(
            "“{word}” is a point, not a cell: shapes start at whole cells (X,Y)"
        ));
    }
    cursor.take();
    Ok(match word {
        "rect" => Shape::Rect(cursor.take_region()?),
        "all" => Shape::All,
        "ellipse" | "circle" => {
            let center = cursor.take_point("the centre X,Y")?;
            let radii = parse_radii(cursor.take_word("radii RXxRY")?)?;
            Shape::Ellipse {
                center,
                radii,
                rough: None,
            }
        }
        "poly" | "polygon" => Shape::Polygon(cursor.take_points(3, "a polygon")?),
        "path" | "line" => Shape::Path {
            points: cursor.take_points(2, "a path")?,
            width: 1.0,
        },
        "cells" => Shape::Cells(cursor.take_cells("cells")?),
        "where" => Shape::Where(named_items(&cursor.take_list("a terrain or tiles")?)?),
        "selection" => Shape::Selection,
        "under" => Shape::Under(
            cursor
                .take_list("an object layer")?
                .iter()
                .map(|layer| layer_ref_of(layer))
                .collect(),
        ),
        other => return Err(format!("“{other}” is not a shape: use {SHAPES}")),
    })
}

/// What `margin` belongs to: the `avoid` or `near` written last.
#[derive(Copy, Clone, PartialEq, Eq)]
enum MarginOf {
    Avoid,
    Near,
}

/// Reads an area: a shape and what narrows it. A word the area does not know goes to `extra`,
/// which takes it (and what follows it) and says true, or says false: then it is a mistake.
pub(crate) fn take_area(
    cursor: &mut Cursor<'_>,
    mut extra: impl FnMut(&mut Cursor<'_>, &str) -> Result<bool, String>,
) -> Result<Area, String> {
    let mut area = Area::of(take_shape(cursor)?);
    let mut after_minus = false;
    let mut margin_of: Option<MarginOf> = None;
    while let Some(word) = cursor.peek() {
        match word {
            "on" | "inside" | "avoid" | "near" => {
                cursor.take();
                let grounds = named_items(&cursor.take_list("a terrain or tiles")?)?;
                match word {
                    "on" => area.on.extend(grounds),
                    "inside" => area.inside.extend(grounds),
                    "avoid" => {
                        area.avoid.push(Nearness { grounds, margin: 0 });
                        margin_of = Some(MarginOf::Avoid);
                    }
                    _ => {
                        area.near.push(Nearness { grounds, margin: 1 });
                        margin_of = Some(MarginOf::Near);
                    }
                }
            }
            "containing" => {
                cursor.take();
                area.containing.extend(
                    cursor.take_cells("containing (the cells whose connected part is kept)")?,
                );
            }
            "minus" => {
                cursor.take();
                area.minus.push(take_shape(cursor)?);
                after_minus = true;
            }
            "rough" | "width" => {
                cursor.take();
                let shape = match area.minus.last_mut() {
                    Some(last) if after_minus => last,
                    _ => &mut area.shape,
                };
                set_shape_option(shape, word, cursor)?;
            }
            "margin" => {
                cursor.take();
                let margin = cursor.take_whole("a margin in cells (a whole number)")?;
                let margin = u32::try_from(margin).map_err(|_| "the margin is too large")?;
                let last = match margin_of {
                    Some(MarginOf::Avoid) => area.avoid.last_mut(),
                    Some(MarginOf::Near) => area.near.last_mut(),
                    None => None,
                };
                match last {
                    Some(nearness) => nearness.margin = margin,
                    None => return Err("margin belongs after avoid … or near …".to_owned()),
                }
            }
            _ => {
                if !extra(cursor, word)? {
                    return Err(format!("unexpected “{word}”"));
                }
            }
        }
    }
    Ok(area)
}

/// `rough R` or `width W` for the shape written just before.
fn set_shape_option(
    shape: &mut Shape,
    option: &str,
    cursor: &mut Cursor<'_>,
) -> Result<(), String> {
    match (option, shape) {
        ("rough", Shape::Ellipse { rough, .. }) => {
            let value = cursor.take_number("a roughness between 0 and 1")?;
            if !(0.0..=1.0).contains(&value) {
                return Err(format!("the roughness is between 0 and 1, not {value}"));
            }
            *rough = Some(value);
        }
        ("rough", _) => {
            return Err("rough belongs to an ellipse or circle written just before it".to_owned());
        }
        ("width", Shape::Path { width, .. }) => {
            let value = cursor.take_number("a width in cells")?;
            if value <= 0.0 {
                return Err(format!("the width is more than 0, not {value}"));
            }
            *width = value;
        }
        _ => return Err("width belongs to a path written just before it".to_owned()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::refs::TileIds;

    fn name(text: &str) -> Named {
        Named::Name(text.to_owned())
    }

    #[test]
    fn shapes_read_as_written() {
        let rect = IRect::from_min_size(IVec2::new(4, 2), IVec2::new(10, 6));
        assert_eq!(parse_area("4,2 10x6").unwrap(), Area::of(Shape::Rect(rect)));
        assert_eq!(
            parse_area("rect 4,2..13,7").unwrap(),
            Area::of(Shape::Rect(rect))
        );
        assert_eq!(
            parse_area("3,3").unwrap(),
            Area::of(Shape::Cells(vec![IVec2::new(3, 3)]))
        );
        assert_eq!(
            parse_area("cells 1,1 2,2").unwrap(),
            Area::of(Shape::Cells(vec![IVec2::ONE, IVec2::splat(2)]))
        );
        assert_eq!(
            parse_area("circle 22.5,26 5 rough 0.2").unwrap(),
            Area::of(Shape::Ellipse {
                center: DVec2::new(22.5, 26.0),
                radii: DVec2::splat(5.0),
                rough: Some(0.2),
            })
        );
        assert_eq!(
            parse_area("where water,terrain:3").unwrap(),
            Area::of(Shape::Where(vec![
                name("water"),
                Named::Tiles(TileIds {
                    tileset: "terrain".into(),
                    ranges: vec![crate::script::refs::IdRange { first: 3, last: 3 }],
                }),
            ]))
        );
        assert_eq!(
            parse_area("under props, #4").unwrap(),
            Area::of(Shape::Under(vec![
                LayerRef::Name("props".into()),
                LayerRef::Id(4)
            ]))
        );
        assert_eq!(
            parse_area("@#12").unwrap(),
            Area::of(Shape::Object(ObjectRef::Id(12)))
        );
        assert_eq!(parse_area("selection").unwrap(), Area::of(Shape::Selection));
    }

    #[test]
    fn options_belong_to_what_was_written_just_before() {
        let area = parse_area(
            "path 1,1 9,1 width 2 minus path 1,5 9,5 width 7 avoid ice margin 1 near water \
             containing 2,1",
        )
        .unwrap();
        assert!(matches!(area.shape, Shape::Path { width, .. } if width == 2.0));
        assert!(matches!(area.minus[0], Shape::Path { width, .. } if width == 7.0));
        assert_eq!(
            area.avoid,
            [Nearness {
                grounds: vec![name("ice")],
                margin: 1
            }]
        );
        assert_eq!(
            area.near,
            [Nearness {
                grounds: vec![name("water")],
                margin: 1
            }]
        );
        assert_eq!(area.containing, [IVec2::new(2, 1)]);
        let area = parse_area("all near mountain margin 2 avoid lake").unwrap();
        assert_eq!((area.near[0].margin, area.avoid[0].margin), (2, 0));
    }

    #[test]
    fn mistakes_say_what_was_expected() {
        assert!(
            parse_area("all margin 2")
                .unwrap_err()
                .contains("margin belongs after")
        );
        assert!(
            parse_area("all containing")
                .unwrap_err()
                .contains("needs a cell")
        );
        assert!(
            parse_area("rect 1,1 2x2 rough 0.5")
                .unwrap_err()
                .contains("rough belongs")
        );
        assert!(
            parse_area("circle 1,1 2 rough 3")
                .unwrap_err()
                .contains("between 0 and 1")
        );
        assert!(parse_area("lake").unwrap_err().contains("is not a shape"));
        assert!(parse_area("1.5,2").unwrap_err().contains("is a point"));
        assert!(
            parse_area("all count 3")
                .unwrap_err()
                .contains("unexpected “count”")
        );
        assert!(
            parse_area("ellipse 1,1")
                .unwrap_err()
                .contains("expected radii")
        );
    }
}
