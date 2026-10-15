//! Quitting asks about unsaved work, whichever way it is asked for. The window's close button
//! and the Quit command go through the frame loop. On macOS the app menu's Quit, the Dock's and a
//! logout call AppKit's `terminate:`, which asks the application's delegate
//! `applicationShouldTerminate:` and quits at once when the delegate has no answer. The delegate
//! is winit's, which has none and must stay winit's (winit stops at any other), so the answer is
//! a method added to winit's delegate class at start: quit now when nothing is unsaved, and
//! otherwise cancel the system's quit and have the frame loop run the Quit command, whose
//! question decides. A logout is interrupted until the question is answered.
//!
//! Adding a method to an Objective-C class tells the runtime that a function handles a message
//! with the given argument types, which the compiler cannot check; it is the workspace's one
//! `unsafe` code (`docs/decisions.md`): the application's crate denies `unsafe_code` where the
//! others forbid it, and allows it in one function here.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// How a quit the system asks for is answered.
#[cfg(any(target_os = "macos", test))]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum SystemQuit {
    /// Nothing would be lost: the application quits.
    Proceed,
    /// Something is unsaved: the system's quit is cancelled and the Quit command asks.
    Cancel,
}

/// What the frame loop and a quit the system asks for share.
pub(crate) struct QuitGuard {
    unsaved: AtomicBool,
    asked: AtomicBool,
    #[cfg(any(target_os = "macos", test))]
    waker: egui::Context,
}

impl QuitGuard {
    /// A guard that wakes `waker` when it holds a quit back.
    pub(crate) fn new(waker: egui::Context) -> Arc<QuitGuard> {
        #[cfg(not(any(target_os = "macos", test)))]
        drop(waker);
        Arc::new(QuitGuard {
            unsaved: AtomicBool::new(false),
            asked: AtomicBool::new(false),
            #[cfg(any(target_os = "macos", test))]
            waker,
        })
    }

    /// Records whether a quit now would lose unsaved work (the frame loop says every frame).
    pub(crate) fn set_unsaved(&self, unsaved: bool) {
        self.unsaved.store(unsaved, Ordering::Relaxed);
    }

    /// Answers a quit the system asks for; one held back waits for the frame loop to ask.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn answer_system_quit(&self) -> SystemQuit {
        if !self.unsaved.load(Ordering::Relaxed) {
            log::debug!("the system asks to quit, and nothing is unsaved");
            return SystemQuit::Proceed;
        }
        log::debug!("the system asks to quit: held back to ask about unsaved work");
        self.asked.store(true, Ordering::Relaxed);
        self.waker.request_repaint();
        SystemQuit::Cancel
    }

    /// Whether a quit was held back since the last call.
    pub(crate) fn take_asked(&self) -> bool {
        self.asked.swap(false, Ordering::Relaxed)
    }
}

/// Has the system's quit answered through `guard` from now on (macOS). Elsewhere the window's
/// close request is the only way to quit, and the frame loop already holds it back.
pub(crate) fn install_system_quit_guard(guard: Arc<QuitGuard>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return appkit::install(guard);
    #[cfg(not(target_os = "macos"))]
    {
        drop(guard);
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod appkit {
    use std::ffi::CStr;
    use std::sync::{Arc, OnceLock};

    use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
    use objc2::{MainThreadMarker, ffi, sel};
    use objc2_app_kit::NSApplication;

    use super::{QuitGuard, SystemQuit};

    /// `NSTerminateCancel`, an `NSApplicationTerminateReply`.
    const TERMINATE_CANCEL: usize = 0;
    /// `NSTerminateNow`.
    const TERMINATE_NOW: usize = 1;

    /// The added method's types: it returns an `NSUInteger` and takes the receiver, the
    /// selector and the sender.
    const TYPES: &CStr = c"Q@:@";

    /// The added method's signature.
    type ShouldTerminate = extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize;

    /// The guard the added method answers with: AppKit calls it with nothing of the window's.
    static GUARD: OnceLock<Arc<QuitGuard>> = OnceLock::new();

    /// `-applicationShouldTerminate:` of winit's delegate. It reads none of its arguments (the
    /// sender may be nil) and does nothing that can fail.
    extern "C-unwind" fn should_terminate(
        _delegate: *mut AnyObject,
        _selector: Sel,
        _sender: *mut AnyObject,
    ) -> usize {
        match GUARD.get().map(|guard| guard.answer_system_quit()) {
            Some(SystemQuit::Cancel) => TERMINATE_CANCEL,
            Some(SystemQuit::Proceed) | None => TERMINATE_NOW,
        }
    }

    pub(super) fn install(guard: Arc<QuitGuard>) -> Result<(), String> {
        let main_thread = MainThreadMarker::new()
            .ok_or_else(|| "the window was not made on the main thread".to_owned())?;
        let delegate = NSApplication::sharedApplication(main_thread)
            .delegate()
            .ok_or_else(|| "the application has no delegate".to_owned())?;
        let class = AsRef::<AnyObject>::as_ref(&*delegate).class();
        let name = sel!(applicationShouldTerminate:);
        if class.instance_method(name).is_some() {
            return Err("the application's delegate already answers it".to_owned());
        }
        GUARD
            .set(guard)
            .map_err(|_| "it was already set up".to_owned())?;
        if add_should_terminate(class, name) {
            Ok(())
        } else {
            Err("the Objective-C runtime refused the method".to_owned())
        }
    }

    /// Adds [`should_terminate`] to `class` as the method `name`; whether the runtime took it.
    #[allow(
        unsafe_code,
        reason = "the one unsafe code of the workspace, reviewed: the compiler cannot check that a function handles an Objective-C message"
    )]
    fn add_should_terminate(class: &'static AnyClass, name: Sel) -> bool {
        // SAFETY: `transmute` only erases `should_terminate`'s signature into the runtime's
        // untyped `Imp`, which the runtime calls with the signature `TYPES` states: an object
        // pointer (the delegate), the selector and an object pointer (the sender), returning an
        // `NSUInteger`, which is `usize` on every macOS target; `should_terminate` has exactly
        // that signature. `class_addMethod` gets a class (classes live as long as the process),
        // the selector, that function and a NUL-terminated C string the runtime copies.
        unsafe {
            let imp = std::mem::transmute::<ShouldTerminate, Imp>(should_terminate);
            ffi::class_addMethod(
                std::ptr::from_ref(class).cast_mut(),
                name,
                imp,
                TYPES.as_ptr(),
            )
            .as_bool()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_system_quit_is_held_back_while_something_is_unsaved() {
        let guard = QuitGuard::new(egui::Context::default());
        assert_eq!(guard.answer_system_quit(), SystemQuit::Proceed);
        assert!(!guard.take_asked(), "nothing unsaved: nothing to ask");
        guard.set_unsaved(true);
        assert_eq!(guard.answer_system_quit(), SystemQuit::Cancel);
        assert!(guard.take_asked() && !guard.take_asked(), "asked once");
    }

    #[test]
    fn the_lints_are_the_workspaces_but_for_unsafe_code() {
        let read = |path: &str| -> toml::Table {
            let text = std::fs::read_to_string(path).unwrap();
            toml::from_str(&text).unwrap()
        };
        let workspace = read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml"));
        let app = read(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
        let mut expected = workspace["workspace"]["lints"].clone();
        expected["rust"]["unsafe_code"] = toml::Value::from("deny");
        assert_eq!(app["lints"], expected);
    }
}
