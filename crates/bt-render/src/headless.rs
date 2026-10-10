//! Pictures without a window. `bt render`, Export as Image, thumbnails and the tests all go
//! through [`Render`]. A picture covers the map's bounding rectangle, as Tiled's Export as Image
//! does, on every orientation and for infinite maps; one larger than the device allows is drawn
//! in bands and stitched, never refused. The result is straight-alpha RGBA8.

use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use bt_core::{Color, LayerId, Map};
use bt_image::ImageStore;
use glam::{DVec2, UVec2};

use crate::compositor::OFFSCREEN;
use crate::error::RenderError;
use crate::input::{Camera, FrameInput, LayerVisibility, ObjectStyle, PreviewTile};
use crate::renderer::MapRenderer;
use crate::supply::ImageSupply;
use crate::vector::VectorItem;

/// A GPU device for pictures, with the renderer and image store they share.
pub struct HeadlessDevice {
    device: wgpu::Device,
    queue: wgpu::Queue,
    shared: Mutex<Shared>,
}

/// What one picture at a time uses.
struct Shared {
    renderer: MapRenderer,
    images: ImageStore,
}

impl HeadlessDevice {
    /// The process's device, made on first use and shared by every caller.
    pub fn acquire() -> Result<&'static HeadlessDevice, RenderError> {
        static DEVICE: OnceLock<Result<HeadlessDevice, RenderError>> = OnceLock::new();
        DEVICE
            .get_or_init(HeadlessDevice::new)
            .as_ref()
            .map_err(Clone::clone)
    }

    /// A device of one's own (for drawing a map as an image while another picture holds the
    /// shared one).
    pub fn new() -> Result<HeadlessDevice, RenderError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: std::env::var_os("BT_SOFTWARE_GPU").is_some(),
            ..wgpu::RequestAdapterOptions::default()
        }))
        .map_err(|error| RenderError::NoAdapter {
            message: error.to_string(),
        })?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("bt headless"),
            required_limits: adapter.limits(),
            ..wgpu::DeviceDescriptor::default()
        }))
        .map_err(|error| RenderError::DeviceLost {
            message: error.to_string(),
        })?;
        let images = ImageStore::builder()
            .build()
            .map_err(|error| RenderError::DeviceLost {
                message: format!("the image workers did not start: {error}"),
            })?;
        let renderer = MapRenderer::builder(&device, &queue, OFFSCREEN).build();
        Ok(HeadlessDevice {
            shared: Mutex::new(Shared { renderer, images }),
            device,
            queue,
        })
    }

    /// The GPU device.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Its queue.
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
}

/// A picture: straight-alpha RGBA8, row by row, and where it is in the world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    /// Its width in pixels.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
    /// Four bytes a pixel.
    pub rgba: Vec<u8>,
}

impl Picture {
    /// The pixel at `x`, `y`, if inside.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let at = ((y * self.width + x) * 4) as usize;
        self.rgba
            .get(at..at + 4)
            .and_then(|p| <[u8; 4]>::try_from(p).ok())
    }
}

/// Sets up a picture of a map.
#[derive(Clone, Debug)]
pub struct Render {
    map: Arc<Map>,
    scale: f64,
    time_ms: u64,
    background: Option<Color>,
    vectors: Arc<[VectorItem]>,
    objects: ObjectStyle,
    layers: LayerVisibility,
    previews: Option<Previews>,
    parallax: bool,
    view: Option<View>,
    band_side: Option<u32>,
}

/// Preview tiles and the layer they are drawn above.
#[derive(Clone, Debug)]
struct Previews {
    layer: LayerId,
    tiles: Arc<[PreviewTile]>,
}

/// A picture of part of the world rather than the whole map.
#[derive(Copy, Clone, Debug)]
struct View {
    origin: DVec2,
    size: UVec2,
}

