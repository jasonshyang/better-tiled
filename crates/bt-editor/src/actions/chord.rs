//! Key chords: modifiers and a key, in the crate's own terms. `command` is ⌘ on macOS and Ctrl
//! elsewhere; `control` is the Control key on macOS only. Keys are named as the US layout names
//! them, unshifted.

use std::fmt;

/// The modifiers of a chord.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Modifiers {
    /// ⌘ on macOS, Ctrl elsewhere.
    pub command: bool,
    /// Shift.
    pub shift: bool,
    /// ⌥ on macOS, Alt elsewhere.
    pub alt: bool,
    /// Control on macOS (Ctrl elsewhere is `command`).
    pub control: bool,
}

impl Modifiers {
    /// No modifier.
    pub const NONE: Modifiers = Modifiers {
        command: false,
        shift: false,
        alt: false,
        control: false,
    };
    /// ⌘ or Ctrl.
    pub const COMMAND: Modifiers = Modifiers {
        command: true,
        ..Modifiers::NONE
    };
    /// ⌘⇧ or Ctrl+Shift.
    pub const COMMAND_SHIFT: Modifiers = Modifiers {
        command: true,
        shift: true,
        ..Modifiers::NONE
    };
    /// ⌘⌥ or Ctrl+Alt.
    pub const COMMAND_ALT: Modifiers = Modifiers {
        command: true,
        alt: true,
        ..Modifiers::NONE
    };
}

/// A key.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    /// A letter (lowercase), a digit, or a punctuation key as the US layout prints it unshifted:
    /// `- = [ ] \ ; ' , . / `` ` ``.
    Char(char),
    /// A function key, F1 to F24.
    Function(u8),
    /// Arrow up.
    Up,
    /// Arrow down.
    Down,
    /// Arrow left.
    Left,
    /// Arrow right.
    Right,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Tab.
    Tab,
    /// Space.
    Space,
    /// Return or Enter.
    Enter,
    /// Escape.
    Escape,
    /// Backspace.
    Backspace,
    /// Delete (forward).
    Delete,
    /// Insert.
    Insert,
}

/// The characters [`Key::Char`] may hold besides letters and digits.
const PUNCTUATION: &str = "-=[]\\;',./`";

/// The names of keys in chord text.
const NAMED: [NamedKey; 15] = [
    NamedKey {
        name: "up",
        key: Key::Up,
    },
    NamedKey {
        name: "down",
        key: Key::Down,
    },
    NamedKey {
        name: "left",
        key: Key::Left,
    },
    NamedKey {
        name: "right",
        key: Key::Right,
    },
    NamedKey {
        name: "home",
        key: Key::Home,
    },
    NamedKey {
        name: "end",
        key: Key::End,
    },
    NamedKey {
        name: "pageup",
        key: Key::PageUp,
    },
    NamedKey {
        name: "pagedown",
        key: Key::PageDown,
    },
    NamedKey {
        name: "tab",
        key: Key::Tab,
    },
    NamedKey {
        name: "space",
        key: Key::Space,
    },
    NamedKey {
        name: "enter",
        key: Key::Enter,
    },
    NamedKey {
        name: "escape",
        key: Key::Escape,
    },
    NamedKey {
        name: "backspace",
        key: Key::Backspace,
    },
    NamedKey {
        name: "delete",
        key: Key::Delete,
    },
    NamedKey {
        name: "insert",
        key: Key::Insert,
    },
];

/// A key with a name of its own in chord text.
struct NamedKey {
    name: &'static str,
    key: Key,
}

impl Key {
    /// A character key, when `c` is one (letters fold to lowercase).
    pub fn of_char(c: char) -> Option<Key> {
        let c = c.to_ascii_lowercase();
        (c.is_ascii_lowercase() || c.is_ascii_digit() || PUNCTUATION.contains(c))
            .then_some(Key::Char(c))
    }

    /// The key's name in chord text.
    fn text(self) -> String {
        match self {
            Key::Char(c) => c.to_string(),
            Key::Function(n) => format!("f{n}"),
            other => NAMED
                .iter()
                .find(|named| named.key == other)
                .map_or_else(String::new, |named| named.name.to_owned()),
        }
    }

