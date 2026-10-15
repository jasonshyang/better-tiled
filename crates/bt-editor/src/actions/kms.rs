//! Tiled keyboard mapping schemes, following shortcutsettingspage.cpp. Imports are atomic:
//! an invalid supported binding leaves the keymap untouched; unknown actions are reported.

use super::{ACTIONS, Chord, Key, Keymap, KeymapError, Modifiers};

/// What importing a mapping scheme changed, and the actions belonging to other tools.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KmsImport {
    /// Commands whose bindings were replaced, including explicitly unbound commands.
    pub imported: usize,
    /// Action ids that this editor does not implement.
    pub unknown_actions: Vec<String>,
}

impl Keymap {
    /// Imports Tiled's `.kms` XML, replacing only the commands the file names.
    pub fn import_kms(&mut self, text: &str) -> Result<KmsImport, KeymapError> {
        let document = roxmltree::Document::parse_with_options(
            text,
            roxmltree::ParsingOptions {
                allow_dtd: true,
                ..Default::default()
            },
        )
        .map_err(|error| KeymapError::InvalidScheme(error.to_string()))?;
        let root = document.root_element();
        if root.tag_name().name() != "mapping" {
            return Err(KeymapError::InvalidScheme(
                "expected a <mapping> root".into(),
            ));
        }
        let mut edited = self.clone();
        let mut report = KmsImport::default();
        let mut imported = std::collections::BTreeSet::new();
        for shortcut in root.children().filter(|node| node.has_tag_name("shortcut")) {
            let id = shortcut
                .attribute("id")
                .ok_or_else(|| KeymapError::InvalidScheme("a shortcut has no id".into()))?;
            let action = ACTIONS.iter().find(|action| {
                action.tiled_id == Some(id) || id.strip_prefix("BetterTiled.") == Some(action.name)
            });
            let Some(action) = action else {
                if !report.unknown_actions.iter().any(|unknown| unknown == id) {
                    report.unknown_actions.push(id.to_owned());
                }
                continue;
            };
            let mut chords = Vec::new();
            for key in shortcut.children().filter(|node| node.has_tag_name("key")) {
                let text = key.attribute("value").ok_or_else(|| {
                    KeymapError::InvalidScheme(format!("{id}: a key has no value"))
                })?;
                if text.trim().is_empty() {
                    continue;
                }
                let chord = Chord::from_qt(text).ok_or_else(|| {
                    KeymapError::InvalidScheme(format!("{id}: unsupported shortcut “{text}”"))
                })?;
                if chord.modifiers.control && !self.is_mac() {
                    return Err(KeymapError::InvalidScheme(format!(
                        "{id}: the Meta modifier is not available on this platform"
                    )));
                }
                if !chords.contains(&chord) {
                    chords.push(chord);
                }
            }
            edited.set_chords(action.id, chords);
            imported.insert(action.id);
        }
        report.imported = imported.len();
        *self = edited;
        Ok(report)
    }

    /// Exports Tiled's XML schema and Qt's portable key text. Better Tiled-only commands use
    /// a namespaced id, which Tiled ignores and Better Tiled can import again.
    pub fn to_kms(&self) -> String {
        let mut out = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE KeyboardMappingScheme>\n<mapping>\n",
        );
        let mut actions: Vec<_> = ACTIONS.iter().collect();
        actions.sort_by_key(|action| {
            action
                .tiled_id
                .map(str::to_owned)
                .unwrap_or_else(|| format!("BetterTiled.{}", action.name))
        });
        for action in actions {
            let id = action
                .tiled_id
                .map(str::to_owned)
                .unwrap_or_else(|| format!("BetterTiled.{}", action.name));
            out.push_str(&format!(" <shortcut id=\"{id}\">\n"));
            for chord in self.chords(action.id) {
                out.push_str(&format!("  <key value=\"{}\"/>\n", chord.to_qt()));
            }
            out.push_str(" </shortcut>\n");
        }
        out.push_str("</mapping>\n");
        out
    }
}

