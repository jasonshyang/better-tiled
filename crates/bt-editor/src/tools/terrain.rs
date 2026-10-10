//! Painting with terrain: the Terrain tool's strokes, fills with a terrain, the previews they
//! show, and taking the terrain under the pointer, over `bt-terrain`. The choice names a terrain
//! set of a tileset by where the tileset lives, so it outlasts edits to the tileset and follows
//! every map that uses it.

use std::collections::BTreeMap;

use bt_core::{LayerId, Map, TileLayer, TilesetSlot};
use bt_edit::TileRegion;
use bt_terrain::{
    BrushTarget, FillRequest, Needed, PaintRequest, Preview, PushedBack, ReplacedSet, Terrain,
    TerrainColor, TerrainError, fill_note, missing_text, preview_fill, preview_paint, pushed_back,
    pushed_back_text, replaced_sets, replaced_text, write_preview,
};
use glam::IVec2;
use thiserror::Error;

use super::kind::ToolKind;
use crate::document::DocumentId;
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;
use crate::notice::Notice;
use crate::store::TilesetHome;

/// The terrain the Terrain tool paints and terrain fills prefer: a terrain set of a tileset,
/// named by where the tileset lives, and one of its terrains ([`TerrainColor::NONE`] erases).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TerrainChoice {
    /// Where the tileset lives.
    pub home: TilesetHome,
    /// The set's index in the tileset.
    pub set: usize,
    /// The terrain.
    pub color: TerrainColor,
}

/// The chosen terrain made ready for a document's map: the prepared set in the map's slot for
/// its tileset, the terrain, the layer it paints, and the terrains' names.
#[derive(Clone, Debug)]
pub struct TerrainInUse {
    /// The prepared set.
    pub terrain: Terrain,
    /// The terrain painted.
    pub color: TerrainColor,
    /// The current layer, a tile layer that is not locked.
    pub layer: LayerId,
    /// The terrains' names, by colour (index 0: no terrain).
    pub names: Vec<String>,
}

impl TerrainInUse {
    /// The name of a terrain of the set.
    pub fn name(&self, color: TerrainColor) -> String {
        self.names
            .get(usize::from(color.0))
            .cloned()
            .unwrap_or_else(|| format!("terrain {}", color.0))
    }
}

/// Why the chosen terrain cannot paint in a document now.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum NoTerrain {
    /// No terrain was chosen.
    #[error("Choose a terrain in the Terrains panel first")]
    NotChosen,
    /// The document is not a map that is open.
    #[error("There is no map to paint terrain on")]
    NoMap,
    /// The chosen set's tileset is not one of the map's.
    #[error("The terrain's tileset is not one of this map's")]
    NotInMap,
    /// No layer is current.
    #[error("Choose a tile layer to paint terrain on")]
    NoLayer,
    /// The current layer is not a tile layer.
    #[error("Terrain paints tile layers: “{0}” is not one")]
    NotTileLayer(String),
    /// The current layer is locked.
    #[error("“{0}” is locked: unlock it to paint terrain on it")]
    Locked(String),
    /// The set or the terrain is gone from the tileset.
    #[error("{0}")]
    Set(TerrainError),
}

/// A step of a Terrain tool stroke.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerrainStroke {
    /// A stroke starts in a document.
    Begin {
        /// The document.
        doc: DocumentId,
    },
    /// The brush paints these targets.
    Paint {
        /// What the brush touched since the last step.
        targets: Vec<BrushTarget>,
        /// Paint each target's half turn around the map's centre too (finite maps).
        mirror: bool,
    },
    /// The stroke is done: one step of the history.
    End,
    /// The stroke is dropped: the map goes back to where it started.
    Cancel,
}

/// A Terrain tool stroke under way.
#[derive(Clone, Debug)]
pub(crate) struct TerrainStrokeState {
    doc: DocumentId,
    in_use: TerrainInUse,
    /// Every dab of the stroke draws from this seed, so each is what its preview showed.
    seed: u64,
    /// What each cell left unfilled needed, the latest need of a cell winning.
    needed: BTreeMap<(i32, i32), Needed>,
    pushed: BTreeMap<u8, Vec<IVec2>>,
    pushed_missing: BTreeMap<u8, bt_terrain::Pattern>,
    replaced: BTreeMap<String, Vec<IVec2>>,
    last: Option<BrushTarget>,
}

