//! The tilesets of a rule set, each held once with the rule maps that use it. Rules name tiles
//! by their place in this table, so they are compiled once whatever map they run on; a run binds
//! the table to the map's tilesets (`engine::bind`) the way Tiled's `unifyTilesets` does.

use bt_core::{Flip, TileId, TilesetRef};

/// A tileset of the rule set.
#[derive(Clone, Debug)]
pub(crate) struct TableEntry {
    /// The tileset as the first rule map using it refers to it: the value, shared with every
    /// map read through the same loader, and its file (`None` when the rule map embeds it).
    pub(crate) reference: TilesetRef,
    /// The rule maps that use it, by their index in the set.
    pub(crate) rule_maps: Vec<usize>,
}

impl TableEntry {
    /// The file's path, or the tileset's name for one a rule map embeds.
    pub(crate) fn label(&self) -> String {
        match &self.reference.file {
            Some(file) => file.path.display().to_string(),
            None => format!("the embedded tileset “{}”", self.reference.tileset.name),
        }
    }
}

/// A tile in the rule set's numbering of tilesets.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TableTile {
    /// The tileset's index in the table.
    pub(crate) tileset: usize,
    /// The tile.
    pub(crate) tile: TileId,
    /// The flips, as the rule map has them.
    pub(crate) flip: Flip,
}

/// The tilesets of a rule set, in the order rule maps first use them.
#[derive(Clone, Debug, Default)]
pub(crate) struct TilesetTable {
    entries: Vec<TableEntry>,
}

impl TilesetTable {
    /// An empty table.
    pub(crate) fn new() -> TilesetTable {
        TilesetTable::default()
    }

    /// The index of the tileset `reference` refers to, adding it when the table does not hold
    /// it yet; `rule_map` is recorded as one that uses it. Only the same value or the same file
    /// is the same tileset here: similar tilesets are unified with the map's when a run binds
    /// them, which is when Tiled unifies them.
    pub(crate) fn insert_tileset(&mut self, reference: &TilesetRef, rule_map: usize) -> usize {
        if let Some(index) = self
            .entries
            .iter()
            .position(|e| e.reference.is_same_tileset_as(reference))
        {
            if let Some(entry) = self.entries.get_mut(index)
                && !entry.rule_maps.contains(&rule_map)
            {
                entry.rule_maps.push(rule_map);
            }
            return index;
        }
        self.entries.push(TableEntry {
            reference: reference.clone(),
            rule_maps: vec![rule_map],
        });
        self.entries.len() - 1
    }

    /// The tileset at `index`.
    pub(crate) fn get(&self, index: usize) -> Option<&TableEntry> {
        self.entries.get(index)
    }

    /// The tilesets, in the order rule maps first use them.
    pub(crate) fn entries(&self) -> &[TableEntry] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{FileRef, Tileset};

    use super::*;

    fn tileset(name: &str) -> Arc<Tileset> {
        Arc::new(Tileset {
            name: name.into(),
            ..Tileset::default()
        })
    }

    #[test]
    fn a_tileset_is_held_once_with_the_rule_maps_using_it() {
        let mut table = TilesetTable::new();
        let grass = tileset("grass");
        let a = table.insert_tileset(&TilesetRef::embedded(Arc::clone(&grass)), 0);
        let b = table.insert_tileset(&TilesetRef::embedded(Arc::clone(&grass)), 1);
        assert_eq!(a, b);
        // The same file read twice is the same tileset.
        let file = |value: Arc<Tileset>| TilesetRef {
            file: Some(FileRef::new("water.tsx", "/rules/water.tsx")),
            first_gid: Some(1),
            tileset: value,
        };
        let c = table.insert_tileset(&file(tileset("water")), 1);
        let d = table.insert_tileset(&file(tileset("water")), 2);
        assert_eq!(c, d);
        // A copy embedded elsewhere is another tileset until a run binds it.
        let e = table.insert_tileset(&TilesetRef::embedded(tileset("grass")), 2);
        assert_ne!(a, e);
        assert_eq!(table.entries().len(), 3);
        assert_eq!(table.get(a).map(|e| e.rule_maps.clone()), Some(vec![0, 1]));
        assert_eq!(
            table.get(c).map(TableEntry::label),
            Some("/rules/water.tsx".into())
        );
        assert_eq!(
            table.get(e).map(TableEntry::label),
            Some("the embedded tileset “grass”".into())
        );
    }
}
