//! Text, laid out with fontdue in Ubuntu Light (egui's own font, so canvas text matches the
//! panels). Each glyph is rasterised once per size and style, packed into the texture pages and
//! drawn as a sprite in the text's colour; underlines and strike-outs are solid quads. Bold is
//! the glyph thickened and italic the glyph sheared, since the binary carries one face.

use std::sync::OnceLock;

use bt_core::{Color, HAlign, TextData, VAlign};
use bt_utils::DRect;
use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle as LayoutText};
use fontdue::{Font, FontSettings};
use glam::{DVec2, UVec2};

/// The font, loaded on first use; `None` when the bundled bytes do not parse (never expected:
/// text is then left out and the log says why).
fn font() -> Option<&'static Font> {
    static FONT: OnceLock<Option<Font>> = OnceLock::new();
    FONT.get_or_init(|| {
        Font::from_bytes(epaint_default_fonts::UBUNTU_LIGHT, FontSettings::default())
            .map_err(|error| log::error!("the bundled font cannot be read: {error}"))
            .ok()
    })
    .as_ref()
}

/// The smallest and largest sizes glyphs are rasterised at, in device pixels.
const RASTER_PX: std::ops::RangeInclusive<f32> = 4.0..=256.0;

/// How text is sized.
#[derive(Copy, Clone, Debug, PartialEq)]
enum TextSize {
    /// Pixels of the world: the text grows with the zoom (text objects).
    World(f32),
    /// Device pixels: the same at any zoom (labels).
    Screen(f32),
}

/// Bold, italic, underlined, struck out.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextStyle {
    /// Thickened.
    pub bold: bool,
    /// Sheared.
    pub italic: bool,
    /// With a line under each line of text.
    pub underline: bool,
    /// With a line through each line of text.
    pub strikeout: bool,
}

/// Text to draw: what it says, where, how big and how it is aligned.
#[derive(Clone, Debug, PartialEq)]
pub struct TextItem {
    text: String,
    origin: DVec2,
    box_size: Option<DVec2>,
    size: TextSize,
    color: Color,
    halign: HAlign,
    valign: VAlign,
    wrap: bool,
    style: TextStyle,
    rotation_degrees: f64,
    pivot: DVec2,
}

impl TextItem {
    /// Black text 16 world pixels high with its top-left at `origin`.
    pub fn new(text: impl Into<String>, origin: DVec2) -> TextItem {
        TextItem {
            text: text.into(),
            origin,
            box_size: None,
            size: TextSize::World(16.0),
            color: Color::BLACK,
            halign: HAlign::Left,
            valign: VAlign::Top,
            wrap: false,
            style: TextStyle::default(),
            rotation_degrees: 0.0,
            pivot: origin,
        }
    }

    /// A Tiled text object's text in its rectangle, turned with the object about `pivot`.
    pub fn from_text_object(
        data: &TextData,
        rect: DRect,
        rotation_degrees: f64,
        pivot: DVec2,
    ) -> TextItem {
        TextItem::new(data.text.clone(), rect.min)
            .size(data.pixel_size as f32)
            .boxed(rect.size())
            .color(data.color)
            .align(data.halign, data.valign)
            .wrap(data.wrap)
            .style(TextStyle {
                bold: data.bold,
                italic: data.italic,
                underline: data.underline,
                strikeout: data.strikeout,
            })
            .rotation(rotation_degrees, pivot)
    }

    /// The height of the font in world pixels.
    pub fn size(mut self, px: f32) -> TextItem {
        self.size = TextSize::World(px);
        self
    }

    /// The height of the font in device pixels, whatever the zoom; the origin stays where it is
    /// in the world.
    pub fn screen_size(mut self, px: f32) -> TextItem {
        self.size = TextSize::Screen(px);
        self
    }

    /// The box the text is aligned (and wrapped) in, from the origin, in world pixels. Without
    /// one, the alignment places the text around the origin: centred on it, or with its right or
    /// bottom edge on it.
    pub fn boxed(mut self, size: DVec2) -> TextItem {
        self.box_size = Some(size);
        self
    }

