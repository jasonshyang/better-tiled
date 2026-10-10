//! The sprite table. Every image the frames draw from is a sprite: a tile, a block of an image
//! layer's image, a glyph. A sprite's entry says where its texels are in the pages and whether
//! they are there yet. Tilesets get a contiguous range of sprites when first seen (an atlas: its
//! base plus the tile id; a collection: one per tile), keyed by the identity of their `Arc`, so
//! an edited tileset gets a new range and the old one goes once nothing else holds that `Arc`.
//! Animated tiles are redirected through a remap table, updated each frame, so no instance
//! changes when a frame does.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use bt_core::{ImageRef, TileId, Tileset, TilesetImage};
use bt_image::{ImageKey, ImagePoll, ImageRequest};
use bt_utils::IRect;
use glam::{IVec2, UVec2};

use crate::pages::{GUTTER, Pages, Region};
use crate::supply::{ImageSupply, image_request};
use crate::text::{GlyphKey, rasterize};

/// The texels are in the pages (0, the default, is pending: nothing is drawn yet).
pub(crate) const READY: u32 = 1;
/// The image cannot be drawn: draw Tiled's marker.
pub(crate) const MISSING: u32 = 2;
/// The texels are in the impostor pages.
pub(crate) const IMPOSTOR: u32 = 3;

/// A sprite as the shader reads it.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GpuSprite {
    /// x, y, width, height in texels at mip level 0.
    pub(crate) rect: [u32; 4],
    /// The page (texture array layer).
    pub(crate) page: u32,
    /// 0 (pending), [`READY`], [`MISSING`] or [`IMPOSTOR`].
    pub(crate) state: u32,
    /// Unused; keeps the size a multiple of 16 bytes.
    pub(crate) pad: [u32; 2],
}

/// The size of a sprite entry in the storage buffer.
pub(crate) const SPRITE_SIZE: u64 = std::mem::size_of::<GpuSprite>() as u64;

/// A run of changed entries, to upload only those.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Dirty {
    pub(crate) start: u32,
    pub(crate) end: u32,
}

impl Dirty {
    fn mark(&mut self, index: u32) {
        if self.start >= self.end {
            self.start = index;
            self.end = index + 1;
        } else {
            self.start = self.start.min(index);
            self.end = self.end.max(index + 1);
        }
    }

    pub(crate) fn is_empty(self) -> bool {
        self.start >= self.end
    }
}

/// A packed part of an image: the image's pixels it holds, where they are, and its sprite.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Block {
    image_rect: IRect,
    region: Region,
    sprite: u32,
}

/// What the renderer knows of an image.
#[derive(Clone, Debug, PartialEq)]
enum ImageState {
    /// Asked of the image store.
    Requested,
    /// In the pages.
    Packed { size: UVec2, blocks: Vec<Block> },
    /// It could not be decoded, or is too large to pack.
    Failed,
}

/// An image the frames draw from.
#[derive(Clone, Debug)]
struct ImageSlot {
    request: ImageRequest,
    state: ImageState,
    /// The tile grid of an atlas, so blocks split between tile rows and columns.
    grid: Option<TileGrid>,
    last_used: u64,
}

/// Where an atlas's tiles are in its image.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct TileGrid {
    margin: u32,
    step: UVec2,
}

/// The sprites of one tileset.
#[derive(Clone, Debug)]
struct TilesetSprites {
    tileset: Arc<Tileset>,
    base: u32,
    count: u32,
    /// A collection's tiles, by id, with their offset from `base`.
    collection: BTreeMap<TileId, u32>,
    /// The images its tiles are cut from: an atlas's one, or each collection tile's.
    images: TilesetImages,
    /// The tiles with an animation.
    animated: Vec<TileId>,
    /// Unique for the life of the renderer: what chunk caches built from these sprites record.
    serial: u64,
    last_used: u64,
}

/// The keys of a tileset's images, kept so they are not worked out (and the files looked at)
/// again.
#[derive(Clone, Debug)]
enum TilesetImages {
    Atlas(ImageKey),
    Collection(BTreeMap<TileId, ImageKey>),
}

