//! `bt catalog` (`10-agents.md`, §10.4): what the map can be built with, by name. Its terrains
//! with their set's kind, the terrains each has tiles with ("borders", `nothing` when it has
//! edge tiles), whether its edge tiles are opaque and where it is used; its sprites
//! ([`crate::catalog_sprites`]); its tiles by id with where each is used ([`crate::catalog_tiles`]);
//! and the object types with their counts and properties. `--find` narrows every list;
//! `--sheet` draws the listed items numbered, or every tile with its id.
//!
//! Writing the sheet is the caller's: the outcome comes with its pixels.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use bt_core::{Cell, ClassUsage, CustomTypes, Map, TileId, TilesetSlot, WangSetType};
use bt_image::DecodedImage;
use bt_terrain::TerrainColor;
use bt_utils::text::{plural, slug};
use glam::UVec2;
use serde::{Deserialize, Serialize};

use crate::catalog_sprites::{SpriteEntry, sprite_list};
use crate::catalog_tiles::{TileDetails, TilesetTiles, tile_neighbours, tile_uses, tileset_tiles};
use crate::error::Error;
use crate::names::{Names, object_type};
use crate::opacity::{EdgeMeter, EdgeOpacity};
use crate::property_text::{assignments_text, properties_text};
use crate::report::{Line, Outcome, Report, Section};
use crate::sheet::{SheetItem, SheetTileset, draw_items, draw_tiles};
use crate::symbols::TileKey;
use crate::tally::{NameCount, Tally, counts_text};
use crate::tile_pictures::TilePictures;
use crate::words::cells_text;

/// The most tiles `tiles TILESET:ID,…` tells about.
const MOST_DETAILED: usize = 12;

/// Unused sprites listed one a line, numbered, when there are no more than this.
const UNUSED_LISTED: usize = 60;

/// The tiles of a tileset listed by layer before the catalog says how to see the rest.
const TILE_LAYERS_SHOWN: usize = 40;

/// What `bt catalog` lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogPart {
    /// Terrains.
    Terrains,
    /// Sprites.
    Sprites,
    /// Tiles by id.
    Tiles,
    /// Object types.
    Types,
}

/// What `bt catalog` is asked.
pub struct CatalogRequest<'a> {
    map: &'a Map,
    names: &'a Names,
    part: Option<CatalogPart>,
    tiles: Option<String>,
    find: Option<String>,
    sheet: Option<SheetAsked>,
    custom_types: Arc<CustomTypes>,
}

/// A sheet asked for.
struct SheetAsked {
    file: String,
    scale: Option<u32>,
}

impl<'a> CatalogRequest<'a> {
    /// Terrains, sprites and object types of `map`, with a line on its tiles.
    pub fn new(map: &'a Map, names: &'a Names) -> CatalogRequest<'a> {
        CatalogRequest {
            map,
            names,
            part: None,
            tiles: None,
            find: None,
            sheet: None,
            custom_types: Arc::new(CustomTypes::default()),
        }
    }

    /// Only this part.
    pub fn part(mut self, part: CatalogPart) -> CatalogRequest<'a> {
        self.part = Some(part);
        self
    }

    /// With `tiles`: one tileset by name, or up to twelve tiles as `TILESET:ID,…`.
    pub fn tiles_of(mut self, written: impl Into<String>) -> CatalogRequest<'a> {
        self.part = Some(CatalogPart::Tiles);
        self.tiles = Some(written.into());
        self
    }

    /// Only items whose names hold `text`.
    pub fn find(mut self, text: impl Into<String>) -> CatalogRequest<'a> {
        self.find = Some(text.into());
        self
    }

    /// A picture of the listed items, for the file `file`, `scale` times larger for tiles.
    pub fn sheet(mut self, file: impl Into<String>, scale: Option<u32>) -> CatalogRequest<'a> {
        self.sheet = Some(SheetAsked {
            file: file.into(),
            scale,
        });
        self
    }

    /// The project's custom types: its object classes are types too.
    pub fn custom_types(mut self, types: Arc<CustomTypes>) -> CatalogRequest<'a> {
        self.custom_types = types;
        self
    }
}

