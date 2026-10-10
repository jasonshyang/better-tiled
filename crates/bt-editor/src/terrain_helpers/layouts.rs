//! Layout… (`06-application.md`, §8): the blocks of autotile layouts found in the edited
//! tileset's image (read on a worker), one block labelled at once (Apply), and every block found
//! labelled at once with terrains told apart by their looks (Label All,
//! `bt_terrain::layouts::KnownTerrains::label_all`), each one step on the tileset. A set that
//! does not exist yet is made in the same step, and the terrains the blocks need past the set's
//! are added, named and coloured as new terrains are.

use std::collections::HashMap;

use bt_core::{Lane, TileId, Tileset, WangId, WangSet, WangSetType};
use bt_terrain::TerrainColor;
use bt_terrain::layouts::{Block, Fit, KnownTerrains, LAYOUTS};

use super::image_job::{FoundLayouts, ImageCut, ImagePurpose, TilesetImageJob};
use crate::document::DocumentId;
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;
use crate::notice::Notice;
use crate::store::TilesetHome;
use crate::terrain_sets::{LabelTerrain, new_terrain};

/// Where the labels of autotile layouts go.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LayoutTarget {
    /// The terrain set at this index of the tileset.
    Set(usize),
    /// A new set of this kind, made in the same step ("Unnamed Set", as New makes one).
    NewSet(WangSetType),
}

/// The autotile layouts looked for in a tileset's image (Layout…).
#[derive(Clone, Debug)]
pub enum LayoutSearch {
    /// The image is being read.
    Searching,
    /// The blocks found.
    Found(FoundLayouts),
    /// The image could not be read, and why.
    Failed(String),
}

/// A search, with the tileset and the cut of its image it was made for.
#[derive(Debug)]
pub(crate) struct StoredSearch {
    home: TilesetHome,
    cut: ImageCut,
    search: LayoutSearch,
}

/// Each document's last search.
pub(crate) type LayoutSearches = HashMap<DocumentId, StoredSearch>;

/// The name of a set Layout… makes.
const NEW_SET_NAME: &str = "Unnamed Set";

impl Editor {
    /// The autotile layouts last looked for in the image of the tileset `doc`'s tileset editor
    /// edits; `None` when none were looked for, or when the tileset's image or its cut into
    /// tiles changed since.
    pub fn layout_search(&self, doc: DocumentId) -> Option<&LayoutSearch> {
        let stored = self.layout_searches.get(&doc)?;
        let home = self.documents.get(doc)?.tileset_editing()?.home();
        let tileset = self.store.tileset_at(home)?;
        (stored.home == home && stored.cut.matches(tileset)).then_some(&stored.search)
    }

    /// The terrains the block of `fit` gets in `into` by its looks, without learning them: the
    /// set's terrains it looks like, or new ones (numbered past the set's). `None` until layouts
    /// were found, or when `into` names no set.
    pub fn layout_guess(&self, doc: DocumentId, into: LayoutTarget, fit: &Fit) -> Option<Block> {
        let Some(LayoutSearch::Found(found)) = self.layout_search(doc) else {
            return None;
        };
        let tileset = self.edited_tileset(doc)?;
        let empty;
        let set = match into {
            LayoutTarget::Set(index) => tileset.wang_sets.get(index)?,
            LayoutTarget::NewSet(kind) => {
                empty = empty_set(kind);
                &empty
            }
        };
        let known = KnownTerrains::from_set(&found.samples, set, &[]);
        Some(known.guess(&found.samples, fit))
    }

    /// What Label All labels in `into`: every block found whose layout is of the set's kind, in
    /// the order found, with the terrains `KnownTerrains::label_all` gives them (past the set's,
    /// new ones). Empty until layouts were found.
    pub fn label_all_blocks(&self, doc: DocumentId, into: LayoutTarget) -> Vec<Block> {
        let Some(LayoutSearch::Found(found)) = self.layout_search(doc) else {
            return Vec::new();
        };
        let Some(tileset) = self.edited_tileset(doc) else {
            return Vec::new();
        };
        let empty;
        let set = match into {
            LayoutTarget::Set(index) => match tileset.wang_sets.get(index) {
                Some(set) => set,
                None => return Vec::new(),
            },
            LayoutTarget::NewSet(kind) => {
                empty = empty_set(kind);
                &empty
            }
        };
        let fits: Vec<Fit> = found
            .fits
            .iter()
            .filter(|fit| LAYOUTS.get(fit.layout).is_some_and(|l| l.kind == set.kind))
            .cloned()
            .collect();
        KnownTerrains::from_set(&found.samples, set, &[]).label_all(&found.samples, &fits)
    }

