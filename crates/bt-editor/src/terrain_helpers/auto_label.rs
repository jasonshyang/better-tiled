//! Auto-label (`06-application.md`, §8; `07-terrain.md`, §10): the terrain the Terrain mode
//! labels with is labelled on every tile at once, from one tile that is all of it. The tileset's
//! image is read on a worker; the labels it proposes are applied as one step on the tileset
//! (`bt_terrain::autolabel::Proposal::apply_to`: other terrains' labels stay), and a notice says
//! how many tiles the terrain was found on.

use bt_core::TileId;
use bt_terrain::autolabel::{Method, Proposal};

use super::image_job::{AutoLabelRequest, ImagePurpose, TilesetImageJob};
use crate::document::DocumentId;
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;
use crate::notice::Notice;
use crate::store::TilesetHome;

impl Editor {
    /// Auto-label: labels the chosen terrain from tile `tile`, which is all of it; the image is
    /// read on a worker.
    pub(crate) fn auto_label(&mut self, doc: DocumentId, tile: TileId) -> Vec<Effect> {
        let Some(editing) = self.documents.get(doc).and_then(|d| d.tileset_editing()) else {
            return Vec::new();
        };
        let home = editing.home();
        let Some(label) = editing.label_terrain().filter(|l| !l.color.is_none()) else {
            return vec![Effect::Notify(Notice::info(
                "Choose the terrain to auto-label first",
            ))];
        };
        let Some(tileset) = self.store.tileset_at(home).cloned() else {
            return Vec::new();
        };
        let Some(set) = tileset.wang_sets.get(label.set) else {
            return vec![warn(EditError::NoSuchTerrainSet(label.set))];
        };
        if usize::from(label.color.0) > set.colors.len() {
            return vec![warn(EditError::NoSuchTerrain(label.color.0))];
        }
        if !tileset.has_tile(tile) {
            return vec![warn(EditError::NoSuchTile(tile))];
        }
        let request = AutoLabelRequest {
            set: label.set,
            kind: set.kind,
            color: label.color,
            reference: tile,
        };
        match TilesetImageJob::new(doc, home, tileset, ImagePurpose::AutoLabel(request)) {
            Some(job) => vec![Effect::ReadTilesetImage(job)],
            None => vec![Effect::Notify(Notice::info(
                "Auto-label reads one image cut into tiles: a collection of images has none",
            ))],
        }
    }

    /// Applies the labels a worker proposed, as one step, and says what it found.
    pub(crate) fn finish_auto_labelling(
        &mut self,
        doc: DocumentId,
        home: TilesetHome,
        request: AutoLabelRequest,
        proposal: Result<Proposal, String>,
    ) -> Vec<Effect> {
        let proposal = match proposal {
            Ok(proposal) => proposal,
            Err(problem) => {
                return vec![Effect::Notify(Notice::warning(format!(
                    "Cannot auto-label: {problem}"
                )))];
            }
        };
        if self.documents.get(doc).is_none() {
            return Vec::new();
        }
        let name = self
            .store
            .tileset_at(home)
            .and_then(|t| t.wang_sets.get(request.set))
            .and_then(|s| s.colors.get(usize::from(request.color.0).saturating_sub(1)))
            .map_or_else(
                || format!("terrain {}", request.color.0),
                |c| c.name.clone(),
            );
        if proposal.tiles.is_empty() {
            return vec![Effect::Notify(Notice::info(format!(
                "No tile looks like tile {}: “{name}” was labelled nowhere",
                request.reference.0
            )))];
        }
        let result = self.transact(doc, "Auto-label Terrain", |tx| {
            let set = tx
                .tileset_at_mut(home)?
                .wang_sets
                .get_mut(request.set)
                .ok_or(EditError::NoSuchTerrainSet(request.set))?;
            if set.kind != request.kind || usize::from(request.color.0) > set.colors.len() {
                // The set changed while the image was read.
                return Err(EditError::NoSuchTerrain(request.color.0));
            }
            Ok(proposal.apply_to(&mut set.tiles))
        });
        let kept = match result {
            Ok(kept) => kept,
            Err(error) => return vec![warn(error)],
        };
        let how = match proposal.method {
            Method::Transparency => "drawn over nothing",
            Method::Auto | Method::Learned => "told apart from the rest by its colours",
        };
        let mut text = format!(
            "Auto-label found “{name}” on {}, {how}",
            bt_utils::text::plural(proposal.tiles.len(), "tile", "tiles")
        );
        if kept > 0 {
            text.push_str(&format!(
                "; {} of another terrain kept",
                bt_utils::text::plural(kept, "lane", "lanes")
            ));
        }
        vec![Effect::Notify(Notice::info(text))]
    }
}
