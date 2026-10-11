//! `bt render` (`10-agents.md`, §10.3): a numbered picture of the map through `bt-render`'s
//! headless path, so what an agent sees can be told apart by cell. Cell numbers run along the top
//! and left, with a grid line every five cells (every cell with `grid`, neither when `clean`);
//! the picture is at most 2048 pixels a side unless a scale says otherwise (up to 8); cells of a
//! highlight (a shape, or what an edit changed) are outlined in red, and marks numbered in their
//! colours. Maps that are not orthogonal are drawn in their own projection with their cells
//! outlined and `x,y` written inside every fifth cell. Shapes take their classes' colours from
//! the project.
//!
//! Writing the file is the caller's: the outcome comes with the picture's pixels.

use std::collections::BTreeMap;
use std::sync::Arc;

use bt_core::{ClassUsage, Color, CustomTypes, HAlign, LayerId, Map, Orientation, VAlign};
use bt_image::DecodedImage;
use bt_render::{HeadlessDevice, ObjectStyle, Render, TextItem, VectorItem};
use bt_utils::text::plural;
use bt_utils::{DRect, IRect};
use glam::{DVec2, IVec2, UVec2};
use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::layer_tree::{is_in, layers_top_first};
use crate::shapes::map_cells;
use crate::words::{number_text, region_text};

/// The longest side of a picture without a scale given.
const MOST_SIDE: f64 = 2048.0;

/// The largest scale.
const MOST_SCALE: f64 = 8.0;

/// Behind cells without tiles, unless the map has a background colour of its own.
const BACKDROP: Color = Color::rgb(48, 48, 52);

/// The colour of a highlight.
const HIGHLIGHT: Color = Color::rgb(255, 40, 40);

/// The height of ruler numbers, in picture pixels.
const RULER_TEXT: f32 = 11.0;

/// The height of numbers inside cells, in picture pixels.
const CELL_TEXT: f32 = 10.0;

/// Grid lines: every fifth cell, every cell, or none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GridLines {
    /// A line every five cells.
    Fives,
    /// A line around every cell.
    Cells,
    /// No grid and no numbers: the map alone.
    Clean,
}

/// Cells outlined in a colour, with a number and a label: one of the editor's marks.
#[derive(Clone, Debug, PartialEq)]
pub struct PictureMark {
    /// The cells.
    pub cells: Vec<IVec2>,
    /// The colour.
    pub color: Color,
    /// The label; empty for none.
    pub label: String,
}

/// What `bt render` draws.
pub struct RenderRequest<'a> {
    map: &'a Map,
    file: String,
    region: Option<IRect>,
    scale: Option<f64>,
    grid: GridLines,
    highlight: Vec<IVec2>,
    marks: Vec<PictureMark>,
    layers: Vec<LayerId>,
    custom_types: Arc<CustomTypes>,
}

impl<'a> RenderRequest<'a> {
    /// The whole map, for the file `file`, a grid line every five cells.
    pub fn new(map: &'a Map, file: impl Into<String>) -> RenderRequest<'a> {
        RenderRequest {
            map,
            file: file.into(),
            region: None,
            scale: None,
            grid: GridLines::Fives,
            highlight: Vec::new(),
            marks: Vec::new(),
            layers: Vec::new(),
            custom_types: Arc::new(CustomTypes::default()),
        }
    }

    /// Only these cells.
    pub fn region(mut self, region: IRect) -> RenderRequest<'a> {
        self.region = Some(region);
        self
    }

    /// Picture pixels per map pixel, above 0 and up to 8.
    pub fn scale(mut self, scale: f64) -> RenderRequest<'a> {
        self.scale = Some(scale);
        self
    }

    /// The grid lines and numbers.
    pub fn grid(mut self, grid: GridLines) -> RenderRequest<'a> {
        self.grid = grid;
        self
    }

    /// Cells outlined in red: a highlight, or what an edit changed.
    pub fn highlight(mut self, cells: Vec<IVec2>) -> RenderRequest<'a> {
        self.highlight = cells;
        self
    }

    /// Marks numbered in their colours, in order.
    pub fn marks(mut self, marks: Vec<PictureMark>) -> RenderRequest<'a> {
        self.marks = marks;
        self
    }

    /// Only these layers (and those in them), shown whatever their visibility.
    pub fn layers(mut self, layers: Vec<LayerId>) -> RenderRequest<'a> {
        self.layers = layers;
        self
    }

    /// The project's custom types: shapes take their classes' colours.
    pub fn custom_types(mut self, types: Arc<CustomTypes>) -> RenderRequest<'a> {
        self.custom_types = types;
        self
    }
}

