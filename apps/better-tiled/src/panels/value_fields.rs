//! The value fields of the Properties panel and the Custom Types editor (Tiled's property
//! editors, `reference/tiled/src/tiled/propertyeditorwidgets.cpp`): text and numbers typed in
//! place, checkboxes, choice boxes, flag checkboxes and colours with "none". A field shows
//! "mixed" when its targets differ, and is greyed out when it cannot be set.
//!
//! What is typed applies as it is typed: the edits of one run of typing (from the field taking
//! the keyboard to Enter, Escape or a click elsewhere) share a run number, so they fold into one
//! step. Escape puts back the value the field had, when its targets agreed on one. A colour
//! dragged in its picker is one run per press.

use bt_core::Color;

/// What a field shows.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Field {
    /// The field's widget, the same from frame to frame.
    pub(crate) id: egui::Id,
    /// The label naming it (screen readers and tests find it by that name).
    pub(crate) label: egui::Id,
    /// Whether its targets' values differ.
    pub(crate) mixed: bool,
    /// Whether it can be set.
    pub(crate) enabled: bool,
}

/// A value a field was given, and the run of edits it belongs to (typing, dragging).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Edited<V> {
    /// The value.
    pub(crate) value: V,
    /// The run, if it is part of one.
    pub(crate) merge: Option<u64>,
}

impl<V> Edited<V> {
    /// A value given at once (a click), not part of a run.
    fn at_once(value: V) -> Edited<V> {
        Edited { value, merge: None }
    }
}

/// The text typed in the field that has the keyboard.
#[derive(Clone, Debug)]
struct Typing {
    id: egui::Id,
    text: String,
    /// What the field showed when it took the keyboard; `None` when it was mixed.
    original: Option<String>,
    run: u64,
}

/// A colour being dragged in its picker.
#[derive(Copy, Clone, Debug)]
struct ColorRun {
    id: egui::Id,
    run: u64,
}

/// The fields' own state: what is being typed, and the runs handed out.
#[derive(Debug, Default)]
pub(crate) struct Fields {
    typing: Option<Typing>,
    color: Option<ColorRun>,
    runs: u64,
}

impl Fields {
    /// Forgets what was being typed (what the fields show changed).
    pub(crate) fn forget_typing(&mut self) {
        self.typing = None;
        self.color = None;
    }

    /// A new run number.
    fn next_run(&mut self) -> u64 {
        self.runs += 1;
        self.runs
    }

    /// A text field showing `current`; what is typed is read with `parse`, and text that does
    /// not read changes nothing.
    pub(crate) fn text<V>(
        &mut self,
        ui: &mut egui::Ui,
        field: Field,
        current: &str,
        parse: impl Fn(&str) -> Option<V>,
    ) -> Option<Edited<V>> {
        let typing_here = self.typing.as_ref().is_some_and(|t| t.id == field.id);
        let mut shown = match &self.typing {
            Some(typing) if typing_here => typing.text.clone(),
            _ if field.mixed => String::new(),
            _ => current.to_owned(),
        };
        let mut edit = egui::TextEdit::singleline(&mut shown)
            .id(field.id)
            .desired_width(f32::INFINITY);
        if field.mixed && !typing_here {
            edit = edit.hint_text("mixed");
        }
        let response = ui.add_enabled(field.enabled, edit).labelled_by(field.label);
        if response.has_focus() && !typing_here {
            let run = self.next_run();
            self.typing = Some(Typing {
                id: field.id,
                text: shown.clone(),
                original: (!field.mixed).then(|| current.to_owned()),
                run,
            });
        }
        let typing = self.typing.as_mut().filter(|t| t.id == field.id)?;
        let mut edited = None;
        if response.changed() {
            typing.text = shown;
            edited = parse(&typing.text).map(|value| Edited {
                value,
                merge: Some(typing.run),
            });
        }
        if response.lost_focus() {
            let escaped = ui.input(|input| input.key_pressed(egui::Key::Escape));
            if escaped
                && let Some(original) = &typing.original
                && *original != typing.text
            {
                edited = parse(original).map(|value| Edited {
                    value,
                    merge: Some(typing.run),
                });
            }
            self.typing = None;
        } else if !response.has_focus() {
            // The field lost the keyboard while it was not drawn.
            self.typing = None;
        }
        edited
    }

    /// A checkbox; a mixed one shows neither state.
    pub(crate) fn checkbox(
        &mut self,
        ui: &mut egui::Ui,
        field: Field,
        current: bool,
    ) -> Option<Edited<bool>> {
        let mut value = current;
        let response = ui
            .add_enabled(
                field.enabled,
                egui::Checkbox::without_text(&mut value).indeterminate(field.mixed),
            )
            .labelled_by(field.label);
        let toggled = response.clicked() || response.changed();
        toggled.then(|| Edited::at_once(if field.mixed { true } else { !current }))
    }

