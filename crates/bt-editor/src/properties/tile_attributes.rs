//! Tiles' built-in attributes (Tiled's `TileProperties`, `reference/tiled/src/tiled/
//! propertieswidget.cpp`): id (read-only), class, probability, and a collection tile's image.
//! An atlas tile without data gets some only when a value set differs from the defaults.

use bt_core::{ImageRef, TileData, TileId, Tileset, TilesetImage};
use glam::UVec2;

use super::attribute::Attribute;
use super::attribute_check::{check_value, image_of, invalid, number_of, text_of};
use super::attribute_row::{AttributeRow, Reading, assign, rows_of, settable};
use super::attribute_value::AttributeValue;
use super::error::PropertyError;

/// The tiles' attributes, in the order Tiled shows them.
const ORDER: [Attribute; 4] = [
    Attribute::Id,
    Attribute::Class,
    Attribute::Probability,
    Attribute::Image,
];

/// A tile and its data, if it has any.
#[derive(Copy, Clone)]
struct Tile<'t> {
    id: TileId,
    data: Option<&'t TileData>,
    in_collection: bool,
}

/// The built-in attributes of the tileset's tiles `tiles` as the Properties panel shows them.
/// A tile the tileset does not have is an error.
pub fn tile_attributes(
    tileset: &Tileset,
    tiles: &[TileId],
) -> Result<Vec<AttributeRow>, PropertyError> {
    let found = found_tiles(tileset, tiles)?;
    Ok(rows_of(&ORDER, &found, |tile, attribute| {
        read(*tile, attribute)
    }))
}

/// Sets one built-in attribute of the tileset's tiles `tiles` (an image only on a collection's
/// tiles). A collection's tile size follows its largest image, as in Tiled. Returns whether any
/// tile changed. Refused, changing nothing, when the value is of the wrong kind or out of
/// range, no tile has the attribute, it is read-only, or a tile is missing.
pub fn set_tile_attribute(
    tileset: &mut Tileset,
    tiles: &[TileId],
    attribute: Attribute,
    value: &AttributeValue,
) -> Result<bool, PropertyError> {
    check_value(attribute, value)?;
    let found = found_tiles(tileset, tiles)?;
    let ids: Vec<TileId> = settable(attribute, &found, read)?
        .into_iter()
        .map(|tile| tile.id)
        .collect();
    let mut changed = false;
    for id in ids {
        let mut data = tileset.tile(id).cloned().unwrap_or_default();
        if write(&mut data, attribute, value)? {
            tileset.tiles.insert(id, data);
            changed = true;
        }
    }
    if changed && attribute == Attribute::Image {
        let largest = largest_image(tileset);
        tileset.tile_width = largest.x;
        tileset.tile_height = largest.y;
    }
    Ok(changed)
}

/// The tiles with these ids.
fn found_tiles<'t>(tileset: &'t Tileset, ids: &[TileId]) -> Result<Vec<Tile<'t>>, PropertyError> {
    let in_collection = matches!(tileset.image, TilesetImage::Collection);
    ids.iter()
        .map(|id| {
            if tileset.has_tile(*id) {
                Ok(Tile {
                    id: *id,
                    data: tileset.tile(*id),
                    in_collection,
                })
            } else {
                Err(PropertyError::NoSuchTile(*id))
            }
        })
        .collect()
}

/// One attribute of a tile; `None` for one it does not have.
fn read(tile: Tile<'_>, attribute: Attribute) -> Option<Reading> {
    let defaults = TileData::default();
    let data = tile.data.unwrap_or(&defaults);
    Some(match attribute {
        Attribute::Id => Reading::fixed(AttributeValue::Integer(i64::from(tile.id.0))),
        Attribute::Class => Reading::editable(AttributeValue::Text(data.class.clone())),
        Attribute::Probability => Reading::editable(AttributeValue::Number(data.probability)),
        Attribute::Image if tile.in_collection => {
            Reading::editable(AttributeValue::Image(data.image.clone()))
        }
        _ => return None,
    })
}

/// Sets one attribute of a tile's data to a value already checked.
fn write(
    data: &mut TileData,
    attribute: Attribute,
    value: &AttributeValue,
) -> Result<bool, PropertyError> {
    Ok(match attribute {
        Attribute::Class => assign(&mut data.class, text_of(attribute, value)?),
        Attribute::Probability => assign(&mut data.probability, number_of(attribute, value)?),
        Attribute::Image => {
            // The image's transparent colour is the tile's, whatever image it shows; a part of
            // the old image means nothing in a new one.
            let transparent = data.image.as_ref().and_then(|image| image.transparent);
            let image = image_of(attribute, value)?.map(|image| ImageRef {
                transparent,
                ..image
            });
            if data.image == image {
                return Ok(false);
            }
            data.image = image;
            data.rect = None;
            true
        }
        _ => return Err(invalid(attribute, value)),
    })
}

