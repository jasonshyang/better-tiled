//! `bt view` (`10-agents.md`, §10.2): the map as text, one character per cell, with column and
//! row rulers and a legend. A cell shows the top-most of the chosen tile layers that has
//! something there: by terrain (the default when a cell shows one) the terrain covering more
//! than half of the tile, so a painted area reads as exactly the cells painted, and an edge lets
//! the layer below show; tiles that are no terrain's as their layer's tiles; by tile each tile;
//! by layer each layer. Objects are capitals where they stand. Symbols keep their meaning in
//! every view of the map ([`crate::symbols`]).
//!
//! Views wider or taller than 160 cells show one character per square of cells, the square's
//! most common sight. On hexagonal and staggered maps whose rows are shifted, each cell takes
//! two characters and the shifted rows start one further right, so cells that touch are next to
//! each other in the text. The edit report views its changed area through [`ViewRequest::around`].

use std::collections::{BTreeMap, HashMap};

use bt_core::{Cell, Layer, LayerId, Map, MapObject, Orientation, StaggerAxis, StaggerIndex};
use bt_utils::IRect;
use bt_utils::text::plural;
use glam::{DVec2, IVec2};
use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::feet::{Feet, pixel_to_point, standing_cell};
use crate::info::object_kind;
use crate::layer_tree::{chosen_layers, is_shown, layers_top_first};
use crate::names::{Names, object_type};
use crate::report::{Line, Outcome, Report};
use crate::shapes::map_cells;
use crate::symbols::{NOTHING, Shown, Symbols, TOO_MANY, TerrainKey, TileKey, object_symbols};
use crate::tally::{Tally, counts_text};
use crate::words::{point_text, region_text};

/// Views wider or taller than this many cells show one character per square of cells.
const MOST_SIDE: i32 = 160;

/// The share of the map's cells one tile must fill on the bottom layer to be a backdrop, in
/// tenths.
const BACKDROP_TENTHS: usize = 9;

/// The objects of a group a legend line names before "and N more".
const OBJECTS_NAMED: usize = 12;

/// What a character of `bt view` stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewBy {
    /// The terrain covering more than half of the cell.
    Terrain,
    /// The kind of tile.
    Tile,
    /// The top layer with a tile there.
    Layer,
}

/// What `bt view` is asked to show.
pub struct ViewRequest<'a> {
    map: &'a Map,
    names: &'a Names,
    title: Option<String>,
    region: Option<IRect>,
    layers: Vec<LayerId>,
    by: Option<ViewBy>,
    runs: bool,
    objects: bool,
    step: Option<u32>,
}

impl<'a> ViewRequest<'a> {
    /// The whole map, every layer shown, by terrain when a cell shows one, objects marked.
    pub fn new(map: &'a Map, names: &'a Names) -> ViewRequest<'a> {
        ViewRequest {
            map,
            names,
            title: None,
            region: None,
            layers: Vec::new(),
            by: None,
            runs: false,
            objects: true,
            step: None,
        }
    }

    /// What the header calls the view: the map's file name, say.
    pub fn title(mut self, title: impl Into<String>) -> ViewRequest<'a> {
        self.title = Some(title.into());
        self
    }

    /// Only these cells.
    pub fn region(mut self, region: IRect) -> ViewRequest<'a> {
        self.region = Some(region);
        self
    }

    /// The area around `cells` (what an edit changed): their bounding rectangle grown by a cell
    /// on each side, on the map, called "the changed area".
    pub fn around(mut self, cells: &[IVec2]) -> ViewRequest<'a> {
        if let Some(bounds) = IRect::bounding(cells.iter().copied()) {
            let grown = IRect::from_min_size(bounds.min() - IVec2::ONE, bounds.size() + 2);
            self.region = Some(if self.map.infinite {
                grown
            } else {
                grown.intersection(map_cells(self.map))
            });
        }
        self.title = Some("the changed area".to_owned());
        self
    }

    /// Only these layers (and the layers in them, for groups).
    pub fn layers(mut self, layers: Vec<LayerId>) -> ViewRequest<'a> {
        self.layers = layers;
        self
    }

    /// What a character stands for.
    pub fn by(mut self, by: ViewBy) -> ViewRequest<'a> {
        self.by = Some(by);
        self
    }

    /// Each row's runs after it (`~14-29`).
    pub fn runs(mut self, runs: bool) -> ViewRequest<'a> {
        self.runs = runs;
        self
    }

    /// Objects left out.
    pub fn without_objects(mut self) -> ViewRequest<'a> {
        self.objects = false;
        self
    }

    /// One character per square of `step` cells a side.
    pub fn step(mut self, step: u32) -> ViewRequest<'a> {
        self.step = Some(step);
        self
    }
}

