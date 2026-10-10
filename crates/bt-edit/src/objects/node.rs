//! A node of a polygon or polyline: one of its points, named by the object and its place.

use bt_core::ObjectId;

/// A node of a polygon or polyline: the object, and the place of one of its points among them,
/// from 0. A segment is named by the node it starts at: it runs to the next node, or for a
/// polygon's last node back to the first.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Node {
    /// The polygon or polyline.
    pub object: ObjectId,
    /// The place of the point among the object's points.
    pub index: usize,
}
