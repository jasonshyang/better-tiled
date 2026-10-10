//! What the last prepared frame cost and whether another is due.

use std::time::Duration;

/// The renderer's account of the last prepared frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderStats {
    /// Sprite instances drawn (tiles, tile objects, image layers, glyphs, impostors).
    pub instances: usize,
    /// Vector vertices drawn.
    pub vector_vertices: usize,
    /// Draw calls.
    pub draws: usize,
    /// Texture pages in use.
    pub pages: usize,
    /// Chunks drawn as impostors.
    pub impostors: usize,
    /// Images still being decoded: the frame lacks them and another frame will show them.
    pub pending_images: usize,
    /// Whether anything on screen animates.
    pub animating: bool,
    /// The frame time (in the frame's `time_ms`) at which an animation shows its next frame.
    pub next_frame_ms: Option<u64>,
    /// Bytes uploaded to the GPU for the frame.
    pub uploaded_bytes: usize,
    /// How long preparing took.
    pub prepare_time: Duration,
}
