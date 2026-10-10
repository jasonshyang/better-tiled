//! Names (`10-agents.md`, §5): what an agent calls the things a map is made of, so that no
//! command asks for a tile id where a name exists. Names are slugs (`pine-tree-big`), and every
//! lookup slugs what it is given, so case and separators never matter.
//!
//! [`Names::of`] builds the map's tables once: its tilesets, its terrains (a colour's name, the
//! set's name when it has one colour, else `set-n`, qualified `set/name` and then
//! `tileset/set/name` while names collide, then numbered), its sprites (the tiles of
//! image-collection tilesets, named after their images, less the frames of animations) and its
//! tile groups (tiles' classes). Layers and objects are found in the map as it is when asked,
//! since a script changes them. What a script defines (materials, groups of sprites, tile names,
//! areas and pieces) is kept beside the tables and survives [`Names::rebuild_tables`].
//!
//! An unknown name answers with the closest three and where the full list is.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use bt_core::{
    Cell, ImageSource, Lane, Layer, LayerId, LayerKind, Map, MapObject, ObjectId, TileId, Tileset,
    TilesetSlot, WangSetType,
};
use bt_edit::{ObjectClip, TileClip};
use bt_terrain::TerrainColor;
use bt_utils::hash::CellHashSet;
use bt_utils::paths::file_stem;
use bt_utils::text::slug;
use glam::{DVec2, IVec2, UVec2};

use crate::context::Context;
use crate::error::{Error, NameKind};
use crate::script::{LayerRef, Named, ObjectRef, TileIds};
use crate::words::capped_list;

/// The most ranges of ids a tileset's range is written with.
const RANGES_SHOWN: usize = 8;

/// The names of one map's tilesets, terrains, sprites and tile groups, and what a script
/// defined.
#[derive(Clone, Debug)]
pub struct Names {
    /// Each tileset's name, by slot.
    tilesets: Vec<String>,
    terrains: Vec<NamedTerrain>,
    sprites: Vec<Sprite>,
    /// Where each sprite is in `sprites`, by its tile.
    sprite_index: HashMap<Cell, usize>,
    /// Tiles named by their class.
    classes: Vec<TileGroup>,
    /// What the script defined; one name means one thing.
    defined: BTreeMap<String, Definition>,
}

/// A terrain: one colour of one terrain set of the map's tilesets, by its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedTerrain {
    /// The name, unique in the map: `water`, `sea/water` or `outdoor/sea/water`.
    pub name: String,
    /// The tileset.
    pub slot: TilesetSlot,
    /// The terrain set's index in its tileset.
    pub set: usize,
    /// The colour in its set.
    pub color: TerrainColor,
    /// Which lanes the set uses.
    pub kind: WangSetType,
}

/// A sprite: a tile of an image-collection tileset, placed as a tile object by its foot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sprite {
    /// The name, after the image file's stem; unique in the map.
    pub name: String,
    /// The tile, unflipped.
    pub tile: Cell,
    /// The image's size in pixels.
    pub size: UVec2,
    /// Whether the tile is animated.
    pub animated: bool,
}

/// Tiles under one name: a class of tiles, `define NAME = tile …`, or tiles written by id; any
/// of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileGroup {
    /// The name, or the tiles as written.
    pub name: String,
    /// The tiles, unflipped.
    pub tiles: Vec<Cell>,
    /// The layer they go on, when the name says.
    pub layer: Option<LayerRef>,
}

/// One part of a material (`define NAME = TERRAIN [layer L] + …`): a terrain and the layer it
/// is painted on. A terrain alone is a material of one part without a layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialPart {
    /// The terrain.
    pub terrain: NamedTerrain,
    /// The layer, when the definition names one.
    pub layer: Option<LayerRef>,
}

/// One sprite of a group (`define NAME = sprite S [layer L] [+ sprite S2 at DX,DY [layer L]]`):
/// the first stands at the foot, the others at their offsets from it. A sprite alone is a group
/// of one.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupPart {
    /// The sprite.
    pub sprite: Sprite,
    /// Where it stands from the first sprite's foot, in tile units.
    pub offset: DVec2,
    /// The layer, when the definition names one.
    pub layer: Option<LayerRef>,
}

/// A piece (`define NAME = copy MAP REGION …`): tiles and objects copied from a map once, placed
/// by a foot cell.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    /// The tiles, a layer per layer name, relative to the region's top-left (the clip's origin is
    /// the region's top-left in the map they came from; its size is the region's size).
    pub tiles: TileClip,
    /// The objects standing in the region, by the name of their layer.
    pub objects: Vec<PieceObjects>,
    /// The foot cell, relative to the region's top-left.
    pub foot: IVec2,
}

/// A piece's objects of one layer.
#[derive(Clone, Debug, PartialEq)]
pub struct PieceObjects {
    /// The name of the layer they go on, made when the map lacks it.
    pub layer: String,
    /// The objects, as they were in the map they came from.
    pub objects: ObjectClip,
}

/// What a name in `where`, `on`, `inside`, `avoid` and `near` stands for.
#[derive(Clone, Debug, PartialEq)]
pub enum Ground {
    /// A terrain: it covers a cell when it is on every lane of its set's kind on some tile
    /// layer, and touches it when it is on any lane.
    Terrain(NamedTerrain),
    /// Tiles: they cover a cell when the cell holds one of them on the layer the name says, or
    /// on any tile layer.
    Tiles(TileGroup),
    /// A defined area: it covers its cells.
    Area {
        /// The area's name.
        name: String,
        /// Its cells.
        cells: Arc<CellHashSet<IVec2>>,
    },
}

/// What a script defined under a name.
#[derive(Clone, Debug)]
enum Definition {
    Material(Vec<MaterialPart>),
    Group(Vec<GroupPart>),
    Tiles(TileGroup),
    Area(Arc<CellHashSet<IVec2>>),
    Piece(Arc<Piece>),
}

impl Definition {
    fn kind(&self) -> NameKind {
        match self {
            Definition::Material(_) => NameKind::Material,
            Definition::Group(_) => NameKind::Group,
            Definition::Tiles(_) => NameKind::Tiles,
            Definition::Area(_) => NameKind::Area,
            Definition::Piece(_) => NameKind::Piece,
        }
    }
}

/// What a terrain name matched.
enum TerrainMatch<'n> {
    /// One terrain: the name, or the last parts of one name.
    One(&'n NamedTerrain),
    /// The last parts of several names.
    Several(Vec<&'n NamedTerrain>),
    /// Nothing.
    Nothing,
}

/// Which kind of layer a lookup needs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LayerWanted {
    Any,
    Tiles,
    Objects,
}

impl Names {
    /// The names of `map`.
    pub fn of(map: &Map) -> Names {
        let mut names = Names {
            tilesets: Vec::new(),
            terrains: Vec::new(),
            sprites: Vec::new(),
            sprite_index: HashMap::new(),
            classes: Vec::new(),
            defined: BTreeMap::new(),
        };
        names.rebuild_tables(map);
        names
    }

    /// Builds the tables again from `map`, after a script added layers or tilesets; what the
    /// script defined is kept.
    pub fn rebuild_tables(&mut self, map: &Map) {
        self.tilesets = tileset_names(map);
        self.terrains = terrain_names(map, &self.tilesets);
        self.sprites = sprite_names(map, &self.tilesets);
        self.sprite_index = self
            .sprites
            .iter()
            .enumerate()
            .map(|(index, sprite)| (sprite.tile, index))
            .collect();
        self.classes = class_groups(map);
    }

