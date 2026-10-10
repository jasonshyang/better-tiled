//! The tileset editor's Terrain mode: the tiles with the chosen set's labels drawn over them and
//! the lane under the pointer outlined, as Tiled's tileset view finds it (a corner set's
//! quadrant, an edge set's triangle, a mixed set's 3×3 grid, the middle doing nothing). A drag
//! labels the lanes it passes with the chosen terrain, the right button clears them, each drag
//! one step. Beside the tiles, the tileset's terrain sets and the chosen set's terrains: choose
//! one to label with, and the sets' and terrains' commands; and the helpers, which take the
//! tiles' clicks while they are open: Layout… (`layouts.rs`), Auto-label…, which labels the
//! chosen terrain from the next tile clicked, and Patterns (`patterns.rs`).

use bt_core::{Color, TileId, Tileset, WangSet, WangSetType};
use bt_editor::{
    DocumentId, Editor, Intent, LabelMark, LabelStroke, LabelTerrain, TerrainSetEdit,
    TilesetEditing,
};
use bt_terrain::TerrainColor;
use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, pos2};

use super::layouts::LayoutsPanel;
use super::patterns::PatternsPanel;
use super::{DrawnTiles, draw_tiles};
use crate::panels::{ShownTiles, TileSource};

/// The Terrain mode's own state.
#[derive(Debug, Default)]
pub(crate) struct TerrainLabeller {
    /// A labelling drag under way.
    drag: Option<LabelDrag>,
    /// A name being edited.
    renaming: Option<Renaming>,
    /// The helper that is open, which takes the tiles' clicks.
    helper: Option<Helper>,
}

/// A helper of the Terrain mode, open beside the sets.
#[derive(Debug)]
enum Helper {
    /// Layout…: blocks of autotile layouts labelled at once.
    Layouts(LayoutsPanel),
    /// Auto-label…: the next click on a tile labels the chosen terrain from that tile.
    AutoLabel {
        /// The document.
        doc: DocumentId,
    },
    /// Patterns: what the chosen set needs; a pattern labels the tiles clicked.
    Patterns(PatternsPanel),
}

impl Helper {
    /// The document it was opened in.
    fn doc(&self) -> DocumentId {
        match self {
            Helper::Layouts(panel) => panel.doc(),
            Helper::AutoLabel { doc } => *doc,
            Helper::Patterns(panel) => panel.doc(),
        }
    }
}

/// What a frame of the Terrain mode works with.
#[derive(Copy, Clone)]
pub(crate) struct TerrainFrame<'a> {
    /// The editor.
    pub(crate) editor: &'a Editor,
    /// The tileset edited.
    pub(crate) tileset: &'a Tileset,
    /// Its tileset editor.
    pub(crate) editing: &'a TilesetEditing,
    /// The document.
    pub(crate) doc: DocumentId,
}

/// A labelling drag: clearing (the right button) or painting, and the lane marked last.
#[derive(Copy, Clone, Debug)]
struct LabelDrag {
    erase: bool,
    last: Option<LabelMark>,
}

/// A set's name, or one of its terrains', being edited.
#[derive(Clone, Debug)]
struct Renaming {
    set: usize,
    color: Option<TerrainColor>,
    text: String,
}

