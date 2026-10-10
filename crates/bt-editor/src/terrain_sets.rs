//! The tileset editor's Terrain mode: the terrain its labelling paints, labelling strokes, and
//! the commands of the tileset's terrain sets (a new set of each kind, rename, duplicate,
//! remove; terrains added, removed, renamed, recoloured, reordered and weighted), each a step on
//! the tileset where it lives. Labels follow Tiled's tileset view (`reference/tiled/src/tiled/
//! tilesetview.cpp`): one lane at a time, a label left all "no terrain" is removed.

use bt_core::{Color, Lane, TileId, Tileset, WangColor, WangId, WangSet, WangSetType};
use bt_terrain::TerrainColor;

use crate::document::DocumentId;
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;
use crate::notice::Notice;

/// The colours new terrains take, in turn (Tiled's `defaultWangColors`; past them the list
/// starts again, where Tiled picks at random).
const NEW_TERRAIN_COLORS: [Color; 15] = [
    Color::rgb(255, 0, 0),
    Color::rgb(0, 255, 0),
    Color::rgb(0, 0, 255),
    Color::rgb(255, 119, 0),
    Color::rgb(0, 233, 255),
    Color::rgb(255, 0, 216),
    Color::rgb(255, 255, 0),
    Color::rgb(160, 0, 255),
    Color::rgb(0, 255, 161),
    Color::rgb(255, 168, 168),
    Color::rgb(180, 168, 255),
    Color::rgb(150, 255, 167),
    Color::rgb(142, 120, 72),
    Color::rgb(90, 90, 90),
    Color::rgb(14, 122, 70),
];

/// The terrain the Terrain mode's labelling paints: a set of the tileset and one of its
/// terrains ([`TerrainColor::NONE`] erases labels).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct LabelTerrain {
    /// The set's index in the tileset.
    pub set: usize,
    /// The terrain.
    pub color: TerrainColor,
}

/// One lane of one tile to label.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LabelMark {
    /// The tile.
    pub tile: TileId,
    /// The lane.
    pub lane: Lane,
    /// Clear the lane instead of painting the chosen terrain (the right button).
    pub erase: bool,
}

/// A step of a labelling stroke.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LabelStroke {
    /// A stroke starts in a document's tileset editor.
    Begin {
        /// The document.
        doc: DocumentId,
    },
    /// These lanes take the chosen terrain (or are cleared).
    Paint {
        /// The lanes.
        marks: Vec<LabelMark>,
    },
    /// The stroke is done: one step of the history.
    End,
    /// The stroke is dropped.
    Cancel,
}

/// A change to the tileset's terrain sets.
#[derive(Clone, Debug, PartialEq)]
pub enum TerrainSetEdit {
    /// A new set of `kind` named "Unnamed Set" with one terrain, as Tiled makes it.
    AddSet {
        /// Which lanes it uses.
        kind: WangSetType,
    },
    /// The set and its labels go.
    RemoveSet {
        /// The set.
        set: usize,
    },
    /// A copy of the set after it.
    DuplicateSet {
        /// The set.
        set: usize,
    },
    /// The set's name.
    RenameSet {
        /// The set.
        set: usize,
        /// Its name.
        name: String,
    },
    /// A new terrain at the end of the set.
    AddTerrain {
        /// The set.
        set: usize,
    },
    /// A terrain goes: its lanes become "no terrain" and the terrains after it move down.
    RemoveTerrain {
        /// The set.
        set: usize,
        /// The terrain.
        color: TerrainColor,
    },
    /// A terrain's name.
    RenameTerrain {
        /// The set.
        set: usize,
        /// The terrain.
        color: TerrainColor,
        /// Its name.
        name: String,
    },
    /// A terrain's colour.
    RecolorTerrain {
        /// The set.
        set: usize,
        /// The terrain.
        color: TerrainColor,
        /// The colour it is shown in.
        rgb: Color,
    },
    /// A terrain's weight when fills choose terrains.
    SetProbability {
        /// The set.
        set: usize,
        /// The terrain.
        color: TerrainColor,
        /// The weight.
        probability: f64,
    },
    /// A terrain moves to another place in the list; the labels follow it.
    MoveTerrain {
        /// The set.
        set: usize,
        /// The terrain.
        from: TerrainColor,
        /// Its place after the move.
        to: TerrainColor,
    },
}

