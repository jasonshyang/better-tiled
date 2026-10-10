//! The command palette: a search field over the editor's ranked entries (commands, recent
//! files, layers, a go-to). Arrows choose, Enter runs, Escape closes; matched letters are
//! highlighted and what cannot run now is dimmed.

use bt_editor::{Editor, Intent, PaletteEntry};

/// How many entries are listed.
const SHOWN: usize = 12;

/// The palette's state while it is open.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PaletteDialog {
    query: String,
    selected: usize,
    focused: bool,
}

impl PaletteDialog {
    /// Shows the palette; a chosen entry goes to `out`. Whether it closed.
    pub(crate) fn show(
        &mut self,
        ctx: &egui::Context,
        editor: &Editor,
        out: &mut Vec<Intent>,
    ) -> bool {
        let entries = editor.palette(&self.query, SHOWN);
        let (up, down, enter, escape) = ctx.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        if down && self.selected + 1 < entries.len() {
            self.selected += 1;
        }
        if up {
            self.selected = self.selected.saturating_sub(1);
        }
        self.selected = self.selected.min(entries.len().saturating_sub(1));
        let mut chosen = None;
        let response = egui::Modal::new(egui::Id::new("palette")).show(ctx, |ui| {
            ui.set_width(520.0);
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Search commands, files and layers, or type x, y")
                    .desired_width(f32::INFINITY),
            );
            if !self.focused {
                field.request_focus();
                self.focused = true;
            }
            if field.changed() {
                self.selected = 0;
            }
            ui.add_space(4.0);
            if entries.is_empty() {
                ui.weak("Nothing matches");
            }
            for (index, entry) in entries.iter().enumerate() {
                let row = ui.add(egui::Button::selectable(
                    index == self.selected,
                    entry_text(ui, entry),
                ));
                if row.clicked() {
                    chosen = Some(index);
                }
            }
        });
        if enter {
            chosen = Some(self.selected);
        }
        if let Some(entry) = chosen.and_then(|index| entries.get(index)) {
            out.push(Intent::RunPalette(entry.target.clone()));
            return true;
        }
        escape || response.should_close()
    }
}

/// An entry's label with its matched letters highlighted, and its detail to the right.
fn entry_text(ui: &egui::Ui, entry: &PaletteEntry) -> egui::text::LayoutJob {
    let style = ui.style();
    let base = if entry.enabled {
        style.visuals.text_color()
    } else {
        style.visuals.weak_text_color()
    };
    let strong = style.visuals.strong_text_color();
    let font = egui::TextStyle::Body.resolve(style);
    let mut job = egui::text::LayoutJob::default();
    for (index, c) in entry.label.chars().enumerate() {
        let matched = entry.matched.contains(&index);
        let format = egui::TextFormat {
            font_id: font.clone(),
            color: if matched && entry.enabled {
                strong
            } else {
                base
            },
            underline: if matched {
                egui::Stroke::new(1.0, base)
            } else {
                egui::Stroke::NONE
            },
            ..egui::TextFormat::default()
        };
        job.append(&c.to_string(), 0.0, format);
    }
    if !entry.detail.is_empty() {
        job.append(
            &entry.detail,
            16.0,
            egui::TextFormat {
                font_id: font,
                color: style.visuals.weak_text_color(),
                ..egui::TextFormat::default()
            },
        );
    }
    job
}
