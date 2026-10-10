//! What a rule map's tile means: Tiled's `MatchType`, read from the tile's `MatchType` property,
//! or its class's when the tile has none (Tiled's `resolvedProperty`). That is the one source of
//! truth: the built-in rule tileset's tiles are special only because their properties say so.

use std::collections::HashMap;

use bt_core::{Cell, ClassUsage, CustomTypes, Property, TileId, Tileset, Value};

use crate::rulemap::tilesets::{TableTile, TilesetTable};

/// The meaning of a tile in a rule map.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum MatchType {
    /// An ordinary tile: matched or written as it is.
    Tile,
    /// The empty cell: required on input, erased on output.
    Empty,
    /// Any tile: on input, the cell must not be empty.
    NonEmpty,
    /// Any tile but those the rule uses on the same target layer.
    Other,
    /// Swaps "any of" and "none of" at its position.
    Negate,
    /// No condition; joins the parts of a rule.
    Ignore,
}

/// A tile's meaning with where it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedMatchType {
    /// What the tile means; `None` when the tileset has no such tile (Tiled's `Unknown`, which
    /// rules treat like an empty cell).
    pub(crate) match_type: Option<MatchType>,
    /// A `MatchType` that is none of Tiled's names, which makes the tile ordinary.
    pub(crate) unknown_name: Option<String>,
    /// The tile's class, when the meaning came from it or could come from it: the tile has a
    /// class and no `MatchType` of its own.
    pub(crate) class: Option<String>,
}

/// The `MatchType` name a property holds: text, or a value of a custom enum by its name.
fn name_of(property: &Property, types: &CustomTypes) -> String {
    match &property.value {
        Value::String(text) => text.clone(),
        Value::Int(index) => property
            .custom_type
            .as_deref()
            .and_then(|name| types.enum_type(name))
            .and_then(|enumeration| enumeration.to_name(*index))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// What tile `id` of `tileset` means, with `types` resolving tile classes.
pub(crate) fn resolve_match_type(
    tileset: &Tileset,
    id: TileId,
    types: &CustomTypes,
) -> ResolvedMatchType {
    if !tileset.has_tile(id) {
        return ResolvedMatchType {
            match_type: None,
            unknown_name: None,
            class: None,
        };
    }
    let data = tileset.tile(id);
    let own = data.and_then(|d| d.properties.get("MatchType"));
    let class = data
        .map(|d| d.class.as_str())
        .filter(|class| own.is_none() && !class.is_empty());
    let from_class = class
        .and_then(|class| types.class_for(class, ClassUsage::TILE))
        .and_then(|class| class.members.get("MatchType"));
    let name = own
        .or(from_class)
        .map(|property| name_of(property, types))
        .unwrap_or_default();
    let (match_type, unknown_name) = match name.as_str() {
        "Empty" => (MatchType::Empty, None),
        "NonEmpty" => (MatchType::NonEmpty, None),
        "Other" => (MatchType::Other, None),
        "Negate" => (MatchType::Negate, None),
        "Ignore" => (MatchType::Ignore, None),
        "" | "Tile" => (MatchType::Tile, None),
        _ => (MatchType::Tile, Some(name.clone())),
    };
    ResolvedMatchType {
        match_type: Some(match_type),
        unknown_name,
        class: class.map(str::to_owned),
    }
}

/// What a rule map's cell is, for compiling.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum RuleCell {
    /// No tile.
    Empty,
    /// A tile its tileset does not have (Tiled's `Unknown`, like an empty cell to the rules).
    Unknown(TableTile),
    /// A tile with its meaning.
    Typed(MatchType, TableTile),
}

/// The meaning of every tile of the rule set met so far, by tileset (in the table's numbering)
/// and tile.
#[derive(Clone, Debug, Default)]
pub(crate) struct MatchTypeCache {
    pub(crate) entries: HashMap<(usize, TileId), ResolvedMatchType>,
}

/// The meanings of one rule map's cells.
pub(crate) struct CellMeanings<'a> {
    /// The rule map's tilesets as indexes into the table, by slot.
    pub(crate) slots: &'a [usize],
    /// The rule set's tilesets.
    pub(crate) table: &'a TilesetTable,
    /// The project's custom types, which give tiles their classes' members.
    pub(crate) types: &'a CustomTypes,
    /// The meanings worked out so far.
    pub(crate) cache: &'a mut MatchTypeCache,
    /// The tiles met in this rule map whose `MatchType` is none of Tiled's names.
    pub(crate) unknown_names: Vec<UnknownName>,
}

/// A tile whose `MatchType` is none of Tiled's names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UnknownName {
    /// The tile, in the table's numbering.
    pub(crate) tileset: usize,
    /// The tile.
    pub(crate) tile: TileId,
    /// The name it has.
    pub(crate) name: String,
}