/// `bt render MAP OUT.png`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RenderOutcome {
    /// The file the picture is for.
    pub file: String,
    /// Its size in pixels.
    pub size: UVec2,
    /// The cells it shows.
    pub region: IRect,
    /// How many pixels a cell is wide in it.
    pub cell_px: f64,
    /// The grid drawn.
    pub grid: GridLines,
    /// How the cells are drawn, when the map is not orthogonal: `diamond`, `hexagon`.
    pub projection: Option<String>,
    /// How many cells are outlined in red.
    pub highlighted: usize,
    /// The marks drawn, numbered from 1.
    pub marks: Vec<MarkDrawn>,
}

/// A mark drawn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkDrawn {
    /// Its number in the picture.
    pub number: usize,
    /// Its label.
    pub label: String,
    /// How many cells it outlines.
    pub cells: usize,
}

/// A picture and what `bt render` says of it.
#[derive(Clone, Debug)]
pub struct Rendered {
    /// What `bt render` reports.
    pub outcome: RenderOutcome,
    /// The picture, to write.
    pub picture: DecodedImage,
}

impl RenderOutcome {
    /// The picture `request` asks for, drawn on `device`.
    pub fn from_request(
        request: &RenderRequest<'_>,
        device: &HeadlessDevice,
    ) -> Result<Rendered, Error> {
        if let Some(scale) = request
            .scale
            .filter(|s| !(*s > 0.0 && *s <= MOST_SCALE && s.is_finite()))
        {
            return Err(Error::RenderScale {
                scale: number_text(scale),
            });
        }
        let source = request.map;
        let full = map_cells(source);
        let region = request.region.unwrap_or(full);
        let orthogonal = source.orientation == Orientation::Orthogonal;
        let mut map = shown_layers(source, &request.layers);
        // Other grids are cropped to the region, so cells around it stay out of the picture.
        let shift = if !orthogonal && region != full {
            bt_edit::crop_to(&mut map, region).map_err(|error| Error::NoPicture {
                why: error.to_string(),
            })?;
            region.min()
        } else {
            IVec2::ZERO
        };
        let geometry = map.geometry();
        let world = if orthogonal {
            let tile = map.tile_size().as_dvec2();
            DRect::from_min_size(
                region.min().as_dvec2() * tile,
                region.size().as_dvec2() * tile,
            )
        } else {
            let bounds = map.bounding_rect();
            DRect::from_min_size(bounds.min().as_dvec2(), bounds.size().as_dvec2())
        };
        let longest = world.size().max_element().max(1.0);
        let scale = request
            .scale
            .unwrap_or_else(|| (MOST_SIDE / longest).min(1.0));
        let rulers = orthogonal && request.grid != GridLines::Clean;
        let margin = if rulers {
            ruler_margin(region)
        } else {
            DVec2::ZERO
        };
        let size = ((world.size() * scale).round() + margin).max(DVec2::ONE);
        let origin = world.min - margin / scale;
        let mut vectors: Vec<VectorItem> = Vec::new();
        let local = |cell: IVec2| cell - shift;
        if orthogonal {
            if request.grid != GridLines::Clean {
                orthogonal_grid(&mut vectors, &map, region, request.grid);
            }
        } else if request.grid != GridLines::Clean {
            projected_grid(&mut vectors, &geometry, world, request.grid, scale, shift);
        }
        let highlighted: Vec<IVec2> = request
            .highlight
            .iter()
            .copied()
            .filter(|c| region.contains(*c))
            .collect();
        for outline in geometry.region_outline(highlighted.iter().map(|&c| local(c))) {
            vectors.push(VectorItem::polygon(outline).stroke(HIGHLIGHT, 2.0));
        }
        let mut marks = Vec::new();
        for (number, mark) in (1..).zip(&request.marks) {
            let inside: Vec<IVec2> = mark
                .cells
                .iter()
                .copied()
                .filter(|c| region.contains(*c))
                .map(local)
                .collect();
            for outline in geometry.region_outline(inside.iter().copied()) {
                vectors.push(VectorItem::polygon(outline).stroke(mark.color, 2.0));
            }
            if let Some(first) = inside.iter().min_by_key(|c| (c.y, c.x)) {
                let at = centroid(&geometry.tile_polygon(*first));
                label(
                    &mut vectors,
                    at,
                    &number.to_string(),
                    mark.color,
                    Color::BLACK,
                    scale,
                );
            }
            marks.push(MarkDrawn {
                number,
                label: mark.label.clone(),
                cells: mark.cells.len(),
            });
        }
        if rulers {
            ruler_vectors(&mut vectors, &map, region, world, origin, scale);
        }
        let picture = Render::of(Arc::new(map))
            .view(origin, size.as_uvec2())
            .scale(scale)
            .background(source.background_color.unwrap_or(BACKDROP))
            .objects(
                ObjectStyle::default().class_colors(Arc::new(class_colors(&request.custom_types))),
            )
            .vectors(Arc::from(vectors))
            .run(device)
            .map_err(|error| Error::NoPicture {
                why: error.to_string(),
            })?;
        let outcome = RenderOutcome {
            file: request.file.clone(),
            size: UVec2::new(picture.width, picture.height),
            region,
            cell_px: f64::from(source.tile_width) * scale,
            grid: request.grid,
            projection: match source.orientation {
                Orientation::Orthogonal => None,
                Orientation::Isometric | Orientation::Staggered => Some("diamond".to_owned()),
                Orientation::Hexagonal => Some("hexagon".to_owned()),
                Orientation::Oblique => Some("parallelogram".to_owned()),
            },
            highlighted: highlighted.len(),
            marks,
        };
        Ok(Rendered {
            outcome,
            picture: DecodedImage {
                width: picture.width,
                height: picture.height,
                rgba: picture.rgba,
            },
        })
    }
}