    /// How a menu shows the key.
    fn label(self, mac: bool) -> String {
        match self {
            Key::Char(c) => c.to_ascii_uppercase().to_string(),
            Key::Function(n) => format!("F{n}"),
            Key::Up => "↑".to_owned(),
            Key::Down => "↓".to_owned(),
            Key::Left => "←".to_owned(),
            Key::Right => "→".to_owned(),
            Key::Enter => if mac { "↩" } else { "Enter" }.to_owned(),
            Key::Escape => if mac { "⎋" } else { "Esc" }.to_owned(),
            Key::Backspace => if mac { "⌫" } else { "Backspace" }.to_owned(),
            Key::Delete => if mac { "⌦" } else { "Del" }.to_owned(),
            Key::Tab => if mac { "⇥" } else { "Tab" }.to_owned(),
            Key::Space => "Space".to_owned(),
            Key::Home => "Home".to_owned(),
            Key::End => "End".to_owned(),
            Key::PageUp => "PgUp".to_owned(),
            Key::PageDown => "PgDown".to_owned(),
            Key::Insert => "Ins".to_owned(),
        }
    }

    fn parse(text: &str) -> Option<Key> {
        let mut chars = text.chars();
        if let Some(c) = chars.next()
            && chars.next().is_none()
        {
            return Key::of_char(c);
        }
        if let Some(n) = text.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
            return (1..=24).contains(&n).then_some(Key::Function(n));
        }
        NAMED
            .iter()
            .find(|named| named.name == text)
            .map(|named| named.key)
    }
}

/// Modifiers and a key.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    /// The modifiers.
    pub modifiers: Modifiers,
    /// The key.
    pub key: Key,
}

impl Chord {
    /// `key` alone.
    pub const fn plain(key: Key) -> Chord {
        Chord {
            modifiers: Modifiers::NONE,
            key,
        }
    }

    /// `key` with `modifiers`.
    pub const fn with(modifiers: Modifiers, key: Key) -> Chord {
        Chord { modifiers, key }
    }

    /// The chord as settings store it: `cmd+shift+k`, `f5`, `alt+up`.
    pub fn to_text(self) -> String {
        let mut parts = Vec::new();
        if self.modifiers.control {
            parts.push("ctrl".to_owned());
        }
        if self.modifiers.alt {
            parts.push("alt".to_owned());
        }
        if self.modifiers.shift {
            parts.push("shift".to_owned());
        }
        if self.modifiers.command {
            parts.push("cmd".to_owned());
        }
        parts.push(self.key.text());
        parts.join("+")
    }

    /// Reads [`Chord::to_text`]'s form; `None` for text that names no chord.
    pub fn parse(text: &str) -> Option<Chord> {
        let text = text.trim().to_ascii_lowercase();
        let mut modifiers = Modifiers::NONE;
        let mut parts: Vec<&str> = text.split('+').collect();
        // A chord of the `+` key would be written `cmd++`: not a key on the US layout.
        let key = Key::parse(parts.pop()?)?;
        for part in parts {
            match part {
                "cmd" | "command" => modifiers.command = true,
                "shift" => modifiers.shift = true,
                "alt" | "option" => modifiers.alt = true,
                "ctrl" | "control" => modifiers.control = true,
                _ => return None,
            }
        }
        Some(Chord { modifiers, key })
    }

    /// How a menu shows the chord: `⇧⌘S` on macOS, `Ctrl+Shift+S` elsewhere.
    pub fn label(self, mac: bool) -> String {
        let m = self.modifiers;
        if mac {
            let mut out = String::new();
            if m.control {
                out.push('⌃');
            }
            if m.alt {
                out.push('⌥');
            }
            if m.shift {
                out.push('⇧');
            }
            if m.command {
                out.push('⌘');
            }
            out.push_str(&self.key.label(true));
            out
        } else {
            let mut parts: Vec<String> = Vec::new();
            if m.command || m.control {
                parts.push("Ctrl".to_owned());
            }
            if m.alt {
                parts.push("Alt".to_owned());
            }
            if m.shift {
                parts.push("Shift".to_owned());
            }
            parts.push(self.key.label(false));
            parts.join("+")
        }
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_text())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_read_back_what_they_write() {
        for text in [
            "cmd+s",
            "shift+cmd+s",
            "alt+up",
            "f5",
            "ctrl+alt+shift+cmd+/",
            "space",
            "cmd+=",
        ] {
            let chord = Chord::parse(text).unwrap();
            assert_eq!(Chord::parse(&chord.to_text()), Some(chord), "{text}");
        }
        assert_eq!(
            Chord::parse("Cmd+Shift+S"),
            Some(Chord::with(Modifiers::COMMAND_SHIFT, Key::Char('s')))
        );
        for bad in ["", "cmd+", "hyper+s", "f25", "cmd+é", "cmd+ab"] {
            assert_eq!(Chord::parse(bad), None, "{bad}");
        }
        let save_as = Chord::with(Modifiers::COMMAND_SHIFT, Key::Char('s'));
        assert_eq!(save_as.label(true), "⇧⌘S");
        assert_eq!(save_as.label(false), "Ctrl+Shift+S");
    }
}