/// `bt catalog MAP`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CatalogOutcome {
    /// What `--find` looked for.
    pub find: Option<String>,
    /// The terrains, when listed.
    pub terrains: Option<Vec<TerrainEntry>>,
    /// The sprites, when listed: those used first.
    pub sprites: Option<Vec<SpriteEntry>>,
    /// Sprites whose feet can't be measured, left out of the pairs.
    pub sprites_unmeasured: usize,
    /// The tiles by tileset, when listed.
    pub tiles: Option<Vec<TilesetTiles>>,
    /// The tiles named by id, when they are.
    pub tile_details: Option<TileDetails>,
    /// How many tiles the map has and uses, when the tiles are not listed.
    pub tiles_summary: Option<TilesSummary>,
    /// The object types, when listed.
    pub types: Option<Vec<TypeEntry>>,
    /// Whether `--find` found nothing.
    pub nothing_found: bool,
    /// The sheet drawn, when one was asked for.
    pub sheet: Option<SheetInfo>,
}

/// A terrain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerrainEntry {
    /// Its number on the sheet, when it has a tile to show it by.
    pub number: Option<usize>,
    /// Its name.
    pub name: String,
    /// Its set's kind: `corner`, `edge` or `mixed`.
    pub kind: String,
    /// Its tileset.
    pub tileset: String,
    /// How many tiles its set labels.
    pub tiles: usize,
    /// The terrains it has tiles with, and `nothing` when it has edge tiles.
    pub borders: Vec<String>,
    /// The terrains of its set it has no tile with.
    pub apart: Vec<String>,
    /// What its edge tiles are.
    pub edges: EdgeOpacity,
    /// Where it is used: cells it covers more than half of, by layer.
    pub used: Vec<NameCount>,
}

/// An object type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeEntry {
    /// Its name.
    pub name: String,
    /// Whether the project defines it as a class.
    pub project: bool,
    /// How many objects of the map have it.
    pub count: usize,
    /// Its properties: the class's members with their defaults, or what the objects carry.
    pub properties: BTreeMap<String, String>,
}

/// How many tiles there are and are used, when they are not listed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TilesSummary {
    /// Tilesets of tiles (not sprites).
    pub tilesets: usize,
    /// Their tiles.
    pub tiles: usize,
    /// The tiles the map uses.
    pub used: usize,
}

/// A sheet drawn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SheetInfo {
    /// The file it is for.
    pub file: String,
    /// Its size in pixels.
    pub size: UVec2,
    /// Whether it shows tiles by id (else the numbered items of the list).
    pub tiles: bool,
}

/// A catalog and its sheet's pixels, to write.
#[derive(Clone, Debug)]
pub struct Cataloged {
    /// What `bt catalog` reports.
    pub outcome: CatalogOutcome,
    /// The sheet, when one was asked for.
    pub sheet: Option<DecodedImage>,
}

