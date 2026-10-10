//! The paint callback: the renderer draws the canvas inside egui's render pass. The renderer
//! is made once, with the window's target format, and kept in egui-wgpu's callback resources;
//! the image store is shared with the Tiles panel; the last frame's account is kept for the
//! status bar and the checks without a person.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use bt_image::ImageStore;
use bt_render::{FrameInput, MapRenderer, RenderStats, wgpu};
use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor};

/// The renderer, in the callback resources.
struct CanvasRenderer {
    renderer: MapRenderer,
}

/// What the canvases share: the image store and the last frame's account.
#[derive(Clone)]
pub(crate) struct CanvasGpu {
    images: Arc<Mutex<ImageStore>>,
    stats: Arc<Mutex<RenderStats>>,
}

impl CanvasGpu {
    /// Makes the renderer for the window's GPU and keeps it in the callback resources.
    pub(crate) fn install(
        render_state: &egui_wgpu::RenderState,
        images: Arc<Mutex<ImageStore>>,
    ) -> CanvasGpu {
        let renderer = MapRenderer::builder(
            &render_state.device,
            &render_state.queue,
            render_state.target_format,
        )
        .build();
        render_state
            .renderer
            .write()
            .callback_resources
            .insert(CanvasRenderer { renderer });
        CanvasGpu {
            images,
            stats: Arc::default(),
        }
    }

    /// The paint callback drawing `view` into `rect`.
    pub(crate) fn callback(
        &self,
        rect: egui::Rect,
        view: FrameInput,
        time_ms: u64,
        waker: egui::Context,
    ) -> egui::PaintCallback {
        egui_wgpu::Callback::new_paint_callback(
            rect,
            CanvasCallback {
                view,
                time_ms,
                images: Arc::clone(&self.images),
                stats: Arc::clone(&self.stats),
                waker,
            },
        )
    }

    /// The account of the last frame drawn.
    pub(crate) fn last_stats(&self) -> RenderStats {
        self.stats
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// One frame of one canvas.
struct CanvasCallback {
    view: FrameInput,
    time_ms: u64,
    images: Arc<Mutex<ImageStore>>,
    stats: Arc<Mutex<RenderStats>>,
    waker: egui::Context,
}

impl CallbackTrait for CanvasCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _screen: &ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(canvas) = resources.get_mut::<CanvasRenderer>() else {
            return Vec::new();
        };
        let prepared = {
            let mut images = self.images.lock().unwrap_or_else(PoisonError::into_inner);
            canvas
                .renderer
                .prepare(&mut *images, std::slice::from_ref(&self.view))
        };
        match prepared {
            Ok(stats) => {
                // Squares of a far view not drawn yet are drawn next frame.
                if stats.pending_impostors > 0 {
                    self.waker.request_repaint();
                }
                // An animation asks for the frame that shows its next step, and no sooner.
                if let Some(next) = stats.next_frame_ms {
                    let delay = next.saturating_sub(self.time_ms).max(1);
                    self.waker
                        .request_repaint_after(Duration::from_millis(delay));
                }
                *self.stats.lock().unwrap_or_else(PoisonError::into_inner) = stats;
            }
            Err(error) => log::error!("the map could not be drawn: {error}"),
        }
        canvas.renderer.encode(encoder);
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        if let Some(canvas) = resources.get::<CanvasRenderer>() {
            canvas.renderer.paint(pass, 0);
        }
    }
}