/// The map with only `layers` (and the layers in them, and the groups they are in) shown, the
/// chosen ones shown whatever their visibility; every layer as it is when none is chosen.
fn shown_layers(map: &Map, layers: &[LayerId]) -> Map {
    let mut shown = map.clone();
    if layers.is_empty() {
        return shown;
    }
    let mut keep: Vec<LayerId> = Vec::new();
    for at in layers_top_first(map) {
        let id = at.layer.id;
        let chosen = layers.iter().any(|&wanted| is_in(map, id, wanted));
        let holds_chosen = layers.iter().any(|&wanted| is_in(map, wanted, id));
        if chosen || holds_chosen {
            keep.push(id);
        }
    }
    shown.for_each_layer_mut(|layer| {
        layer.visible = keep.contains(&layer.id);
    });
    shown
}

/// The colour of each object class the project defines.
fn class_colors(types: &CustomTypes) -> BTreeMap<String, Color> {
    types
        .types
        .iter()
        .filter_map(|t| {
            let class = t.as_class()?;
            class
                .use_as
                .intersects(ClassUsage::OBJECT)
                .then(|| (t.name.clone(), class.color))
        })
        .collect()
}

/// The room the rulers take, in picture pixels: across the left for the row numbers, down the
/// top for the column numbers.
fn ruler_margin(region: IRect) -> DVec2 {
    let widest = [region.y, region.y + region.h - 1]
        .iter()
        .map(|y| y.to_string().len())
        .max()
        .unwrap_or(1);
    DVec2::new(widest as f64 * 7.0 + 8.0, 16.0)
}

/// Grid lines of an orthogonal map: every fifth (or every) column and row of the region,
/// stronger every tenth.
fn orthogonal_grid(vectors: &mut Vec<VectorItem>, map: &Map, region: IRect, grid: GridLines) {
    let tile = map.tile_size().as_dvec2();
    let every = if grid == GridLines::Cells { 1 } else { 5 };
    let minor = Color::rgba(255, 255, 255, 60);
    let major = Color::rgba(255, 255, 255, 130);
    let top = f64::from(region.y) * tile.y;
    let bottom = f64::from(region.y + region.h) * tile.y;
    let left = f64::from(region.x) * tile.x;
    let right = f64::from(region.x + region.w) * tile.x;
    for x in region.x..=region.x + region.w {
        if x.rem_euclid(every) == 0 {
            let color = if x.rem_euclid(10) == 0 { major } else { minor };
            let at = f64::from(x) * tile.x;
            vectors.push(
                VectorItem::polyline(vec![DVec2::new(at, top), DVec2::new(at, bottom)])
                    .stroke(color, 1.0),
            );
        }
    }
    for y in region.y..=region.y + region.h {
        if y.rem_euclid(every) == 0 {
            let color = if y.rem_euclid(10) == 0 { major } else { minor };
            let at = f64::from(y) * tile.y;
            vectors.push(
                VectorItem::polyline(vec![DVec2::new(left, at), DVec2::new(right, at)])
                    .stroke(color, 1.0),
            );
        }
    }
}