impl Render {
    /// A picture of `map` at scale 1, frame time 0, on a transparent background.
    pub fn of(map: Arc<Map>) -> Render {
        Render {
            map,
            scale: 1.0,
            time_ms: 0,
            background: None,
            vectors: Arc::from(Vec::new()),
            objects: ObjectStyle::default(),
            layers: LayerVisibility::default(),
            previews: None,
            parallax: false,
            view: None,
            band_side: None,
        }
    }

    /// A picture of `size` pixels with the world point `origin` at its top-left, rather than of
    /// the map's bounding rectangle.
    pub fn view(mut self, origin: DVec2, size: UVec2) -> Render {
        self.view = Some(View {
            origin,
            size: size.max(UVec2::ONE),
        });
        self
    }

    /// The view switches over the layers (highlight, layers hidden in the view).
    pub fn layers(mut self, layers: LayerVisibility) -> Render {
        self.layers = layers;
        self
    }

    /// Preview tiles drawn above `layer`.
    pub fn previews(mut self, layer: LayerId, tiles: Arc<[PreviewTile]>) -> Render {
        self.previews = Some(Previews { layer, tiles });
        self
    }

    /// Whether parallax factors move layers with the view (off by default, as in Tiled's Export
    /// as Image).
    pub fn parallax(mut self, on: bool) -> Render {
        self.parallax = on;
        self
    }

    /// Picture pixels per world pixel.
    pub fn scale(mut self, scale: f64) -> Render {
        self.scale = scale;
        self
    }

    /// The time animations are shown at.
    pub fn time_ms(mut self, time_ms: u64) -> Render {
        self.time_ms = time_ms;
        self
    }

    /// The colour behind the map.
    pub fn background(mut self, color: Color) -> Render {
        self.background = Some(color);
        self
    }

    /// Vector items drawn with the map.
    pub fn vectors(mut self, items: Arc<[VectorItem]>) -> Render {
        self.vectors = items;
        self
    }

    /// How shape objects are drawn.
    pub fn objects(mut self, style: ObjectStyle) -> Render {
        self.objects = style;
        self
    }

    /// Draws in bands at most this many pixels a side (the device's limit otherwise).
    pub fn band_side(mut self, side: u32) -> Render {
        self.band_side = Some(side.max(1));
        self
    }

    /// The picture's size.
    pub fn size(&self) -> UVec2 {
        if let Some(view) = self.view {
            return view.size;
        }
        let bounds = self.map.bounding_rect();
        (DVec2::new(f64::from(bounds.w), f64::from(bounds.h)) * self.scale)
            .round()
            .max(DVec2::ONE)
            .as_uvec2()
    }

    /// The world point at the picture's top-left: pixel `p` shows `origin + p / scale`.
    pub fn origin(&self) -> DVec2 {
        if let Some(view) = self.view {
            return view.origin;
        }
        let scale = self.scale.max(1e-9);
        (self.map.bounding_rect().min().as_dvec2() * scale).round() / scale
    }

    /// Draws the picture with the shared device.
    pub fn run(self, device: &HeadlessDevice) -> Result<Picture, RenderError> {
        let mut shared = device.shared.lock().unwrap_or_else(PoisonError::into_inner);
        let Shared { renderer, images } = &mut *shared;
        self.run_with(&device.device, &device.queue, renderer, images)
    }

