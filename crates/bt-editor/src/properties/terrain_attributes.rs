//! The built-in attributes of a terrain set and of a terrain (Tiled's Wang set and Wang colour
//! properties, `reference/tiled/src/tiled/propertieswidget.cpp`): a set's name, class and type;
//! a terrain's name, class, colour and probability.

use bt_core::{Tileset, WangColor, WangSet};
use bt_terrain::TerrainColor;

use super::attribute::Attribute;
use super::attribute_check::{check_value, color_of, invalid, number_of, text_of};
use super::attribute_row::{AttributeRow, Reading, assign, rows_of, settable};
use super::attribute_value::AttributeValue;
use super::error::PropertyError;

/// A set's attributes, in the order Tiled shows them.
const SET_ORDER: [Attribute; 3] = [Attribute::Name, Attribute::Class, Attribute::TerrainType];

/// A terrain's attributes, in the order Tiled shows them.
const TERRAIN_ORDER: [Attribute; 4] = [
    Attribute::Name,
    Attribute::Class,
    Attribute::Color,
    Attribute::Probability,
];

/// The attributes of the tileset's terrain set `set` as the Properties panel shows them.
pub fn terrain_set_attributes(
    tileset: &Tileset,
    set: usize,
) -> Result<Vec<AttributeRow>, PropertyError> {
    let set = set_of(tileset, set)?;
    Ok(rows_of(&SET_ORDER, &[set], |set, attribute| {
        read_set(set, attribute)
    }))
}

/// Sets one attribute of the terrain set `set`; whether it changed. Changing the type keeps the
/// labels: the solver reads only the lanes of the new type, as in Tiled.
pub fn set_terrain_set_attribute(
    tileset: &mut Tileset,
    set: usize,
    attribute: Attribute,
    value: &AttributeValue,
) -> Result<bool, PropertyError> {
    check_value(attribute, value)?;
    let set = set_of_mut(tileset, set)?;
    settable(attribute, &[&*set], read_set)?;
    Ok(match (attribute, value) {
        (Attribute::Name, _) => assign(&mut set.name, text_of(attribute, value)?),
        (Attribute::Class, _) => assign(&mut set.class, text_of(attribute, value)?),
        (Attribute::TerrainType, AttributeValue::TerrainType(kind)) => assign(&mut set.kind, *kind),
        _ => return Err(invalid(attribute, value)),
    })
}

/// The attributes of terrain `color` of the tileset's set `set`.
pub fn terrain_attributes(
    tileset: &Tileset,
    set: usize,
    color: TerrainColor,
) -> Result<Vec<AttributeRow>, PropertyError> {
    let terrain = terrain_of(set_of(tileset, set)?, set, color)?;
    Ok(rows_of(&TERRAIN_ORDER, &[terrain], |terrain, attribute| {
        read_terrain(terrain, attribute)
    }))
}

/// Sets one attribute of terrain `color` of set `set`; whether it changed. A terrain always has
/// a colour.
pub fn set_terrain_attribute(
    tileset: &mut Tileset,
    set: usize,
    color: TerrainColor,
    attribute: Attribute,
    value: &AttributeValue,
) -> Result<bool, PropertyError> {
    check_value(attribute, value)?;
    let terrain = terrain_of_mut(set_of_mut(tileset, set)?, set, color)?;
    settable(attribute, &[&*terrain], read_terrain)?;
    Ok(match attribute {
        Attribute::Name => assign(&mut terrain.name, text_of(attribute, value)?),
        Attribute::Class => assign(&mut terrain.class, text_of(attribute, value)?),
        Attribute::Color => match color_of(attribute, value)? {
            Some(rgb) => assign(&mut terrain.color, rgb),
            None => return Err(invalid(attribute, value)),
        },
        Attribute::Probability => assign(&mut terrain.probability, number_of(attribute, value)?),
        _ => return Err(invalid(attribute, value)),
    })
}

