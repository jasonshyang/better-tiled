//! Commands: the action table, chords and the keymap.

mod chord;
mod keymap;
mod table;

pub use chord::{Chord, Key, Modifiers};
pub use keymap::{Keymap, KeymapError};
pub use table::{ACTIONS, ActionId, ActionKind, ActionSpec, Context};
