//! Tiled's built-in rule tileset (`:/automap-tiles.tsx`), which AutoMap rule maps refer to: a
//! value compiled into the binary, with an image drawn for better-tiled.

use bt_core::{
    EmbeddedImage, FileFormat, FillMode, ImageRef, ImageSource, Properties, Property, TileData,
    TileId, TileRenderSize, Tileset, TilesetImage, Value,
};
use glam::UVec2;

/// How files refer to the built-in rule tileset.
pub(crate) const REFERENCE: &str = ":/automap-tiles.tsx";

/// The tiles' `MatchType`s, by tile id.
const MATCH_TYPES: [&str; 5] = ["Negate", "Ignore", "NonEmpty", "Empty", "Other"];

/// The image: five 32×32 tiles in a row (`scripts/automap-tiles-png.py`).
const IMAGE: &[u8] = include_bytes!("automap-tiles.png");

/// The built-in rule tileset: Negate, Ignore, NonEmpty, Empty and Other, each tile with its
/// `MatchType` property, laid out as Tiled's.
pub fn builtin_automap_tiles() -> Tileset {
    let tiles = MATCH_TYPES
        .iter()
        .zip(0u32..)
        .map(|(kind, id)| {
            let mut properties = Properties::new();
            properties.insert(
                "MatchType".to_owned(),
                Property::new(Value::String((*kind).to_owned())),
            );
            (
                TileId(id),
                TileData {
                    properties,
                    ..TileData::default()
                },
            )
        })
        .collect();
    let image = ImageRef {
        source: ImageSource::Embedded(EmbeddedImage {
            format: "png".to_owned(),
            bytes: IMAGE.into(),
        }),
        declared_size: UVec2::new(160, 32),
        transparent: None,
    };
    Tileset {
        name: "AutoMap Rules".to_owned(),
        tile_width: 32,
        tile_height: 32,
        image: TilesetImage::Atlas {
            image,
            columns: 5,
            tile_count: 5,
        },
        tile_render_size: TileRenderSize::Grid,
        fill_mode: FillMode::PreserveAspectFit,
        tiles,
        next_tile_id: 5,
        file: FileFormat::default(),
        ..Tileset::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rule_tiles_are_five_with_their_match_types() {
        let tileset = builtin_automap_tiles();
        assert_eq!(tileset.tile_count(), 5);
        assert_eq!(
            tileset.tile(TileId(3)).unwrap().properties["MatchType"].value,
            Value::String("Empty".into())
        );
        assert_eq!(
            bt_image::probe_size_of_bytes(IMAGE, "png").unwrap(),
            UVec2::new(160, 32)
        );
    }
}
