//! Far zoom. Zoomed far out, a large map shows millions of cells and no instance count stays
//! inside a frame's budget. Below a threshold, runs of tile layers are drawn from impostors:
//! pictures of world-aligned squares of a run, [`SQUARE_TEXELS`] a side. A run is a stretch of
//! tile layers that composite plainly with nothing drawn between them (`draw_list.rs` decides);
//! an impostor holds every layer of its run with its offset, opacity and tint, drawn from the
//! chunks' own instances in Tiled's order and clipped to its square, so squares meet exactly and
//! an oversized tile stacks across them as Tiled stacks it.
//!
//! The resolution is the power of two (device pixels per world pixel) that meets the screen's,
//! so an impostor is never blurrier than drawing the tiles would be, unless the pages cannot hold
//! the squares in view, when it steps down. The squares a view shows are bounded by the screen's
//! area, not the map's, and a square's cells and layers together by [`SQUARE_WORK`], so drawing
//! one takes a bounded time at any zoom, tile size and number of layers.
//! An impostor is valid while its run's chunks, attributes and tilesets are the ones it was drawn
//! from: the run's chunks are held, so an address is never reused while it is compared. Squares
//! are drawn, nearest the view's centre first, for [`DRAW_TIME`] a frame and at least one; until
//! a square is drawn it shows its last picture, or nothing, and the frame reports it pending.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bt_core::{Chunk, LayerId, TileLayer};
use bt_utils::DRect;
use glam::{DVec2, IVec2, UVec2};

use crate::chunks::{ChunkCache, ChunkContext, tiles_for_view};
use crate::draw_list::LayerParams;
use crate::instance::{Instance, plain};
use crate::pages::Region;
use crate::sprites::Sprites;

/// Cells smaller than this on screen (device pixels) are drawn through impostors.
pub(crate) const IMPOSTOR_CELL_PX: f64 = 8.0;

/// The most texels an impostor's side has (a slot's side).
pub(crate) const SQUARE_TEXELS: u32 = 128;

/// The most cells times layers a square holds: about what one square costs to draw (a few
/// milliseconds for ten layers of 64 by 64 cells).
const SQUARE_WORK: f64 = 64.0 * 64.0 * 10.0;

/// The most cells along a square's side.
const MAX_SQUARE_CELLS: f64 = 64.0;

/// The fewest cells along a square's side, however many layers its run has.
const MIN_SQUARE_CELLS: f64 = 8.0;

/// The side of an impostor page.
const PAGE_SIDE: u32 = 2048;

/// Impostors a page side holds.
const SQUARES_PER_SIDE: u32 = PAGE_SIDE / SQUARE_TEXELS;

/// How long a frame spends drawing squares (it draws at least one).
const DRAW_TIME: Duration = Duration::from_millis(3);

/// The coarsest resolution a run steps down to: 2^-16 device pixels per world pixel.
const COARSEST_LEVEL: i32 = -16;

/// One tile layer of a run, as its impostors hold it.
#[derive(Copy, Clone, Debug)]
pub(crate) struct RunLayer<'m> {
    /// The layer.
    pub(crate) id: LayerId,
    /// Its cells.
    pub(crate) tiles: &'m TileLayer,
    /// Its offset (without parallax, which moves the whole run), opacity and tint.
    pub(crate) params: LayerParams,
}

impl RunLayer<'_> {
    fn offset(&self) -> DVec2 {
        DVec2::new(
            f64::from(self.params.offset[0]),
            f64::from(self.params.offset[1]),
        )
    }
}

/// A run to draw over a view.
pub(crate) struct RunView<'r, 'm> {
    /// The run's layers, bottom first.
    pub(crate) layers: &'r [RunLayer<'m>],
    /// The world rectangle in view, in the run's space (before its parallax shift).
    pub(crate) view_rect: DRect,
    /// Device pixels per world pixel.
    pub(crate) zoom_px: f64,
}

/// Identifies an impostor: its run, its resolution and its square.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ImpostorKey {
    run: u64,
    level: i32,
    square: IVec2,
}

/// A place for one impostor in the pages.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Slot {
    page: u32,
    index: u32,
}