/// The grid of a map that is not orthogonal, in its projection: the lines between every fifth
/// cell (every line with `Cells`), and `x,y` inside every fifth cell (every cell with `Cells`
/// when the numbers fit).
fn projected_grid(
    vectors: &mut Vec<VectorItem>,
    geometry: &bt_core::MapGeometry,
    world: DRect,
    grid: GridLines,
    scale: f64,
    shift: IVec2,
) {
    for line in geometry.grid_lines(world, IVec2::splat(5)) {
        let color = match (line.major, grid) {
            (true, _) => Color::rgba(255, 255, 255, 130),
            (false, GridLines::Cells) => Color::rgba(255, 255, 255, 60),
            (false, _) => continue,
        };
        vectors.push(VectorItem::polyline(vec![line.from, line.to]).stroke(color, 1.0));
    }
    let cell_px = f64::from(geometry.tile_width) * scale;
    let every = if grid == GridLines::Cells && cell_px >= 44.0 {
        1
    } else {
        5
    };
    let cells = IRect::from_min_size(IVec2::ZERO, IVec2::new(geometry.width, geometry.height));
    for local in cells.points() {
        let cell = local + shift;
        if cell.x.rem_euclid(every) != 0 || cell.y.rem_euclid(every) != 0 {
            continue;
        }
        let at = centroid(&geometry.tile_polygon(local));
        label(
            vectors,
            at,
            &format!("{},{}", cell.x, cell.y),
            Color::rgba(0, 0, 0, 160),
            Color::WHITE,
            scale,
        );
    }
}

/// The middle of a cell's outline.
fn centroid(points: &[DVec2]) -> DVec2 {
    let count = points.len().max(1) as f64;
    points.iter().copied().sum::<DVec2>() / count
}

/// Text on a box, centred on `at` (world pixels), the same size at any scale.
fn label(
    vectors: &mut Vec<VectorItem>,
    at: DVec2,
    text: &str,
    back: Color,
    fore: Color,
    scale: f64,
) {
    let width = (text.chars().count() as f64 * f64::from(CELL_TEXT) * 0.6 + 6.0) / scale;
    let height = (f64::from(CELL_TEXT) + 4.0) / scale;
    vectors.push(
        VectorItem::rect(DRect::from_min_size(
            at - DVec2::new(width, height) / 2.0,
            DVec2::new(width, height),
        ))
        .fill(back),
    );
    vectors.push(VectorItem::text(
        TextItem::new(text, at)
            .screen_size(CELL_TEXT)
            .color(fore)
            .align(HAlign::Center, VAlign::Center),
    ));
}

/// The rulers of an orthogonal picture: white bands along the top and left with the numbers of
/// the columns and rows, as many as fit apart.
fn ruler_vectors(
    vectors: &mut Vec<VectorItem>,
    map: &Map,
    region: IRect,
    world: DRect,
    origin: DVec2,
    scale: f64,
) {
    let tile = map.tile_size().as_dvec2();
    let white = Color::WHITE;
    vectors.push(
        VectorItem::rect(DRect::from_min_size(
            origin,
            DVec2::new(world.max.x - origin.x, world.min.y - origin.y),
        ))
        .fill(white),
    );
    vectors.push(
        VectorItem::rect(DRect::from_min_size(
            origin,
            DVec2::new(world.min.x - origin.x, world.max.y - origin.y),
        ))
        .fill(white),
    );
    let widest = [
        region.x,
        region.x + region.w - 1,
        region.y,
        region.y + region.h - 1,
    ]
    .iter()
    .map(|v| v.to_string().len())
    .max()
    .unwrap_or(1);
    let needed = widest as f64 * 7.0 + 6.0;
    let across = label_every(tile.x * scale, needed);
    let down = label_every(tile.y * scale, f64::from(RULER_TEXT) + 3.0);
    let band = (world.min.y - origin.y) / 2.0;
    for x in region.x..region.x + region.w {
        if x.rem_euclid(across) == 0 {
            let at = DVec2::new((f64::from(x) + 0.5) * tile.x, world.min.y - band);
            vectors.push(VectorItem::text(
                TextItem::new(x.to_string(), at)
                    .screen_size(RULER_TEXT)
                    .color(Color::BLACK)
                    .align(HAlign::Center, VAlign::Center),
            ));
        }
    }
    for y in region.y..region.y + region.h {
        if y.rem_euclid(down) == 0 {
            let at = DVec2::new(world.min.x - 3.0 / scale, (f64::from(y) + 0.5) * tile.y);
            vectors.push(VectorItem::text(
                TextItem::new(y.to_string(), at)
                    .screen_size(RULER_TEXT)
                    .color(Color::BLACK)
                    .align(HAlign::Right, VAlign::Center),
            ));
        }
    }
}