impl<'a> ViewRequest<'a> {
    /// The map viewed.
    pub(crate) fn map(&self) -> &'a Map {
        self.map
    }

    /// Its names.
    pub(crate) fn names(&self) -> &'a Names {
        self.names
    }

    /// The cells asked for, if any.
    pub(crate) fn region_asked(&self) -> Option<IRect> {
        self.region
    }

    /// The title asked for, if any.
    pub(crate) fn title_asked(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// The layers asked for; none for every layer shown.
    pub(crate) fn layers_asked(&self) -> &[LayerId] {
        &self.layers
    }
}

/// A view of the map as text: `bt view`, `--ids` and `--corners`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewOutcome {
    /// The view: a header, the rows with their rulers, and a legend.
    pub text: String,
}

impl Outcome for ViewOutcome {
    const SCHEMA: &'static str = "bt.view/1";

    fn report(&self) -> Report {
        let mut report = Report::new();
        for line in self.text.lines() {
            report.add_line(Line::fact(line));
        }
        report
    }
}

/// What a cell shows, found on a layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sight {
    shown: Shown,
    layer: LayerId,
    tile: Cell,
}

/// A tile filling the bottom layer (a sky, a backdrop), which shows as nothing.
#[derive(Clone, Copy, Debug)]
struct Backdrop {
    layer: LayerId,
    tile: Cell,
}

/// Objects marked by one capital: the sprites of a layer, or the objects of one type on it.
struct ObjectGroup {
    layer: LayerId,
    layer_name: String,
    sprites: bool,
    word: String,
    symbol: char,
}

/// An object marked in the view.
struct Mark {
    group: usize,
    cell: IVec2,
    entry: String,
}

