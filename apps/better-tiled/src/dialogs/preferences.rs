//! The Preferences dialog: General, Interface, Keyboard and Theme, edited on a copy that OK
//! hands to the editor. The Keyboard page lists every command with its keys, finds commands by
//! name, records a key for one (Escape stops recording), names the commands that share it, and
//! puts defaults back.

use bt_core::Color;
use bt_editor::{
    ACTIONS, ActionId, Chord, Editor, Intent, Keymap, ObjectSelection, SettingColor, Settings,
};

use super::form::{ButtonsAnswer, DialogKeys, choice, dialog_buttons};
use crate::focus::chord_of;

/// The pages of the dialog.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
enum Page {
    #[default]
    General,
    Interface,
    Keyboard,
    Theme,
}

const PAGES: [(Page, &str); 4] = [
    (Page::General, "General"),
    (Page::Interface, "Interface"),
    (Page::Keyboard, "Keyboard"),
    (Page::Theme, "Theme"),
];

const OBJECT_SELECTIONS: [(ObjectSelection, &str); 3] = [
    (ObjectSelection::AnyLayer, "From any layer"),
    (
        ObjectSelection::PreferCurrentLayer,
        "Prefer the current layer",
    ),
    (ObjectSelection::CurrentLayerOnly, "Only the current layer"),
];

/// The dialog's state: the preferences being edited.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PreferencesDialog {
    draft: Settings,
    /// The draft's key bindings, which the Keyboard page edits.
    keymap: Keymap,
    page: Page,
    search: String,
    /// The command whose key is being recorded.
    recording: Option<ActionId>,
    /// The Keyboard page's search field was given the keyboard.
    find_focused: bool,
    mac: bool,
}

impl PreferencesDialog {
    /// The dialog over the editor's preferences.
    pub(crate) fn new(editor: &Editor) -> PreferencesDialog {
        PreferencesDialog {
            draft: editor.settings().clone(),
            keymap: editor.keymap().clone(),
            page: Page::General,
            search: String::new(),
            recording: None,
            find_focused: false,
            mac: editor.keymap().is_mac(),
        }
    }