impl Editor {
    /// The chosen terrain made ready for `doc`'s map, or why it cannot paint there now.
    pub fn terrain_in_use(&self, doc: DocumentId) -> Result<TerrainInUse, NoTerrain> {
        let choice = self.tools.terrain.ok_or(NoTerrain::NotChosen)?;
        let document = self.documents.get(doc).ok_or(NoTerrain::NoMap)?;
        let map = self.map_of(doc).ok_or(NoTerrain::NoMap)?;
        let slot = (0..map.tilesets.len())
            .filter_map(TilesetSlot::new)
            .find(|slot| self.store.home_of_slot(document.asset(), *slot) == Some(choice.home))
            .ok_or(NoTerrain::NotInMap)?;
        let terrain = Terrain::new(map, slot, choice.set).map_err(NoTerrain::Set)?;
        if !terrain.has_color(choice.color) {
            return Err(NoTerrain::Set(TerrainError::NoSuchColor {
                color: choice.color.0,
                count: terrain.color_count(),
            }));
        }
        let layer = document
            .selection()
            .current_layer
            .ok_or(NoTerrain::NoLayer)?;
        let current = map.layer(layer).ok_or(NoTerrain::NoLayer)?;
        if current.as_tile_layer().is_none() {
            return Err(NoTerrain::NotTileLayer(current.name.clone()));
        }
        if map.effective_layer(layer).is_some_and(|l| l.locked) {
            return Err(NoTerrain::Locked(current.name.clone()));
        }
        let mut names = vec!["no terrain".to_owned()];
        if let Some(set) = map
            .tileset(slot)
            .and_then(|tileset| tileset.wang_sets.get(choice.set))
        {
            names.extend(set.colors.iter().map(|c| c.name.clone()));
        }
        Ok(TerrainInUse {
            terrain,
            color: choice.color,
            layer,
            names,
        })
    }

    /// What painting `targets` now would place: exactly what the stroke's dab places, with the
    /// cells no tile fits. `None` when the terrain cannot paint here (see
    /// [`Editor::terrain_in_use`]) or symmetry is asked for on an infinite map.
    pub fn terrain_preview(
        &self,
        doc: DocumentId,
        targets: &[BrushTarget],
        mirror: bool,
    ) -> Option<Preview> {
        let in_use = self.terrain_in_use(doc).ok()?;
        let map = self.map_of(doc)?;
        let request = PaintRequest::new(in_use.layer, &in_use.terrain, in_use.color)
            .targets(targets.to_vec())
            .seed(self.tools.terrain_seed)
            .mirror(mirror);
        preview_paint(map, &request).ok()
    }

    /// What a fill of `area` with the chosen terrain would place.
    pub fn terrain_fill_preview(&self, doc: DocumentId, area: &TileRegion) -> Option<Preview> {
        let in_use = self.terrain_in_use(doc).ok()?;
        let map = self.map_of(doc)?;
        preview_fill(map, &self.fill_request(&in_use, area)).ok()
    }