    /// The colour.
    pub fn color(mut self, color: Color) -> TextItem {
        self.color = color;
        self
    }

    /// The alignment; `Justify` is drawn as `Left`.
    pub fn align(mut self, halign: HAlign, valign: VAlign) -> TextItem {
        self.halign = halign;
        self.valign = valign;
        self
    }

    /// Whether lines wrap at the box's width.
    pub fn wrap(mut self, wrap: bool) -> TextItem {
        self.wrap = wrap;
        self
    }

    /// Bold, italic, underline and strike-out.
    pub fn style(mut self, style: TextStyle) -> TextItem {
        self.style = style;
        self
    }

    /// Moves the text and its pivot by `by` world pixels.
    pub(crate) fn move_by(&mut self, by: DVec2) {
        self.origin += by;
        self.pivot += by;
    }

    /// Turns the text clockwise about `pivot` (world pixels).
    pub fn rotation(mut self, degrees: f64, pivot: DVec2) -> TextItem {
        self.rotation_degrees = degrees;
        self.pivot = pivot;
        self
    }
}

/// A glyph as rasterised: which one, at which size in device pixels, in which style.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct GlyphKey {
    pub(crate) glyph: u16,
    pub(crate) px: u16,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
}

/// A glyph placed in the world: the rectangle its bitmap covers.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct PlacedGlyph {
    pub(crate) key: GlyphKey,
    pub(crate) rect: DRect,
}

/// Text laid out for one zoom.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LaidOutText {
    pub(crate) glyphs: Vec<PlacedGlyph>,
    /// Underlines and strike-outs, as rectangles.
    pub(crate) rules: Vec<DRect>,
    pub(crate) color: Color,
    pub(crate) rotation_radians: f64,
    pub(crate) pivot: DVec2,
}

impl LaidOutText {
    /// Nothing to draw.
    fn nothing(item: &TextItem) -> LaidOutText {
        LaidOutText {
            glyphs: Vec::new(),
            rules: Vec::new(),
            color: item.color,
            rotation_radians: item.rotation_degrees.to_radians(),
            pivot: item.pivot,
        }
    }
}

/// The units a text is laid out in: its nominal size in them, what one is in world pixels, and
/// how many device pixels high the font is drawn.
struct LayoutUnits {
    nominal: f32,
    to_world: f64,
    device_px: f64,
}

impl LayoutUnits {
    /// World pixels for world-sized text, device pixels for screen-sized text.
    fn of(size: TextSize, zoom: f64) -> LayoutUnits {
        match size {
            TextSize::World(px) => LayoutUnits {
                nominal: px.max(1.0),
                to_world: 1.0,
                device_px: f64::from(px) * zoom,
            },
            TextSize::Screen(px) => LayoutUnits {
                nominal: px.max(1.0),
                to_world: 1.0 / zoom,
                device_px: f64::from(px),
            },
        }
    }
}

/// A rasterised glyph: straight RGBA, white with the coverage as alpha.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GlyphBitmap {
    pub(crate) size: UVec2,
    pub(crate) rgba: Vec<u8>,
}

/// How much a style widens a glyph bitmap of `height` pixels rasterised at `px`.
struct Widening {
    bold: u32,
    italic: u32,
}

impl Widening {
    fn of(key: GlyphKey, height: u32) -> Widening {
        Widening {
            bold: if key.bold {
                (u32::from(key.px) / 16).max(1)
            } else {
                0
            },
            italic: if key.italic {
                (f64::from(height) * ITALIC_SLANT).ceil() as u32
            } else {
                0
            },
        }
    }
}

/// The shear of italic glyphs: pixels right per pixel up.
const ITALIC_SLANT: f64 = 0.2;

