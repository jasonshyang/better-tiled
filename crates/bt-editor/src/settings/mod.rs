//! The preferences and the editor's own folder.

mod migrate;
#[allow(
    clippy::module_inception,
    reason = "the settings module holds the settings"
)]
mod settings;
mod storage;

pub use migrate::MigrateError;
pub use settings::{
    Behaviour, General, Interface, MAX_FINE_GRID_DIVISIONS, MajorGrid, ObjectSelection,
    SETTINGS_VERSION, SettingColor, Settings, Snapping, Theme,
};
pub use storage::{SettingsError, data_folder};