    /// Layout…: looks for autotile layouts in the edited tileset's image, on a worker.
    pub(crate) fn find_layouts(&mut self, doc: DocumentId) -> Vec<Effect> {
        let Some(home) = self.edited_home(doc) else {
            return Vec::new();
        };
        let Some(tileset) = self.store.tileset_at(home).cloned() else {
            return Vec::new();
        };
        let Some(job) = TilesetImageJob::new(doc, home, tileset, ImagePurpose::FindLayouts) else {
            return vec![Effect::Notify(Notice::info(
                "A collection of images has no grid: no autotile layout fits it",
            ))];
        };
        let open: Vec<DocumentId> = self.documents.in_order().map(|d| d.id()).collect();
        self.layout_searches.retain(|d, _| open.contains(d));
        self.layout_searches.insert(
            doc,
            StoredSearch {
                home,
                cut: job.cut().clone(),
                search: LayoutSearch::Searching,
            },
        );
        vec![Effect::ReadTilesetImage(job)]
    }

    /// Takes in the layouts a worker found, when they are of the search under way.
    pub(crate) fn finish_finding_layouts(
        &mut self,
        doc: DocumentId,
        home: TilesetHome,
        cut: ImageCut,
        outcome: Result<FoundLayouts, String>,
    ) -> Vec<Effect> {
        let Some(stored) = self.layout_searches.get_mut(&doc) else {
            return Vec::new();
        };
        if stored.home != home || stored.cut != cut {
            // A later search is under way, for another tileset or another cut of its image.
            return Vec::new();
        }
        match outcome {
            Ok(found) => {
                stored.search = LayoutSearch::Found(found);
                Vec::new()
            }
            Err(problem) => {
                let text = format!("Cannot look for layouts in the tileset's image: {problem}");
                stored.search = LayoutSearch::Failed(problem);
                vec![Effect::Notify(Notice::warning(text))]
            }
        }
    }

    /// Apply: labels the block of a layout in `into`, as one step.
    pub(crate) fn apply_layout(
        &mut self,
        doc: DocumentId,
        into: LayoutTarget,
        block: Block,
    ) -> Vec<Effect> {
        let Some(home) = self.edited_home(doc) else {
            return Vec::new();
        };
        let Some(tileset) = self.store.tileset_at(home) else {
            return Vec::new();
        };
        if let Some(problem) = block_problem(tileset, into, &block) {
            return vec![Effect::Notify(Notice::warning(problem))];
        }
        let result = self.transact(doc, "Apply Layout", |tx| {
            label_blocks(tx.tileset_at_mut(home)?, into, &[block])
        });
        match result {
            Ok(set) => {
                self.choose_labelled_set(doc, set, block.terrain);
                Vec::new()
            }
            Err(error) => vec![warn(error)],
        }
    }