impl CatalogOutcome {
    /// The catalog `request` asks for, with its sheet drawn when asked.
    pub fn from_request(request: &CatalogRequest<'_>) -> Result<Cataloged, Error> {
        let map = request.map;
        let names = request.names;
        let find = request.find.as_deref().map(slug);
        let find = find.as_deref();
        let wants = |part: CatalogPart| match request.part {
            Some(asked) => asked == part,
            None => part != CatalogPart::Tiles,
        };
        if let Some(sheet) = &request.sheet
            && let Some(scale) = sheet.scale.filter(|s| !(1..=8).contains(s))
        {
            return Err(Error::SheetScale { scale });
        }
        let mut pictures = TilePictures::new();
        let mut items: Vec<SheetItem> = Vec::new();
        let terrains = wants(CatalogPart::Terrains).then(|| terrain_entries(map, names, find));
        let mut terrains = terrains;
        for entry in terrains.iter_mut().flatten() {
            if let Some(tile) = terrain_tile(map, names, &entry.name) {
                entry.number = Some(items.len() + 1);
                items.push(SheetItem {
                    number: items.len() + 1,
                    tile,
                });
            }
        }
        let mut sprites_unmeasured = 0;
        let sprites = wants(CatalogPart::Sprites).then(|| {
            let list = sprite_list(map, names, find);
            sprites_unmeasured = list.unmeasured;
            list.entries
        });
        let mut sprites = sprites;
        for entry in sprites.iter_mut().flatten() {
            if let Ok(sprite) = names.sprite(&entry.name) {
                entry.number = Some(items.len() + 1);
                items.push(SheetItem {
                    number: items.len() + 1,
                    tile: sprite.tile,
                });
            }
        }
        let uses = tile_uses(map);
        let mut sheet_tilesets: Vec<SheetTileset> = Vec::new();
        let mut tiles = None;
        let mut tile_details = None;
        if wants(CatalogPart::Tiles) {
            match request.tiles.as_deref() {
                Some(written) if written.contains(':') => {
                    let picked = named_tiles(map, names, written)?;
                    if picked.len() > MOST_DETAILED {
                        return Err(Error::TooManyTiles {
                            count: picked.len(),
                            most: MOST_DETAILED,
                        });
                    }
                    for tile in &picked {
                        let Some(slot) = tile.slot() else {
                            continue;
                        };
                        match sheet_tilesets.iter_mut().find(|s| s.slot == slot) {
                            Some(sheet) => sheet.only.push(tile.tile()),
                            None => sheet_tilesets.push(SheetTileset {
                                slot,
                                only: vec![tile.tile()],
                            }),
                        }
                    }
                    tile_details = Some(tile_neighbours(map, names, &picked));
                }
                Some(tileset) => {
                    let slot = names.tileset(tileset)?;
                    sheet_tilesets.push(SheetTileset {
                        slot,
                        only: Vec::new(),
                    });
                    tiles = Some(
                        tileset_tiles(map, names, slot, &uses, &mut pictures)
                            .into_iter()
                            .collect(),
                    );
                }
                None => {
                    let slots = tile_tilesets(map, &uses);
                    let listed: Vec<TilesetTiles> = slots
                        .iter()
                        .filter_map(|&slot| tileset_tiles(map, names, slot, &uses, &mut pictures))
                        .collect();
                    sheet_tilesets.extend(slots.into_iter().map(|slot| SheetTileset {
                        slot,
                        only: Vec::new(),
                    }));
                    tiles = Some(listed);
                }
            }
        }
        let summarised = tile_tilesets(map, &uses);
        let tiles_summary =
            (request.part.is_none() && !summarised.is_empty()).then(|| TilesSummary {
                tilesets: summarised.len(),
                tiles: summarised
                    .iter()
                    .filter_map(|&slot| map.tileset(slot))
                    .map(|t| t.tile_ids().len())
                    .sum(),
                used: uses
                    .keys()
                    .filter(|key| summarised.contains(&key.slot))
                    .count(),
            });
        let types =
            wants(CatalogPart::Types).then(|| type_entries(map, &request.custom_types, find));
        let nothing_found = find.is_some_and(|f| {
            terrains.as_ref().is_none_or(Vec::is_empty)
                && sprites.as_ref().is_none_or(Vec::is_empty)
                && types.as_ref().is_none_or(Vec::is_empty)
                && !names.tile_classes().iter().any(|g| g.name.contains(f))
        });
        let mut sheet = None;
        let mut sheet_info = None;
        if let Some(asked) = &request.sheet {
            let used: HashSet<TileKey> = uses.keys().copied().collect();
            let of_tiles = wants(CatalogPart::Tiles) || items.is_empty();
            let image = if of_tiles {
                if sheet_tilesets.is_empty() {
                    sheet_tilesets = tile_tilesets(map, &uses)
                        .into_iter()
                        .map(|slot| SheetTileset {
                            slot,
                            only: Vec::new(),
                        })
                        .collect();
                }
                draw_tiles(map, &sheet_tilesets, &used, asked.scale, &mut pictures)?
            } else {
                draw_items(map, &items, &mut pictures)?
            };
            sheet_info = Some(SheetInfo {
                file: asked.file.clone(),
                size: UVec2::new(image.width, image.height),
                tiles: of_tiles,
            });
            sheet = Some(image);
        }
        Ok(Cataloged {
            outcome: CatalogOutcome {
                find: request.find.clone(),
                terrains,
                sprites,
                sprites_unmeasured,
                tiles,
                tile_details,
                tiles_summary,
                types,
                nothing_found,
                sheet: sheet_info,
            },
            sheet,
        })
    }
}

/// The tilesets whose tiles are tiles: atlases, and collections whose tiles a tile layer holds
/// (the others' tiles are sprites).
fn tile_tilesets(map: &Map, uses: &BTreeMap<TileKey, Tally>) -> Vec<TilesetSlot> {
    (0..map.tilesets.len())
        .filter_map(TilesetSlot::new)
        .filter(|&slot| {
            map.tileset(slot).is_some_and(|tileset| {
                !tileset.is_collection() || uses.keys().any(|key| key.slot == slot)
            })
        })
        .collect()
}

