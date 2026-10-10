//! Blend modes. Only while a visible layer has one: the view is drawn into an offscreen `accum`
//! target; runs of normal layers go straight in; a blended layer is drawn alone into a `layer`
//! target, `accum` is copied to `backdrop`, and a full-screen pass combines the two with the
//! mode's formula, writing back into `accum`. Painting the view then blits `accum`.

use bt_core::BlendMode;
use glam::UVec2;

/// The format of the offscreen targets.
pub(crate) const OFFSCREEN: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The shader's number for a blend mode (`blend.wgsl`).
pub(crate) fn mode_index(mode: BlendMode) -> u32 {
    match mode {
        BlendMode::Normal => 0,
        BlendMode::Add => 1,
        BlendMode::Multiply => 2,
        BlendMode::Screen => 3,
        BlendMode::Overlay => 4,
        BlendMode::Darken => 5,
        BlendMode::Lighten => 6,
        BlendMode::ColorDodge => 7,
        BlendMode::ColorBurn => 8,
        BlendMode::HardLight => 9,
        BlendMode::SoftLight => 10,
        BlendMode::Difference => 11,
        BlendMode::Exclusion => 12,
    }
}

/// The offscreen images of one composited view.
pub(crate) struct Targets {
    pub(crate) size: UVec2,
    pub(crate) accum: wgpu::Texture,
    pub(crate) accum_view: wgpu::TextureView,
    pub(crate) backdrop: wgpu::Texture,
    pub(crate) layer_view: wgpu::TextureView,
    pub(crate) composite_group: wgpu::BindGroup,
    pub(crate) blit_group: wgpu::BindGroup,
}

/// The pipelines and per-view targets.
pub(crate) struct Compositor {
    composite_layout: wgpu::BindGroupLayout,
    blit_layout: wgpu::BindGroupLayout,
    pub(crate) composite: wgpu::RenderPipeline,
    pub(crate) blit: wgpu::RenderPipeline,
    pub(crate) targets: Vec<Option<Targets>>,
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

impl Compositor {
    /// The pipelines, blitting into `target_format`.
    pub(crate) fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Compositor {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bt blend shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/blend.wgsl").into()),
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bt composite"),
            entries: &[texture_entry(0), texture_entry(1)],
        });
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bt blit"),
            entries: &[texture_entry(0)],
        });
        let pipeline = |label: &str,
                        layout: &wgpu::BindGroupLayout,
                        entry: &str,
                        format: wgpu::TextureFormat,
                        blend: Option<wgpu::BlendState>| {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_full"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        // The composite replaces `accum` (it includes the backdrop); the blit draws the
        // premultiplied result over the view.
        let composite = pipeline(
            "bt composite",
            &composite_layout,
            "fs_composite",
            OFFSCREEN,
            None,
        );
        let blit = pipeline(
            "bt blit",
            &blit_layout,
            "fs_blit",
            target_format,
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        );
        Compositor {
            composite_layout,
            blit_layout,
            composite,
            blit,
            targets: Vec::new(),
        }
    }

    /// Makes (or remakes, for a new size) the targets of view `index`.
    pub(crate) fn ensure_targets(&mut self, device: &wgpu::Device, index: usize, size: UVec2) {
        while self.targets.len() <= index {
            self.targets.push(None);
        }
        let stale = self
            .targets
            .get(index)
            .is_none_or(|slot| slot.as_ref().is_none_or(|t| t.size != size));
        if stale {
            let made = self.make_targets(device, size);
            if let Some(slot) = self.targets.get_mut(index) {
                *slot = Some(made);
            }
        }
    }

    fn make_targets(&self, device: &wgpu::Device, size: UVec2) -> Targets {
        let texture = |label| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size.x.max(1),
                    height: size.y.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: OFFSCREEN,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let accum = texture("bt accum");
        let backdrop = texture("bt backdrop");
        let layer = texture("bt layer");
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let accum_view = view(&accum);
        let backdrop_view = view(&backdrop);
        let layer_view = view(&layer);
        let composite_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bt composite"),
            layout: &self.composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&backdrop_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&layer_view),
                },
            ],
        });
        let blit_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bt blit"),
            layout: &self.blit_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&accum_view),
            }],
        });
        Targets {
            size,
            accum,
            accum_view,
            backdrop,
            layer_view,
            composite_group,
            blit_group,
        }
    }
}
