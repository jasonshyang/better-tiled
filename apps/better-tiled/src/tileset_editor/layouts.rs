//! Layout… in the Terrain mode (`06-application.md`, §8): the autotile layouts of
//! `bt_terrain::layouts::LAYOUTS`, each with a picture of its tiles, the blocks found in the
//! tileset's image outlined on the tiles, a block chosen by a click (on a block found, or on the
//! tile where the chosen layout's block starts), the set it goes into and its two terrains
//! (guessed from its looks), Apply and Label All. The blocks are looked for on a worker, once
//! when the panel opens and again on request.

use bt_core::{TileId, WangSet, WangSetType};
use bt_editor::{DocumentId, Intent, LayoutSearch, LayoutTarget};
use bt_terrain::TerrainColor;
use bt_terrain::layouts::{Block, Fit, LAYOUTS, Layout};
use bt_utils::IRect;
use egui::{Color32, Rect, Sense, Shape, Stroke, vec2};
use glam::UVec2;

use super::terrain::{TerrainFrame, TileFrame, diamond_ratio, draw_labels, lane_region};
use super::{DrawnTiles, draw_tiles};
use crate::panels::{ShownTiles, TileSource};

/// A tile of a layout's picture, in points.
const PICTURE_CELL: f32 = 6.0;
/// The terrain in a layout's picture.
const PICTURE_TERRAIN: Color32 = Color32::from_rgb(110, 190, 90);
/// What it is drawn over.
const PICTURE_UNDER: Color32 = Color32::from_gray(90);

/// The terrain chosen for one side of a block.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum SidePick {
    /// No terrain: drawn over nothing.
    Nothing,
    /// A terrain of the set.
    Terrain(TerrainColor),
    /// A terrain the set does not have yet, added by the step.
    New,
}

/// Layout…'s own state.
#[derive(Debug)]
pub(crate) struct LayoutsPanel {
    doc: DocumentId,
    /// The layout: an index into `LAYOUTS`.
    layout: usize,
    /// The block's top-left tile, once chosen.
    origin: Option<UVec2>,
    /// The set chosen for the labels; `None` until one is, or when it does not suit the layout.
    into: Option<LayoutTarget>,
    /// The terrain of the block (`#` in the layout).
    terrain: SidePick,
    /// What it is drawn over (`.`).
    under: SidePick,
    /// The blocks were asked for.
    asked: bool,
    /// The terrains are to be guessed again (the block or the set changed).
    guess_again: bool,
}

impl LayoutsPanel {
    /// The panel of `doc`'s tileset editor, opening: the blocks are looked for at once.
    pub(crate) fn new(doc: DocumentId) -> LayoutsPanel {
        LayoutsPanel {
            doc,
            layout: 0,
            origin: None,
            into: None,
            terrain: SidePick::New,
            under: SidePick::Nothing,
            asked: false,
            guess_again: false,
        }
    }

    /// The document it was opened in.
    pub(crate) fn doc(&self) -> DocumentId {
        self.doc
    }