/// The tiles written `TILESET:ID,…`.
fn named_tiles(map: &Map, names: &Names, written: &str) -> Result<Vec<Cell>, Error> {
    let items: Vec<String> = written
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect();
    let named = crate::script::named_items(&items).map_err(|why| Error::NotTiles {
        written: written.to_owned(),
        why,
    })?;
    Ok(names.tiles(map, &named)?.tiles)
}

/// The terrains whose names hold `find`, with what they border and where they are used.
fn terrain_entries(map: &Map, names: &Names, find: Option<&str>) -> Vec<TerrainEntry> {
    let mut used: BTreeMap<String, Tally> = BTreeMap::new();
    map.for_each_layer(|layer, _| {
        if let Some(tiles) = layer.as_tile_layer() {
            for placed in tiles.cells() {
                if let Some(terrain) = names.terrain_covering(map, placed.cell) {
                    used.entry(terrain.name.clone())
                        .or_default()
                        .add_one(layer.name.clone());
                }
            }
        }
    });
    let mut meter = EdgeMeter::new();
    let mut entries = Vec::new();
    for terrain in names.terrains() {
        if find.is_some_and(|f| !terrain.name.contains(f)) {
            continue;
        }
        let Some(set) = map
            .tileset(terrain.slot)
            .and_then(|tileset| tileset.wang_sets.get(terrain.set))
        else {
            continue;
        };
        let lanes: Vec<bt_core::Lane> = bt_core::Lane::ALL
            .into_iter()
            .filter(|lane| set.kind.uses(*lane))
            .collect();
        let mut with: Vec<u8> = Vec::new();
        for label in set.tiles.values() {
            if lanes
                .iter()
                .any(|&lane| label.lane(lane) == terrain.color.0)
            {
                for &lane in &lanes {
                    let color = label.lane(lane);
                    if color != terrain.color.0 && !with.contains(&color) {
                        with.push(color);
                    }
                }
            }
        }
        with.sort_unstable();
        let name_of = |color: u8| {
            names
                .terrain_of(terrain.slot, terrain.set, TerrainColor(color))
                .map_or_else(|| format!("terrain {color}"), |t| t.name.clone())
        };
        let mut borders: Vec<String> = with
            .iter()
            .filter(|&&c| c != 0)
            .map(|&c| name_of(c))
            .collect();
        if with.contains(&0) {
            borders.push("nothing".to_owned());
        }
        let apart = (1..=u8::try_from(set.colors.len()).unwrap_or(u8::MAX))
            .filter(|&c| c != terrain.color.0 && !with.contains(&c))
            .map(name_of)
            .collect();
        entries.push(TerrainEntry {
            number: None,
            name: terrain.name.clone(),
            kind: match set.kind {
                WangSetType::Corner => "corner",
                WangSetType::Edge => "edge",
                WangSetType::Mixed => "mixed",
            }
            .to_owned(),
            tileset: names
                .tileset_name(terrain.slot)
                .unwrap_or("tileset")
                .to_owned(),
            tiles: set.tiles.len(),
            borders,
            apart,
            edges: meter.measure(map, terrain),
            used: used
                .get(&terrain.name)
                .map(Tally::sorted)
                .unwrap_or_default(),
        });
    }
    entries
}

/// The tile that shows a terrain on the sheet: its colour's, else its set's.
fn terrain_tile(map: &Map, names: &Names, name: &str) -> Option<Cell> {
    let terrain = names.terrain(name).ok()?;
    let set = map.tileset(terrain.slot)?.wang_sets.get(terrain.set)?;
    let color = set
        .colors
        .get(usize::from(terrain.color.0).checked_sub(1)?)?;
    let tile: TileId = color.tile.or(set.tile)?;
    Some(Cell::new(terrain.slot, tile))
}

/// The project's object classes and the types the map's objects have, whose names hold `find`.
fn type_entries(map: &Map, types: &CustomTypes, find: Option<&str>) -> Vec<TypeEntry> {
    let mut used: BTreeMap<String, TypeEntry> = BTreeMap::new();
    map.for_each_layer(|layer, _| {
        let Some(objects) = layer.as_object_layer() else {
            return;
        };
        for object in objects.objects.iter() {
            let kind = object_type(map, object);
            if kind.is_empty() {
                continue;
            }
            let entry = used.entry(kind.to_owned()).or_insert_with(|| TypeEntry {
                name: kind.to_owned(),
                project: false,
                count: 0,
                properties: BTreeMap::new(),
            });
            entry.count += 1;
            for (key, value) in properties_text(&object.properties) {
                entry.properties.entry(key).or_insert(value);
            }
        }
    });
    let mut entries = Vec::new();
    for custom in &types.types {
        let Some(class) = custom.as_class() else {
            continue;
        };
        if !class.use_as.intersects(ClassUsage::OBJECT) {
            continue;
        }
        let count = used.remove(&custom.name).map_or(0, |e| e.count);
        entries.push(TypeEntry {
            name: custom.name.clone(),
            project: true,
            count,
            properties: properties_text(&class.members),
        });
    }
    entries.extend(used.into_values());
    entries.retain(|entry| find.is_none_or(|f| slug(&entry.name).contains(f)));
    entries
}

