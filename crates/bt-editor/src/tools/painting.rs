//! Painting with the tile tools: the brush's strokes and the eraser's, painted live as a gesture
//! and recorded as one step; Bucket Fill and Shape Fill; the Picker; capturing a brush off the
//! map. Every preview the window shows comes from the same placement as the paint, with the
//! same seed, so what is previewed is what is painted.

use bt_core::{Cell, LayerId, LayerKind, Map, PlacedCell};
use bt_edit::{
    Brush, BrushMode, EditError as OperationError, TileClip, TileRegion, erase, erase_targets,
    fill_area, find_target_layers, find_tileset, paste,
};
use bt_utils::IRect;
use bt_utils::rng::Rng;
use glam::IVec2;

use super::kind::ToolKind;
use crate::document::{DocumentId, Selection};
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;
use crate::notice::Notice;

/// A step of a tile tool's stroke.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stroke {
    /// A stroke starts in a document.
    Begin {
        /// The document.
        doc: DocumentId,
        /// What it does.
        kind: StrokeKind,
    },
    /// The brush is put down at these cells: one placement each, as a drag paints, or with
    /// `line` one placement along them all, skipping overlaps (Shift's line).
    Paint {
        /// The cells.
        points: Vec<IVec2>,
        /// One placement along the cells.
        line: bool,
    },
    /// The eraser passes over these cells.
    Erase {
        /// The cells, each the middle of a square of the brush size.
        points: Vec<IVec2>,
        /// Every shown, unlocked tile layer rather than the selected ones.
        all_layers: bool,
    },
    /// The stroke is done: one step of the history.
    End,
    /// The stroke is dropped: the map goes back to where it started.
    Cancel,
}

/// What a stroke does.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum StrokeKind {
    /// Paints the brush.
    Paint,
    /// Erases.
    Erase,
}

/// A stroke under way.
#[derive(Clone, Debug)]
pub(crate) struct StrokeState {
    doc: DocumentId,
    kind: StrokeKind,
    /// Picks variations and random tiles along the stroke; it starts from the tools' seed, so
    /// the first placement is the one previewed.
    rng: Rng,
    /// The locked layers the stroke left alone, to say so at its end.
    skipped: Vec<LayerId>,
    /// The last cell painted.
    last: Option<IVec2>,
}

/// Bucket Fill's area and what it paints there.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FillPreview {
    /// The cells filled.
    pub area: TileRegion,
    /// The tiles painted, in the map's tilesets.
    pub cells: Vec<PlacedCell>,
}

impl Editor {
    /// Makes `brush` the brush; the Brush tool becomes current (picking a brush means painting
    /// with it).
    pub(crate) fn set_brush(&mut self, brush: Brush) {
        let brush = brush
            .with_mode(if self.tools.random {
                BrushMode::Random
            } else {
                BrushMode::Stamp
            })
            .with_size(self.tools.brush_size);
        self.tools.brush = Some(brush);
        self.tools.set_tool(ToolKind::Brush);
    }

    /// The tiles picked in the Tiles panel (cells of the document's map's tilesets at their
    /// places in the tileset) become the brush.
    pub(crate) fn pick_tiles(&mut self, doc: DocumentId, cells: &[PlacedCell]) -> Vec<Effect> {
        let Some(map) = self.map_of(doc).cloned() else {
            return Vec::new();
        };
        match Brush::from_cells(&map, cells) {
            Ok(brush) => {
                self.set_brush(brush);
                Vec::new()
            }
            Err(error) => vec![warn(error.into())],
        }
    }

    /// What the brush would paint at `points` now, in the map's tilesets: what the window
    /// previews, and what a press there paints. Tiles of tilesets the map does not have yet
    /// (added when painted) are not previewed.
    pub fn brush_preview(&self, doc: DocumentId, points: &[IVec2], line: bool) -> Vec<PlacedCell> {
        let (Some(brush), Some(map)) = (&self.tools.brush, self.map_of(doc)) else {
            return Vec::new();
        };
        let geometry = map.geometry();
        let mut rng = Rng::new(self.tools.seed);
        let groups = point_groups(points, line);
        groups
            .iter()
            .filter_map(|group| brush.placement(group, &geometry, &mut rng))
            .flat_map(|clip| cells_in_map(map, &clip))
            .collect()
    }