    /// Draws the panel; whether it stays open.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        cx: TerrainFrame<'_>,
        out: &mut Vec<Intent>,
    ) -> bool {
        let search = cx.editor.layout_search(cx.doc);
        if search.is_none() && !self.asked {
            out.push(Intent::FindLayouts { doc: cx.doc });
            self.asked = true;
        }
        let fits: Vec<Fit> = match search {
            Some(LayoutSearch::Found(found)) => found.fits().to_vec(),
            _ => Vec::new(),
        };
        if self.origin.is_none()
            && let Some(first) = fits.first()
        {
            self.choose_fit(first);
        }
        if self.guess_again {
            self.guess(cx, &fits);
        }
        let mut open = true;
        ui.horizontal(|ui| {
            ui.strong("Layout");
            if ui
                .button("Look again")
                .on_hover_text("Look for the blocks in the tileset's image again")
                .clicked()
            {
                out.push(Intent::FindLayouts { doc: cx.doc });
            }
            if ui.button("Close").clicked() {
                open = false;
            }
        });
        match search {
            None | Some(LayoutSearch::Searching) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak("Looking for layouts in the image…");
                });
            }
            Some(LayoutSearch::Failed(problem)) => {
                ui.colored_label(ui.visuals().error_fg_color, problem);
            }
            Some(LayoutSearch::Found(_)) => self.show_found(ui, &fits),
        }
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("layouts")
            .max_height(ui.available_height() * 0.45)
            .show(ui, |ui| self.show_layouts(ui, &fits));
        ui.separator();
        self.show_block(ui, cx, &fits, out);
        open
    }

    /// The blocks found, each chosen by a click.
    fn show_found(&mut self, ui: &mut egui::Ui, fits: &[Fit]) {
        if fits.is_empty() {
            ui.weak("No block of a layout was found: choose a layout and click the tile where its block starts.");
            return;
        }
        ui.label(format!(
            "Found {}:",
            bt_utils::text::plural(fits.len(), "block", "blocks")
        ));
        for fit in fits {
            let Some(layout) = LAYOUTS.get(fit.layout) else {
                continue;
            };
            let chosen = self.layout == fit.layout && self.origin == Some(fit.origin);
            let text = format!(
                "{} at column {}, row {}",
                layout.name, fit.origin.x, fit.origin.y
            );
            let over = if fit.over_nothing {
                "drawn over nothing"
            } else {
                "drawn over another terrain"
            };
            if ui
                .selectable_label(chosen, text)
                .on_hover_text(over)
                .clicked()
            {
                self.choose_fit(fit);
            }
        }
    }

    /// The layouts, each with its picture; a click chooses one.
    fn show_layouts(&mut self, ui: &mut egui::Ui, fits: &[Fit]) {
        for (index, layout) in LAYOUTS.iter().enumerate() {
            ui.horizontal(|ui| {
                layout_picture(ui, layout);
                let response = ui
                    .selectable_label(self.layout == index, layout.name)
                    .on_hover_text(layout.about);
                if response.clicked() && self.layout != index {
                    self.layout = index;
                    match fits.iter().find(|f| f.layout == index) {
                        Some(fit) => self.choose_fit(fit),
                        None => self.guess_again = true,
                    }
                }
            });
        }
    }

    /// The block chosen, its set and terrains, Apply and Label All.
    fn show_block(
        &mut self,
        ui: &mut egui::Ui,
        cx: TerrainFrame<'_>,
        fits: &[Fit],
        out: &mut Vec<Intent>,
    ) {
        let Some(layout) = LAYOUTS.get(self.layout) else {
            return;
        };
        match self.origin {
            Some(origin) => ui.label(format!(
                "“{}” from column {}, row {}",
                layout.name, origin.x, origin.y
            )),
            None => ui.weak(format!(
                "Click the tile where a block of “{}” starts",
                layout.name
            )),
        };
        let into = self.target(cx);
        let set = match into {
            LayoutTarget::Set(index) => cx.tileset.wang_sets.get(index),
            LayoutTarget::NewSet(_) => None,
        };
        egui::Grid::new("layout-block")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                ui.label("Into");
                self.choose_target(ui, cx, layout.kind, into);
                ui.end_row();
                ui.label("Block's terrain");
                choose_side(ui, "layout-terrain", &mut self.terrain, set, false);
                ui.end_row();
                ui.label("Drawn over");
                choose_side(ui, "layout-under", &mut self.under, set, true);
                ui.end_row();
            });
        let count = set.map_or(0, |s| s.colors.len());
        let block = self.block(count);
        ui.horizontal(|ui| {
            let apply = ui
                .add_enabled(block.is_some(), egui::Button::new("Apply"))
                .on_hover_text("Label the block's tiles as the layout says (one step)")
                .on_disabled_hover_text(
                    "Choose a block, and two different terrains for its two sides",
                );
            if apply.clicked()
                && let Some(block) = block
            {
                out.push(Intent::ApplyLayout {
                    doc: cx.doc,
                    into,
                    block,
                });
                self.labelled();
            }
            let kind = target_kind(cx, into);
            let of_kind = fits
                .iter()
                .filter(|f| LAYOUTS.get(f.layout).is_some_and(|l| Some(l.kind) == kind))
                .count();
            let all = ui
                .add_enabled(of_kind > 0, egui::Button::new("Label All"))
                .on_hover_text(format!(
                    "Label every block found of a {} layout ({of_kind}), telling their terrains \
                     apart by their looks (one step)",
                    kind.map_or("", kind_name)
                ))
                .on_disabled_hover_text("No block of a layout of the set's kind was found");
            if all.clicked() {
                out.push(Intent::LabelAllLayouts { doc: cx.doc, into });
                self.labelled();
            }
        });
    }

    /// After a step labelled blocks: the set they went into is the one the Terrain mode labels
    /// with (a new one included), so it is the default again, and the terrains are guessed again
    /// against it.
    fn labelled(&mut self) {
        self.into = None;
        self.guess_again = true;
    }

    /// The set chooser: the sets of the layout's kind, and a new one.
    fn choose_target(
        &mut self,
        ui: &mut egui::Ui,
        cx: TerrainFrame<'_>,
        kind: WangSetType,
        into: LayoutTarget,
    ) {
        let name = |target: LayoutTarget| match target {
            LayoutTarget::Set(index) => cx
                .tileset
                .wang_sets
                .get(index)
                .map_or_else(String::new, |s| s.name.clone()),
            LayoutTarget::NewSet(kind) => format!("New {} set", kind_name(kind)),
        };
        let mut chosen = into;
        egui::ComboBox::from_id_salt("layout-into")
            .selected_text(name(into))
            .show_ui(ui, |ui| {
                for (index, set) in cx.tileset.wang_sets.iter().enumerate() {
                    if set.kind == kind {
                        ui.selectable_value(&mut chosen, LayoutTarget::Set(index), &set.name);
                    }
                }
                let new = LayoutTarget::NewSet(kind);
                ui.selectable_value(&mut chosen, new, name(new));
            });
        if chosen != into {
            self.into = Some(chosen);
            self.guess_again = true;
        }
    }

    /// The tiles with the labels of the set the block goes into, the blocks found outlined and
    /// the chosen block outlined boldly; a click chooses a block.
    pub(crate) fn show_tiles(
        &mut self,
        ui: &mut egui::Ui,
        cx: TerrainFrame<'_>,
        zoom: &mut f32,
        source: &mut TileSource<'_>,
    ) -> Option<ShownTiles> {
        let fits: Vec<Fit> = match cx.editor.layout_search(cx.doc) {
            Some(LayoutSearch::Found(found)) => found.fits().to_vec(),
            _ => Vec::new(),
        };
        let set = match self.target(cx) {
            LayoutTarget::Set(index) => cx.tileset.wang_sets.get(index),
            LayoutTarget::NewSet(_) => None,
        };
        let columns = cx.tileset.columns().max(1);
        let chosen = self.block_area();
        let diamond = diamond_ratio(cx.tileset);
        let found_stroke = Stroke::new(1.5, ui.visuals().strong_text_color());
        let chosen_stroke = Stroke::new(3.0, ui.visuals().selection.stroke.color);
        ui.weak("Click a block found, or the tile where the chosen layout's block starts.");
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
                let at = UVec2::new(id.0 % columns, id.0 / columns);
                for fit in &fits {
                    outline_edges(ui.painter(), rect, fit.area(), at, found_stroke);
                }
                if let Some(area) = chosen {
                    outline_edges(ui.painter(), rect, area, at, chosen_stroke);
                }
            },
        )?;
        let DrawnTiles { response, shown } = drawn;
        let tile_at = |at: egui::Pos2| {
            let index = shown.grid.index_at(at)?;
            shown.ids.get(index).copied()
        };
        if response.clicked()
            && let Some(id) = response.interact_pointer_pos().and_then(tile_at)
        {
            self.click_tile(id, columns, &fits);
        }
        Some(shown)
    }

    /// A click on a tile: the block found there, else the chosen layout's block from there.
    fn click_tile(&mut self, id: TileId, columns: u32, fits: &[Fit]) {
        let at = UVec2::new(id.0 % columns, id.0 / columns);
        match fits.iter().find(|fit| contains(fit.area(), at)) {
            Some(fit) => self.choose_fit(fit),
            None => {
                self.origin = Some(at);
                self.guess_again = true;
            }
        }
    }

    /// Chooses a block found: its layout and place; its terrains are guessed.
    fn choose_fit(&mut self, fit: &Fit) {
        self.layout = fit.layout;
        self.origin = Some(fit.origin);
        self.guess_again = true;
    }

    /// Guesses the block's terrains from its looks: the block found there, or the chosen
    /// layout's block from the chosen tile (over nothing when what it is drawn over is nothing).
    fn guess(&mut self, cx: TerrainFrame<'_>, fits: &[Fit]) {
        let Some(origin) = self.origin else {
            return;
        };
        if !matches!(
            cx.editor.layout_search(cx.doc),
            Some(LayoutSearch::Found(_))
        ) {
            return;
        }
        self.guess_again = false;
        let fit = fits
            .iter()
            .find(|f| f.layout == self.layout && f.origin == origin)
            .cloned()
            .unwrap_or(Fit {
                layout: self.layout,
                origin,
                over_nothing: self.under == SidePick::Nothing,
            });
        let into = self.target(cx);
        let Some(guess) = cx.editor.layout_guess(cx.doc, into, &fit) else {
            return;
        };
        let count = match into {
            LayoutTarget::Set(index) => cx
                .tileset
                .wang_sets
                .get(index)
                .map_or(0, |s| s.colors.len()),
            LayoutTarget::NewSet(_) => 0,
        };
        let pick = |color: TerrainColor| {
            if color.is_none() {
                SidePick::Nothing
            } else if usize::from(color.0) <= count {
                SidePick::Terrain(color)
            } else {
                SidePick::New
            }
        };
        self.terrain = match pick(guess.terrain) {
            SidePick::Nothing => SidePick::New,
            other => other,
        };
        self.under = pick(guess.under);
    }

    /// Where the labels go: the set chosen, while it suits the layout; else the set the Terrain
    /// mode labels with, when it does; else the first set of the layout's kind; else a new one.
    fn target(&self, cx: TerrainFrame<'_>) -> LayoutTarget {
        let kind = LAYOUTS
            .get(self.layout)
            .map_or(WangSetType::Corner, |l| l.kind);
        let suits = |target: LayoutTarget| match target {
            LayoutTarget::Set(index) => cx
                .tileset
                .wang_sets
                .get(index)
                .is_some_and(|s| s.kind == kind),
            LayoutTarget::NewSet(new) => new == kind,
        };
        let labelled = cx
            .editing
            .label_terrain()
            .map(|label| LayoutTarget::Set(label.set));
        let first = cx
            .tileset
            .wang_sets
            .iter()
            .position(|s| s.kind == kind)
            .map(LayoutTarget::Set);
        [self.into, labelled, first]
            .into_iter()
            .flatten()
            .find(|target| suits(*target))
            .unwrap_or(LayoutTarget::NewSet(kind))
    }

    /// The block to label in a set of `count` terrains: new terrains are numbered after them.
    /// `None` without a place, or when the block's terrain is nothing.
    fn block(&self, count: usize) -> Option<Block> {
        let origin = self.origin?;
        let next = u8::try_from(count + 1).ok()?;
        let terrain = match self.terrain {
            SidePick::Nothing => return None,
            SidePick::Terrain(color) => color,
            SidePick::New => TerrainColor(next),
        };
        let under = match self.under {
            SidePick::Nothing => TerrainColor::NONE,
            SidePick::Terrain(color) => color,
            SidePick::New if self.terrain == SidePick::New => TerrainColor(next.checked_add(1)?),
            SidePick::New => TerrainColor(next),
        };
        (terrain != under).then_some(Block {
            layout: self.layout,
            origin,
            terrain,
            under,
        })
    }

    /// The chosen block's tiles, as columns and rows of the atlas.
    fn block_area(&self) -> Option<IRect> {
        let origin = self.origin?;
        let size = LAYOUTS.get(self.layout)?.size();
        Some(IRect::from_min_size(origin.as_ivec2(), size.as_ivec2()))
    }
}