impl Outcome for CatalogOutcome {
    const SCHEMA: &'static str = "bt.catalog/1";

    fn report(&self) -> Report {
        let mut report = Report::new();
        if let Some(terrains) = &self.terrains {
            let mut section =
                Section::headed("terrains (paint with them; the tiles around fit themselves):");
            if terrains.is_empty() {
                section.add_line(Line::fact("none"));
            }
            section.add_lines(terrains.iter().map(|t| Line::fact(terrain_text(t))));
            report.add_section(section);
        }
        if let Some(sprites) = &self.sprites {
            report.add_section(sprites_section(
                sprites,
                self.find.is_some() || self.sheet.is_some(),
            ));
            if self.sprites_unmeasured > 0 {
                report.add_line(Line::note(format!(
                    "the feet of {} can't be measured, so they are left out of the pairs (bt objects MAP says why)",
                    plural(self.sprites_unmeasured, "sprite", "sprites")
                )));
            }
        }
        if let Some(summary) = &self.tiles_summary {
            report.add_line(Line::fact(format!(
                "tiles: {} in {}, {} used (bt catalog MAP tiles lists them by id, with where each is used)",
                plural(summary.tiles, "tile", "tiles"),
                plural(summary.tilesets, "tileset", "tilesets"),
                summary.used
            )));
        }
        if let Some(tiles) = &self.tiles {
            report.add_section(tiles_section(tiles));
        }
        if let Some(details) = &self.tile_details {
            report.add_section(details_section(details));
        }
        if let Some(types) = &self.types {
            let mut section = Section::headed("object types (object TYPE at X,Y key=value…):");
            if types.is_empty() {
                section.add_line(Line::fact(if self.find.is_some() {
                    "none matching"
                } else {
                    "none yet: any word is a type, e.g. object spawn at 3.5,4"
                }));
            }
            section.add_lines(types.iter().map(|t| {
                let whose = if t.project { "project class; " } else { "" };
                let properties = assignments_text(&t.properties);
                let properties = if properties.is_empty() {
                    String::new()
                } else {
                    format!(" {properties}")
                };
                Line::fact(format!(
                    "{} ({whose}{} in this map){properties}",
                    t.name, t.count
                ))
            }));
            report.add_section(section);
        }
        if self.nothing_found
            && let Some(find) = &self.find
        {
            report.add_line(Line::fact(format!(
                "Nothing here is called “{find}”. Most tiles have no name: bt catalog MAP tiles lists them by id (--sheet draws them), and bt at MAP X,Y shows a cell's."
            )));
        }
        if let Some(sheet) = &self.sheet {
            report.add_line(Line::fact(if sheet.tiles {
                format!(
                    "wrote {} ({}x{} px): every tile with its id under it, laid out as in its tileset; the tiles the map uses are outlined in yellow",
                    sheet.file, sheet.size.x, sheet.size.y
                )
            } else {
                format!(
                    "wrote {} ({}x{} px): the numbered pictures of the list above",
                    sheet.file, sheet.size.x, sheet.size.y
                )
            }));
        }
        report
    }
}

