//! The Godot version a project is made for, read from `config/features` in its project.godot
//! (`config/features=PackedStringArray("4.7", "Forward Plus")`): what its scenes may use.

use std::path::Path;

use super::read;

/// Godot's major and minor version.
pub(super) type Version = (u32, u32);

/// The first Godot whose scenes carry node `unique_id`s and `parent_id_path`
/// (godotengine/godot#106837).
pub(super) const UNIQUE_IDS: Version = (4, 6);

/// Whether scenes in the project at `project` (the folder holding project.godot) get node
/// `unique_id`s. A project that names no version gets none: names alone still carry identity.
pub(super) fn unique_ids(project: &Path) -> bool {
    std::fs::read_to_string(project.join("project.godot"))
        .ok()
        .and_then(|text| features(&text))
        .is_some_and(|v| v >= UNIQUE_IDS)
}

/// The first Godot that merges the collision shapes of a TileMapLayer's physics quadrant
/// (`physics_quadrant_size` cells square) into one body.
pub(super) const PHYSICS_QUADRANTS: Version = (4, 5);

/// Whether the Godot of the project at `project` merges physics quadrants. A project that names
/// no version is taken to: a Godot without the setting passes over it without a word, as 4.7.2
/// does with a setting a node doesn't have.
pub(super) fn merges_physics_quadrants(project: &Path) -> bool {
    std::fs::read_to_string(project.join("project.godot"))
        .ok()
        .and_then(|text| features(&text))
        .is_none_or(|v| v >= PHYSICS_QUADRANTS)
}

/// The version `config/features` names in the `[application]` section of a project.godot.
pub(super) fn features(project_godot: &str) -> Option<Version> {
    let mut section = "";
    for line in project_godot.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim();
        } else if section == "application"
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "config/features"
        {
            return read::strings(value)?
                .iter()
                .find_map(|feature| version(feature));
        }
    }
    None
}

/// `4.7` (or `4.7.2`) as (4, 7); the other features (`Forward Plus`, `C#`, …) as nothing.
fn version(feature: &str) -> Option<Version> {
    let mut parts = feature.split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}