impl TilesetImages {
    fn of(&self, tile: TileId) -> Option<&ImageKey> {
        match self {
            TilesetImages::Atlas(key) => Some(key),
            TilesetImages::Collection(keys) => keys.get(&tile),
        }
    }

    fn all(&self) -> Vec<ImageKey> {
        match self {
            TilesetImages::Atlas(key) => vec![key.clone()],
            TilesetImages::Collection(keys) => keys.values().cloned().collect(),
        }
    }
}

/// A glyph in the pages.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct GlyphSprite {
    sprite: u32,
    page: u32,
    last_used: u64,
}

/// The sprite table, the remap table and what they are made of.
pub(crate) struct Sprites {
    table: Vec<GpuSprite>,
    remap: Vec<u32>,
    free: Vec<std::ops::Range<u32>>,
    table_dirty: Dirty,
    remap_dirty: Dirty,
    tilesets: HashMap<usize, TilesetSprites>,
    images: HashMap<ImageKey, ImageSlot>,
    glyphs: HashMap<GlyphKey, GlyphSprite>,
    next_serial: u64,
}

/// The id of a tileset in the registry: its `Arc`'s address (held while registered, so it is
/// never reused meanwhile).
pub(crate) fn tileset_id(tileset: &Arc<Tileset>) -> usize {
    Arc::as_ptr(tileset) as usize
}

impl Sprites {
    pub(crate) fn new() -> Sprites {
        Sprites {
            // Sprite 0 is nothing: solid quads and markers use it.
            table: vec![GpuSprite::default()],
            remap: vec![0],
            free: Vec::new(),
            table_dirty: Dirty { start: 0, end: 1 },
            remap_dirty: Dirty { start: 0, end: 1 },
            tilesets: HashMap::new(),
            images: HashMap::new(),
            glyphs: HashMap::new(),
            next_serial: 1,
        }
    }

    pub(crate) fn table(&self) -> &[GpuSprite] {
        &self.table
    }

    pub(crate) fn remap(&self) -> &[u32] {
        &self.remap
    }

    /// The changed runs since the last call, and forgets them.
    pub(crate) fn take_dirty(&mut self) -> DirtyTables {
        DirtyTables {
            table: std::mem::take(&mut self.table_dirty),
            remap: std::mem::take(&mut self.remap_dirty),
        }
    }

    fn allocate(&mut self, count: u32) -> u32 {
        if let Some(position) = self
            .free
            .iter()
            .position(|range| range.len() as u32 >= count)
            && let Some(range) = self.free.get_mut(position)
        {
            let start = range.start;
            range.start += count;
            if range.start >= range.end {
                self.free.remove(position);
            }
            for index in start..start + count {
                self.set(index, GpuSprite::default());
                self.set_remap(index, index);
            }
            return start;
        }
        let start = self.table.len() as u32;
        for index in start..start + count {
            self.table.push(GpuSprite::default());
            self.remap.push(index);
            self.table_dirty.mark(index);
            self.remap_dirty.mark(index);
        }
        start
    }

    fn release(&mut self, start: u32, count: u32) {
        if count > 0 {
            self.free.push(start..start + count);
        }
    }

    fn set(&mut self, index: u32, sprite: GpuSprite) {
        if let Some(entry) = self.table.get_mut(index as usize)
            && *entry != sprite
        {
            *entry = sprite;
            self.table_dirty.mark(index);
        }
    }

    fn set_remap(&mut self, index: u32, target: u32) {
        if let Some(entry) = self.remap.get_mut(index as usize)
            && *entry != target
        {
            *entry = target;
            self.remap_dirty.mark(index);
        }
    }

