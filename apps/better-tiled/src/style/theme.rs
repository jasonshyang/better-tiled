//! The egui style from the theme settings: dark or light, the selection colour and the size of
//! interface text.

use bt_editor::Theme;
use egui::{Color32, FontId, TextStyle};

/// Styles the window after `theme`.
pub(crate) fn apply_theme(ctx: &egui::Context, theme: &Theme) {
    let mut visuals = if theme.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let [r, g, b, _] = theme.selection_color.0.to_array();
    visuals.selection.bg_fill = Color32::from_rgb(r, g, b);
    ctx.set_visuals(visuals);
    let size = theme.font_size.clamp(8.0, 32.0);
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(size * 0.75)),
            (TextStyle::Body, FontId::proportional(size)),
            (TextStyle::Button, FontId::proportional(size)),
            (TextStyle::Heading, FontId::proportional(size * 1.25)),
            (TextStyle::Monospace, FontId::monospace(size * 0.95)),
        ]
        .into();
    });
}
