//! The user's key bindings over the action table's defaults. Passed to whoever needs it;
//! nothing publishes it through a global.

use std::collections::BTreeMap;

use super::chord::Chord;
use super::table::{ACTIONS, ActionId, Context};

/// A binding in settings that does not name a command or a chord.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum KeymapError {
    /// No command has this name.
    #[error("the keymap names “{0}”, which is not a command")]
    UnknownAction(String),
    /// The text is not a chord.
    #[error("the keymap binds “{action}” to “{chord}”, which is not a key")]
    UnknownChord {
        /// The command.
        action: String,
        /// The text.
        chord: String,
    },
}

/// Key bindings: the table's defaults for the platform, with the user's overrides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keymap {
    overrides: BTreeMap<ActionId, Vec<Chord>>,
    mac: bool,
}

impl Keymap {
    /// The defaults, for macOS or for other systems.
    pub fn new(mac: bool) -> Keymap {
        Keymap {
            overrides: BTreeMap::new(),
            mac,
        }
    }

    /// The defaults with the overrides settings store (command name → chord texts).
    pub fn from_settings(
        saved: &BTreeMap<String, Vec<String>>,
        mac: bool,
    ) -> Result<Keymap, KeymapError> {
        let mut keymap = Keymap::new(mac);
        for (name, texts) in saved {
            let id = ActionId::from_name(name)
                .ok_or_else(|| KeymapError::UnknownAction(name.clone()))?;
            let chords = texts
                .iter()
                .map(|text| {
                    Chord::parse(text).ok_or_else(|| KeymapError::UnknownChord {
                        action: name.clone(),
                        chord: text.clone(),
                    })
                })
                .collect::<Result<Vec<Chord>, KeymapError>>()?;
            keymap.overrides.insert(id, chords);
        }
        Ok(keymap)
    }

    /// The overrides as settings store them.
    pub fn to_settings(&self) -> BTreeMap<String, Vec<String>> {
        self.overrides
            .iter()
            .map(|(id, chords)| {
                (
                    id.spec().name.to_owned(),
                    chords.iter().map(|c| c.to_text()).collect(),
                )
            })
            .collect()
    }

    /// Whether the defaults are macOS's.
    pub fn is_mac(&self) -> bool {
        self.mac
    }

    /// A command's chords, the first the one menus show.
    pub fn chords(&self, id: ActionId) -> Vec<Chord> {
        match self.overrides.get(&id) {
            Some(chords) => chords.clone(),
            None => {
                let spec = id.spec();
                if self.mac { spec.keys_mac } else { spec.keys }.to_vec()
            }
        }
    }

    /// The chord menus show for a command.
    pub fn chord(&self, id: ActionId) -> Option<Chord> {
        self.chords(id).first().copied()
    }

    /// How hints and messages name a command's key: `⇧⌘S`, or `None` when it has none.
    pub fn key_text(&self, id: ActionId) -> Option<String> {
        self.chord(id).map(|chord| chord.label(self.mac))
    }

    /// A command's name with its key, for messages: “Save (⌘S)”, or the name alone.
    pub fn with_key(&self, id: ActionId) -> String {
        let label = id.spec().label.trim_end_matches('…');
        match self.key_text(id) {
            Some(key) => format!("{label} ({key})"),
            None => label.to_owned(),
        }
    }

    /// Binds a command to `chords` instead of its defaults.
    pub fn set_chords(&mut self, id: ActionId, chords: Vec<Chord>) {
        self.overrides.insert(id, chords);
    }

    /// Puts a command's defaults back.
    pub fn reset(&mut self, id: ActionId) {
        self.overrides.remove(&id);
    }

    /// Puts every default back.
    pub fn reset_all(&mut self) {
        self.overrides.clear();
    }

    /// Whether a command's keys are the user's.
    pub fn is_customized(&self, id: ActionId) -> bool {
        self.overrides.contains_key(&id)
    }

    /// The other commands bound to `chord` whose contexts can be active together with `id`'s.
    pub fn conflicts(&self, id: ActionId, chord: Chord) -> Vec<ActionId> {
        let context = id.spec().context;
        ACTIONS
            .iter()
            .filter(|spec| spec.id != id)
            .filter(|spec| overlap(spec.context, context))
            .filter(|spec| self.chords(spec.id).contains(&chord))
            .map(|spec| spec.id)
            .collect()
    }

    /// The commands bound to a chord.
    pub fn actions_for(&self, chord: Chord) -> Vec<ActionId> {
        ACTIONS
            .iter()
            .filter(|spec| self.chords(spec.id).contains(&chord))
            .map(|spec| spec.id)
            .collect()
    }
}

/// Whether two contexts can both be active.
fn overlap(_a: Context, _b: Context) -> bool {
    // Every context so far holds whenever a document is open.
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::chord::{Key, Modifiers};

    #[test]
    fn overrides_replace_defaults_and_round_trip_through_settings() {
        let mut keymap = Keymap::new(true);
        let save = Chord::with(Modifiers::COMMAND, Key::Char('s'));
        assert_eq!(keymap.chord(ActionId::Save), Some(save));
        assert_eq!(keymap.key_text(ActionId::Save).as_deref(), Some("⌘S"));
        assert_eq!(keymap.with_key(ActionId::SaveAs), "Save As (⇧⌘S)");
        let other = Chord::with(Modifiers::COMMAND_ALT, Key::Char('k'));
        keymap.set_chords(ActionId::Save, vec![other]);
        assert!(keymap.is_customized(ActionId::Save));
        assert_eq!(keymap.actions_for(other), vec![ActionId::Save]);
        assert_eq!(
            keymap.conflicts(ActionId::Open, other),
            vec![ActionId::Save]
        );
        let saved = keymap.to_settings();
        assert_eq!(saved.get("save"), Some(&vec!["alt+cmd+k".to_owned()]));
        assert_eq!(Keymap::from_settings(&saved, true), Ok(keymap.clone()));
        keymap.reset_all();
        assert_eq!(keymap.chord(ActionId::Save), Some(save));
        let wrong = BTreeMap::from([("sav".to_owned(), vec!["cmd+s".to_owned()])]);
        assert_eq!(
            Keymap::from_settings(&wrong, true),
            Err(KeymapError::UnknownAction("sav".into()))
        );
        let bad = BTreeMap::from([("save".to_owned(), vec!["cmd+".to_owned()])]);
        assert!(matches!(
            Keymap::from_settings(&bad, true),
            Err(KeymapError::UnknownChord { .. })
        ));
        assert_eq!(
            Keymap::new(false).key_text(ActionId::Redo).as_deref(),
            Some("Ctrl+Y")
        );
    }
}
