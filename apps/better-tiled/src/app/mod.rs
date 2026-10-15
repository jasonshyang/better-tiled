//! The application: the window's state, its frame, and the chrome around the canvas (menu bar,
//! tab strip, status bar).

#[cfg(test)]
mod automap_tests;
mod commands;
#[cfg(test)]
mod commands_tests;
#[cfg(test)]
mod custom_types_tests;
mod effects;
mod frame;
mod godot;
mod images;
#[cfg(test)]
mod images_tests;
mod live;
#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod m3_tests;
mod menu;
#[cfg(test)]
mod properties_tests;
mod shortcuts;
mod startup;
mod status_bar;
mod tabs;
#[cfg(test)]
mod terrain_helpers_tests;
#[cfg(test)]
mod tests;
mod toolbar;

pub(crate) use frame::{App, PanelTab};
pub(crate) use startup::Startup;

#[cfg(test)]
mod godot_tests;

#[cfg(test)]
mod shortcuts_tests;
#[cfg(test)]
mod stamps_tests;

#[cfg(test)]
mod template_tests;

#[cfg(test)]
mod world_tests;

#[cfg(test)]
mod project_tests;
