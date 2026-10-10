//! The application: the window's state, its frame, and the chrome around the canvas (menu bar,
//! tab strip, status bar).

mod effects;
mod frame;
mod menu;
#[cfg(test)]
mod properties_tests;
mod startup;
mod status_bar;
mod tabs;
#[cfg(test)]
mod tests;
mod toolbar;

pub(crate) use frame::App;
pub(crate) use startup::Startup;
