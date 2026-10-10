//! The texture pages: one 2D-array texture of square RGBA8 pages that every image the frames
//! draw is packed into, by a shelf packer with a two-texel gutter (the image's edge repeated, so
//! minification never pulls in a neighbour). Each page has a second mip level at half size for
//! minified sampling. Pages are evicted least recently used under a byte budget; the texture
//! grows a page at a time and is recreated with its contents copied.

use glam::UVec2;

/// The texels around every packed image.
pub(crate) const GUTTER: u32 = 2;

/// Where an image is in the pages.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Region {
    /// The page (texture array layer).
    pub(crate) page: u32,
    /// The texel of the image's top-left at mip level 0 (even).
    pub(crate) origin: UVec2,
    /// The image's size.
    pub(crate) size: UVec2,
}

/// A shelf of a page: a row of images of about the same height.
#[derive(Copy, Clone, Debug)]
struct Shelf {
    y: u32,
    height: u32,
    x: u32,
}

/// Packs rectangles into one square page, shelf by shelf.
#[derive(Clone, Debug)]
pub(crate) struct ShelfPacker {
    side: u32,
    shelves: Vec<Shelf>,
    next_y: u32,
}

impl ShelfPacker {
    pub(crate) fn new(side: u32) -> ShelfPacker {
        ShelfPacker {
            side,
            shelves: Vec::new(),
            next_y: 0,
        }
    }

    /// The top-left of a free `size` rectangle, on the shelf that wastes least height or on a
    /// new one; `None` when the page is full.
    pub(crate) fn allocate(&mut self, size: UVec2) -> Option<UVec2> {
        if size.x == 0 || size.y == 0 || size.x > self.side || size.y > self.side {
            return None;
        }
        let side = self.side;
        let fitting = self
            .shelves
            .iter_mut()
            .filter(|shelf| {
                shelf.height >= size.y
                    && shelf.height <= size.y + size.y / 2 + 2
                    && shelf.x + size.x <= side
            })
            .min_by_key(|shelf| shelf.height);
        if let Some(shelf) = fitting {
            let origin = UVec2::new(shelf.x, shelf.y);
            shelf.x += size.x;
            return Some(origin);
        }
        if self.next_y + size.y > self.side {
            return None;
        }
        let origin = UVec2::new(0, self.next_y);
        self.shelves.push(Shelf {
            y: self.next_y,
            height: size.y,
            x: size.x,
        });
        self.next_y += size.y;
        Some(origin)
    }

    /// Forgets everything packed.
    pub(crate) fn clear(&mut self) {
        self.shelves.clear();
        self.next_y = 0;
    }
}

/// A page: its packer and the frame it was last drawn from.
#[derive(Clone, Debug)]
struct Page {
    packer: ShelfPacker,
    last_used: u64,
}

/// The pages and their texture.
pub(crate) struct Pages {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    side: u32,
    capacity: u32,
    max_layers: u32,
    pages: Vec<Page>,
    budget_bytes: usize,
    /// Bumped whenever the texture is replaced, so bind groups are rebuilt.
    generation: u64,
}

/// What [`Pages::allocate`] did beyond placing the image.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Allocation {
    /// The image's place, if it fits a page.
    pub(crate) region: Option<Region>,
    /// Pages emptied to make room: whatever was packed there must be packed again.
    pub(crate) evicted: Vec<u32>,
}

impl Pages {
    /// Pages of `side` texels (at most what the device allows), kept under `budget_bytes`.
    pub(crate) fn new(device: &wgpu::Device, side: u32, budget_bytes: usize) -> Pages {
        let limits = device.limits();
        let side = side.min(limits.max_texture_dimension_2d).max(64);
        let capacity = 1;
        let texture = create_texture(device, side, capacity);
        let view = array_view(&texture);
        Pages {
            texture,
            view,
            side,
            capacity,
            max_layers: limits.max_texture_array_layers.max(1),
            pages: Vec::new(),
            budget_bytes,
            generation: 0,
        }
    }

    /// The side of a page in texels.
    pub(crate) fn side(&self) -> u32 {
        self.side
    }

    /// The texture's array view.
    pub(crate) fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Bumped when the texture is replaced.
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// The pages in use.
    pub(crate) fn count(&self) -> usize {
        self.pages.len()
    }

    /// The memory a page takes, both mip levels.
    fn page_bytes(&self) -> usize {
        let texels = (self.side as usize) * (self.side as usize);
        texels * 4 + texels
    }

    /// Records that `page` is drawn from in `frame`.
    pub(crate) fn touch(&mut self, page: u32, frame: u64) {
        if let Some(page) = self.pages.get_mut(page as usize) {
            page.last_used = frame;
        }
    }