    // The tables.

    /// The terrains, tileset by tileset and set by set.
    pub fn terrains(&self) -> &[NamedTerrain] {
        &self.terrains
    }

    /// The sprites, tileset by tileset.
    pub fn sprites(&self) -> &[Sprite] {
        &self.sprites
    }

    /// The tiles named by their class.
    pub fn tile_classes(&self) -> &[TileGroup] {
        &self.classes
    }

    /// A tileset's name.
    pub fn tileset_name(&self, slot: TilesetSlot) -> Option<&str> {
        self.tilesets.get(slot.index()).map(String::as_str)
    }

    /// A tileset by name.
    pub fn tileset(&self, query: &str) -> Result<TilesetSlot, Error> {
        let wanted = slug(query);
        self.tilesets
            .iter()
            .position(|name| *name == wanted)
            .and_then(TilesetSlot::new)
            .ok_or_else(|| {
                let names: Vec<&str> = self.tilesets.iter().map(String::as_str).collect();
                Error::unknown(NameKind::Tileset, query, &names)
            })
    }

    /// A terrain by name: its own name, or the last parts of exactly one qualified name
    /// (`water` for `sea/water`).
    pub fn terrain(&self, query: &str) -> Result<&NamedTerrain, Error> {
        match self.match_terrain(query) {
            TerrainMatch::One(terrain) => Ok(terrain),
            TerrainMatch::Several(terrains) => Err(ambiguous(query, &terrains)),
            TerrainMatch::Nothing => {
                if let Some(kind) = self.kind_of(query) {
                    return Err(Error::WrongKind {
                        name: query.to_owned(),
                        found: kind,
                        wanted: NameKind::Terrain,
                    });
                }
                Err(Error::unknown(
                    NameKind::Terrain,
                    query,
                    &self.terrain_name_list(),
                ))
            }
        }
    }

    /// The terrain of a colour of a set of a tileset.
    pub fn terrain_of(
        &self,
        slot: TilesetSlot,
        set: usize,
        color: TerrainColor,
    ) -> Option<&NamedTerrain> {
        self.terrains
            .iter()
            .find(|t| t.slot == slot && t.set == set && t.color == color)
    }

    /// A sprite by name, or as `TILESET:ID`.
    pub fn sprite(&self, query: &str) -> Result<&Sprite, Error> {
        if let Some((tileset, id)) = query.rsplit_once(':') {
            let slot = self.tileset(tileset)?;
            let id: u32 = id.trim().parse().map_err(|_| Error::NotASprite {
                query: query.to_owned(),
            })?;
            return self
                .sprite_of(Cell::new(slot, TileId(id)))
                .ok_or_else(|| Error::NotASprite {
                    query: query.to_owned(),
                });
        }
        let wanted = slug(query);
        if let Some(sprite) = self.sprites.iter().find(|s| s.name == wanted) {
            return Ok(sprite);
        }
        if let Some(kind) = self.kind_of(query) {
            return Err(Error::WrongKind {
                name: query.to_owned(),
                found: kind,
                wanted: NameKind::Sprite,
            });
        }
        let names: Vec<&str> = self.sprites.iter().map(|s| s.name.as_str()).collect();
        Err(Error::unknown(NameKind::Sprite, query, &names))
    }

    /// The sprite a tile is, whatever its flips.
    pub fn sprite_of(&self, tile: Cell) -> Option<&Sprite> {
        let unflipped = Cell::new(tile.slot()?, tile.tile());
        let index = *self.sprite_index.get(&unflipped)?;
        self.sprites.get(index)
    }

    /// How a tile reads: its sprite's name, else `TILESET:ID`; `nothing` for the empty cell.
    pub fn tile_text(&self, tile: Cell) -> String {
        if let Some(sprite) = self.sprite_of(tile) {
            return sprite.name.clone();
        }
        match tile.slot() {
            Some(slot) => match self.tileset_name(slot) {
                Some(tileset) => format!("{tileset}:{}", tile.tile()),
                None => format!("tileset {}:{}", slot.index(), tile.tile()),
            },
            None => "nothing".to_owned(),
        }
    }

    /// The terrain covering more than half of a tile: on more than half of the lanes its set
    /// uses, in the first of its tileset's terrain sets where one does.
    pub fn terrain_covering(&self, map: &Map, tile: Cell) -> Option<&NamedTerrain> {
        let slot = tile.slot()?;
        let tileset = map.tileset(slot)?;
        tileset
            .wang_sets
            .iter()
            .enumerate()
            .find_map(|(index, set)| {
                let label = set.tiles.get(&tile.tile())?;
                let lanes: Vec<u8> = Lane::ALL
                    .into_iter()
                    .filter(|&lane| set.kind.uses(lane))
                    .map(|lane| label.lane(lane))
                    .collect();
                let color = lanes.iter().copied().find(|&color| {
                    color != 0 && lanes.iter().filter(|&&c| c == color).count() * 2 > lanes.len()
                })?;
                self.terrain_of(slot, index, TerrainColor(color))
            })
    }

    /// What a tile shows, in words: the terrain it is wholly, `grass/water edges` for a
    /// terrain's edge, else [`Names::tile_text`].
    pub fn tile_label(&self, map: &Map, tile: Cell) -> String {
        self.terrain_label(map, tile)
            .unwrap_or_else(|| self.tile_text(tile))
    }

    /// The terrains a tile shows, in the first of its tileset's terrain sets that puts one on
    /// it: `water`, or `grass/water edges`.
    fn terrain_label(&self, map: &Map, tile: Cell) -> Option<String> {
        let slot = tile.slot()?;
        let tileset = map.tileset(slot)?;
        tileset
            .wang_sets
            .iter()
            .enumerate()
            .find_map(|(index, set)| {
                let label = set.tiles.get(&tile.tile())?;
                let lanes: Vec<u8> = Lane::ALL
                    .into_iter()
                    .filter(|&lane| set.kind.uses(lane))
                    .map(|lane| label.lane(lane))
                    .collect();
                let whole = lanes.windows(2).all(|pair| pair.first() == pair.last());
                let mut colors: Vec<u8> = lanes.into_iter().filter(|&c| c != 0).collect();
                colors.sort_unstable();
                colors.dedup();
                if colors.is_empty() {
                    return None;
                }
                let words: Vec<String> = colors
                    .iter()
                    .map(
                        |&color| match self.terrain_of(slot, index, TerrainColor(color)) {
                            Some(terrain) => terrain.name.clone(),
                            None => format!("terrain {color}"),
                        },
                    )
                    .collect();
                Some(if whole && words.len() == 1 {
                    words.join("")
                } else {
                    format!("{} edges", words.join("/"))
                })
            })
    }

    // What operations take.

    /// What `paint` and `erase` paint: a material's parts, or a terrain as a material of one
    /// part.
    pub fn material(&self, query: &str) -> Result<Vec<MaterialPart>, Error> {
        if let Some(Definition::Material(parts)) = self.defined.get(&slug(query)) {
            return Ok(parts.clone());
        }
        match self.match_terrain(query) {
            TerrainMatch::One(terrain) => Ok(vec![MaterialPart {
                terrain: terrain.clone(),
                layer: None,
            }]),
            TerrainMatch::Several(terrains) => Err(ambiguous(query, &terrains)),
            TerrainMatch::Nothing => {
                if let Some(kind) = self.kind_of(query) {
                    return Err(Error::WrongKind {
                        name: query.to_owned(),
                        found: kind,
                        wanted: NameKind::Terrain,
                    });
                }
                let mut names = self.terrain_name_list();
                names.extend(self.defined_names(NameKind::Material));
                Err(Error::unknown(NameKind::Terrain, query, &names))
            }
        }
    }