impl ViewOutcome {
    /// The view `request` asks for.
    pub fn from_request(request: &ViewRequest<'_>) -> Result<ViewOutcome, Error> {
        let map = request.map;
        let region = request.region.unwrap_or_else(|| map_cells(map));
        let step = match request.step {
            Some(0) => return Err(Error::ZeroStep),
            Some(step) => i32::try_from(step).unwrap_or(i32::MAX),
            None => ((region.w.max(region.h) + MOST_SIDE - 1) / MOST_SIDE).max(1),
        };
        let chosen = chosen_layers(map, &request.layers);
        let tile_layers: Vec<&Layer> = chosen
            .iter()
            .copied()
            .filter(|layer| layer.as_tile_layer().is_some())
            .collect();
        let by = request
            .by
            .unwrap_or_else(|| default_by(map, request.names, &tile_layers, region));
        let view = Viewer {
            map,
            names: request.names,
            tile_layers: &tile_layers,
            backdrop: backdrop(map, request.names, &tile_layers, by),
            by,
        };
        let symbols = Symbols::of(map, request.names);
        let cols = (region.w + step - 1) / step;
        let rows = (region.h + step - 1) / step;
        let mut grid: Vec<char> = Vec::new();
        let mut counts: Tally = Tally::new();
        let mut seen: HashMap<Shown, SightCounts> = HashMap::new();
        for row in 0..rows {
            for col in 0..cols {
                let origin = region.min() + IVec2::new(col, row) * step;
                let sight = if step == 1 {
                    view.sight(origin)
                } else {
                    view.square_sight(origin, step, region)
                };
                let symbol = match sight {
                    Some(sight) => {
                        let symbol = symbols.of_shown(sight.shown);
                        counts.add_one(symbol.to_string());
                        seen.entry(sight.shown).or_default().add(&view, sight);
                        symbol
                    }
                    None => NOTHING,
                };
                grid.push(symbol);
            }
        }
        // Objects where they stand.
        let mut groups = object_groups(map, request.names);
        let marks = if request.objects && by != ViewBy::Layer {
            object_marks(map, request.names, &chosen, &mut groups)
        } else {
            Vec::new()
        };
        let mut marked = 0usize;
        let mut in_view: BTreeMap<usize, Vec<&Mark>> = BTreeMap::new();
        for mark in &marks {
            if !region.contains(mark.cell) {
                continue;
            }
            let local = (mark.cell - region.min()) / step;
            let index = usize::try_from(local.y * cols + local.x).ok();
            if let (Some(index), Some(group)) = (index, groups.get(mark.group))
                && let Some(slot) = grid.get_mut(index)
            {
                *slot = group.symbol;
                marked += 1;
                in_view.entry(mark.group).or_default().push(mark);
            }
        }
        let layout = Layout::of(map, region, step, cols);
        let mut text = header(request, map, region, step, &tile_layers, &layout);
        layout.write_rulers(&mut text);
        for (row, line) in (0..rows).zip(grid.chunks(usize::try_from(cols.max(1)).unwrap_or(1))) {
            let line: String = line.iter().collect();
            let y = region.y + row * step;
            text.push_str(&layout.row_text(y, &line));
            if request.runs {
                let runs = runs_text(&line, region.x, step);
                if !runs.is_empty() {
                    text.push_str(&format!("   {runs}"));
                }
            }
            text.push('\n');
        }
        let unit = if step > 1 { "squares" } else { "cells" };
        text.push_str("legend:\n");
        let mut nothing = format!("  {NOTHING}  nothing");
        if by == ViewBy::Terrain {
            nothing.push_str(", or only the edge of a terrain");
        }
        if let Some(backdrop) = view.backdrop {
            let layer = map.layer(backdrop.layer).map_or("", |l| l.name.as_str());
            nothing.push_str(&format!(
                ", or the backdrop of {layer} ({})",
                request.names.tile_text(backdrop.tile)
            ));
        }
        text.push_str(&nothing);
        text.push('\n');
        let mut legend: Vec<(Shown, &SightCounts)> = seen
            .iter()
            .map(|(shown, counts)| (*shown, counts))
            .collect();
        legend.sort_by(|a, b| b.1.total.cmp(&a.1.total).then_with(|| a.0.cmp(&b.0)));
        let mut beyond = Tally::new();
        for (shown, sights) in legend {
            let symbol = symbols.of_shown(shown);
            if symbol == TOO_MANY {
                beyond.add(format!("{shown:?}"), sights.total);
                continue;
            }
            text.push_str(&format!(
                "  {symbol}  {}: {}\n",
                legend_words(&view, shown, sights),
                plural(sights.total, unit.trim_end_matches('s'), unit)
            ));
        }
        if beyond.kinds() > 0 {
            let total: usize = beyond.sorted().iter().map(|c| c.count).sum();
            text.push_str(&format!(
                "  {TOO_MANY}  {} more kinds, {} (too many to give each a symbol: bt view MAP --ids --region R shows each cell's tile)\n",
                beyond.kinds(),
                plural(total, unit.trim_end_matches('s'), unit)
            ));
        }
        for (index, marks) in &in_view {
            let Some(group) = groups.get(*index) else {
                continue;
            };
            let count = if marks.len() > 1 {
                format!(" ×{}", marks.len())
            } else {
                String::new()
            };
            if group.sprites {
                let mut kinds = Tally::new();
                for mark in marks {
                    kinds.add_one(mark.entry.clone());
                }
                text.push_str(&format!(
                    "  {}  sprites on {}{count}: {}\n",
                    group.symbol,
                    group.layer_name,
                    counts_text(&kinds.sorted(), 4)
                ));
            } else {
                let entries: Vec<&str> = marks.iter().map(|m| m.entry.as_str()).collect();
                text.push_str(&format!(
                    "  {}  {}{count} on {}: {}\n",
                    group.symbol,
                    group.word,
                    group.layer_name,
                    crate::words::capped_list(&entries, OBJECTS_NAMED)
                ));
            }
        }
        let outside = marks.len().saturating_sub(marked);
        if outside > 0 {
            text.push_str(&format!(
                "  ({} of the objects stand outside the view)\n",
                outside
            ));
        }
        Ok(ViewOutcome { text })
    }
}

/// By terrain when a cell of the region shows a terrain on one of the layers, else by tile: a
/// map without terrains would read as one letter.
fn default_by(map: &Map, names: &Names, tile_layers: &[&Layer], region: IRect) -> ViewBy {
    let shows_terrain = tile_layers
        .iter()
        .filter_map(|l| l.as_tile_layer())
        .any(|tiles| {
            tiles
                .cells()
                .filter(|placed| region.contains(placed.at))
                .any(|placed| names.terrain_covering(map, placed.cell).is_some())
        });
    if shows_terrain {
        ViewBy::Terrain
    } else {
        ViewBy::Tile
    }
}

/// The plain tile (no terrain's) filling at least nine tenths of the map's cells on the bottom
/// one of `tile_layers`, a sky or a backdrop, which shows as nothing by terrain and by tile:
/// judged on the whole map, so views of its parts agree.
fn backdrop(map: &Map, names: &Names, tile_layers: &[&Layer], by: ViewBy) -> Option<Backdrop> {
    if by == ViewBy::Layer {
        return None;
    }
    let layer = tile_layers.last()?;
    let tiles = layer.as_tile_layer()?;
    let cells = map_cells(map);
    let area = usize::try_from(i64::from(cells.w.max(0)) * i64::from(cells.h.max(0))).ok()?;
    let mut counts: HashMap<Cell, usize> = HashMap::new();
    for placed in tiles.cells().filter(|placed| cells.contains(placed.at)) {
        *counts.entry(placed.cell).or_default() += 1;
    }
    let (tile, count) = counts
        .into_iter()
        .max_by_key(|(tile, count)| (*count, TileKey::of(*tile), tile.flip().bits()))?;
    let plain = names.terrain_covering(map, tile).is_none() && !is_terrain_tile(map, tile);
    (plain && area > 0 && count * 10 >= area * BACKDROP_TENTHS).then_some(Backdrop {
        layer: layer.id,
        tile,
    })
}