    /// A place for an image of `size`, on a page with room, on a new page, or on the least
    /// recently used page once the budget is reached (never one drawn from in `frame`).
    pub(crate) fn allocate(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: UVec2,
        frame: u64,
    ) -> Allocation {
        let padded = padded_size(size);
        let mut allocation = Allocation::default();
        if padded.x > self.side || padded.y > self.side {
            return allocation;
        }
        for (index, page) in (0u32..).zip(self.pages.iter_mut()) {
            if let Some(at) = page.packer.allocate(padded) {
                page.last_used = frame;
                allocation.region = Some(Region {
                    page: index,
                    origin: at + UVec2::splat(GUTTER),
                    size,
                });
                return allocation;
            }
        }
        let over_budget = (self.pages.len() + 1) * self.page_bytes() > self.budget_bytes;
        let full = self.pages.len() as u32 >= self.max_layers;
        if over_budget || full {
            let oldest = (0u32..)
                .zip(self.pages.iter())
                .filter(|(_, page)| page.last_used < frame)
                .min_by_key(|(_, page)| page.last_used)
                .map(|(index, _)| index);
            if let Some(index) = oldest
                && let Some(page) = self.pages.get_mut(index as usize)
            {
                page.packer.clear();
                page.last_used = frame;
                allocation.evicted.push(index);
                allocation.region = page.packer.allocate(padded).map(|at| Region {
                    page: index,
                    origin: at + UVec2::splat(GUTTER),
                    size,
                });
                return allocation;
            }
            if full {
                log::warn!(
                    "every texture page is drawn from this frame and the device allows no more"
                );
                return allocation;
            }
            log::debug!("texture pages over budget: every page is drawn from this frame");
        }
        let index = self.pages.len() as u32;
        self.ensure_capacity(device, queue, index + 1);
        let mut page = Page {
            packer: ShelfPacker::new(self.side),
            last_used: frame,
        };
        allocation.region = page.packer.allocate(padded).map(|at| Region {
            page: index,
            origin: at + UVec2::splat(GUTTER),
            size,
        });
        self.pages.push(page);
        allocation
    }

    /// Grows the texture to hold `layers` pages, copying the pages already there.
    fn ensure_capacity(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, layers: u32) {
        if layers <= self.capacity {
            return;
        }
        let capacity = (self.capacity * 2).max(layers).min(self.max_layers);
        let texture = create_texture(device, self.side, capacity);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("bt pages grow"),
        });
        for mip in 0..2 {
            let side = self.side >> mip;
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: mip,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: mip,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: side,
                    height: side,
                    depth_or_array_layers: self.capacity,
                },
            );
        }
        queue.submit([encoder.finish()]);
        self.view = array_view(&texture);
        self.texture = texture;
        self.capacity = capacity;
        self.generation += 1;
    }

    /// Copies `rgba` (straight alpha, `region.size`) into its region with its gutter, and its
    /// half-size version into the second mip level.
    pub(crate) fn upload(&self, queue: &wgpu::Queue, region: Region, rgba: &[u8]) {
        let padded = pad_with_edges(rgba, region.size);
        let at = region.origin - UVec2::splat(GUTTER);
        write(
            queue,
            &self.texture,
            0,
            region.page,
            at,
            padded.size,
            &padded.rgba,
        );
        let half = downsample(&padded.rgba, padded.size);
        write(
            queue,
            &self.texture,
            1,
            region.page,
            at / 2,
            half.size,
            &half.rgba,
        );
    }
}

/// The size an image takes in a page: with its gutter, rounded up to even sides so its half
/// lands on whole texels.
pub(crate) fn padded_size(size: UVec2) -> UVec2 {
    let with_gutter = size + UVec2::splat(2 * GUTTER);
    (with_gutter + UVec2::ONE) / 2 * 2
}

fn create_texture(device: &wgpu::Device, side: u32, layers: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bt pages"),
        size: wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: layers,
        },
        mip_level_count: 2,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn array_view(texture: &wgpu::Texture) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("bt pages view"),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..wgpu::TextureViewDescriptor::default()
    })
}

fn write(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    mip: u32,
    page: u32,
    at: UVec2,
    size: UVec2,
    rgba: &[u8],
) {
    if size.x == 0 || size.y == 0 {
        return;
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: mip,
            origin: wgpu::Origin3d {
                x: at.x,
                y: at.y,
                z: page,
            },
            aspect: wgpu::TextureAspect::All,
        },
        rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * size.x),
            rows_per_image: Some(size.y),
        },
        wgpu::Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
    );
}

/// Pixels and their size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Texels {
    pub(crate) size: UVec2,
    pub(crate) rgba: Vec<u8>,
}

