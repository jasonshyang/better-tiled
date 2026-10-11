//! Tiles as they are (`10-agents.md`, §7, §8.2): `tile` puts tiles in cells, picked at random
//! when several (a tile written twice comes up twice as often), nothing fitted around them;
//! `clear` empties cells; `copy` copies cells, and the objects standing in them, from a map.

use bt_core::{Cell, LayerId};
use glam::IVec2;

use super::error::RunFailure;
use super::runner::Runner;
use crate::script::{Area, CopySource, LayerRef, Layers, Named};

impl Runner<'_> {
    /// `tile TILE,… SHAPE [layer L]`.
    pub(super) fn put_tiles(
        &mut self,
        tiles: &[Named],
        area: &Area,
        layer: Option<&LayerRef>,
    ) -> Result<(), RunFailure> {
        let cells = self.cells_of(area)?;
        self.put_tiles_in(tiles, &cells, layer)
    }

    /// Puts the tiles `tiles` names in `cells`, picked at random.
    pub(super) fn put_tiles_in(
        &mut self,
        tiles: &[Named],
        cells: &[IVec2],
        layer: Option<&LayerRef>,
    ) -> Result<(), RunFailure> {
        let choices = self.names.tile_choices(&self.map, tiles)?;
        let layer = self.tiles_layer(&choices, layer)?;
        self.unlocked(layer)?;
        if cells.is_empty() {
            self.note(format!(
                "tile {}: the shape has no cells (outside the map, or nothing matched)",
                choices.name
            ));
            return Ok(());
        }
        let mut rng = self.rng();
        let count = u64::try_from(choices.tiles.len()).unwrap_or(u64::MAX);
        let Some(target) = self
            .map
            .layer_mut(layer)
            .and_then(|l| l.as_tile_layer_mut())
        else {
            return Ok(());
        };
        for &cell in cells {
            let pick = rng
                .below(count)
                .and_then(|index| usize::try_from(index).ok())
                .and_then(|index| choices.tiles.get(index));
            if let Some(&tile) = pick
                && target.contains(cell)
            {
                target.set_cell(cell, tile)?;
            }
        }
        Ok(())
    }

    /// `clear SHAPE layer L,…|all`.
    pub(super) fn clear_cells(&mut self, area: &Area, layers: &Layers) -> Result<(), RunFailure> {
        let cells = self.cells_of(area)?;
        let ids = self.tile_layers(layers)?;
        self.clear_in(&cells, &ids)
    }

    /// Empties `cells` on the tile layers `ids`.
    pub(super) fn clear_in(&mut self, cells: &[IVec2], ids: &[LayerId]) -> Result<(), RunFailure> {
        for &id in ids {
            self.unlocked(id)?;
        }
        for &id in ids {
            let Some(tiles) = self.map.layer_mut(id).and_then(|l| l.as_tile_layer_mut()) else {
                continue;
            };
            for &cell in cells {
                if tiles.contains(cell) {
                    tiles.set_cell(cell, Cell::EMPTY)?;
                }
            }
            tiles.prune_empty_chunks();
        }
        Ok(())
    }

    /// The tile layers `layers` names, or all of them.
    pub(super) fn tile_layers(&self, layers: &Layers) -> Result<Vec<LayerId>, RunFailure> {
        match layers {
            Layers::All => {
                let mut ids = Vec::new();
                self.map.for_each_layer(|layer, _| {
                    if layer.as_tile_layer().is_some() {
                        ids.push(layer.id);
                    }
                });
                Ok(ids)
            }
            Layers::Only(refs) => refs
                .iter()
                .map(|layer| Ok(self.names.tile_layer(&self.map, layer)?))
                .collect(),
        }
    }

    /// `copy MAP REGION [layers L,…] to X,Y [objects]`.
    pub(super) fn copy_from(&mut self, source: &CopySource, to: IVec2) -> Result<(), RunFailure> {
        let piece = self.take_piece(source, &[], None)?;
        let top_left = to;
        self.put_piece(&piece, top_left, "copy")
    }
}
