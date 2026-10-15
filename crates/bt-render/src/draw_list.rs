//! The layer walk that turns a view into draws: depth-first and bottom up, with Tiled's offsets
//! (parallax included), opacity multiplied down the tree, tints, the Highlight Current Layer
//! fade and dimming, tile layers from the chunk caches, objects in their draw order, image
//! layers repeated, previews above their layer, the vector items that belong to each layer, and
//! layers with a blend mode set apart for compositing.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use bt_core::{
    BlendMode, Color, DrawOrder, EffectiveLayer, ImageLayer, Layer, LayerId, LayerKind, Map,
    MapGeometry, ObjectLayer, Shape, TileLayer,
};
use bt_utils::DRect;
use glam::{DVec2, UVec2};

use crate::chunks::{ChunkCache, ChunkContext, tiles_for_view};
use crate::error::RenderError;
use crate::impostors::{IMPOSTOR_CELL_PX, Impostors, RunLayer, RunSources, RunView};
use crate::input::FrameInput;
use crate::instance::{Instance, cell_instance, marker, object_instance, plain, solid};
use crate::objects::{object_color, shape_vectors};
use crate::pages::Pages;
use crate::sprites::{ImageLayerSprites, Sprites};
use crate::supply::ImageSupply;
use crate::text::{TextItem, lay_out};
use crate::vector::{Placement, VectorItem, VectorVertex, tessellate};

/// The opacity of layers above the highlighted ones (Tiled's `opacityFactor`).
const DIMMED: f64 = 0.4;
/// The opacity of the fade under the highlighted layers (Tiled's `darkeningFactor`).
const FADE: f64 = 0.6;
/// The most copies of a repeated image layer drawn along an axis.
const MAX_REPEATS: i64 = 1024;
/// Cells smaller than this on screen (device pixels) are drawn through impostors when a view
/// shows more than [`FAR_CELLS`] of them; smaller than [`IMPOSTOR_CELL_PX`], always.
const FAR_CELL_PX: f64 = 16.0;
/// The cells (summed over tile layers) above which a view of small cells uses impostors.
const FAR_CELLS: u64 = 100_000;

/// A view's camera as the shaders read it.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Globals {
    pub(crate) origin: [f32; 2],
    pub(crate) zoom: f32,
    pub(crate) pad0: f32,
    pub(crate) viewport: [f32; 2],
    pub(crate) pad1: [f32; 2],
}

/// What applies to a run of draws: an offset, an opacity and a tint.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LayerParams {
    pub(crate) offset: [f32; 2],
    pub(crate) opacity: f32,
    pub(crate) tinted: u32,
    pub(crate) tint: [f32; 4],
}

impl LayerParams {
    pub(crate) fn plain(offset: DVec2, opacity: f64) -> LayerParams {
        LayerParams {
            offset: [offset.x as f32, offset.y as f32],
            opacity: opacity as f32,
            tinted: 0,
            tint: [1.0; 4],
        }
    }

    pub(crate) fn tinted(offset: DVec2, opacity: f64, tint: Option<Color>) -> LayerParams {
        let mut params = LayerParams::plain(offset, opacity);
        if let Some(tint) = tint.filter(|t| *t != Color::WHITE) {
            params.tinted = 1;
            params.tint = tint.to_array().map(|c| f32::from(c) / 255.0);
        }
        params
    }
}

/// Sprites or vectors.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum StepKind {
    Sprites,
    Vectors,
}

/// A run of instances or vertices drawn with one set of parameters.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Step {
    pub(crate) kind: StepKind,
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) params: u32,
}

/// Steps composited together: the normal layers between blended ones, or one blended layer.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Group {
    pub(crate) blend: BlendMode,
    pub(crate) first_step: usize,
    pub(crate) end_step: usize,
}

/// The draws of one view.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ViewDraws {
    pub(crate) globals: Globals,
    pub(crate) viewport: UVec2,
    pub(crate) steps: Vec<Step>,
    pub(crate) groups: Vec<Group>,
}

impl ViewDraws {
    /// Whether a blend mode needs the view composited offscreen.
    pub(crate) fn composited(&self) -> bool {
        self.groups
            .iter()
            .any(|group| group.blend != BlendMode::Normal)
    }
}

