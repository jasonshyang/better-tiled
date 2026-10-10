//! Patterns (`06-application.md`, §8; `07-terrain.md`, §11): the combinations of terrains a set
//! needs, with how many tiles have each (`bt_terrain::patterns::needed`), and a tile labelled
//! with a whole pattern, each click one step, as Tiled's Patterns tab assigns a pattern to the
//! tiles clicked.

use bt_core::{Lane, TileId, TilesetSlot, WangId};
use bt_terrain::Terrain;
use bt_terrain::patterns::{Patterns, needed};

use crate::document::DocumentId;
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;
use crate::notice::Notice;
use crate::store::TilesetHome;

impl Editor {
    /// The patterns set `set` of the tileset `doc`'s tileset editor edits needs, with how many
    /// tiles (flipped and turned variants the tileset allows included) have each; `None` when it
    /// has no such set.
    pub fn needed_patterns(&self, doc: DocumentId, set: usize) -> Option<Patterns> {
        let home = self.edited_home(doc)?;
        let tileset = self.store.tileset_at(home)?;
        // The slot only says where painted tiles would come from; patterns do not paint.
        let slot = match home {
            TilesetHome::Embedded { slot, .. } => slot,
            TilesetHome::Asset(_) => TilesetSlot::new(0)?,
        };
        let terrain = Terrain::from_tileset(tileset, slot, set).ok()?;
        Some(needed(&terrain))
    }

    /// Patterns: labels `tile` with `pattern` in the set the Terrain mode labels with, as one
    /// step; its label becomes the pattern on the lanes of the set's kind (all of no terrain
    /// removes it).
    pub(crate) fn label_pattern(
        &mut self,
        doc: DocumentId,
        pattern: WangId,
        tile: TileId,
    ) -> Vec<Effect> {
        let Some(editing) = self.documents.get(doc).and_then(|d| d.tileset_editing()) else {
            return Vec::new();
        };
        let home = editing.home();
        let Some(label) = editing.label_terrain() else {
            return vec![Effect::Notify(Notice::info(
                "Choose the terrain set to label first",
            ))];
        };
        let result = self.transact(doc, "Label Terrain Pattern", |tx| {
            let tileset = tx.tileset_at_mut(home)?;
            if !tileset.has_tile(tile) {
                return Err(EditError::NoSuchTile(tile));
            }
            let set = tileset
                .wang_sets
                .get_mut(label.set)
                .ok_or(EditError::NoSuchTerrainSet(label.set))?;
            let mut lanes = [0_u8; 8];
            for lane in Lane::ALL {
                if !set.kind.uses(lane) {
                    continue;
                }
                let color = pattern.lane(lane);
                if usize::from(color) > set.colors.len() {
                    return Err(EditError::NoSuchTerrain(color));
                }
                if let Some(slot) = lanes.get_mut(lane.index()) {
                    *slot = color;
                }
            }
            let labelled = WangId::from_lanes(lanes);
            if labelled.is_empty() {
                set.tiles.remove(&tile);
            } else {
                set.tiles.insert(tile, labelled);
            }
            Ok(())
        });
        result.err().map(warn).into_iter().collect()
    }
}
