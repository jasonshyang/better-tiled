//! The property names the export gives a meaning to (docs/design/12-godot-export.md). Tiled's names mean what Tiled's
//! Godot exporter makes of them; the names better-tiled adds start with `godot`, so they can't
//! collide with a game's own. A property the export uses is consumed: listed in the report, and
//! not written as metadata.
#![allow(
    dead_code,
    reason = "the conventions are taken up by W5-W7, W10, W14 and W16"
)]

use std::collections::BTreeSet;

use bt_core::{Properties, Property};

// Tiled's names (case-sensitive).

/// Map: the `res://….tres` the shared TileSet goes in.
pub(super) const TILESET_RES_PATH: &str = "tilesetResPath";
/// Any layer, a group (with its children), an object: left out.
pub(super) const NO_EXPORT: &str = "noExport";
/// Tile layer: left out, its tilesets still in the TileSet.
pub(super) const TILESET_ONLY: &str = "tilesetOnly";
/// Map, group, any layer: `y_sort_enabled`.
pub(super) const Y_SORT_ENABLED: &str = "ySortEnabled";
/// Group, any layer: `z_index`. On a tile it is custom data, as in Tiled.
pub(super) const Z_INDEX: &str = "zIndex";
/// Object (then its template, its tile, its class): the scene it is an instance of.
pub(super) const RES_PATH: &str = "resPath";
/// Tileset: deprecated in Tiled, ignored with a note.
pub(super) const EXPORT_ALTERNATES: &str = "exportAlternates";
/// Tile: custom data, as in Tiled.
pub(super) const Y_SORT_ORIGIN: &str = "ySortOrigin";

// better-tiled's names.

/// Tile layer, tile: `y_sort_origin`.
pub(super) const GODOT_Y_SORT_ORIGIN: &str = "godotYSortOrigin";
/// Tile, object: `z_index`.
pub(super) const GODOT_Z_INDEX: &str = "godotZIndex";
/// A tile's collision object: its physics layer (default 0).
pub(super) const GODOT_PHYSICS_LAYER: &str = "godotPhysicsLayer";
/// A tile's collision object: a one-way shape, and its margin.
pub(super) const GODOT_ONE_WAY: &str = "godotOneWay";
pub(super) const GODOT_ONE_WAY_MARGIN: &str = "godotOneWayMargin";
/// A tile's collision object: `false` keeps it out of physics (navigation or occlusion only).
pub(super) const GODOT_COLLISION: &str = "godotCollision";
/// A tile's collision object: a navigation polygon of that layer.
pub(super) const GODOT_NAVIGATION_LAYER: &str = "godotNavigationLayer";
/// A tile's collision object: an occluder of that layer.
pub(super) const GODOT_OCCLUSION_LAYER: &str = "godotOcclusionLayer";
/// Object, class: the node type of a shape without `resPath`.
pub(super) const GODOT_NODE: &str = "godotNode";
/// Object: its body's `collision_layer` and `collision_mask`.
pub(super) const GODOT_COLLISION_LAYER: &str = "godotCollisionLayer";
pub(super) const GODOT_COLLISION_MASK: &str = "godotCollisionMask";

/// The names an export used, for the report.
#[derive(Debug, Default)]
pub(super) struct Consumed {
    names: BTreeSet<&'static str>,
}

impl Consumed {
    /// The property `name` among `properties`, noted as used when it is there.
    pub fn take<'p>(
        &mut self,
        properties: &'p Properties,
        name: &'static str,
    ) -> Option<&'p Property> {
        let property = properties.get(name)?;
        self.names.insert(name);
        Some(property)
    }

    pub fn names(&self) -> BTreeSet<String> {
        self.names.iter().map(|n| (*n).to_owned()).collect()
    }
}