/// The kind of the set `into` names.
fn target_kind(cx: TerrainFrame<'_>, into: LayoutTarget) -> Option<WangSetType> {
    match into {
        LayoutTarget::Set(index) => cx.tileset.wang_sets.get(index).map(|s| s.kind),
        LayoutTarget::NewSet(kind) => Some(kind),
    }
}

/// How the panel names a kind of set.
fn kind_name(kind: WangSetType) -> &'static str {
    match kind {
        WangSetType::Corner => "corner",
        WangSetType::Edge => "edge",
        WangSetType::Mixed => "mixed",
    }
}

/// The chooser of one side's terrain: the set's terrains, a new one, and with `nothing`, no
/// terrain.
fn choose_side(
    ui: &mut egui::Ui,
    id: &str,
    pick: &mut SidePick,
    set: Option<&WangSet>,
    nothing: bool,
) {
    let name = |pick: SidePick| match pick {
        SidePick::Nothing => "Nothing".to_owned(),
        SidePick::New => "New terrain".to_owned(),
        SidePick::Terrain(color) => set
            .and_then(|s| s.colors.get(usize::from(color.0).saturating_sub(1)))
            .map_or_else(|| format!("Terrain {}", color.0), |c| c.name.clone()),
    };
    egui::ComboBox::from_id_salt(id)
        .selected_text(name(*pick))
        .show_ui(ui, |ui| {
            if nothing {
                ui.selectable_value(pick, SidePick::Nothing, "Nothing")
                    .on_hover_text("The terrain is drawn over transparency");
            }
            for (i, terrain) in set
                .map_or(&[][..], |s| s.colors.as_slice())
                .iter()
                .enumerate()
            {
                let color = TerrainColor(u8::try_from(i + 1).unwrap_or(u8::MAX));
                ui.selectable_value(pick, SidePick::Terrain(color), &terrain.name);
            }
            ui.selectable_value(pick, SidePick::New, "New terrain");
        });
}

