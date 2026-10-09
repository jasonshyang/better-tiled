//! Terrain sets, and the terrain shapes of files before Tiled 1.5 converted into them.

use bt_core::{Color, Lane, TileId, Tileset, WangColor, WangId, WangSet, WangSetType};

use super::context::{Finisher, text_of};
use crate::names;
use crate::raw::{RawTerrain, RawWangColor, RawWangId, RawWangSet, Scalar, WangColorRole};
use crate::warning::{Target, WarningKind};

/// Tiled's palette for terrains without a valid colour (`libtiled/wangset.cpp`); colours past
/// the fifteenth take it again from the start, where Tiled picks random ones.
const DEFAULT_COLORS: [Color; 15] = [
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

/// The palette colour of the terrain at `index` (0-based).
fn default_color(index: usize) -> Color {
    DEFAULT_COLORS
        .get(index % DEFAULT_COLORS.len())
        .copied()
        .unwrap_or(Color::BLACK)
}

/// A tile's terrain corners in files before Tiled 1.5: top-left, top-right, bottom-left,
/// bottom-right terrain indexes.
pub(crate) struct TileTerrain {
    /// The tile.
    pub(crate) tile: TileId,
    /// The four corners as written; `None` or -1 is no terrain.
    pub(crate) corners: Vec<Option<Scalar>>,
}

/// The lanes the corners of [`TileTerrain`] go to.
const CORNER_LANES: [Lane; 4] = [
    Lane::TopLeft,
    Lane::TopRight,
    Lane::BottomLeft,
    Lane::BottomRight,
];

impl Finisher<'_> {
    /// Adds the tileset's terrain sets: the terrain types of old files as a corner set named
    /// “Terrains” first, then the terrain sets in file order.
    pub(crate) fn wang_sets_into(
        &mut self,
        tileset: &mut Tileset,
        terrains: Vec<RawTerrain>,
        tile_terrains: Vec<TileTerrain>,
        sets: Vec<RawWangSet>,
    ) {
        let name = tileset.name.clone();
        if !terrains.is_empty() {
            self.warnings.record_about(
                WarningKind::LegacyShape,
                Some(Target::Tileset(name.clone())),
                format!(
                    "the terrain types of tileset “{name}” become the corner terrain set “Terrains”"
                ),
            );
            let set = self.terrain_set_of(&name, terrains, tile_terrains);
            tileset.wang_sets.push(set);
        } else if !tile_terrains.is_empty() {
            self.record(
                WarningKind::InvalidWangId,
                format!("tiles of tileset “{name}” have terrain corners, but it has no terrain types; they are dropped"),
            );
        }
        for raw in sets {
            let set = self.wang_set_of(&name, raw);
            tileset.wang_sets.push(set);
        }
    }

    /// The “Terrains” set of a file before Tiled 1.5: terrain `n` is colour `n + 1`.
    fn terrain_set_of(
        &mut self,
        tileset: &str,
        terrains: Vec<RawTerrain>,
        tiles: Vec<TileTerrain>,
    ) -> WangSet {
        let mut colors = Vec::with_capacity(terrains.len());
        for (index, terrain) in terrains.into_iter().enumerate() {
            let tile = self
                .i32_of(
                    &terrain.tile,
                    format_args!("a terrain's tile in tileset “{tileset}”"),
                )
                .and_then(|tile| u32::try_from(tile).ok())
                .map(TileId);
            colors.push(WangColor {
                name: text_of(&terrain.name),
                class: String::new(),
                color: default_color(index),
                tile,
                probability: 1.0,
                properties: self.properties_of(terrain.properties),
            });
        }
        let mut set = WangSet {
            name: "Terrains".to_owned(),
            class: String::new(),
            kind: WangSetType::Corner,
            tile: None,
            colors,
            tiles: Default::default(),
            properties: Default::default(),
        };
        let mut invalid = Vec::new();
        for TileTerrain { tile, corners } in tiles {
            match terrain_wang_id(&corners, set.colors.len()) {
                Some(id) if !id.is_empty() => {
                    set.tiles.insert(tile, id);
                }
                Some(_) => {}
                None => invalid.push(tile),
            }
        }
        if let Some(first) = invalid.first() {
            self.record(
                WarningKind::InvalidWangId,
                format!(
                    "{} tiles of tileset “{tileset}” have terrain corners that name no terrain (tile {first} first); they are dropped",
                    invalid.len()
                ),
            );
        }
        set
    }

    /// A terrain set. Sets of Tiled 1.1 to 1.4 had separate corner and edge colours; they become
    /// one list and the tiles' colours are renumbered, as Tiled's reader does.
    fn wang_set_of(&mut self, tileset: &str, raw: RawWangSet) -> WangSet {
        let name = text_of(&raw.name);
        let what = format!("terrain set “{name}” of tileset “{tileset}”");
        let kind = self
            .named_of(
                &raw.kind,
                format_args!("{what}'s type"),
                names::wang_set_type_from_name,
            )
            .unwrap_or(WangSetType::Mixed);
        let tile = self
            .i32_of(&raw.tile, format_args!("{what}'s tile"))
            .and_then(|tile| u32::try_from(tile).ok())
            .map(TileId);
        let mut set = WangSet {
            name,
            class: text_of(&raw.class),
            kind,
            tile,
            colors: Vec::with_capacity(raw.colors.len()),
            tiles: Default::default(),
            properties: self.properties_of(raw.properties),
        };
        let mut corner_colors: Vec<u8> = Vec::new();
        let mut edge_colors: Vec<u8> = Vec::new();
        for color in raw.colors {
            let index = set.colors.len();
            let number = u8::try_from(index + 1).unwrap_or(u8::MAX);
            match color.role {
                WangColorRole::Corner => corner_colors.push(number),
                WangColorRole::Edge => edge_colors.push(number),
                WangColorRole::Any => {}
            }
            let color = self.wang_color_of(&what, color, index);
            set.colors.push(color);
        }
        let legacy = !corner_colors.is_empty() || !edge_colors.is_empty();
        if legacy {
            self.record(
                WarningKind::LegacyShape,
                format!("{what} has separate corner and edge colours (Tiled 1.1 to 1.4); they become one list"),
            );
        }
        let mut invalid = Vec::new();
        for tile in raw.tiles {
            let Some(tile_id) = self
                .u32_of(&tile.tile_id, format_args!("a tile id of {what}"))
                .map(TileId)
            else {
                continue;
            };
            let parsed = tile.wang_id.as_ref().and_then(parsed_wang_id);
            let Some(mut id) = parsed else {
                invalid.push(tile_id);
                continue;
            };
            if legacy {
                id = renumbered(id, &corner_colors, &edge_colors);
            }
            if id
                .lanes()
                .iter()
                .any(|&lane| usize::from(lane) > set.colors.len())
            {
                invalid.push(tile_id);
                continue;
            }
            if !id.is_empty() {
                set.tiles.insert(tile_id, id);
            }
        }
        if let Some(first) = invalid.first() {
            self.record(
                WarningKind::InvalidWangId,
                format!(
                    "{} tiles of {what} have terrain that names no colour of the set (tile {first} first); they are dropped",
                    invalid.len()
                ),
            );
        }
        if corner_colors.is_empty() && !edge_colors.is_empty() {
            set.kind = WangSetType::Edge;
        }
        if edge_colors.is_empty() && !corner_colors.is_empty() {
            set.kind = WangSetType::Corner;
        }
        set
    }

    fn wang_color_of(&mut self, set: &str, raw: RawWangColor, index: usize) -> WangColor {
        let name = text_of(&raw.name);
        let what = format!("terrain “{name}” of {set}");
        let color = self
            .color_of(&raw.color, format_args!("{what}'s colour"))
            .unwrap_or_else(|| default_color(index));
        WangColor {
            class: text_of(&raw.class),
            color,
            tile: self
                .i32_of(&raw.tile, format_args!("{what}'s tile"))
                .and_then(|tile| u32::try_from(tile).ok())
                .map(TileId),
            // Tiled's reader defaults to 0 here, but documents 1 and always writes it.
            probability: self
                .real_of(&raw.probability, format_args!("{what}'s probability"))
                .unwrap_or(1.0),
            properties: self.properties_of(raw.properties),
            name,
        }
    }
}