impl CellMeanings<'_> {
    /// What `cell`, a cell of the rule map, is.
    pub(crate) fn of(&mut self, cell: Cell) -> RuleCell {
        let Some(tileset) = cell
            .slot()
            .and_then(|slot| self.slots.get(slot.index()))
            .copied()
        else {
            return RuleCell::Empty;
        };
        let tile = TableTile {
            tileset,
            tile: cell.tile(),
            flip: cell.flip(),
        };
        let key = (tileset, cell.tile());
        let resolved = match self.cache.entries.get(&key) {
            Some(resolved) => resolved,
            None => {
                let Some(entry) = self.table.get(tileset) else {
                    return RuleCell::Empty;
                };
                let resolved =
                    resolve_match_type(&entry.reference.tileset, cell.tile(), self.types);
                self.cache.entries.entry(key).or_insert(resolved)
            }
        };
        if let Some(name) = &resolved.unknown_name
            && !self
                .unknown_names
                .iter()
                .any(|u| u.tileset == tileset && u.tile == cell.tile())
        {
            self.unknown_names.push(UnknownName {
                tileset,
                tile: cell.tile(),
                name: name.clone(),
            });
        }
        match resolved.match_type {
            Some(kind) => RuleCell::Typed(kind, tile),
            None => RuleCell::Unknown(tile),
        }
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{ClassType, CustomType, EnumType, Properties, TileData, TilesetImage};

    use super::*;

    fn tileset_with(tile: TileData) -> Tileset {
        let mut tileset = Tileset {
            tile_width: 16,
            tile_height: 16,
            image: TilesetImage::Collection,
            ..Tileset::default()
        };
        tileset.tiles.insert(TileId(0), tile);
        tileset
    }

    fn with_property(value: Value) -> TileData {
        let mut properties = Properties::new();
        properties.insert("MatchType".to_owned(), Property::new(value));
        TileData {
            properties,
            ..TileData::default()
        }
    }

    #[test]
    fn the_built_in_tiles_mean_what_their_properties_say() {
        let tiles = bt_tmx_free_builtin();
        let types = CustomTypes::new();
        let kinds: Vec<Option<MatchType>> = (0..5)
            .map(|id| resolve_match_type(&tiles, TileId(id), &types).match_type)
            .collect();
        assert_eq!(
            kinds,
            [
                Some(MatchType::Negate),
                Some(MatchType::Ignore),
                Some(MatchType::NonEmpty),
                Some(MatchType::Empty),
                Some(MatchType::Other)
            ]
        );
        assert_eq!(
            resolve_match_type(&tiles, TileId(9), &types).match_type,
            None
        );
    }

    /// The five tiles of Tiled's rule tileset, built here so the test needs no file reader.
    fn bt_tmx_free_builtin() -> Tileset {
        let mut tileset = tileset_with(with_property(Value::String("Negate".into())));
        for (id, name) in [(1, "Ignore"), (2, "NonEmpty"), (3, "Empty"), (4, "Other")] {
            tileset
                .tiles
                .insert(TileId(id), with_property(Value::String(name.into())));
        }
        tileset
    }

    #[test]
    fn names_are_case_sensitive_and_unknown_ones_are_kept() {
        let types = CustomTypes::new();
        let tileset = tileset_with(with_property(Value::String("empty".into())));
        let resolved = resolve_match_type(&tileset, TileId(0), &types);
        assert_eq!(resolved.match_type, Some(MatchType::Tile));
        assert_eq!(resolved.unknown_name.as_deref(), Some("empty"));
    }

    #[test]
    fn a_class_gives_the_match_type_when_the_tile_has_none() {
        let mut members = Properties::new();
        members.insert(
            "MatchType".to_owned(),
            Property::new(Value::String("NonEmpty".into())),
        );
        let mut types = CustomTypes::new();
        types.add(CustomType::class(
            "Solid",
            ClassType {
                members,
                use_as: ClassUsage::TILE,
                ..ClassType::default()
            },
        ));
        let tileset = tileset_with(TileData {
            class: "Solid".into(),
            ..TileData::default()
        });
        let resolved = resolve_match_type(&tileset, TileId(0), &types);
        assert_eq!(resolved.match_type, Some(MatchType::NonEmpty));
        assert_eq!(resolved.class.as_deref(), Some("Solid"));
        // Without the project's types the class means nothing.
        let plain = resolve_match_type(&tileset, TileId(0), &CustomTypes::new());
        assert_eq!(plain.match_type, Some(MatchType::Tile));
        assert_eq!(plain.class.as_deref(), Some("Solid"));
    }

    #[test]
    fn an_enum_value_counts_by_its_name() {
        let mut types = CustomTypes::new();
        types.add(CustomType::enumeration(
            "MatchKind",
            EnumType {
                values: vec!["Tile".into(), "Empty".into()],
                ..EnumType::default()
            },
        ));
        let tileset = tileset_with(with_property(Value::Int(1)));
        let mut typed = tileset.clone();
        if let Some(tile) = typed.tiles.get_mut(&TileId(0)) {
            tile.properties.insert(
                "MatchType".to_owned(),
                Property::typed(Value::Int(1), "MatchKind"),
            );
        }
        assert_eq!(
            resolve_match_type(&typed, TileId(0), &types).match_type,
            Some(MatchType::Empty)
        );
        assert_eq!(
            resolve_match_type(&tileset, TileId(0), &types).match_type,
            Some(MatchType::Tile),
            "a number without its enum is no name"
        );
    }
}