    /// Takes a step of a stroke.
    pub(crate) fn stroke(&mut self, step: Stroke) -> Vec<Effect> {
        match step {
            Stroke::Begin { doc, kind } => {
                if let Err(error) = self.begin_gesture(doc, kind.label()) {
                    return vec![warn(error)];
                }
                self.stroke = Some(StrokeState {
                    doc,
                    kind,
                    rng: Rng::new(self.tools.seed),
                    skipped: Vec::new(),
                    last: None,
                });
                Vec::new()
            }
            Stroke::Paint { points, line } => self.paint_points(&points, line),
            Stroke::Erase { points, all_layers } => self.erase_points(&points, all_layers),
            Stroke::End => {
                let Some(stroke) = self.stroke.take() else {
                    return Vec::new();
                };
                if let Err(error) = self.end_gesture() {
                    return vec![warn(error)];
                }
                if stroke.kind == StrokeKind::Paint {
                    self.tools.last_paint = stroke.last;
                    self.tools.reseed();
                }
                self.locked_notice(stroke.doc, &stroke.skipped)
                    .into_iter()
                    .collect()
            }
            Stroke::Cancel => {
                self.stroke = None;
                match self.cancel_gesture() {
                    Ok(()) => Vec::new(),
                    Err(error) => vec![warn(error)],
                }
            }
        }
    }