/// A terrain id as written: eight colours (comma-separated or a JSON array), or a number with
/// four bits a lane (hexadecimal text before Tiled 1.5).
fn parsed_wang_id(raw: &RawWangId) -> Option<WangId> {
    match raw {
        RawWangId::Text(text) if text.contains(',') => WangId::from_tiled_text(text),
        RawWangId::Text(text) => {
            let text = text.trim();
            let hex = text
                .strip_prefix("0x")
                .or_else(|| text.strip_prefix("0X"))
                .unwrap_or(text);
            packed_wang_id(u32::from_str_radix(hex, 16).ok()?)
        }
        RawWangId::Lanes(lanes) => {
            let mut out = [0u8; 8];
            if lanes.len() != out.len() {
                return None;
            }
            for (slot, lane) in out.iter_mut().zip(lanes) {
                *slot = u8::try_from(lane.to_int()?).ok()?;
            }
            Some(WangId::from_lanes(out))
        }
        RawWangId::Packed(value) => packed_wang_id(u32::try_from(*value).ok()?),
    }
}

/// The lanes of a number with four bits a lane, lane `i` in bits `4i` to `4i + 3`.
fn packed_wang_id(value: u32) -> Option<WangId> {
    let mut lanes = [0u8; 8];
    for (i, lane) in (0u32..).zip(lanes.iter_mut()) {
        *lane = u8::try_from((value >> (i * 4)) & 0xF).ok()?;
    }
    Some(WangId::from_lanes(lanes))
}