    /// Label All: labels every block found whose layout is of `into`'s kind, as one step.
    pub(crate) fn label_all_layouts(&mut self, doc: DocumentId, into: LayoutTarget) -> Vec<Effect> {
        let Some(home) = self.edited_home(doc) else {
            return Vec::new();
        };
        let Some(tileset) = self.store.tileset_at(home) else {
            return Vec::new();
        };
        let Some(kind) = target_kind(tileset, into) else {
            return vec![warn(EditError::NoSuchTerrainSet(target_index(into)))];
        };
        if !matches!(self.layout_search(doc), Some(LayoutSearch::Found(_))) {
            return vec![Effect::Notify(Notice::info(
                "Look for layouts in the tileset's image first (Layout…)",
            ))];
        }
        let blocks = self.label_all_blocks(doc, into);
        if blocks.is_empty() {
            return vec![Effect::Notify(Notice::info(format!(
                "No block of a {} layout was found in the tileset's image",
                kind_name(kind)
            )))];
        }
        let terrains_before = match into {
            LayoutTarget::Set(index) => tileset.wang_sets.get(index).map_or(0, |s| s.colors.len()),
            LayoutTarget::NewSet(_) => 0,
        };
        let result = self.transact(doc, "Label All Layouts", |tx| {
            label_blocks(tx.tileset_at_mut(home)?, into, &blocks)
        });
        let set = match result {
            Ok(set) => set,
            Err(error) => return vec![warn(error)],
        };
        self.choose_labelled_set(doc, set, TerrainColor(1));
        let terrains_after = self
            .edited_tileset(doc)
            .and_then(|t| t.wang_sets.get(set))
            .map_or(0, |s| s.colors.len());
        let added = terrains_after.saturating_sub(terrains_before);
        let mut text = format!(
            "Labelled {}",
            bt_utils::text::plural(blocks.len(), "block", "blocks")
        );
        if added > 0 {
            text.push_str(&format!(
                ", adding {}",
                bt_utils::text::plural(added, "terrain", "terrains")
            ));
        }
        vec![Effect::Notify(Notice::info(text))]
    }

    /// Where the tileset `doc`'s tileset editor edits lives.
    pub(crate) fn edited_home(&self, doc: DocumentId) -> Option<TilesetHome> {
        Some(self.documents.get(doc)?.tileset_editing()?.home())
    }

    /// After a layout labelled set `set`, its terrains are the ones labelling paints: `color`,
    /// unless a terrain of that set is chosen already.
    fn choose_labelled_set(&mut self, doc: DocumentId, set: usize, color: TerrainColor) {
        let Some(editing) = self.documents.get_mut(doc).and_then(|d| d.tileset.as_mut()) else {
            return;
        };
        if editing.label.is_none_or(|label| label.set != set) {
            editing.label = Some(LabelTerrain { set, color });
        }
    }
}

impl ImageCut {
    /// Whether `tileset`'s image is cut this way.
    pub(crate) fn matches(&self, tileset: &Tileset) -> bool {
        ImageCut::of(tileset).as_ref() == Some(self)
    }
}

/// Why `block` cannot be labelled in `into` of `tileset`, as a sentence; `None` when it can.
fn block_problem(tileset: &Tileset, into: LayoutTarget, block: &Block) -> Option<String> {
    let Some(layout) = LAYOUTS.get(block.layout) else {
        return Some(format!("There is no autotile layout {}", block.layout));
    };
    let Some(kind) = target_kind(tileset, into) else {
        return Some(EditError::NoSuchTerrainSet(target_index(into)).to_string());
    };
    if kind != layout.kind {
        return Some(format!(
            "“{}” labels a {} set, not a {} set: choose a {} set or a new one",
            layout.name,
            kind_name(layout.kind),
            kind_name(kind),
            kind_name(layout.kind)
        ));
    }
    if block.terrain.is_none() {
        return Some("Choose the terrain the block is of".to_owned());
    }
    if block.terrain == block.under {
        return Some(
            "A block's terrain and what it is drawn over must be different terrains".to_owned(),
        );
    }
    let size = layout.size();
    let columns = tileset.columns();
    let rows = tileset.tile_count().div_ceil(columns.max(1));
    let end = block.origin + size;
    if end.x > columns || end.y > rows {
        return Some(format!(
            "“{}” is {}×{} tiles: from column {}, row {} it goes past the tileset's {}×{}",
            layout.name, size.x, size.y, block.origin.x, block.origin.y, columns, rows
        ));
    }
    None
}

