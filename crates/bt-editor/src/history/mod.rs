//! Transactions and the documents' histories.

mod apply;
mod gesture;
#[allow(
    clippy::module_inception,
    reason = "the history module holds the history"
)]
mod history;
mod transaction;
mod tx;

#[cfg(test)]
mod tests;

pub(crate) use gesture::Gesture;

pub use history::{DEFAULT_HISTORY_LIMIT, History, HistoryEntry};
pub use transaction::{AssetChange, AutoMapChange, MergeKey, SelectionChange, Transaction};
pub use tx::Tx;