/// A frame's instances, vertices, parameters and views.
#[derive(Clone, Debug, Default)]
pub(crate) struct FrameData {
    pub(crate) instances: Vec<Instance>,
    pub(crate) vertices: Vec<VectorVertex>,
    pub(crate) params: Vec<LayerParams>,
    pub(crate) views: Vec<ViewDraws>,
}

impl FrameData {
    pub(crate) fn clear(&mut self) {
        self.instances.clear();
        self.vertices.clear();
        self.params.clear();
        self.views.clear();
    }
}

/// What the walk draws with.
pub(crate) struct Resources<'a> {
    pub(crate) sprites: &'a mut Sprites,
    pub(crate) pages: &'a mut Pages,
    pub(crate) chunks: &'a mut ChunkCache,
    pub(crate) impostors: &'a mut Impostors,
    pub(crate) store: &'a mut dyn ImageSupply,
    pub(crate) device: &'a wgpu::Device,
    pub(crate) queue: &'a wgpu::Queue,
    pub(crate) frame: u64,
}

/// The vector items of a view by where they go.
struct PlacedVectors<'a> {
    on: HashMap<LayerId, Vec<&'a VectorItem>>,
    above: HashMap<LayerId, Vec<&'a VectorItem>>,
    top: Vec<&'a VectorItem>,
}

/// Where the walk is relative to the highlighted layers.
struct Highlight<'a> {
    selected: &'a [LayerId],
    lowest: Option<LayerId>,
    seen: bool,
}

/// Checks that everything the frame names exists in its map.
pub(crate) fn check_layers(input: &FrameInput) -> Result<(), RenderError> {
    let map = &input.map;
    let named = input
        .preview_layer
        .into_iter()
        .chain(input.layers.highlighted().iter().copied())
        .chain(
            input
                .vectors
                .iter()
                .filter_map(|item| match item.placement() {
                    Placement::AboveAll => None,
                    Placement::AboveLayer(id) | Placement::OnLayer(id) => Some(id),
                }),
        );
    for id in named {
        if map.layer(id).is_none() {
            return Err(RenderError::NoSuchLayer(id));
        }
    }
    Ok(())
}

/// The cells of the visible tile layers a view of `view_rect` may show, summed over the layers
/// (an estimate of the instances drawing them would take).
fn visible_cells(map: &Map, geometry: &MapGeometry, view_rect: DRect) -> u64 {
    let mut cells = 0u64;
    map.for_each_layer(|layer, _| {
        let LayerKind::Tile(tiles) = &layer.kind else {
            return;
        };
        let Some(effective) = map.effective_layer(layer.id) else {
            return;
        };
        let Some(used) = tiles.chunk_bounds() else {
            return;
        };
        if !effective.visible {
            return;
        }
        let shown = tiles_for_view(geometry, map, view_rect, effective.offset).intersection(used);
        if !shown.is_empty() {
            cells += u64::from(shown.w.unsigned_abs()) * u64::from(shown.h.unsigned_abs());
        }
    });
    cells
}

/// A hash of the sprites the map's tilesets have, for the chunk caches.
fn tileset_signature(map: &Map, sprites: &Sprites) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for tileset in &map.tilesets {
        sprites.serial_of(&tileset.tileset).hash(&mut hasher);
    }
    hasher.finish()
}