/// Labels `blocks` in the set `into` names (a new one goes after the others), adding the
/// terrains past the set's that they use, named and coloured as new terrains are; tiles a layout
/// leaves without a label lose theirs. A terrain without a tile to stand for it takes a tile of
/// the blocks that is all of it, and the set, without one, the first such tile of a block's
/// terrain. The set's index.
fn label_blocks(
    tileset: &mut Tileset,
    into: LayoutTarget,
    blocks: &[Block],
) -> Result<usize, EditError> {
    let columns = tileset.columns();
    let labels: Vec<(TerrainColor, TileId, Option<WangId>)> = blocks
        .iter()
        .flat_map(|block| {
            block
                .labels(columns)
                .into_iter()
                .map(|(tile, label)| (block.terrain, tile, label))
        })
        .collect();
    if let Some(&(_, missing, _)) = labels.iter().find(|(_, tile, _)| !tileset.has_tile(*tile)) {
        return Err(EditError::NoSuchTile(missing));
    }
    let index = match into {
        LayoutTarget::Set(index) => index,
        LayoutTarget::NewSet(kind) => {
            tileset.wang_sets.push(WangSet {
                name: NEW_SET_NAME.to_owned(),
                ..empty_set(kind)
            });
            tileset.wang_sets.len() - 1
        }
    };
    let set = tileset
        .wang_sets
        .get_mut(index)
        .ok_or(EditError::NoSuchTerrainSet(index))?;
    let needed = blocks
        .iter()
        .map(|block| usize::from(block.terrain.0.max(block.under.0)))
        .max()
        .unwrap_or(0);
    if needed > WangSet::MAX_COLORS {
        return Err(EditError::TooManyTerrains);
    }
    while set.colors.len() < needed {
        set.colors.push(new_terrain(set.colors.len()));
    }
    for (terrain, tile, label) in labels {
        let Some(label) = label else {
            set.tiles.remove(&tile);
            continue;
        };
        set.tiles.insert(tile, label);
        let Some(whole) = all_of_one_terrain(label, set.kind) else {
            continue;
        };
        if let Some(color) = usize::from(whole.0)
            .checked_sub(1)
            .and_then(|i| set.colors.get_mut(i))
        {
            color.tile.get_or_insert(tile);
        }
        if whole == terrain {
            set.tile.get_or_insert(tile);
        }
    }
    Ok(index)
}

/// The terrain on every lane of `kind` of a label, when one terrain is on all of them.
fn all_of_one_terrain(label: WangId, kind: WangSetType) -> Option<TerrainColor> {
    let mut lanes = Lane::ALL.iter().filter(|lane| match kind {
        WangSetType::Corner => lane.is_corner(),
        WangSetType::Edge => !lane.is_corner(),
        WangSetType::Mixed => true,
    });
    let first = label.lane(*lanes.next()?);
    (first != 0 && lanes.all(|lane| label.lane(*lane) == first)).then_some(TerrainColor(first))
}

/// A set of `kind` with no terrains and no labels.
fn empty_set(kind: WangSetType) -> WangSet {
    WangSet {
        name: String::new(),
        class: String::new(),
        kind,
        tile: None,
        colors: Vec::new(),
        tiles: Default::default(),
        properties: Default::default(),
    }
}

/// The kind of the set `into` names; `None` for a set the tileset lacks.
fn target_kind(tileset: &Tileset, into: LayoutTarget) -> Option<WangSetType> {
    match into {
        LayoutTarget::Set(index) => tileset.wang_sets.get(index).map(|set| set.kind),
        LayoutTarget::NewSet(kind) => Some(kind),
    }
}

/// The index `into` names, for a message about it.
fn target_index(into: LayoutTarget) -> usize {
    match into {
        LayoutTarget::Set(index) => index,
        LayoutTarget::NewSet(_) => 0,
    }
}

/// How a message names a kind of set.
fn kind_name(kind: WangSetType) -> &'static str {
    match kind {
        WangSetType::Corner => "corner",
        WangSetType::Edge => "edge",
        WangSetType::Mixed => "mixed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_is_of_one_terrain_on_the_lanes_of_its_kind() {
        let corners = WangId::from_lanes([0, 2, 0, 2, 0, 2, 0, 2]);
        assert_eq!(
            all_of_one_terrain(corners, WangSetType::Corner),
            Some(TerrainColor(2))
        );
        assert_eq!(all_of_one_terrain(corners, WangSetType::Mixed), None);
        let edge = WangId::from_lanes([1, 0, 1, 0, 2, 0, 1, 0]);
        assert_eq!(all_of_one_terrain(edge, WangSetType::Edge), None);
        assert_eq!(
            all_of_one_terrain(WangId::default(), WangSetType::Corner),
            None,
            "no terrain"
        );
    }
}
