//! One AutoMap problem as a line: its severity's mark in its colour, then what it says.

use bt_automap::{Problem, Severity};

/// Draws `problem` as one wrapped line.
pub(crate) fn problem_line(ui: &mut egui::Ui, problem: &Problem) {
    let (mark, color) = match problem.severity() {
        Severity::Error => ("⛔", ui.visuals().error_fg_color),
        Severity::Warning => ("⚠", ui.visuals().warn_fg_color),
        Severity::Note => ("ℹ", ui.visuals().weak_text_color()),
    };
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(color, mark);
        ui.label(&problem.text);
    });
}
