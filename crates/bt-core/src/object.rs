//! Objects of object layers, and collision shapes of tiles.

use glam::DVec2;

use crate::cell::Cell;
use crate::file_ref::FileRef;
use crate::ids::ObjectId;
use crate::property::Properties;
use crate::text::TextData;

/// An object, in Tiled's pixel space.
///
/// For rectangles, ellipses, capsules and text, `pos` is the top-left of the box; for points it
/// is the point; polygon and polyline points are relative to `pos`; for tile objects `pos` is the
/// alignment origin (bottom-left by default, bottom-centre on isometric maps) and `size` is the
/// drawn size. Rotation is in degrees, clockwise, around `pos`.
#[derive(Clone, Debug, PartialEq)]
pub struct MapObject {
    /// Unique in the map (or, for a collision shape, in its tile).
    pub id: ObjectId,
    /// The name; empty for none.
    pub name: String,
    /// The class; empty for none.
    pub class: String,
    /// Where the object is.
    pub pos: DVec2,
    /// Its size.
    pub size: DVec2,
    /// Degrees, clockwise.
    pub rotation: f64,
    /// Whether it is shown.
    pub visible: bool,
    /// Opacity, 0 to 1 (Tiled 1.12).
    pub opacity: f64,
    /// Its shape.
    pub shape: Shape,
    /// The tile a tile object draws, at `size`; its shape is a rectangle.
    pub tile: Option<Cell>,
    /// The template a template instance comes from.
    pub template: Option<FileRef>,
    /// Which fields a template instance sets itself; only those are written. The instance holds
    /// the template's values for the others, so it is complete without the template at hand.
    pub overrides: ObjectOverrides,
    /// Custom properties: for a template instance, its own on top of the template's.
    pub properties: Properties,
}

impl Default for MapObject {
    fn default() -> MapObject {
        MapObject {
            id: ObjectId(0),
            name: String::new(),
            class: String::new(),
            pos: DVec2::ZERO,
            size: DVec2::ZERO,
            rotation: 0.0,
            visible: true,
            opacity: 1.0,
            shape: Shape::Rectangle,
            tile: None,
            template: None,
            overrides: ObjectOverrides::empty(),
            properties: Properties::new(),
        }
    }
}

/// The shape of an object.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// A rectangle (also the shape of tile objects).
    Rectangle,
    /// An ellipse inscribed in the box.
    Ellipse,
    /// A point.
    Point,
    /// A rectangle with fully rounded ends along its longer side (Tiled 1.12).
    Capsule,
    /// A closed polygon; points relative to the object's position.
    Polygon(Vec<DVec2>),
    /// An open polyline; points relative to the object's position.
    Polyline(Vec<DVec2>),
    /// Text in the box.
    Text(TextData),
}

bitflags::bitflags! {
    /// The fields a template instance sets itself.
    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
    pub struct ObjectOverrides: u16 {
        /// The name.
        const NAME = 1 << 0;
        /// The class.
        const CLASS = 1 << 1;
        /// The size.
        const SIZE = 1 << 2;
        /// The rotation.
        const ROTATION = 1 << 3;
        /// The opacity.
        const OPACITY = 1 << 4;
        /// The visibility.
        const VISIBLE = 1 << 5;
        /// The tile.
        const TILE = 1 << 6;
        /// The shape (and a polygon's or polyline's points).
        const SHAPE = 1 << 7;
        /// The text and how it is set.
        const TEXT = 1 << 8;
    }
}