fn read_set(set: &WangSet, attribute: Attribute) -> Option<Reading> {
    Some(match attribute {
        Attribute::Name => Reading::editable(AttributeValue::Text(set.name.clone())),
        Attribute::Class => Reading::editable(AttributeValue::Text(set.class.clone())),
        Attribute::TerrainType => Reading::editable(AttributeValue::TerrainType(set.kind)),
        _ => return None,
    })
}

fn read_terrain(terrain: &WangColor, attribute: Attribute) -> Option<Reading> {
    Some(match attribute {
        Attribute::Name => Reading::editable(AttributeValue::Text(terrain.name.clone())),
        Attribute::Class => Reading::editable(AttributeValue::Text(terrain.class.clone())),
        Attribute::Color => Reading::editable(AttributeValue::Color(Some(terrain.color))),
        Attribute::Probability => Reading::editable(AttributeValue::Number(terrain.probability)),
        _ => return None,
    })
}

fn set_of(tileset: &Tileset, set: usize) -> Result<&WangSet, PropertyError> {
    tileset
        .wang_sets
        .get(set)
        .ok_or(PropertyError::NoSuchTerrainSet(set))
}

fn set_of_mut(tileset: &mut Tileset, set: usize) -> Result<&mut WangSet, PropertyError> {
    tileset
        .wang_sets
        .get_mut(set)
        .ok_or(PropertyError::NoSuchTerrainSet(set))
}

fn terrain_of(
    set: &WangSet,
    index: usize,
    color: TerrainColor,
) -> Result<&WangColor, PropertyError> {
    usize::from(color.0)
        .checked_sub(1)
        .and_then(|i| set.colors.get(i))
        .ok_or(PropertyError::NoSuchTerrain {
            set: index,
            color: color.0,
        })
}

fn terrain_of_mut(
    set: &mut WangSet,
    index: usize,
    color: TerrainColor,
) -> Result<&mut WangColor, PropertyError> {
    usize::from(color.0)
        .checked_sub(1)
        .and_then(|i| set.colors.get_mut(i))
        .ok_or(PropertyError::NoSuchTerrain {
            set: index,
            color: color.0,
        })
}

#[cfg(test)]
mod tests {
    use bt_core::{Color, WangSetType};
    use bt_test_data::fixtures::Atlas;

    use super::*;

    fn tileset() -> Tileset {
        let mut tileset = (*Atlas::builder().build_embedded().unwrap().tileset).clone();
        tileset.wang_sets.push(WangSet {
            name: "Ground".to_owned(),
            class: String::new(),
            kind: WangSetType::Corner,
            tile: None,
            colors: vec![WangColor {
                name: "Sand".to_owned(),
                class: String::new(),
                color: Color::rgb(1, 2, 3),
                tile: None,
                probability: 1.0,
                properties: Default::default(),
            }],
            tiles: Default::default(),
            properties: Default::default(),
        });
        tileset
    }

    #[test]
    fn a_set_and_a_terrain_show_and_change_tileds_attributes() {
        let mut tileset = tileset();
        let set_rows: Vec<Attribute> = terrain_set_attributes(&tileset, 0)
            .unwrap()
            .iter()
            .map(|row| row.attribute)
            .collect();
        assert_eq!(set_rows, SET_ORDER);
        let terrain = TerrainColor(1);
        assert!(
            set_terrain_set_attribute(
                &mut tileset,
                0,
                Attribute::TerrainType,
                &AttributeValue::TerrainType(WangSetType::Mixed)
            )
            .unwrap()
        );
        assert_eq!(tileset.wang_sets[0].kind, WangSetType::Mixed);
        assert!(
            set_terrain_attribute(
                &mut tileset,
                0,
                terrain,
                Attribute::Probability,
                &AttributeValue::Number(0.5)
            )
            .unwrap()
        );
        assert_eq!(tileset.wang_sets[0].colors[0].probability, 0.5);
        assert!(
            set_terrain_attribute(
                &mut tileset,
                0,
                terrain,
                Attribute::Color,
                &AttributeValue::Color(None)
            )
            .is_err(),
            "a terrain keeps a colour"
        );
        assert!(terrain_attributes(&tileset, 0, TerrainColor(2)).is_err());
        assert!(terrain_set_attributes(&tileset, 1).is_err());
    }
}