    fn paint_points(&mut self, points: &[IVec2], line: bool) -> Vec<Effect> {
        let (Some(mut stroke), Some(brush)) = (self.stroke.take(), self.tools.brush.clone()) else {
            return Vec::new();
        };
        let Some(asset) = self.documents.get(stroke.doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let selection = self.selection_of(stroke.doc);
        let result = self.gesture_apply(|tx| {
            let map = tx.map_mut(asset)?;
            let geometry = map.geometry();
            for group in point_groups(points, line) {
                let Some(clip) = brush.placement(&group, &geometry, &mut stroke.rng) else {
                    continue;
                };
                let skipped = paste_clip(map, &clip, &selection)?;
                stroke.skipped.extend(skipped);
            }
            Ok::<_, EditError>(())
        });
        stroke.last = points.last().copied().or(stroke.last);
        self.stroke = Some(stroke);
        result.err().map(warn).into_iter().collect()
    }

    fn erase_points(&mut self, points: &[IVec2], all_layers: bool) -> Vec<Effect> {
        let Some(mut stroke) = self.stroke.take() else {
            return Vec::new();
        };
        let Some(asset) = self.documents.get(stroke.doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let selection = self.selection_of(stroke.doc);
        let region = squares(points, self.tools.brush_size);
        let region = if selection.tiles.is_empty() {
            region
        } else {
            region.intersect(&selection.tiles)
        };
        let result = self.gesture_apply(|tx| {
            let map = tx.map_mut(asset)?;
            let targets =
                erase_targets(map, &selection.layers, all_layers).map_err(EditError::from)?;
            // The eraser's targets leave locked layers out; they are named at the end.
            stroke
                .skipped
                .extend(locked_erase_layers(map, &selection.layers, all_layers));
            erase(map, &targets, &region).map_err(EditError::from)?;
            Ok::<_, EditError>(())
        });
        self.stroke = Some(stroke);
        result.err().map(warn).into_iter().collect()
    }

    /// Bucket Fill's area from `start` and what the brush paints there; `whole_selection` fills
    /// the selected cells instead (Shift).
    pub fn fill_preview(
        &self,
        doc: DocumentId,
        start: IVec2,
        extra: &[Cell],
        whole_selection: bool,
    ) -> FillPreview {
        let (Some(brush), Some(map)) = (&self.tools.brush, self.map_of(doc)) else {
            return FillPreview::default();
        };
        let selection = self.selection_of(doc);
        let area = fill_region(map, &selection, start, extra, whole_selection);
        let cells = brush
            .fill(&area, &mut Rng::new(self.tools.seed))
            .map(|clip| cells_in_map(map, &clip))
            .unwrap_or_default();
        FillPreview { area, cells }
    }

    /// Fills as [`Editor::fill_preview`] shows, as one step.
    pub(crate) fn fill(
        &mut self,
        doc: DocumentId,
        start: IVec2,
        extra: &[Cell],
        whole_selection: bool,
    ) -> Vec<Effect> {
        let Some(map) = self.map_of(doc).cloned() else {
            return Vec::new();
        };
        let selection = self.selection_of(doc);
        let area = fill_region(&map, &selection, start, extra, whole_selection);
        self.paint_area(doc, &area, "Fill")
    }

    /// What Shape Fill paints over `region`.
    pub fn shape_preview(&self, doc: DocumentId, region: &TileRegion) -> Vec<PlacedCell> {
        let (Some(brush), Some(map)) = (&self.tools.brush, self.map_of(doc)) else {
            return Vec::new();
        };
        let region = paintable(map, &self.selection_of(doc), region);
        brush
            .fill(&region, &mut Rng::new(self.tools.seed))
            .map(|clip| cells_in_map(map, &clip))
            .unwrap_or_default()
    }

    /// Paints the brush over `region` as Shape Fill does, as one step.
    pub(crate) fn shape_fill(&mut self, doc: DocumentId, region: &TileRegion) -> Vec<Effect> {
        let Some(map) = self.map_of(doc).cloned() else {
            return Vec::new();
        };
        let region = paintable(&map, &self.selection_of(doc), region);
        self.paint_area(doc, &region, "Shape Fill")
    }

    /// Fills `area` with the brush as one step; the seed moves on.
    fn paint_area(&mut self, doc: DocumentId, area: &TileRegion, label: &str) -> Vec<Effect> {
        let Some(brush) = self.tools.brush.clone() else {
            return vec![Effect::Notify(Notice::info(
                "Pick tiles first: there is no brush to fill with",
            ))];
        };
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let selection = self.selection_of(doc);
        let seed = self.tools.seed;
        let result = self.transact(doc, label, |tx| {
            let map = tx.map_mut(asset)?;
            let Some(clip) = brush.fill(area, &mut Rng::new(seed)) else {
                return Ok(Vec::new());
            };
            paste_clip(map, &clip, &selection)
        });
        self.tools.reseed();
        match result {
            Ok(skipped) => self.locked_notice(doc, &skipped).into_iter().collect(),
            Err(error) => vec![warn(error)],
        }
    }

    /// The Picker: the topmost tile shown at `at` becomes the brush.
    pub(crate) fn pick_from_map(&mut self, doc: DocumentId, at: IVec2) -> Vec<Effect> {
        let Some(map) = self.map_of(doc).cloned() else {
            return Vec::new();
        };
        let mut topmost = None;
        map.for_each_layer(|layer, _| {
            let shown = map.effective_layer(layer.id).is_some_and(|l| l.visible);
            if let LayerKind::Tile(cells) = &layer.kind
                && shown
            {
                let cell = cells.cell(at);
                if !cell.is_empty() {
                    topmost = Some(cell);
                }
            }
        });
        let Some(cell) = topmost else {
            return vec![Effect::Notify(Notice::info(
                "There is no tile there to pick",
            ))];
        };
        self.pick_tiles(
            doc,
            &[PlacedCell {
                at: IVec2::ZERO,
                cell,
            }],
        )
    }

    /// The Brush's right-drag: the tiles of the selected layers in `rect` become the brush; with
    /// `cut` they are erased too, as one step.
    pub(crate) fn capture(&mut self, doc: DocumentId, rect: IRect, cut: bool) -> Vec<Effect> {
        let Some(map) = self.map_of(doc).cloned() else {
            return Vec::new();
        };
        let selection = self.selection_of(doc);
        let layers = capture_layers(&map, &selection);
        let brush = match Brush::capture(&map, &layers, rect) {
            Ok(brush) => brush,
            Err(error) => return vec![warn(error.into())],
        };
        let mut effects = Vec::new();
        if cut {
            let asset = self.documents.get(doc).map(|d| d.asset());
            let result = self.transact(doc, "Cut", |tx| {
                let Some(asset) = asset else {
                    return Ok(Vec::new());
                };
                let map = tx.map_mut(asset)?;
                let report =
                    erase(map, &layers, &TileRegion::from_rect(rect)).map_err(EditError::from)?;
                Ok::<_, EditError>(report.skipped_locked)
            });
            match result {
                Ok(skipped) => effects.extend(self.locked_notice(doc, &skipped)),
                Err(error) => effects.push(warn(error)),
            }
        }
        self.set_brush(brush);
        effects
    }

    /// The document's selection, or none.
    fn selection_of(&self, doc: DocumentId) -> Selection {
        self.documents
            .get(doc)
            .map(|d| d.selection().clone())
            .unwrap_or_default()
    }

    /// What to say about locked layers an edit left alone, if any.
    fn locked_notice(&self, doc: DocumentId, skipped: &[LayerId]) -> Option<Effect> {
        let map = self.map_of(doc)?;
        let mut names: Vec<String> = Vec::new();
        for id in skipped {
            if let Some(layer) = map.layer(*id)
                && !names.contains(&layer.name)
            {
                names.push(layer.name.clone());
            }
        }
        let quoted = bt_utils::text::quoted_list(&names);
        match names.len() {
            0 => None,
            1 => Some(Effect::Notify(Notice::info(format!(
                "{quoted} is locked: its tiles were left as they were"
            )))),
            _ => Some(Effect::Notify(Notice::info(format!(
                "{quoted} are locked: their tiles were left as they were"
            )))),
        }
    }
}

impl StrokeKind {
    /// The step's name in the history.
    fn label(self) -> &'static str {
        match self {
            StrokeKind::Paint => "Paint",
            StrokeKind::Erase => "Erase",
        }
    }
}

/// The placements a set of points makes: one per point, or one along them all.
fn point_groups(points: &[IVec2], line: bool) -> Vec<Vec<IVec2>> {
    if line {
        vec![points.to_vec()]
    } else {
        points.iter().map(|&p| vec![p]).collect()
    }
}

/// Pastes a clip where it goes into the layers Tiled's rules pick, masked by the selection; the
/// locked targets it left alone.
fn paste_clip(
    map: &mut Map,
    clip: &TileClip,
    selection: &Selection,
) -> Result<Vec<LayerId>, EditError> {
    let targets = find_target_layers(map, clip, selection.current_layer, &selection.layers)
        .map_err(EditError::from)?;
    let report = paste(map, clip, clip.origin, &targets, &selection.tiles)
        .map_err(|error: OperationError| EditError::from(error))?;
    Ok(report.skipped_locked)
}

/// A clip's tiles where it goes, in the map's tilesets; tiles of tilesets the map lacks are
/// left out.
fn cells_in_map(map: &Map, clip: &TileClip) -> Vec<PlacedCell> {
    let slots: Vec<Option<bt_core::TilesetSlot>> = clip
        .tilesets
        .iter()
        .map(|tileset| find_tileset(map, tileset))
        .collect();
    let mut cells = Vec::new();
    for layer in &clip.layers {
        for placed in layer.cells.cells() {
            let Some(slot) = placed
                .cell
                .slot()
                .and_then(|s| slots.get(s.index()).copied().flatten())
            else {
                continue;
            };
            cells.push(PlacedCell {
                at: clip.origin + placed.at,
                cell: Cell::new(slot, placed.cell.tile()).with_flip(placed.cell.flip()),
            });
        }
    }
    cells
}

/// The cells of squares of side `size` around each point (one cell for size 1), as the eraser
/// and a sized brush cover them.
fn squares(points: &[IVec2], size: u32) -> TileRegion {
    let side = i32::try_from(size.max(1)).unwrap_or(1);
    let before = (side - 1) / 2;
    TileRegion::from_rects(
        points
            .iter()
            .map(|&p| IRect::from_min_size(p - IVec2::splat(before), IVec2::splat(side))),
    )
}

/// Bucket Fill's area: the selection itself when asked for, else the fill area from `start` on
/// the current tile layer.
fn fill_region(
    map: &Map,
    selection: &Selection,
    start: IVec2,
    extra: &[Cell],
    whole_selection: bool,
) -> TileRegion {
    if whole_selection {
        return paintable(map, selection, &selection.tiles);
    }
    let Some(layer) = selection
        .current_layer
        .and_then(|id| map.layer(id))
        .and_then(|l| l.as_tile_layer())
    else {
        return TileRegion::new();
    };
    if !layer.contains(start) {
        return TileRegion::new();
    }
    fill_area(layer, start, extra, &map.geometry(), &selection.tiles)
}

/// The part of `region` a paint may touch: inside a finite map, and inside the selection when
/// there is one.
fn paintable(map: &Map, selection: &Selection, region: &TileRegion) -> TileRegion {
    let region = if map.infinite {
        region.clone()
    } else {
        region.clipped(map.tile_bounding_rect())
    };
    if selection.tiles.is_empty() {
        region
    } else {
        region.intersect(&selection.tiles)
    }
}

/// The shown tile layers the eraser would empty but for their locks: the selected ones, or with
/// `all` every one.
fn locked_erase_layers(map: &Map, selected: &[LayerId], all: bool) -> Vec<LayerId> {
    let mut locked = Vec::new();
    map.for_each_layer(|layer, _| {
        let chosen = all || selected.contains(&layer.id);
        let state = map.effective_layer(layer.id);
        if chosen && layer.as_tile_layer().is_some() && state.is_some_and(|s| s.visible && s.locked)
        {
            locked.push(layer.id);
        }
    });
    locked
}

/// The layers a capture takes tiles from: the selected tile layers, or the current one.
fn capture_layers(map: &Map, selection: &Selection) -> Vec<LayerId> {
    let selected: Vec<LayerId> = selection
        .layers
        .iter()
        .copied()
        .filter(|id| map.layer(*id).and_then(|l| l.as_tile_layer()).is_some())
        .collect();
    if selected.is_empty() {
        selection.current_layer.into_iter().collect()
    } else {
        selected
    }
}
