//! The interface's fonts: egui's own, and on macOS the system's Apple Symbols after them for the
//! key names menus, the palette and hints show (`⇧⌘S`). egui's fonts have ⌘ but not ⇧, ⌥, ⌃,
//! the arrows or the other keys' symbols; elsewhere key names are spelled out (`Ctrl+Shift+S`).

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

/// The macOS font with every key symbol, in every version since 10.3.
const MAC_KEY_SYMBOLS: &str = "/System/Library/Fonts/Apple Symbols.ttf";

/// egui's fonts, with the key symbols' after them on macOS. A symbols font that cannot be read
/// is logged, and the symbols it would draw show as boxes.
pub(crate) fn interface_fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    if cfg!(target_os = "macos") {
        match std::fs::read(MAC_KEY_SYMBOLS) {
            Ok(bytes) => add_fallback(&mut fonts, "key symbols", bytes),
            Err(error) => {
                log::warn!("the key symbols' font {MAC_KEY_SYMBOLS} cannot be read: {error}");
            }
        }
    }
    fonts
}

/// Adds a font after the others of both families, for the glyphs they lack.
fn add_fallback(fonts: &mut FontDefinitions, name: &str, bytes: Vec<u8>) {
    fonts
        .font_data
        .insert(name.to_owned(), Arc::new(FontData::from_owned(bytes)));
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push(name.to_owned());
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use bt_editor::{ACTIONS, Keymap};
    use fontdue::{Font, FontSettings};

    use super::*;

    /// Every name the default macOS keymap shows, and every modifier a binding may hold, has
    /// its glyphs in some font of each family (menus and the palette draw them proportional,
    /// the Keyboard preferences monospace). Asked of the fonts themselves: egui's `has_glyph`
    /// takes a glyph of its replacement face for a missing one.
    #[test]
    fn every_key_name_on_macos_has_its_glyphs() {
        let keymap = Keymap::new(true);
        let mut names: Vec<String> = ACTIONS
            .iter()
            .flat_map(|spec| keymap.chords(spec.id))
            .map(|chord| chord.label(true))
            .collect();
        names.push("⌃⌥⇧⌘".to_owned());
        let fonts = interface_fonts();
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            let faces: Vec<Font> = fonts.families[&family]
                .iter()
                .map(|name| {
                    Font::from_bytes(&*fonts.font_data[name].font, FontSettings::default()).unwrap()
                })
                .collect();
            for name in &names {
                let lacking: String = name
                    .chars()
                    .filter(|c| faces.iter().all(|face| face.lookup_glyph_index(*c) == 0))
                    .collect();
                assert!(
                    lacking.is_empty(),
                    "{family:?}: nothing draws {lacking} of {name}"
                );
            }
        }
    }
}