    /// What `place` and `scatter` place when the name is not a piece ([`Names::piece`]): a
    /// group's sprites, or a sprite (by name or `TILESET:ID`) as a group of one.
    pub fn group(&self, query: &str) -> Result<Vec<GroupPart>, Error> {
        if let Some(Definition::Group(parts)) = self.defined.get(&slug(query)) {
            return Ok(parts.clone());
        }
        let alone = |sprite: &Sprite| {
            vec![GroupPart {
                sprite: sprite.clone(),
                offset: DVec2::ZERO,
                layer: None,
            }]
        };
        if query.contains(':') {
            return self.sprite(query).map(alone);
        }
        if let Some(sprite) = self.sprites.iter().find(|s| s.name == slug(query)) {
            return Ok(alone(sprite));
        }
        if let Some(kind) = self.kind_of(query) {
            return Err(Error::WrongKind {
                name: query.to_owned(),
                found: kind,
                wanted: NameKind::Sprite,
            });
        }
        let mut names: Vec<&str> = self.sprites.iter().map(|s| s.name.as_str()).collect();
        names.extend(self.defined_names(NameKind::Group));
        names.extend(self.defined_names(NameKind::Piece));
        Err(Error::unknown(NameKind::Sprite, query, &names))
    }

    /// A piece by name, if the script defined one.
    pub fn piece(&self, query: &str) -> Option<Arc<Piece>> {
        match self.defined.get(&slug(query)) {
            Some(Definition::Piece(piece)) => Some(Arc::clone(piece)),
            _ => None,
        }
    }

    /// What `tile` places, any of them: tiles by id, tile groups (defined, then classes) and
    /// sprites' tiles, with the layer a tile name declares.
    pub fn tiles(&self, map: &Map, named: &[Named]) -> Result<TileGroup, Error> {
        let mut tiles: Vec<Cell> = Vec::new();
        let mut written: Vec<String> = Vec::new();
        let mut declared: Option<Declared> = None;
        for name in named {
            match name {
                Named::Tiles(ids) => {
                    tiles.extend(self.tile_ids(map, ids)?);
                    written.push(tile_ids_text(ids));
                }
                Named::Name(name) => {
                    let group = self.tile_group(name)?;
                    if let Some(layer) = &group.layer {
                        match &declared {
                            None => {
                                declared = Some(Declared {
                                    name: group.name.clone(),
                                    layer: layer.clone(),
                                });
                            }
                            Some(first) if first.layer != *layer => {
                                return Err(Error::LayersDisagree {
                                    first: first.name.clone(),
                                    first_layer: layer_ref_text(&first.layer),
                                    second: group.name.clone(),
                                    second_layer: layer_ref_text(layer),
                                });
                            }
                            Some(_) => {}
                        }
                    }
                    tiles.extend(group.tiles);
                    written.push(group.name);
                }
            }
        }
        let mut seen = std::collections::HashSet::new();
        tiles.retain(|tile| seen.insert(*tile));
        Ok(TileGroup {
            name: written.join(","),
            tiles,
            layer: declared.map(|d| d.layer),
        })
    }

    /// The grounds names stand for in `where`, `on`, `inside`, `avoid` and `near`: each name is
    /// a terrain, a tile group (defined, then a class) or a defined area, in that order; tiles
    /// written by id are one ground each.
    pub fn grounds(&self, map: &Map, named: &[Named]) -> Result<Vec<Ground>, Error> {
        named.iter().map(|name| self.ground(map, name)).collect()
    }

    fn ground(&self, map: &Map, named: &Named) -> Result<Ground, Error> {
        let query = match named {
            Named::Tiles(ids) => {
                return Ok(Ground::Tiles(TileGroup {
                    name: tile_ids_text(ids),
                    tiles: self.tile_ids(map, ids)?,
                    layer: None,
                }));
            }
            Named::Name(query) => query,
        };
        match self.match_terrain(query) {
            TerrainMatch::One(terrain) => return Ok(Ground::Terrain(terrain.clone())),
            TerrainMatch::Several(terrains) => return Err(ambiguous(query, &terrains)),
            TerrainMatch::Nothing => {}
        }
        let key = slug(query);
        match self.defined.get(&key) {
            Some(Definition::Tiles(group)) => return Ok(Ground::Tiles(group.clone())),
            Some(Definition::Area(cells)) if self.class(&key).is_none() => {
                return Ok(Ground::Area {
                    name: key,
                    cells: Arc::clone(cells),
                });
            }
            _ => {}
        }
        if let Some(group) = self.class(&key) {
            return Ok(Ground::Tiles(group.clone()));
        }
        if let Some(kind) = self.kind_of(query) {
            return Err(Error::WrongKind {
                name: query.to_owned(),
                found: kind,
                wanted: NameKind::Ground,
            });
        }
        let mut names = self.terrain_name_list();
        names.extend(self.classes.iter().map(|g| g.name.as_str()));
        names.extend(self.defined_names(NameKind::Tiles));
        names.extend(self.defined_names(NameKind::Area));
        Err(Error::unknown(NameKind::Ground, query, &names))
    }

    /// A tile name: defined, a class, or a sprite's tile.
    fn tile_group(&self, query: &str) -> Result<TileGroup, Error> {
        let key = slug(query);
        if let Some(Definition::Tiles(group)) = self.defined.get(&key) {
            return Ok(group.clone());
        }
        if let Some(group) = self.class(&key) {
            return Ok(group.clone());
        }
        if let Some(sprite) = self.sprites.iter().find(|s| s.name == key) {
            return Ok(TileGroup {
                name: sprite.name.clone(),
                tiles: vec![sprite.tile],
                layer: None,
            });
        }
        if let Some(kind) = self.kind_of(query) {
            return Err(Error::WrongKind {
                name: query.to_owned(),
                found: kind,
                wanted: NameKind::Tiles,
            });
        }
        let mut names: Vec<&str> = self.defined_names(NameKind::Tiles);
        names.extend(self.classes.iter().map(|g| g.name.as_str()));
        names.extend(self.sprites.iter().map(|s| s.name.as_str()));
        Err(Error::unknown(NameKind::Tiles, query, &names))
    }

    /// The tiles `TILESET:ID,…` stands for; an id the tileset lacks says the tileset's range.
    fn tile_ids(&self, map: &Map, ids: &TileIds) -> Result<Vec<Cell>, Error> {
        let slot = self.tileset(&ids.tileset)?;
        let tileset = map.tileset(slot).ok_or_else(|| {
            let names: Vec<&str> = self.tilesets.iter().map(String::as_str).collect();
            Error::unknown(NameKind::Tileset, &ids.tileset, &names)
        })?;
        let name = self.tileset_name(slot).unwrap_or(&ids.tileset).to_owned();
        let mut cells = Vec::new();
        for range in &ids.ranges {
            if range.first > range.last {
                return Err(Error::BackwardRange {
                    tileset: ids.tileset.clone(),
                    first: range.first,
                    last: range.last,
                });
            }
            for id in range.first..=range.last {
                if !tileset.has_tile(TileId(id)) {
                    return Err(Error::NoSuchTile {
                        tileset: name,
                        id,
                        range: range_text(tileset),
                    });
                }
                cells.push(Cell::new(slot, TileId(id)));
            }
        }
        Ok(cells)
    }

    // Layers and objects, as the map is now.