impl Slot {
    /// The part of the slot a square of `texels` a side takes.
    fn region(self, texels: u32) -> Region {
        Region {
            page: self.page,
            origin: UVec2::new(
                (self.index % SQUARES_PER_SIDE) * SQUARE_TEXELS,
                (self.index / SQUARES_PER_SIDE) * SQUARE_TEXELS,
            ),
            size: UVec2::splat(texels.min(SQUARE_TEXELS)),
        }
    }

    /// The slot's number across all pages.
    fn number(self) -> u32 {
        self.page * SQUARES_PER_SIDE * SQUARES_PER_SIDE + self.index
    }
}

/// How big a run's squares are at a resolution: their side in texels and in world pixels.
#[derive(Copy, Clone, Debug, PartialEq)]
struct SquareSize {
    texels: u32,
    side: f64,
}

/// The sprite showing a slot's square of a size.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct SlotSprite {
    slot: u32,
    texels: u32,
}

/// An impostor drawn (or being drawn this frame).
#[derive(Clone, Debug)]
struct Impostor {
    slot: Slot,
    content: u64,
    last_used: u64,
    /// Drawn while some of its images were still being decoded: drawn again.
    complete: bool,
}

/// What a run's impostors were drawn from: a hash of its layers and the chunks it holds.
#[derive(Clone, Debug)]
struct RunRecord {
    content: u64,
    _held: Vec<Arc<Chunk>>,
    last_used: u64,
}

/// A picture to draw into the pages: where, from which world square, at which scale, and the
/// layers' instances (ranges of [`ImpostorWork::instances`]) with their parameters.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ImpostorJob {
    pub(crate) region: Region,
    pub(crate) square: DRect,
    pub(crate) scale: f64,
    pub(crate) layers: Vec<JobLayer>,
}

/// A layer of a job.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct JobLayer {
    pub(crate) params: LayerParams,
    pub(crate) instances: Range<u32>,
}

/// The impostor pages and what is in them.
pub(crate) struct Impostors {
    array_view: wgpu::TextureView,
    layer_views: Vec<wgpu::TextureView>,
    slot_sprites: HashMap<SlotSprite, u32>,
    free: Vec<Slot>,
    entries: HashMap<ImpostorKey, Impostor>,
    runs: HashMap<u64, RunRecord>,
    jobs: Vec<ImpostorJob>,
    job_keys: Vec<ImpostorKey>,
    instances: Vec<Instance>,
    deadline: Instant,
    used_this_frame: usize,
    pending: usize,
}

/// What draws a run's squares: the chunks and their context.
pub(crate) struct RunSources<'s, 'c> {
    pub(crate) chunks: &'s mut ChunkCache,
    pub(crate) cx: &'s ChunkContext<'c>,
    pub(crate) frame: u64,
}

/// A square shown this frame and where its picture is.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct PlacedSquare {
    rect: DRect,
    slot: Slot,
    texels: u32,
}

/// What a square already has.
#[derive(Copy, Clone, Debug)]
struct Existing {
    slot: Slot,
    valid: bool,
}