    /// Makes `tileset`'s sprites exist, asking the store for its images; marks it used in
    /// `frame`.
    pub(crate) fn register_tileset(
        &mut self,
        tileset: &Arc<Tileset>,
        store: &mut dyn ImageSupply,
        frame: u64,
    ) {
        let id = tileset_id(tileset);
        if let Some(entry) = self.tilesets.get_mut(&id) {
            entry.last_used = frame;
            return;
        }
        let collection: BTreeMap<TileId, u32> = match tileset.image {
            TilesetImage::Atlas { .. } => BTreeMap::new(),
            TilesetImage::Collection => (0u32..)
                .zip(tileset.tiles.keys())
                .map(|(i, id)| (*id, i))
                .collect(),
        };
        let count = match tileset.image {
            TilesetImage::Atlas { tile_count, .. } => tile_count,
            TilesetImage::Collection => collection.len() as u32,
        };
        let base = self.allocate(count);
        let images = match &tileset.image {
            TilesetImage::Atlas { image, .. } => {
                let grid = TileGrid {
                    margin: tileset.margin,
                    step: tileset.tile_size() + UVec2::splat(tileset.spacing),
                };
                TilesetImages::Atlas(self.request(image, Some(grid), store, frame))
            }
            TilesetImage::Collection => {
                let mut keys = BTreeMap::new();
                for (id, tile) in &tileset.tiles {
                    if let Some(image) = &tile.image {
                        keys.insert(*id, self.request(image, None, store, frame));
                    }
                }
                TilesetImages::Collection(keys)
            }
        };
        let animated = tileset
            .tiles
            .iter()
            .filter(|(_, tile)| !tile.animation.is_empty())
            .map(|(id, _)| *id)
            .collect();
        let serial = self.next_serial;
        self.next_serial += 1;
        self.tilesets.insert(
            id,
            TilesetSprites {
                tileset: Arc::clone(tileset),
                base,
                count,
                collection,
                images,
                animated,
                serial,
                last_used: frame,
            },
        );
        self.fill_tileset(id);
    }

    /// Asks the store for an image the frames draw from.
    fn request(
        &mut self,
        image: &ImageRef,
        grid: Option<TileGrid>,
        store: &mut dyn ImageSupply,
        frame: u64,
    ) -> ImageKey {
        let request = image_request(image);
        let key = store.request(request.clone());
        self.images
            .entry(key.clone())
            .and_modify(|slot| slot.last_used = frame)
            .or_insert(ImageSlot {
                request,
                state: ImageState::Requested,
                grid,
                last_used: frame,
            });
        key
    }

    /// An image layer's image: asked for when first seen; its blocks' sprites once packed.
    pub(crate) fn image_layer(
        &mut self,
        image: &ImageRef,
        store: &mut dyn ImageSupply,
        pages: &mut Pages,
        frame: u64,
    ) -> ImageLayerSprites {
        let key = self.request(image, None, store, frame);
        let Some(slot) = self.images.get(&key) else {
            return ImageLayerSprites::Pending;
        };
        if let ImageState::Packed { blocks, .. } = &slot.state {
            for block in blocks {
                pages.touch(block.region.page, frame);
            }
        }
        match &slot.state {
            ImageState::Requested => ImageLayerSprites::Pending,
            ImageState::Failed => ImageLayerSprites::Missing,
            ImageState::Packed { size, blocks } => ImageLayerSprites::Ready {
                size: *size,
                blocks: blocks
                    .iter()
                    .map(|block| ImageBlock {
                        rect: block.image_rect,
                        sprite: block.sprite,
                    })
                    .collect(),
            },
        }
    }