impl TerrainLabeller {
    /// Draws the Terrain mode in the rest of `ui`; where the tiles were drawn, if any were.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        cx: TerrainFrame<'_>,
        zoom: &mut f32,
        source: &mut TileSource<'_>,
        out: &mut Vec<Intent>,
    ) -> Option<ShownTiles> {
        if self.helper.as_ref().is_some_and(|h| h.doc() != cx.doc) {
            self.helper = None;
        }
        egui::Panel::right("terrain-sets")
            .resizable(true)
            .default_size(260.0)
            .show(ui, |ui| self.show_sets(ui, cx, out));
        match &mut self.helper {
            Some(Helper::Layouts(panel)) => {
                let open = egui::Panel::right("terrain-layouts")
                    .resizable(true)
                    .default_size(280.0)
                    .show(ui, |ui| panel.show(ui, cx, out))
                    .inner;
                let shown = panel.show_tiles(ui, cx, zoom, source);
                if !open {
                    self.helper = None;
                }
                shown
            }
            Some(Helper::AutoLabel { .. }) => self.show_auto_label_tiles(ui, cx, zoom, source, out),
            Some(Helper::Patterns(panel)) => {
                let open = egui::Panel::right("terrain-patterns")
                    .resizable(true)
                    .default_size(260.0)
                    .show(ui, |ui| panel.show(ui, cx))
                    .inner;
                let shown = panel.show_tiles(ui, cx, zoom, source, out);
                if !open {
                    self.helper = None;
                }
                shown
            }
            None => self.show_label_tiles(ui, cx, zoom, source, out),
        }
    }

    /// Auto-label…: the tiles with the chosen set's labels; a click on one labels the chosen
    /// terrain from it, and the helper closes.
    fn show_auto_label_tiles(
        &mut self,
        ui: &mut egui::Ui,
        cx: TerrainFrame<'_>,
        zoom: &mut f32,
        source: &mut TileSource<'_>,
        out: &mut Vec<Intent>,
    ) -> Option<ShownTiles> {
        let label = cx.editing.label_terrain();
        let set = label.and_then(|l| cx.tileset.wang_sets.get(l.set));
        let name = label
            .zip(set)
            .and_then(|(l, s)| s.colors.get(usize::from(l.color.0).saturating_sub(1)))
            .map_or_else(String::new, |c| c.name.clone());
        ui.horizontal(|ui| {
            ui.label(format!(
                "Click a tile that is all “{name}”: the terrain is labelled on every tile that \
                 shows it"
            ));
            let escape = !crate::focus::is_typing(ui.ctx())
                && ui.input(|input| input.key_pressed(egui::Key::Escape));
            if ui.button("Cancel").clicked() || escape {
                self.helper = None;
            }
        });
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
            && let Some(tile) = response
                .interact_pointer_pos()
                .and_then(|at| shown.grid.index_at(at))
                .and_then(|index| shown.ids.get(index).copied())
        {
            out.push(Intent::AutoLabel { doc: cx.doc, tile });
            self.helper = None;
        }
        Some(shown)
    }

    /// The tiles with the chosen set's labels, labelled lane by lane.
    fn show_label_tiles(
        &mut self,
        ui: &mut egui::Ui,
        cx: TerrainFrame<'_>,
        zoom: &mut f32,
        source: &mut TileSource<'_>,
        out: &mut Vec<Intent>,
    ) -> Option<ShownTiles> {
        let TerrainFrame {
            tileset,
            editing,
            doc,
            ..
        } = cx;
        let label = editing.label_terrain();
        let set = label.and_then(|l| tileset.wang_sets.get(l.set));
        let diamond = diamond_ratio(tileset);
        let drawn = draw_tiles(
            ui,
            tileset,
            zoom,
            source,
            Sense::click_and_drag(),
            |ui, id, rect| {
                if let Some(set) = set {
                    draw_labels(ui.painter(), set, id, TileFrame::of(rect, diamond));
                }
            },
        )?;
        let DrawnTiles { response, shown } = drawn;
        let Some(set) = set else {
            if response.hovered() {
                response.on_hover_text("Choose a terrain set and a terrain to label tiles with");
            }
            return Some(shown);
        };
        let hovered = response.hover_pos().and_then(|at| {
            let index = shown.grid.index_at(at)?;
            let tile = shown.ids.get(index).copied()?;
            let frame = TileFrame::of(shown.grid.rect(index), diamond);
            let local = frame.local(at);
            let lane = lane_at(set.kind, local.x, local.y)?;
            Some(HoveredLane { tile, lane, frame })
        });
        if let Some(hovered) = hovered {
            let stroke = Stroke::new(2.0, ui.visuals().strong_text_color());
            ui.painter().add(Shape::closed_line(
                hovered.frame.region(set.kind, hovered.lane),
                stroke,
            ));
        }
        self.handle_pointer(ui, doc, hovered, out);
        Some(shown)
    }

    /// Starts, continues and ends labelling drags.
    fn handle_pointer(
        &mut self,
        ui: &egui::Ui,
        doc: DocumentId,
        hovered: Option<HoveredLane>,
        out: &mut Vec<Intent>,
    ) {
        let (primary_pressed, secondary_pressed, any_down) = ui.input(|input| {
            (
                input.pointer.primary_pressed(),
                input.pointer.secondary_pressed(),
                input.pointer.any_down(),
            )
        });
        let mark = |erase: bool| {
            hovered.map(|h| LabelMark {
                tile: h.tile,
                lane: bt_core::Lane::ALL
                    .get(h.lane)
                    .copied()
                    .unwrap_or(bt_core::Lane::Top),
                erase,
            })
        };
        if self.drag.is_none() && (primary_pressed || secondary_pressed) {
            let erase = secondary_pressed;
            if let Some(first) = mark(erase) {
                out.push(Intent::LabelStroke(LabelStroke::Begin { doc }));
                out.push(Intent::LabelStroke(LabelStroke::Paint {
                    marks: vec![first],
                }));
                self.drag = Some(LabelDrag {
                    erase,
                    last: Some(first),
                });
            }
            return;
        }
        let Some(drag) = &mut self.drag else {
            return;
        };
        if !any_down {
            self.drag = None;
            out.push(Intent::LabelStroke(LabelStroke::End));
            return;
        }
        if let Some(next) = mark(drag.erase)
            && drag.last != Some(next)
        {
            drag.last = Some(next);
            out.push(Intent::LabelStroke(LabelStroke::Paint {
                marks: vec![next],
            }));
        }
    }

    /// The terrain sets and the chosen set's terrains, with their commands, and the helpers.
    fn show_sets(&mut self, ui: &mut egui::Ui, cx: TerrainFrame<'_>, out: &mut Vec<Intent>) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                self.show_set_list(ui, cx, out);
                ui.separator();
                self.show_helper_buttons(ui, cx);
            });
    }

    /// The buttons that open the helpers.
    fn show_helper_buttons(&mut self, ui: &mut egui::Ui, cx: TerrainFrame<'_>) {
        let atlas = !cx.tileset.is_collection();
        let layouts_open = matches!(self.helper, Some(Helper::Layouts(_)));
        let layouts = ui
            .add_enabled(atlas, egui::Button::selectable(layouts_open, "Layout…"))
            .on_hover_text(
                "Label whole blocks of tiles at once, from where they sit in a common autotile \
                 layout: blob sets, Godot and Tilesetter sheets, 3×3 islands and holes",
            )
            .on_disabled_hover_text("For tilesets cut from one image");
        if layouts.clicked() {
            self.helper = if layouts_open {
                None
            } else {
                Some(Helper::Layouts(LayoutsPanel::new(cx.doc)))
            };
        }
        let terrain_chosen = cx
            .editing
            .label_terrain()
            .is_some_and(|label| !label.color.is_none());
        let picking = matches!(self.helper, Some(Helper::AutoLabel { .. }));
        let auto = ui
            .add_enabled(
                atlas && terrain_chosen,
                egui::Button::selectable(picking, "Auto-label…"),
            )
            .on_hover_text(
                "Label the chosen terrain on every tile at once, from one tile that is all of \
                 it: click this, then that tile",
            )
            .on_disabled_hover_text(
                "Choose a terrain to label (not Erase), on a tileset cut from one image",
            );
        if auto.clicked() {
            self.helper = if picking {
                None
            } else {
                Some(Helper::AutoLabel { doc: cx.doc })
            };
        }
        let patterns_open = matches!(self.helper, Some(Helper::Patterns(_)));
        let patterns = ui
            .add(egui::Button::selectable(patterns_open, "Patterns"))
            .on_hover_text(
                "The patterns of terrains the chosen set needs, the missing ones in red: choose \
                 one, then click tiles to label them with it",
            );
        if patterns.clicked() {
            self.helper = if patterns_open {
                None
            } else {
                Some(Helper::Patterns(PatternsPanel::new(cx.doc)))
            };
        }
    }

    /// The terrain sets and the chosen set's terrains, with their commands.
    fn show_set_list(&mut self, ui: &mut egui::Ui, cx: TerrainFrame<'_>, out: &mut Vec<Intent>) {
        let TerrainFrame {
            tileset,
            editing,
            doc,
            ..
        } = cx;
        let edit = |edit: TerrainSetEdit| Intent::EditTerrainSets { doc, edit };
        let label = editing.label_terrain();
        ui.horizontal(|ui| {
            ui.strong("Terrain Sets");
            ui.menu_button("New", |ui| {
                for (kind, name) in [
                    (WangSetType::Corner, "Corner Set"),
                    (WangSetType::Edge, "Edge Set"),
                    (WangSetType::Mixed, "Mixed Set"),
                ] {
                    if ui.button(name).clicked() {
                        out.push(edit(TerrainSetEdit::AddSet { kind }));
                        ui.close();
                    }
                }
            });
        });
        if tileset.wang_sets.is_empty() {
            ui.weak("No terrain sets yet: New makes one.");
        }
        for (index, set) in tileset.wang_sets.iter().enumerate() {
            if self.rename_field(ui, index, None, out, doc) {
                continue;
            }
            let chosen = label.is_some_and(|l| l.set == index);
            let response = ui.selectable_label(chosen, &set.name);
            if response.clicked() {
                let color = TerrainColor(u8::from(!set.colors.is_empty()));
                out.push(Intent::ChooseLabelTerrain {
                    doc,
                    label: LabelTerrain { set: index, color },
                });
            }
            response.context_menu(|ui| {
                if ui.button("Rename").clicked() {
                    self.renaming = Some(Renaming {
                        set: index,
                        color: None,
                        text: set.name.clone(),
                    });
                    ui.close();
                }
                if ui.button("Duplicate").clicked() {
                    out.push(edit(TerrainSetEdit::DuplicateSet { set: index }));
                    ui.close();
                }
                if ui.button("Remove").clicked() {
                    out.push(edit(TerrainSetEdit::RemoveSet { set: index }));
                    ui.close();
                }
            });
        }
        let Some((label, set)) = label.and_then(|l| Some((l, tileset.wang_sets.get(l.set)?)))
        else {
            return;
        };
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong("Terrains");
            if ui.button("Add").clicked() {
                out.push(edit(TerrainSetEdit::AddTerrain { set: label.set }));
            }
        });
        let choose = |color: TerrainColor| Intent::ChooseLabelTerrain {
            doc,
            label: LabelTerrain {
                set: label.set,
                color,
            },
        };
        if ui
            .selectable_label(label.color.is_none(), "Erase")
            .on_hover_text("Label with no terrain")
            .clicked()
        {
            out.push(choose(TerrainColor::NONE));
        }
        let count = set.colors.len();
        for (i, terrain) in set.colors.iter().enumerate() {
            let color = TerrainColor(u8::try_from(i + 1).unwrap_or(u8::MAX));
            if self.rename_field(ui, label.set, Some(color), out, doc) {
                continue;
            }
            ui.horizontal(|ui| {
                let [r, g, b, a] = terrain.color.to_array();
                let mut shown = Color32::from_rgba_unmultiplied(r, g, b, a);
                if egui::color_picker::color_edit_button_srgba(
                    ui,
                    &mut shown,
                    egui::color_picker::Alpha::Opaque,
                )
                .changed()
                {
                    let [r, g, b, _] = shown.to_array();
                    out.push(edit(TerrainSetEdit::RecolorTerrain {
                        set: label.set,
                        color,
                        rgb: Color::rgb(r, g, b),
                    }));
                }
                let response = ui.selectable_label(label.color == color, &terrain.name);
                if response.clicked() {
                    out.push(choose(color));
                }
                response.context_menu(|ui| {
                    if ui.button("Rename").clicked() {
                        self.renaming = Some(Renaming {
                            set: label.set,
                            color: Some(color),
                            text: terrain.name.clone(),
                        });
                        ui.close();
                    }
                    if i > 0 && ui.button("Move Up").clicked() {
                        out.push(edit(TerrainSetEdit::MoveTerrain {
                            set: label.set,
                            from: color,
                            to: TerrainColor(color.0 - 1),
                        }));
                        ui.close();
                    }
                    if i + 1 < count && ui.button("Move Down").clicked() {
                        out.push(edit(TerrainSetEdit::MoveTerrain {
                            set: label.set,
                            from: color,
                            to: TerrainColor(color.0 + 1),
                        }));
                        ui.close();
                    }
                    if ui.button("Remove").clicked() {
                        out.push(edit(TerrainSetEdit::RemoveTerrain {
                            set: label.set,
                            color,
                        }));
                        ui.close();
                    }
                });
                let mut probability = terrain.probability;
                let weight = ui
                    .add(
                        egui::DragValue::new(&mut probability)
                            .range(0.0..=1000.0)
                            .speed(0.01),
                    )
                    .on_hover_text("How often a fill picks this terrain");
                if weight.changed() && !weight.dragged() || weight.drag_stopped() {
                    out.push(edit(TerrainSetEdit::SetProbability {
                        set: label.set,
                        color,
                        probability,
                    }));
                }
            });
        }
    }

    /// The field a name is edited in, when this name is being edited; whether it was shown.
    fn rename_field(
        &mut self,
        ui: &mut egui::Ui,
        set: usize,
        color: Option<TerrainColor>,
        out: &mut Vec<Intent>,
        doc: DocumentId,
    ) -> bool {
        let Some(renaming) = &mut self.renaming else {
            return false;
        };
        if renaming.set != set || renaming.color != color {
            return false;
        }
        let response = ui.text_edit_singleline(&mut renaming.text);
        response.request_focus();
        let done = response.lost_focus();
        if done {
            let cancelled = ui.input(|input| input.key_pressed(egui::Key::Escape));
            if !cancelled {
                let name = renaming.text.clone();
                let edit = match color {
                    None => TerrainSetEdit::RenameSet { set, name },
                    Some(color) => TerrainSetEdit::RenameTerrain { set, color, name },
                };
                out.push(Intent::EditTerrainSets { doc, edit });
            }
            self.renaming = None;
        }
        true
    }
}

