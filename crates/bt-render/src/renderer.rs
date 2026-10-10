//! The renderer: created once per window or process, it holds every cache and GPU resource, and
//! draws frames in three calls: `prepare` (register, pack, animate, build the draw list,
//! upload), `encode` (the offscreen compositing of blend modes) and `paint` (one view's draws
//! into the caller's render pass).

use std::num::NonZeroU64;
use std::time::Instant;

use bytemuck::Pod;

use crate::chunks::ChunkCache;
use crate::compositor::{Compositor, OFFSCREEN, mode_index};
use crate::draw_list::{
    FrameData, Globals, LayerParams, Resources, Step, StepKind, build_view, check_layers,
};
use crate::error::RenderError;
use crate::impostors::{ImpostorWork, Impostors};
use crate::input::FrameInput;
use crate::instance::{INSTANCE_SIZE, solid};
use crate::pages::Pages;
use crate::sprites::{SPRITE_SIZE, Sprites};
use crate::stats::RenderStats;
use crate::supply::ImageSupply;
use crate::vector::VECTOR_VERTEX_SIZE;

/// One layer of an impostor: its parameters' entry and its instances.
struct LayerDraw {
    params: u32,
    instances: std::ops::Range<u32>,
}

/// The stride of dynamic uniform entries (the alignment every device accepts).
const UNIFORM_STRIDE: u64 = 256;

/// The default byte budget for texture pages and caches.
const DEFAULT_BUDGET: usize = 512 << 20;

/// The default side of a texture page.
const DEFAULT_PAGE: u32 = 4096;

/// Sets up a [`MapRenderer`].
pub struct MapRendererBuilder {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    budget_bytes: usize,
    page_side: u32,
}

impl MapRendererBuilder {
    /// The memory pages and caches may take before the least recently used go (512 MiB by
    /// default).
    pub fn budget(mut self, bytes: usize) -> MapRendererBuilder {
        self.budget_bytes = bytes;
        self
    }

    /// The side of a texture page in texels (4096 by default, at most what the device allows).
    pub fn page_side(mut self, texels: u32) -> MapRendererBuilder {
        self.page_side = texels;
        self
    }

    /// The renderer.
    pub fn build(self) -> MapRenderer {
        let gpu = Gpu::new(&self.device, self.format);
        // Texture pages take half the budget, impostors and chunk caches a quarter each.
        let pages = Pages::new(&self.device, self.page_side, self.budget_bytes / 2);
        let impostor_page_bytes = 2048 * 2048 * 4;
        let impostor_pages = (self.budget_bytes / 4 / impostor_page_bytes).max(1) as u32;
        MapRenderer {
            compositor: Compositor::new(&self.device, self.format),
            impostors: Impostors::new(&self.device, impostor_pages),
            gpu,
            pages,
            sprites: Sprites::new(),
            chunks: ChunkCache::new(self.budget_bytes / 4),
            frame: 0,
            data: FrameData::default(),
            stats: RenderStats::default(),
            device: self.device,
            queue: self.queue,
        }
    }
}

/// Draws maps: for the editor's canvas through a paint callback, for headless pictures through
/// [`crate::Render`].
pub struct MapRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    gpu: Gpu,
    pages: Pages,
    sprites: Sprites,
    chunks: ChunkCache,
    impostors: Impostors,
    compositor: Compositor,
    frame: u64,
    data: FrameData,
    stats: RenderStats,
}