/// Draws the edges of the tile at `at` (column and row) drawn in `rect` that lie on the border
/// of `area`, so a block's outline is drawn tile by tile, inside the view's clip.
fn outline_edges(painter: &egui::Painter, rect: Rect, area: IRect, at: UVec2, stroke: Stroke) {
    if !contains(area, at) {
        return;
    }
    let (x, y) = (i64::from(at.x), i64::from(at.y));
    let r = rect.shrink(stroke.width * 0.5);
    let edges = [
        (y == i64::from(area.y), [r.left_top(), r.right_top()]),
        (
            y == i64::from(area.y) + i64::from(area.h) - 1,
            [r.left_bottom(), r.right_bottom()],
        ),
        (x == i64::from(area.x), [r.left_top(), r.left_bottom()]),
        (
            x == i64::from(area.x) + i64::from(area.w) - 1,
            [r.right_top(), r.right_bottom()],
        ),
    ];
    for (on_border, points) in edges {
        if on_border {
            painter.line_segment(points, stroke);
        }
    }
}

/// Whether the tile at `at` (column and row) is in `area`.
fn contains(area: IRect, at: UVec2) -> bool {
    let (x, y) = (i64::from(at.x), i64::from(at.y));
    x >= i64::from(area.x)
        && y >= i64::from(area.y)
        && x < i64::from(area.x) + i64::from(area.w)
        && y < i64::from(area.y) + i64::from(area.h)
}