/// The lane of a tile under the pointer.
#[derive(Copy, Clone, Debug)]
struct HoveredLane {
    tile: TileId,
    lane: usize,
    frame: TileFrame,
}

/// Where a tile's lanes are drawn and found: its rectangle, or on an isometric tileset the
/// diamond Tiled's tileset view maps it to (`setupTilesetGridTransform`: the square around the
/// tile's middle turned 45° and flattened by the grid's height over its width), whose top point
/// is the tile's top-left corner.
#[derive(Copy, Clone, Debug)]
pub(super) struct TileFrame {
    /// The square, or the rectangle, the lanes are laid out in before the turn.
    rect: Rect,
    /// The grid's height over its width, for an isometric tileset.
    diamond: Option<f32>,
}

impl TileFrame {
    /// The frame of a tile drawn in `rect`; `diamond` is [`diamond_ratio`]'s.
    pub(super) fn of(rect: Rect, diamond: Option<f32>) -> TileFrame {
        let rect = match diamond {
            Some(_) => {
                Rect::from_center_size(rect.center(), egui::vec2(rect.width(), rect.width()))
            }
            None => rect,
        };
        TileFrame { rect, diamond }
    }

    /// Where a point of the laid-out square is drawn.
    fn to_screen(self, p: Pos2) -> Pos2 {
        let Some(ratio) = self.diamond else {
            return p;
        };
        let c = self.rect.center();
        let d = p - c;
        let half = std::f32::consts::FRAC_1_SQRT_2;
        // Turned 45°, then scaled by 1/√2 across and ratio/√2 down.
        let turned = egui::vec2(d.x * half - d.y * half, d.x * half + d.y * half);
        c + egui::vec2(turned.x * half, turned.y * half * ratio)
    }