impl MapRenderer {
    /// A builder for a renderer drawing into targets of `format` on `device`.
    pub fn builder(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> MapRendererBuilder {
        MapRendererBuilder {
            device: device.clone(),
            queue: queue.clone(),
            format,
            budget_bytes: DEFAULT_BUDGET,
            page_side: DEFAULT_PAGE,
        }
    }

    /// Prepares a frame of `views`: registers their tilesets, packs the images `images` has
    /// decoded, updates animations, builds the draws and uploads them. A view that names a
    /// layer its map lacks is an error and nothing is prepared.
    pub fn prepare(
        &mut self,
        images: &mut dyn ImageSupply,
        views: &[FrameInput],
    ) -> Result<RenderStats, RenderError> {
        let started = Instant::now();
        for view in views {
            check_layers(view)?;
        }
        self.frame += 1;
        let frame = self.frame;
        self.data.clear();
        self.impostors.begin_frame(started);
        for view in views {
            for tileset in &view.map.tilesets {
                self.sprites
                    .register_tileset(&tileset.tileset, images, frame);
            }
        }
        self.sprites.touch_pages(&mut self.pages, frame);
        let pending =
            self.sprites
                .take_images(images, &mut self.pages, &self.device, &self.queue, frame);
        let time_ms = views.first().map_or(0, |view| view.time_ms);
        let next_frame_ms = self.sprites.animate(time_ms, frame);
        for view in views {
            let mut resources = Resources {
                sprites: &mut self.sprites,
                pages: &mut self.pages,
                chunks: &mut self.chunks,
                impostors: &mut self.impostors,
                store: images,
                device: &self.device,
                queue: &self.queue,
                frame,
            };
            build_view(view, &mut self.data, &mut resources);
        }
        let uploaded = self.upload();
        let work = self.impostors.take_work();
        if pending > 0 {
            self.impostors.mark_incomplete(&work);
        }
        self.draw_impostors(work);
        self.impostors.forget_runs_before(frame);
        self.chunks.trim(frame);
        self.sprites.release_unused(frame);
        self.stats = RenderStats {
            instances: self.data.instances.len(),
            vector_vertices: self.data.vertices.len(),
            draws: self.data.views.iter().map(|view| view.steps.len()).sum(),
            pages: self.pages.count(),
            impostors: self.impostors.count(),
            pending_impostors: self.impostors.pending(),
            pending_images: pending,
            animating: next_frame_ms.is_some(),
            next_frame_ms,
            uploaded_bytes: uploaded,
            prepare_time: started.elapsed(),
        };
        Ok(self.stats.clone())
    }

    /// The offscreen compositing of the prepared views that have layers with blend modes.
    pub fn encode(&mut self, encoder: &mut wgpu::CommandEncoder) {
        for (index, view) in self.data.views.iter().enumerate() {
            if view.composited() {
                self.compositor
                    .ensure_targets(&self.device, index, view.viewport);
            }
        }
        for (index, view) in self.data.views.iter().enumerate() {
            if !view.composited() {
                continue;
            }
            let Some(Some(targets)) = self.compositor.targets.get(index) else {
                continue;
            };
            let pipelines = self.gpu.offscreen.as_ref().unwrap_or(&self.gpu.target);
            drop(begin_pass(encoder, &targets.accum_view, true));
            for group in &view.groups {
                let steps = view
                    .steps
                    .get(group.first_step..group.end_step)
                    .unwrap_or(&[]);
                if group.blend == bt_core::BlendMode::Normal {
                    let mut pass = begin_pass(encoder, &targets.accum_view, false);
                    self.draw_steps(&mut pass, pipelines, index, steps);
                    continue;
                }
                {
                    let mut pass = begin_pass(encoder, &targets.layer_view, true);
                    self.draw_steps(&mut pass, pipelines, index, steps);
                }
                encoder.copy_texture_to_texture(
                    targets.accum.as_image_copy(),
                    targets.backdrop.as_image_copy(),
                    wgpu::Extent3d {
                        width: targets.size.x.max(1),
                        height: targets.size.y.max(1),
                        depth_or_array_layers: 1,
                    },
                );
                let mut pass = begin_pass(encoder, &targets.accum_view, false);
                pass.set_pipeline(&self.compositor.composite);
                pass.set_bind_group(0, &targets.composite_group, &[]);
                let mode = mode_index(group.blend);
                pass.draw(0..3, mode..mode + 1);
            }
        }
    }

    /// Draws prepared view `index` into the caller's pass, whose target has the renderer's format
    /// and whose viewport is the view's.
    pub fn paint(&self, pass: &mut wgpu::RenderPass<'_>, index: usize) {
        let Some(view) = self.data.views.get(index) else {
            return;
        };
        if view.composited() {
            if let Some(Some(targets)) = self.compositor.targets.get(index) {
                pass.set_pipeline(&self.compositor.blit);
                pass.set_bind_group(0, &targets.blit_group, &[]);
                pass.draw(0..3, 0..1);
            }
            return;
        }
        self.draw_steps(pass, &self.gpu.target, index, &view.steps);
    }

    /// Waits for the images the last prepared frames asked for (headless pictures).
    pub(crate) fn wait_for_images(&mut self, images: &mut dyn ImageSupply) {
        self.sprites.wait_for_images(images);
    }

    /// Lets go of every cached chunk and tileset the last frame did not draw (a closed tab, a
    /// switch to a smaller map). Preparing also trims, but only above the budget.
    pub fn trim(&mut self) {
        self.chunks.trim_unused(self.frame);
        self.sprites.release_unused(self.frame);
    }

    /// Forgets what was read from image files that changed on disk; whatever uses them is
    /// drawn from the files again.
    pub fn forget_image_files(&mut self, files: &[std::path::PathBuf]) {
        self.sprites.forget_files(files);
    }

    /// The last prepared frame's account.
    pub fn stats(&self) -> &RenderStats {
        &self.stats
    }

    /// The largest target side the device allows.
    pub fn max_texture_side(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }

    fn draw_steps(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        pipelines: &Pipelines,
        view: usize,
        steps: &[Step],
    ) {
        let Some(group0) = &self.gpu.group0 else {
            return;
        };
        let Some(group1) = &self.gpu.group1 else {
            return;
        };
        let globals_offset = (view as u64 * UNIFORM_STRIDE) as u32;
        let mut current: Option<StepKind> = None;
        for step in steps {
            if current != Some(step.kind) {
                match step.kind {
                    StepKind::Sprites => {
                        pass.set_pipeline(&pipelines.sprite);
                        pass.set_vertex_buffer(0, self.gpu.instances.buffer.slice(..));
                    }
                    StepKind::Vectors => {
                        pass.set_pipeline(&pipelines.vector);
                        pass.set_vertex_buffer(0, self.gpu.vertices.buffer.slice(..));
                    }
                }
                pass.set_bind_group(0, group0, &[globals_offset]);
                current = Some(step.kind);
            }
            pass.set_bind_group(
                1,
                group1,
                &[(u64::from(step.params) * UNIFORM_STRIDE) as u32],
            );
            match step.kind {
                StepKind::Sprites => pass.draw(0..6, step.start..step.end),
                StepKind::Vectors => pass.draw(step.start..step.end, 0..1),
            }
        }
    }

    /// Draws the impostors planned this frame into their pages, page by page.
    /// Draws this frame's impostors into their slots: each slot cleared, then its run's layers
    /// with their parameters, clipped to the slot.
    fn draw_impostors(&mut self, work: ImpostorWork) {
        if work.jobs.is_empty() {
            return;
        }
        let mut instances = work.instances;
        let mut globals = Vec::with_capacity(work.jobs.len());
        // The clear's parameters first, then each job's layers'.
        let mut params = vec![LayerParams::plain(glam::DVec2::ZERO, 1.0)];
        let mut clears = Vec::with_capacity(work.jobs.len());
        let mut layer_draws: Vec<Vec<LayerDraw>> = Vec::with_capacity(work.jobs.len());
        for job in &work.jobs {
            globals.push(Globals {
                origin: [job.square.min.x as f32, job.square.min.y as f32],
                zoom: job.scale as f32,
                pad0: 0.0,
                viewport: [job.region.size.x as f32, job.region.size.y as f32],
                pad1: [0.0; 2],
            });
            clears.push(instances.len() as u32);
            instances.push(solid(job.square, bt_core::Color::rgba(0, 0, 0, 0)));
            let mut draws = Vec::with_capacity(job.layers.len());
            for layer in &job.layers {
                draws.push(LayerDraw {
                    params: params.len() as u32,
                    instances: layer.instances.clone(),
                });
                params.push(layer.params);
            }
            layer_draws.push(draws);
        }
        let gpu = &mut self.gpu;
        gpu.impostor_globals
            .write_strided(&self.device, &self.queue, &globals);
        gpu.impostor_params
            .write_strided(&self.device, &self.queue, &params);
        gpu.impostor_instances
            .write_all(&self.device, &self.queue, &instances);
        gpu.rebind_impostor_pass(&self.device, &self.pages);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("bt impostors"),
            });
        let pipelines = gpu.offscreen.as_ref().unwrap_or(&gpu.target);
        let mut pages: Vec<u32> = work.jobs.iter().map(|job| job.region.page).collect();
        pages.sort_unstable();
        pages.dedup();
        for page in pages {
            let Some(view) = self.impostors.layer_view(page) else {
                continue;
            };
            let Some(group0) = &gpu.impostor_group0 else {
                continue;
            };
            let Some(group1) = &gpu.impostor_group1 else {
                continue;
            };
            let mut pass = begin_pass(&mut encoder, view, false);
            pass.set_vertex_buffer(0, gpu.impostor_instances.buffer.slice(..));
            for (index, job) in work.jobs.iter().enumerate() {
                if job.region.page != page {
                    continue;
                }
                let region = job.region;
                pass.set_viewport(
                    region.origin.x as f32,
                    region.origin.y as f32,
                    region.size.x as f32,
                    region.size.y as f32,
                    0.0,
                    1.0,
                );
                pass.set_scissor_rect(
                    region.origin.x,
                    region.origin.y,
                    region.size.x,
                    region.size.y,
                );
                pass.set_bind_group(0, group0, &[(index as u64 * UNIFORM_STRIDE) as u32]);
                if let Some(clear) = clears.get(index) {
                    pass.set_pipeline(&pipelines.replace);
                    pass.set_bind_group(1, group1, &[0]);
                    pass.draw(0..6, *clear..*clear + 1);
                }
                pass.set_pipeline(&pipelines.sprite);
                for draw in layer_draws.get(index).map_or(&[][..], Vec::as_slice) {
                    pass.set_bind_group(
                        1,
                        group1,
                        &[(u64::from(draw.params) * UNIFORM_STRIDE) as u32],
                    );
                    pass.draw(0..6, draw.instances.clone());
                }
            }
        }
        self.queue.submit([encoder.finish()]);
    }

    /// Uploads the frame's data; returns the bytes written.
    fn upload(&mut self) -> usize {
        let device = &self.device;
        let queue = &self.queue;
        let gpu = &mut self.gpu;
        let mut bytes = 0;
        let globals: Vec<Globals> = self.data.views.iter().map(|view| view.globals).collect();
        bytes += gpu.globals.write_strided(device, queue, &globals);
        bytes += gpu.params.write_strided(device, queue, &self.data.params);
        bytes += gpu.instances.write_all(device, queue, &self.data.instances);
        bytes += gpu.vertices.write_all(device, queue, &self.data.vertices);
        let dirty = self.sprites.take_dirty();
        let table_grew = gpu.sprites.ensure(
            device,
            (self.sprites.table().len() as u64 * SPRITE_SIZE).max(SPRITE_SIZE),
        );
        let remap_grew = gpu
            .remap
            .ensure(device, (self.sprites.remap().len() as u64 * 4).max(4));
        let table = self.sprites.table();
        let remap = self.sprites.remap();
        if table_grew {
            bytes += gpu.sprites.write_at(queue, 0, table);
        } else if !dirty.table.is_empty() {
            let range = table
                .get(dirty.table.start as usize..dirty.table.end as usize)
                .unwrap_or(&[]);
            bytes += gpu
                .sprites
                .write_at(queue, u64::from(dirty.table.start) * SPRITE_SIZE, range);
        }
        if remap_grew {
            bytes += gpu.remap.write_at(queue, 0, remap);
        } else if !dirty.remap.is_empty() {
            let range = remap
                .get(dirty.remap.start as usize..dirty.remap.end as usize)
                .unwrap_or(&[]);
            bytes += gpu
                .remap
                .write_at(queue, u64::from(dirty.remap.start) * 4, range);
        }
        gpu.rebind(device, &self.pages, &self.impostors);
        bytes
    }
}

