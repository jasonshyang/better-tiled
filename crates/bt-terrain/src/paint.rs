//! The four entry points: paint and fill, and their previews, which are exactly what the paint
//! or fill will place with the same request.

use bt_core::{LayerId, Map, TileLayer};
use bt_utils::IRect;

use crate::color::TerrainColor;
use crate::error::TerrainError;
use crate::filler::{Filler, Outcome};
use crate::request::{FillRequest, PaintRequest, Preview, Report};
use crate::terrain::Terrain;
use crate::topology::Topology;
use glam::UVec2;

/// What the brush would place for `request`: the targets painted, neighbours whose tiles no
/// longer fit re-solved out to the set's maximum terrain distance, cells left empty where that
/// fits best.
pub fn preview_paint(map: &Map, request: &PaintRequest<'_>) -> Result<Preview, TerrainError> {
    let layer = checked_layer(map, request.layer, request.terrain, request.color)?;
    if request.mirror && map.infinite {
        return Err(TerrainError::MirrorOnInfiniteMap);
    }
    let topology = Topology::of(map);
    let mut filler = Filler::new(request.terrain, layer, &topology, true, true, request.seed);
    let kind = request.terrain.brush_kind(request.color);
    for &target in &request.targets {
        if let crate::request::BrushTarget::Edge { side, .. } = target
            && crate::wangid::is_corner(side.index())
        {
            return Err(TerrainError::NotAnEdge(side));
        }
        filler.set_target(target, request.color.0, kind);
    }
    if request.mirror {
        filler.mirror_region(UVec2::new(map.width, map.height));
    }
    Ok(preview_of(filler.run()))
}

/// Paints `request` on its layer: what [`preview_paint`] shows.
pub fn paint(map: &mut Map, request: &PaintRequest<'_>) -> Result<Report, TerrainError> {
    let preview = preview_paint(map, request)?;
    write_preview(map, request.layer, preview)
}

/// What a fill of `request` would place: terrain that connects to the tiles around the cells,
/// preferring the chosen terrain when there is one, each choice looking one step ahead.
pub fn preview_fill(map: &Map, request: &FillRequest<'_>) -> Result<Preview, TerrainError> {
    let layer = checked_layer(map, request.layer, request.terrain, request.color)?;
    let topology = Topology::of(map);
    let mut filler = Filler::new(
        request.terrain,
        layer,
        &topology,
        false,
        request.allow_empty,
        request.seed,
    );
    filler.set_region(&request.cells);
    if !request.color.is_none() {
        filler.prefer_color(request.color.0, request.terrain.brush_kind(request.color));
    }
    Ok(preview_of(filler.run()))
}

/// Fills `request` on its layer: what [`preview_fill`] shows.
pub fn fill(map: &mut Map, request: &FillRequest<'_>) -> Result<Report, TerrainError> {
    let preview = preview_fill(map, request)?;
    write_preview(map, request.layer, preview)
}

/// The tile layer a request names, once the terrain and terrain it uses belong to the map.
fn checked_layer<'m>(
    map: &'m Map,
    id: LayerId,
    terrain: &Terrain,
    color: TerrainColor,
) -> Result<&'m TileLayer, TerrainError> {
    if map.tileset(terrain.slot()).is_none() {
        return Err(TerrainError::NoSuchTileset(terrain.slot()));
    }
    if !terrain.has_color(color) {
        return Err(TerrainError::NoSuchColor {
            color: color.0,
            count: terrain.color_count(),
        });
    }
    let layer = map.layer(id).ok_or(TerrainError::NoSuchLayer(id))?;
    layer
        .as_tile_layer()
        .ok_or_else(|| TerrainError::NotTileLayer(layer.name.clone()))
}

fn preview_of(outcome: Outcome) -> Preview {
    Preview {
        cells: outcome.placed,
        invalid: outcome.invalid,
        needed: outcome.needed,
    }
}

/// Writes a preview's cells into the layer; the report's bounds cover the cells that changed.
fn write_preview(map: &mut Map, id: LayerId, preview: Preview) -> Result<Report, TerrainError> {
    let layer = map.layer_mut(id).ok_or(TerrainError::NoSuchLayer(id))?;
    let name = layer.name.clone();
    let cells = layer
        .as_tile_layer_mut()
        .ok_or(TerrainError::NotTileLayer(name))?;
    let mut changed: Option<IRect> = None;
    for placed in &preview.cells {
        // The solver places only cells inside a finite layer, so the write cannot be refused.
        if matches!(cells.set_cell(placed.at, placed.cell), Ok(true)) {
            let cell = IRect::from_min_size(placed.at, glam::IVec2::ONE);
            changed = Some(changed.map_or(cell, |c| c.union(cell)));
        }
    }
    Ok(Report {
        changed,
        invalid: preview.invalid,
        needed: preview.needed,
    })
}
