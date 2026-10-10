//! A tileset's built-in attributes (Tiled's `TilesetProperties`, `reference/tiled/src/tiled/
//! propertieswidget.cpp`): name, class, object alignment, drawing offset, tile render size,
//! fill mode, background colour, grid and allowed transformations; for an atlas, its image,
//! transparent colour, tile size, margin and spacing, read-only (Tiled changes those through
//! its tileset parameters dialog).

use bt_core::{GridOrientation, Tileset, TilesetGrid, TilesetImage, Transformations};
use glam::UVec2;

use super::attribute::Attribute;
use super::attribute_check::{bool_of, check_value, color_of, integer_of, invalid, text_of};
use super::attribute_row::{AttributeRow, Reading, assign, rows_of, settable};
use super::attribute_value::AttributeValue;
use super::error::PropertyError;

/// The tileset's attributes, in the order Tiled shows them.
const ORDER: [Attribute; 21] = [
    Attribute::Name,
    Attribute::Class,
    Attribute::ObjectAlignment,
    Attribute::TileOffsetX,
    Attribute::TileOffsetY,
    Attribute::TileRenderSize,
    Attribute::FillMode,
    Attribute::BackgroundColor,
    Attribute::GridOrientation,
    Attribute::GridWidth,
    Attribute::GridHeight,
    Attribute::AllowFlipHorizontally,
    Attribute::AllowFlipVertically,
    Attribute::AllowRotate,
    Attribute::PreferUntransformed,
    Attribute::Image,
    Attribute::TransparentColor,
    Attribute::TileWidth,
    Attribute::TileHeight,
    Attribute::Margin,
    Attribute::Spacing,
];

/// The tileset's built-in attributes as the Properties panel shows them.
pub fn tileset_attributes(tileset: &Tileset) -> Vec<AttributeRow> {
    rows_of(&ORDER, &[tileset], |tileset, attribute| {
        read(tileset, attribute)
    })
}

/// Sets one of the tileset's built-in attributes; returns whether the tileset changed. Refused
/// when the value is of the wrong kind or out of range, or the attribute is read-only.
pub fn set_tileset_attribute(
    tileset: &mut Tileset,
    attribute: Attribute,
    value: &AttributeValue,
) -> Result<bool, PropertyError> {
    check_value(attribute, value)?;
    settable(attribute, &[&*tileset], read)?;
    write(tileset, attribute, value)
}

/// The tileset's grid: its own, else the tile size with square cells.
fn grid_of(tileset: &Tileset) -> TilesetGrid {
    tileset.grid.unwrap_or(TilesetGrid {
        orientation: GridOrientation::Orthogonal,
        size: tileset.tile_size(),
    })
}

/// The transformation flag an attribute stands for.
fn transformation(attribute: Attribute) -> Option<Transformations> {
    match attribute {
        Attribute::AllowFlipHorizontally => Some(Transformations::HFLIP),
        Attribute::AllowFlipVertically => Some(Transformations::VFLIP),
        Attribute::AllowRotate => Some(Transformations::ROTATE),
        Attribute::PreferUntransformed => Some(Transformations::PREFER_UNTRANSFORMED),
        _ => None,
    }
}

