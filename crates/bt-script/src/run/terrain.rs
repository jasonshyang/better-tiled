//! `paint` and `erase` (`10-agents.md`, §8.3): every cell of the shape is a full-tile target of
//! the terrain brush (`07-terrain.md`, §5), with the operation's seed, so the cells become the
//! terrain and the tiles around fit themselves. What came back is said: the cells no tile fits
//! (left as they were) with what the set lacks, the terrains the paint pushed back with the
//! tile that would have let them stay, and the cells of other terrain sets it replaced (a set
//! takes other sets' tiles for empty cells). A material paints its parts in order.
//!
//! `erase` takes the terrain off the cells of the shape it is on, which become empty, the tiles
//! around repaired; cells of the shape without it are left alone.

use bt_core::{LayerId, Map};
use bt_terrain::{
    BrushTarget, PaintRequest, Terrain, TerrainColor, missing_text, preview_paint, pushed_back,
    pushed_back_text, replaced_sets, replaced_text, write_preview,
};
use glam::IVec2;

use super::error::RunFailure;
use super::runner::Runner;
use crate::names::{NamedTerrain, Names};
use crate::script::{Area, LayerRef};

impl Runner<'_> {
    /// `paint TERRAIN SHAPE [layer L]`, or with `erase` `erase TERRAIN SHAPE [layer L]`.
    pub(super) fn paint_terrain(
        &mut self,
        terrain: &str,
        area: &Area,
        layer: Option<&LayerRef>,
        erase: bool,
    ) -> Result<(), RunFailure> {
        let parts = self.names.material(terrain)?;
        let cells = self.cells_of(area)?;
        for part in parts {
            let layer = part.layer.as_ref().or(layer);
            self.paint_cells(&part.terrain, &cells, layer, erase)?;
        }
        Ok(())
    }

    /// Paints (or erases) one terrain on `cells` as full-tile targets.
    pub(super) fn paint_cells(
        &mut self,
        terrain: &NamedTerrain,
        cells: &[IVec2],
        layer: Option<&LayerRef>,
        erase: bool,
    ) -> Result<(), RunFailure> {
        let verb = if erase { "erase" } else { "paint" };
        let layer = self.terrain_layer(terrain, layer)?;
        self.unlocked(layer)?;
        let set = Terrain::new(&self.map, terrain.slot, terrain.set)?;
        let color = if erase {
            TerrainColor::NONE
        } else {
            terrain.color
        };
        let targets: Vec<IVec2> = if erase {
            cells
                .iter()
                .copied()
                .filter(|&cell| touches(&self.map, &set, layer, terrain.color, cell))
                .collect()
        } else {
            cells.to_vec()
        };
        if targets.is_empty() {
            let why = if erase && !cells.is_empty() {
                format!(
                    "{} isn't on those cells of {}",
                    terrain.name,
                    layer_name(&self.map, layer)
                )
            } else {
                "the shape has no cells (outside the map, or nothing matched)".to_owned()
            };
            self.note(format!("{verb} {}: {why}", terrain.name));
            return Ok(());
        }
        let request = PaintRequest::new(layer, &set, color)
            .targets(targets.into_iter().map(BrushTarget::Tile).collect())
            .seed(self.op_seed());
        let before = self
            .map
            .layer(layer)
            .and_then(|l| l.as_tile_layer())
            .cloned();
        let preview = preview_paint(&self.map, &request)?;
        let pushed = before
            .as_ref()
            .map(|before| pushed_back(&set, before, &preview, color))
            .unwrap_or_default();
        let replaced = before
            .as_ref()
            .map(|before| replaced_sets(&self.map, &set, before, &preview))
            .unwrap_or_default();
        let report = write_preview(&mut self.map, layer, preview)?;
        let (missing, pushed_text) = {
            let name = terrain_namer(&self.names, terrain);
            let missing = missing_text(&set, &name, &report.needed);
            let pushed_text = pushed_back_text(&set, &name, color, &pushed).map(|text| {
                if erase {
                    text.replacen(
                        &format!("painting {}", name(TerrainColor::NONE)),
                        &format!("erasing {}", terrain.name),
                        1,
                    )
                } else {
                    text
                }
            });
            (missing, pushed_text)
        };
        let replaced = replaced_text(&replaced);
        if let Some(text) = missing {
            self.note(format!("{verb} {}: {text}", terrain.name));
        }
        if let Some(text) = pushed_text {
            self.note(text);
        }
        if let Some(text) = replaced {
            self.note(format!("{verb} {}: {text}", terrain.name));
        }
        for cell in report.invalid {
            if !self.unfilled.contains(&cell) {
                self.unfilled.push(cell);
            }
        }
        Ok(())
    }
}

/// How the feedback sentences name the terrains of `terrain`'s set: bt's names, `nothing` for
/// no terrain.
fn terrain_namer<'n>(
    names: &'n Names,
    terrain: &NamedTerrain,
) -> impl Fn(TerrainColor) -> String + 'n {
    let (slot, set) = (terrain.slot, terrain.set);
    move |color: TerrainColor| {
        if color.is_none() {
            return "nothing".to_owned();
        }
        names
            .terrain_of(slot, set, color)
            .map_or_else(|| format!("terrain {}", color.0), |t| t.name.clone())
    }
}

/// Whether the terrain `color` of `set` is on any lane of the cell's tile on `layer`.
fn touches(map: &Map, set: &Terrain, layer: LayerId, color: TerrainColor, cell: IVec2) -> bool {
    map.layer(layer)
        .and_then(|l| l.as_tile_layer())
        .map(|tiles| tiles.cell(cell))
        .filter(|tile| !tile.is_empty() && tile.slot() == Some(set.slot()))
        .is_some_and(|tile| set.wang_of_cell(tile).lanes().contains(&color.0))
}

/// A layer's name.
fn layer_name(map: &Map, layer: LayerId) -> String {
    map.layer(layer)
        .map_or_else(String::new, |l| l.name.clone())
}
