//! Patterns in the Terrain mode (`06-application.md`, §8): the combinations of terrains the
//! chosen set needs (`bt_terrain::patterns::needed`), each drawn as a tile of its lanes, the ones
//! no tile has framed in red; a click chooses one, and each click on a tile then labels the tile
//! with it, one step a click. A set needing more patterns than can be listed says how many.

use std::sync::Arc;

use bt_core::{Lane, Tileset, WangId, WangSet};
use bt_editor::{DocumentId, Intent};
use bt_terrain::patterns::{NeededPattern, Patterns};
use egui::{Color32, Sense, Shape, Stroke, StrokeKind, vec2};

use super::terrain::{TerrainFrame, TileFrame, diamond_ratio, draw_labels, lane_region};
use super::{DrawnTiles, draw_tiles};
use crate::panels::{ShownTiles, TileSource};

/// The side of a pattern's picture, in points.
const PATTERN_SIZE: f32 = 30.0;

/// The patterns of one version of a set, kept until the tileset changes.
#[derive(Debug)]
struct NeededOf {
    tileset: Arc<Tileset>,
    set: usize,
    patterns: Patterns,
}

/// The Patterns panel's own state.
#[derive(Debug)]
pub(crate) struct PatternsPanel {
    doc: DocumentId,
    /// The pattern tiles are labelled with.
    chosen: Option<WangId>,
    /// Only the patterns no tile has are listed.
    missing_only: bool,
    needed: Option<NeededOf>,
}

impl PatternsPanel {
    /// The panel of `doc`'s tileset editor.
    pub(crate) fn new(doc: DocumentId) -> PatternsPanel {
        PatternsPanel {
            doc,
            chosen: None,
            missing_only: false,
            needed: None,
        }
    }

    /// The document it was opened in.
    pub(crate) fn doc(&self) -> DocumentId {
        self.doc
    }

    /// The patterns the chosen set needs, worked out again only when the tileset changed.
    fn refresh(&mut self, cx: TerrainFrame<'_>) -> Option<usize> {
        let set = cx.editing.label_terrain()?.set;
        let tileset = cx.editor.edited_tileset(cx.doc)?;
        let current = self
            .needed
            .as_ref()
            .is_some_and(|n| n.set == set && Arc::ptr_eq(&n.tileset, tileset));
        if !current {
            self.needed = cx
                .editor
                .needed_patterns(cx.doc, set)
                .map(|patterns| NeededOf {
                    tileset: Arc::clone(tileset),
                    set,
                    patterns,
                });
        }
        self.needed.as_ref().map(|n| n.set)
    }

    /// Draws the panel; whether it stays open.
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, cx: TerrainFrame<'_>) -> bool {
        let mut open = true;
        ui.horizontal(|ui| {
            ui.strong("Patterns");
            if ui.button("Close").clicked() {
                open = false;
            }
        });
        let Some(index) = self.refresh(cx) else {
            ui.weak("Choose a terrain set to see the patterns it needs.");
            return open;
        };
        let (Some(set), Some(needed)) = (cx.tileset.wang_sets.get(index), &self.needed) else {
            return open;
        };
        let patterns = &needed.patterns;
        let listed = u64::try_from(patterns.listed.len()).unwrap_or(u64::MAX);
        if patterns.total > listed {
            ui.label(format!(
                "“{}” needs {} patterns: too many to list.",
                set.name, patterns.total
            ));
            return open;
        }
        let missing = patterns.listed.len() - patterns.present();
        ui.label(format!(
            "“{}” needs {}: {} present, {missing} missing.",
            set.name,
            bt_utils::text::plural(patterns.listed.len(), "pattern", "patterns"),
            patterns.present()
        ));
        ui.checkbox(&mut self.missing_only, "Only the missing ones");
        // The places in the list of the patterns shown.
        let shown: Vec<usize> = (0..patterns.listed.len())
            .filter(|&i| !self.missing_only || patterns.listed.get(i).is_some_and(|p| p.tiles == 0))
            .collect();
        let columns = ((ui.available_width() / (PATTERN_SIZE + 4.0)).floor() as usize).max(1);
        let rows = shown.len().div_ceil(columns);
        let mut chosen = self.chosen;
        egui::ScrollArea::vertical()
            .id_salt("patterns")
            .auto_shrink([false, true])
            .max_height(ui.available_height() - 60.0)
            .show_rows(ui, PATTERN_SIZE + 4.0, rows, |ui, range| {
                for row in range {
                    ui.horizontal(|ui| {
                        let start = row * columns;
                        let end = (start + columns).min(shown.len());
                        for &place in shown.get(start..end).unwrap_or_default() {
                            let Some(pattern) = patterns.listed.get(place) else {
                                continue;
                            };
                            let response = pattern_picture(ui, set, pattern, chosen, place);
                            if response.clicked() {
                                chosen = Some(pattern.pattern);
                            }
                        }
                    });
                }
            });
        self.chosen = chosen;
        match self.chosen {
            Some(pattern) => {
                ui.label(describe(set, pattern));
                ui.weak("Click tiles to label each with it.");
            }
            None => {
                ui.weak("Choose a pattern, then click tiles to label them with it.");
            }
        }
        open
    }

    /// The tiles with the chosen set's labels; with a pattern chosen, a click labels a tile with
    /// it.
    pub(crate) fn show_tiles(
        &mut self,
        ui: &mut egui::Ui,
        cx: TerrainFrame<'_>,
        zoom: &mut f32,
        source: &mut TileSource<'_>,
        out: &mut Vec<Intent>,
    ) -> Option<ShownTiles> {
        let set = cx
            .editing
            .label_terrain()
            .and_then(|label| cx.tileset.wang_sets.get(label.set));
        let diamond = diamond_ratio(cx.tileset);
        let drawn = draw_tiles(
            ui,
            cx.tileset,
            zoom,
            source,
            Sense::click(),
            |ui, id, rect| {
                if let Some(set) = set {
                    draw_labels(ui.painter(), set, id, TileFrame::of(rect, diamond));
                }
            },
        )?;
        let DrawnTiles { response, shown } = drawn;
        if response.clicked()
            && let Some(pattern) = self.chosen
            && let Some(tile) = response
                .interact_pointer_pos()
                .and_then(|at| shown.grid.index_at(at))
                .and_then(|index| shown.ids.get(index).copied())
        {
            out.push(Intent::LabelPattern {
                doc: cx.doc,
                pattern,
                tile,
            });
        }
        Some(shown)
    }
}

