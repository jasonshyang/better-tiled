//! How a script refers to what a map holds (`10-agents.md`, §5): content by name or tiles by
//! id, layers by name or id, objects by id, name or type, or as the editor's selection or what
//! the previous operation made. These are syntax only; `crate::names` resolves them.

/// Map content as a script names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Named {
    /// By name, slugged when looked up: a terrain, a tile group, a sprite, a material, a group,
    /// a piece or a defined area. Which kinds count is the operation's.
    Name(String),
    /// Tiles by id, which together are one ground: any of them.
    Tiles(TileIds),
}

/// Tiles by id: `TILESET:ID`, `TILESET:A-B`, and after one of those more ids or ranges of the
/// same tileset (`terrain:1-4,9`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileIds {
    /// The tileset, by name, slugged when looked up.
    pub tileset: String,
    /// The ids, in the order written.
    pub ranges: Vec<IdRange>,
}

/// Tile ids from `first` to `last`, both included: `ID` is a range of one.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct IdRange {
    /// The first id.
    pub first: u32,
    /// The last id.
    pub last: u32,
}

/// A layer as a script names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayerRef {
    /// By name, slugged when looked up; several layers of one name must be told apart by id.
    Name(String),
    /// `#ID`.
    Id(u32),
}

/// Objects as a script names them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectRef {
    /// `#ID`.
    Id(u32),
    /// By name when one object has it, else by type when exactly one object has that type.
    Name(String),
    /// `selected`: the objects selected in the editor.
    Selected,
    /// `last`: the objects the previous operation that made objects made.
    Last,
}

/// Map content written as a comma list: `TILESET:ID` and `TILESET:A-B` start tiles by id, the
/// bare ids and ranges after one of them add to it, and anything else is a name.
pub(crate) fn named_items(items: &[String]) -> Result<Vec<Named>, String> {
    let mut named: Vec<Named> = Vec::new();
    for item in items {
        if let Some((tileset, ids)) = item.rsplit_once(':') {
            if tileset.is_empty() {
                return Err(format!("“{item}” names no tileset: write TILESET:ID"));
            }
            named.push(Named::Tiles(TileIds {
                tileset: tileset.to_owned(),
                ranges: vec![id_range(item, ids)?],
            }));
            continue;
        }
        if item.starts_with(|c: char| c.is_ascii_digit()) {
            match named.last_mut() {
                Some(Named::Tiles(tiles)) => tiles.ranges.push(id_range(item, item)?),
                _ => {
                    return Err(format!(
                        "“{item}” is an id without a tileset: write TILESET:ID, e.g. terrain:{item}"
                    ));
                }
            }
            continue;
        }
        named.push(Named::Name(item.clone()));
    }
    Ok(named)
}

/// `ID` or `A-B`, written in `item`.
fn id_range(item: &str, ids: &str) -> Result<IdRange, String> {
    let id = |text: &str| {
        text.trim().parse::<u32>().map_err(|_| {
            format!("“{item}” is not tiles: write TILESET:ID or TILESET:A-B with whole numbers")
        })
    };
    match ids.split_once('-') {
        Some((first, last)) => {
            let (first, last) = (id(first)?, id(last)?);
            if first > last {
                return Err(format!(
                    "“{item}” runs backwards: write the lower id first, {last}-{first}"
                ));
            }
            Ok(IdRange { first, last })
        }
        None => {
            let id = id(ids)?;
            Ok(IdRange {
                first: id,
                last: id,
            })
        }
    }
}

/// Objects as written: `#ID`, `selected`, `last`, else a name or a type.
pub(crate) fn object_ref_of(word: &str) -> ObjectRef {
    match word {
        "selected" => ObjectRef::Selected,
        "last" => ObjectRef::Last,
        _ => match word.strip_prefix('#').and_then(|id| id.parse().ok()) {
            Some(id) => ObjectRef::Id(id),
            None => ObjectRef::Name(word.to_owned()),
        },
    }
}

/// A layer as written: `#ID`, else a name.
pub(crate) fn layer_ref_of(word: &str) -> LayerRef {
    match word.strip_prefix('#').and_then(|id| id.parse().ok()) {
        Some(id) => LayerRef::Id(id),
        None => LayerRef::Name(word.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(text: &str) -> Vec<String> {
        text.split(',').map(str::to_owned).collect()
    }

    #[test]
    fn ids_after_a_tileset_add_to_its_tiles() {
        assert_eq!(
            named_items(&items("terrain:1-4,9,water,hex-mini:10")).unwrap(),
            [
                Named::Tiles(TileIds {
                    tileset: "terrain".into(),
                    ranges: vec![IdRange { first: 1, last: 4 }, IdRange { first: 9, last: 9 }],
                }),
                Named::Name("water".into()),
                Named::Tiles(TileIds {
                    tileset: "hex-mini".into(),
                    ranges: vec![IdRange {
                        first: 10,
                        last: 10
                    }],
                }),
            ]
        );
        assert!(
            named_items(&items("9"))
                .unwrap_err()
                .contains("without a tileset")
        );
        assert!(
            named_items(&items("t:5-2"))
                .unwrap_err()
                .contains("backwards")
        );
        assert!(
            named_items(&items(":5"))
                .unwrap_err()
                .contains("names no tileset")
        );
    }

    #[test]
    fn objects_and_layers_by_id_or_name() {
        assert_eq!(object_ref_of("#12"), ObjectRef::Id(12));
        assert_eq!(object_ref_of("selected"), ObjectRef::Selected);
        assert_eq!(object_ref_of("last"), ObjectRef::Last);
        assert_eq!(object_ref_of("entry"), ObjectRef::Name("entry".into()));
        assert_eq!(layer_ref_of("#3"), LayerRef::Id(3));
        assert_eq!(layer_ref_of("ground"), LayerRef::Name("ground".into()));
    }
}
