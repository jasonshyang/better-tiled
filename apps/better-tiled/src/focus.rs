//! Keyboard routing. Only text fields keep the keyboard: any other focused widget gives it up,
//! so Space, Tab and the arrows never press buttons or walk the panels. Key presses become
//! commands through the keymap, with exact modifiers; plain keys are not shortcuts while a text
//! field has the keyboard, and the keys a text field handles itself stay its own.

use bt_editor::{ActionId, Chord, Editor, Enabled, Key, Modifiers};

/// Whether a text field has the keyboard.
pub(crate) fn is_typing(ctx: &egui::Context) -> bool {
    ctx.memory(|memory| memory.focused())
        .is_some_and(|id| egui::text_edit::TextEditState::load(ctx, id).is_some())
}

/// Takes the keyboard from a focused widget that is not a text field, and keeps Tab from
/// moving focus between widgets when no text field has it.
pub(crate) fn enforce_focus(ctx: &egui::Context) {
    if is_typing(ctx) {
        return;
    }
    if let Some(id) = ctx.memory(|memory| memory.focused()) {
        ctx.memory_mut(|memory| memory.surrender_focus(id));
    }
    ctx.input_mut(|input| {
        input.consume_key(egui::Modifiers::NONE, egui::Key::Tab);
        input.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab);
    });
}

/// The commands whose keys were pressed this frame; their key presses are consumed.
pub(crate) fn shortcuts(ctx: &egui::Context, editor: &Editor) -> Vec<ActionId> {
    let typing = is_typing(ctx);
    let mac = editor.keymap().is_mac();
    let presses: Vec<KeyPress> = ctx.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat,
                    modifiers,
                    ..
                } => Some(KeyPress {
                    key: *key,
                    modifiers: *modifiers,
                    repeat: *repeat,
                }),
                _ => None,
            })
            .collect()
    });
    let mut run = Vec::new();
    for press in presses {
        let Some(key) = key_of(press.key) else {
            continue;
        };
        let modifiers = modifiers_of(press.modifiers, mac);
        if typing && !modifiers.command {
            continue;
        }
        let chord = Chord::with(modifiers, key);
        let candidates = editor.keymap().actions_for(chord);
        let chosen = candidates
            .iter()
            .copied()
            .find(|id| editor.enabled(*id) == Enabled::Enabled)
            .or_else(|| {
                // A disabled command takes its key only when it can say why.
                candidates
                    .iter()
                    .copied()
                    .find(|id| matches!(editor.enabled(*id), Enabled::Disabled(Some(_))))
            });
        let Some(id) = chosen else {
            continue;
        };
        let spec = id.spec();
        if (typing && spec.text_edit) || (press.repeat && !spec.repeats) {
            continue;
        }
        ctx.input_mut(|input| input.consume_key(press.modifiers, press.key));
        run.push(id);
    }
    run
}

/// A key pressed this frame.
#[derive(Copy, Clone, Debug)]
struct KeyPress {
    key: egui::Key,
    modifiers: egui::Modifiers,
    repeat: bool,
}

/// The editor's modifiers for the toolkit's: ⌘ is the command key on macOS, Ctrl elsewhere.
fn modifiers_of(modifiers: egui::Modifiers, mac: bool) -> Modifiers {
    if mac {
        Modifiers {
            command: modifiers.mac_cmd || modifiers.command,
            shift: modifiers.shift,
            alt: modifiers.alt,
            control: modifiers.ctrl,
        }
    } else {
        Modifiers {
            command: modifiers.ctrl,
            shift: modifiers.shift,
            alt: modifiers.alt,
            control: false,
        }
    }
}

/// The editor's key for the toolkit's; a shifted symbol counts as its key.
fn key_of(key: egui::Key) -> Option<Key> {
    use egui::Key as K;
    let named = match key {
        K::ArrowUp => Key::Up,
        K::ArrowDown => Key::Down,
        K::ArrowLeft => Key::Left,
        K::ArrowRight => Key::Right,
        K::Home => Key::Home,
        K::End => Key::End,
        K::PageUp => Key::PageUp,
        K::PageDown => Key::PageDown,
        K::Tab => Key::Tab,
        K::Space => Key::Space,
        K::Enter => Key::Enter,
        K::Escape => Key::Escape,
        K::Backspace => Key::Backspace,
        K::Delete => Key::Delete,
        K::Insert => Key::Insert,
        K::Minus => Key::Char('-'),
        K::Equals | K::Plus => Key::Char('='),
        K::Comma => Key::Char(','),
        K::Period => Key::Char('.'),
        K::Semicolon | K::Colon => Key::Char(';'),
        K::Quote => Key::Char('\''),
        K::Backtick => Key::Char('`'),
        K::Slash | K::Questionmark => Key::Char('/'),
        K::Backslash | K::Pipe => Key::Char('\\'),
        K::OpenBracket | K::OpenCurlyBracket => Key::Char('['),
        K::CloseBracket | K::CloseCurlyBracket => Key::Char(']'),
        _ => return function_key(key).or_else(|| letter_or_digit(key)),
    };
    Some(named)
}

/// A letter or digit key (their names are the character).
fn letter_or_digit(key: egui::Key) -> Option<Key> {
    let mut chars = key.name().chars();
    let first = chars.next()?;
    if chars.next().is_some() || !first.is_ascii_alphanumeric() {
        return None;
    }
    Key::of_char(first)
}

/// A function key, F1 to F24.
fn function_key(key: egui::Key) -> Option<Key> {
    let name = key.name();
    let number = name.strip_prefix('F')?.parse::<u8>().ok()?;
    (1..=24).contains(&number).then_some(Key::Function(number))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_modifiers_become_the_editors() {
        assert_eq!(key_of(egui::Key::S), Some(Key::Char('s')));
        assert_eq!(key_of(egui::Key::Num0), Some(Key::Char('0')));
        assert_eq!(key_of(egui::Key::Equals), Some(Key::Char('=')));
        assert_eq!(key_of(egui::Key::Plus), Some(Key::Char('=')));
        assert_eq!(key_of(egui::Key::Minus), Some(Key::Char('-')));
        assert_eq!(key_of(egui::Key::Slash), Some(Key::Char('/')));
        assert_eq!(key_of(egui::Key::OpenBracket), Some(Key::Char('[')));
        assert_eq!(key_of(egui::Key::F5), Some(Key::Function(5)));
        assert_eq!(key_of(egui::Key::Tab), Some(Key::Tab));
        let command = egui::Modifiers::COMMAND;
        assert!(modifiers_of(command, cfg!(target_os = "macos")).command);
        let ctrl_on_mac = modifiers_of(egui::Modifiers::CTRL, true);
        assert!(!ctrl_on_mac.command && ctrl_on_mac.control);
    }

    #[test]
    fn a_shortcut_runs_its_command_once_and_is_consumed() {
        let ctx = egui::Context::default();
        let editor = Editor::new();
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key {
            key: egui::Key::O,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        });
        let mut ran = Vec::new();
        let mut output = ctx.run_ui(input, |ui| {
            ran = shortcuts(ui.ctx(), &editor);
            assert!(!ui.input(|i| i.key_pressed(egui::Key::O)));
        });
        output.textures_delta.clear();
        assert_eq!(ran, [ActionId::Open]);
    }
}