/// A terrain id of Tiled 1.1 to 1.4 with its corner and edge colour numbers mapped into the
/// merged list.
fn renumbered(id: WangId, corner_colors: &[u8], edge_colors: &[u8]) -> WangId {
    let mut out = id;
    for lane in Lane::ALL {
        let colors = if lane.is_corner() {
            corner_colors
        } else {
            edge_colors
        };
        let value = id.lane(lane);
        if value > 0
            && let Some(&merged) = colors.get(usize::from(value) - 1)
        {
            out = out.with_lane(lane, merged);
        }
    }
    out
}

/// The terrain id of a tile's corners (terrain `n` is colour `n + 1`); `None` when a corner
/// names no terrain of the set or the corners are not four.
fn terrain_wang_id(corners: &[Option<Scalar>], color_count: usize) -> Option<WangId> {
    if corners.len() != CORNER_LANES.len() {
        return None;
    }
    let mut id = WangId::default();
    for (corner, lane) in corners.iter().zip(CORNER_LANES) {
        let Some(corner) = corner else {
            continue;
        };
        let index = corner.to_int()?;
        if index == -1 {
            continue;
        }
        let index = usize::try_from(index).ok().filter(|i| *i < color_count)?;
        id = id.with_lane(lane, u8::try_from(index + 1).ok()?);
    }
    Some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_ids_read_in_every_form() {
        let text = |t: &str| parsed_wang_id(&RawWangId::Text(t.to_owned()));
        assert_eq!(
            text("0,1,0,2,0,3,0,4"),
            Some(WangId::from_lanes([0, 1, 0, 2, 0, 3, 0, 4]))
        );
        assert_eq!(
            text("0x10101010"),
            Some(WangId::from_lanes([0, 1, 0, 1, 0, 1, 0, 1]))
        );
        assert_eq!(
            text("21"),
            Some(WangId::from_lanes([1, 2, 0, 0, 0, 0, 0, 0]))
        );
        assert_eq!(text("1,2,3"), None);
        assert_eq!(text("zz"), None);
        let lanes = RawWangId::Lanes((0..8).map(|i| Scalar::Int(i % 2)).collect());
        assert_eq!(
            parsed_wang_id(&lanes),
            Some(WangId::from_lanes([0, 1, 0, 1, 0, 1, 0, 1]))
        );
        assert_eq!(parsed_wang_id(&RawWangId::Packed(0x21)), text("21"));
    }

    #[test]
    fn old_corners_and_edges_merge() {
        // Corner colour 1 is merged colour 2, edge colour 1 is merged colour 1.
        let id = WangId::from_lanes([1, 1, 1, 1, 0, 0, 0, 0]);
        assert_eq!(renumbered(id, &[2], &[1]).lanes(), [1, 2, 1, 2, 0, 0, 0, 0]);
    }

    #[test]
    fn terrain_corners_become_corner_lanes() {
        let corners = |c: [Option<i64>; 4]| c.map(|v| v.map(Scalar::Int)).to_vec();
        let id = terrain_wang_id(&corners([Some(0), None, Some(-1), Some(1)]), 2).unwrap();
        assert_eq!((id.lane(Lane::TopLeft), id.lane(Lane::TopRight)), (1, 0));
        assert_eq!(
            (id.lane(Lane::BottomLeft), id.lane(Lane::BottomRight)),
            (0, 2)
        );
        assert_eq!(
            terrain_wang_id(&corners([Some(5), None, None, None]), 2),
            None
        );
        assert_eq!(
            terrain_wang_id(&corners([None, None, None, None])[..3], 2),
            None
        );
    }
}