/// What finds a cell's sight.
struct Viewer<'v> {
    map: &'v Map,
    names: &'v Names,
    tile_layers: &'v [&'v Layer],
    backdrop: Option<Backdrop>,
    by: ViewBy,
}

impl Viewer<'_> {
    /// What the cell shows: the top-most layer with something there that counts.
    fn sight(&self, cell: IVec2) -> Option<Sight> {
        for layer in self.tile_layers {
            let Some(tiles) = layer.as_tile_layer() else {
                continue;
            };
            let tile = tiles.cell(cell);
            let Some(slot) = tile.slot() else {
                continue;
            };
            if self
                .backdrop
                .is_some_and(|b| b.layer == layer.id && b.tile == tile)
            {
                continue;
            }
            let shown = match self.by {
                ViewBy::Layer => Shown::Layer(layer.id),
                ViewBy::Tile => Shown::Tile(TileKey {
                    slot,
                    id: tile.tile(),
                }),
                ViewBy::Terrain => match self.names.terrain_covering(self.map, tile) {
                    Some(terrain) => Shown::Terrain(TerrainKey {
                        slot: terrain.slot,
                        set: terrain.set,
                        color: terrain.color.0,
                    }),
                    // A terrain's edge lets the layer below show.
                    None if is_terrain_tile(self.map, tile) => continue,
                    None => Shown::OtherTiles(layer.id),
                },
            };
            return Some(Sight {
                shown,
                layer: layer.id,
                tile,
            });
        }
        None
    }

    /// What a square of cells shows most.
    fn square_sight(&self, origin: IVec2, step: i32, region: IRect) -> Option<Sight> {
        let mut tally: Vec<(Sight, usize)> = Vec::new();
        for dy in 0..step {
            for dx in 0..step {
                let cell = origin + IVec2::new(dx, dy);
                if !region.contains(cell) {
                    continue;
                }
                if let Some(sight) = self.sight(cell) {
                    match tally.iter_mut().find(|(s, _)| s.shown == sight.shown) {
                        Some((_, count)) => *count += 1,
                        None => tally.push((sight, 1)),
                    }
                }
            }
        }
        tally
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.shown.cmp(&a.0.shown)))
            .map(|(sight, _)| sight)
    }
}

/// Whether some terrain set of its tileset puts a terrain on the tile.
fn is_terrain_tile(map: &Map, tile: Cell) -> bool {
    map.tileset_of(tile).is_some_and(|tileset| {
        tileset.wang_sets.iter().any(|set| {
            set.tiles
                .get(&tile.tile())
                .is_some_and(|label| !label.is_empty())
        })
    })
}

/// How often a sight was seen, and on which layers and as which tiles.
#[derive(Default)]
struct SightCounts {
    total: usize,
    layers: Tally,
    tiles: Tally,
}

impl SightCounts {
    fn add(&mut self, view: &Viewer<'_>, sight: Sight) {
        self.total += 1;
        if let Some(layer) = view.map.layer(sight.layer) {
            self.layers.add_one(layer.name.clone());
        }
        if matches!(sight.shown, Shown::OtherTiles(_))
            && let Some(slot) = sight.tile.slot()
        {
            self.tiles
                .add_one(view.names.tile_text(Cell::new(slot, sight.tile.tile())));
        }
    }
}

/// What a legend line says a sight is.
fn legend_words(view: &Viewer<'_>, shown: Shown, sights: &SightCounts) -> String {
    let layer_name = |id: LayerId| view.map.layer(id).map_or("", |l| l.name.as_str());
    let layers = || {
        let names: Vec<String> = sights.layers.sorted().into_iter().map(|c| c.name).collect();
        names.join(", ")
    };
    match shown {
        Shown::Terrain(key) => {
            let name = view
                .names
                .terrain_of(key.slot, key.set, bt_terrain::TerrainColor(key.color))
                .map_or_else(|| "a terrain".to_owned(), |t| t.name.clone());
            format!("{name} (terrain on {})", layers())
        }
        Shown::OtherTiles(id) => format!(
            "tiles of {} that are no terrain's ({})",
            layer_name(id),
            counts_text(&sights.tiles.sorted(), 4)
        ),
        Shown::Tile(tile) => format!("{} (on {})", view.names.tile_text(tile.cell()), layers()),
        Shown::Layer(id) => format!("layer {}", layer_name(id)),
    }
}