impl Chord {
    /// Reads one Qt portable chord. `Ctrl` is the command modifier on each platform, as Qt
    /// swaps Control and Meta on macOS. Multi-stroke sequences are rejected explicitly.
    pub fn from_qt(text: &str) -> Option<Chord> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let mut parts: Vec<_> = text.split('+').collect();
        let key_text = if text.ends_with('+') {
            parts.pop()?;
            parts.pop()?;
            "+"
        } else {
            parts.pop()?
        };
        let mut modifiers = Modifiers::NONE;
        for part in parts {
            match part.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "control" => modifiers.command = true,
                "meta" => modifiers.control = true,
                "alt" => modifiers.alt = true,
                "shift" => modifiers.shift = true,
                _ => return None,
            }
        }
        let name = match key_text.trim().to_ascii_lowercase().as_str() {
            "return" => "enter".to_owned(),
            "esc" => "escape".to_owned(),
            "del" => "delete".to_owned(),
            "ins" => "insert".to_owned(),
            "pgup" => "pageup".to_owned(),
            "pgdown" | "pgdn" => "pagedown".to_owned(),
            "+" => {
                modifiers.shift = true;
                "=".to_owned()
            }
            other => other.to_owned(),
        };
        let key = Chord::parse(&name)?.key;
        Some(Chord::with(modifiers, key))
    }

    /// Writes Qt's portable form, independent of how a platform labels its keys.
    pub fn to_qt(self) -> String {
        let mut parts = Vec::new();
        if self.modifiers.control {
            parts.push("Meta".to_owned());
        }
        if self.modifiers.command {
            parts.push("Ctrl".to_owned());
        }
        if self.modifiers.alt {
            parts.push("Alt".to_owned());
        }
        if self.modifiers.shift {
            parts.push("Shift".to_owned());
        }
        parts.push(match self.key {
            Key::Char(c) => c.to_ascii_uppercase().to_string(),
            Key::PageUp => "PgUp".to_owned(),
            Key::PageDown => "PgDown".to_owned(),
            Key::Enter => "Return".to_owned(),
            Key::Escape => "Esc".to_owned(),
            Key::Delete => "Del".to_owned(),
            Key::Insert => "Ins".to_owned(),
            Key::Up => "Up".to_owned(),
            Key::Down => "Down".to_owned(),
            Key::Left => "Left".to_owned(),
            Key::Right => "Right".to_owned(),
            Key::Home => "Home".to_owned(),
            Key::End => "End".to_owned(),
            Key::Tab => "Tab".to_owned(),
            Key::Space => "Space".to_owned(),
            Key::Backspace => "Backspace".to_owned(),
            Key::Function(number) => format!("F{number}"),
        });
        parts.join("+")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ActionId;

    #[test]
    fn tiled_schemes_keep_unbound_and_multiple_keys_and_report_unknown_actions() {
        let mut keymap = Keymap::new(true);
        let report = keymap
            .import_kms(
                r#"<?xml version="1.0"?><!DOCTYPE KeyboardMappingScheme><mapping>
          <shortcut id="Save"><key value="Ctrl+Shift+S"/><key value="F6"/></shortcut>
          <shortcut id="Open"/><shortcut id="SnapNothing"><key value="F4"/></shortcut>
          <shortcut id="ScriptedAction"><key value="not a supported key"/></shortcut>
          <shortcut id="StampTool"><key value="B"/></shortcut>
        </mapping>"#,
            )
            .unwrap();
        assert_eq!(report.imported, 4);
        assert_eq!(report.unknown_actions, ["ScriptedAction"]);
        assert_eq!(
            keymap.chords(ActionId::Save),
            [
                Chord::parse("cmd+shift+s").unwrap(),
                Chord::parse("f6").unwrap()
            ]
        );
        assert!(keymap.chords(ActionId::Open).is_empty());
        assert_eq!(keymap.chord(ActionId::SnapNone), Chord::parse("f4"));
        assert_eq!(keymap.chord(ActionId::BrushTool), Chord::parse("b"));
    }

    #[test]
    fn invalid_imports_never_apply_part_of_a_scheme() {
        let mut keys = Keymap::new(false);
        let before = keys.clone();
        for text in [
            "<garbage/>",
            "<mapping>",
            r#"<mapping><shortcut id="Save"><key value="F7"/></shortcut><shortcut id="Open"><key value="Ctrl+K, Ctrl+O"/></shortcut></mapping>"#,
        ] {
            assert!(keys.import_kms(text).is_err());
            assert_eq!(keys, before);
        }
    }

    #[test]
    fn exported_schemes_restore_every_effective_binding_on_either_platform() {
        for mac in [false, true] {
            let mut original = Keymap::new(mac);
            original.set_chords(
                ActionId::GodotSetup,
                vec![Chord::parse(if mac { "ctrl+cmd+f8" } else { "cmd+f8" }).unwrap()],
            );
            original.set_chords(ActionId::Open, Vec::new());
            let mut restored = Keymap::new(mac);
            let report = restored.import_kms(&original.to_kms()).unwrap();
            assert!(report.unknown_actions.is_empty());
            for action in ACTIONS {
                assert_eq!(
                    restored.chords(action.id),
                    original.chords(action.id),
                    "{}",
                    action.name
                );
            }
        }
    }

    #[test]
    fn qt_modifiers_and_punctuation_are_portable() {
        for text in [
            "Ctrl+S",
            "Meta+Alt+Shift+F12",
            "Ctrl+,",
            "Ctrl++",
            "Shift+PgDown",
            "Space",
            "Del",
            "Return",
        ] {
            let chord = Chord::from_qt(text).unwrap();
            assert_eq!(Chord::from_qt(&chord.to_qt()), Some(chord), "{text}");
        }
        assert_eq!(Chord::from_qt("Ctrl+S").unwrap().label(true), "⌘S");
        assert!(Chord::from_qt("Ctrl+K, Ctrl+C").is_none());
    }
}