    /// Draws the picture with a renderer and images of the caller's (a map drawn as an image
    /// uses the images decoded for it).
    pub fn run_with(
        self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &mut MapRenderer,
        images: &mut dyn ImageSupply,
    ) -> Result<Picture, RenderError> {
        let size = self.size();
        let origin = self.origin();
        let limit = self
            .band_side
            .unwrap_or(u32::MAX)
            .min(renderer.max_texture_side())
            .max(1);
        let mut picture = Picture {
            width: size.x,
            height: size.y,
            rgba: vec![0; (size.x as usize) * (size.y as usize) * 4],
        };
        let mut y = 0;
        while y < size.y {
            let band_height = limit.min(size.y - y);
            let mut x = 0;
            while x < size.x {
                let band_width = limit.min(size.x - x);
                let band = UVec2::new(band_width, band_height);
                let camera = Camera {
                    origin: origin + DVec2::new(f64::from(x), f64::from(y)) / self.scale.max(1e-9),
                    zoom: self.scale,
                };
                let pixels = self.band(device, queue, renderer, images, camera, band)?;
                for row in 0..band_height {
                    let from = (row * band_width * 4) as usize;
                    let to = (((y + row) * size.x + x) * 4) as usize;
                    let length = (band_width * 4) as usize;
                    let Some(source) = pixels.get(from..from + length) else {
                        continue;
                    };
                    if let Some(target) = picture.rgba.get_mut(to..to + length) {
                        target.copy_from_slice(source);
                    }
                }
                x += band_width;
            }
            y += band_height;
        }
        Ok(picture)
    }

    /// One band, read back as straight alpha.
    fn band(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &mut MapRenderer,
        images: &mut dyn ImageSupply,
        camera: Camera,
        size: UVec2,
    ) -> Result<Vec<u8>, RenderError> {
        let mut input = FrameInput::new(Arc::clone(&self.map), camera, size)
            .time_ms(self.time_ms)
            .vectors(Arc::clone(&self.vectors))
            .objects(self.objects.clone())
            .layers(self.layers.clone())
            .parallax(self.parallax)
            .background(self.background);
        if let Some(previews) = &self.previews {
            input = input.previews(previews.layer, Arc::clone(&previews.tiles));
        }
        // Everything the frame needs is asked for, waited for, then drawn; far out, the squares
        // of tile layers are drawn a frame's budget at a time until none is left.
        renderer.prepare(images, std::slice::from_ref(&input))?;
        renderer.wait_for_images(images);
        let mut stats = renderer.prepare(images, std::slice::from_ref(&input))?;
        if stats.pending_images > 0 {
            renderer.wait_for_images(images);
            stats = renderer.prepare(images, std::slice::from_ref(&input))?;
        }
        while stats.pending_impostors > 0 {
            stats = renderer.prepare(images, std::slice::from_ref(&input))?;
        }
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bt picture"),
            size: wgpu::Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OFFSCREEN,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let bytes_per_row = (4 * size.x).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bt picture readback"),
            size: u64::from(bytes_per_row) * u64::from(size.y),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("bt picture"),
        });
        renderer.encode(&mut encoder);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bt picture"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            renderer.paint(&mut pass, 0);
        }
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(size.y),
                },
            },
            wgpu::Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        readback.map_async(wgpu::MapMode::Read, .., move |result| {
            let _ = sender.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| RenderError::DeviceLost {
                message: error.to_string(),
            })?;
        receiver
            .recv()
            .map_err(|error| RenderError::DeviceLost {
                message: error.to_string(),
            })?
            .map_err(|error| RenderError::DeviceLost {
                message: error.to_string(),
            })?;
        let data = readback
            .get_mapped_range(..)
            .map_err(|error| RenderError::DeviceLost {
                message: error.to_string(),
            })?;
        let mut out = Vec::with_capacity((size.x * size.y * 4) as usize);
        for row in data.chunks(bytes_per_row as usize).take(size.y as usize) {
            for texel in row
                .get(..(size.x * 4) as usize)
                .unwrap_or(&[])
                .chunks_exact(4)
            {
                out.extend_from_slice(&unpremultiplied(texel));
            }
        }
        Ok(out)
    }
}

/// A premultiplied texel as straight alpha.
fn unpremultiplied(texel: &[u8]) -> [u8; 4] {
    let [r, g, b, a] = <[u8; 4]>::try_from(texel).unwrap_or([0; 4]);
    if a == 0 {
        return [0; 4];
    }
    let un = |v: u8| {
        u8::try_from((u32::from(v) * 255 + u32::from(a) / 2) / u32::from(a)).unwrap_or(u8::MAX)
    };
    [un(r), un(g), un(b), a]
}