/// Lays out `item` for a view at `zoom` device pixels per world pixel.
pub(crate) fn lay_out(item: &TextItem, zoom: f64) -> LaidOutText {
    let Some(font) = font() else {
        return LaidOutText::nothing(item);
    };
    let zoom = zoom.max(1e-9);
    let units = LayoutUnits::of(item.size, zoom);
    let nominal = units.nominal;
    let unit_to_world = units.to_world;
    let raster_px = (units.device_px as f32)
        .round()
        .clamp(*RASTER_PX.start(), *RASTER_PX.end());
    let scale = f64::from(raster_px) / f64::from(nominal);
    let box_units = item.box_size.map(|size| size / unit_to_world);
    let mut layout: Layout = Layout::new(CoordinateSystem::PositiveYDown);
    layout.reset(&LayoutSettings {
        max_width: box_units.filter(|_| item.wrap).map(|size| size.x as f32),
        ..LayoutSettings::default()
    });
    layout.append(&[font], &LayoutText::new(&item.text, nominal, 0));
    let Some(lines) = layout.lines() else {
        return LaidOutText::nothing(item);
    };
    let glyphs = layout.glyphs();
    let line_height = f64::from(
        font.horizontal_line_metrics(nominal)
            .map_or(nominal * 1.2, |m| m.new_line_size),
    );
    let block_height = line_height * lines.len() as f64;
    let mut out = LaidOutText::nothing(item);
    let align_x = match item.halign {
        HAlign::Left | HAlign::Justify => 0.0,
        HAlign::Center => 0.5,
        HAlign::Right => 1.0,
    };
    let align_y = match item.valign {
        VAlign::Top => 0.0,
        VAlign::Center => 0.5,
        VAlign::Bottom => 1.0,
    };
    // Where the block's top-left goes, in layout units from the origin.
    let block_top = match box_units {
        Some(size) => (size.y - block_height) * align_y,
        None => -block_height * align_y,
    };
    for line in lines {
        let line_glyphs = glyphs.get(line.glyph_start..=line.glyph_end).unwrap_or(&[]);
        let line_width = line_glyphs
            .iter()
            .map(|g| {
                let m = font.metrics_indexed(g.key.glyph_index, nominal);
                f64::from(g.x) - f64::from(m.xmin) + f64::from(m.advance_width)
            })
            .fold(0.0f64, f64::max);
        let line_left = match box_units {
            Some(size) => (size.x - line_width) * align_x,
            None => -line_width * align_x,
        };
        let baseline = block_top + f64::from(line.baseline_y);
        for glyph in line_glyphs {
            if glyph.parent.is_whitespace() || glyph.width == 0 || glyph.height == 0 {
                continue;
            }
            let at_nominal = font.metrics_indexed(glyph.key.glyph_index, nominal);
            let pen_x = f64::from(glyph.x) - f64::from(at_nominal.xmin);
            let raster = font.metrics_indexed(glyph.key.glyph_index, raster_px);
            let key = GlyphKey {
                glyph: glyph.key.glyph_index,
                px: raster_px as u16,
                bold: item.style.bold,
                italic: item.style.italic,
            };
            let widening = Widening::of(key, raster.height as u32);
            let width = (raster.width as u32 + widening.bold + widening.italic) as f64 / scale;
            let height = raster.height as f64 / scale;
            let left = pen_x + f64::from(raster.xmin) / scale;
            let top = baseline - f64::from(raster.ymin + raster.height as i32) / scale;
            let min = item.origin + DVec2::new(line_left + left, top) * unit_to_world;
            out.glyphs.push(PlacedGlyph {
                key,
                rect: DRect::from_min_size(min, DVec2::new(width, height) * unit_to_world),
            });
        }
        let thickness = (f64::from(nominal) / 14.0).max(1.0 / scale);
        let rule = |y: f64| {
            DRect::from_min_size(
                item.origin + DVec2::new(line_left, y) * unit_to_world,
                DVec2::new(line_width, thickness) * unit_to_world,
            )
        };
        if item.style.underline {
            out.rules.push(rule(baseline + f64::from(nominal) * 0.1));
        }
        if item.style.strikeout {
            out.rules.push(rule(baseline - f64::from(nominal) * 0.3));
        }
    }
    out
}

