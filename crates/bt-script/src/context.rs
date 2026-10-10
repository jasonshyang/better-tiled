//! What a script's names and shapes refer to that the map does not hold (`10-agents.md`, §5,
//! §6.1): what is selected in the editor (`selection`, `selected`), the objects the operation
//! before made (`last`), and the seed of the operation's randomness (`rough`). The caller gives
//! it: `bt` from the editor's state and `--seed`, the runner operation by operation.

use bt_core::ObjectId;
use glam::IVec2;

use crate::error::Error;

/// What the caller knows that the map does not.
#[derive(Clone, Debug, PartialEq)]
pub struct Context {
    seed: u64,
    selection: Option<Selection>,
    last: Vec<ObjectId>,
}

/// What is selected in the editor.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    /// The selected cells.
    pub cells: Vec<IVec2>,
    /// The selected objects.
    pub objects: Vec<ObjectId>,
}

impl Context {
    /// A context with `seed`, for a map that is not open in the editor and before any operation
    /// made objects.
    pub fn new(seed: u64) -> Context {
        Context {
            seed,
            selection: None,
            last: Vec::new(),
        }
    }

    /// This context for a map open in the editor with `selection` selected.
    pub fn with_selection(mut self, selection: Selection) -> Context {
        self.selection = Some(selection);
        self
    }

    /// This context with `last` standing for `objects`.
    pub fn with_last(mut self, objects: Vec<ObjectId>) -> Context {
        self.last = objects;
        self
    }

    /// Sets the seed, for the next operation.
    pub fn set_seed(&mut self, seed: u64) {
        self.seed = seed;
    }

    /// Sets the objects `last` stands for: what the operation that just ran made.
    pub fn set_last(&mut self, objects: Vec<ObjectId>) {
        self.last = objects;
    }

    /// The seed of the operation's randomness.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// The cells selected in the editor (`selection`); an error when the map is not open there
    /// or no cells are selected.
    pub fn selected_cells(&self) -> Result<&[IVec2], Error> {
        let selection = self
            .selection
            .as_ref()
            .ok_or(Error::NotInEditor { word: "selection" })?;
        if selection.cells.is_empty() {
            return Err(Error::NothingSelected {
                word: "selection",
                what: "cells",
            });
        }
        Ok(&selection.cells)
    }

    /// The objects selected in the editor (`selected`); an error when the map is not open there
    /// or no objects are selected.
    pub fn selected_objects(&self) -> Result<&[ObjectId], Error> {
        let selection = self
            .selection
            .as_ref()
            .ok_or(Error::NotInEditor { word: "selected" })?;
        if selection.objects.is_empty() {
            return Err(Error::NothingSelected {
                word: "selected",
                what: "objects",
            });
        }
        Ok(&selection.objects)
    }

    /// The objects `last` stands for; an error when no operation before made any.
    pub fn last_objects(&self) -> Result<&[ObjectId], Error> {
        if self.last.is_empty() {
            return Err(Error::NothingLast);
        }
        Ok(&self.last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_editors_selection_is_there_only_when_the_map_is_open_in_it() {
        let closed = Context::new(7);
        assert_eq!(closed.seed(), 7);
        assert_eq!(
            closed.selected_cells(),
            Err(Error::NotInEditor { word: "selection" })
        );
        assert_eq!(closed.last_objects(), Err(Error::NothingLast));
        let open = Context::new(7).with_selection(Selection {
            cells: vec![IVec2::new(1, 2)],
            objects: Vec::new(),
        });
        assert_eq!(open.selected_cells(), Ok(&[IVec2::new(1, 2)][..]));
        assert_eq!(
            open.selected_objects().unwrap_err().to_string(),
            "`selected`: no objects are selected in the editor"
        );
        let mut running = open.with_last(vec![ObjectId(4)]);
        assert_eq!(running.last_objects(), Ok(&[ObjectId(4)][..]));
        running.set_last(vec![ObjectId(5), ObjectId(6)]);
        running.set_seed(9);
        assert_eq!(running.last_objects().map(<[ObjectId]>::len), Ok(2));
        assert_eq!(running.seed(), 9);
    }
}