    /// Shows the dialog; OK sends the preferences to `out`. Whether it closed.
    pub(crate) fn show(&mut self, ctx: &egui::Context, out: &mut Vec<Intent>) -> bool {
        if self.recording.is_some() {
            self.record_key(ctx);
        }
        let keys = if self.recording.is_some() {
            DialogKeys::default()
        } else {
            DialogKeys::take(ctx)
        };
        let mut answer = ButtonsAnswer::None;
        let response = egui::Modal::new(egui::Id::new("preferences")).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.heading("Preferences");
            ui.horizontal(|ui| {
                for (page, label) in PAGES {
                    if ui.selectable_label(self.page == page, label).clicked() {
                        self.page = page;
                        self.recording = None;
                        self.find_focused = false;
                    }
                }
            });
            ui.separator();
            match self.page {
                Page::General => self.general(ui),
                Page::Interface => self.interface(ui),
                Page::Keyboard => self.keyboard(ui),
                Page::Theme => self.theme(ui),
            }
            ui.separator();
            answer = dialog_buttons(ui, true);
        });
        if answer == ButtonsAnswer::Ok || keys.enter {
            out.push(Intent::ChangeSettings(Box::new(self.draft.clone())));
            return true;
        }
        answer == ButtonsAnswer::Cancel || keys.escape || response.should_close()
    }

    fn general(&mut self, ui: &mut egui::Ui) {
        let general = &mut self.draft.general;
        ui.checkbox(
            &mut general.restore_session,
            "Open the last session's files at start",
        );
        ui.checkbox(
            &mut general.safe_writing,
            "Write files safely (through a temporary file)",
        );
        ui.checkbox(
            &mut general.recovery,
            "Keep snapshots of unsaved work, to recover it after a crash",
        );
        ui.checkbox(
            &mut general.watch_files,
            "Notice files changed by other programs",
        );
        ui.add_enabled_ui(cfg!(unix), |ui| {
            ui.checkbox(&mut general.live_link, "Let bt work with the open maps")
                .on_disabled_hover_text("bt works with files only on Windows");
        });
        ui.horizontal(|ui| {
            ui.label("Undo steps kept per document");
            ui.add(egui::DragValue::new(&mut general.history_limit).range(10..=100_000));
        });
    }

    fn interface(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("interface")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                let interface = &mut self.draft.interface;
                ui.label("Grid colour");
                color_field(ui, &mut interface.grid_color);
                ui.end_row();
                ui.label("Major grid every");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut interface.major_grid.columns)
                            .range(0..=1000)
                            .suffix(" columns"),
                    );
                    ui.add(
                        egui::DragValue::new(&mut interface.major_grid.rows)
                            .range(0..=1000)
                            .suffix(" rows"),
                    );
                });
                ui.end_row();
                ui.label("Fine grid divisions");
                ui.add(
                    egui::DragValue::new(&mut interface.fine_grid_divisions)
                        .range(1..=bt_editor::MAX_FINE_GRID_DIVISIONS),
                );
                ui.end_row();
                ui.label("Highlighted layer fades to");
                color_field(ui, &mut interface.fade_color);
                ui.end_row();
                ui.label("Object outline width");
                ui.add(
                    egui::DragValue::new(&mut interface.object_line_width)
                        .range(0.5..=8.0)
                        .speed(0.1)
                        .suffix(" pt"),
                );
                ui.end_row();
                let behaviour = &mut self.draft.behaviour;
                ui.label("Objects are selected");
                choice(
                    ui,
                    "object_selection",
                    &mut behaviour.object_selection,
                    &OBJECT_SELECTIONS,
                );
                ui.end_row();
            });
        let behaviour = &mut self.draft.behaviour;
        ui.checkbox(&mut behaviour.wheel_zooms, "The mouse wheel zooms");
        ui.checkbox(
            &mut behaviour.select_enclosed,
            "A rubber band selects only the objects it encloses",
        );
        ui.checkbox(
            &mut behaviour.tool_key_returns,
            "A tool's key pressed again goes back to the previous tool",
        );
    }

    fn theme(&mut self, ui: &mut egui::Ui) {
        let theme = &mut self.draft.theme;
        ui.checkbox(&mut theme.dark, "Dark");
        ui.horizontal(|ui| {
            ui.label("Selection colour");
            color_field(ui, &mut theme.selection_color);
        });
        ui.horizontal(|ui| {
            ui.label("Font size");
            ui.add(
                egui::DragValue::new(&mut theme.font_size)
                    .range(9.0..=24.0)
                    .speed(0.1)
                    .suffix(" pt"),
            );
        });
    }

    fn keyboard(&mut self, ui: &mut egui::Ui) {
        let keymap = self.keymap.clone();
        ui.horizontal(|ui| {
            ui.label("Find");
            let find = ui.text_edit_singleline(&mut self.search);
            if !self.find_focused {
                find.request_focus();
                self.find_focused = true;
            }
            if ui.button("Reset All").clicked() {
                self.keymap.reset_all();
                self.draft.keymap = self.keymap.to_settings();
                self.recording = None;
            }
        });
        let words: Vec<&str> = self.search.split_whitespace().collect();
        let mut changed: Option<Keymap> = None;
        egui::ScrollArea::vertical()
            .max_height(360.0)
            .show(ui, |ui| {
                egui::Grid::new("keyboard")
                    .num_columns(3)
                    .striped(true)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        for spec in ACTIONS {
                            let text = format!("{} {}", spec.label, spec.keywords);
                            if bt_utils::text::search_score(&words, &text).is_none() {
                                continue;
                            }
                            let label = spec.label.trim_end_matches('…');
                            if keymap.is_customized(spec.id) {
                                ui.strong(label);
                            } else {
                                ui.label(label);
                            }
                            let chords = keymap.chords(spec.id);
                            let shown = if self.recording == Some(spec.id) {
                                "Press a key… (Escape stops)".to_owned()
                            } else if chords.is_empty() {
                                "—".to_owned()
                            } else {
                                chords
                                    .iter()
                                    .map(|c| c.label(self.mac))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            };
                            let conflicts: Vec<String> = chords
                                .iter()
                                .flat_map(|c| keymap.conflicts(spec.id, *c))
                                .map(|id| id.spec().label.trim_end_matches('…').to_owned())
                                .collect();
                            ui.horizontal(|ui| {
                                ui.monospace(shown);
                                if !conflicts.is_empty() {
                                    ui.colored_label(
                                        ui.visuals().warn_fg_color,
                                        format!("also {}", bt_utils::text::and_list(&conflicts)),
                                    );
                                }
                            });
                            ui.horizontal(|ui| {
                                if ui.small_button("Record").clicked() {
                                    self.recording = Some(spec.id);
                                }
                                if ui.small_button("Clear").clicked() {
                                    let mut edited = keymap.clone();
                                    edited.set_chords(spec.id, Vec::new());
                                    changed = Some(edited);
                                }
                                if keymap.is_customized(spec.id)
                                    && ui.small_button("Reset").clicked()
                                {
                                    let mut edited = keymap.clone();
                                    edited.reset(spec.id);
                                    changed = Some(edited);
                                }
                            });
                            ui.end_row();
                        }
                    });
            });
        if let Some(edited) = changed {
            self.keymap = edited;
            self.draft.keymap = self.keymap.to_settings();
        }
    }

    /// Takes the frame's first key press as the recorded command's key.
    fn record_key(&mut self, ctx: &egui::Context) {
        let Some(id) = self.recording else {
            return;
        };
        let pressed = ctx.input_mut(|input| {
            let found = input.events.iter().find_map(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            });
            if found.is_some() {
                input
                    .events
                    .retain(|event| !matches!(event, egui::Event::Key { .. }));
            }
            found
        });
        let Some((key, modifiers)) = pressed else {
            return;
        };
        self.recording = None;
        if key == egui::Key::Escape && modifiers.is_none() {
            return;
        }
        if let Some(chord) = chord_of(key, modifiers, self.mac) {
            self.bind(id, chord);
        }
    }

    /// Binds `id` to `chord` alone.
    fn bind(&mut self, id: ActionId, chord: Chord) {
        self.keymap.set_chords(id, vec![chord]);
        self.draft.keymap = self.keymap.to_settings();
    }
}

/// A colour field over a setting's colour, opaque.
fn color_field(ui: &mut egui::Ui, color: &mut SettingColor) {
    let mut shown = egui::Color32::from_rgb(color.0.r, color.0.g, color.0.b);
    if egui::color_picker::color_edit_button_srgba(
        ui,
        &mut shown,
        egui::color_picker::Alpha::Opaque,
    )
    .changed()
    {
        *color = SettingColor(Color::rgba(shown.r(), shown.g(), shown.b(), color.0.a));
    }
}