    fn fill_request<'t>(&self, in_use: &'t TerrainInUse, area: &TileRegion) -> FillRequest<'t> {
        FillRequest::new(in_use.layer, &in_use.terrain, in_use.color)
            .cells(area.iter().collect())
            .seed(self.tools.terrain_seed)
    }

    /// Chooses terrain `color` of set `set` of the map's tileset in `slot`, and the Terrain
    /// tool.
    pub(crate) fn choose_terrain(
        &mut self,
        doc: DocumentId,
        slot: TilesetSlot,
        set: usize,
        color: TerrainColor,
    ) -> Vec<Effect> {
        let Some(home) = self
            .documents
            .get(doc)
            .and_then(|d| self.store.home_of_slot(d.asset(), slot))
        else {
            return vec![Effect::Notify(Notice::info(format!(
                "The map has no tileset in slot {slot}"
            )))];
        };
        self.tools.terrain = Some(TerrainChoice { home, set, color });
        self.tools.set_tool(ToolKind::Terrain);
        Vec::new()
    }

    /// The terrain under `target` in `doc`'s current layer becomes the one painted (the Terrain
    /// tool's right-click; Tiled's "capture hover color").
    pub(crate) fn capture_terrain(&mut self, doc: DocumentId, target: BrushTarget) -> Vec<Effect> {
        let in_use = match self.terrain_in_use(doc) {
            Ok(in_use) => in_use,
            Err(reason) => return vec![Effect::Notify(Notice::info(reason.to_string()))],
        };
        let Some(layer) = self.tile_layer(doc, in_use.layer) else {
            return Vec::new();
        };
        let lanes = in_use
            .terrain
            .wang_of_cell(layer.cell(target.point()))
            .lanes();
        let color = match target {
            BrushTarget::Corner(_) => lanes.get(7).copied(),
            BrushTarget::Edge { side, .. } => lanes.get(side.index()).copied(),
            BrushTarget::Tile(_) => lanes.iter().copied().find(|&c| c != 0),
        }
        .unwrap_or(0);
        if let Some(choice) = &mut self.tools.terrain {
            choice.color = TerrainColor(color);
        }
        Vec::new()
    }

    /// Takes a step of a Terrain tool stroke.
    pub(crate) fn terrain_stroke(&mut self, step: TerrainStroke) -> Vec<Effect> {
        match step {
            TerrainStroke::Begin { doc } => {
                let in_use = match self.terrain_in_use(doc) {
                    Ok(in_use) => in_use,
                    Err(reason) => return vec![Effect::Notify(Notice::info(reason.to_string()))],
                };
                if let Err(error) = self.begin_gesture(doc, "Paint Terrain") {
                    return vec![warn(error)];
                }
                self.terrain_stroke = Some(TerrainStrokeState {
                    doc,
                    in_use,
                    seed: self.tools.terrain_seed,
                    needed: BTreeMap::new(),
                    pushed: BTreeMap::new(),
                    pushed_missing: BTreeMap::new(),
                    replaced: BTreeMap::new(),
                    last: None,
                });
                Vec::new()
            }
            TerrainStroke::Paint { targets, mirror } => self.paint_terrain(&targets, mirror),
            TerrainStroke::End => {
                let Some(stroke) = self.terrain_stroke.take() else {
                    return Vec::new();
                };
                if let Err(error) = self.end_gesture() {
                    return vec![warn(error)];
                }
                self.tools.last_terrain_target = stroke.last;
                self.tools.reseed_terrain();
                stroke_notices(&stroke)
            }
            TerrainStroke::Cancel => {
                self.terrain_stroke = None;
                match self.cancel_gesture() {
                    Ok(()) => Vec::new(),
                    Err(error) => vec![warn(error)],
                }
            }
        }
    }

    fn paint_terrain(&mut self, targets: &[BrushTarget], mirror: bool) -> Vec<Effect> {
        let Some(mut stroke) = self.terrain_stroke.take() else {
            return Vec::new();
        };
        let Some(asset) = self.documents.get(stroke.doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let in_use = stroke.in_use.clone();
        let request = PaintRequest::new(in_use.layer, &in_use.terrain, in_use.color)
            .targets(targets.to_vec())
            .seed(stroke.seed)
            .mirror(mirror);
        let result = self.gesture_apply(|tx| {
            let map = tx.map_mut(asset)?;
            let preview = preview_paint(map, &request)?;
            let before = tile_layer_of(map, in_use.layer)?;
            for pushed in pushed_back(&in_use.terrain, &before, &preview, in_use.color) {
                let cells = stroke.pushed.entry(pushed.terrain.0).or_default();
                cells.extend(
                    pushed
                        .cells
                        .iter()
                        .filter(|p| !cells.contains(p))
                        .collect::<Vec<_>>(),
                );
                if let Some(missing) = pushed.missing {
                    stroke
                        .pushed_missing
                        .entry(pushed.terrain.0)
                        .or_insert(missing);
                }
            }
            for replaced in replaced_sets(map, &in_use.terrain, &before, &preview) {
                let cells = stroke.replaced.entry(replaced.set_name).or_default();
                cells.extend(
                    replaced
                        .cells
                        .iter()
                        .filter(|p| !cells.contains(p))
                        .collect::<Vec<_>>(),
                );
            }
            let report = write_preview(map, in_use.layer, preview)?;
            for need in report.needed {
                stroke.needed.insert((need.cell.y, need.cell.x), need);
            }
            Ok::<_, EditError>(())
        });
        stroke.last = targets.last().copied().or(stroke.last);
        self.terrain_stroke = Some(stroke);
        result.err().map(warn).into_iter().collect()
    }

    /// Fills `area` with the chosen terrain as one step, as a terrain fill previews it.
    pub(crate) fn terrain_fill(
        &mut self,
        doc: DocumentId,
        area: &TileRegion,
        label: &str,
    ) -> Vec<Effect> {
        let in_use = match self.terrain_in_use(doc) {
            Ok(in_use) => in_use,
            Err(reason) => return vec![Effect::Notify(Notice::info(reason.to_string()))],
        };
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let request = self.fill_request(&in_use, area);
        let result = self.transact(doc, label, |tx| {
            let map = tx.map_mut(asset)?;
            Ok::<_, EditError>(bt_terrain::fill(map, &request)?)
        });
        self.tools.reseed_terrain();
        let report = match result {
            Ok(report) => report,
            Err(error) => return vec![warn(error)],
        };
        let name = |c: TerrainColor| in_use.name(c);
        let mut notes: Vec<String> = Vec::new();
        notes.extend(missing_text(&in_use.terrain, &name, &report.needed));
        notes.extend(fill_note(&in_use.terrain, &name, in_use.color));
        notes
            .into_iter()
            .map(|text| Effect::Notify(Notice::info(capitalized(&text))))
            .collect()
    }

    fn tile_layer(&self, doc: DocumentId, layer: LayerId) -> Option<&TileLayer> {
        self.map_of(doc)?.layer(layer)?.as_tile_layer()
    }
}