    /// A choice box over `options`, showing the one at `selected` (none when the targets
    /// differ or the value is not among them, `shown` then).
    pub(crate) fn choice(
        &mut self,
        ui: &mut egui::Ui,
        field: Field,
        selected: Option<usize>,
        shown: &str,
        options: &[String],
    ) -> Option<Edited<usize>> {
        let text = match selected {
            _ if field.mixed => "mixed",
            Some(index) => options.get(index).map_or(shown, String::as_str),
            None => shown,
        };
        let mut chosen = None;
        let combo = ui.add_enabled_ui(field.enabled, |ui| {
            egui::ComboBox::from_id_salt(field.id)
                .selected_text(text)
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for (index, option) in options.iter().enumerate() {
                        let is_selected = selected == Some(index) && !field.mixed;
                        if ui.selectable_label(is_selected, option).clicked() && !is_selected {
                            chosen = Some(index);
                        }
                    }
                })
        });
        combo.inner.response.labelled_by(field.label);
        chosen.map(Edited::at_once)
    }

    /// A checkbox for each of `names`, the flags `current` holds checked.
    pub(crate) fn flags(
        &mut self,
        ui: &mut egui::Ui,
        field: Field,
        current: i64,
        names: &[String],
    ) -> Option<Edited<i64>> {
        let mut edited = None;
        ui.horizontal_wrapped(|ui| {
            for (bit, name) in names.iter().enumerate() {
                let flag = 1_i64 << bit;
                let mut set = current & flag != 0;
                let checkbox = egui::Checkbox::new(&mut set, name).indeterminate(field.mixed);
                if ui.add_enabled(field.enabled, checkbox).changed() {
                    let flags = if set { current | flag } else { current & !flag };
                    edited = Some(Edited::at_once(flags));
                }
            }
        });
        edited
    }

    /// A colour button, with a button that clears it when `can_clear` (no colour).
    pub(crate) fn color(
        &mut self,
        ui: &mut egui::Ui,
        field: Field,
        current: Option<Color>,
        can_clear: bool,
    ) -> Option<Edited<Option<Color>>> {
        let mut rgba = current.map_or([0, 0, 0, 0], Color::to_array);
        let mut edited = None;
        ui.add_enabled_ui(field.enabled, |ui| {
            let response = ui
                .color_edit_button_srgba_unmultiplied(&mut rgba)
                .labelled_by(field.label);
            if response.changed() {
                let pressed = ui.input(|input| input.pointer.any_pressed());
                let run = match self.color {
                    Some(color) if color.id == field.id && !pressed => color.run,
                    _ => self.next_run(),
                };
                self.color = Some(ColorRun { id: field.id, run });
                let [r, g, b, a] = rgba;
                edited = Some(Edited {
                    value: Some(Color::rgba(r, g, b, a)),
                    merge: Some(run),
                });
            }
            let text = if field.mixed { "mixed" } else { "" };
            if current.is_none() && !field.mixed {
                ui.weak("none");
            } else if !text.is_empty() {
                ui.weak(text);
            }
            if can_clear
                && current.is_some()
                && ui.small_button("✕").on_hover_text("No color").clicked()
            {
                edited = Some(Edited::at_once(None));
            }
        });
        edited
    }
}

/// A row's name, `width` wide and aligned left, as a field's label.
pub(crate) fn name_cell(ui: &mut egui::Ui, width: f32, label: egui::Label) -> egui::Response {
    let size = egui::vec2(width, ui.spacing().interact_size.y);
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    ui.allocate_ui_with_layout(size, layout, |ui| {
        ui.set_min_width(width);
        ui.add(label)
    })
    .inner
}

/// A number as it is typed: an integer as it is, a real number in its shortest form.
pub(crate) fn number_text(value: f64) -> String {
    format!("{value}")
}

/// Typed text read as an integer.
pub(crate) fn parse_integer(text: &str) -> Option<i64> {
    text.trim().parse().ok()
}

/// Typed text read as a finite real number.
pub(crate) fn parse_number(text: &str) -> Option<f64> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_numbers_read_as_numbers() {
        assert_eq!(parse_integer(" 42 "), Some(42));
        assert_eq!(parse_integer("4.2"), None);
        assert_eq!(parse_number("-0.5"), Some(-0.5));
        assert_eq!(parse_number("inf"), None);
        assert_eq!(parse_number("-"), None);
        assert_eq!(number_text(2.0), "2");
        assert_eq!(number_text(0.25), "0.25");
    }
}