    /// Takes in decoded images: packs them, fills the sprites of the tilesets that use them.
    /// Returns how many images are still being decoded.
    pub(crate) fn take_images(
        &mut self,
        store: &mut dyn ImageSupply,
        pages: &mut Pages,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: u64,
    ) -> usize {
        let waiting: Vec<ImageKey> = self
            .images
            .iter()
            .filter(|(_, slot)| slot.state == ImageState::Requested)
            .map(|(key, _)| key.clone())
            .collect();
        let mut pending = 0;
        let mut changed = false;
        for key in waiting {
            match store.poll(&key) {
                ImagePoll::Pending => pending += 1,
                ImagePoll::NotRequested => {
                    // Evicted from the store since it was asked for: ask again.
                    if let Some(slot) = self.images.get(&key) {
                        store.request(slot.request.clone());
                    }
                    pending += 1;
                }
                ImagePoll::Failed(error) => {
                    log::warn!("{error}");
                    if let Some(slot) = self.images.get_mut(&key) {
                        slot.state = ImageState::Failed;
                    }
                    changed = true;
                }
                ImagePoll::Ready(image) => {
                    let grid = self.images.get(&key).and_then(|slot| slot.grid);
                    let size = UVec2::new(image.width, image.height);
                    let mut blocks = Vec::new();
                    let mut failed = false;
                    for rect in blocks_for(size, grid, pages.side() - 2 * GUTTER - 2) {
                        let allocation =
                            pages.allocate(device, queue, rect.size().as_uvec2(), frame);
                        for page in allocation.evicted {
                            self.forget_page(page, store);
                        }
                        let Some(region) = allocation.region else {
                            failed = true;
                            break;
                        };
                        pages.upload(queue, region, &crop(&image.rgba, size, rect));
                        let sprite = self.allocate(1);
                        self.set(
                            sprite,
                            GpuSprite {
                                rect: [
                                    region.origin.x,
                                    region.origin.y,
                                    region.size.x,
                                    region.size.y,
                                ],
                                page: region.page,
                                state: READY,
                                pad: [0; 2],
                            },
                        );
                        blocks.push(Block {
                            image_rect: rect,
                            region,
                            sprite,
                        });
                    }
                    if let Some(slot) = self.images.get_mut(&key) {
                        slot.state = if failed {
                            log::warn!(
                                "an image of {}×{} pixels does not fit the texture pages",
                                size.x,
                                size.y
                            );
                            ImageState::Failed
                        } else {
                            ImageState::Packed { size, blocks }
                        };
                    }
                    changed = true;
                }
            }
        }
        if changed {
            let ids: Vec<usize> = self.tilesets.keys().copied().collect();
            for id in ids {
                self.fill_tileset(id);
            }
        }
        pending
    }

    /// A sprite for an impostor slot in `region` of the impostor pages (kept for the slot).
    pub(crate) fn impostor_sprite(&mut self, region: Region) -> u32 {
        let sprite = self.allocate(1);
        self.set(
            sprite,
            GpuSprite {
                rect: [
                    region.origin.x,
                    region.origin.y,
                    region.size.x,
                    region.size.y,
                ],
                page: region.page,
                state: IMPOSTOR,
                pad: [0; 2],
            },
        );
        sprite
    }

    /// Waits for every image asked for and not yet decoded (headless renders).
    pub(crate) fn wait_for_images(&mut self, store: &mut dyn ImageSupply) {
        let waiting: Vec<ImageRequest> = self
            .images
            .values()
            .filter(|slot| slot.state == ImageState::Requested)
            .map(|slot| slot.request.clone())
            .collect();
        for request in waiting {
            store.wait(request);
        }
    }

    /// An emptied page: every image and glyph packed there must be packed again.
    fn forget_page(&mut self, page: u32, store: &mut dyn ImageSupply) {
        let mut released = Vec::new();
        for slot in self.images.values_mut() {
            if let ImageState::Packed { blocks, .. } = &slot.state
                && blocks.iter().any(|block| block.region.page == page)
            {
                released.extend(blocks.iter().map(|block| block.sprite));
                slot.state = ImageState::Requested;
                store.request(slot.request.clone());
            }
        }
        let glyphs: Vec<GlyphKey> = self
            .glyphs
            .iter()
            .filter(|(_, glyph)| glyph.page == page)
            .map(|(key, _)| *key)
            .collect();
        for key in glyphs {
            if let Some(glyph) = self.glyphs.remove(&key) {
                released.push(glyph.sprite);
            }
        }
        for sprite in released {
            self.set(sprite, GpuSprite::default());
            self.release(sprite, 1);
        }
    }

    /// Recomputes a tileset's sprite entries from the state of its images.
    fn fill_tileset(&mut self, id: usize) {
        let Some(entry) = self.tilesets.get(&id) else {
            return;
        };
        let tileset = Arc::clone(&entry.tileset);
        let base = entry.base;
        let assignments: Vec<Assignment> = match &tileset.image {
            TilesetImage::Atlas { .. } => (0..entry.count)
                .map(|tile| Assignment {
                    sprite: base + tile,
                    tile: TileId(tile),
                })
                .collect(),
            TilesetImage::Collection => entry
                .collection
                .iter()
                .map(|(tile, offset)| Assignment {
                    sprite: base + offset,
                    tile: *tile,
                })
                .collect(),
        };
        let keys = entry.images.clone();
        for assignment in assignments {
            let sprite = self.tile_sprite(&tileset, &keys, assignment.tile);
            self.set(assignment.sprite, sprite);
        }
    }