/// Builds the draws of one view.
pub(crate) fn build_view(input: &FrameInput, data: &mut FrameData, res: &mut Resources<'_>) {
    let map = input.map.as_ref();
    let geometry = MapGeometry::from_map(map);
    let camera = input.camera;
    let view_rect = camera.visible_rect(input.viewport_px);
    let mut vectors = PlacedVectors {
        on: HashMap::new(),
        above: HashMap::new(),
        top: Vec::new(),
    };
    for item in input.vectors.iter() {
        match item.placement() {
            Placement::AboveAll => vectors.top.push(item),
            Placement::AboveLayer(id) => vectors.above.entry(id).or_default().push(item),
            Placement::OnLayer(id) => vectors.on.entry(id).or_default().push(item),
        }
    }
    let selected = input.layers.highlighted();
    let mut highlight = Highlight {
        selected,
        lowest: None,
        seen: false,
    };
    if !selected.is_empty() {
        map.for_each_layer(|layer, _| {
            if highlight.lowest.is_none() && selected.contains(&layer.id) {
                highlight.lowest = Some(layer.id);
            }
        });
    }
    let tile_px = f64::from(map.tile_width.max(map.tile_height));
    let cell_px = tile_px * camera.zoom;
    let far = cell_px < IMPOSTOR_CELL_PX
        || (cell_px < FAR_CELL_PX && visible_cells(map, &geometry, view_rect) > FAR_CELLS);
    let mut walk = Walk {
        input,
        map,
        geometry,
        view_rect,
        far,
        run: Vec::new(),
        run_shift: DVec2::ZERO,
        tilesets: tileset_signature(map, res.sprites),
        data,
        res,
        draws: ViewDraws {
            globals: Globals {
                origin: [camera.origin.x as f32, camera.origin.y as f32],
                zoom: camera.zoom as f32,
                pad0: 0.0,
                viewport: [input.viewport_px.x as f32, input.viewport_px.y as f32],
                pad1: [0.0; 2],
            },
            viewport: input.viewport_px,
            steps: Vec::new(),
            groups: Vec::new(),
        },
        group_blend: BlendMode::Normal,
        group_start: 0,
        vectors,
        highlight,
    };
    if let Some(background) = input.background {
        let params = walk.params(LayerParams::plain(DVec2::ZERO, 1.0));
        walk.push_instances(
            params,
            &[solid(
                input.background_bounds.unwrap_or(view_rect),
                background,
            )],
        );
    }
    walk.layers(&map.layers, EffectiveLayer::ROOT, false);
    let top: Vec<&VectorItem> = walk.vectors.top.clone();
    let neutral = walk.params(LayerParams::plain(DVec2::ZERO, 1.0));
    walk.vector_items(&top, neutral);
    walk.close_group();
    let draws = walk.draws;
    walk.data.views.push(draws);
}

/// The state of one view's walk.
struct Walk<'w, 'r, 'a> {
    input: &'w FrameInput,
    map: &'w Map,
    geometry: MapGeometry,
    view_rect: DRect,
    /// Tile layers are drawn through impostors.
    far: bool,
    /// Tile layers waiting to be drawn as one run of impostors.
    run: Vec<RunLayer<'w>>,
    /// How far parallax moves the waiting run.
    run_shift: DVec2,
    tilesets: u64,
    data: &'w mut FrameData,
    res: &'w mut Resources<'r>,
    draws: ViewDraws,
    group_blend: BlendMode,
    group_start: usize,
    vectors: PlacedVectors<'a>,
    highlight: Highlight<'a>,
}

