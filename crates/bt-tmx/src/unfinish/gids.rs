//! Global ids for writing: each tileset keeps the first gid it was read with while those still
//! fit, else gets Tiled's running sum.

use bt_core::{Cell, TileId, TilesetRef};

use crate::gid::join;

/// What a map's data uses of one tileset.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct TilesetUse {
    /// The highest tile id that written data names.
    pub(crate) highest_tile: Option<TileId>,
    /// Whether template instances take tiles from it without naming them.
    pub(crate) inherited: bool,
}

/// The first gid written for each tileset, `None` for tilesets not written (those only template
/// instances take tiles from).
pub(crate) struct FirstGids {
    firsts: Vec<Option<u32>>,
    /// Cells naming tilesets that are not written.
    pub(crate) unnamed: usize,
    /// The tilesets whose first gid had to change.
    pub(crate) renumbered: Vec<Renumbering>,
}

/// A tileset whose first gid had to change.
pub(crate) struct Renumbering {
    /// The tileset's name.
    pub(crate) name: String,
    /// The first gid it was read with.
    pub(crate) from: u32,
    /// The one it is written with.
    pub(crate) to: u32,
}

impl FirstGids {
    /// Assigns first gids: a tileset keeps its hint when it is not below the end of the tiles
    /// the previous one uses; otherwise it starts after the previous tileset's whole range
    /// (`max(next tile id, tile count, used span)`, Tiled's running sum), and the change is
    /// noted.
    pub(crate) fn assign(tilesets: &[TilesetRef], uses: &[TilesetUse]) -> FirstGids {
        let mut firsts = Vec::with_capacity(tilesets.len());
        let mut renumbered = Vec::new();
        let mut lowest_allowed = 1u32;
        let mut after_previous = 1u32;
        for (tileset, use_) in tilesets
            .iter()
            .zip(uses.iter().chain(std::iter::repeat(&TilesetUse::default())))
        {
            if use_.inherited && use_.highest_tile.is_none() && tileset.first_gid.is_none() {
                firsts.push(None);
                continue;
            }
            let used_span = use_.highest_tile.map_or(1, |tile| tile.0.saturating_add(1));
            let full_span = tileset
                .tileset
                .next_tile_id
                .max(tileset.tileset.tile_count())
                .max(used_span);
            let first = match tileset.first_gid {
                Some(hint) if hint >= lowest_allowed && hint > 0 => hint,
                hint => {
                    let first = after_previous.max(lowest_allowed);
                    if let Some(from) = hint {
                        renumbered.push(Renumbering {
                            name: tileset.tileset.name.clone(),
                            from,
                            to: first,
                        });
                    }
                    first
                }
            };
            firsts.push(Some(first));
            lowest_allowed = first.saturating_add(used_span);
            after_previous = first.saturating_add(full_span);
        }
        FirstGids {
            firsts,
            unnamed: 0,
            renumbered,
        }
    }

    /// First gid 1 for the tileset in `slot` alone, as a template's tileset is written.
    pub(crate) fn only(slot: usize, count: usize) -> FirstGids {
        FirstGids {
            firsts: (0..count).map(|i| (i == slot).then_some(1)).collect(),
            unnamed: 0,
            renumbered: Vec::new(),
        }
    }

    /// The first gid written for the tileset at `index`.
    pub(crate) fn first_of(&self, index: usize) -> Option<u32> {
        self.firsts.get(index).copied().flatten()
    }

    /// The global id of a cell; 0 (counted) for one whose tileset is not written.
    pub(crate) fn gid_of(&mut self, cell: Cell) -> u32 {
        let Some(slot) = cell.slot() else {
            return 0;
        };
        match self.first_of(slot.index()) {
            Some(first) => join(first.saturating_add(cell.tile().0), cell.flip()),
            None => {
                self.unnamed += 1;
                0
            }
        }
    }
}