fn begin_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    clear: bool,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("bt offscreen"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: if clear {
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                } else {
                    wgpu::LoadOp::Load
                },
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

/// A buffer that grows to what it must hold.
struct GrowingBuffer {
    buffer: wgpu::Buffer,
    capacity: u64,
    usage: wgpu::BufferUsages,
    label: &'static str,
    generation: u64,
}

impl GrowingBuffer {
    fn new(
        device: &wgpu::Device,
        label: &'static str,
        usage: wgpu::BufferUsages,
        capacity: u64,
    ) -> GrowingBuffer {
        GrowingBuffer {
            buffer: create_buffer(device, label, usage, capacity),
            capacity,
            usage,
            label,
            generation: 0,
        }
    }

    /// Grows to at least `size` bytes; whether it was replaced.
    fn ensure(&mut self, device: &wgpu::Device, size: u64) -> bool {
        if size <= self.capacity {
            return false;
        }
        let capacity = size.next_power_of_two();
        self.buffer = create_buffer(device, self.label, self.usage, capacity);
        self.capacity = capacity;
        self.generation += 1;
        true
    }

    fn write_at<T: Pod>(&self, queue: &wgpu::Queue, offset: u64, items: &[T]) -> usize {
        let bytes: &[u8] = bytemuck::cast_slice(items);
        if !bytes.is_empty() {
            queue.write_buffer(&self.buffer, offset, bytes);
        }
        bytes.len()
    }

    fn write_all<T: Pod>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        items: &[T],
    ) -> usize {
        let size = std::mem::size_of_val(items) as u64;
        self.ensure(device, size.max(16));
        self.write_at(queue, 0, items)
    }

    /// Writes each item at a multiple of the uniform stride.
    fn write_strided<T: Pod>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        items: &[T],
    ) -> usize {
        let size = (items.len() as u64).max(1) * UNIFORM_STRIDE;
        self.ensure(device, size);
        let mut staging = vec![0u8; (items.len() as u64 * UNIFORM_STRIDE) as usize];
        for (index, item) in items.iter().enumerate() {
            let at = index * UNIFORM_STRIDE as usize;
            let bytes: &[u8] = bytemuck::bytes_of(item);
            if let Some(target) = staging.get_mut(at..at + bytes.len()) {
                target.copy_from_slice(bytes);
            }
        }
        if !staging.is_empty() {
            queue.write_buffer(&self.buffer, 0, &staging);
        }
        staging.len()
    }
}