    /// A layer of any kind, by name or `#ID`.
    pub fn layer(&self, map: &Map, layer: &LayerRef) -> Result<LayerId, Error> {
        find_layer(map, layer, LayerWanted::Any)
    }

    /// A tile layer, by name or `#ID`. Of several layers of the name, the one tile layer is
    /// meant.
    pub fn tile_layer(&self, map: &Map, layer: &LayerRef) -> Result<LayerId, Error> {
        find_layer(map, layer, LayerWanted::Tiles)
    }

    /// An object layer, by name or `#ID`. Of several layers of the name, the one object layer is
    /// meant.
    pub fn object_layer(&self, map: &Map, layer: &LayerRef) -> Result<LayerId, Error> {
        find_layer(map, layer, LayerWanted::Objects)
    }

    /// The objects a reference stands for: `#ID`; a name when one object has it, else a type
    /// when one object has it; `selected` and `last` from `context`.
    pub fn objects(
        &self,
        map: &Map,
        target: &ObjectRef,
        context: &Context,
    ) -> Result<Vec<ObjectId>, Error> {
        let exists = |ids: &[ObjectId]| -> Result<Vec<ObjectId>, Error> {
            match ids.iter().find(|&&id| map.object(id).is_none()) {
                Some(missing) => Err(Error::NoSuchObject { id: missing.0 }),
                None => Ok(ids.to_vec()),
            }
        };
        match target {
            ObjectRef::Id(id) => exists(&[ObjectId(*id)]),
            ObjectRef::Selected => exists(context.selected_objects()?),
            ObjectRef::Last => exists(context.last_objects()?),
            ObjectRef::Name(name) => objects_named(map, name),
        }
    }

    // What a script defines.

    /// Defines a material: terrains painted together, in order.
    pub fn define_material(&mut self, name: &str, parts: Vec<MaterialPart>) -> Result<(), Error> {
        self.define(name, Definition::Material(parts))
    }

    /// Defines a group: sprites placed together at offsets from the first's foot.
    pub fn define_group(&mut self, name: &str, parts: Vec<GroupPart>) -> Result<(), Error> {
        self.define(name, Definition::Group(parts))
    }

    /// Defines a tile name for tiles, with the layer they go on when the definition names one.
    pub fn define_tiles(
        &mut self,
        name: &str,
        tiles: Vec<Cell>,
        layer: Option<LayerRef>,
    ) -> Result<(), Error> {
        let group = TileGroup {
            name: slug(name),
            tiles,
            layer,
        };
        self.define(name, Definition::Tiles(group))
    }

    /// Defines an area: the cells as they are now.
    pub fn define_area(
        &mut self,
        name: &str,
        cells: impl IntoIterator<Item = IVec2>,
    ) -> Result<(), Error> {
        let cells: CellHashSet<IVec2> = cells.into_iter().collect();
        self.define(name, Definition::Area(Arc::new(cells)))
    }

    /// Defines a piece.
    pub fn define_piece(&mut self, name: &str, piece: Piece) -> Result<(), Error> {
        self.define(name, Definition::Piece(Arc::new(piece)))
    }

    /// Keeps `definition` under `name`, in place of anything the script defined under it
    /// before; refused when the map calls something by the name.
    fn define(&mut self, name: &str, definition: Definition) -> Result<(), Error> {
        let key = slug(name);
        if let Some(by) = self.map_kind_of(&key) {
            return Err(Error::NameTaken { name: key, by });
        }
        self.defined.insert(key, definition);
        Ok(())
    }

    // Lookups shared by the above.

    fn match_terrain(&self, query: &str) -> TerrainMatch<'_> {
        let parts: Vec<String> = query.split('/').map(slug).collect();
        if parts.iter().any(String::is_empty) {
            return TerrainMatch::Nothing;
        }
        let wanted = parts.join("/");
        if let Some(terrain) = self.terrains.iter().find(|t| t.name == wanted) {
            return TerrainMatch::One(terrain);
        }
        let tails: Vec<&NamedTerrain> = self
            .terrains
            .iter()
            .filter(|t| {
                let name: Vec<&str> = t.name.split('/').collect();
                name.len() > parts.len()
                    && name
                        .iter()
                        .rev()
                        .zip(parts.iter().rev())
                        .all(|(have, want)| have == want)
            })
            .collect();
        match tails.len() {
            0 => TerrainMatch::Nothing,
            1 => tails
                .first()
                .map_or(TerrainMatch::Nothing, |&one| TerrainMatch::One(one)),
            _ => TerrainMatch::Several(tails),
        }
    }

    fn class(&self, key: &str) -> Option<&TileGroup> {
        self.classes.iter().find(|g| g.name == key)
    }

    /// What the map or the script calls `query`, if anything.
    fn kind_of(&self, query: &str) -> Option<NameKind> {
        let key = slug(query);
        match self.defined.get(&key) {
            Some(definition) => Some(definition.kind()),
            None => self.map_kind_of(query),
        }
    }

    /// What the map calls `query`, if anything: a terrain, a sprite or a class of tiles.
    fn map_kind_of(&self, query: &str) -> Option<NameKind> {
        let key = slug(query);
        if !matches!(self.match_terrain(query), TerrainMatch::Nothing) {
            Some(NameKind::Terrain)
        } else if self.sprites.iter().any(|s| s.name == key) {
            Some(NameKind::Sprite)
        } else if self.class(&key).is_some() {
            Some(NameKind::Tiles)
        } else {
            None
        }
    }

    fn terrain_name_list(&self) -> Vec<&str> {
        self.terrains.iter().map(|t| t.name.as_str()).collect()
    }

    fn defined_names(&self, kind: NameKind) -> Vec<&str> {
        self.defined
            .iter()
            .filter(|(_, definition)| definition.kind() == kind)
            .map(|(name, _)| name.as_str())
            .collect()
    }
}

/// The first tile name that declared a layer.
struct Declared {
    name: String,
    layer: LayerRef,
}

/// An object's type: its class, else its tile's (as Tiled reads it).
pub fn object_type<'m>(map: &'m Map, object: &'m MapObject) -> &'m str {
    if !object.class.is_empty() {
        return &object.class;
    }
    object
        .tile
        .and_then(|tile| map.tile_data(tile))
        .map_or("", |data| data.class.as_str())
}

/// The objects called `name`, else of type `name`: exactly one of either.
fn objects_named(map: &Map, name: &str) -> Result<Vec<ObjectId>, Error> {
    let wanted = slug(name);
    let mut by_name = Vec::new();
    let mut by_type = Vec::new();
    let mut known: Vec<String> = Vec::new();
    map.for_each_layer(|layer, _| {
        for object in layer
            .as_object_layer()
            .map_or(&[][..], |o| o.objects.as_slice())
        {
            let own = slug(&object.name);
            let kind = slug(object_type(map, object));
            if !own.is_empty() && own == wanted {
                by_name.push(object.id);
            }
            if !kind.is_empty() && kind == wanted {
                by_type.push(object.id);
            }
            known.extend([own, kind].into_iter().filter(|n| !n.is_empty()));
        }
    });
    if by_name.len() > 1 {
        return Err(Error::SeveralObjects {
            name: name.to_owned(),
            ids: by_name,
        });
    }
    if !by_name.is_empty() {
        return Ok(by_name);
    }
    match by_type.len() {
        0 => {
            let known: Vec<&str> = known.iter().map(String::as_str).collect();
            Err(Error::unknown(NameKind::Object, name, &known))
        }
        1 => Ok(by_type),
        _ => Err(Error::SeveralOfType {
            name: name.to_owned(),
            ids: by_type,
        }),
    }
}