    /// Where a drawn point is in the laid-out square, from 0 to 1 across and down.
    fn local(self, at: Pos2) -> egui::Vec2 {
        let p = match self.diamond {
            Some(ratio) if ratio > 0.0 => {
                let c = self.rect.center();
                let half = std::f32::consts::FRAC_1_SQRT_2;
                let scaled = egui::vec2((at.x - c.x) / half, (at.y - c.y) / (half * ratio));
                // Turned back 45°.
                c + egui::vec2(
                    scaled.x * half + scaled.y * half,
                    -scaled.x * half + scaled.y * half,
                )
            }
            _ => at,
        };
        (p - self.rect.min) / self.rect.size()
    }

    /// The region of a lane, as drawn.
    fn region(self, kind: WangSetType, lane: usize) -> Vec<Pos2> {
        lane_region(kind, lane, self.rect)
            .into_iter()
            .map(|p| self.to_screen(p))
            .collect()
    }
}

/// The grid's height over its width when the tileset's grid is isometric.
pub(super) fn diamond_ratio(tileset: &Tileset) -> Option<f32> {
    let grid = tileset.grid?;
    (grid.orientation == bt_core::GridOrientation::Isometric && grid.size.x > 0)
        .then(|| grid.size.y as f32 / grid.size.x as f32)
}