    /// Where a tile's texels are, from the state of its image.
    fn tile_sprite(&self, tileset: &Tileset, keys: &TilesetImages, tile: TileId) -> GpuSprite {
        let missing = GpuSprite {
            state: MISSING,
            ..GpuSprite::default()
        };
        let Some(image) = tileset.tile_image(tile) else {
            return missing;
        };
        let Some(slot) = keys.of(tile).and_then(|key| self.images.get(key)) else {
            return missing;
        };
        match &slot.state {
            ImageState::Requested => GpuSprite::default(),
            ImageState::Failed => missing,
            ImageState::Packed { size, blocks } => {
                let rect = image.rect;
                let inside = rect.x >= 0
                    && rect.y >= 0
                    && rect.w > 0
                    && rect.h > 0
                    && rect.max().x <= size.x as i32
                    && rect.max().y <= size.y as i32;
                let block = blocks
                    .iter()
                    .find(|block| block.image_rect.intersection(rect) == rect);
                match block.filter(|_| inside) {
                    Some(block) => {
                        let at =
                            block.region.origin.as_ivec2() + (rect.min() - block.image_rect.min());
                        GpuSprite {
                            rect: [at.x as u32, at.y as u32, rect.w as u32, rect.h as u32],
                            page: block.region.page,
                            state: READY,
                            pad: [0; 2],
                        }
                    }
                    None => missing,
                }
            }
        }
    }

    /// The serial number of a registered tileset's sprites: the same while it keeps them.
    pub(crate) fn serial_of(&self, tileset: &Arc<Tileset>) -> Option<u64> {
        self.tilesets
            .get(&tileset_id(tileset))
            .map(|entry| entry.serial)
    }

    /// The sprite a tile of a registered tileset is drawn from (before the remap); `None` for a
    /// tile the tileset does not have.
    pub(crate) fn sprite_of(&self, tileset: &Arc<Tileset>, tile: TileId) -> Option<u32> {
        let entry = self.tilesets.get(&tileset_id(tileset))?;
        match entry.tileset.image {
            TilesetImage::Atlas { .. } => (tile.0 < entry.count).then_some(entry.base + tile.0),
            TilesetImage::Collection => entry
                .collection
                .get(&tile)
                .map(|offset| entry.base + offset),
        }
    }

    /// Points every animated tile used in `frame` at its frame for `time_ms`; returns when the
    /// next frame is due, if anything animates.
    pub(crate) fn animate(&mut self, time_ms: u64, frame: u64) -> Option<u64> {
        let mut next: Option<u64> = None;
        let mut redirects = Vec::new();
        for entry in self
            .tilesets
            .values()
            .filter(|entry| entry.last_used == frame)
        {
            for &tile in &entry.animated {
                let shown = entry.tileset.animated_tile(tile, time_ms);
                if let Some(from) = sprite_in(entry, tile)
                    && let Some(to) = sprite_in(entry, shown)
                {
                    redirects.push(Redirect { from, to });
                }
                if let Some(due) = next_change(&entry.tileset, tile, time_ms) {
                    next = Some(next.map_or(due, |n| n.min(due)));
                }
            }
        }
        for redirect in redirects {
            self.set_remap(redirect.from, redirect.to);
        }
        next
    }