/// Rasterises a glyph: white, its coverage as alpha, thickened for bold and sheared for italic.
pub(crate) fn rasterize(key: GlyphKey) -> Option<GlyphBitmap> {
    let font = font()?;
    let (metrics, coverage) = font.rasterize_indexed(key.glyph, f32::from(key.px));
    let width = u32::try_from(metrics.width).ok()?;
    let height = u32::try_from(metrics.height).ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    let widening = Widening::of(key, height);
    let out_width = width + widening.bold + widening.italic;
    let mut alpha = vec![0u8; (out_width * height) as usize];
    for y in 0..height {
        // Rows nearer the top move further right.
        let shear = if key.italic {
            ((f64::from(height - 1 - y)) * ITALIC_SLANT).round() as u32
        } else {
            0
        };
        for x in 0..width {
            let value = coverage.get((y * width + x) as usize).copied().unwrap_or(0);
            for thicken in 0..=widening.bold {
                let target = (y * out_width + x + shear + thicken) as usize;
                if let Some(cell) = alpha.get_mut(target) {
                    *cell = (*cell).max(value);
                }
            }
        }
    }
    Some(GlyphBitmap {
        size: UVec2::new(out_width, height),
        rgba: alpha.iter().flat_map(|&a| [255, 255, 255, a]).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_lays_out_in_its_box_and_rasterises() {
        let item = TextItem::new("Hi there", DVec2::new(100.0, 50.0))
            .size(20.0)
            .boxed(DVec2::new(200.0, 100.0))
            .align(HAlign::Right, VAlign::Bottom);
        let laid = lay_out(&item, 1.0);
        assert_eq!(laid.glyphs.len(), 7, "the space has no glyph");
        let bounds =
            DRect::from_points(laid.glyphs.iter().flat_map(|g| [g.rect.min, g.rect.max])).unwrap();
        assert!(bounds.max.x <= 300.5 && bounds.max.x > 280.0, "{bounds:?}");
        assert!(bounds.max.y <= 150.5 && bounds.max.y > 130.0, "{bounds:?}");
        // Zooming in rasterises bigger glyphs at the same world places.
        let zoomed = lay_out(&item, 2.0);
        assert_eq!(zoomed.glyphs[0].key.px, 40);
        assert!((zoomed.glyphs[0].rect.min - laid.glyphs[0].rect.min).length() < 0.6);
        // Screen-sized labels keep their device size.
        let label = TextItem::new("A", DVec2::ZERO).screen_size(12.0);
        let far = lay_out(&label, 0.25);
        assert_eq!(far.glyphs[0].key.px, 12);
        assert!(
            far.glyphs[0].rect.size().y > 20.0,
            "{:?}",
            far.glyphs[0].rect
        );
        let bitmap = rasterize(laid.glyphs[0].key).unwrap();
        assert_eq!(
            bitmap.rgba.len(),
            (bitmap.size.x * bitmap.size.y * 4) as usize
        );
        assert!(bitmap.rgba.chunks(4).any(|p| p[3] == 255));
    }

    #[test]
    fn styles_widen_glyphs_and_add_rules() {
        let plain = GlyphKey {
            glyph: font().unwrap().lookup_glyph_index('l'),
            px: 32,
            bold: false,
            italic: false,
        };
        let bold = rasterize(GlyphKey {
            bold: true,
            ..plain
        })
        .unwrap();
        let italic = rasterize(GlyphKey {
            italic: true,
            ..plain
        })
        .unwrap();
        let regular = rasterize(plain).unwrap();
        assert_eq!(bold.size.x, regular.size.x + 2);
        assert!(italic.size.x > regular.size.x);
        let item = TextItem::new("ab\ncd", DVec2::ZERO).style(TextStyle {
            underline: true,
            strikeout: true,
            ..TextStyle::default()
        });
        assert_eq!(lay_out(&item, 1.0).rules.len(), 4);
    }
}
