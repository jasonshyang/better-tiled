//! The catalog's sprites (`10-agents.md`, §10.4): each with its image size and animation, where
//! its copies are, the sprite usually paired with it (a tree and its shadow: the median offset
//! to the nearest sprite on another layer within two cells, when at least sixty percent of its
//! copies have one there) and what all its copies share (their type and properties, as
//! `place … like #ID` would give them).

use std::collections::BTreeMap;

use bt_core::{LayerId, Map, MapObject};
use glam::{DVec2, UVec2};
use serde::{Deserialize, Serialize};

use crate::feet::Feet;
use crate::layer_tree::layers_top_first;
use crate::names::{Names, Sprite, object_type};
use crate::property_text::properties_text;
use crate::tally::{NameCount, Tally};

/// How far, in cells, a sprite's partner stands at most.
const PARTNER_REACH: f64 = 2.0;

/// The share of copies that must have the partner, in tenths.
const PARTNER_TENTHS: usize = 6;

/// A sprite of the map's tilesets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpriteEntry {
    /// Its number on the sheet.
    pub number: Option<usize>,
    /// Its name.
    pub name: String,
    /// Its image's size in pixels.
    pub size: UVec2,
    /// Whether it is animated.
    pub animated: bool,
    /// Its copies on the map, layer by layer.
    pub used: Vec<NameCount>,
    /// The sprite usually placed with it.
    pub usually_with: Option<Partner>,
    /// What all its copies share.
    pub shared: Option<Shared>,
}

/// The sprite usually placed with another, at an offset from its foot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Partner {
    /// The partner sprite.
    pub sprite: String,
    /// Its foot from the sprite's, in tile units: the median of the copies'.
    pub offset: DVec2,
    /// The layer it is on.
    pub layer: String,
    /// How many copies have it there.
    pub with: usize,
    /// How many copies there are.
    pub copies: usize,
}

/// What every copy of a sprite has: its type and properties.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shared {
    /// How many copies there are.
    pub copies: usize,
    /// The type they all have; empty when they differ or have none.
    pub object_type: String,
    /// The properties they all have, with the same values.
    pub properties: BTreeMap<String, String>,
    /// A copy, for `place … like #ID`.
    pub example: u32,
}

/// The map's sprites: those it uses, then the rest, each in the order of the tilesets.
pub(crate) struct SpriteList {
    /// The sprites.
    pub(crate) entries: Vec<SpriteEntry>,
    /// The copies whose feet can't be measured, left out of the pairs.
    pub(crate) unmeasured: usize,
}

/// A copy of a sprite on the map.
struct Copy<'m> {
    sprite: String,
    layer: LayerId,
    layer_name: String,
    foot: Option<DVec2>,
    object: &'m MapObject,
}

/// The sprites of `map` whose names hold `find` (all without it).
pub(crate) fn sprite_list(map: &Map, names: &Names, find: Option<&str>) -> SpriteList {
    let mut feet = Feet::new();
    let mut copies: Vec<Copy<'_>> = Vec::new();
    let mut unmeasured = 0;
    for at in layers_top_first(map) {
        let Some(objects) = at.layer.as_object_layer() else {
            continue;
        };
        for object in objects.objects.iter() {
            let Some(sprite) = object.tile.and_then(|tile| names.sprite_of(tile)) else {
                continue;
            };
            let foot = feet.standing_point(map, object).ok();
            if foot.is_none() {
                unmeasured += 1;
            }
            copies.push(Copy {
                sprite: sprite.name.clone(),
                layer: at.layer.id,
                layer_name: at.layer.name.clone(),
                foot,
                object,
            });
        }
    }
    let matching: Vec<&Sprite> = names
        .sprites()
        .iter()
        .filter(|s| find.is_none_or(|f| s.name.contains(f)))
        .collect();
    let mut used = Vec::new();
    let mut rest = Vec::new();
    for sprite in matching {
        let mine: Vec<&Copy<'_>> = copies.iter().filter(|c| c.sprite == sprite.name).collect();
        let mut layers = Tally::new();
        for copy in &mine {
            layers.add_one(copy.layer_name.clone());
        }
        let entry = SpriteEntry {
            number: None,
            name: sprite.name.clone(),
            size: sprite.size,
            animated: sprite.animated,
            used: layers.sorted(),
            usually_with: partner(&mine, &copies),
            shared: shared(map, &mine),
        };
        if mine.is_empty() {
            rest.push(entry);
        } else {
            used.push(entry);
        }
    }
    used.extend(rest);
    SpriteList {
        entries: used,
        unmeasured,
    }
}

