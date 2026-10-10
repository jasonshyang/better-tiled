//! The quit key on macOS. The app menu's Quit item would end the run at once, unsaved work and
//! all; taking its key equivalent away lets ⌘Q reach the window, where the Quit command asks
//! about unsaved work as closing the window does. Choosing Quit from the menu, the Dock or a
//! logout still ends the run without asking: asking there needs the application delegate's
//! `applicationShouldTerminate:`, which cannot be added without `unsafe`. The window then
//! writes its last recovery snapshots on the way out, so the next start offers the work back.

/// Lets ⌘Q reach the window (macOS); nothing elsewhere, where the window's close request is
/// the only way to quit.
#[cfg(target_os = "macos")]
pub(crate) fn route_quit_key_to_window() {
    use objc2::{MainThreadMarker, sel};
    use objc2_app_kit::NSApplication;
    use objc2_foundation::NSString;

    let Some(main_thread) = MainThreadMarker::new() else {
        log::warn!("the quit key stays the app menu's: the window was not made on the main thread");
        return;
    };
    let Some(menu) = NSApplication::sharedApplication(main_thread).mainMenu() else {
        log::warn!("the quit key stays as it is: there is no app menu");
        return;
    };
    let no_key = NSString::new();
    for index in 0..menu.numberOfItems() {
        let Some(submenu) = menu.itemAtIndex(index).and_then(|item| item.submenu()) else {
            continue;
        };
        for entry in 0..submenu.numberOfItems() {
            if let Some(item) = submenu.itemAtIndex(entry)
                && item.action() == Some(sel!(terminate:))
            {
                item.setKeyEquivalent(&no_key);
            }
        }
    }
}

/// Lets ⌘Q reach the window (macOS); nothing elsewhere, where the window's close request is
/// the only way to quit.
#[cfg(not(target_os = "macos"))]
pub(crate) fn route_quit_key_to_window() {}
