//! The tiles the map shows, each with its flips under a key of its own (no global ids): where
//! to draw it from, its sprite's and terrains' names, its class, properties, animation and
//! collision shapes. Cells and tile objects name tiles by these keys. Also the tilesets, as
//! the JSON lists them.

use std::collections::{BTreeMap, HashMap};

use bt_core::{Cell, ClassUsage, Flip, Properties, TileId, Tileset, TilesetImage, TilesetSlot};
use bt_terrain::{Terrain, TerrainColor};
use bt_utils::IRect;
use glam::{DVec2, UVec2};

use super::definition::definition;
use super::document::{Bounds, DrawnFrom, FrameEntry, TerrainLanes, TileEntry, TilesetEntry};
use super::error::ResolvedWarning;
use super::resolving::Resolving;
use super::values::resolved_properties;
use crate::at::{LaneWord, lanes_in_reading_order};

/// The tiles shown so far, by key, and the terrain sets read to name their lanes.
#[derive(Default)]
pub(crate) struct Palette {
    /// Each tile with its flips, by key.
    tiles: BTreeMap<usize, TileEntry>,
    /// The key of each tile with its flips.
    keys: HashMap<Cell, usize>,
    /// Each tileset's terrain sets, read the first time one of its tiles is keyed; `None` for
    /// a set that cannot be read.
    terrains: HashMap<TilesetSlot, Vec<Option<Terrain>>>,
}

/// Why a tile has no key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unkeyed {
    /// It is one of AutoMap's rule tiles, which are left out.
    RuleTile(TilesetSlot),
    /// Its tileset is not one of the map's.
    NoTileset,
}

impl Palette {
    /// The key of `cell`'s tile with its flips, given one the first time it is asked for.
    pub(crate) fn key_of(
        &mut self,
        resolving: &mut Resolving,
        cell: Cell,
    ) -> Result<usize, Unkeyed> {
        if let Some(&key) = self.keys.get(&cell) {
            return Ok(key);
        }
        let slot = cell.slot().ok_or(Unkeyed::NoTileset)?;
        if resolving.is_rule_tileset(slot) {
            return Err(Unkeyed::RuleTile(slot));
        }
        let map = resolving.map;
        let tileset = map.tileset(slot).ok_or(Unkeyed::NoTileset)?;
        let entry = self.tile_entry(resolving, slot, tileset, cell);
        let key = self.tiles.len() + 1;
        self.tiles.insert(key, entry);
        self.keys.insert(cell, key);
        Ok(key)
    }

    /// The tiles, by key.
    pub(crate) fn into_tiles(self) -> BTreeMap<usize, TileEntry> {
        self.tiles
    }

    /// The entry of `cell`'s tile, of `tileset` in `slot`.
    fn tile_entry(
        &mut self,
        resolving: &mut Resolving,
        slot: TilesetSlot,
        tileset: &Tileset,
        cell: Cell,
    ) -> TileEntry {
        let map = resolving.map;
        let id = cell.tile();
        let name = tileset_name(resolving, slot, tileset);
        let data = tileset.tile(id);
        let animation = data.map_or_else(Vec::new, |data| {
            data.animation
                .iter()
                .map(|frame| FrameEntry {
                    tile_id: frame.tile.0,
                    drawn_from: drawn_from(resolving, slot, tileset, frame.tile),
                    duration_ms: frame.duration_ms,
                })
                .collect()
        });
        let collision =
            data.and_then(|data| data.collision.as_ref())
                .map_or_else(Vec::new, |group| {
                    group
                        .layer
                        .objects
                        .iter()
                        .map(|shape| definition(resolving, shape, None, DVec2::ZERO))
                        .collect()
                });
        let flip = cell.flip();
        TileEntry {
            tileset: name,
            tile_id: id.0,
            drawn_from: drawn_from(resolving, slot, tileset, id),
            flip_h: flip.contains(Flip::H),
            flip_v: flip.contains(Flip::V),
            flip_d: flip.contains(Flip::D),
            rotated_hex_120: flip.contains(Flip::HEX120),
            sprite: resolving.names.sprite_of(cell).map(|s| s.name.clone()),
            terrain: resolving
                .names
                .terrain_covering(map, cell)
                .map(|t| t.name.clone()),
            terrains: self.terrain_lanes(resolving, slot, tileset, cell),
            class: data.map_or_else(String::new, |data| data.class.clone()),
            properties: resolved_properties(
                resolving.types,
                data.map_or("", |data| data.class.as_str()),
                ClassUsage::TILE,
                data.map_or(&Properties::new(), |data| &data.properties),
            ),
            animation,
            collision,
        }
    }

