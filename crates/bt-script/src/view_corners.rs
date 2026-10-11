//! `bt view --corners` (`10-agents.md`, §10.2): the terrain at each corner of the cells. Corner
//! terrains are painted on corners: a cell's tile follows its four corners, and cells next to
//! each other share two, so this shows how wide a gap or a strip really is, and `!` where the
//! tiles around a corner disagree. Corner X,Y is the top-left corner of cell X,Y; it is read on
//! the top-most chosen layer whose tiles around it carry a terrain there.

use std::collections::HashMap;

use bt_core::{Cell, Lane, Map, Orientation, TileLayer, TilesetSlot, WangSetType};
use bt_terrain::{Terrain, TerrainColor};
use glam::IVec2;

use crate::error::Error;
use crate::info::orientation_name;
use crate::layer_tree::chosen_layers;
use crate::shapes::map_cells;
use crate::symbols::{NOTHING, Symbols, TOO_MANY};
use crate::view::{ViewOutcome, ViewRequest};
use crate::words::region_text;

/// The most cells across and down `--corners` shows.
const MOST_SIDE: i32 = 160;

/// The symbol of a corner whose tiles disagree.
const DISAGREE: char = '!';

/// The cells around a corner, from it, and the lane of each that is the corner.
const AROUND: [Around; 4] = [
    Around {
        offset: IVec2::new(0, 0),
        lane: Lane::TopLeft,
    },
    Around {
        offset: IVec2::new(-1, 0),
        lane: Lane::TopRight,
    },
    Around {
        offset: IVec2::new(-1, -1),
        lane: Lane::BottomRight,
    },
    Around {
        offset: IVec2::new(0, -1),
        lane: Lane::BottomLeft,
    },
];

/// A cell around a corner and its lane at the corner.
struct Around {
    offset: IVec2,
    lane: Lane,
}

/// A terrain at a corner: a colour of a set of a tileset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct CornerTerrain {
    slot: TilesetSlot,
    set: usize,
    color: u8,
}

/// A terrain set of a tileset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct SetKey {
    slot: TilesetSlot,
    set: usize,
}

/// What a layer has at a corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AtCorner {
    /// No tile around it is of a set with corners.
    Unknown,
    /// The tiles around it agree.
    Terrain(CornerTerrain),
    /// They disagree.
    Disagree,
}

impl ViewOutcome {
    /// The terrain at each corner of the cells `request` asks for.
    pub fn corners(request: &ViewRequest<'_>) -> Result<ViewOutcome, Error> {
        let map = request.map();
        let names = request.names();
        if matches!(
            map.orientation,
            Orientation::Hexagonal | Orientation::Staggered
        ) {
            return Err(Error::CornersNotSquare {
                orientation: orientation_name(map.orientation),
            });
        }
        let region = request.region_asked().unwrap_or_else(|| map_cells(map));
        if region.is_empty() {
            let title = request.title_asked().unwrap_or("the view");
            return Ok(ViewOutcome {
                text: format!("{title}: no cells\n"),
            });
        }
        if region.w > MOST_SIDE || region.h > MOST_SIDE {
            return Err(Error::ViewTooLarge {
                what: "--corners",
                most: IVec2::splat(MOST_SIDE),
                size: region.size(),
            });
        }
        let layers: Vec<&TileLayer> = chosen_layers(map, request.layers_asked())
            .into_iter()
            .filter_map(|layer| layer.as_tile_layer())
            .collect();
        let symbols = Symbols::of(map, names);
        let mut reader = CornerReader {
            map,
            terrains: HashMap::new(),
        };
        let mut rows: Vec<String> = Vec::new();
        let mut legend: Vec<CornerTerrain> = Vec::new();
        let mut disagreements = 0usize;
        for y in region.y..=region.y + region.h {
            let mut row = String::new();
            for x in region.x..=region.x + region.w {
                let corner = IVec2::new(x, y);
                let found =
                    layers
                        .iter()
                        .map(|tiles| reader.at(tiles, corner))
                        .find(|at| match at {
                            AtCorner::Unknown => false,
                            AtCorner::Terrain(terrain) => terrain.color != 0,
                            AtCorner::Disagree => true,
                        });
                row.push(match found {
                    None | Some(AtCorner::Unknown) => NOTHING,
                    Some(AtCorner::Disagree) => {
                        disagreements += 1;
                        DISAGREE
                    }
                    Some(AtCorner::Terrain(terrain)) => {
                        if !legend.contains(&terrain) {
                            legend.push(terrain);
                        }
                        symbols.of_terrain(terrain.slot, terrain.set, TerrainColor(terrain.color))
                    }
                });
            }
            rows.push(row);
        }
        let title = request
            .title_asked()
            .map_or_else(String::new, |t| format!("{t}: "));
        let mut text = format!(
            "{title}the corners of cells {} (corner X,Y is the top-left corner of cell X,Y; the \
             cell's other corners are to its right and below)\n",
            region_text(region)
        );
        let label = [region.y, region.y + region.h]
            .iter()
            .map(|y| y.to_string().len())
            .max()
            .unwrap_or(1)
            .max(2);
        let pad = " ".repeat(label + 2);
        let columns = region.w + 1;
        if columns > 10 {
            let tens: String = (0..columns)
                .map(|c| {
                    let x = region.x + c;
                    if x.rem_euclid(10) == 0 {
                        char::from_digit((x / 10).unsigned_abs() % 10, 10).unwrap_or(' ')
                    } else {
                        ' '
                    }
                })
                .collect();
            text.push_str(&format!("{pad}{}\n", tens.trim_end()));
        }
        let units: String = (0..columns)
            .map(|c| {
                char::from_digit((region.x + c).rem_euclid(10).unsigned_abs(), 10).unwrap_or(' ')
            })
            .collect();
        text.push_str(&format!("{pad}{units}\n"));
        for (y, row) in (region.y..).zip(&rows) {
            text.push_str(&format!("{y:>label$}  {row}\n"));
        }
        text.push_str("legend:\n");
        text.push_str(&format!("  {NOTHING}  no terrain\n"));
        for terrain in &legend {
            let color = TerrainColor(terrain.color);
            let symbol = symbols.of_terrain(terrain.slot, terrain.set, color);
            let name = names
                .terrain_of(terrain.slot, terrain.set, color)
                .map_or_else(|| format!("terrain {}", terrain.color), |t| t.name.clone());
            if symbol == TOO_MANY {
                text.push_str(&format!("  {TOO_MANY}  {name}, among others\n"));
            } else {
                text.push_str(&format!("  {symbol}  {name}\n"));
            }
        }
        if disagreements > 0 {
            text.push_str(&format!(
                "  {DISAGREE}  the tiles around the corner disagree ({disagreements}: bt check lists seams)\n"
            ));
        }
        Ok(ViewOutcome { text })
    }
}