/// A copy of the tile layer `id` of `map` (its chunks are shared, so the copy is cheap).
fn tile_layer_of(map: &Map, id: LayerId) -> Result<TileLayer, EditError> {
    map.layer(id)
        .and_then(|layer| layer.as_tile_layer())
        .cloned()
        .ok_or(EditError::NoSuchLayer(id))
}

/// What to say when a stroke ends: the cells no tile fits, the terrain it pushed back, the
/// other terrain sets it painted over.
fn stroke_notices(stroke: &TerrainStrokeState) -> Vec<Effect> {
    let in_use = &stroke.in_use;
    let name = |c: TerrainColor| in_use.name(c);
    let needed: Vec<Needed> = stroke.needed.values().cloned().collect();
    let mut notes: Vec<String> = Vec::new();
    notes.extend(missing_text(&in_use.terrain, &name, &needed));
    let mut pushed: Vec<PushedBack> = stroke
        .pushed
        .iter()
        .map(|(&gone, cells)| PushedBack {
            terrain: TerrainColor(gone),
            cells: cells.clone(),
            missing: stroke.pushed_missing.get(&gone).copied(),
        })
        .collect();
    pushed.sort_by_key(|p| std::cmp::Reverse(p.cells.len()));
    notes.extend(pushed_back_text(
        &in_use.terrain,
        &name,
        in_use.color,
        &pushed,
    ));
    let replaced: Vec<ReplacedSet> = stroke
        .replaced
        .iter()
        .map(|(set_name, cells)| ReplacedSet {
            set_name: set_name.clone(),
            cells: cells.clone(),
        })
        .collect();
    notes.extend(replaced_text(&replaced));
    notes
        .into_iter()
        .map(|text| Effect::Notify(Notice::info(capitalized(&text))))
        .collect()
}

/// The sentence with its first letter capitalised, as notices start.
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