/// The image with its edge repeated into the gutter (and one more row or column to make the
/// sides even).
pub(crate) fn pad_with_edges(rgba: &[u8], size: UVec2) -> Texels {
    let padded = padded_size(size);
    let mut out = vec![0u8; (padded.x * padded.y * 4) as usize];
    if size.x == 0 || size.y == 0 {
        return Texels {
            size: padded,
            rgba: out,
        };
    }
    for y in 0..padded.y {
        let sy = y.saturating_sub(GUTTER).min(size.y - 1);
        for x in 0..padded.x {
            let sx = x.saturating_sub(GUTTER).min(size.x - 1);
            let from = ((sy * size.x + sx) * 4) as usize;
            let to = ((y * padded.x + x) * 4) as usize;
            let Some(source) = rgba.get(from..from + 4) else {
                continue;
            };
            if let Some(target) = out.get_mut(to..to + 4) {
                target.copy_from_slice(source);
            }
        }
    }
    Texels {
        size: padded,
        rgba: out,
    }
}

/// Half the size, each texel the alpha-weighted mean of four (so transparent texels do not
/// darken their neighbours).
pub(crate) fn downsample(rgba: &[u8], size: UVec2) -> Texels {
    let half = size / 2;
    let mut out = vec![0u8; (half.x * half.y * 4) as usize];
    for y in 0..half.y {
        for x in 0..half.x {
            let mut sum = WeightedSum::default();
            for dy in 0..2 {
                for dx in 0..2 {
                    let at = (((2 * y + dy) * size.x + 2 * x + dx) * 4) as usize;
                    if let Some(&[r, g, b, a]) = rgba
                        .get(at..at + 4)
                        .and_then(|p| <&[u8; 4]>::try_from(p).ok())
                    {
                        let weight = u32::from(a);
                        sum.r += u32::from(r) * weight;
                        sum.g += u32::from(g) * weight;
                        sum.b += u32::from(b) * weight;
                        sum.alpha += weight;
                    }
                }
            }
            let to = ((y * half.x + x) * 4) as usize;
            if let Some(target) = out.get_mut(to..to + 4) {
                target.copy_from_slice(&sum.mean());
            }
        }
    }
    Texels {
        size: half,
        rgba: out,
    }
}

/// Four texels' colours weighted by their alpha, and their alpha.
#[derive(Copy, Clone, Debug, Default)]
struct WeightedSum {
    r: u32,
    g: u32,
    b: u32,
    alpha: u32,
}

impl WeightedSum {
    /// The mean texel.
    fn mean(self) -> [u8; 4] {
        let channel = |c: u32| {
            (c + self.alpha / 2)
                .checked_div(self.alpha)
                .map_or(0, |mean| u8::try_from(mean).unwrap_or(u8::MAX))
        };
        [
            channel(self.r),
            channel(self.g),
            channel(self.b),
            u8::try_from((self.alpha + 2) / 4).unwrap_or(u8::MAX),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packing_never_overlaps_and_fills_shelves() {
        let mut packer = ShelfPacker::new(256);
        let mut placed: Vec<(UVec2, UVec2)> = Vec::new();
        let sizes = [
            (36, 36),
            (20, 36),
            (100, 20),
            (36, 30),
            (200, 100),
            (60, 60),
            (36, 36),
        ];
        for &(w, h) in sizes.iter().cycle().take(40) {
            let size = UVec2::new(w, h);
            if let Some(at) = packer.allocate(size) {
                assert!(at.x + w <= 256 && at.y + h <= 256);
                for (other, other_size) in &placed {
                    let apart = at.x + w <= other.x
                        || other.x + other_size.x <= at.x
                        || at.y + h <= other.y
                        || other.y + other_size.y <= at.y;
                    assert!(apart, "{at} {size} overlaps {other} {other_size}");
                }
                placed.push((at, size));
            }
        }
        assert!(placed.len() > 10);
        assert_eq!(packer.allocate(UVec2::new(257, 1)), None);
        packer.clear();
        assert_eq!(packer.allocate(UVec2::new(256, 256)), Some(UVec2::ZERO));
    }

    #[test]
    fn gutters_repeat_edges_and_halves_weigh_alpha() {
        assert_eq!(padded_size(UVec2::new(3, 4)), UVec2::new(8, 8));
        // 1×1 red: the whole padded block is red.
        let padded = pad_with_edges(&[255, 0, 0, 255], UVec2::ONE);
        assert_eq!(padded.size, UVec2::new(6, 6));
        assert!(padded.rgba.chunks(4).all(|p| p == [255, 0, 0, 255]));
        // A transparent texel does not darken an opaque neighbour.
        let half = downsample(
            &[255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            UVec2::new(2, 2),
        );
        assert_eq!(half.rgba, vec![255, 255, 255, 64]);
    }
}
