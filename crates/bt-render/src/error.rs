//! What stops a frame or a picture from being drawn.

use bt_core::LayerId;
use glam::UVec2;

/// Why the renderer could not draw.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    /// No GPU adapter could be found (headless renders).
    #[error("no GPU adapter: {message}")]
    NoAdapter {
        /// What the GPU library said.
        message: String,
    },
    /// The GPU device was lost or could not be created.
    #[error("the GPU device is lost: {message}")]
    DeviceLost {
        /// What the GPU library said.
        message: String,
    },
    /// A target larger than the device allows: a view composited for blend modes, or a band of
    /// a headless picture that cannot be split further.
    #[error("{}×{} is larger than the GPU allows ({limit} pixels a side)", size.x, size.y)]
    TooLarge {
        /// The size asked for.
        size: UVec2,
        /// The device's largest texture side.
        limit: u32,
    },
    /// A frame names a layer the map does not have.
    #[error("the frame names layer {0}, which the map does not have")]
    NoSuchLayer(LayerId),
}
