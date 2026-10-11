//! `bt at MAP X,Y` (`10-agents.md`, §10.1): one cell, layer by layer from the top: on each tile
//! layer its tile, the tile's sprite or class and flips, and the terrain on each of its lanes;
//! then the objects standing in the cell and those whose box covers it.

use bt_core::{Cell, Lane, Map, WangSetType};
use bt_terrain::{Terrain, TerrainColor};
use glam::{IVec2, UVec2};
use serde::{Deserialize, Serialize};

use crate::feet::Feet;
use crate::layer_tree::layers_top_first;
use crate::names::Names;
use crate::objects::{ObjectInfo, flips_text};
use crate::report::{Line, Outcome, Report, Section};
use crate::shapes::map_cells;
use crate::words::cell_text;

/// `bt at MAP X,Y`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CellOutcome {
    /// The cell.
    pub cell: IVec2,
    /// The size of the finite map the cell is outside of, when it is.
    pub outside: Option<UVec2>,
    /// The tile layers, the top one first.
    pub layers: Vec<CellLayer>,
    /// The objects standing in the cell.
    pub standing: Vec<ObjectInfo>,
    /// The objects whose box covers the cell, standing elsewhere.
    pub over: Vec<ObjectInfo>,
}

/// A tile layer at the cell.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CellLayer {
    /// The layer's name.
    pub layer: String,
    /// How many groups it is in.
    pub depth: usize,
    /// Whether it is hidden.
    pub hidden: bool,
    /// Its tile there; `None` for an empty cell.
    pub tile: Option<CellTile>,
}

/// A tile at the cell.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CellTile {
    /// The tile, as `TILESET:ID`.
    pub tile: String,
    /// The sprite it is, if it is one.
    pub sprite: Option<String>,
    /// Its class; empty for none.
    pub class: String,
    /// Its flips: `h`, `v`, `d`; empty for none.
    pub flipped: String,
    /// The terrains on its lanes, set by set.
    pub terrains: Vec<LanesOf>,
}

/// A terrain (or `nothing`) and the lanes of a tile it is on, in one terrain set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanesOf {
    /// The terrain's name, or `nothing`.
    pub terrain: String,
    /// The lanes: `top-left`, `top`, …
    pub lanes: Vec<String>,
}

impl CellOutcome {
    /// What is at `cell` of `map`.
    pub fn at(map: &Map, names: &Names, cell: IVec2) -> CellOutcome {
        let outside = (!map.infinite && !map_cells(map).contains(cell))
            .then(|| UVec2::new(map.width, map.height));
        let mut layers = Vec::new();
        let mut standing = Vec::new();
        let mut over = Vec::new();
        let mut feet = Feet::new();
        for at in layers_top_first(map) {
            if let Some(tiles) = at.layer.as_tile_layer() {
                let tile = tiles.cell(cell);
                layers.push(CellLayer {
                    layer: at.layer.name.clone(),
                    depth: at.depth,
                    hidden: !at.layer.visible,
                    tile: (!tile.is_empty()).then(|| cell_tile(map, names, tile)),
                });
            }
            if let Some(objects) = at.layer.as_object_layer() {
                for object in objects.objects.iter() {
                    let info =
                        ObjectInfo::from_object(map, names, &mut feet, &at.layer.name, object);
                    if info.cell == Some(cell) {
                        standing.push(info);
                    } else if info.bounds.overlaps_cell(cell) {
                        over.push(info);
                    }
                }
            }
        }
        CellOutcome {
            cell,
            outside,
            layers,
            standing,
            over,
        }
    }
}

/// A tile with its sprite, class, flips and lanes.
fn cell_tile(map: &Map, names: &Names, tile: Cell) -> CellTile {
    let unflipped = tile
        .slot()
        .map_or(Cell::EMPTY, |slot| Cell::new(slot, tile.tile()));
    let text = match tile.slot().and_then(|slot| names.tileset_name(slot)) {
        Some(tileset) => format!("{tileset}:{}", tile.tile()),
        None => names.tile_text(unflipped),
    };
    CellTile {
        tile: text,
        sprite: names.sprite_of(tile).map(|s| s.name.clone()),
        class: map
            .tile_data(tile)
            .map_or_else(String::new, |data| data.class.clone()),
        flipped: flips_text(tile.flip()),
        terrains: lanes_of(map, names, tile),
    }
}

