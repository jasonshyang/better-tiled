//! The action table: every command declared once, with what menus and the palette show, when it
//! applies, and its default keys (Tiled's, where Tiled has them).

use super::chord::{Chord, Key, Modifiers};

/// A command.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ActionId {
    /// Open a file.
    Open,
    /// Save the current document.
    Save,
    /// Save the current document to another file.
    SaveAs,
    /// Save everything unsaved.
    SaveAll,
    /// Close the current document.
    Close,
    /// Quit.
    Quit,
    /// Undo in the current document.
    Undo,
    /// Redo in the current document.
    Redo,
    /// Zoom in.
    ZoomIn,
    /// Zoom out.
    ZoomOut,
    /// Zoom to 100 %.
    ZoomNormal,
    /// Fit the map in the view.
    FitInView,
    /// Show or hide the tile grid.
    ShowGrid,
    /// Tiled's Highlight Current Layer.
    HighlightCurrentLayer,
    /// The next tab.
    NextDocument,
    /// The previous tab.
    PreviousDocument,
}

/// When a command applies.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Context {
    /// Always.
    Always,
    /// With a document open.
    Document,
}

/// What a command is.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ActionKind {
    /// Runs and is done.
    Command,
    /// Switches something on or off (menus show a check).
    Toggle,
}

/// A command's declaration.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ActionSpec {
    /// Its id.
    pub id: ActionId,
    /// Its stable name, in settings and `.kms` files: `save_as`.
    pub name: &'static str,
    /// What menus show.
    pub label: &'static str,
    /// More words the palette matches.
    pub keywords: &'static str,
    /// When it applies.
    pub context: Context,
    /// Its default keys on Windows and Linux.
    pub keys: &'static [Chord],
    /// Its default keys on macOS.
    pub keys_mac: &'static [Chord],
    /// Command or toggle.
    pub kind: ActionKind,
    /// Whether it fires again while its key is held.
    pub repeats: bool,
    /// Whether a focused text field handles its key itself.
    pub text_edit: bool,
}

const fn cmd(c: char) -> Chord {
    Chord::with(Modifiers::COMMAND, Key::Char(c))
}

const fn cmd_shift(c: char) -> Chord {
    Chord::with(Modifiers::COMMAND_SHIFT, Key::Char(c))
}

const fn cmd_alt(c: char) -> Chord {
    Chord::with(Modifiers::COMMAND_ALT, Key::Char(c))
}

const fn command(
    id: ActionId,
    name: &'static str,
    label: &'static str,
    keywords: &'static str,
) -> ActionSpec {
    ActionSpec {
        id,
        name,
        label,
        keywords,
        context: Context::Always,
        keys: &[],
        keys_mac: &[],
        kind: ActionKind::Command,
        repeats: false,
        text_edit: false,
    }
}

const NEXT_TAB: Chord = Chord::with(
    Modifiers {
        command: true,
        ..Modifiers::NONE
    },
    Key::Tab,
);
const PREVIOUS_TAB: Chord = Chord::with(Modifiers::COMMAND_SHIFT, Key::Tab);
const MAC_NEXT_TAB: Chord = Chord::with(
    Modifiers {
        control: true,
        ..Modifiers::NONE
    },
    Key::Tab,
);
const MAC_PREVIOUS_TAB: Chord = Chord::with(
    Modifiers {
        control: true,
        shift: true,
        ..Modifiers::NONE
    },
    Key::Tab,
);

const OPEN: ActionSpec = ActionSpec {
    keys: &[cmd('o')],
    keys_mac: &[cmd('o')],
    ..command(ActionId::Open, "open", "Open…", "file load")
};

const SAVE: ActionSpec = ActionSpec {
    context: Context::Document,
    keys: &[cmd('s')],
    keys_mac: &[cmd('s')],
    ..command(ActionId::Save, "save", "Save", "file write")
};

const SAVE_AS: ActionSpec = ActionSpec {
    context: Context::Document,
    keys: &[cmd_shift('s')],
    keys_mac: &[cmd_shift('s')],
    ..command(ActionId::SaveAs, "save_as", "Save As…", "file write rename")
};

const SAVE_ALL: ActionSpec = ActionSpec {
    keys: &[cmd_alt('s')],
    keys_mac: &[cmd_alt('s')],
    ..command(
        ActionId::SaveAll,
        "save_all",
        "Save All",
        "file write every",
    )
};

const CLOSE: ActionSpec = ActionSpec {
    context: Context::Document,
    keys: &[cmd('w')],
    keys_mac: &[cmd('w')],
    ..command(ActionId::Close, "close", "Close", "tab document")
};

const QUIT: ActionSpec = ActionSpec {
    keys: &[cmd('q')],
    keys_mac: &[cmd('q')],
    ..command(ActionId::Quit, "quit", "Quit", "exit")
};

const UNDO: ActionSpec = ActionSpec {
    context: Context::Document,
    keys: &[cmd('z')],
    keys_mac: &[cmd('z')],
    repeats: true,
    text_edit: true,
    ..command(ActionId::Undo, "undo", "Undo", "history back")
};

const REDO: ActionSpec = ActionSpec {
    context: Context::Document,
    keys: &[cmd('y'), cmd_shift('z')],
    keys_mac: &[cmd_shift('z')],
    repeats: true,
    text_edit: true,
    ..command(ActionId::Redo, "redo", "Redo", "history forward")
};

const ZOOM_IN: ActionSpec = ActionSpec {
    context: Context::Document,
    keys: &[cmd('=')],
    keys_mac: &[cmd('=')],
    repeats: true,
    ..command(ActionId::ZoomIn, "zoom_in", "Zoom In", "view bigger")
};