fn create_buffer(
    device: &wgpu::Device,
    label: &str,
    usage: wgpu::BufferUsages,
    size: u64,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// The sprite and vector pipelines for one target format.
struct Pipelines {
    sprite: wgpu::RenderPipeline,
    vector: wgpu::RenderPipeline,
    /// Sprites that replace what is under them (clearing an impostor's slot).
    replace: wgpu::RenderPipeline,
}

/// What the bind groups were made from, to remake them when it changes.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
struct BoundTo {
    pages: u64,
    globals: u64,
    sprites: u64,
    remap: u64,
    params: u64,
}

/// The GPU objects.
struct Gpu {
    layout0: wgpu::BindGroupLayout,
    layout1: wgpu::BindGroupLayout,
    target: Pipelines,
    offscreen: Option<Pipelines>,
    globals: GrowingBuffer,
    params: GrowingBuffer,
    instances: GrowingBuffer,
    vertices: GrowingBuffer,
    sprites: GrowingBuffer,
    remap: GrowingBuffer,
    /// What the impostor pass binds instead of the impostor pages it draws into.
    dummy_view: wgpu::TextureView,
    group0: Option<wgpu::BindGroup>,
    group1: Option<wgpu::BindGroup>,
    bound: Option<BoundTo>,
    impostor_globals: GrowingBuffer,
    impostor_instances: GrowingBuffer,
    impostor_params: GrowingBuffer,
    impostor_group0: Option<wgpu::BindGroup>,
    impostor_group1: Option<wgpu::BindGroup>,
    impostor_bound: Option<BoundTo>,
}

impl Gpu {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Gpu {
        let uniform = |binding, visibility, size: u64| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: NonZeroU64::new(size),
            },
            count: None,
        };
        let storage = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let array_texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        };
        let layout0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bt frame"),
            entries: &[
                uniform(
                    0,
                    wgpu::ShaderStages::VERTEX_FRAGMENT,
                    std::mem::size_of::<Globals>() as u64,
                ),
                array_texture(1),
                storage(2),
                storage(3),
                array_texture(4),
            ],
        });
        let layout1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bt layer"),
            entries: &[uniform(
                0,
                wgpu::ShaderStages::VERTEX_FRAGMENT,
                std::mem::size_of::<LayerParams>() as u64,
            )],
        });
        let target = pipelines(device, &layout0, &layout1, format);
        let offscreen =
            (format != OFFSCREEN).then(|| pipelines(device, &layout0, &layout1, OFFSCREEN));
        let dummy = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bt no impostors"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OFFSCREEN,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let dummy_view = dummy.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..wgpu::TextureViewDescriptor::default()
        });
        Gpu {
            layout0,
            layout1,
            target,
            offscreen,
            globals: GrowingBuffer::new(
                device,
                "bt globals",
                wgpu::BufferUsages::UNIFORM,
                UNIFORM_STRIDE * 4,
            ),
            params: GrowingBuffer::new(
                device,
                "bt layer params",
                wgpu::BufferUsages::UNIFORM,
                UNIFORM_STRIDE * 64,
            ),
            instances: GrowingBuffer::new(
                device,
                "bt instances",
                wgpu::BufferUsages::VERTEX,
                INSTANCE_SIZE * 4096,
            ),
            vertices: GrowingBuffer::new(
                device,
                "bt vectors",
                wgpu::BufferUsages::VERTEX,
                VECTOR_VERTEX_SIZE * 4096,
            ),
            sprites: GrowingBuffer::new(
                device,
                "bt sprites",
                wgpu::BufferUsages::STORAGE,
                SPRITE_SIZE * 1024,
            ),
            remap: GrowingBuffer::new(device, "bt remap", wgpu::BufferUsages::STORAGE, 4 * 1024),
            dummy_view,
            group0: None,
            group1: None,
            bound: None,
            impostor_globals: GrowingBuffer::new(
                device,
                "bt impostor globals",
                wgpu::BufferUsages::UNIFORM,
                UNIFORM_STRIDE * 16,
            ),
            impostor_instances: GrowingBuffer::new(
                device,
                "bt impostor instances",
                wgpu::BufferUsages::VERTEX,
                INSTANCE_SIZE * 4096,
            ),
            impostor_params: GrowingBuffer::new(
                device,
                "bt impostor params",
                wgpu::BufferUsages::UNIFORM,
                UNIFORM_STRIDE * 16,
            ),
            impostor_group0: None,
            impostor_group1: None,
            impostor_bound: None,
        }
    }

    /// Remakes the bind groups whose buffers or textures changed.
    fn rebind(&mut self, device: &wgpu::Device, pages: &Pages, impostors: &Impostors) {
        let now = BoundTo {
            pages: pages.generation(),
            globals: self.globals.generation,
            sprites: self.sprites.generation,
            remap: self.remap.generation,
            params: self.params.generation,
        };
        let before = self.bound;
        let changed0 = before.is_none_or(|b| {
            b.pages != now.pages
                || b.globals != now.globals
                || b.sprites != now.sprites
                || b.remap != now.remap
        });
        if changed0 || self.group0.is_none() {
            self.group0 = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bt frame"),
                layout: &self.layout0,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &self.globals.buffer,
                            offset: 0,
                            size: NonZeroU64::new(std::mem::size_of::<Globals>() as u64),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(pages.view()),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.sprites.buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: self.remap.buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(impostors.array_view()),
                    },
                ],
            }));
        }
        if before.is_none_or(|b| b.params != now.params) || self.group1.is_none() {
            self.group1 = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bt layer"),
                layout: &self.layout1,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.params.buffer,
                        offset: 0,
                        size: NonZeroU64::new(std::mem::size_of::<LayerParams>() as u64),
                    }),
                }],
            }));
        }
        self.bound = Some(now);
    }
}

