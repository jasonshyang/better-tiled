//! The Map menu's commands that need no dialog: Crop to Selection and Autocrop, each one step;
//! the selection moves with the map's contents.

use bt_edit::{ResizeReport, autocrop, crop_to};

use crate::document::DocumentId;
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;

impl Editor {
    /// Crop to Selection: the map cut down to the selected cells' bounding rectangle.
    pub(crate) fn crop_to_selection(&mut self, doc: DocumentId) -> Vec<Effect> {
        let Some(bounds) = self
            .documents
            .get(doc)
            .and_then(|d| d.selection().tiles.bounds())
        else {
            return Vec::new();
        };
        self.resize_command(doc, "Crop to Selection", |map| crop_to(map, bounds))
    }

    /// Autocrop: the map cut down to the cells its tiles use.
    pub(crate) fn autocrop_map(&mut self, doc: DocumentId) -> Vec<Effect> {
        self.resize_command(doc, "Autocrop", autocrop)
    }

    /// Runs a resize as one step; the selected cells move with the map's contents.
    fn resize_command(
        &mut self,
        doc: DocumentId,
        label: &str,
        resize: impl FnOnce(&mut bt_core::Map) -> Result<ResizeReport, bt_edit::EditError>,
    ) -> Vec<Effect> {
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let result = self.transact(doc, label, |tx| {
            let map = tx.map_mut(asset)?;
            let report = resize(map).map_err(EditError::from)?;
            let moved = tx.selection.tiles.translated(report.tile_offset);
            tx.selection.tiles = moved;
            Ok::<_, EditError>(())
        });
        result.err().map(warn).into_iter().collect()
    }
}