    /// The terrain on each lane of `cell`'s tile as it is drawn, for each terrain set of
    /// `tileset` that labels the tile.
    fn terrain_lanes(
        &mut self,
        resolving: &mut Resolving,
        slot: TilesetSlot,
        tileset: &Tileset,
        cell: Cell,
    ) -> Vec<TerrainLanes> {
        let sets = self.terrains.entry(slot).or_insert_with(|| {
            tileset
                .wang_sets
                .iter()
                .enumerate()
                .map(
                    |(index, set)| match Terrain::from_tileset(tileset, slot, index) {
                        Ok(terrain) => Some(terrain),
                        Err(error) => {
                            resolving.record_warning(ResolvedWarning::TerrainSet {
                                tileset: tileset.name.clone(),
                                set: set.name.clone(),
                                error,
                            });
                            None
                        }
                    },
                )
                .collect()
        });
        let mut out = Vec::new();
        for (index, (set, terrain)) in tileset.wang_sets.iter().zip(sets.iter()).enumerate() {
            let Some(terrain) = terrain else {
                continue;
            };
            if !set.tiles.contains_key(&cell.tile()) {
                continue;
            }
            let label = terrain.wang_of_cell(cell);
            let lanes = lanes_in_reading_order(set.kind)
                .into_iter()
                .map(|LaneWord { lane, word }| {
                    let color = label.lane(lane);
                    let terrain = (color != 0).then(|| {
                        resolving
                            .names
                            .terrain_of(slot, index, TerrainColor(color))
                            .map_or_else(|| format!("terrain {color}"), |t| t.name.clone())
                    });
                    (word, terrain)
                })
                .collect();
            out.push(TerrainLanes {
                set: set.name.clone(),
                lanes,
            });
        }
        out
    }
}

/// The tileset's name as bt gives it, unique in the map.
fn tileset_name(resolving: &Resolving, slot: TilesetSlot, tileset: &Tileset) -> String {
    resolving
        .names
        .tileset_name(slot)
        .map_or_else(|| tileset.name.clone(), str::to_owned)
}

/// Where tile `id` of `tileset`, in `slot`, is drawn from: its image, the part of it, its atlas
/// place. Tiles without an image are said once for their tileset.
pub(crate) fn drawn_from(
    resolving: &mut Resolving,
    slot: TilesetSlot,
    tileset: &Tileset,
    id: TileId,
) -> DrawnFrom {
    let Some(image) = tileset.tile_image(id) else {
        resolving.record_tile_without_image(slot, id);
        return DrawnFrom::default();
    };
    // An atlas has columns when it has a tile's image.
    let atlas = match &tileset.image {
        TilesetImage::Atlas { columns, .. } if *columns > 0 => {
            Some(UVec2::new(id.0 % columns, id.0 / columns))
        }
        TilesetImage::Atlas { .. } | TilesetImage::Collection => None,
    };
    let what = || match &tileset.image {
        TilesetImage::Atlas { .. } => format!("tileset “{}”", tileset.name),
        TilesetImage::Collection => format!("tile {} of tileset “{}”", id.0, tileset.name),
    };
    DrawnFrom {
        image: resolving.image_path(image.image, what),
        source_rect: Some(int_bounds(image.rect)),
        atlas,
    }
}

/// An integer rectangle as the JSON writes rectangles.
fn int_bounds(rect: IRect) -> Bounds<i32> {
    Bounds {
        left: rect.x,
        top: rect.y,
        width: rect.w,
        height: rect.h,
    }
}

/// The map's tilesets as the JSON lists them, without AutoMap's rule tiles.
pub(crate) fn tileset_entries(resolving: &mut Resolving) -> Vec<TilesetEntry> {
    let map = resolving.map;
    let mut out = Vec::new();
    for (index, used) in map.tilesets.iter().enumerate() {
        let Some(slot) = TilesetSlot::new(index) else {
            continue;
        };
        if resolving.is_rule_tileset(slot) {
            continue;
        }
        let tileset = used.tileset.as_ref();
        let atlas = match &tileset.image {
            TilesetImage::Atlas { image, columns, .. } => Some(AtlasParts {
                image: resolving.image_path(image, || format!("tileset “{}”", tileset.name)),
                image_size: (image.declared_size != UVec2::ZERO).then_some(image.declared_size),
                columns: *columns,
            }),
            TilesetImage::Collection => None,
        };
        out.push(TilesetEntry {
            name: tileset_name(resolving, slot, tileset),
            tiled_name: tileset.name.clone(),
            kind: if atlas.is_some() { "atlas" } else { "images" },
            image: atlas.as_ref().and_then(|a| a.image.clone()),
            image_size: atlas.as_ref().and_then(|a| a.image_size),
            tile_size: tileset.tile_size(),
            tile_count: tileset.tile_count(),
            columns: atlas.as_ref().map(|a| a.columns),
            rows: atlas
                .as_ref()
                .filter(|a| a.columns > 0)
                .map(|a| tileset.tile_count().div_ceil(a.columns)),
            margin: atlas.as_ref().map(|_| tileset.margin),
            spacing: atlas.as_ref().map(|_| tileset.spacing),
            tile_offset: tileset.tile_offset,
            class: tileset.class.clone(),
            properties: resolved_properties(
                resolving.types,
                &tileset.class,
                ClassUsage::TILESET,
                &tileset.properties,
            ),
        });
    }
    out
}

/// What an atlas tileset adds to its entry.
struct AtlasParts {
    image: Option<String>,
    image_size: Option<UVec2>,
    columns: u32,
}
