//! The shape language as written (`10-agents.md`, §6): a shape, then what narrows it, written in
//! scripts and on the command line (`bt areas MAP SHAPE`, `bt mark`, `bt render --highlight`).
//! These are syntax only; `crate::shapes` turns them into cells on a map, resolving the names
//! they hold.

use bt_utils::IRect;
use glam::{DVec2, IVec2};

use super::refs::{LayerRef, Named, ObjectRef};

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
