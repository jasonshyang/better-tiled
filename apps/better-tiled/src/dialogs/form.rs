//! What the dialogs share: Enter and Escape, choice boxes, and the OK and Cancel buttons.

/// The keys a dialog answers to, taken from the frame's input so nothing else acts on them.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DialogKeys {
    pub(crate) enter: bool,
    pub(crate) escape: bool,
}

impl DialogKeys {
    /// Takes Enter and Escape out of the frame's input.
    pub(crate) fn take(ctx: &egui::Context) -> DialogKeys {
        ctx.input_mut(|input| DialogKeys {
            enter: input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            escape: input.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
        })
    }
}

/// Which of a dialog's buttons was pressed.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum ButtonsAnswer {
    /// Neither.
    None,
    /// OK.
    Ok,
    /// Cancel.
    Cancel,
}

/// Cancel and OK, right-aligned; OK only when `ok_enabled`.
pub(crate) fn dialog_buttons(ui: &mut egui::Ui, ok_enabled: bool) -> ButtonsAnswer {
    let mut answer = ButtonsAnswer::None;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui
            .add_enabled(ok_enabled, egui::Button::new("OK"))
            .clicked()
        {
            answer = ButtonsAnswer::Ok;
        }
        if ui.button("Cancel").clicked() {
            answer = ButtonsAnswer::Cancel;
        }
    });
    answer
}

/// A choice box over `options`, each a value with its label.
pub(crate) fn choice<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut T,
    options: &[(T, &str)],
) {
    let selected = options
        .iter()
        .find(|(option, _)| option == value)
        .map_or("", |(_, label)| *label);
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            for (option, label) in options {
                ui.selectable_value(value, *option, *label);
            }
        });
}