impl Impostors {
    /// Room for `pages` impostor pages.
    pub(crate) fn new(device: &wgpu::Device, pages: u32) -> Impostors {
        let limits = device.limits();
        let side = PAGE_SIDE.min(limits.max_texture_dimension_2d);
        let count = pages.clamp(1, limits.max_texture_array_layers.max(1));
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bt impostors"),
            size: wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: count,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: crate::compositor::OFFSCREEN,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let array_view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("bt impostors"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..wgpu::TextureViewDescriptor::default()
        });
        let layer_views = (0..count)
            .map(|layer| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("bt impostor page"),
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: layer,
                    array_layer_count: Some(1),
                    ..wgpu::TextureViewDescriptor::default()
                })
            })
            .collect();
        let per_page = (side / SQUARE_TEXELS).pow(2);
        let free = (0..count)
            .flat_map(|page| (0..per_page).map(move |index| Slot { page, index }))
            .rev()
            .collect();
        Impostors {
            array_view,
            layer_views,
            slot_sprites: HashMap::new(),
            free,
            entries: HashMap::new(),
            runs: HashMap::new(),
            jobs: Vec::new(),
            job_keys: Vec::new(),
            instances: Vec::new(),
            deadline: Instant::now(),
            used_this_frame: 0,
            pending: 0,
        }
    }

    pub(crate) fn array_view(&self) -> &wgpu::TextureView {
        &self.array_view
    }

    pub(crate) fn layer_view(&self, page: u32) -> Option<&wgpu::TextureView> {
        self.layer_views.get(page as usize)
    }

    /// Starts a frame that began at `started`: squares are drawn until a while after it, and none
    /// is used yet.
    pub(crate) fn begin_frame(&mut self, started: Instant) {
        self.deadline = started + DRAW_TIME;
        self.used_this_frame = 0;
        self.pending = 0;
        self.jobs.clear();
        self.job_keys.clear();
        self.instances.clear();
    }

    /// The squares showing `run` over its view, planning the drawing of those that need it.
    pub(crate) fn draw_run(
        &mut self,
        run: &RunView<'_, '_>,
        sources: &mut RunSources<'_, '_>,
    ) -> Vec<PlacedSquare> {
        let run_key = run_key(run.layers, sources.cx);
        let content = self.record_run(run_key, run.layers, sources);
        let map = sources.cx.map;
        let cell_side = (f64::from(map.tile_width) * f64::from(map.tile_height))
            .sqrt()
            .max(1.0);
        let layers = run.layers.len();
        let level = self.level_for(run.view_rect, run.zoom_px, cell_side, layers);
        let size = square_size(level, cell_side, layers);
        let side = size.side;
        let mut squares = squares_in(run.view_rect, side);
        let center = (run.view_rect.min + run.view_rect.max) * 0.5;
        squares.sort_by(|a, b| {
            let da =
                (square_rect(*a, side).min + DVec2::splat(side * 0.5) - center).length_squared();
            let db =
                (square_rect(*b, side).min + DVec2::splat(side * 0.5) - center).length_squared();
            da.total_cmp(&db)
        });
        let mut placed = Vec::with_capacity(squares.len());
        for square in squares {
            let key = ImpostorKey {
                run: run_key,
                level,
                square,
            };
            let rect = square_rect(square, side);
            let existing = self.entries.get(&key).map(|entry| Existing {
                slot: entry.slot,
                valid: entry.content == content && entry.complete,
            });
            let may_draw = self.jobs.is_empty() || Instant::now() < self.deadline;
            let slot = match existing {
                Some(existing) if existing.valid => Some(existing.slot),
                // No slot this frame (more squares in view than slots): nothing to wait for.
                _ if may_draw => self.plan(&key, rect, size.texels, content, run, sources),
                _ => {
                    self.pending += 1;
                    // A stale picture until the square is drawn again; nothing when there is none.
                    existing.map(|existing| existing.slot)
                }
            };
            let Some(slot) = slot else {
                continue;
            };
            if let Some(entry) = self.entries.get_mut(&key) {
                entry.last_used = sources.frame;
            }
            self.used_this_frame += 1;
            placed.push(PlacedSquare {
                rect,
                slot,
                texels: size.texels,
            });
        }
        placed
    }

    /// The instances drawing `placed` squares from their pictures.
    pub(crate) fn quads(
        &mut self,
        placed: &[PlacedSquare],
        sprites: &mut Sprites,
    ) -> Vec<Instance> {
        placed
            .iter()
            .map(|square| {
                let sprite = self.sprite_of(square.slot, square.texels, sprites);
                plain(square.rect, sprite, [255; 4])
            })
            .collect()
    }

    /// Plans the drawing of a square; its slot, or `None` when no slot can be had this frame.
    fn plan(
        &mut self,
        key: &ImpostorKey,
        rect: DRect,
        texels: u32,
        content: u64,
        run: &RunView<'_, '_>,
        sources: &mut RunSources<'_, '_>,
    ) -> Option<Slot> {
        let slot = match self.entries.get(key) {
            Some(entry) => entry.slot,
            None => self.allocate(sources.frame)?,
        };
        let scale = f64::from(texels) / (rect.max.x - rect.min.x);
        let mut layers = Vec::with_capacity(run.layers.len());
        for layer in run.layers {
            let cells = tiles_for_view(&sources.cx.geometry, sources.cx.map, rect, layer.offset());
            let start = self.instances.len() as u32;
            sources.chunks.collect(
                layer.tiles,
                cells,
                sources.cx,
                sources.frame,
                &mut self.instances,
            );
            let end = self.instances.len() as u32;
            if end > start {
                layers.push(JobLayer {
                    params: layer.params,
                    instances: start..end,
                });
            }
        }
        self.jobs.push(ImpostorJob {
            region: slot.region(texels),
            square: rect,
            scale,
            layers,
        });
        self.job_keys.push(key.clone());
        self.entries.insert(
            key.clone(),
            Impostor {
                slot,
                content,
                last_used: sources.frame,
                complete: true,
            },
        );
        Some(slot)
    }

    /// A free slot, or the slot of the impostor least recently used before `frame`.
    fn allocate(&mut self, frame: u64) -> Option<Slot> {
        if let Some(slot) = self.free.pop() {
            return Some(slot);
        }
        let oldest = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.last_used < frame)
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(key, _)| key.clone())?;
        self.entries.remove(&oldest).map(|entry| entry.slot)
    }

    /// The resolution for a run over `view_rect`: the power of two meeting `zoom_px`, coarser while
    /// its squares would not fit the slots this frame has left.
    /// A run's squares are as small as their work bound allows, so a coarser resolution helps only
    /// while it makes them fewer.
    fn level_for(&self, view_rect: DRect, zoom_px: f64, cell_side: f64, layers: usize) -> i32 {
        let mut level = zoom_px.max(f64::MIN_POSITIVE).log2().ceil() as i32;
        let total = self.free.len() + self.entries.len();
        let left = total.saturating_sub(self.used_this_frame) as u64;
        let count =
            |level: i32| square_count(view_rect, square_size(level, cell_side, layers).side);
        while level > COARSEST_LEVEL && count(level) > left && count(level - 1) < count(level) {
            level -= 1;
        }
        level
    }

    /// Notes what a run is now; its content hash. A run whose chunks or attributes changed holds
    /// its new chunks instead of the old.
    fn record_run(
        &mut self,
        run_key: u64,
        layers: &[RunLayer<'_>],
        sources: &RunSources<'_, '_>,
    ) -> u64 {
        let mut hasher = DefaultHasher::new();
        run_key.hash(&mut hasher);
        for layer in layers {
            hash_params(&layer.params, &mut hasher);
            for chunk in layer.tiles.chunks() {
                chunk.pos.hash(&mut hasher);
                (Arc::as_ptr(chunk.chunk) as usize).hash(&mut hasher);
            }
        }
        let content = hasher.finish();
        let frame = sources.frame;
        match self.runs.get_mut(&run_key) {
            Some(record) if record.content == content => record.last_used = frame,
            _ => {
                let held = layers
                    .iter()
                    .flat_map(|layer| layer.tiles.chunks().map(|chunk| Arc::clone(chunk.chunk)))
                    .collect();
                self.runs.insert(
                    run_key,
                    RunRecord {
                        content,
                        _held: held,
                        last_used: frame,
                    },
                );
            }
        }
        content
    }

    fn sprite_of(&mut self, slot: Slot, texels: u32, sprites: &mut Sprites) -> u32 {
        let key = SlotSprite {
            slot: slot.number(),
            texels,
        };
        *self
            .slot_sprites
            .entry(key)
            .or_insert_with(|| sprites.impostor_sprite(slot.region(texels)))
    }

    /// The pictures to draw this frame, with their instances.
    pub(crate) fn take_work(&mut self) -> ImpostorWork {
        ImpostorWork {
            jobs: std::mem::take(&mut self.jobs),
            keys: std::mem::take(&mut self.job_keys),
            instances: std::mem::take(&mut self.instances),
        }
    }

    /// Marks the impostors of `work` as drawn while images were missing, to draw them again.
    pub(crate) fn mark_incomplete(&mut self, work: &ImpostorWork) {
        for key in &work.keys {
            if let Some(entry) = self.entries.get_mut(key) {
                entry.complete = false;
            }
        }
    }

    /// Lets go of the runs not drawn since `frame` (their chunks are no longer held).
    pub(crate) fn forget_runs_before(&mut self, frame: u64) {
        self.runs.retain(|_, record| record.last_used >= frame);
    }

    /// The impostors kept.
    pub(crate) fn count(&self) -> usize {
        self.entries.len()
    }

    /// The squares in view that are not drawn yet (the drawing budget ran out).
    pub(crate) fn pending(&self) -> usize {
        self.pending
    }
}