impl TerrainSetEdit {
    fn label(&self) -> &'static str {
        match self {
            TerrainSetEdit::AddSet { .. } => "Add Terrain Set",
            TerrainSetEdit::RemoveSet { .. } => "Remove Terrain Set",
            TerrainSetEdit::DuplicateSet { .. } => "Duplicate Terrain Set",
            TerrainSetEdit::RenameSet { .. } => "Rename Terrain Set",
            TerrainSetEdit::AddTerrain { .. } => "Add Terrain",
            TerrainSetEdit::RemoveTerrain { .. } => "Remove Terrain",
            TerrainSetEdit::RenameTerrain { .. } => "Rename Terrain",
            TerrainSetEdit::RecolorTerrain { .. } => "Change Terrain Color",
            TerrainSetEdit::SetProbability { .. } => "Change Terrain Probability",
            TerrainSetEdit::MoveTerrain { .. } => "Move Terrain",
        }
    }
}

/// A labelling stroke under way.
#[derive(Clone, Debug)]
pub(crate) struct LabelStrokeState {
    doc: DocumentId,
    label: LabelTerrain,
}

impl Editor {
    /// Chooses the set and terrain the Terrain mode's labelling paints.
    pub(crate) fn choose_label_terrain(
        &mut self,
        doc: DocumentId,
        label: LabelTerrain,
    ) -> Vec<Effect> {
        let Some(tileset) = self.edited_tileset(doc) else {
            return Vec::new();
        };
        let Some(set) = tileset.wang_sets.get(label.set) else {
            return vec![warn(EditError::NoSuchTerrainSet(label.set))];
        };
        if usize::from(label.color.0) > set.colors.len() {
            return vec![warn(EditError::NoSuchTerrain(label.color.0))];
        }
        if let Some(editing) = self.documents.get_mut(doc).and_then(|d| d.tileset.as_mut()) {
            editing.label = Some(label);
        }
        Vec::new()
    }

    /// Takes a step of a labelling stroke.
    pub(crate) fn label_stroke(&mut self, step: LabelStroke) -> Vec<Effect> {
        match step {
            LabelStroke::Begin { doc } => {
                let label = self
                    .documents
                    .get(doc)
                    .and_then(|d| d.tileset_editing())
                    .and_then(|editing| editing.label_terrain());
                let Some(label) = label else {
                    return vec![Effect::Notify(Notice::info(
                        "Choose a terrain to label tiles with first",
                    ))];
                };
                if let Err(error) = self.begin_gesture(doc, "Label Terrain") {
                    return vec![warn(error)];
                }
                self.label_stroke = Some(LabelStrokeState { doc, label });
                Vec::new()
            }
            LabelStroke::Paint { marks } => {
                let Some(stroke) = self.label_stroke.clone() else {
                    return Vec::new();
                };
                let Some(home) = self
                    .documents
                    .get(stroke.doc)
                    .and_then(|d| d.tileset_editing())
                    .map(|editing| editing.home())
                else {
                    return Vec::new();
                };
                let result = self.gesture_apply(|tx| {
                    let tileset = tx.tileset_at_mut(home)?;
                    label_tiles(tileset, stroke.label, &marks)
                });
                result.err().map(warn).into_iter().collect()
            }
            LabelStroke::End => {
                if self.label_stroke.take().is_none() {
                    return Vec::new();
                }
                match self.end_gesture() {
                    Ok(_) => Vec::new(),
                    Err(error) => vec![warn(error)],
                }
            }
            LabelStroke::Cancel => {
                self.label_stroke = None;
                match self.cancel_gesture() {
                    Ok(()) => Vec::new(),
                    Err(error) => vec![warn(error)],
                }
            }
        }
    }

    /// Changes the edited tileset's terrain sets, as one step.
    pub(crate) fn edit_terrain_sets(
        &mut self,
        doc: DocumentId,
        edit: TerrainSetEdit,
    ) -> Vec<Effect> {
        let Some(home) = self
            .documents
            .get(doc)
            .and_then(|d| d.tileset_editing())
            .map(|editing| editing.home())
        else {
            return Vec::new();
        };
        let label = edit.label();
        let result = self.transact(doc, label, |tx| {
            apply_set_edit(tx.tileset_at_mut(home)?, &edit)
        });
        if let Err(error) = result {
            return vec![warn(error)];
        }
        self.follow_set_edit(doc, &edit);
        Vec::new()
    }

