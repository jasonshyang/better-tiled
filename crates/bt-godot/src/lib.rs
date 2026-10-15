//! Godot 4.3+ scene export and its regeneration contract.
//! Scenes are planned in memory and changed user files are refused before writing.
//! See `docs/design/12-godot-export.md`.

mod animation;
mod build;
mod code;
mod collision;
mod contract;
mod conventions;
mod error;
mod export;
mod grid;
mod images;
mod inheritors;
mod layers;
mod names;
mod objects;
mod options;
mod plan;
mod project_scan;
mod read;
mod report;
mod request;
mod resolve;
mod resource;
mod scene;
pub mod setup;
mod shared;
pub mod starter;
mod terrain;
mod text;
mod tiledata;
mod tileset;
mod validate;
mod values;
pub mod verify;
mod version;

pub(crate) use build::Build;
pub use code::Code;
pub use contract::{FileStatus, Found, Refusal, Status, Summary, hand_made, way_out};
pub use error::Error;
pub use export::{
    AllExported, Exported, Exporter, Generated, SceneCopy, Scenes, export, export_all, export_map,
    generated_scenes, images_copied, scene_of, status,
};
pub(crate) use export::{RULE_TILES, project_root, res_path, resolved_path, scene_target};
pub use inheritors::LevelScene;
pub use options::Options;
pub use report::{Counts, Element, File, Note, Renamed, Report, Severity, State};
pub use request::ExportRequest;


pub use resource::validate_resource_text;