    /// The sprite of a glyph, rasterised and packed the first time it is drawn.
    pub(crate) fn glyph(
        &mut self,
        key: GlyphKey,
        pages: &mut Pages,
        store: &mut dyn ImageSupply,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: u64,
    ) -> Option<u32> {
        if let Some(glyph) = self.glyphs.get_mut(&key) {
            glyph.last_used = frame;
            pages.touch(glyph.page, frame);
            return Some(glyph.sprite);
        }
        let bitmap = rasterize(key)?;
        let allocation = pages.allocate(device, queue, bitmap.size, frame);
        for page in allocation.evicted {
            self.forget_page(page, store);
        }
        let region = allocation.region?;
        pages.upload(queue, region, &bitmap.rgba);
        let sprite = self.allocate(1);
        self.set(
            sprite,
            GpuSprite {
                rect: [
                    region.origin.x,
                    region.origin.y,
                    region.size.x,
                    region.size.y,
                ],
                page: region.page,
                state: READY,
                pad: [0; 2],
            },
        );
        self.glyphs.insert(
            key,
            GlyphSprite {
                sprite,
                page: region.page,
                last_used: frame,
            },
        );
        Some(sprite)
    }

    /// Marks the pages of the images used in `frame` as drawn from, so they are not evicted.
    pub(crate) fn touch_pages(&mut self, pages: &mut Pages, frame: u64) {
        let used: Vec<ImageKey> = self
            .tilesets
            .values()
            .filter(|entry| entry.last_used == frame)
            .flat_map(|entry| entry.images.all())
            .collect();
        for key in used {
            if let Some(slot) = self.images.get_mut(&key) {
                slot.last_used = frame;
            }
        }
        for slot in self.images.values().filter(|slot| slot.last_used == frame) {
            if let ImageState::Packed { blocks, .. } = &slot.state {
                for block in blocks {
                    pages.touch(block.region.page, frame);
                }
            }
        }
    }

    /// Lets go of tilesets nothing else holds any more and that were not drawn in `frame`.
    pub(crate) fn release_unused(&mut self, frame: u64) {
        let gone: Vec<usize> = self
            .tilesets
            .iter()
            .filter(|(_, entry)| entry.last_used < frame && Arc::strong_count(&entry.tileset) == 1)
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            if let Some(entry) = self.tilesets.remove(&id) {
                for index in entry.base..entry.base + entry.count {
                    self.set(index, GpuSprite::default());
                    self.set_remap(index, index);
                }
                self.release(entry.base, entry.count);
            }
        }
    }
}

/// The sprites of an image layer's image.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ImageLayerSprites {
    /// Still being decoded.
    Pending,
    /// It cannot be drawn.
    Missing,
    /// Packed, in blocks.
    Ready {
        size: UVec2,
        blocks: Vec<ImageBlock>,
    },
}

/// A block of an image and its sprite.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImageBlock {
    pub(crate) rect: IRect,
    pub(crate) sprite: u32,
}

/// The tables' changed runs.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DirtyTables {
    pub(crate) table: Dirty,
    pub(crate) remap: Dirty,
}

/// A sprite entry to fill for a tile.
struct Assignment {
    sprite: u32,
    tile: TileId,
}

/// An animated tile's sprite pointed at its current frame's.
struct Redirect {
    from: u32,
    to: u32,
}

fn sprite_in(entry: &TilesetSprites, tile: TileId) -> Option<u32> {
    match entry.tileset.image {
        TilesetImage::Atlas { .. } => (tile.0 < entry.count).then_some(entry.base + tile.0),
        TilesetImage::Collection => entry
            .collection
            .get(&tile)
            .map(|offset| entry.base + offset),
    }
}

/// When an animated tile next changes frame, in frame time.
fn next_change(tileset: &Tileset, tile: TileId, time_ms: u64) -> Option<u64> {
    let frames = &tileset.tile(tile)?.animation;
    let total: u64 = frames.iter().map(|f| u64::from(f.duration_ms)).sum();
    if total == 0 || frames.len() < 2 {
        return None;
    }
    let into = time_ms % total;
    let mut end = 0u64;
    for frame in frames {
        end += u64::from(frame.duration_ms);
        if into < end {
            return Some(time_ms + (end - into));
        }
    }
    Some(time_ms + (total - into))
}