/// A layer by `#ID` or name, of the kind wanted: of several of the name, the one of that kind.
fn find_layer(map: &Map, layer: &LayerRef, wanted: LayerWanted) -> Result<LayerId, Error> {
    let found: Vec<&Layer> = match layer {
        LayerRef::Id(id) => vec![
            map.layer(LayerId(*id))
                .ok_or(Error::NoSuchLayer { id: *id })?,
        ],
        LayerRef::Name(name) => {
            let key = slug(name);
            let mut all = Vec::new();
            let mut known = Vec::new();
            map.for_each_layer(|layer, _| {
                let own = slug(&layer.name);
                if own == key {
                    all.push(layer);
                }
                known.push(own);
            });
            if all.is_empty() {
                let known: Vec<&str> = known.iter().map(String::as_str).collect();
                return Err(Error::unknown(NameKind::Layer, name, &known));
            }
            all
        }
    };
    let of_kind: Vec<&Layer> = found
        .iter()
        .copied()
        .filter(|layer| match wanted {
            LayerWanted::Any => true,
            LayerWanted::Tiles => matches!(layer.kind, LayerKind::Tile(_)),
            LayerWanted::Objects => matches!(layer.kind, LayerKind::Object(_)),
        })
        .collect();
    match of_kind.as_slice() {
        [one] => Ok(one.id),
        [] => {
            let first = found.first().map_or("", |layer| layer.name.as_str());
            let is = found
                .first()
                .map_or("a layer", |layer| layer_kind_words(layer));
            Err(Error::WrongLayerKind {
                layer: first.to_owned(),
                is,
                wanted: match wanted {
                    LayerWanted::Tiles => "a tile layer",
                    LayerWanted::Objects => "an object layer",
                    LayerWanted::Any => "a layer",
                },
            })
        }
        several => Err(Error::SeveralLayers {
            name: layer_ref_text(layer),
            ids: several.iter().map(|layer| layer.id).collect(),
        }),
    }
}

/// "a tile layer", "an object layer", "an image layer", "a group".
fn layer_kind_words(layer: &Layer) -> &'static str {
    match layer.kind {
        LayerKind::Tile(_) => "a tile layer",
        LayerKind::Object(_) => "an object layer",
        LayerKind::Image(_) => "an image layer",
        LayerKind::Group(_) => "a group",
    }
}

/// A layer as written: its name, or `#ID`.
fn layer_ref_text(layer: &LayerRef) -> String {
    match layer {
        LayerRef::Name(name) => name.clone(),
        LayerRef::Id(id) => format!("#{id}"),
    }
}

