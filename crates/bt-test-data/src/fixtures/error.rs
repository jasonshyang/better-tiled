//! What can go wrong building a fixture.

use std::path::PathBuf;

use glam::UVec2;

/// A fixture that could not be built: a mistake in the test's own input, or a file that could
/// not be written.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FixtureError {
    /// A grid token that means nothing.
    #[error(
        "grid line {line}: “{token}” is not a cell (`.`, a tile id or a rule tile letter X, I, N, E, O, with an optional `@slot` and flips h, v, d, x)"
    )]
    UnknownToken {
        /// The line, from 1.
        line: usize,
        /// The token.
        token: String,
    },
    /// A grid whose rows differ in length.
    #[error("grid line {line} has {found} cells where the first row has {expected}")]
    RaggedGrid {
        /// The line, from 1.
        line: usize,
        /// The first row's length.
        expected: usize,
        /// This row's.
        found: usize,
    },
    /// A cell outside the layer it was put in.
    #[error("cell ({x}, {y}) is outside the layer")]
    OutsideLayer {
        /// Its column.
        x: i32,
        /// Its row.
        y: i32,
    },
    /// An image that could not be encoded or written.
    #[error("cannot write the image {}: {message}", path.display())]
    Image {
        /// Where it was going.
        path: PathBuf,
        /// Why it failed.
        message: String,
    },
    /// A picture given to an atlas that is not the size its grid makes.
    #[error("the picture is {found} pixels, not the {expected} the atlas's grid makes")]
    PictureSize {
        /// The size the grid makes.
        expected: UVec2,
        /// The picture's.
        found: UVec2,
    },
}