impl Gpu {
    /// Remakes the impostor pass's bind groups when their buffers or the pages changed. The pass
    /// binds a placeholder where the frame binds the impostor pages, since it draws into them.
    fn rebind_impostor_pass(&mut self, device: &wgpu::Device, pages: &Pages) {
        let now = BoundTo {
            pages: pages.generation(),
            globals: self.impostor_globals.generation,
            sprites: self.sprites.generation,
            remap: self.remap.generation,
            params: self.impostor_params.generation,
        };
        if self.impostor_bound == Some(now)
            && self.impostor_group0.is_some()
            && self.impostor_group1.is_some()
        {
            return;
        }
        self.impostor_group0 = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bt impostor frame"),
            layout: &self.layout0,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.impostor_globals.buffer,
                        offset: 0,
                        size: NonZeroU64::new(std::mem::size_of::<Globals>() as u64),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(pages.view()),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.sprites.buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.remap.buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&self.dummy_view),
                },
            ],
        }));
        self.impostor_group1 = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bt impostor layer"),
            layout: &self.layout1,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &self.impostor_params.buffer,
                    offset: 0,
                    size: NonZeroU64::new(std::mem::size_of::<LayerParams>() as u64),
                }),
            }],
        }));
        self.impostor_bound = Some(now);
    }
}