/// Reads corners off tile layers, with each terrain set read once.
struct CornerReader<'m> {
    map: &'m Map,
    terrains: HashMap<SetKey, Option<Terrain>>,
}

impl CornerReader<'_> {
    /// What a layer's tiles around `corner` say is there.
    fn at(&mut self, tiles: &TileLayer, corner: IVec2) -> AtCorner {
        let mut found: Option<CornerTerrain> = None;
        for around in AROUND {
            let tile = tiles.cell(corner + around.offset);
            let Some(terrain) = self.corner_terrain(tile, around.lane) else {
                continue;
            };
            match found {
                None => found = Some(terrain),
                Some(earlier) if earlier != terrain => return AtCorner::Disagree,
                Some(_) => {}
            }
        }
        found.map_or(AtCorner::Unknown, AtCorner::Terrain)
    }

    /// The terrain on a tile's lane, in the first set with corners that labels it, its flips
    /// applied.
    fn corner_terrain(&mut self, tile: Cell, lane: Lane) -> Option<CornerTerrain> {
        let slot = tile.slot()?;
        let tileset = self.map.tileset(slot)?;
        let (index, _) = tileset.wang_sets.iter().enumerate().find(|(_, set)| {
            set.kind != WangSetType::Edge && set.tiles.contains_key(&tile.tile())
        })?;
        let map = self.map;
        let terrain = self
            .terrains
            .entry(SetKey { slot, set: index })
            .or_insert_with(|| Terrain::new(map, slot, index).ok())
            .as_ref()?;
        Some(CornerTerrain {
            slot,
            set: index,
            color: terrain.wang_of_cell(tile).lane(lane),
        })
    }
}

#[cfg(test)]
mod tests {
    use bt_test_data::fixtures::MapBuilder;
    use bt_utils::IRect;
    use glam::UVec2;

    use super::*;
    use crate::names::Names;
    use crate::view::tests::pond_map;

    #[test]
    fn corners_show_how_wide_a_painted_area_really_is() {
        let map = pond_map();
        let names = Names::of(&map);
        let region = IRect::from_min_size(IVec2::new(2, 1), IVec2::new(6, 4));
        let corners = ViewOutcome::corners(&ViewRequest::new(&map, &names).region(region)).unwrap();
        assert_eq!(
            corners.text,
            "the corners of cells 2,1 6x4 (corner X,Y is the top-left corner of cell X,Y; the \
             cell's other corners are to its right and below)\n    \
             2345678\n \
             1  \"\"\"\"\"\"\"\n \
             2  \"~~~~~\"\n \
             3  \"~~~~~\"\n \
             4  \"~~~~~\"\n \
             5  \"\"\"\"\"\"\"\n\
             legend:\n  \
             .  no terrain\n  \
             \"  grass\n  \
             ~  water\n"
        );
        let hex = MapBuilder::new(UVec2::new(14, 12), UVec2::new(4, 4))
            .orientation(Orientation::Hexagonal)
            .build()
            .unwrap();
        let names = Names::of(&hex);
        assert_eq!(
            ViewOutcome::corners(&ViewRequest::new(&hex, &names))
                .unwrap_err()
                .to_string(),
            "--corners reads the corners squares share, and this map is hexagonal: bt view MAP \
             and bt at MAP X,Y show its terrains"
        );
    }
}