/// The parts an image is packed in: the whole image when it fits a page, else blocks no larger
/// than `max` a side, split between an atlas's tile rows and columns.
fn blocks_for(size: UVec2, grid: Option<TileGrid>, max: u32) -> Vec<IRect> {
    if size.x <= max && size.y <= max {
        return vec![IRect::from_min_size(IVec2::ZERO, size.as_ivec2())];
    }
    let cuts = |length: u32, start: u32, step: u32| -> Vec<u32> {
        let mut cuts = vec![0];
        let mut from = 0u32;
        while length - from > max {
            let limit = from + max;
            // The last tile boundary at or before the limit, if past where the block starts.
            let aligned = limit
                .saturating_sub(start)
                .checked_div(step)
                .map(|tiles| start + tiles * step)
                .filter(|&boundary| boundary > from)
                .unwrap_or(limit);
            cuts.push(aligned);
            from = aligned;
        }
        cuts.push(length);
        cuts
    };
    let columns = cuts(
        size.x,
        grid.map_or(0, |g| g.margin),
        grid.map_or(0, |g| g.step.x),
    );
    let rows = cuts(
        size.y,
        grid.map_or(0, |g| g.margin),
        grid.map_or(0, |g| g.step.y),
    );
    let mut blocks = Vec::new();
    for row in rows.windows(2) {
        let [top, bottom] = row else { continue };
        for column in columns.windows(2) {
            let [left, right] = column else { continue };
            blocks.push(IRect::from_min_size(
                IVec2::new(*left as i32, *top as i32),
                IVec2::new((*right - *left) as i32, (*bottom - *top) as i32),
            ));
        }
    }
    blocks
}

/// The pixels of `rect` of an image.
fn crop(rgba: &[u8], size: UVec2, rect: IRect) -> Vec<u8> {
    let mut out = Vec::with_capacity((rect.w.max(0) * rect.h.max(0) * 4) as usize);
    for y in rect.y..rect.y + rect.h {
        let start = ((y as u32 * size.x + rect.x as u32) * 4) as usize;
        let end = start + (rect.w as usize) * 4;
        out.extend_from_slice(rgba.get(start..end).unwrap_or(&[]));
    }
    out
}

#[cfg(test)]
mod tests {
    use bt_core::Frame;

    use super::*;

    #[test]
    fn big_images_split_between_tiles() {
        assert_eq!(blocks_for(UVec2::new(100, 50), None, 100).len(), 1);
        let grid = TileGrid {
            margin: 1,
            step: UVec2::new(10, 10),
        };
        let blocks = blocks_for(UVec2::new(30, 95), Some(grid), 40);
        // Rows cut at 31 and 61 (tile boundaries), one column.
        assert_eq!(
            blocks,
            vec![
                IRect::from_min_size(IVec2::new(0, 0), IVec2::new(30, 31)),
                IRect::from_min_size(IVec2::new(0, 31), IVec2::new(30, 40)),
                IRect::from_min_size(IVec2::new(0, 71), IVec2::new(30, 24)),
            ]
        );
        assert!(blocks.iter().all(|b| b.h <= 40));
        assert_eq!(
            crop(
                &[1, 1, 1, 1, 2, 2, 2, 2],
                UVec2::new(2, 1),
                IRect::from_min_size(IVec2::new(1, 0), IVec2::ONE)
            ),
            vec![2, 2, 2, 2]
        );
    }

    #[test]
    fn animations_say_when_they_change() {
        let mut tileset = Tileset::default();
        tileset.tiles.insert(
            TileId(0),
            bt_core::TileData {
                animation: vec![
                    Frame {
                        tile: TileId(1),
                        duration_ms: 100,
                    },
                    Frame {
                        tile: TileId(2),
                        duration_ms: 0,
                    },
                    Frame {
                        tile: TileId(3),
                        duration_ms: 50,
                    },
                ],
                ..bt_core::TileData::default()
            },
        );
        assert_eq!(next_change(&tileset, TileId(0), 0), Some(100));
        assert_eq!(next_change(&tileset, TileId(0), 120), Some(150));
        assert_eq!(next_change(&tileset, TileId(0), 160), Some(250));
        assert_eq!(next_change(&tileset, TileId(1), 0), None);
    }

    #[test]
    fn dirty_runs_grow_to_cover_changes() {
        let mut dirty = Dirty::default();
        assert!(dirty.is_empty());
        dirty.mark(5);
        dirty.mark(2);
        assert_eq!(dirty, Dirty { start: 2, end: 6 });
    }
}