/// The groups of objects of the whole map, top layer first, each with its capital: given from
/// every object layer, so letters stay the same in every view.
fn object_groups(map: &Map, names: &Names) -> Vec<ObjectGroup> {
    let mut groups: Vec<ObjectGroup> = Vec::new();
    for at in layers_top_first(map) {
        let Some(objects) = at.layer.as_object_layer() else {
            continue;
        };
        for object in objects.objects.iter() {
            let sprites = object.tile.is_some() && object_type(map, object).is_empty();
            let word = if sprites {
                "sprites".to_owned()
            } else {
                object_kind(map, names, object)
            };
            if !groups
                .iter()
                .any(|g| g.layer == at.layer.id && g.word == word)
            {
                groups.push(ObjectGroup {
                    layer: at.layer.id,
                    layer_name: at.layer.name.clone(),
                    sprites,
                    word,
                    symbol: TOO_MANY,
                });
            }
        }
    }
    let words: Vec<String> = groups
        .iter()
        .map(|g| {
            if g.sprites {
                g.layer_name.clone()
            } else {
                g.word.clone()
            }
        })
        .collect();
    for (group, symbol) in groups.iter_mut().zip(object_symbols(&words)) {
        group.symbol = symbol;
    }
    groups
}

/// Where each shown object of the chosen layers stands, with what the legend says of it.
fn object_marks(
    map: &Map,
    names: &Names,
    chosen: &[&Layer],
    groups: &mut [ObjectGroup],
) -> Vec<Mark> {
    let mut feet = Feet::new();
    let mut marks = Vec::new();
    for layer in chosen {
        let Some(objects) = layer.as_object_layer() else {
            continue;
        };
        if !is_shown(map, layer.id) {
            continue;
        }
        for object in objects.objects.iter().filter(|o| o.visible) {
            let sprites = object.tile.is_some() && object_type(map, object).is_empty();
            let word = if sprites {
                "sprites".to_owned()
            } else {
                object_kind(map, names, object)
            };
            let Some(group) = groups
                .iter()
                .position(|g| g.layer == layer.id && g.word == word)
            else {
                continue;
            };
            let point = standing_point(map, &mut feet, object);
            let entry = if sprites {
                object
                    .tile
                    .map_or_else(String::new, |tile| names.tile_text(tile))
            } else {
                let name = if object.name.is_empty() || object.name == word {
                    String::new()
                } else {
                    format!(" {}", crate::property_text::quoted(&object.name))
                };
                format!("#{}{name} {}", object.id.0, point_text(point))
            };
            marks.push(Mark {
                group,
                cell: standing_cell(point),
                entry,
            });
        }
    }
    marks
}

/// Where an object stands: its foot or point, else the middle of its box when its foot cannot
/// be measured (an image that cannot be read): the view marks it there all the same.
fn standing_point(map: &Map, feet: &mut Feet, object: &MapObject) -> DVec2 {
    feet.standing_point(map, object).unwrap_or_else(|_| {
        let geometry = map.geometry();
        let tileset = object.tile.and_then(|tile| map.tileset_of(tile));
        let bounds = bt_core::object_pixel_outline(&geometry, object, tileset).bounds();
        pixel_to_point(&geometry, (bounds.min + bounds.max) * 0.5)
    })
}

/// `desert.tmx: cells 0,0 40x40; layers Ground`, and what else the view should say first.
fn header(
    request: &ViewRequest<'_>,
    map: &Map,
    region: IRect,
    step: i32,
    tile_layers: &[&Layer],
    layout: &Layout,
) -> String {
    let mut text = match &request.title {
        Some(title) => format!("{title}: cells {}", region_text(region)),
        None => format!("cells {}", region_text(region)),
    };
    if step > 1 {
        text.push_str(&format!(", one character per {step}x{step} cells"));
    }
    let shown: Vec<&str> = tile_layers.iter().map(|l| l.name.as_str()).collect();
    text.push_str(&format!(
        "; layers {}",
        if shown.is_empty() {
            "(none)".to_owned()
        } else {
            shown.join(", ")
        }
    ));
    if request.layers.is_empty() {
        let hidden: Vec<String> = layers_top_first(map)
            .iter()
            .filter(|at| at.layer.as_tile_layer().is_some() && !is_shown(map, at.layer.id))
            .map(|at| at.layer.name.clone())
            .collect();
        if !hidden.is_empty() {
            text.push_str(&format!(
                "; hidden, left out: {} (--layers shows them)",
                hidden.join(", ")
            ));
        }
    }
    if layout.shifted {
        let which = match map.stagger_index {
            StaggerIndex::Odd => "odd",
            StaggerIndex::Even => "even",
        };
        text.push_str(&format!(
            "; {which} rows sit half a cell to the right, as on the map"
        ));
    }
    text.push('\n');
    text
}

