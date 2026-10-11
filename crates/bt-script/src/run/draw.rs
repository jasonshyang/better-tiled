//! `draw X,Y` … `end` (`10-agents.md`, §8.8): characters on a grid, each standing for what its
//! legend says, painted in order: terrains and materials first, then the cells to empty, then
//! tiles as drawn (nothing fitted around them), then sprites at the bottom middle of their cells
//! and objects at the centre of theirs. `.` and spaces leave cells as they are; rows reaching
//! outside a finite map leave those cells out, with a note.

use bt_core::Map;
use glam::{DVec2, IVec2};

use super::error::RunFailure;
use super::pieces::pick;
use super::runner::Runner;
use super::sprites::Placeables;
use crate::error::{Error, NameKind};
use crate::script::{Drawing, Geometry, Legend, Named, ObjectOptions};
use crate::shapes::map_cells;

impl Runner<'_> {
    /// `draw X,Y` with its legend and rows.
    pub(super) fn draw(&mut self, drawing: &Drawing) -> Result<(), RunFailure> {
        let placed = DrawnCells::from_drawing(&self.map, drawing);
        if placed.outside {
            self.note(format!(
                "draw: the rows (from line {}) reach outside the map; those cells are left out",
                drawing.rows_line
            ));
        }
        let mut drawn_as_tiles: Vec<char> = Vec::new();
        for entry in &drawing.legend {
            let Legend::Paint(parts) = &entry.meaning else {
                continue;
            };
            let cells = placed.cells_with(entry.symbol);
            if let [part] = parts.as_slice()
                && self.is_tile_name(&part.terrain)
            {
                drawn_as_tiles.push(entry.symbol);
                continue;
            }
            for part in parts {
                for found in self.names.material(&part.terrain)? {
                    let layer = part.layer.as_ref().or(found.layer.as_ref()).cloned();
                    self.paint_cells(&found.terrain, &cells, layer.as_ref(), false)?;
                }
            }
        }
        for entry in &drawing.legend {
            if let Legend::Clear(layers) = &entry.meaning {
                let ids = self.tile_layers(layers)?;
                self.clear_in(&placed.cells_with(entry.symbol), &ids)?;
            }
        }
        for entry in &drawing.legend {
            let cells = placed.cells_with(entry.symbol);
            match &entry.meaning {
                Legend::Tiles { tiles, layer } => {
                    self.put_tiles_in(tiles, &cells, layer.as_ref())?
                }
                Legend::Paint(parts) if drawn_as_tiles.contains(&entry.symbol) => {
                    for part in parts {
                        let tiles = [Named::Name(part.terrain.clone())];
                        self.put_tiles_in(&tiles, &cells, part.layer.as_ref())?;
                    }
                }
                _ => {}
            }
        }
        let mut rng = self.rng();
        for entry in &drawing.legend {
            let cells = placed.cells_with(entry.symbol);
            match &entry.meaning {
                Legend::Sprites { sprites, layer } => {
                    let options = ObjectOptions {
                        layer: layer.clone(),
                        ..ObjectOptions::default()
                    };
                    match self.placeables(sprites)? {
                        Placeables::Groups(groups) => {
                            for &cell in &cells {
                                let Some(parts) = pick(&groups, &mut rng) else {
                                    continue;
                                };
                                let foot = cell.as_dvec2() + DVec2::new(0.5, 1.0);
                                self.place_group(&parts, foot, false, &options)?;
                            }
                        }
                        Placeables::Pieces(pieces) => {
                            if layer.is_some() {
                                return Err(RunFailure::refused(
                                    "a piece's tiles and objects go on the layers of their names",
                                ));
                            }
                            self.place_pieces(&pieces, &cells)?;
                        }
                    }
                }
                Legend::Object {
                    kind,
                    layer,
                    assignments,
                } => {
                    let options = ObjectOptions {
                        layer: layer.clone(),
                        assignments: assignments.clone(),
                        ..ObjectOptions::default()
                    };
                    let layer = self.typed_layer(kind, layer.as_ref())?;
                    self.unlocked(layer)?;
                    for &cell in &cells {
                        let point = cell.as_dvec2() + 0.5;
                        self.make_object(layer, kind, &Geometry::Point(point), &options)?;
                    }
                }
                Legend::Paint(_) | Legend::Tiles { .. } | Legend::Clear(_) => {}
            }
        }
        Ok(())
    }

    /// Whether `name` is a name for tiles rather than a terrain or material.
    fn is_tile_name(&self, name: &str) -> bool {
        matches!(
            self.names.material(name),
            Err(Error::WrongKind {
                found: NameKind::Tiles,
                ..
            })
        )
    }
}

/// The cells of a drawing by character, inside the map.
struct DrawnCells {
    cells: Vec<DrawnCell>,
    /// Whether some characters stood outside a finite map.
    outside: bool,
}

/// A character of a drawing at its cell.
struct DrawnCell {
    symbol: char,
    cell: IVec2,
}

impl DrawnCells {
    fn from_drawing(map: &Map, drawing: &Drawing) -> DrawnCells {
        let bounds = map_cells(map);
        let mut cells = Vec::new();
        let mut outside = false;
        for (y, row) in (0i32..).zip(&drawing.rows) {
            for (x, symbol) in (0i32..).zip(row.chars()) {
                if symbol == '.' || symbol == ' ' {
                    continue;
                }
                let cell = drawing.origin + IVec2::new(x, y);
                let on_map = cell.cmpge(bounds.min()).all() && cell.cmplt(bounds.max()).all();
                if !(map.infinite || on_map) {
                    outside = true;
                    continue;
                }
                cells.push(DrawnCell { symbol, cell });
            }
        }
        DrawnCells { cells, outside }
    }

    /// The cells drawn with `symbol`, row by row.
    fn cells_with(&self, symbol: char) -> Vec<IVec2> {
        self.cells
            .iter()
            .filter(|drawn| drawn.symbol == symbol)
            .map(|drawn| drawn.cell)
            .collect()
    }
}