/// What to draw into the impostor pages before the frame.
#[derive(Clone, Debug, Default)]
pub(crate) struct ImpostorWork {
    pub(crate) jobs: Vec<ImpostorJob>,
    keys: Vec<ImpostorKey>,
    pub(crate) instances: Vec<Instance>,
}

/// A run's identity: its layers, the grid and the tilesets' sprites.
fn run_key(layers: &[RunLayer<'_>], cx: &ChunkContext<'_>) -> u64 {
    let mut hasher = DefaultHasher::new();
    cx.geometry.hash(&mut hasher);
    cx.tilesets.hash(&mut hasher);
    for layer in layers {
        layer.id.hash(&mut hasher);
    }
    hasher.finish()
}

fn hash_params(params: &LayerParams, hasher: &mut DefaultHasher) {
    for value in params.offset {
        value.to_bits().hash(hasher);
    }
    params.opacity.to_bits().hash(hasher);
    params.tinted.hash(hasher);
    for value in params.tint {
        value.to_bits().hash(hasher);
    }
}

/// The size of a run's squares at `level` (2^level device pixels per world pixel), for cells of
/// `cell_side` world pixels and `layers` layers: as many cells as [`SQUARE_WORK`] allows, within
/// a slot, and a whole number of texels a side so the picture's scale is the resolution's.
fn square_size(level: i32, cell_side: f64, layers: usize) -> SquareSize {
    let scale = 2f64.powi(level);
    let cells = (SQUARE_WORK / layers.max(1) as f64)
        .sqrt()
        .clamp(MIN_SQUARE_CELLS, MAX_SQUARE_CELLS);
    let texels = ((cells * cell_side * scale).floor() as u32).clamp(1, SQUARE_TEXELS);
    SquareSize {
        texels,
        side: f64::from(texels) / scale,
    }
}

fn square_rect(square: IVec2, side: f64) -> DRect {
    DRect::from_min_size(square.as_dvec2() * side, DVec2::splat(side))
}

/// How many squares of `side` meet `rect`.
fn square_count(rect: DRect, side: f64) -> u64 {
    let first = (rect.min / side).floor();
    let last = (rect.max / side).ceil();
    let across = (last - first).max(DVec2::ZERO);
    (across.x.min(1e9) as u64).saturating_mul(across.y.min(1e9) as u64)
}

/// The squares of `side` meeting `rect`.
fn squares_in(rect: DRect, side: f64) -> Vec<IVec2> {
    let first = (rect.min / side).floor();
    let last = (rect.max / side).ceil() - DVec2::ONE;
    let clamp = |v: f64| v.clamp(f64::from(i32::MIN / 2), f64::from(i32::MAX / 2)) as i32;
    let mut squares = Vec::new();
    for y in clamp(first.y)..=clamp(last.y) {
        for x in clamp(first.x)..=clamp(last.x) {
            squares.push(IVec2::new(x, y));
        }
    }
    squares
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squares_tile_the_view_at_the_resolution() {
        // Few layers of large cells: a whole slot at the resolution.
        let full = square_size(-2, 32.0, 2);
        assert_eq!((full.texels, full.side), (128, 512.0));
        // Ten layers of 16-pixel cells far out: 64 cells a side, in fewer texels.
        let far = square_size(-5, 16.0, 10);
        assert_eq!((far.texels, far.side), (32, 1024.0));
        assert_eq!(far.side / 16.0, 64.0);
        // Many layers: fewer cells, never below the floor.
        let deep = square_size(-5, 16.0, 1000);
        assert_eq!(deep.side / 16.0, 8.0);
        let view = DRect::from_min_size(DVec2::new(-10.0, 0.0), DVec2::new(300.0, 256.0));
        let squares = squares_in(view, 256.0);
        assert_eq!(square_count(view, 256.0), 3);
        assert_eq!(
            squares,
            [IVec2::new(-1, 0), IVec2::new(0, 0), IVec2::new(1, 0)]
        );
        assert_eq!(
            square_rect(IVec2::new(-1, 2), 256.0).min,
            DVec2::new(-256.0, 512.0)
        );
        let slot = Slot { page: 1, index: 17 };
        assert_eq!(slot.region(64).origin, UVec2::new(128, 128));
        assert_eq!(slot.region(64).size, UVec2::splat(64));
        assert_eq!(slot.number(), 256 + 17);
    }
}