/// Premultiplied source-over.
const OVER: wgpu::BlendState = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;

fn pipelines(
    device: &wgpu::Device,
    layout0: &wgpu::BindGroupLayout,
    layout1: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> Pipelines {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("bt layout"),
        bind_group_layouts: &[Some(layout0), Some(layout1)],
        immediate_size: 0,
    });
    let sprite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("bt sprite shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shaders/sprite.wgsl").into()),
    });
    let vector_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("bt vector shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shaders/vector.wgsl").into()),
    });
    let target = [Some(wgpu::ColorTargetState {
        format,
        blend: Some(OVER),
        write_mask: wgpu::ColorWrites::ALL,
    })];
    let sprite_pipeline = |label: &str, blend: wgpu::BlendState| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &sprite_shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: INSTANCE_SIZE,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x4, 1 => Float32x4, 2 => Uint32, 3 => Uint32, 4 => Unorm8x4
                    ],
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &sprite_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        })
    };
    let sprite = sprite_pipeline("bt sprites", OVER);
    let replace = sprite_pipeline("bt sprites replacing", wgpu::BlendState::REPLACE);
    let vector = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("bt vectors"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &vector_shader,
            entry_point: Some("vs_main"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: VECTOR_VERTEX_SIZE,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![
                    0 => Float32x2, 1 => Float32x2, 2 => Unorm8x4, 3 => Float32, 4 => Float32, 5 => Float32, 6 => Float32x3
                ],
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &vector_shader,
            entry_point: Some("fs_main"),
            targets: &target,
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        multiview_mask: None,
        cache: None,
    });
    Pipelines {
        sprite,
        vector,
        replace,
    }
}
