//! New Tileset from RPG Maker autotiles (`06-application.md`, §9; `07-terrain.md`, §12): on a
//! worker, an [`AutotilesJob`] puts each A2 autotile of the sheet together into the 47 tiles of
//! a blob set, writes them as a new image beside the sheet (`<stem> blob.png`, never over a
//! file) and makes the tileset of that image, one terrain per autotile
//! (`bt_terrain::rpg_maker::{expand, tileset}`). The tileset comes back as
//! [`crate::Intent::AutotilesExpanded`] and is added to the map as New Tileset adds tilesets:
//! embedded, as one step. Undoing the step leaves the image written.

use std::path::PathBuf;

use bt_core::{Tileset, TilesetImage};
use bt_terrain::rpg_maker;

use crate::document::DocumentId;
use crate::editor::Editor;
use crate::effect::Effect;
use crate::notice::Notice;

/// RPG Maker autotiles to put together for New Tileset, on a worker with
/// [`AutotilesJob::run`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutotilesJob {
    doc: DocumentId,
    sheet: PathBuf,
    tile: u32,
    name: String,
}

/// What an [`AutotilesJob`] made, for [`crate::Intent::AutotilesExpanded`].
#[derive(Debug)]
pub struct ExpandedAutotiles {
    doc: DocumentId,
    sheet: PathBuf,
    /// The tileset, or why the sheet could not be put together.
    outcome: Result<Tileset, String>,
}

impl AutotilesJob {
    /// The autotiles of `sheet`, tiles `tile` pixels square, for a tileset named `name` added
    /// to `doc`'s map.
    pub(crate) fn new(doc: DocumentId, sheet: PathBuf, tile: u32, name: &str) -> AutotilesJob {
        AutotilesJob {
            doc,
            sheet,
            tile,
            name: name.to_owned(),
        }
    }

    /// The document whose map the tileset is for.
    pub fn doc(&self) -> DocumentId {
        self.doc
    }

    /// Reads the sheet, writes the blob sets beside it and makes their tileset; for a worker
    /// thread.
    pub fn run(&self) -> ExpandedAutotiles {
        let outcome = rpg_maker::expand(&self.sheet, self.tile)
            .map(|expansion| rpg_maker::tileset(&expansion, &self.name))
            .map_err(|error| error.to_string());
        ExpandedAutotiles {
            doc: self.doc,
            sheet: self.sheet.clone(),
            outcome,
        }
    }
}

impl Editor {
    /// Adds the tileset a worker made of RPG Maker autotiles to the map it is for, as one step,
    /// and says what was written.
    pub(crate) fn finish_expanding_autotiles(
        &mut self,
        expanded: ExpandedAutotiles,
    ) -> Vec<Effect> {
        let ExpandedAutotiles {
            doc,
            sheet,
            outcome,
        } = expanded;
        let sheet_name = bt_utils::paths::display_name(&sheet);
        let tileset = match outcome {
            Ok(tileset) => tileset,
            Err(problem) => {
                return vec![Effect::Notify(Notice::error(format!(
                    "Cannot make autotiles of “{sheet_name}”: {problem}"
                )))];
            }
        };
        let written = match &tileset.image {
            TilesetImage::Atlas { image, .. } => image
                .path()
                .map_or_else(String::new, bt_utils::paths::display_name),
            TilesetImage::Collection => String::new(),
        };
        if self.documents.get(doc).is_none() {
            return vec![Effect::Notify(Notice::warning(format!(
                "“{written}” was written beside “{sheet_name}”, but the map it was for is closed"
            )))];
        }
        let autotiles = tileset.wang_sets.first().map_or(0, |set| set.colors.len());
        let name = tileset.name.clone();
        let effects = self.add_new_tileset(doc, tileset);
        if !effects.is_empty() {
            return effects;
        }
        vec![Effect::Notify(Notice::info(format!(
            "Wrote “{written}” beside “{sheet_name}”: one terrain of “{name}” per autotile, {}",
            bt_utils::text::plural(autotiles, "autotile", "autotiles")
        )))]
    }
}