const ZOOM_OUT: ActionSpec = ActionSpec {
    context: Context::Document,
    keys: &[cmd('-')],
    keys_mac: &[cmd('-')],
    repeats: true,
    ..command(ActionId::ZoomOut, "zoom_out", "Zoom Out", "view smaller")
};

const ZOOM_NORMAL: ActionSpec = ActionSpec {
    context: Context::Document,
    keys: &[cmd('0')],
    keys_mac: &[cmd('0')],
    ..command(
        ActionId::ZoomNormal,
        "zoom_normal",
        "Normal Size",
        "view 100 actual pixels",
    )
};

const FIT_IN_VIEW: ActionSpec = ActionSpec {
    context: Context::Document,
    keys: &[cmd('/')],
    keys_mac: &[cmd('/')],
    ..command(
        ActionId::FitInView,
        "fit_in_view",
        "Fit Map in View",
        "view whole",
    )
};

const SHOW_GRID: ActionSpec = ActionSpec {
    context: Context::Document,
    keys: &[cmd('g')],
    keys_mac: &[cmd('g')],
    kind: ActionKind::Toggle,
    ..command(
        ActionId::ShowGrid,
        "show_grid",
        "Show Grid",
        "view tiles lines",
    )
};

const HIGHLIGHT_CURRENT_LAYER: ActionSpec = ActionSpec {
    context: Context::Document,
    keys: &[cmd('h')],
    keys_mac: &[],
    kind: ActionKind::Toggle,
    ..command(
        ActionId::HighlightCurrentLayer,
        "highlight_current_layer",
        "Highlight Current Layer",
        "view layer fade",
    )
};

const NEXT_DOCUMENT: ActionSpec = ActionSpec {
    context: Context::Document,
    keys: &[NEXT_TAB],
    keys_mac: &[MAC_NEXT_TAB, cmd_shift(']')],
    ..command(
        ActionId::NextDocument,
        "next_document",
        "Next Tab",
        "document switch",
    )
};

const PREVIOUS_DOCUMENT: ActionSpec = ActionSpec {
    context: Context::Document,
    keys: &[PREVIOUS_TAB],
    keys_mac: &[MAC_PREVIOUS_TAB, cmd_shift('[')],
    ..command(
        ActionId::PreviousDocument,
        "previous_document",
        "Previous Tab",
        "document switch",
    )
};

/// Every command, in the order of [`ActionId`].
pub const ACTIONS: [ActionSpec; 16] = [
    OPEN,
    SAVE,
    SAVE_AS,
    SAVE_ALL,
    CLOSE,
    QUIT,
    UNDO,
    REDO,
    ZOOM_IN,
    ZOOM_OUT,
    ZOOM_NORMAL,
    FIT_IN_VIEW,
    SHOW_GRID,
    HIGHLIGHT_CURRENT_LAYER,
    NEXT_DOCUMENT,
    PREVIOUS_DOCUMENT,
];

impl ActionId {
    /// Its declaration.
    pub fn spec(self) -> &'static ActionSpec {
        match self {
            ActionId::Open => &OPEN,
            ActionId::Save => &SAVE,
            ActionId::SaveAs => &SAVE_AS,
            ActionId::SaveAll => &SAVE_ALL,
            ActionId::Close => &CLOSE,
            ActionId::Quit => &QUIT,
            ActionId::Undo => &UNDO,
            ActionId::Redo => &REDO,
            ActionId::ZoomIn => &ZOOM_IN,
            ActionId::ZoomOut => &ZOOM_OUT,
            ActionId::ZoomNormal => &ZOOM_NORMAL,
            ActionId::FitInView => &FIT_IN_VIEW,
            ActionId::ShowGrid => &SHOW_GRID,
            ActionId::HighlightCurrentLayer => &HIGHLIGHT_CURRENT_LAYER,
            ActionId::NextDocument => &NEXT_DOCUMENT,
            ActionId::PreviousDocument => &PREVIOUS_DOCUMENT,
        }
    }

    /// The command with a stable name.
    pub fn from_name(name: &str) -> Option<ActionId> {
        ACTIONS
            .iter()
            .find(|spec| spec.name == name)
            .map(|spec| spec.id)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    /// Every id, written out so that a new variant fails to compile here until it is listed.
    fn every_id(id: ActionId) -> usize {
        match id {
            ActionId::Open => 0,
            ActionId::Save => 1,
            ActionId::SaveAs => 2,
            ActionId::SaveAll => 3,
            ActionId::Close => 4,
            ActionId::Quit => 5,
            ActionId::Undo => 6,
            ActionId::Redo => 7,
            ActionId::ZoomIn => 8,
            ActionId::ZoomOut => 9,
            ActionId::ZoomNormal => 10,
            ActionId::FitInView => 11,
            ActionId::ShowGrid => 12,
            ActionId::HighlightCurrentLayer => 13,
            ActionId::NextDocument => 14,
            ActionId::PreviousDocument => 15,
        }
    }

    #[test]
    fn the_table_is_complete_and_its_names_and_keys_are_unique() {
        for (index, spec) in ACTIONS.iter().enumerate() {
            assert_eq!(every_id(spec.id), index, "{:?} out of order", spec.id);
            assert_eq!(spec.id.spec(), spec);
            assert_eq!(ActionId::from_name(spec.name), Some(spec.id));
        }
        let names: HashSet<&str> = ACTIONS.iter().map(|s| s.name).collect();
        assert_eq!(names.len(), ACTIONS.len());
        for mac in [false, true] {
            let mut seen = HashSet::new();
            for spec in &ACTIONS {
                let keys = if mac { spec.keys_mac } else { spec.keys };
                for chord in keys {
                    assert!(
                        seen.insert(*chord),
                        "{} is bound twice (mac: {mac})",
                        chord.label(mac)
                    );
                }
            }
        }
    }
}
