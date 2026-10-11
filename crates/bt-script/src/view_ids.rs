//! `bt view --ids` (`10-agents.md`, §10.2): each cell's tile id, layer by layer from the top,
//! for exact work with tiles that have no name. A layer of one tileset shows its ids alone and
//! names the tileset once; a layer of several writes `TILESET:ID`.

use bt_core::{Cell, TilesetSlot};
use bt_utils::IRect;
use glam::IVec2;

use crate::error::Error;
use crate::layer_tree::{chosen_layers, is_shown, layers_top_first};
use crate::objects::flips_text;
use crate::shapes::map_cells;
use crate::view::{ViewOutcome, ViewRequest};
use crate::words::region_text;

/// The most cells across `--ids` shows, each with its number.
const MOST_ACROSS: i32 = 60;

/// The most cells down.
const MOST_DOWN: i32 = 100;

impl ViewOutcome {
    /// Each cell's tile id in the region `request` asks for, on its tile layers from the top
    /// (what it says of characters, runs and squares does not apply).
    pub fn ids(request: &ViewRequest<'_>) -> Result<ViewOutcome, Error> {
        let map = request.map();
        let names = request.names();
        let region = request.region_asked().unwrap_or_else(|| map_cells(map));
        if region.is_empty() {
            let title = request.title_asked().unwrap_or("the view");
            return Ok(ViewOutcome {
                text: format!("{title}: no cells\n"),
            });
        }
        if region.w > MOST_ACROSS || region.h > MOST_DOWN {
            return Err(Error::ViewTooLarge {
                what: "--ids",
                most: IVec2::new(MOST_ACROSS, MOST_DOWN),
                size: region.size(),
            });
        }
        let mut text = match request.title_asked() {
            Some(title) => format!("{title}: cells {}", region_text(region)),
            None => format!("cells {}", region_text(region)),
        };
        text.push_str(
            ", each cell's tile id, layer by layer from the top (. empty; h, v, d: flipped \
             horizontally, vertically, diagonally)",
        );
        if request.layers_asked().is_empty() {
            let hidden: Vec<String> = layers_top_first(map)
                .iter()
                .filter(|at| at.layer.as_tile_layer().is_some() && !is_shown(map, at.layer.id))
                .map(|at| at.layer.name.clone())
                .collect();
            if !hidden.is_empty() {
                text.push_str(&format!(
                    "; hidden, left out: {} (--layers shows them)",
                    hidden.join(", ")
                ));
            }
        }
        text.push('\n');
        let mut shown = 0;
        for layer in chosen_layers(map, request.layers_asked()) {
            let Some(tiles) = layer.as_tile_layer() else {
                continue;
            };
            let cells: Vec<Cell> = region.points().map(|p| tiles.cell(p)).collect();
            if cells.iter().all(|c| c.is_empty()) {
                continue;
            }
            shown += 1;
            let mut slots: Vec<TilesetSlot> = cells.iter().filter_map(|c| c.slot()).collect();
            slots.sort_unstable();
            slots.dedup();
            let one = match slots.as_slice() {
                [slot] => Some(*slot),
                _ => None,
            };
            let texts: Vec<String> = cells
                .iter()
                .map(|cell| match cell.slot() {
                    None => ".".to_owned(),
                    Some(_) if one.is_some() => {
                        format!("{}{}", cell.tile(), flips_text(cell.flip()))
                    }
                    Some(slot) => format!(
                        "{}:{}{}",
                        names.tileset_name(slot).unwrap_or("tileset"),
                        cell.tile(),
                        flips_text(cell.flip())
                    ),
                })
                .collect();
            write_layer(
                &mut text,
                &layer.name,
                one.and_then(|s| names.tileset_name(s)),
                region,
                &texts,
            );
        }
        if shown == 0 {
            text.push_str("no tiles there\n");
        }
        Ok(ViewOutcome { text })
    }
}

/// A layer's ids under its name, with a column ruler and row numbers.
fn write_layer(
    text: &mut String,
    layer: &str,
    tileset: Option<&str>,
    region: IRect,
    texts: &[String],
) {
    let which = tileset.map_or_else(String::new, |t| format!(" (tiles of {t})"));
    text.push_str(&format!("{layer}{which}:\n"));
    let widest_x = [region.x, region.x + region.w - 1]
        .iter()
        .map(|x| x.to_string().len())
        .max()
        .unwrap_or(1);
    let width = texts
        .iter()
        .map(String::len)
        .max()
        .unwrap_or(1)
        .max(widest_x)
        + 1;
    let label = [region.y, region.y + region.h - 1]
        .iter()
        .map(|y| y.to_string().len())
        .max()
        .unwrap_or(1)
        .max(2);
    let ruler: String = (0..region.w)
        .map(|x| format!("{:>width$}", region.x + x))
        .collect();
    text.push_str(&format!("{:label$}{ruler}\n", ""));
    let columns = usize::try_from(region.w.max(1)).unwrap_or(1);
    for (row, chunk) in (0..).zip(texts.chunks(columns)) {
        let line: String = chunk.iter().map(|t| format!("{t:>width$}")).collect();
        text.push_str(&format!("{:>label$}{line}\n", region.y + row));
    }
}

#[cfg(test)]
mod tests {
    use bt_utils::IRect;
    use glam::IVec2;

    use super::*;
    use crate::names::Names;
    use crate::view::tests::pond_map;

    #[test]
    fn ids_are_written_layer_by_layer_with_flips() {
        let map = pond_map();
        let names = Names::of(&map);
        let region = IRect::from_min_size(IVec2::new(0, 0), IVec2::new(3, 2));
        let ids = ViewOutcome::ids(&ViewRequest::new(&map, &names).region(region)).unwrap();
        assert_eq!(
            ids.text,
            "cells 0,0 3x2, each cell's tile id, layer by layer from the top (. empty; h, v, d: \
             flipped horizontally, vertically, diagonally)\n\
             deco (tiles of things):\n    \
             0  1  2\n \
             0  . 2v  .\n \
             1  .  2  .\n\
             ground (tiles of ground):\n   \
             0 1 2\n \
             0 0 0 0\n \
             1 0 0 4\n"
        );
        let large = ViewOutcome::ids(
            &ViewRequest::new(&map, &names)
                .region(IRect::from_min_size(IVec2::ZERO, IVec2::new(61, 2))),
        );
        assert_eq!(
            large.unwrap_err().to_string(),
            "--ids shows at most 60x100 cells, and this is 61x2: narrow it with --region X,Y WxH"
        );
    }
}