/// A small picture of a layout: each tile's lanes in the terrain's colour where the layout puts
/// the terrain, in grey where it puts what the terrain is drawn over.
fn layout_picture(ui: &mut egui::Ui, layout: &Layout) {
    let size = layout.size();
    let (rect, _) = ui.allocate_exact_size(
        vec2(size.x as f32, size.y as f32) * PICTURE_CELL,
        Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    let width = size.x.max(1) as usize;
    for (index, slot) in layout.slots().into_iter().enumerate() {
        let Some(slot) = slot else { continue };
        let cell = Rect::from_min_size(
            rect.min
                + vec2(
                    (index % width) as f32 * PICTURE_CELL,
                    (index / width) as f32 * PICTURE_CELL,
                ),
            vec2(PICTURE_CELL, PICTURE_CELL),
        );
        painter.rect_filled(cell, 0.0, PICTURE_UNDER);
        for lane in 0..8 {
            if slot.terrain & (1 << lane) != 0 {
                painter.add(Shape::convex_polygon(
                    lane_region(layout.kind, lane, cell),
                    PICTURE_TERRAIN,
                    Stroke::NONE,
                ));
            }
        }
        if layout.kind == WangSetType::Mixed && slot.middle {
            painter.rect_filled(
                Rect::from_center_size(cell.center(), cell.size() / 3.0),
                0.0,
                PICTURE_TERRAIN,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_terrains_are_numbered_after_the_sets() {
        let mut panel = LayoutsPanel::new(DocumentId(1));
        assert_eq!(panel.block(2), None, "no place yet");
        panel.origin = Some(UVec2::new(1, 2));
        panel.terrain = SidePick::New;
        panel.under = SidePick::New;
        let block = panel.block(2).unwrap();
        assert_eq!(
            (block.terrain, block.under),
            (TerrainColor(3), TerrainColor(4))
        );
        panel.terrain = SidePick::Terrain(TerrainColor(1));
        let block = panel.block(2).unwrap();
        assert_eq!(
            (block.terrain, block.under),
            (TerrainColor(1), TerrainColor(3))
        );
        panel.under = SidePick::Terrain(TerrainColor(1));
        assert_eq!(panel.block(2), None, "the same terrain on both sides");
    }

    #[test]
    fn a_tile_is_outlined_on_the_border_of_its_block() {
        let area = IRect {
            x: 1,
            y: 1,
            w: 3,
            h: 2,
        };
        assert!(contains(area, UVec2::new(3, 2)));
        assert!(!contains(area, UVec2::new(4, 2)));
        assert!(!contains(area, UVec2::new(0, 1)));
    }
}