/// `  1. water: corner set of ground (16 tiles); borders grass, nothing (see-through edges); on ground ×40`.
fn terrain_text(terrain: &TerrainEntry) -> String {
    let number = terrain
        .number
        .map_or_else(String::new, |n| format!("{n}. "));
    let edges = match &terrain.edges {
        EdgeOpacity::NoEdges => String::new(),
        EdgeOpacity::Opaque => " (opaque edges: they draw their own ground around it)".to_owned(),
        EdgeOpacity::SeeThrough => " (see-through edges)".to_owned(),
        EdgeOpacity::Unmeasured { why } => format!(" (its edges can't be measured: {why})"),
    };
    let apart = if terrain.apart.is_empty() {
        String::new()
    } else {
        format!("; no tile joins it to {}", terrain.apart.join(", "))
    };
    let borders = if terrain.borders.is_empty() {
        "nothing".to_owned()
    } else {
        terrain.borders.join(", ")
    };
    let place = if terrain.used.is_empty() {
        "not used yet".to_owned()
    } else {
        let layers: Vec<String> = terrain
            .used
            .iter()
            .map(|u| format!("{} ({})", u.name, plural(u.count, "cell", "cells")))
            .collect();
        format!("on {}", layers.join(", "))
    };
    format!(
        "{number}{}: {} set of {} ({}); borders {borders}{edges}{apart}; {place}",
        terrain.name,
        terrain.kind,
        terrain.tileset,
        plural(terrain.tiles, "tile", "tiles")
    )
}

/// The sprites, those used first; the unused ones one a line when few or asked for.
fn sprites_section(sprites: &[SpriteEntry], list_all: bool) -> Section {
    let mut section = Section::headed(format!(
        "sprites (place them by their feet; {}):",
        plural(sprites.len(), "sprite", "sprites")
    ));
    let used = sprites.iter().filter(|s| !s.used.is_empty());
    let unused: Vec<&SpriteEntry> = sprites.iter().filter(|s| s.used.is_empty()).collect();
    if sprites.is_empty() {
        section.add_line(Line::fact("none"));
    }
    for sprite in used {
        let mut text = sprite_head(sprite);
        let layers: Vec<String> = sprite
            .used
            .iter()
            .map(|u| format!("{} ×{}", u.name, u.count))
            .collect();
        text.push_str(&format!("; on {}", layers.join(", ")));
        if let Some(partner) = &sprite.usually_with {
            text.push_str(&format!(
                "; usually with {} at {} on {} ({} of {})",
                partner.sprite,
                crate::words::point_text(partner.offset),
                partner.layer,
                partner.with,
                partner.copies
            ));
        }
        if let Some(shared) = &sprite.shared {
            let mut parts = Vec::new();
            if !shared.object_type.is_empty() {
                parts.push(format!("type {}", shared.object_type));
            }
            if !shared.properties.is_empty() {
                parts.push(assignments_text(&shared.properties));
            }
            text.push_str(&format!(
                "; all {} have {} (place … like #{} gives that)",
                shared.copies,
                parts.join(" and "),
                shared.example
            ));
        }
        section.add_line(Line::fact(text));
    }
    if !unused.is_empty() {
        let mut line = Line::fact(format!("not used yet ({}):", unused.len()));
        if list_all || unused.len() <= UNUSED_LISTED {
            line = line.with_details(unused.iter().map(|s| Line::fact(sprite_head(s))));
        } else {
            let names: Vec<&str> = unused.iter().map(|s| s.name.as_str()).collect();
            line = line.with_detail(Line::fact(names.join(", ")));
            line = line.with_detail(Line::fact(
                "(--find TEXT narrows the list, --sheet OUT.png shows them)",
            ));
        }
        section.add_line(line);
    }
    section
}

/// `3. tree: 32x48 px, animated`.
fn sprite_head(sprite: &SpriteEntry) -> String {
    let number = sprite.number.map_or_else(String::new, |n| format!("{n}. "));
    format!(
        "{number}{}: {}x{} px{}",
        sprite.name,
        sprite.size.x,
        sprite.size.y,
        if sprite.animated { ", animated" } else { "" }
    )
}