/// One attribute of the tileset; `None` for one it does not have (an atlas's image, of a
/// collection).
fn read(tileset: &Tileset, attribute: Attribute) -> Option<Reading> {
    let integer = |number: u32| AttributeValue::Integer(i64::from(number));
    let atlas_image = match &tileset.image {
        TilesetImage::Atlas { image, .. } => Some(image),
        TilesetImage::Collection => None,
    };
    if let Some(flag) = transformation(attribute) {
        return Some(Reading::editable(AttributeValue::Bool(
            tileset.transformations.contains(flag),
        )));
    }
    Some(match attribute {
        Attribute::Name => Reading::editable(AttributeValue::Text(tileset.name.clone())),
        Attribute::Class => Reading::editable(AttributeValue::Text(tileset.class.clone())),
        Attribute::ObjectAlignment => {
            Reading::editable(AttributeValue::Alignment(tileset.object_alignment))
        }
        Attribute::TileOffsetX => {
            Reading::editable(AttributeValue::Integer(i64::from(tileset.tile_offset.x)))
        }
        Attribute::TileOffsetY => {
            Reading::editable(AttributeValue::Integer(i64::from(tileset.tile_offset.y)))
        }
        Attribute::TileRenderSize => {
            Reading::editable(AttributeValue::TileRenderSize(tileset.tile_render_size))
        }
        Attribute::FillMode => Reading::editable(AttributeValue::FillMode(tileset.fill_mode)),
        Attribute::BackgroundColor => {
            Reading::editable(AttributeValue::Color(tileset.background_color))
        }
        Attribute::GridOrientation => Reading::editable(AttributeValue::GridOrientation(
            grid_of(tileset).orientation,
        )),
        Attribute::GridWidth => Reading::editable(integer(grid_of(tileset).size.x)),
        Attribute::GridHeight => Reading::editable(integer(grid_of(tileset).size.y)),
        Attribute::Image => Reading::fixed(AttributeValue::Image(Some(atlas_image?.clone()))),
        Attribute::TransparentColor => {
            Reading::fixed(AttributeValue::Color(atlas_image?.transparent))
        }
        Attribute::TileWidth => {
            atlas_image?;
            Reading::fixed(integer(tileset.tile_width))
        }
        Attribute::TileHeight => {
            atlas_image?;
            Reading::fixed(integer(tileset.tile_height))
        }
        Attribute::Margin => {
            atlas_image?;
            Reading::fixed(integer(tileset.margin))
        }
        Attribute::Spacing => {
            atlas_image?;
            Reading::fixed(integer(tileset.spacing))
        }
        _ => return None,
    })
}

/// Sets one attribute of the tileset to a value already checked.
fn write(
    tileset: &mut Tileset,
    attribute: Attribute,
    value: &AttributeValue,
) -> Result<bool, PropertyError> {
    if let Some(flag) = transformation(attribute) {
        let mut transformations = tileset.transformations;
        transformations.set(flag, bool_of(attribute, value)?);
        return Ok(assign(&mut tileset.transformations, transformations));
    }
    let grid = grid_of(tileset);
    Ok(match (attribute, value) {
        (Attribute::Name, _) => assign(&mut tileset.name, text_of(attribute, value)?),
        (Attribute::Class, _) => assign(&mut tileset.class, text_of(attribute, value)?),
        (Attribute::ObjectAlignment, AttributeValue::Alignment(alignment)) => {
            assign(&mut tileset.object_alignment, *alignment)
        }
        (Attribute::TileOffsetX, _) => {
            assign(&mut tileset.tile_offset.x, integer_of(attribute, value)?)
        }
        (Attribute::TileOffsetY, _) => {
            assign(&mut tileset.tile_offset.y, integer_of(attribute, value)?)
        }
        (Attribute::TileRenderSize, AttributeValue::TileRenderSize(size)) => {
            assign(&mut tileset.tile_render_size, *size)
        }
        (Attribute::FillMode, AttributeValue::FillMode(mode)) => {
            assign(&mut tileset.fill_mode, *mode)
        }
        (Attribute::BackgroundColor, _) => {
            assign(&mut tileset.background_color, color_of(attribute, value)?)
        }
        (Attribute::GridOrientation, AttributeValue::GridOrientation(orientation)) => set_grid(
            tileset,
            TilesetGrid {
                orientation: *orientation,
                ..grid
            },
        ),
        (Attribute::GridWidth, _) => set_grid(
            tileset,
            TilesetGrid {
                size: UVec2::new(integer_of(attribute, value)?, grid.size.y),
                ..grid
            },
        ),
        (Attribute::GridHeight, _) => set_grid(
            tileset,
            TilesetGrid {
                size: UVec2::new(grid.size.x, integer_of(attribute, value)?),
                ..grid
            },
        ),
        _ => return Err(invalid(attribute, value)),
    })
}