/// `tileset:1-4,9`.
fn tile_ids_text(ids: &TileIds) -> String {
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

/// The ids a tileset has, in words: "its tiles are 0-31", "its tiles are 0-5, 7, 9-12".
fn range_text(tileset: &Tileset) -> String {
    let mut ids: Vec<u32> = tileset.tile_ids().into_iter().map(|id| id.0).collect();
    ids.sort_unstable();
    let mut ranges: Vec<String> = Vec::new();
    let mut run: Option<IdRun> = None;
    for id in ids {
        run = match run {
            Some(open) if open.last + 1 == id => Some(IdRun {
                first: open.first,
                last: id,
            }),
            Some(open) => {
                ranges.push(open.text());
                Some(IdRun {
                    first: id,
                    last: id,
                })
            }
            None => Some(IdRun {
                first: id,
                last: id,
            }),
        };
    }
    ranges.extend(run.map(|open| open.text()));
    if ranges.is_empty() {
        "it has no tiles".to_owned()
    } else {
        format!("its tiles are {}", capped_list(&ranges, RANGES_SHOWN))
    }
}

/// Consecutive tile ids.
#[derive(Clone, Copy)]
struct IdRun {
    first: u32,
    last: u32,
}

impl IdRun {
    fn text(self) -> String {
        if self.first == self.last {
            self.first.to_string()
        } else {
            format!("{}-{}", self.first, self.last)
        }
    }
}

/// An ambiguous terrain name, with the qualified names it could be.
fn ambiguous(query: &str, terrains: &[&NamedTerrain]) -> Error {
    Error::Ambiguous {
        query: query.to_owned(),
        options: terrains.iter().map(|t| t.name.clone()).collect(),
    }
}

/// Makes names unique: the first keeps its name, later ones get `-2`, `-3`, skipping names
/// that are taken.
fn uniquify(names: &mut [String]) {
    let taken: std::collections::HashSet<String> = names.iter().cloned().collect();
    let mut used = std::collections::HashSet::new();
    for name in names.iter_mut() {
        if used.insert(name.clone()) {
            continue;
        }
        let mut number = 2;
        loop {
            let candidate = format!("{name}-{number}");
            if !taken.contains(&candidate) && !used.contains(&candidate) {
                used.insert(candidate.clone());
                *name = candidate;
                break;
            }
            number += 1;
        }
    }
}

/// The tilesets' names, by slot: their slugs, `tileset` for one without a name, made unique.
fn tileset_names(map: &Map) -> Vec<String> {
    let mut names: Vec<String> = map
        .tilesets
        .iter()
        .map(|tileset| match slug(&tileset.tileset.name) {
            name if name.is_empty() => "tileset".to_owned(),
            name => name,
        })
        .collect();
    uniquify(&mut names);
    names
}

/// A terrain before its name is settled: its name at each level of qualification.
struct TerrainDraft {
    terrain: NamedTerrain,
    levels: [String; 3],
    level: usize,
}

/// The terrains: a colour's name, the set's name when the set has one colour, else `set-n`;
/// names that collide are qualified `set/name`, then `tileset/set/name`, and what still
/// collides is numbered.
fn terrain_names(map: &Map, tilesets: &[String]) -> Vec<NamedTerrain> {
    let mut drafts: Vec<TerrainDraft> = Vec::new();
    for (index, used) in map.tilesets.iter().enumerate() {
        let Some(slot) = TilesetSlot::new(index) else {
            continue;
        };
        let tileset_name = tilesets.get(index).map_or("tileset", String::as_str);
        for (set_index, set) in used.tileset.wang_sets.iter().enumerate() {
            let set_name = match slug(&set.name) {
                name if name.is_empty() => "set".to_owned(),
                name => name,
            };
            for (color_index, color) in set.colors.iter().enumerate() {
                let Ok(color_number) = u8::try_from(color_index + 1) else {
                    continue;
                };
                let own = match slug(&color.name) {
                    name if !name.is_empty() => name,
                    _ if set.colors.len() == 1 => set_name.clone(),
                    _ => format!("{set_name}-{color_number}"),
                };
                let in_set = format!("{set_name}/{own}");
                let in_tileset = format!("{tileset_name}/{in_set}");
                drafts.push(TerrainDraft {
                    terrain: NamedTerrain {
                        name: String::new(),
                        slot,
                        set: set_index,
                        color: TerrainColor(color_number),
                        kind: set.kind,
                    },
                    levels: [own, in_set, in_tileset],
                    level: 0,
                });
            }
        }
    }
    for level in 1..=2 {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for draft in &drafts {
            if let Some(name) = draft.levels.get(draft.level) {
                *counts.entry(name.clone()).or_default() += 1;
            }
        }
        for draft in &mut drafts {
            let current = draft.levels.get(draft.level);
            if current
                .and_then(|name| counts.get(name))
                .is_some_and(|&n| n > 1)
            {
                draft.level = level;
            }
        }
    }
    let mut names: Vec<String> = drafts
        .iter()
        .map(|draft| draft.levels.get(draft.level).cloned().unwrap_or_default())
        .collect();
    uniquify(&mut names);
    drafts
        .into_iter()
        .zip(names)
        .map(|(draft, name)| NamedTerrain {
            name,
            ..draft.terrain
        })
        .collect()
}

/// The sprites: the tiles of image collections with an image, less the tiles that are only
/// frames of another tile's animation, named after their image file's stem.
fn sprite_names(map: &Map, tilesets: &[String]) -> Vec<Sprite> {
    let mut sprites = Vec::new();
    for (index, used) in map.tilesets.iter().enumerate() {
        let tileset = &used.tileset;
        let Some(slot) = TilesetSlot::new(index) else {
            continue;
        };
        if !tileset.is_collection() {
            continue;
        }
        let tileset_name = tilesets.get(index).map_or("tileset", String::as_str);
        let frames: std::collections::HashSet<TileId> = tileset
            .tiles
            .iter()
            .flat_map(|(&owner, tile)| {
                tile.animation
                    .iter()
                    .map(|frame| frame.tile)
                    .filter(move |&frame| frame != owner)
            })
            .filter(|frame| tileset.tile(*frame).is_none_or(|t| t.animation.is_empty()))
            .collect();
        for id in tileset.tile_ids() {
            let Some(tile) = tileset.tile(id) else {
                continue;
            };
            let Some(image) = &tile.image else {
                continue;
            };
            if frames.contains(&id) {
                continue;
            }
            let stem = match &image.source {
                ImageSource::File(file) if !file.source.is_empty() => slug(file_stem(&file.source)),
                ImageSource::File(file) => slug(file_stem(&file.path.to_string_lossy())),
                ImageSource::Embedded(_) => String::new(),
            };
            let name = if stem.is_empty() {
                format!("{tileset_name}-{id}")
            } else {
                stem
            };
            sprites.push(Sprite {
                name,
                tile: Cell::new(slot, id),
                size: tileset.tile_pixel_size(id),
                animated: !tile.animation.is_empty(),
            });
        }
    }
    let mut names: Vec<String> = sprites.iter().map(|s| s.name.clone()).collect();
    uniquify(&mut names);
    for (sprite, name) in sprites.iter_mut().zip(names) {
        sprite.name = name;
    }
    sprites
}

/// The tiles of each class, named after it; one class in several tilesets is one name.
fn class_groups(map: &Map) -> Vec<TileGroup> {
    let mut groups: Vec<TileGroup> = Vec::new();
    for (index, used) in map.tilesets.iter().enumerate() {
        let Some(slot) = TilesetSlot::new(index) else {
            continue;
        };
        for (&id, tile) in &used.tileset.tiles {
            let name = slug(&tile.class);
            if name.is_empty() {
                continue;
            }
            let cell = Cell::new(slot, id);
            match groups.iter_mut().find(|g| g.name == name) {
                Some(group) => group.tiles.push(cell),
                None => groups.push(TileGroup {
                    name,
                    tiles: vec![cell],
                    layer: None,
                }),
            }
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use bt_core::{
        FileRef, Frame, ImageRef, MapObject, Shape, TileData, TilesetImage, WangSetType,
    };
    use bt_test_data::fixtures::{Atlas, MapBuilder, TerrainSetBuilder};

    use super::*;
    use crate::script::IdRange;

    fn slot(index: usize) -> TilesetSlot {
        TilesetSlot::new(index).unwrap()
    }

    fn named(name: &str) -> Named {
        Named::Name(name.to_owned())
    }

    fn ids(tileset: &str, ranges: &[[u32; 2]]) -> Named {
        Named::Tiles(TileIds {
            tileset: tileset.to_owned(),
            ranges: ranges
                .iter()
                .map(|r| IdRange {
                    first: r[0],
                    last: r[1],
                })
                .collect(),
        })
    }

    /// An atlas named `name` with these terrain sets.
    fn atlas_with(name: &str, sets: Vec<TerrainSetBuilder>) -> Arc<Tileset> {
        let atlas = Atlas::builder().name(name).build_embedded().unwrap();
        let mut tileset = Tileset::clone(&atlas.tileset);
        for set in sets {
            tileset = set.added_to(tileset);
        }
        Arc::new(tileset)
    }

    /// A collection tileset whose tiles are images at these paths, some animated.
    fn collection(name: &str, images: &[&str], animations: &[(u32, &[u32])]) -> Arc<Tileset> {
        let mut tiles = BTreeMap::new();
        for (id, path) in (0u32..).zip(images) {
            let image = ImageRef::file(
                FileRef::new(*path, format!("/art/{path}")),
                UVec2::new(32, 48),
            );
            let animation = animations
                .iter()
                .find(|(owner, _)| *owner == id)
                .map(|(_, frames)| {
                    frames
                        .iter()
                        .map(|&tile| Frame {
                            tile: TileId(tile),
                            duration_ms: 100,
                        })
                        .collect()
                })
                .unwrap_or_default();
            tiles.insert(
                TileId(id),
                TileData {
                    image: Some(image),
                    animation,
                    ..TileData::default()
                },
            );
        }
        Arc::new(Tileset {
            name: name.to_owned(),
            tile_width: 32,
            tile_height: 48,
            image: TilesetImage::Collection,
            next_tile_id: u32::try_from(images.len()).unwrap(),
            tiles,
            ..Tileset::default()
        })
    }

    fn corner_set(name: &str, terrains: &[&str]) -> TerrainSetBuilder {
        TerrainSetBuilder::new(WangSetType::Corner)
            .name(name)
            .terrains(terrains)
    }

    fn map_with(tilesets: Vec<Arc<Tileset>>) -> Map {
        let mut builder = MapBuilder::new(UVec2::new(16, 16), UVec2::new(8, 6));
        for tileset in tilesets {
            builder = builder.tileset(tileset);
        }
        builder.build().unwrap()
    }

    #[test]
    fn terrains_are_named_by_colour_then_qualified_while_they_collide() {
        let map = map_with(vec![
            atlas_with(
                "Outdoor",
                vec![
                    corner_set("Land", &["Grass", "Water"]),
                    corner_set("Sea", &["Water", "Sand"]),
                    corner_set("Mud", &["", ""]),
                    corner_set("Lava", &[""]),
                ],
            ),
            atlas_with("Outdoor", vec![corner_set("Land", &["Water"])]),
        ]);
        let names = Names::of(&map);
        let all: Vec<&str> = names.terrains().iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            all,
            [
                "grass",
                "outdoor/land/water",
                "sea/water",
                "sand",
                "mud-1",
                "mud-2",
                "lava",
                "outdoor-2/land/water"
            ]
        );
        assert_eq!(names.tileset_name(slot(1)), Some("outdoor-2"));
        // Case and separators never matter; a qualified name is reached by its last parts
        // when they are of exactly one name.
        assert_eq!(names.terrain("Sea / Water").unwrap().name, "sea/water");
        assert_eq!(names.terrain("SAND").unwrap().color, TerrainColor(2));
        let ambiguous = names.terrain("water").unwrap_err();
        assert_eq!(
            ambiguous.to_string(),
            "“water” is ambiguous: outdoor/land/water, sea/water, outdoor-2/land/water"
        );
        assert!(matches!(
            names.terrain("land/water"),
            Err(Error::Ambiguous { .. })
        ));
        let outdoor2 = names.terrain("Outdoor-2/Land/Water").unwrap();
        assert_eq!((outdoor2.slot, outdoor2.set), (slot(1), 0));
        assert_eq!(
            names
                .terrain_of(slot(0), 1, TerrainColor(1))
                .map(|t| t.name.as_str()),
            Some("sea/water")
        );
        let unknown = names.terrain("gras").unwrap_err();
        assert!(
            unknown
                .to_string()
                .starts_with("unknown terrain “gras”: did you mean grass?"),
            "{unknown}"
        );
    }

    #[test]
    fn terrains_that_collide_in_one_tileset_are_numbered() {
        let map = map_with(vec![atlas_with(
            "Ground",
            vec![
                corner_set("Land", &["Water"]),
                corner_set("Land", &["Water"]),
            ],
        )]);
        let names = Names::of(&map);
        let all: Vec<&str> = names.terrains().iter().map(|t| t.name.as_str()).collect();
        assert_eq!(all, ["ground/land/water", "ground/land/water-2"]);
        assert_eq!(names.terrain("ground/land/water-2").unwrap().set, 1);
    }

    #[test]
    fn sprites_are_collection_tiles_named_after_their_images_without_animation_frames() {
        let props = collection(
            "Props",
            &[
                "trees/Pine Tree.png",
                "rock.png",
                "fire-1.png",
                "fire-2.png",
                "fire-3.png",
            ],
            &[(2, &[2, 3, 4])],
        );
        let more = collection("More Props", &["rock.png"], &[]);
        let atlas = atlas_with("ground", Vec::new());
        let map = map_with(vec![atlas, props, more]);
        let names = Names::of(&map);
        let all: Vec<&str> = names.sprites().iter().map(|s| s.name.as_str()).collect();
        assert_eq!(all, ["pine-tree", "rock", "fire-1", "rock-2"]);
        let fire = names.sprite("Fire 1").unwrap();
        assert!(fire.animated);
        assert_eq!(fire.size, UVec2::new(32, 48));
        assert_eq!(names.sprite("props:1").unwrap().name, "rock");
        assert_eq!(names.sprite("more-props:0").unwrap().name, "rock-2");
        assert_eq!(
            names.sprite("props:3").unwrap_err().to_string(),
            "props:3 is not a sprite: sprites are the tiles of image-collection tilesets, less the frames of animations"
        );
        assert!(
            names.sprite("ground:1").is_err(),
            "atlas tiles are not sprites"
        );
        assert_eq!(names.tile_text(Cell::new(slot(1), TileId(0))), "pine-tree");
        assert_eq!(names.tile_text(Cell::new(slot(0), TileId(5))), "ground:5");
        assert_eq!(names.tile_text(Cell::EMPTY), "nothing");
        // A sprite placed alone is a group of one.
        let group = names.group("rock-2").unwrap();
        assert_eq!(group.len(), 1);
        assert_eq!(group[0].offset, DVec2::ZERO);
        let unknown = names.sprite("pine").unwrap_err();
        assert!(
            unknown.to_string().contains("did you mean pine-tree?"),
            "{unknown}"
        );
    }

    #[test]
    fn tiles_go_by_id_with_the_tilesets_range_when_one_is_missing() {
        let ground = atlas_with("Ground", Vec::new());
        let props = collection("props", &["a.png", "b.png"], &[]);
        let mut gaps = Tileset::clone(&props);
        gaps.tiles.insert(
            TileId(5),
            TileData {
                image: Some(ImageRef::file(FileRef::new("c.png", "/c.png"), UVec2::ONE)),
                ..TileData::default()
            },
        );
        gaps.name = "gaps".into();
        let map = map_with(vec![ground, props, Arc::new(gaps)]);
        let names = Names::of(&map);
        let tiles = names
            .tiles(&map, &[ids("ground", &[[3, 5], [9, 9]])])
            .unwrap();
        assert_eq!(tiles.name, "ground:3-5,9");
        let found: Vec<u32> = tiles.tiles.iter().map(|t| t.tile().0).collect();
        assert_eq!(found, [3, 4, 5, 9]);
        assert_eq!(
            names
                .tiles(&map, &[ids("ground", &[[99, 99]])])
                .unwrap_err()
                .to_string(),
            "ground has no tile 99 (its tiles are 0-15)"
        );
        assert_eq!(
            names
                .tiles(&map, &[ids("gaps", &[[2, 2]])])
                .unwrap_err()
                .to_string(),
            "gaps has no tile 2 (its tiles are 0-1, 5)"
        );
        assert!(matches!(
            names.tiles(&map, &[ids("ground", &[[5, 3]])]),
            Err(Error::BackwardRange { .. })
        ));
        assert!(matches!(
            names.tiles(&map, &[ids("grass", &[[1, 1]])]),
            Err(Error::Unknown {
                kind: NameKind::Tileset,
                ..
            })
        ));
        // A sprite's name is its tile.
        let sprite = names.tiles(&map, &[named("b")]).unwrap();
        assert_eq!(sprite.tiles, [Cell::new(slot(1), TileId(1))]);
    }

    #[test]
    fn grounds_are_terrains_then_tile_groups_then_ids_then_areas() {
        let mut ground = Tileset::clone(&atlas_with(
            "ground",
            vec![corner_set("Land", &["Grass", "Water"])],
        ));
        ground.tiles.insert(
            TileId(15),
            TileData {
                class: "Stone".into(),
                ..TileData::default()
            },
        );
        let map = map_with(vec![Arc::new(ground)]);
        let mut names = Names::of(&map);
        names
            .define_area("pond", [IVec2::new(1, 1), IVec2::new(2, 1)])
            .unwrap();
        names
            .define_tiles(
                "pebbles",
                vec![Cell::new(slot(0), TileId(7))],
                Some(LayerRef::Name("ground".into())),
            )
            .unwrap();
        let grounds = names
            .grounds(
                &map,
                &[
                    named("water"),
                    named("stone"),
                    named("pebbles"),
                    ids("ground", &[[1, 2]]),
                    named("pond"),
                ],
            )
            .unwrap();
        assert!(matches!(&grounds[0], Ground::Terrain(t) if t.name == "water"));
        assert!(matches!(&grounds[1], Ground::Tiles(g) if g.name == "stone" && g.tiles.len() == 1));
        assert!(
            matches!(&grounds[2], Ground::Tiles(g) if g.layer == Some(LayerRef::Name("ground".into())))
        );
        assert!(matches!(&grounds[3], Ground::Tiles(g) if g.name == "ground:1-2"));
        assert!(matches!(&grounds[4], Ground::Area { cells, .. } if cells.len() == 2));
        // A tile name where a terrain is required.
        assert_eq!(
            names.material("stone").unwrap_err().to_string(),
            "stone is tiles, not a terrain: place tiles with `tile stone SHAPE`"
        );
        assert_eq!(
            names.material("Water").unwrap(),
            vec![MaterialPart {
                terrain: names.terrain("water").unwrap().clone(),
                layer: None
            }]
        );
        let unknown = names.grounds(&map, &[named("pebble")]).unwrap_err();
        assert!(
            unknown
                .to_string()
                .starts_with("unknown terrain or tile “pebble”: did you mean pebbles?"),
            "{unknown}"
        );
        // The map's names are not given to definitions.
        assert_eq!(
            names.define_area("Grass", []).unwrap_err().to_string(),
            "“grass” is already a terrain of the map: give the definition another name"
        );
        assert!(matches!(
            names.define_tiles("stone", Vec::new(), None),
            Err(Error::NameTaken {
                by: NameKind::Tiles,
                ..
            })
        ));
    }

    #[test]
    fn definitions_are_kept_when_the_tables_are_rebuilt_and_one_name_means_one_thing() {
        let mut map = map_with(vec![atlas_with(
            "ground",
            vec![corner_set("Land", &["Grass", "Water"])],
        )]);
        let mut names = Names::of(&map);
        let water = names.terrain("water").unwrap().clone();
        let parts = vec![
            MaterialPart {
                terrain: water.clone(),
                layer: Some(LayerRef::Name("ground".into())),
            },
            MaterialPart {
                terrain: names.terrain("grass").unwrap().clone(),
                layer: Some(LayerRef::Name("shore".into())),
            },
        ];
        names.define_material("wet", parts.clone()).unwrap();
        assert_eq!(names.material("WET").unwrap(), parts);
        // The script adds a tileset of sprites.
        map.tilesets.push(bt_core::TilesetRef::embedded(collection(
            "trees",
            &["oak.png"],
            &[],
        )));
        names.rebuild_tables(&map);
        assert_eq!(names.material("wet").unwrap(), parts);
        assert_eq!(
            names.sprite("oak").unwrap().tile,
            Cell::new(slot(1), TileId(0))
        );
        // Defining the name again as something else replaces it.
        names.define_area("wet", [IVec2::ZERO]).unwrap();
        assert_eq!(
            names.material("wet").unwrap_err().to_string(),
            "wet is an area, not a terrain: use it as a shape, `where wet`"
        );
        assert_eq!(
            names.group("water").unwrap_err().to_string(),
            "water is a terrain, not a sprite: paint it with `paint water SHAPE`"
        );
        let piece = Piece {
            tiles: TileClip::default(),
            objects: Vec::new(),
            foot: IVec2::new(1, 2),
        };
        names.define_piece("hut", piece.clone()).unwrap();
        assert_eq!(names.piece("Hut").as_deref(), Some(&piece));
        assert!(names.piece("wet").is_none());
    }

    #[test]
    fn layers_of_one_name_are_told_apart_by_id_or_by_kind() {
        let atlas = atlas_with("ground", Vec::new());
        let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(4, 4))
            .tileset(atlas)
            .tile_layer("Ground", "")
            .object_layer("ground", Vec::new())
            .tile_layer("walls", "")
            .tile_layer("walls", "")
            .build()
            .unwrap();
        let names = Names::of(&map);
        let ground = LayerRef::Name("ground".into());
        assert_eq!(names.tile_layer(&map, &ground), Ok(LayerId(1)));
        assert_eq!(names.object_layer(&map, &ground), Ok(LayerId(2)));
        assert_eq!(
            names.layer(&map, &ground).unwrap_err().to_string(),
            "several layers are called “ground”: name one by id (#1, #2)"
        );
        assert_eq!(
            names
                .tile_layer(&map, &LayerRef::Name("walls".into()))
                .unwrap_err()
                .to_string(),
            "several layers are called “walls”: name one by id (#3, #4)"
        );
        assert_eq!(names.tile_layer(&map, &LayerRef::Id(4)), Ok(LayerId(4)));
        assert_eq!(
            names
                .object_layer(&map, &LayerRef::Id(4))
                .unwrap_err()
                .to_string(),
            "walls is a tile layer, not an object layer"
        );
        assert_eq!(
            names.layer(&map, &LayerRef::Id(9)).unwrap_err().to_string(),
            "the map has no layer #9 (bt info MAP lists the layers)"
        );
        assert!(
            names
                .layer(&map, &LayerRef::Name("wals".into()))
                .unwrap_err()
                .to_string()
                .starts_with("unknown layer “wals”: did you mean walls?")
        );
    }

    #[test]
    fn objects_go_by_id_name_type_selection_and_last() {
        let object = |name: &str, class: &str| MapObject {
            name: name.into(),
            class: class.into(),
            shape: Shape::Point,
            ..MapObject::default()
        };
        let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(4, 4))
            .object_layer(
                "things",
                vec![
                    object("Entry", "door"),
                    object("", "spawn"),
                    object("", "spawn"),
                    object("chest", ""),
                    object("chest", ""),
                ],
            )
            .build()
            .unwrap();
        let names = Names::of(&map);
        let none = Context::new(1);
        let find = |name: &str| names.objects(&map, &ObjectRef::Name(name.into()), &none);
        assert_eq!(find("entry"), Ok(vec![ObjectId(1)]));
        assert_eq!(find("door"), Ok(vec![ObjectId(1)]), "by its type");
        assert_eq!(
            find("spawn").unwrap_err().to_string(),
            "there are 2 spawn objects: name one by id (#2, #3)"
        );
        assert_eq!(
            find("chest").unwrap_err().to_string(),
            "several objects are called “chest”: name one by id (#4, #5)"
        );
        assert!(
            find("dor")
                .unwrap_err()
                .to_string()
                .starts_with("unknown object “dor”: did you mean door?")
        );
        assert_eq!(
            names.objects(&map, &ObjectRef::Id(9), &none),
            Err(Error::NoSuchObject { id: 9 })
        );
        assert_eq!(
            names.objects(&map, &ObjectRef::Selected, &none),
            Err(Error::NotInEditor { word: "selected" })
        );
        let open = Context::new(1)
            .with_selection(crate::context::Selection {
                cells: Vec::new(),
                objects: vec![ObjectId(2), ObjectId(3)],
            })
            .with_last(vec![ObjectId(4)]);
        assert_eq!(
            names.objects(&map, &ObjectRef::Selected, &open),
            Ok(vec![ObjectId(2), ObjectId(3)])
        );
        assert_eq!(
            names.objects(&map, &ObjectRef::Last, &open),
            Ok(vec![ObjectId(4)])
        );
    }

    #[test]
    fn tileds_examples_name_their_terrains_and_sprites() {
        let Some(examples) = bt_test_data::test_data("reference/tiled/examples") else {
            return;
        };
        let mut loader = bt_tmx::Loader::default();
        let desert = loader.load_map(&examples.join("desert.tmx")).unwrap().value;
        let names = Names::of(&desert);
        let terrains: Vec<&str> = names.terrains().iter().map(|t| t.name.as_str()).collect();
        assert_eq!(terrains, ["desert", "brick", "cobblestone", "dirt"]);
        let sandbox = loader
            .load_map(&examples.join("sticker-knight/map/sandbox.tmx"))
            .unwrap()
            .value;
        let names = Names::of(&sandbox);
        assert_eq!(names.sprites().len(), 62, "every image of objs.tsx");
        assert_eq!(names.sprite("blobBlue").unwrap().size, UVec2::new(96, 64));
        assert_eq!(names.sprite("Hero").unwrap().name, "hero");
        assert_eq!(names.sprite("objs:0").unwrap().name, "alter");
    }

    #[test]
    fn tiles_say_what_terrain_they_show() {
        let set = corner_set("Land", &["Grass", "Water"]).every_combination();
        let map = map_with(vec![atlas_with("ground", vec![set])]);
        let names = Names::of(&map);
        let tile = |id: u32| Cell::new(slot(0), TileId(id));
        // Tile 15 is all water, tile 14 has grass at one corner, tile 3 two of each.
        assert_eq!(names.tile_label(&map, tile(15)), "water");
        assert_eq!(names.tile_label(&map, tile(14)), "grass/water edges");
        assert_eq!(
            names
                .terrain_covering(&map, tile(14))
                .map(|t| t.name.as_str()),
            Some("water")
        );
        assert_eq!(names.terrain_covering(&map, tile(3)), None);
    }
}