impl<'w, 'r, 'a> Walk<'w, 'r, 'a>
where
    'w: 'a,
{
    fn params(&mut self, params: LayerParams) -> u32 {
        let index = self.data.params.len() as u32;
        self.data.params.push(params);
        index
    }

    fn push_step(&mut self, kind: StepKind, start: u32, end: u32, params: u32) {
        // Whatever is drawn next goes above the run of tile layers waiting to be drawn.
        self.flush_run();
        if start == end {
            return;
        }
        let in_group = self.draws.steps.len() > self.group_start;
        if let Some(last) = self.draws.steps.last_mut()
            && in_group
            && last.kind == kind
            && last.params == params
            && last.end == start
        {
            last.end = end;
            return;
        }
        self.draws.steps.push(Step {
            kind,
            start,
            end,
            params,
        });
    }

    fn push_instances(&mut self, params: u32, instances: &[Instance]) {
        let start = self.data.instances.len() as u32;
        self.data.instances.extend_from_slice(instances);
        let end = self.data.instances.len() as u32;
        self.push_step(StepKind::Sprites, start, end, params);
    }

    /// Ends the current group and starts one with `blend`.
    fn begin_group(&mut self, blend: BlendMode) {
        if blend == self.group_blend && blend == BlendMode::Normal {
            return;
        }
        self.close_group();
        self.group_blend = blend;
    }

    fn close_group(&mut self) {
        self.flush_run();
        let end = self.draws.steps.len();
        if end > self.group_start {
            self.draws.groups.push(Group {
                blend: self.group_blend,
                first_step: self.group_start,
                end_step: end,
            });
        }
        self.group_start = end;
        self.group_blend = BlendMode::Normal;
    }

    fn parallax_offset(&self, effective: &EffectiveLayer) -> DVec2 {
        if !self.input.parallax {
            return effective.offset;
        }
        let center = (self.view_rect.min + self.view_rect.max) * 0.5;
        effective.offset + (DVec2::ONE - effective.parallax) * (center - self.map.parallax_origin)
    }

    fn layers(&mut self, layers: &'w [Layer], parent: EffectiveLayer, inside_selected: bool) {
        for layer in layers {
            if self.input.layers.is_hidden(layer.id) {
                continue;
            }
            let effective = parent.combined_with(layer);
            let selected = inside_selected || self.highlight.selected.contains(&layer.id);
            if self.highlight.lowest == Some(layer.id) {
                self.fade(&parent);
                self.highlight.seen = true;
            }
            if !effective.visible {
                continue;
            }
            let dim =
                if self.highlight.seen && !selected && !matches!(layer.kind, LayerKind::Group(_)) {
                    DIMMED
                } else {
                    1.0
                };
            let offset = self.parallax_offset(&effective);
            let opacity = effective.opacity * dim;
            match &layer.kind {
                LayerKind::Group(children) => {
                    self.layers(children, effective, selected);
                }
                LayerKind::Tile(tiles) if self.far => {
                    self.add_to_run(layer, tiles, &effective, opacity);
                }
                LayerKind::Tile(tiles) => {
                    self.begin_group(layer.blend_mode);
                    let params = self.params(LayerParams::tinted(offset, opacity, effective.tint));
                    self.tile_layer(tiles, offset, params);
                }
                LayerKind::Object(objects) => {
                    self.begin_group(layer.blend_mode);
                    self.objects(layer.id, objects, offset, opacity, effective.tint);
                }
                LayerKind::Image(image) => {
                    self.begin_group(layer.blend_mode);
                    let params = self.params(LayerParams::tinted(offset, opacity, effective.tint));
                    self.image_layer(image, offset, params);
                }
            }
            if self.input.preview_layer == Some(layer.id) {
                let params = self.params(LayerParams::tinted(offset, opacity, effective.tint));
                self.previews(params);
            }
            if let Some(items) = self.vectors.on.get(&layer.id).cloned() {
                let params = self.params(LayerParams::plain(offset, opacity));
                self.vector_items(&items, params);
            }
            if !matches!(layer.kind, LayerKind::Group(_)) && layer.blend_mode != BlendMode::Normal {
                self.close_group();
            }
            if let Some(items) = self.vectors.above.get(&layer.id).cloned() {
                let params = self.params(LayerParams::plain(DVec2::ZERO, 1.0));
                self.vector_items(&items, params);
            }
        }
    }

    /// A tile layer's cells from the chunk caches.
    fn tile_layer(&mut self, tiles: &TileLayer, offset: DVec2, params: u32) {
        let tiles_rect = tiles_for_view(&self.geometry, self.map, self.view_rect, offset);
        let cx = ChunkContext {
            map: self.map,
            geometry: self.geometry,
            sprites: self.res.sprites,
            tilesets: self.tilesets,
        };
        let start = self.data.instances.len() as u32;
        self.res.chunks.collect(
            tiles,
            tiles_rect,
            &cx,
            self.res.frame,
            &mut self.data.instances,
        );
        let end = self.data.instances.len() as u32;
        self.push_step(StepKind::Sprites, start, end, params);
    }

    /// Adds a tile layer to the run of impostors waiting to be drawn, drawing the waiting run
    /// first when the layer cannot join it (a blend mode, another parallax).
    fn add_to_run(
        &mut self,
        layer: &'w Layer,
        tiles: &'w TileLayer,
        effective: &EffectiveLayer,
        opacity: f64,
    ) {
        let shift = self.parallax_offset(effective) - effective.offset;
        if !self.run.is_empty()
            && (shift != self.run_shift || layer.blend_mode != BlendMode::Normal)
        {
            self.flush_run();
        }
        self.begin_group(layer.blend_mode);
        self.run.push(RunLayer {
            id: layer.id,
            tiles,
            params: LayerParams::tinted(effective.offset, opacity, effective.tint),
        });
        self.run_shift = shift;
    }

    /// Draws the waiting run of tile layers from its impostors.
    fn flush_run(&mut self) {
        if self.run.is_empty() {
            return;
        }
        let layers = std::mem::take(&mut self.run);
        let shift = self.run_shift;
        let cx = ChunkContext {
            map: self.map,
            geometry: self.geometry,
            sprites: self.res.sprites,
            tilesets: self.tilesets,
        };
        let placed = self.res.impostors.draw_run(
            &RunView {
                layers: &layers,
                view_rect: self.view_rect.translated(-shift),
                zoom_px: self.input.camera.zoom,
            },
            &mut RunSources {
                chunks: self.res.chunks,
                cx: &cx,
                frame: self.res.frame,
            },
        );
        let instances = self.res.impostors.quads(&placed, self.res.sprites);
        let params = self.params(LayerParams::plain(shift, 1.0));
        self.push_instances(params, &instances);
    }

    /// Tiled's darkening rectangle under the lowest highlighted layer.
    fn fade(&mut self, parent: &EffectiveLayer) {
        let color = self.input.layers.fade();
        let params = self.params(LayerParams::plain(DVec2::ZERO, parent.opacity * FADE));
        let rect = self.view_rect.expanded(DVec2::ONE);
        self.push_instances(
            params,
            &[solid(rect, Color::rgb(color.r, color.g, color.b))],
        );
    }

    fn objects(
        &mut self,
        layer_id: LayerId,
        layer: &'w ObjectLayer,
        offset: DVec2,
        opacity: f64,
        tint: Option<Color>,
    ) {
        let sprites_params = self.params(LayerParams::tinted(offset, opacity, tint));
        let plain_params = self.params(LayerParams::plain(offset, opacity));
        let mut order: Vec<&bt_core::MapObject> =
            layer.objects.iter().filter(|o| o.visible).collect();
        if layer.draw_order == DrawOrder::TopDown {
            let geometry = self.geometry;
            order.sort_by(|a, b| {
                let ay = geometry.pixel_to_screen(a.pos).y;
                let by = geometry.pixel_to_screen(b.pos).y;
                ay.total_cmp(&by)
            });
        }
        let style = self.input.objects.clone();
        for object in order {
            if let Some(cell) = object.tile {
                let tileset = cell
                    .slot()
                    .and_then(|slot| self.map.tilesets.get(slot.index()));
                let sprite =
                    tileset.and_then(|t| self.res.sprites.sprite_of(&t.tileset, cell.tile()));
                let instance = object_instance(
                    &self.geometry,
                    tileset.map(|t| t.tileset.as_ref()),
                    object,
                    cell,
                    sprite,
                );
                self.push_instances(sprites_params, &[instance]);
            } else if let Shape::Text(text) = &object.shape {
                let origin = self.geometry.pixel_to_screen(object.pos);
                let rect = DRect::from_min_size(origin, object.size);
                let mut item = TextItem::from_text_object(text, rect, object.rotation, origin);
                if object.opacity < 1.0 {
                    let mut color = text.color;
                    color.a = (f64::from(color.a) * object.opacity.clamp(0.0, 1.0)).round() as u8;
                    item = item.color(color);
                }
                self.text(&item, plain_params);
            } else if style.draws_shapes() {
                let color = object_color(object, layer, &style);
                let mut items = Vec::new();
                shape_vectors(object, color, &self.geometry, &style, layer_id, &mut items);
                let refs: Vec<&VectorItem> = items.iter().collect();
                self.tessellated(&refs, plain_params);
            }
        }
    }

    fn image_layer(&mut self, layer: &ImageLayer, offset: DVec2, params: u32) {
        let Some(image) = &layer.image else {
            return;
        };
        let sprites =
            self.res
                .sprites
                .image_layer(image, self.res.store, self.res.pages, self.res.frame);
        let local = self.view_rect.translated(-offset);
        match sprites {
            ImageLayerSprites::Pending => {}
            ImageLayerSprites::Missing => {
                let size = image.declared_size.as_dvec2();
                if size.x > 0.0 && size.y > 0.0 {
                    self.push_instances(
                        params,
                        &[marker(DRect::from_min_size(DVec2::ZERO, size), [255; 4])],
                    );
                }
            }
            ImageLayerSprites::Ready { size, blocks } => {
                let size = size.as_dvec2().max(DVec2::ONE);
                let copies =
                    |repeat: bool, min: f64, max: f64, step: f64| -> std::ops::Range<i64> {
                        if !repeat {
                            return 0..1;
                        }
                        let first = (min / step).floor() as i64;
                        let last = (max / step).ceil() as i64;
                        first..last.min(first + MAX_REPEATS)
                    };
                let xs = copies(layer.repeat_x, local.min.x, local.max.x, size.x);
                let ys = copies(layer.repeat_y, local.min.y, local.max.y, size.y);
                let mut instances = Vec::new();
                for y in ys {
                    for x in xs.clone() {
                        let at = DVec2::new(x as f64 * size.x, y as f64 * size.y);
                        for block in &blocks {
                            let rect = DRect::from_min_size(
                                at + block.rect.min().as_dvec2(),
                                block.rect.size().as_dvec2(),
                            );
                            if rect.intersects(local) {
                                instances.push(plain(rect, block.sprite, [255; 4]));
                            }
                        }
                    }
                }
                self.push_instances(params, &instances);
            }
        }
    }

    fn previews(&mut self, params: u32) {
        let mut instances = Vec::with_capacity(self.input.previews.len());
        for preview in self.input.previews.iter() {
            let cell = preview.cell;
            let tileset = cell
                .slot()
                .and_then(|slot| self.map.tilesets.get(slot.index()));
            let sprite = tileset.and_then(|t| self.res.sprites.sprite_of(&t.tileset, cell.tile()));
            let mut instance = cell_instance(
                &self.geometry,
                tileset.map(|t| t.tileset.as_ref()),
                cell,
                preview.at,
                sprite,
            );
            instance.color[3] = (preview.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
            instances.push(instance);
        }
        self.push_instances(params, &instances);
    }

    /// Vector items: shapes tessellated, text as glyphs.
    fn vector_items(&mut self, items: &[&VectorItem], params: u32) {
        let shapes: Vec<&VectorItem> = items
            .iter()
            .copied()
            .filter(|item| item.as_text().is_none())
            .collect();
        self.tessellated(&shapes, params);
        for item in items {
            if let Some(text) = item.as_text() {
                self.text(text, params);
            }
        }
    }

    fn tessellated(&mut self, items: &[&VectorItem], params: u32) {
        let start = self.data.vertices.len() as u32;
        for item in items {
            tessellate(item, self.input.camera.zoom, &mut self.data.vertices);
        }
        let end = self.data.vertices.len() as u32;
        self.push_step(StepKind::Vectors, start, end, params);
    }

    fn text(&mut self, item: &TextItem, params: u32) {
        let laid = lay_out(item, self.input.camera.zoom);
        let color = laid.color.to_array();
        let pivot = laid.pivot;
        let rotation = laid.rotation_radians as f32;
        let mut instances = Vec::with_capacity(laid.glyphs.len() + laid.rules.len());
        for glyph in &laid.glyphs {
            let Some(sprite) = self.res.sprites.glyph(
                glyph.key,
                self.res.pages,
                self.res.store,
                self.res.device,
                self.res.queue,
                self.res.frame,
            ) else {
                continue;
            };
            let mut instance = plain(glyph.rect, sprite, color);
            instance.pivot_rot = [pivot.x as f32, pivot.y as f32, rotation, 0.0];
            instances.push(instance);
        }
        for rule in &laid.rules {
            let mut instance = solid(*rule, laid.color);
            instance.pivot_rot = [pivot.x as f32, pivot.y as f32, rotation, 0.0];
            instances.push(instance);
        }
        self.push_instances(params, &instances);
    }
}
