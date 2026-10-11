//! Names a script defines (`10-agents.md`, §7): materials (terrains painted together), groups
//! of sprites, names for tiles, and areas (the cells as they are on that line). Pieces are
//! defined with the pieces they are (`super::pieces`). A definition lasts the script; one in a
//! `--with` file lasts across commands.

use super::error::RunFailure;
use super::runner::Runner;
use crate::names::{GroupPart, MaterialPart, Sprite};
use crate::script::{Area, LayerRef, Named, SpritePart, TerrainPart, TileIds};

impl Runner<'_> {
    /// `define NAME = TERRAIN [layer L] + …`: terrains painted together, in order. A part may be
    /// a material defined before; a part's layer is its own, else the material's part's.
    pub(super) fn define_material(
        &mut self,
        name: &str,
        parts: &[TerrainPart],
    ) -> Result<(), RunFailure> {
        let mut resolved: Vec<MaterialPart> = Vec::new();
        for part in parts {
            for mut found in self.names.material(&part.terrain)? {
                if part.layer.is_some() {
                    found.layer = part.layer.clone();
                }
                resolved.push(found);
            }
        }
        Ok(self.names.define_material(name, resolved)?)
    }

    /// `define NAME = sprite S [layer L] [+ sprite S2 at DX,DY [layer L]] …`.
    pub(super) fn define_group(
        &mut self,
        name: &str,
        parts: &[SpritePart],
    ) -> Result<(), RunFailure> {
        let mut resolved = Vec::new();
        for part in parts {
            resolved.push(GroupPart {
                sprite: self.sprite_named(&part.sprite)?,
                offset: part.offset,
                layer: part.layer.clone(),
            });
        }
        Ok(self.names.define_group(name, resolved)?)
    }

    /// `define NAME = tile TILE,… [layer L]`.
    pub(super) fn define_tiles(
        &mut self,
        name: &str,
        tiles: &[Named],
        layer: Option<&LayerRef>,
    ) -> Result<(), RunFailure> {
        let group = self.names.tile_choices(&self.map, tiles)?;
        Ok(self
            .names
            .define_tiles(name, group.tiles, layer.cloned().or(group.layer))?)
    }

    /// `define NAME = area SHAPE`: the cells as they are now.
    pub(super) fn define_area(&mut self, name: &str, area: &Area) -> Result<(), RunFailure> {
        let cells = self.cells_of(area)?;
        if cells.is_empty() {
            self.note(format!("define {name}: the area has no cells"));
        }
        Ok(self.names.define_area(name, cells)?)
    }

    /// The sprite a script names: by name, or as `TILESET:ID`.
    pub(super) fn sprite_named(&self, named: &Named) -> Result<Sprite, RunFailure> {
        let query = match named {
            Named::Name(name) => name.clone(),
            Named::Tiles(ids) => one_tile_text(ids).ok_or_else(|| {
                RunFailure::refused(format!(
                    "{}: name one sprite, as TILESET:ID",
                    tile_ids_text(ids)
                ))
            })?,
        };
        Ok(self.names.sprite(&query)?.clone())
    }
}

/// `TILESET:ID` when `ids` name one tile.
pub(super) fn one_tile_text(ids: &TileIds) -> Option<String> {
    match ids.ranges.as_slice() {
        [range] if range.first == range.last => Some(format!("{}:{}", ids.tileset, range.first)),
        _ => None,
    }
}

/// Tiles as written: `tileset:1-4,9`.
pub(super) fn tile_ids_text(ids: &TileIds) -> String {
    let ranges: Vec<String> = ids
        .ranges
        .iter()
        .map(|r| {
            if r.first == r.last {
                r.first.to_string()
            } else {
                format!("{}-{}", r.first, r.last)
            }
        })
        .collect();
    format!("{}:{}", ids.tileset, ranges.join(","))
}
