//! The crate's error type.

use glam::{IVec2, UVec2};

/// Why an operation on the model failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// A cell was set outside a finite tile layer.
    #[error("cell {},{} is outside the layer's {}×{} cells", position.x, position.y, size.x, size.y)]
    CellOutOfBounds {
        /// The position asked for.
        position: IVec2,
        /// The layer's size.
        size: UVec2,
    },
}