    /// Keeps the labelling's choice on the same set and terrain after an edit moves them.
    fn follow_set_edit(&mut self, doc: DocumentId, edit: &TerrainSetEdit) {
        let sets = self.edited_tileset(doc).map_or(0, |t| t.wang_sets.len());
        let Some(editing) = self.documents.get_mut(doc).and_then(|d| d.tileset.as_mut()) else {
            return;
        };
        let label = editing.label;
        editing.label = match (edit, label) {
            (TerrainSetEdit::AddSet { .. }, _) => Some(LabelTerrain {
                set: sets.saturating_sub(1),
                color: TerrainColor(1),
            }),
            (TerrainSetEdit::DuplicateSet { set }, Some(label)) if label.set > *set => {
                Some(LabelTerrain {
                    set: label.set + 1,
                    ..label
                })
            }
            (TerrainSetEdit::RemoveSet { set }, Some(label)) if label.set == *set => None,
            (TerrainSetEdit::RemoveSet { set }, Some(label)) if label.set > *set => {
                Some(LabelTerrain {
                    set: label.set - 1,
                    ..label
                })
            }
            (TerrainSetEdit::RemoveTerrain { set, color }, Some(label)) if label.set == *set => {
                Some(LabelTerrain {
                    color: if label.color == *color {
                        TerrainColor::NONE
                    } else if label.color > *color {
                        TerrainColor(label.color.0 - 1)
                    } else {
                        label.color
                    },
                    ..label
                })
            }
            (TerrainSetEdit::MoveTerrain { set, from, to }, Some(label)) if label.set == *set => {
                Some(LabelTerrain {
                    color: moved_color(label.color, *from, *to),
                    ..label
                })
            }
            (_, label) => label,
        };
    }
}

/// Paints the marks with the label's terrain (or clears them); a label left all "no terrain" is
/// removed.
fn label_tiles(
    tileset: &mut Tileset,
    label: LabelTerrain,
    marks: &[LabelMark],
) -> Result<(), EditError> {
    for mark in marks {
        if !tileset.has_tile(mark.tile) {
            return Err(EditError::NoSuchTile(mark.tile));
        }
    }
    let set = tileset
        .wang_sets
        .get_mut(label.set)
        .ok_or(EditError::NoSuchTerrainSet(label.set))?;
    if usize::from(label.color.0) > set.colors.len() {
        return Err(EditError::NoSuchTerrain(label.color.0));
    }
    for mark in marks {
        let current = set.tiles.get(&mark.tile).copied().unwrap_or_default();
        let color = if mark.erase { 0 } else { label.color.0 };
        let labelled = current.with_lane(mark.lane, color);
        if labelled.is_empty() {
            set.tiles.remove(&mark.tile);
        } else {
            set.tiles.insert(mark.tile, labelled);
        }
    }
    Ok(())
}