/// How rows and rulers are laid out.
struct Layout {
    region: IRect,
    step: i32,
    cols: i32,
    /// Rows shifted: two characters a cell, the shifted rows one further right.
    shifted: bool,
    stagger_odd: bool,
    label_width: usize,
}

impl Layout {
    fn of(map: &Map, region: IRect, step: i32, cols: i32) -> Layout {
        let shifted = step == 1
            && matches!(
                map.orientation,
                Orientation::Hexagonal | Orientation::Staggered
            )
            && map.stagger_axis == StaggerAxis::Y;
        let widest = [region.y, region.y + region.h - 1]
            .iter()
            .map(|y| y.to_string().len())
            .max()
            .unwrap_or(1)
            .max(2);
        Layout {
            region,
            step,
            cols,
            shifted,
            stagger_odd: map.stagger_index == StaggerIndex::Odd,
            label_width: widest,
        }
    }

    /// The characters of a row as printed: spread to two a cell on shifted grids.
    fn spread(&self, line: &str) -> String {
        if !self.shifted {
            return line.to_owned();
        }
        let spread: String = line.chars().flat_map(|c| [c, ' ']).collect();
        spread.trim_end().to_owned()
    }

    fn pad(&self) -> String {
        " ".repeat(self.label_width + 2)
    }

    /// The column rulers: tens and units for one character a cell, else the first cell of
    /// every tenth character.
    fn write_rulers(&self, text: &mut String) {
        let pad = self.pad();
        if self.step == 1 {
            if self.cols > 10 {
                let tens: String = (0..self.cols)
                    .map(|c| {
                        let x = self.region.x + c;
                        if x.rem_euclid(10) == 0 {
                            digit((x / 10).unsigned_abs() % 10)
                        } else {
                            ' '
                        }
                    })
                    .collect();
                text.push_str(&format!("{pad}{}\n", self.spread(&tens).trim_end()));
            }
            let units: String = (0..self.cols)
                .map(|c| digit((self.region.x + c).rem_euclid(10).unsigned_abs()))
                .collect();
            text.push_str(&format!("{pad}{}\n", self.spread(&units)));
        } else {
            let mut ruler: Vec<char> = vec![' '; usize::try_from(self.cols).unwrap_or(0) + 8];
            for c in (0..self.cols).step_by(10) {
                let label = (self.region.x + c * self.step).to_string();
                for (i, ch) in label.chars().enumerate() {
                    if let Some(slot) = usize::try_from(c).ok().and_then(|c| ruler.get_mut(c + i)) {
                        *slot = ch;
                    }
                }
            }
            let ruler: String = ruler.into_iter().collect();
            text.push_str(&format!("{pad}{}\n", ruler.trim_end()));
        }
    }

    /// A row with its number.
    fn row_text(&self, y: i32, line: &str) -> String {
        let shift = if self.shifted && ((y & 1) != 0) == self.stagger_odd {
            " "
        } else {
            ""
        };
        format!(
            "{y:>width$}  {shift}{}",
            self.spread(line),
            width = self.label_width
        )
    }
}

/// A digit's character.
fn digit(value: u32) -> char {
    char::from_digit(value % 10, 10).unwrap_or(' ')
}