/// Number every n-th cell, n of 1, 2, 5, 10, 20, 50…, so numbers `needed` pixels wide don't
/// touch on cells `cell_px` pixels wide.
fn label_every(cell_px: f64, needed: f64) -> i32 {
    let mut every = 1;
    for n in [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000] {
        every = n;
        if f64::from(n) * cell_px >= needed {
            break;
        }
    }
    every
}

impl crate::report::Outcome for RenderOutcome {
    const SCHEMA: &'static str = "bt.render/1";

    fn report(&self) -> crate::report::Report {
        let mut report = crate::report::Report::new();
        let drawn = match (&self.projection, self.grid) {
            (_, GridLines::Clean) => String::new(),
            (None, _) => ", numbered along the top and left".to_owned(),
            (Some(cell), GridLines::Cells) => format!(
                ", each {cell} outlined and numbered x,y inside it (every 5th when the numbers don't fit: a larger --scale fits them)"
            ),
            (Some(cell), GridLines::Fives) => {
                format!(", every 5th {cell} outlined and numbered x,y inside it")
            }
        };
        report.add_line(crate::report::Line::fact(format!(
            "wrote {} ({}x{} px): cells {}, 1 cell = {} px{drawn}",
            self.file,
            self.size.x,
            self.size.y,
            region_text(self.region),
            number_text(self.cell_px)
        )));
        if self.highlighted > 0 {
            report.add_line(crate::report::Line::fact(format!(
                "{} outlined in red",
                plural(self.highlighted, "cell", "cells")
            )));
        }
        for mark in &self.marks {
            let label = if mark.label.is_empty() {
                String::new()
            } else {
                format!(" “{}”", mark.label)
            };
            report.add_line(crate::report::Line::fact(format!(
                "mark {}{label}: {}",
                mark.number,
                plural(mark.cells, "cell", "cells")
            )));
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{ClassType, CustomType, MapObject, Shape};
    use bt_test_data::fixtures::{Atlas, MapBuilder};

    use super::*;
    use crate::report::{Outcome, from_json, to_json};

    /// The headless device, or `None` (the test returns) when there is no GPU and skipping is on.
    fn device() -> Option<&'static HeadlessDevice> {
        match HeadlessDevice::acquire() {
            Ok(device) => Some(device),
            Err(error) => {
                bt_test_data::missing("a GPU adapter", &error.to_string());
                None
            }
        }
    }

    fn checkered() -> (Atlas, Map) {
        let atlas = Atlas::builder().name("ground").build_embedded().unwrap();
        let rows: Vec<String> = (0..8)
            .map(|y| {
                (0..12)
                    .map(|x| ((x + y) % 16).to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(12, 8))
            .tileset(Arc::clone(&atlas.tileset))
            .tile_layer("ground", &rows.join("\n"))
            .build()
            .unwrap();
        (atlas, map)
    }

    #[test]
    fn pictures_have_numbered_edges_and_a_clean_one_is_the_map_alone() {
        let Some(device) = device() else { return };
        let (atlas, map) = checkered();
        let region = IRect::from_min_size(IVec2::new(2, 1), IVec2::new(6, 4));
        let rendered = RenderOutcome::from_request(
            &RenderRequest::new(&map, "level.png")
                .region(region)
                .scale(2.0)
                .highlight(vec![IVec2::new(3, 2)]),
            device,
        )
        .unwrap();
        let margin = ruler_margin(region);
        assert_eq!(
            rendered.outcome.size,
            UVec2::new(6 * 32 + margin.x as u32, 4 * 32 + margin.y as u32)
        );
        assert_eq!(
            rendered.outcome.report().text(),
            format!(
                "wrote level.png ({}x{} px): cells 2,1 6x4, 1 cell = 32 px, numbered along the top and left\n\
                 1 cell outlined in red\n",
                rendered.outcome.size.x, rendered.outcome.size.y
            )
        );
        // The rulers are white; the highlighted cell's edge is red.
        let picture = &rendered.picture;
        assert_eq!(picture.pixel(1, 1), Some([255, 255, 255, 255]));
        let cell = (DVec2::new(1.0, 1.0) * 32.0 + margin).as_uvec2();
        let edge = picture.pixel(cell.x + 16, cell.y).unwrap();
        assert!(edge[0] > 200 && edge[1] < 90, "{edge:?}");
        assert_eq!(
            from_json::<RenderOutcome>(&to_json(&rendered.outcome).unwrap()).unwrap(),
            rendered.outcome
        );
        let clean = RenderOutcome::from_request(
            &RenderRequest::new(&map, "clean.png").grid(GridLines::Clean),
            device,
        )
        .unwrap();
        assert_eq!(clean.outcome.size, UVec2::new(12 * 16, 8 * 16));
        // Cell 0,0 holds tile 0 as drawn.
        assert_eq!(
            clean.picture.pixel(8, 8),
            atlas.tile_pixel(bt_core::TileId(0), UVec2::new(8, 8))
        );
        assert!(matches!(
            RenderOutcome::from_request(&RenderRequest::new(&map, "x.png").scale(9.0), device),
            Err(Error::RenderScale { .. })
        ));
    }

    #[test]
    fn shapes_take_their_classes_colours_from_the_project() {
        let Some(device) = device() else { return };
        let zone = MapObject {
            class: "zone".into(),
            shape: Shape::Rectangle,
            pos: DVec2::new(32.0, 16.0),
            size: DVec2::new(48.0, 32.0),
            ..MapObject::default()
        };
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(8, 6))
            .object_layer("design", vec![zone])
            .build()
            .unwrap();
        let mut types = CustomTypes::default();
        types.add(CustomType::class(
            "zone",
            ClassType {
                color: Color::rgb(16, 80, 240),
                draw_fill: false,
                use_as: ClassUsage::OBJECT,
                ..ClassType::default()
            },
        ));
        let rendered = RenderOutcome::from_request(
            &RenderRequest::new(&map, "zones.png")
                .grid(GridLines::Clean)
                .custom_types(Arc::new(types)),
            device,
        )
        .unwrap();
        // The zone's left edge, halfway down: the class's blue.
        let [r, g, b, _] = rendered.picture.pixel(32, 32).unwrap();
        assert!(b > 150 && r < 100 && g < 140, "{:?}", [r, g, b]);
    }

    #[test]
    fn grids_that_are_not_square_are_drawn_in_their_projection() {
        let Some(device) = device() else { return };
        let Some(examples) = bt_test_data::test_data("reference/tiled/examples") else {
            return;
        };
        let mut loader = bt_tmx::Loader::default();
        let iso = loader
            .load_map(&examples.join("isometric_grass_and_water.tmx"))
            .unwrap()
            .value;
        let rendered = RenderOutcome::from_request(
            &RenderRequest::new(&iso, "iso.png")
                .region(IRect::from_min_size(IVec2::new(5, 5), IVec2::new(6, 6)))
                .highlight(vec![IVec2::new(6, 6)]),
            device,
        )
        .unwrap();
        assert_eq!(rendered.outcome.projection.as_deref(), Some("diamond"));
        assert!(
            rendered
                .outcome
                .report()
                .text()
                .contains("every 5th diamond outlined and numbered x,y inside it"),
            "{}",
            rendered.outcome.report().text()
        );
        let hex = loader
            .load_map(&examples.join("hexagonal-mini.tmx"))
            .unwrap()
            .value;
        let rendered =
            RenderOutcome::from_request(&RenderRequest::new(&hex, "hex.png"), device).unwrap();
        assert_eq!(rendered.outcome.projection.as_deref(), Some("hexagon"));
        assert_eq!(
            rendered.outcome.region,
            IRect::from_min_size(IVec2::ZERO, IVec2::new(20, 20))
        );
    }
}