/// The lane at `x`, `y` (0 to 1 across the tile) for a set of `kind`, as Tiled's tileset view
/// finds it; the middle ninth is no lane.
fn lane_at(kind: WangSetType, x: f32, y: f32) -> Option<usize> {
    let grid = |v: f32| ((v * 3.0).floor() as i32).clamp(0, 2);
    let lane = match (grid(x), grid(y)) {
        (0, 0) => 7,
        (1, 0) => 0,
        (2, 0) => 1,
        (0, 1) => 6,
        (2, 1) => 2,
        (0, 2) => 5,
        (1, 2) => 4,
        (2, 2) => 3,
        _ => return None,
    };
    Some(match kind {
        WangSetType::Mixed => lane,
        WangSetType::Corner => match (x > 0.5, y > 0.5) {
            (true, true) => 3,
            (true, false) => 1,
            (false, true) => 5,
            (false, false) => 7,
        },
        WangSetType::Edge => {
            let (dx, dy) = (x - 0.5, y - 0.5);
            match (dx < dy, dx > -dy) {
                (true, true) => 4,
                (true, false) => 6,
                (false, true) => 2,
                (false, false) => 0,
            }
        }
    })
}

/// The part of a tile's rectangle that stands for a lane: a corner set's quadrant, an edge
/// set's triangle, a mixed set's ninth.
pub(super) fn lane_region(kind: WangSetType, lane: usize, rect: Rect) -> Vec<Pos2> {
    let at = |fx: f32, fy: f32| {
        pos2(
            rect.min.x + rect.width() * fx,
            rect.min.y + rect.height() * fy,
        )
    };
    let square =
        |x0: f32, y0: f32, x1: f32, y1: f32| vec![at(x0, y0), at(x1, y0), at(x1, y1), at(x0, y1)];
    match kind {
        WangSetType::Corner => match lane {
            1 => square(0.5, 0.0, 1.0, 0.5),
            3 => square(0.5, 0.5, 1.0, 1.0),
            5 => square(0.0, 0.5, 0.5, 1.0),
            _ => square(0.0, 0.0, 0.5, 0.5),
        },
        WangSetType::Edge => {
            let centre = at(0.5, 0.5);
            match lane {
                2 => vec![at(1.0, 0.0), at(1.0, 1.0), centre],
                4 => vec![at(1.0, 1.0), at(0.0, 1.0), centre],
                6 => vec![at(0.0, 1.0), at(0.0, 0.0), centre],
                _ => vec![at(0.0, 0.0), at(1.0, 0.0), centre],
            }
        }
        WangSetType::Mixed => {
            let third = 1.0 / 3.0;
            let (gx, gy) = match lane {
                0 => (1.0, 0.0),
                1 => (2.0, 0.0),
                2 => (2.0, 1.0),
                3 => (2.0, 2.0),
                4 => (1.0, 2.0),
                5 => (0.0, 2.0),
                6 => (0.0, 1.0),
                _ => (0.0, 0.0),
            };
            square(
                gx * third,
                gy * third,
                (gx + 1.0) * third,
                (gy + 1.0) * third,
            )
        }
    }
}