/// A pattern drawn as a tile of its lanes in their terrains' colours, framed in red when no
/// tile has it and boldly when it is chosen; it answers clicks as "Pattern n", the `place`-th
/// in the set's list counted from 1.
fn pattern_picture(
    ui: &mut egui::Ui,
    set: &WangSet,
    needed: &NeededPattern,
    chosen: Option<WangId>,
    place: usize,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(PATTERN_SIZE, PATTERN_SIZE), Sense::click());
    let is_chosen = chosen == Some(needed.pattern);
    let missing = needed.tiles == 0;
    let label = format!("Pattern {}", place + 1);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, is_chosen, &label)
    });
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
    let inner = rect.shrink(2.0);
    for lane in Lane::ALL {
        let color = needed.pattern.lane(lane);
        let Some(terrain) = usize::from(color)
            .checked_sub(1)
            .and_then(|i| set.colors.get(i))
        else {
            continue;
        };
        if !set.kind.uses(lane) {
            continue;
        }
        let [r, g, b, _] = terrain.color.to_array();
        painter.add(Shape::convex_polygon(
            lane_region(set.kind, lane.index(), inner),
            Color32::from_rgb(r, g, b),
            Stroke::NONE,
        ));
    }
    let frame = if is_chosen {
        Stroke::new(3.0, ui.visuals().selection.stroke.color)
    } else if missing {
        Stroke::new(2.0, ui.visuals().error_fg_color)
    } else {
        Stroke::new(1.0, ui.visuals().weak_text_color())
    };
    painter.rect_stroke(rect.shrink(1.0), 0.0, frame, StrokeKind::Inside);
    let tiles = match needed.tiles {
        0 => "no tile has it".to_owned(),
        n => format!(
            "{} {} it",
            bt_utils::text::plural(n, "tile", "tiles"),
            if n == 1 { "has" } else { "have" }
        ),
    };
    response.on_hover_text(format!("{}; {tiles}", describe(set, needed.pattern)))
}

/// A pattern in words: the terrain on each lane of the set's kind.
fn describe(set: &WangSet, pattern: WangId) -> String {
    let name = |color: u8| match usize::from(color).checked_sub(1) {
        None => "nothing".to_owned(),
        Some(i) => set
            .colors
            .get(i)
            .map_or_else(|| format!("terrain {color}"), |c| c.name.clone()),
    };
    let lanes: Vec<String> = Lane::ALL
        .into_iter()
        .filter(|&lane| set.kind.uses(lane))
        .map(|lane| format!("{} {}", lane_name(lane), name(pattern.lane(lane))))
        .collect();
    lanes.join(", ")
}

/// How a lane is named in words.
fn lane_name(lane: Lane) -> &'static str {
    match lane {
        Lane::Top => "top",
        Lane::TopRight => "top-right",
        Lane::Right => "right",
        Lane::BottomRight => "bottom-right",
        Lane::Bottom => "bottom",
        Lane::BottomLeft => "bottom-left",
        Lane::Left => "left",
        Lane::TopLeft => "top-left",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pattern_is_said_lane_by_lane() {
        let set = WangSet {
            name: "Ground".into(),
            class: String::new(),
            kind: bt_core::WangSetType::Corner,
            tile: None,
            colors: ["Grass", "Dirt"]
                .map(|name| bt_core::WangColor {
                    name: name.into(),
                    class: String::new(),
                    color: bt_core::Color::rgb(0, 0, 0),
                    tile: None,
                    probability: 1.0,
                    properties: Default::default(),
                })
                .to_vec(),
            tiles: Default::default(),
            properties: Default::default(),
        };
        assert_eq!(
            describe(&set, WangId::from_lanes([0, 1, 0, 2, 0, 0, 0, 1])),
            "top-right Grass, bottom-right Dirt, bottom-left nothing, top-left Grass"
        );
    }
}
