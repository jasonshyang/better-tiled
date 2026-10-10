//! The open assets.

mod asset;
mod asset_store;

pub use asset::{Asset, AssetId, AssetKind, OnDisk, Root};
pub use asset_store::{AssetStore, NewAsset};