/// Gives the tileset `grid`; whether its grid changed (a tileset without a grid of its own has
/// the tile size with square cells).
fn set_grid(tileset: &mut Tileset, grid: TilesetGrid) -> bool {
    if grid_of(tileset) == grid {
        return false;
    }
    tileset.grid = Some(grid);
    true
}

#[cfg(test)]
mod tests {
    use bt_core::{Alignment, Color};
    use bt_test_data::fixtures::Atlas;

    use super::*;

    fn row(tileset: &Tileset, attribute: Attribute) -> Option<AttributeRow> {
        tileset_attributes(tileset)
            .into_iter()
            .find(|row| row.attribute == attribute)
    }

    #[test]
    fn an_atlas_shows_its_image_read_only_and_a_collection_does_not() {
        let atlas = Atlas::builder().build_embedded().unwrap().tileset;
        assert_eq!(tileset_attributes(&atlas).len(), 21);
        let image = row(&atlas, Attribute::Image).unwrap();
        assert!(image.read_only);
        assert!(row(&atlas, Attribute::Margin).unwrap().read_only);
        assert_eq!(
            row(&atlas, Attribute::GridWidth).unwrap().value,
            AttributeValue::Integer(16),
            "the tile size, without a grid of its own"
        );
        let collection = Tileset::default();
        assert!(row(&collection, Attribute::Image).is_none());
        assert!(row(&collection, Attribute::TileWidth).is_none());
        assert_eq!(tileset_attributes(&collection).len(), 15);
    }

    #[test]
    fn tileset_attributes_set_with_ranges_checked() {
        let mut tileset = (*Atlas::builder().build_embedded().unwrap().tileset).clone();
        let mut set = |attribute, value| set_tileset_attribute(&mut tileset, attribute, &value);
        assert_eq!(
            set(Attribute::Name, AttributeValue::Text("terrain".into())),
            Ok(true)
        );
        assert_eq!(
            set(
                Attribute::ObjectAlignment,
                AttributeValue::Alignment(Alignment::Center)
            ),
            Ok(true)
        );
        assert_eq!(
            set(Attribute::TileOffsetY, AttributeValue::Integer(-4)),
            Ok(true)
        );
        assert_eq!(
            set(Attribute::AllowRotate, AttributeValue::Bool(true)),
            Ok(true)
        );
        assert_eq!(
            set(Attribute::AllowRotate, AttributeValue::Bool(true)),
            Ok(false)
        );
        assert_eq!(
            set(Attribute::GridWidth, AttributeValue::Integer(16)),
            Ok(false),
            "the grid it has already"
        );
        assert_eq!(
            set(Attribute::GridWidth, AttributeValue::Integer(32)),
            Ok(true)
        );
        assert_eq!(
            set(Attribute::GridHeight, AttributeValue::Integer(0)),
            Err(PropertyError::OutOfRange {
                attribute: Attribute::GridHeight,
                min: Some(1.0),
                max: Some(f64::from(i32::MAX))
            })
        );
        assert_eq!(
            set(Attribute::Margin, AttributeValue::Integer(2)),
            Err(PropertyError::ReadOnly(Attribute::Margin))
        );
        assert_eq!(
            set(
                Attribute::BackgroundColor,
                AttributeValue::Color(Some(Color::rgb(0, 0, 0)))
            ),
            Ok(true)
        );
        assert_eq!(tileset.name, "terrain");
        assert_eq!(tileset.object_alignment, Alignment::Center);
        assert_eq!(tileset.tile_offset.y, -4);
        assert!(tileset.transformations.contains(Transformations::ROTATE));
        assert_eq!(
            tileset.grid,
            Some(TilesetGrid {
                orientation: GridOrientation::Orthogonal,
                size: UVec2::new(32, 16)
            })
        );
    }
}