/// Applies a terrain set edit to the tileset.
fn apply_set_edit(tileset: &mut Tileset, edit: &TerrainSetEdit) -> Result<(), EditError> {
    match edit {
        TerrainSetEdit::AddSet { kind } => {
            tileset.wang_sets.push(WangSet {
                name: "Unnamed Set".to_owned(),
                class: String::new(),
                kind: *kind,
                tile: None,
                colors: vec![new_terrain(0)],
                tiles: Default::default(),
                properties: Default::default(),
            });
        }
        TerrainSetEdit::RemoveSet { set } => {
            if *set >= tileset.wang_sets.len() {
                return Err(EditError::NoSuchTerrainSet(*set));
            }
            tileset.wang_sets.remove(*set);
        }
        TerrainSetEdit::DuplicateSet { set } => {
            let mut copy = set_mut(tileset, *set)?.clone();
            copy.name = format!("Copy of {}", copy.name);
            tileset.wang_sets.insert(set + 1, copy);
        }
        TerrainSetEdit::RenameSet { set, name } => set_mut(tileset, *set)?.name = name.clone(),
        TerrainSetEdit::AddTerrain { set } => {
            let set = set_mut(tileset, *set)?;
            if set.colors.len() >= WangSet::MAX_COLORS {
                return Err(EditError::TooManyTerrains);
            }
            set.colors.push(new_terrain(set.colors.len()));
        }
        TerrainSetEdit::RemoveTerrain { set, color } => {
            let set = set_mut(tileset, *set)?;
            terrain_index(set, *color)?;
            set.colors.remove(usize::from(color.0) - 1);
            relabel(set, |c| match c.cmp(&color.0) {
                std::cmp::Ordering::Less => c,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => c - 1,
            });
        }
        TerrainSetEdit::RenameTerrain { set, color, name } => {
            let set = set_mut(tileset, *set)?;
            let index = terrain_index(set, *color)?;
            if let Some(terrain) = set.colors.get_mut(index) {
                terrain.name = name.clone();
            }
        }
        TerrainSetEdit::RecolorTerrain { set, color, rgb } => {
            let set = set_mut(tileset, *set)?;
            let index = terrain_index(set, *color)?;
            if let Some(terrain) = set.colors.get_mut(index) {
                terrain.color = *rgb;
            }
        }
        TerrainSetEdit::SetProbability {
            set,
            color,
            probability,
        } => {
            if !probability.is_finite() || *probability < 0.0 {
                return Err(EditError::BadProbability(*probability));
            }
            let set = set_mut(tileset, *set)?;
            let index = terrain_index(set, *color)?;
            if let Some(terrain) = set.colors.get_mut(index) {
                terrain.probability = *probability;
            }
        }
        TerrainSetEdit::MoveTerrain { set, from, to } => {
            let set = set_mut(tileset, *set)?;
            let from_index = terrain_index(set, *from)?;
            let to_index = terrain_index(set, *to)?;
            let moved = set.colors.remove(from_index);
            set.colors.insert(to_index, moved);
            relabel(set, |c| moved_color(TerrainColor(c), *from, *to).0);
        }
    }
    Ok(())
}

/// Where terrain `color` is after the terrain at `from` moves to `to`.
fn moved_color(color: TerrainColor, from: TerrainColor, to: TerrainColor) -> TerrainColor {
    let c = color.0;
    let (f, t) = (from.0, to.0);
    TerrainColor(if c == f {
        t
    } else if f < t && c > f && c <= t {
        c - 1
    } else if t < f && c >= t && c < f {
        c + 1
    } else {
        c
    })
}

/// Rewrites every lane of every label through `map` (0 stays 0); labels left empty go.
fn relabel(set: &mut WangSet, map: impl Fn(u8) -> u8) {
    let tiles: Vec<(TileId, WangId)> = set.tiles.iter().map(|(t, w)| (*t, *w)).collect();
    for (tile, wang) in tiles {
        let lanes = wang.lanes().map(|c| if c == 0 { 0 } else { map(c) });
        let relabelled = WangId::from_lanes(lanes);
        if relabelled.is_empty() {
            set.tiles.remove(&tile);
        } else {
            set.tiles.insert(tile, relabelled);
        }
    }
}

fn set_mut(tileset: &mut Tileset, set: usize) -> Result<&mut WangSet, EditError> {
    tileset
        .wang_sets
        .get_mut(set)
        .ok_or(EditError::NoSuchTerrainSet(set))
}

/// The index of `color` in the set's list; "no terrain" and terrains past the end are errors.
fn terrain_index(set: &WangSet, color: TerrainColor) -> Result<usize, EditError> {
    let index = usize::from(color.0)
        .checked_sub(1)
        .filter(|i| *i < set.colors.len())
        .ok_or(EditError::NoSuchTerrain(color.0))?;
    Ok(index)
}

/// The terrain added `count`-th to a set: Tiled's colour for that place, named by its place,
/// weight 1.
fn new_terrain(count: usize) -> WangColor {
    WangColor {
        name: format!("Terrain {}", count + 1),
        class: String::new(),
        color: NEW_TERRAIN_COLORS
            .get(count % NEW_TERRAIN_COLORS.len())
            .copied()
            .unwrap_or(Color::rgb(255, 0, 0)),
        tile: None,
        probability: 1.0,
        properties: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_a_terrain_renumbers_the_ones_between() {
        let c = TerrainColor;
        // 1 moves to 3: 2 and 3 move down.
        assert_eq!(
            [1, 2, 3, 4].map(|x| moved_color(c(x), c(1), c(3)).0),
            [3, 1, 2, 4]
        );
        // 3 moves to 1: 1 and 2 move up.
        assert_eq!(
            [1, 2, 3, 4].map(|x| moved_color(c(x), c(3), c(1)).0),
            [2, 3, 1, 4]
        );
    }
}
