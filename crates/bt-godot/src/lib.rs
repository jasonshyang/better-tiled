//! Godot text resources: syntax validation and serialization primitives.
#[allow(dead_code, reason = "shared syntax primitives for the scene exporter")]
mod read;
#[allow(dead_code, reason = "shared value serialization for scene generation")]
mod text;
mod resource;
pub use resource::validate_resource_text;