/// The terrains on the lanes of `tile`, as drawn (its flips turn its lanes), in each terrain set
/// that labels it.
fn lanes_of(map: &Map, names: &Names, tile: Cell) -> Vec<LanesOf> {
    let (Some(slot), Some(tileset)) = (tile.slot(), map.tileset_of(tile)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (index, set) in tileset.wang_sets.iter().enumerate() {
        if !set.tiles.contains_key(&tile.tile()) {
            continue;
        }
        let Ok(terrain) = Terrain::new(map, slot, index) else {
            continue;
        };
        let label = terrain.wang_of_cell(tile);
        let mut by_color: Vec<LanesOf> = Vec::new();
        let mut colors: Vec<u8> = Vec::new();
        for LaneWord { lane, word } in lanes_in_reading_order(set.kind) {
            let color = label.lane(lane);
            let name = if color == 0 {
                "nothing".to_owned()
            } else {
                names
                    .terrain_of(slot, index, TerrainColor(color))
                    .map_or_else(|| format!("terrain {color}"), |t| t.name.clone())
            };
            match colors.iter().position(|&c| c == color) {
                Some(at) => {
                    if let Some(entry) = by_color.get_mut(at) {
                        entry.lanes.push(word.to_owned());
                    }
                }
                None => {
                    colors.push(color);
                    by_color.push(LanesOf {
                        terrain: name,
                        lanes: vec![word.to_owned()],
                    });
                }
            }
        }
        out.extend(by_color);
    }
    out
}

/// A lane and the word for it.
#[derive(Clone, Copy, Debug)]
struct LaneWord {
    lane: Lane,
    word: &'static str,
}

/// The lanes a set of `kind` uses, in reading order, with their words.
fn lanes_in_reading_order(kind: WangSetType) -> Vec<LaneWord> {
    let lane = |lane, word| LaneWord { lane, word };
    [
        lane(Lane::TopLeft, "top-left"),
        lane(Lane::Top, "top"),
        lane(Lane::TopRight, "top-right"),
        lane(Lane::Left, "left"),
        lane(Lane::Right, "right"),
        lane(Lane::BottomLeft, "bottom-left"),
        lane(Lane::Bottom, "bottom"),
        lane(Lane::BottomRight, "bottom-right"),
    ]
    .into_iter()
    .filter(|named| kind.uses(named.lane))
    .collect()
}

impl Outcome for CellOutcome {
    const SCHEMA: &'static str = "bt.at/1";

    fn report(&self) -> Report {
        let mut report = Report::new();
        let mut section = Section::headed(format!(
            "cell {}, layer by layer from the top:",
            cell_text(self.cell)
        ));
        if let Some(size) = self.outside {
            section.add_line(Line::note(format!(
                "outside the map, which is {}x{} cells",
                size.x, size.y
            )));
        }
        if self.layers.is_empty() {
            section.add_line(Line::fact("the map has no tile layers"));
        }
        for layer in &self.layers {
            let indent = "  ".repeat(layer.depth);
            let hidden = if layer.hidden { " (hidden)" } else { "" };
            let text = match &layer.tile {
                Some(tile) => tile_text(tile),
                None => "empty".to_owned(),
            };
            section.add_line(Line::fact(format!(
                "{indent}{}{hidden}: {text}",
                layer.layer
            )));
        }
        report.add_section(section);
        let mut standing = Section::headed("objects standing in it:");
        standing.add_lines(self.standing.iter().map(|o| Line::fact(o.line_text())));
        if !self.standing.is_empty() {
            report.add_section(standing);
        }
        let mut over = Section::headed("objects over it, standing elsewhere:");
        over.add_lines(self.over.iter().map(|o| Line::fact(o.line_text())));
        if !self.over.is_empty() {
            report.add_section(over);
        }
        report
    }
}

/// `ground:5 (sprite rock) flipped h, class stone; water at top-left, top-right; grass at …`.
fn tile_text(tile: &CellTile) -> String {
    let mut text = tile.tile.clone();
    if let Some(sprite) = &tile.sprite {
        text.push_str(&format!(" (sprite {sprite})"));
    }
    if !tile.flipped.is_empty() {
        text.push_str(&format!(" flipped {}", tile.flipped));
    }
    if !tile.class.is_empty() {
        text.push_str(&format!(", class {}", tile.class));
    }
    let lanes: Vec<String> = match tile.terrains.as_slice() {
        [whole] => vec![format!("{} (the whole cell)", whole.terrain)],
        parts => parts
            .iter()
            .map(|part| format!("{} at {}", part.terrain, part.lanes.join(", ")))
            .collect(),
    };
    if !lanes.is_empty() {
        text.push_str(&format!("; {}", lanes.join("; ")));
    }
    text
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{Tileset, WangSetType};
    use bt_test_data::fixtures::{Atlas, MapBuilder, TerrainSetBuilder, combination_tile};

    use super::*;
    use crate::report::{from_json, to_json};

    #[test]
    fn a_cell_reads_layer_by_layer_with_its_lanes_and_objects() {
        let set = TerrainSetBuilder::new(WangSetType::Corner)
            .name("Land")
            .terrains(&["Grass", "Water"])
            .every_combination();
        let atlas = Atlas::builder().name("ground").build_embedded().unwrap();
        let tileset = Arc::new(set.added_to(Tileset::clone(&atlas.tileset)));
        // Water at the top-right corner only: lanes top-right, bottom-right, bottom-left,
        // top-left are 2, 1, 1, 1.
        let edge = combination_tile(2, [2, 1, 1, 1]).unwrap().0;
        let grass = combination_tile(2, [1, 1, 1, 1]).unwrap().0;
        let spawn = bt_core::MapObject {
            class: "spawn".into(),
            shape: bt_core::Shape::Point,
            pos: glam::DVec2::new(24.0, 8.0),
            ..bt_core::MapObject::default()
        };
        let zone = bt_core::MapObject {
            class: "zone".into(),
            pos: glam::DVec2::new(16.0, 0.0),
            size: glam::DVec2::new(32.0, 32.0),
            ..bt_core::MapObject::default()
        };
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(3, 2))
            .tileset(tileset)
            .tile_layer(
                "ground",
                &format!("{grass} {edge} {grass}\n{grass} {grass} {grass}"),
            )
            .tile_layer("deco", "")
            .object_layer("design", vec![spawn, zone])
            .build()
            .unwrap();
        let names = Names::of(&map);
        let outcome = CellOutcome::at(&map, &names, IVec2::new(1, 0));
        assert_eq!(outcome.layers.len(), 2);
        assert_eq!(outcome.standing.len(), 1);
        assert_eq!(
            outcome.over.len(),
            1,
            "the zone stands in cell 2,0 and covers 1,0"
        );
        assert_eq!(
            outcome.report().text(),
            format!(
                "cell 1,0, layer by layer from the top:\n  \
                 deco: empty\n  \
                 ground: ground:{edge}; grass at top-left, bottom-left, bottom-right; water at top-right\n\
                 objects standing in it:\n  \
                 #1 point at 1.5,0.5 on design: type=spawn\n\
                 objects over it, standing elsewhere:\n  \
                 #2 rect 1,0 2x2 on design: type=zone\n"
            )
        );
        assert_eq!(
            from_json::<CellOutcome>(&to_json(&outcome).unwrap()).unwrap(),
            outcome
        );
        let outside = CellOutcome::at(&map, &names, IVec2::new(5, 5));
        assert_eq!(outside.outside, Some(UVec2::new(3, 2)));
        assert!(
            outside
                .report()
                .text()
                .contains("note: outside the map, which is 3x2 cells")
        );
    }
}