/// The sprite on another layer standing nearest to most copies of a sprite, within two cells,
/// when at least sixty percent of the copies have it: with the median offset between feet.
fn partner(mine: &[&Copy<'_>], all: &[Copy<'_>]) -> Option<Partner> {
    /// A sprite on a layer.
    #[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
    struct OnLayer {
        sprite: String,
        layer: LayerId,
    }
    #[derive(Default)]
    struct Offsets {
        x: Vec<f64>,
        y: Vec<f64>,
        layer: String,
    }
    let mut found: BTreeMap<OnLayer, Offsets> = BTreeMap::new();
    for copy in mine {
        let Some(foot) = copy.foot else {
            continue;
        };
        let distance = |other: &Copy<'_>| other.foot.map(|f| f.distance(foot));
        let nearest = all
            .iter()
            .filter(|other| other.layer != copy.layer)
            .filter(|other| distance(other).is_some_and(|d| d <= PARTNER_REACH))
            .min_by(|a, b| {
                let far = f64::INFINITY;
                distance(a)
                    .unwrap_or(far)
                    .total_cmp(&distance(b).unwrap_or(far))
            });
        if let Some(other) = nearest
            && let Some(at) = other.foot
        {
            let entry = found
                .entry(OnLayer {
                    sprite: other.sprite.clone(),
                    layer: other.layer,
                })
                .or_default();
            entry.x.push(at.x - foot.x);
            entry.y.push(at.y - foot.y);
            entry.layer.clone_from(&other.layer_name);
        }
    }
    let (OnLayer { sprite, .. }, offsets) = found.into_iter().max_by_key(|(_, o)| o.x.len())?;
    let with = offsets.x.len();
    let copies = mine.len();
    if with < 2 || with * 10 < copies * PARTNER_TENTHS {
        return None;
    }
    Some(Partner {
        sprite,
        offset: DVec2::new(median(offsets.x), median(offsets.y)),
        layer: offsets.layer,
        with,
        copies,
    })
}

/// The middle value, the upper of the two middle ones for an even count.
fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied().unwrap_or(0.0)
}

/// What every one of at least two copies has: their type, and the properties of the same value.
fn shared(map: &Map, mine: &[&Copy<'_>]) -> Option<Shared> {
    let [first, rest @ ..] = mine else {
        return None;
    };
    if rest.is_empty() {
        return None;
    }
    let kind = object_type(map, first.object);
    let object_type = if mine.iter().all(|c| object_type(map, c.object) == kind) {
        kind.to_owned()
    } else {
        String::new()
    };
    let properties = properties_text(&first.object.properties)
        .into_iter()
        .filter(|(key, _)| {
            let value = first.object.properties.get(key);
            rest.iter().all(|c| c.object.properties.get(key) == value)
        })
        .collect::<BTreeMap<String, String>>();
    if object_type.is_empty() && properties.is_empty() {
        return None;
    }
    Some(Shared {
        copies: mine.len(),
        object_type,
        properties,
        example: first.object.id.0,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{Cell, Property, TileId, TilesetSlot, Value};
    use bt_test_data::fixtures::{Atlas, MapBuilder};

    use super::*;

    /// A tile object of tile `id` of the drawn collection-like atlas, its foot at `foot`.
    fn sprite(id: u32, foot: DVec2) -> MapObject {
        MapObject {
            tile: Some(Cell::new(TilesetSlot::new(0).unwrap(), TileId(id))),
            pos: DVec2::new(foot.x * 16.0 - 8.0, foot.y * 16.0),
            size: DVec2::splat(16.0),
            class: "coin".into(),
            properties: [("points".to_owned(), Property::new(Value::Int(1)))]
                .into_iter()
                .collect(),
            ..MapObject::default()
        }
    }

    #[test]
    fn partners_are_the_sprites_usually_beside_and_copies_share_what_they_all_have() {
        // A collection whose tiles 0 and 1 are a tree and its shadow: the atlas's tiles as
        // images of their own.
        let atlas = Atlas::builder().name("trees").build_embedded().unwrap();
        let mut collection = bt_core::Tileset::clone(&atlas.tileset);
        let image = match &collection.image {
            bt_core::TilesetImage::Atlas { image, .. } => image.clone(),
            bt_core::TilesetImage::Collection => panic!("an atlas"),
        };
        collection.image = bt_core::TilesetImage::Collection;
        for id in 0..2u32 {
            collection.tiles.insert(
                TileId(id),
                bt_core::TileData {
                    image: Some(image.clone()),
                    rect: Some(bt_utils::IRect::from_min_size(
                        glam::IVec2::new(i32::try_from(id).unwrap() * 16, 0),
                        glam::IVec2::splat(16),
                    )),
                    ..bt_core::TileData::default()
                },
            );
        }
        let trees: Vec<MapObject> = (0..4)
            .map(|i| sprite(0, DVec2::new(2.5 + f64::from(i) * 4.0, 3.0)))
            .collect();
        let shadows: Vec<MapObject> = (0..3)
            .map(|i| sprite(1, DVec2::new(3.0 + f64::from(i) * 4.0, 4.0)))
            .collect();
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(20, 6))
            .tileset(Arc::new(collection))
            .object_layer("shadows", shadows)
            .object_layer("props", trees)
            .build()
            .unwrap();
        let names = Names::of(&map);
        let list = sprite_list(&map, &names, None);
        assert_eq!(list.unmeasured, 0);
        let tree = &list.entries[0];
        assert_eq!(
            tree.used,
            [NameCount {
                name: "props".into(),
                count: 4
            }]
        );
        let partner = tree.usually_with.as_ref().unwrap();
        assert_eq!(
            (partner.layer.as_str(), partner.with, partner.copies),
            ("shadows", 3, 4)
        );
        assert_eq!(partner.offset, DVec2::new(0.5, 1.0));
        let shared = tree.shared.as_ref().unwrap();
        assert_eq!(shared.object_type, "coin");
        assert_eq!(shared.properties["points"], "1");
    }
}
