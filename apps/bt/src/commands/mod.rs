//! The commands, one module per group: thin, arguments in and an outcome out
//! (`10-agents.md`, §3).

pub(crate) mod catalog;
pub(crate) mod check;
pub(crate) mod diff;
pub(crate) mod edit;
pub(crate) mod editor;
pub(crate) mod explain;
pub(crate) mod export;
pub(crate) mod know;
pub(crate) mod layers;
pub(crate) mod look;
#[cfg(test)]
pub(crate) mod testing;
pub(crate) mod undo;
pub(crate) mod view;
