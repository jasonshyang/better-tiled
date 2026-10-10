//! Far zoom. Zoomed far out, a large map shows millions of cells and no instance count stays
//! inside the frame's budget, so below a threshold each visible chunk of a tile layer is drawn
//! as one quad: an impostor, a picture of the chunk the renderer draws into impostor pages from
//! the chunk's own instances. An impostor of `k` pixels per cell serves zooms up to
//! `k / tile size`, and `k` is the power of two that meets the screen's resolution, so an
//! impostor is never blurrier than drawing the tiles would be. Impostors are drawn at the frame
//! time they were made (animations do not move at that zoom) and evicted least recently used.

use std::collections::HashMap;

use bt_utils::DRect;
use glam::UVec2;

use crate::chunks::ChunkKey;
use crate::pages::{Region, ShelfPacker};
use crate::sprites::Sprites;

/// Cells smaller than this on screen (device pixels) are drawn through impostors.
pub(crate) const IMPOSTOR_CELL_PX: f64 = 8.0;

/// The side of an impostor page.
const PAGE_SIDE: u32 = 2048;

/// A chunk's impostor at one resolution.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ImpostorKey {
    pub(crate) chunk: ChunkKey,
    /// Pixels per cell.
    pub(crate) level: u32,
}

/// An impostor drawn or planned.
#[derive(Clone, Debug)]
struct Impostor {
    sprite: u32,
    page: u32,
    last_used: u64,
    /// Drawn while some of its images were still being decoded: drawn again.
    complete: bool,
}

/// A picture to draw into the impostor pages: a chunk, the world rectangle it covers, the scale
/// and where it goes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ImpostorJob {
    pub(crate) chunk: ChunkKey,
    pub(crate) bounds: DRect,
    pub(crate) scale: f64,
    pub(crate) region: Region,
    key: ImpostorKey,
}

/// A page of impostors.
#[derive(Clone, Debug)]
struct ImpostorPage {
    packer: ShelfPacker,
    last_used: u64,
}

/// The impostor pages and what is in them.
pub(crate) struct Impostors {
    array_view: wgpu::TextureView,
    layer_views: Vec<wgpu::TextureView>,
    pages: Vec<ImpostorPage>,
    entries: HashMap<ImpostorKey, Impostor>,
    jobs: Vec<ImpostorJob>,
    /// Pages emptied for reuse, to clear before drawing into them.
    to_clear: Vec<u32>,
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
        Impostors {
            array_view,
            layer_views,
            pages: (0..count)
                .map(|_| ImpostorPage {
                    packer: ShelfPacker::new(side),
                    last_used: 0,
                })
                .collect(),
            entries: HashMap::new(),
            jobs: Vec::new(),
            to_clear: (0..count).collect(),
        }
    }

    pub(crate) fn array_view(&self) -> &wgpu::TextureView {
        &self.array_view
    }

    pub(crate) fn layer_view(&self, page: u32) -> Option<&wgpu::TextureView> {
        self.layer_views.get(page as usize)
    }

    /// The level for cells `cell_px` device pixels across: the smallest power of two at least as
    /// fine.
    pub(crate) fn level_for(cell_px: f64) -> u32 {
        (cell_px.max(1.0).ceil() as u32).next_power_of_two()
    }

    /// The sprite of a chunk's impostor, planning its drawing when it has none (or an
    /// incomplete one). `None` when no page has room this frame.
    pub(crate) fn sprite_for(
        &mut self,
        key: ImpostorKey,
        bounds: DRect,
        tile_px: f64,
        sprites: &mut Sprites,
        frame: u64,
    ) -> Option<u32> {
        if let Some(entry) = self.entries.get_mut(&key)
            && entry.complete
        {
            entry.last_used = frame;
            if let Some(page) = self.pages.get_mut(entry.page as usize) {
                page.last_used = frame;
            }
            return Some(entry.sprite);
        }
        let scale = f64::from(key.level) / tile_px.max(1.0);
        let size = (bounds.size() * scale)
            .ceil()
            .max(glam::DVec2::ONE)
            .as_uvec2();
        let region = self.allocate(size, sprites, frame)?;
        let sprite = sprites.impostor_sprite(region);
        if let Some(old) = self.entries.insert(
            key.clone(),
            Impostor {
                sprite,
                page: region.page,
                last_used: frame,
                complete: true,
            },
        ) {
            sprites.free_sprite(old.sprite);
        }
        self.jobs.push(ImpostorJob {
            chunk: key.chunk.clone(),
            bounds,
            scale,
            region,
            key,
        });
        Some(sprite)
    }

    /// A place on a page with room, or on the least recently used page not drawn from in
    /// `frame`, emptied.
    fn allocate(&mut self, size: UVec2, sprites: &mut Sprites, frame: u64) -> Option<Region> {
        for (index, page) in (0u32..).zip(self.pages.iter_mut()) {
            if let Some(origin) = page.packer.allocate(size) {
                page.last_used = frame;
                return Some(Region {
                    page: index,
                    origin,
                    size,
                });
            }
        }
        let oldest = (0u32..)
            .zip(self.pages.iter())
            .filter(|(_, page)| page.last_used < frame)
            .min_by_key(|(_, page)| page.last_used)
            .map(|(index, _)| index)?;
        let gone: Vec<ImpostorKey> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.page == oldest)
            .map(|(key, _)| key.clone())
            .collect();
        for key in gone {
            if let Some(entry) = self.entries.remove(&key) {
                sprites.free_sprite(entry.sprite);
            }
        }
        self.to_clear.push(oldest);
        let page = self.pages.get_mut(oldest as usize)?;
        page.packer.clear();
        page.last_used = frame;
        page.packer.allocate(size).map(|origin| Region {
            page: oldest,
            origin,
            size,
        })
    }

    /// The pictures to draw this frame and the pages to clear first.
    pub(crate) fn take_work(&mut self) -> ImpostorWork {
        ImpostorWork {
            clear: std::mem::take(&mut self.to_clear),
            jobs: std::mem::take(&mut self.jobs),
        }
    }

    /// Marks the impostors of `jobs` as drawn while images were missing, to draw them again.
    pub(crate) fn mark_incomplete(&mut self, jobs: &[ImpostorJob]) {
        for job in jobs {
            if let Some(entry) = self.entries.get_mut(&job.key) {
                entry.complete = false;
            }
        }
    }

    /// The impostors in use.
    pub(crate) fn count(&self) -> usize {
        self.entries.len()
    }
}

/// What to draw into the impostor pages before the frame.
#[derive(Clone, Debug, Default)]
pub(crate) struct ImpostorWork {
    pub(crate) clear: Vec<u32>,
    pub(crate) jobs: Vec<ImpostorJob>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_meet_the_screen() {
        assert_eq!(Impostors::level_for(0.3), 1);
        assert_eq!(Impostors::level_for(2.0), 2);
        assert_eq!(Impostors::level_for(5.5), 8);
    }
}