/// `~14-29 ,17-18`: the runs of a row that show something, in cells.
pub(crate) fn runs_text(line: &str, first_x: i32, step: i32) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut runs = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let Some(&symbol) = chars.get(start) else {
            break;
        };
        let mut end = start;
        while chars.get(end) == Some(&symbol) {
            end += 1;
        }
        if symbol != NOTHING {
            let from = first_x + i32::try_from(start).unwrap_or(0) * step;
            let to = first_x + i32::try_from(end).unwrap_or(0) * step - 1;
            runs.push(if from == to {
                format!("{symbol}{from}")
            } else {
                format!("{symbol}{from}-{to}")
            });
        }
        start = end;
    }
    runs.join(" ")
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;

    use bt_core::{Shape, Tileset, WangSetType};
    use bt_test_data::fixtures::{Atlas, MapBuilder, TerrainSetBuilder, combination_tile};
    use glam::UVec2;

    use super::*;
    use crate::report::{from_json, to_json};

    /// The rows of a view's text: the lines after the rulers that start with a row number.
    pub(crate) fn rows(text: &str) -> Vec<String> {
        text.lines()
            .filter(|line| {
                line.get(..2)
                    .is_some_and(|label| label.trim().parse::<i32>().is_ok())
            })
            .filter_map(|line| line.get(4..).map(str::to_owned))
            .collect()
    }

    /// A 10×6 map of grass with a pond of water painted on 3,2 4x2 (its edges around it), a
    /// layer `deco` of plain tiles at 1,0 and 1,1 above it, and a tree at 8.5,5.5.
    pub(crate) fn pond_map() -> Map {
        let set = TerrainSetBuilder::new(WangSetType::Corner)
            .name("Land")
            .terrains(&["Grass", "Water"])
            .every_combination();
        let atlas = Atlas::builder().name("ground").build_embedded().unwrap();
        let tileset = Arc::new(set.added_to(Tileset::clone(&atlas.tileset)));
        let things = Atlas::builder().name("things").build_embedded().unwrap();
        let tile = |c: [u8; 4]| combination_tile(2, c).unwrap().0;
        let inside = |x: i32, y: i32| (3..7).contains(&x) && (2..4).contains(&y);
        let rows: Vec<String> = (0..6)
            .map(|y| {
                (0..10)
                    .map(|x| {
                        // A corner is water when a painted cell has it.
                        let corner = |dx: i32, dy: i32| {
                            let any = [(0, 0), (-1, 0), (0, -1), (-1, -1)]
                                .iter()
                                .any(|(ox, oy)| inside(x + dx + ox, y + dy + oy));
                            if any { 2 } else { 1 }
                        };
                        tile([corner(1, 0), corner(1, 1), corner(0, 1), corner(0, 0)]).to_string()
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        let tree = MapObject {
            class: "tree".into(),
            shape: Shape::Point,
            pos: DVec2::new(8.0 * 16.0 + 8.0, 16.0 * 5.0 + 8.0),
            ..MapObject::default()
        };
        MapBuilder::new(UVec2::new(16, 16), UVec2::new(10, 6))
            .tileset(tileset)
            .tileset(Arc::clone(&things.tileset))
            .tile_layer("ground", &rows.join("\n"))
            .tile_layer("deco", ". 2@1v . . . . . . . .\n. 2@1 . . . . . . . .")
            .object_layer("design", vec![tree])
            .build()
            .unwrap()
    }

    #[test]
    fn a_painted_area_reads_as_exactly_its_cells() {
        let map = pond_map();
        let names = Names::of(&map);
        let view =
            ViewOutcome::from_request(&ViewRequest::new(&map, &names).title("pond.tmx")).unwrap();
        assert_eq!(
            view.text,
            "pond.tmx: cells 0,0 10x6; layers deco, ground\n    \
             0123456789\n \
             0  \"d\"\"\"\"\"\"\"\"\n \
             1  \"d\"....\"\"\"\n \
             2  \"\".~~~~.\"\"\n \
             3  \"\".~~~~.\"\"\n \
             4  \"\"\"....\"\"\"\n \
             5  \"\"\"\"\"\"\"\"T\"\n\
             legend:\n  \
             .  nothing, or only the edge of a terrain\n  \
             \"  grass (terrain on ground): 38 cells\n  \
             ~  water (terrain on ground): 8 cells\n  \
             d  tiles of deco that are no terrain's (things:2 ×2): 2 cells\n  \
             T  tree on design: #1 8.5,5.5\n"
        );
        assert_eq!(
            from_json::<ViewOutcome>(&to_json(&view).unwrap()).unwrap(),
            view
        );
    }

    #[test]
    fn views_by_tile_and_layer_with_runs_regions_and_steps() {
        let map = pond_map();
        let names = Names::of(&map);
        let by_layer = ViewOutcome::from_request(
            &ViewRequest::new(&map, &names)
                .by(ViewBy::Layer)
                .region(IRect::from_min_size(IVec2::new(0, 0), IVec2::new(4, 2)))
                .runs(true),
        )
        .unwrap();
        assert_eq!(
            rows(&by_layer.text),
            ["gdgg   g0 d1 g2-3", "gdgg   g0 d1 g2-3"]
        );
        let by_tile =
            ViewOutcome::from_request(&ViewRequest::new(&map, &names).by(ViewBy::Tile)).unwrap();
        assert!(
            by_tile.text.contains("things:2 (on deco): 2 cells"),
            "{}",
            by_tile.text
        );
        let stepped = ViewOutcome::from_request(&ViewRequest::new(&map, &names).step(2)).unwrap();
        assert!(
            stepped
                .text
                .starts_with("cells 0,0 10x6, one character per 2x2 cells;"),
            "{}",
            stepped.text
        );
        assert_eq!(rows(&stepped.text).len(), 3);
        assert!(matches!(
            ViewOutcome::from_request(&ViewRequest::new(&map, &names).step(0)),
            Err(Error::ZeroStep)
        ));
        // The edit report's view of a changed area.
        let around = ViewOutcome::from_request(
            &ViewRequest::new(&map, &names).around(&[IVec2::new(3, 2), IVec2::new(4, 2)]),
        )
        .unwrap();
        assert!(
            around.text.starts_with("the changed area: cells 2,1 4x3;"),
            "{}",
            around.text
        );
    }

    #[test]
    fn tileds_examples_read_as_they_look() {
        let Some(examples) = bt_test_data::test_data("reference/tiled/examples") else {
            return;
        };
        let mut loader = bt_tmx::Loader::default();
        let mut load = |file: &str| loader.load_map(&examples.join(file)).unwrap().value;
        let view = |map: &Map| {
            ViewOutcome::from_request(&ViewRequest::new(map, &Names::of(map)))
                .unwrap()
                .text
        };
        // The desert: its terrains by their hints, the brick wall and the dirt path at the left.
        let mut desert = load("desert.tmx");
        let before = view(&desert);
        assert!(
            before.contains("  :  desert (terrain on Ground): 1240 cells\n"),
            "{before}"
        );
        assert!(
            before.contains("  #  brick (terrain on Ground)"),
            "{before}"
        );
        assert_eq!(&rows(&before)[16][..12], ",,..########", "{before}");
        // An edit: a cobblestone tile where the desert was. Every symbol keeps its meaning.
        let cobblestone = desert.layers[0]
            .as_tile_layer()
            .unwrap()
            .cell(IVec2::new(13, 6));
        desert.layers[0]
            .as_tile_layer_mut()
            .unwrap()
            .set_cell(IVec2::new(0, 0), cobblestone)
            .unwrap();
        let after = view(&desert);
        assert_eq!(&rows(&after)[0][..2], "a:", "{after}");
        assert!(
            after.contains("  a  cobblestone (terrain on Ground): 156 cells\n"),
            "{after}"
        );
        assert!(after.contains("  :  desert (terrain on Ground)"));
        // The sewers have no terrains: by tile.
        let sewers = view(&load("sewers.tmx"));
        assert!(
            sewers.contains("  s  sewer-tileset:27 (on Bottom): 2049 cells\n"),
            "{sewers}"
        );
        // An isometric map reads as its grid, unturned.
        let iso = view(&load("isometric_grass_and_water.tmx"));
        assert!(
            iso.contains("  \"  grass (terrain on Tile Layer 1)"),
            "{iso}"
        );
        assert!(
            iso.contains("  ~  water (terrain on Tile Layer 1)"),
            "{iso}"
        );
        // A hexagonal map's shifted rows start one further right, two characters a cell.
        let hex = view(&load("hexagonal-mini.tmx"));
        assert!(hex.contains("; odd rows sit half a cell to the right, as on the map\n"));
        let hex_rows = rows(&hex);
        assert!(hex_rows[0].starts_with("o o o e "), "{hex}");
        assert!(hex_rows[1].starts_with(" o h e "), "{hex}");
        // Sticker Knight's sandbox is objects only: capitals where their feet are.
        let knight = view(&load("sticker-knight/map/sandbox.tmx"));
        assert!(knight.contains("; layers (none)\n"));
        assert!(
            knight.contains("  S  sprites on shading ×17: shadow ×17\n"),
            "{knight}"
        );
        assert!(
            knight.contains("  L  sprites on parallax clouds ×5: cloud ×5\n"),
            "{knight}"
        );
    }

    #[test]
    fn symbols_keep_their_meaning_before_and_after_an_edit() {
        let map = pond_map();
        let names = Names::of(&map);
        let before = ViewOutcome::from_request(&ViewRequest::new(&map, &names)).unwrap();
        // The pond drained: every ground tile grass.
        let mut drained = map.clone();
        let grass = combination_tile(2, [1, 1, 1, 1]).unwrap();
        let slot = bt_core::TilesetSlot::new(0).unwrap();
        if let Some(tiles) = drained.layers[0].as_tile_layer_mut() {
            for y in 0..6 {
                for x in 0..10 {
                    tiles
                        .set_cell(IVec2::new(x, y), Cell::new(slot, grass))
                        .unwrap();
                }
            }
        }
        let after =
            ViewOutcome::from_request(&ViewRequest::new(&drained, &Names::of(&drained))).unwrap();
        assert!(before.text.contains("\"  grass (terrain on ground)"));
        assert!(after.text.contains("\"  grass (terrain on ground)"));
        assert!(after.text.contains("d  tiles of deco"));
        assert!(!after.text.contains('~'));
    }
}