/// The tiles by tileset.
fn tiles_section(tiles: &[TilesetTiles]) -> Section {
    let mut section = Section::headed(
        "tiles (place them with `tile TILESET:ID SHAPE`; where, on, inside, avoid, near and draw take them too):",
    );
    if tiles.is_empty() {
        section.add_line(Line::fact("none"));
    }
    let many = tiles.iter().map(|t| t.layers.len()).sum::<usize>() > TILE_LAYERS_SHOWN;
    for tileset in tiles {
        let mut line = Line::fact(format!(
            "{}: {} ({}), {} used",
            tileset.tileset,
            plural(tileset.tiles, "tile", "tiles"),
            tileset.tile_size,
            tileset.used
        ));
        if !many || tiles.len() == 1 {
            for layer in &tileset.layers {
                line = line.with_detail(Line::fact(format!(
                    "on {} ({}): {}",
                    layer.layer,
                    plural(layer.cells, "cell", "cells"),
                    counts_text(&layer.tiles, usize::MAX)
                )));
            }
        }
        if !tileset.unused.is_empty() {
            line = line.with_detail(Line::fact(format!("not used: {}", tileset.unused)));
        }
        if !tileset.blank.is_empty() {
            line = line.with_detail(Line::fact(format!(
                "blank (nothing drawn): {}",
                tileset.blank
            )));
        }
        if let Some(why) = &tileset.unreadable {
            line = line.with_detail(Line::warning(format!(
                "its images can't be read, so its blank tiles aren't known: {why}"
            )));
        }
        if !tileset.classes.is_empty() {
            line = line.with_detail(Line::fact(format!(
                "named by their class: {}",
                tileset.classes.join("; ")
            )));
        }
        section.add_line(line);
    }
    if many && tiles.len() > 1 {
        section.add_line(Line::fact(
            "(bt catalog MAP tiles TILESET lists where each tile of one tileset is used)",
        ));
    }
    section
}

/// The tiles named by id, with their neighbours.
fn details_section(details: &TileDetails) -> Section {
    let mut section = Section::headed("the tiles named, where each is and what is next to it:");
    for tile in &details.tiles {
        let class = if tile.class.is_empty() {
            String::new()
        } else {
            format!(" (class {})", tile.class)
        };
        let layers: Vec<String> = tile
            .layers
            .iter()
            .map(|u| format!("{} ×{}", u.name, u.count))
            .collect();
        let line = Line::fact(format!(
            "{}{class}: on {}, at {}",
            tile.tile,
            layers.join(", "),
            cells_text(&tile.at, 10)
        ))
        .with_details(
            tile.sides
                .iter()
                .map(|side| Line::fact(format!("{}: {}", side.side, counts_text(&side.tiles, 8)))),
        );
        section.add_line(line);
    }
    if !details.unused.is_empty() {
        section.add_line(Line::fact(format!(
            "not on the map: {}",
            details.unused.join(", ")
        )));
    }
    section
}

#[cfg(test)]
mod tests {
    use bt_core::{ClassType, CustomType, MapObject, Property, Shape, Tileset, Value};
    use bt_test_data::fixtures::{Atlas, MapBuilder, TerrainSetBuilder, combination_tile};

    use super::*;
    use crate::report::{from_json, to_json};

    /// A map of grass with water painted, a set of grass and water with every combination, and
    /// a typed spawn.
    fn pond() -> Map {
        let set = TerrainSetBuilder::new(WangSetType::Corner)
            .name("Land")
            .terrains(&["Grass", "Water"])
            .every_combination();
        let atlas = Atlas::builder().name("ground").build_embedded().unwrap();
        let tileset = Arc::new(set.added_to(Tileset::clone(&atlas.tileset)));
        let grass = combination_tile(2, [1, 1, 1, 1]).unwrap().0;
        let water = combination_tile(2, [2, 2, 2, 2]).unwrap().0;
        let mut spawn = MapObject {
            class: "spawn".into(),
            shape: Shape::Point,
            ..MapObject::default()
        };
        spawn
            .properties
            .insert("wave".into(), Property::new(Value::Int(1)));
        MapBuilder::new(UVec2::splat(16), UVec2::new(3, 1))
            .tileset(tileset)
            .tile_layer("ground", &format!("{grass} {grass} {water}"))
            .object_layer("design", vec![spawn])
            .build()
            .unwrap()
    }

