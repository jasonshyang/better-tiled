//! The Terrains panel: the terrain sets of the Tiles panel's current tileset, each with an erase
//! row and its terrains (the colour, the tile that stands for it, the name); choosing one makes it
//! the terrain painted and the Terrain tool current.

use bt_core::{TilesetSlot, WangSetType};
use bt_editor::{DocumentId, Editor, Intent};
use bt_terrain::TerrainColor;
use egui::{Color32, Rect, Sense, vec2};

use super::tile_grid::{TileSource, paint_tile};

/// The side of a terrain's swatch and thumbnail, in points.
const THUMB: f32 = 20.0;

/// The Terrains panel (it keeps nothing of its own: the sets fold as egui remembers).
#[derive(Default)]
pub(crate) struct TerrainsPanel;

impl TerrainsPanel {
    /// Draws the sets of the tileset in slot `tileset` of `doc`'s map; a choice goes to `out`.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        editor: &Editor,
        doc: DocumentId,
        tileset: usize,
        source: &mut TileSource<'_>,
        out: &mut Vec<Intent>,
    ) {
        let (Some(map), Some(slot)) = (editor.map_of(doc), TilesetSlot::new(tileset)) else {
            return;
        };
        let Some(tileset) = map.tileset(slot) else {
            ui.weak("The map uses no tilesets.");
            return;
        };
        if tileset.wang_sets.is_empty() {
            ui.weak(format!("“{}” has no terrain sets.", tileset.name));
            return;
        }
        let home = editor
            .document(doc)
            .and_then(|d| editor.store().home_of_slot(d.asset(), slot));
        let chosen = editor.tools().terrain().filter(|c| Some(c.home) == home);
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (index, set) in tileset.wang_sets.iter().enumerate() {
                let kind = match set.kind {
                    WangSetType::Corner => "corners",
                    WangSetType::Edge => "edges",
                    WangSetType::Mixed => "corners and edges",
                };
                egui::CollapsingHeader::new(&set.name)
                    .id_salt(("terrain set", index))
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.weak(kind);
                        let is_chosen = |color: TerrainColor| {
                            chosen.is_some_and(|c| c.set == index && c.color == color)
                        };
                        let erase = TerrainColor::NONE;
                        if ui
                            .selectable_label(is_chosen(erase), "Erase terrain")
                            .on_hover_text("Paint no terrain: what the set draws over nothing")
                            .clicked()
                        {
                            out.push(choose(doc, slot, index, erase));
                        }
                        for (i, terrain) in set.colors.iter().enumerate() {
                            let color = TerrainColor(u8::try_from(i + 1).unwrap_or(u8::MAX));
                            let row = ui.horizontal(|ui| {
                                let (swatch, _) = ui
                                    .allocate_exact_size(vec2(THUMB / 2.0, THUMB), Sense::hover());
                                let [r, g, b, a] = terrain.color.to_array();
                                ui.painter().rect_filled(
                                    swatch,
                                    2.0,
                                    Color32::from_rgba_unmultiplied(r, g, b, a),
                                );
                                let (thumb, _) =
                                    ui.allocate_exact_size(vec2(THUMB, THUMB), Sense::hover());
                                if let Some(tile) = terrain.tile {
                                    paint_tile(ui, tileset, tile, square(thumb), source);
                                }
                                // Packs leave terrains unnamed; a number tells them apart.
                                let name = if terrain.name.is_empty() {
                                    egui::RichText::new(format!("Terrain {}", i + 1)).italics()
                                } else {
                                    egui::RichText::new(&terrain.name)
                                };
                                ui.selectable_label(is_chosen(color), name)
                            });
                            if row.inner.clicked() {
                                out.push(choose(doc, slot, index, color));
                            }
                        }
                    });
            }
        });
    }
}

fn choose(doc: DocumentId, slot: TilesetSlot, set: usize, color: TerrainColor) -> Intent {
    Intent::ChooseTerrain {
        doc,
        slot,
        set,
        color,
    }
}

/// The square a thumbnail is drawn in.
fn square(rect: Rect) -> Rect {
    let side = rect.width().min(rect.height());
    Rect::from_center_size(rect.center(), vec2(side, side))
}