/// Draws a tile's labels of `set`: each lane's region in its terrain's colour.
pub(super) fn draw_labels(painter: &egui::Painter, set: &WangSet, id: TileId, frame: TileFrame) {
    let Some(label) = set.tiles.get(&id) else {
        return;
    };
    for (lane, &color) in label.lanes().iter().enumerate() {
        let Some(terrain) = usize::from(color)
            .checked_sub(1)
            .and_then(|i| set.colors.get(i))
        else {
            continue;
        };
        let [r, g, b, _] = terrain.color.to_array();
        painter.add(Shape::convex_polygon(
            frame.region(set.kind, lane),
            Color32::from_rgba_unmultiplied(r, g, b, 110),
            Stroke::new(1.0, Color32::from_rgb(r, g, b)),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lanes_are_found_as_tileds_tileset_view_finds_them() {
        assert_eq!(lane_at(WangSetType::Mixed, 0.5, 0.5), None, "the middle");
        assert_eq!(lane_at(WangSetType::Mixed, 0.1, 0.1), Some(7));
        assert_eq!(lane_at(WangSetType::Mixed, 0.5, 0.1), Some(0));
        assert_eq!(lane_at(WangSetType::Corner, 0.6, 0.1), Some(1));
        assert_eq!(lane_at(WangSetType::Corner, 0.4, 0.9), Some(5));
        assert_eq!(lane_at(WangSetType::Edge, 0.9, 0.5), Some(2));
        assert_eq!(lane_at(WangSetType::Edge, 0.5, 0.95), Some(4));
        assert_eq!(lane_at(WangSetType::Edge, 0.05, 0.5), Some(6));
        assert_eq!(lane_at(WangSetType::Edge, 0.5, 0.05), Some(0));
    }

    #[test]
    fn an_isometric_tileset_labels_on_its_diamond() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(64.0, 64.0));
        // A 2:1 grid: the diamond is 64 wide and 32 high around the tile's middle.
        let frame = TileFrame::of(rect, Some(0.5));
        let top = frame.to_screen(frame.rect.min);
        assert!(
            (top - pos2(32.0, 16.0)).length() < 1e-3,
            "the top-left corner is the top point: {top:?}"
        );
        let right = frame.to_screen(pos2(64.0, 0.0));
        assert!((right - pos2(64.0, 32.0)).length() < 1e-3, "{right:?}");
        // Just below the top point is the top-left corner's lane of a corner set.
        let local = frame.local(pos2(32.0, 20.0));
        assert_eq!(lane_at(WangSetType::Corner, local.x, local.y), Some(7));
        let back = frame.to_screen(frame.rect.min + frame.rect.size() * local);
        assert!(
            (back - pos2(32.0, 20.0)).length() < 1e-3,
            "round trip: {back:?}"
        );
    }
}