    #[test]
    fn the_catalog_names_terrains_types_and_tiles() {
        let map = pond();
        let names = Names::of(&map);
        let mut types = CustomTypes::default();
        types.add(CustomType::class(
            "exit",
            ClassType {
                members: [("to".to_owned(), Property::new(Value::String(String::new())))]
                    .into_iter()
                    .collect(),
                use_as: ClassUsage::OBJECT,
                ..ClassType::default()
            },
        ));
        let cataloged = CatalogOutcome::from_request(
            &CatalogRequest::new(&map, &names).custom_types(Arc::new(types)),
        )
        .unwrap();
        let outcome = &cataloged.outcome;
        let terrains = outcome.terrains.as_ref().unwrap();
        assert_eq!(terrains[0].name, "grass");
        assert_eq!(terrains[0].borders, ["water"]);
        assert_eq!(terrains[0].edges, EdgeOpacity::NoEdges);
        assert_eq!(
            terrains[1].used,
            [NameCount {
                name: "ground".into(),
                count: 1
            }]
        );
        assert_eq!(
            outcome.report().text(),
            "terrains (paint with them; the tiles around fit themselves):\n  \
             grass: corner set of ground (16 tiles); borders water; on ground (2 cells)\n  \
             water: corner set of ground (16 tiles); borders grass; on ground (1 cell)\n\
             sprites (place them by their feet; 0 sprites):\n  \
             none\n\
             tiles: 16 tiles in 1 tileset, 2 used (bt catalog MAP tiles lists them by id, with where each is used)\n\
             object types (object TYPE at X,Y key=value…):\n  \
             exit (project class; 0 in this map) to=\"\"\n  \
             spawn (1 in this map) wave=1\n"
        );
        assert_eq!(
            from_json::<CatalogOutcome>(&to_json(outcome).unwrap()).unwrap(),
            *outcome
        );
        let found = CatalogOutcome::from_request(&CatalogRequest::new(&map, &names).find("Wat"))
            .unwrap()
            .outcome;
        assert_eq!(found.terrains.unwrap().len(), 1);
        let nothing = CatalogOutcome::from_request(&CatalogRequest::new(&map, &names).find("lava"))
            .unwrap()
            .outcome;
        assert!(nothing.nothing_found);
        assert!(
            nothing
                .report()
                .text()
                .contains("Nothing here is called “lava”.")
        );
    }

    #[test]
    fn tiles_by_id_and_their_sheet() {
        let map = pond();
        let names = Names::of(&map);
        let cataloged = CatalogOutcome::from_request(
            &CatalogRequest::new(&map, &names)
                .tiles_of("ground")
                .sheet("tiles.png", Some(1)),
        )
        .unwrap();
        let tiles = cataloged.outcome.tiles.as_ref().unwrap();
        assert_eq!(tiles[0].used, 2);
        let sheet = cataloged.sheet.unwrap();
        assert_eq!(
            cataloged.outcome.sheet.as_ref().unwrap().size,
            UVec2::new(sheet.width, sheet.height)
        );
        assert!(
            cataloged
                .outcome
                .report()
                .text()
                .contains("wrote tiles.png (")
        );
        let water = combination_tile(2, [2, 2, 2, 2]).unwrap().0;
        let details = CatalogOutcome::from_request(
            &CatalogRequest::new(&map, &names).tiles_of(format!("ground:{water}")),
        )
        .unwrap()
        .outcome;
        let text = details.report().text();
        assert!(
            text.contains(&format!("ground:{water}: on ground ×1, at 2,0\n")),
            "{text}"
        );
        assert!(text.contains("left of it: 0\n"), "{text}");
        let too_many = CatalogOutcome::from_request(
            &CatalogRequest::new(&map, &names).tiles_of("ground:0-12"),
        );
        assert!(matches!(
            too_many,
            Err(Error::TooManyTiles { count: 13, .. })
        ));
        assert!(matches!(
            CatalogOutcome::from_request(
                &CatalogRequest::new(&map, &names).sheet("x.png", Some(9))
            ),
            Err(Error::SheetScale { scale: 9 })
        ));
    }

    #[test]
    fn tileds_examples_catalog_their_terrains_sprites_and_edges() {
        let Some(examples) = bt_test_data::test_data("reference/tiled/examples") else {
            return;
        };
        let mut loader = bt_tmx::Loader::default();
        let desert = loader.load_map(&examples.join("desert.tmx")).unwrap().value;
        let names = Names::of(&desert);
        let outcome = CatalogOutcome::from_request(&CatalogRequest::new(&desert, &names))
            .unwrap()
            .outcome;
        let terrains = outcome.terrains.unwrap();
        assert_eq!(terrains.len(), 4);
        assert!(
            terrains.iter().all(|t| t.number.is_some()),
            "each has a tile to show it by"
        );
        let knight = loader
            .load_map(&examples.join("sticker-knight/map/sandbox.tmx"))
            .unwrap()
            .value;
        let names = Names::of(&knight);
        let outcome = CatalogOutcome::from_request(&CatalogRequest::new(&knight, &names))
            .unwrap()
            .outcome;
        let sprites = outcome.sprites.unwrap();
        assert_eq!(sprites.len(), 62);
        let shadow = sprites.iter().find(|s| s.name == "shadow").unwrap();
        assert_eq!(
            shadow.used,
            [NameCount {
                name: "shading".into(),
                count: 17
            }]
        );
    }
}
