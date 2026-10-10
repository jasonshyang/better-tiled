//! Offset Map: how far the contents move in tiles, whether they wrap along each axis, which
//! layers move and within what bounds, as Tiled's dialog asks them (`OffsetMapDialog`,
//! `reference/tiled/src/tiled/offsetmapdialog.cpp`). The bounds can be the selection's only when
//! tiles are selected (and are then by default); wrapping needs bounds, so it is off for the whole
//! of an infinite map. OK moves the contents as one step; Escape cancels.

use bt_editor::{DocumentId, Editor, Intent, LayerChoice, MapOffset};

use super::form::{ButtonsAnswer, DialogKeys, choice, dialog_buttons};

/// The furthest the dialog moves the contents, in tiles (Tiled's).
const MAX_CELLS: i32 = 999;

const LAYER_CHOICES: [(LayerChoice, &str); 3] = [
    (LayerChoice::AllVisible, "All Visible Layers"),
    (LayerChoice::All, "All Layers"),
    (LayerChoice::Selected, "Selected Layers"),
];

const BOUNDS: [(bool, &str); 2] = [(false, "Whole Map"), (true, "Current Selection")];

/// The Offset Map dialog's state.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OffsetMapDialog {
    doc: DocumentId,
    offset: MapOffset,
    /// Whether tiles are selected, so the bounds can be the selection's.
    has_selection: bool,
    /// Whether the map is infinite: its whole has no edges to wrap around.
    infinite: bool,
}

impl OffsetMapDialog {
    /// A dialog for the map of `doc`: within the selection when tiles are selected.
    pub(crate) fn new(doc: DocumentId, editor: &Editor) -> OffsetMapDialog {
        let has_selection = editor
            .document(doc)
            .is_some_and(|d| !d.selection().tiles.is_empty());
        let infinite = editor.map_of(doc).is_some_and(|map| map.infinite);
        OffsetMapDialog {
            doc,
            offset: MapOffset {
                within_selection: has_selection,
                ..MapOffset::default()
            },
            has_selection,
            infinite,
        }
    }

    /// Whether wrapping can be asked for: there are edges to wrap around (Tiled's
    /// `OffsetMapDialog::boundsSelectionChanged`).
    fn wraps(&self) -> bool {
        self.offset.within_selection || !self.infinite
    }

    /// Shows the dialog; the offset goes to `out`. Whether it closed.
    pub(crate) fn show(&mut self, ctx: &egui::Context, out: &mut Vec<Intent>) -> bool {
        let keys = DialogKeys::take(ctx);
        let mut answer = ButtonsAnswer::None;
        let response = egui::Modal::new(egui::Id::new("offset_map")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.heading("Offset Map");
            ui.add_space(6.0);
            let wraps = self.wraps();
            if !wraps {
                self.offset.wrap_x = false;
                self.offset.wrap_y = false;
            }
            egui::Grid::new("offset_map_fields")
                .num_columns(3)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    offset_row(
                        ui,
                        "X",
                        &mut self.offset.cells.x,
                        &mut self.offset.wrap_x,
                        wraps,
                    );
                    offset_row(
                        ui,
                        "Y",
                        &mut self.offset.cells.y,
                        &mut self.offset.wrap_y,
                        wraps,
                    );
                    ui.label("Layers");
                    choice(ui, "offset_layers", &mut self.offset.layers, &LAYER_CHOICES);
                    ui.end_row();
                    ui.label("Bounds");
                    ui.add_enabled_ui(self.has_selection, |ui| {
                        choice(
                            ui,
                            "offset_bounds",
                            &mut self.offset.within_selection,
                            &BOUNDS,
                        );
                    });
                    ui.end_row();
                });
            ui.add_space(6.0);
            answer = dialog_buttons(ui, true);
        });
        if answer == ButtonsAnswer::Ok || keys.enter {
            out.push(Intent::OffsetMap {
                doc: self.doc,
                offset: self.offset,
            });
            return true;
        }
        answer == ButtonsAnswer::Cancel || keys.escape || response.should_close()
    }
}

/// A labelled number of tiles along one axis, with its Wrap box.
fn offset_row(ui: &mut egui::Ui, label: &str, cells: &mut i32, wrap: &mut bool, wraps: bool) {
    let label = ui.label(label);
    ui.add(
        egui::DragValue::new(cells)
            .range(-MAX_CELLS..=MAX_CELLS)
            .suffix(" tiles"),
    )
    .labelled_by(label.id);
    ui.add_enabled(wraps, egui::Checkbox::new(wrap, "Wrap"));
    ui.end_row();
}

#[cfg(test)]
mod tests {
    use bt_core::Map;
    use bt_editor::{SelectionMode, TileRegion};
    use bt_utils::IRect;
    use glam::IVec2;

    use super::*;

    fn editor_with(map: Map) -> (Editor, DocumentId) {
        let mut editor = Editor::new();
        let doc = editor.open_new_map(map);
        (editor, doc)
    }

    #[test]
    fn the_bounds_are_the_selections_when_tiles_are_selected() {
        let map = Map {
            width: 4,
            height: 4,
            ..Map::default()
        };
        let (mut editor, doc) = editor_with(map);
        assert!(!OffsetMapDialog::new(doc, &editor).offset.within_selection);
        editor.apply(Intent::SelectTiles {
            doc,
            region: TileRegion::from_rect(IRect::from_min_size(IVec2::ZERO, IVec2::ONE)),
            mode: SelectionMode::Replace,
        });
        let dialog = OffsetMapDialog::new(doc, &editor);
        assert!(dialog.has_selection && dialog.offset.within_selection);
    }

    #[test]
    fn the_whole_of_an_infinite_map_does_not_wrap() {
        let map = Map {
            infinite: true,
            ..Map::default()
        };
        let (editor, doc) = editor_with(map);
        let mut dialog = OffsetMapDialog::new(doc, &editor);
        assert!(!dialog.wraps());
        dialog.offset.within_selection = true;
        assert!(dialog.wraps(), "the selection has edges");
    }
}
