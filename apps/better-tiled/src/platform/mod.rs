//! What differs between platforms.

mod quit;

#[cfg(test)]
pub(crate) use quit::SystemQuit;
pub(crate) use quit::{QuitGuard, install_system_quit_guard};