/// The size of a collection's largest tile (Tiled's `Tileset::updateTileSize`).
fn largest_image(tileset: &Tileset) -> UVec2 {
    tileset
        .tiles
        .keys()
        .map(|id| tileset.tile_pixel_size(*id))
        .fold(UVec2::ZERO, UVec2::max)
}

#[cfg(test)]
mod tests {
    use bt_core::FileRef;
    use bt_test_data::fixtures::Atlas;

    use super::*;

    fn image(path: &str, size: UVec2) -> ImageRef {
        ImageRef::file(FileRef::new(path, format!("/{path}")), size)
    }

    fn collection() -> Tileset {
        let mut tileset = Tileset {
            tile_width: 32,
            tile_height: 32,
            ..Tileset::default()
        };
        for (id, size) in [(0, UVec2::new(32, 32)), (1, UVec2::new(16, 8))] {
            tileset.tiles.insert(
                TileId(id),
                TileData {
                    image: Some(image(&format!("{id}.png"), size)),
                    ..TileData::default()
                },
            );
        }
        tileset
    }

    #[test]
    fn several_tiles_show_their_probability_and_class() {
        let mut atlas = (*Atlas::builder().build_embedded().unwrap().tileset).clone();
        atlas.tiles.insert(
            TileId(2),
            TileData {
                probability: 0.5,
                ..TileData::default()
            },
        );
        let rows = tile_attributes(&atlas, &[TileId(1), TileId(2)]).unwrap();
        assert_eq!(rows.len(), 3, "an atlas's tiles have no image of their own");
        let probability = &rows[2];
        assert_eq!(probability.attribute, Attribute::Probability);
        assert!(probability.mixed);
        assert_eq!(probability.value, AttributeValue::Number(1.0));
        assert_eq!(
            tile_attributes(&atlas, &[TileId(16)]).unwrap_err(),
            PropertyError::NoSuchTile(TileId(16))
        );
    }

    #[test]
    fn atlas_tiles_get_data_only_when_a_value_changes() {
        let mut atlas = (*Atlas::builder().build_embedded().unwrap().tileset).clone();
        let tiles = [TileId(1), TileId(3)];
        assert_eq!(
            set_tile_attribute(
                &mut atlas,
                &tiles,
                Attribute::Probability,
                &AttributeValue::Number(1.0)
            ),
            Ok(false)
        );
        assert!(atlas.tiles.is_empty());
        assert_eq!(
            set_tile_attribute(
                &mut atlas,
                &tiles,
                Attribute::Probability,
                &AttributeValue::Number(2.0)
            ),
            Ok(true)
        );
        assert_eq!(atlas.tile(TileId(3)).unwrap().probability, 2.0);
        assert_eq!(
            set_tile_attribute(
                &mut atlas,
                &tiles,
                Attribute::Probability,
                &AttributeValue::Number(-1.0)
            ),
            Err(PropertyError::OutOfRange {
                attribute: Attribute::Probability,
                min: Some(0.0),
                max: None
            })
        );
        assert_eq!(
            set_tile_attribute(
                &mut atlas,
                &tiles,
                Attribute::Id,
                &AttributeValue::Integer(4)
            ),
            Err(PropertyError::ReadOnly(Attribute::Id))
        );
        assert_eq!(
            set_tile_attribute(
                &mut atlas,
                &tiles,
                Attribute::Image,
                &AttributeValue::Image(None)
            ),
            Err(PropertyError::NotApplicable(Attribute::Image))
        );
    }

    #[test]
    fn a_collection_tiles_image_changes_and_the_tile_size_follows() {
        let mut tileset = collection();
        assert_eq!(
            set_tile_attribute(
                &mut tileset,
                &[TileId(1)],
                Attribute::Image,
                &AttributeValue::Image(Some(image("big.png", UVec2::new(48, 40))))
            ),
            Ok(true)
        );
        assert_eq!(tileset.tile_size(), UVec2::new(48, 40));
        assert_eq!(
            set_tile_attribute(
                &mut tileset,
                &[TileId(1)],
                Attribute::Image,
                &AttributeValue::Image(Some(image("small.png", UVec2::new(8, 8))))
            ),
            Ok(true)
        );
        assert_eq!(
            tileset.tile_size(),
            UVec2::new(32, 32),
            "tile 0 is the largest again"
        );
    }
}
