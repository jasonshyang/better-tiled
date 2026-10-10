//! The modal stack, the one place that knows whether something modal is open: a question from
//! the editor waiting for an answer (unsaved work, recovery, a file changed on disk), an error
//! with its details, or a dialog.
//! Enter gives the default answer; Escape cancels, when Cancel is an answer.

use bt_editor::{Choice, Decision, Editor, Intent, Notice, Purpose, Question};

use super::palette::PaletteDialog;

/// Something modal.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Modal {
    /// A question from the editor.
    Decision(Decision),
    /// An error with its details.
    Error(Notice),
    /// The command palette.
    Palette(PaletteDialog),
}

/// The open modals; the last is shown.
#[derive(Debug, Default)]
pub(crate) struct Modals {
    stack: Vec<Modal>,
}

impl Modals {
    /// Opens a modal over the others.
    pub(crate) fn push(&mut self, modal: Modal) {
        self.stack.push(modal);
    }

    /// Whether a modal is open.
    pub(crate) fn is_open(&self) -> bool {
        !self.stack.is_empty()
    }

    /// Whether a question about quitting is waiting for an answer.
    pub(crate) fn asks_about_quitting(&self) -> bool {
        self.stack.iter().any(|modal| {
            matches!(
                modal,
                Modal::Decision(Decision {
                    question: Question::Unsaved {
                        purpose: Purpose::Quit,
                        ..
                    },
                    ..
                })
            )
        })
    }

    /// Shows the top modal; an answer goes to `out` and closes it.
    pub(crate) fn show(&mut self, ctx: &egui::Context, editor: &Editor, out: &mut Vec<Intent>) {
        let Some(modal) = self.stack.last_mut() else {
            return;
        };
        let closed = match modal {
            Modal::Decision(decision) => show_decision(ctx, decision, out),
            Modal::Error(notice) => show_error(ctx, notice),
            Modal::Palette(palette) => palette.show(ctx, editor, out),
        };
        if closed {
            self.stack.pop();
        }
    }
}

fn show_decision(ctx: &egui::Context, decision: &Decision, out: &mut Vec<Intent>) -> bool {
    let mut answer = None;
    let response =
        egui::Modal::new(egui::Id::new("decision").with(decision.id.0)).show(ctx, |ui| {
            ui.set_max_width(420.0);
            let text = question_text(&decision.question);
            ui.heading(text.title);
            ui.add_space(4.0);
            for line in text.lines {
                ui.label(line);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for choice in &decision.choices {
                    if ui.button(choice_label(*choice)).clicked() {
                        answer = Some(*choice);
                    }
                }
            });
        });
    let keys = ctx.input(|input| KeysPressed {
        enter: input.key_pressed(egui::Key::Enter),
        escape: input.key_pressed(egui::Key::Escape),
    });
    if answer.is_none() && keys.enter {
        answer = decision.choices.first().copied();
    }
    if answer.is_none()
        && (keys.escape || response.should_close())
        && decision.choices.contains(&Choice::Cancel)
    {
        answer = Some(Choice::Cancel);
    }
    match answer {
        Some(choice) => {
            out.push(Intent::Decided {
                decision: decision.id,
                choice,
            });
            true
        }
        None => false,
    }
}

fn show_error(ctx: &egui::Context, notice: &Notice) -> bool {
    let mut closed = false;
    let response = egui::Modal::new(egui::Id::new("error")).show(ctx, |ui| {
        ui.set_max_width(480.0);
        ui.heading(&notice.text);
        if let Some(detail) = &notice.detail {
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .max_height(240.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(detail).monospace());
                });
        }
        ui.add_space(8.0);
        if ui.button("OK").clicked() {
            closed = true;
        }
    });
    let keys = ctx.input(|input| KeysPressed {
        enter: input.key_pressed(egui::Key::Enter),
        escape: input.key_pressed(egui::Key::Escape),
    });
    closed || keys.enter || keys.escape || response.should_close()
}

/// The keys a modal answers to.
#[derive(Copy, Clone, Debug)]
struct KeysPressed {
    enter: bool,
    escape: bool,
}

/// What a question says.
#[derive(Clone, Debug, PartialEq, Eq)]
struct QuestionText {
    title: String,
    lines: Vec<String>,
}

/// A question's title and lines.
fn question_text(question: &Question) -> QuestionText {
    match question {
        Question::Unsaved { names, purpose } => {
            let when = match purpose {
                Purpose::Close => "closing",
                Purpose::Quit => "quitting",
            };
            match names.as_slice() {
                [name] => QuestionText {
                    title: format!("Save the changes to “{name}” before {when}?"),
                    lines: vec!["Your changes are lost if you don't save them.".to_owned()],
                },
                _ => QuestionText {
                    title: format!("Save the changes to {} files before {when}?", names.len()),
                    lines: names.iter().map(|name| format!("• {name}")).collect(),
                },
            }
        }
        Question::Recover { names } => QuestionText {
            title: "better-tiled did not quit cleanly".to_owned(),
            lines: std::iter::once("Recover the unsaved changes to:".to_owned())
                .chain(names.iter().map(|name| format!("• {name}")))
                .collect(),
        },
        Question::Conflict { name, returned } => QuestionText {
            title: if *returned {
                format!("“{name}” is back, with other contents")
            } else {
                format!("“{name}” changed on disk")
            },
            lines: vec![
                "It has unsaved changes here. Keep them, or take the file as it is on disk?"
                    .to_owned(),
                "Reloading can be undone.".to_owned(),
            ],
        },
    }
}

fn choice_label(choice: Choice) -> &'static str {
    match choice {
        Choice::Save => "Save",
        Choice::DontSave => "Don't Save",
        Choice::Cancel => "Cancel",
        Choice::Recover => "Recover",
        Choice::Discard => "Discard",
        Choice::KeepMine => "Keep My Changes",
        Choice::Reload => "Reload",
    }
}

#[cfg(test)]
mod tests {
    use bt_editor::DecisionId;

    use super::*;

    #[test]
    fn questions_name_what_is_unsaved() {
        let one = question_text(&Question::Unsaved {
            names: vec!["desert.tmx".to_owned()],
            purpose: Purpose::Quit,
        });
        assert_eq!(
            one.title,
            "Save the changes to “desert.tmx” before quitting?"
        );
        assert_eq!(one.lines.len(), 1);
        let two = question_text(&Question::Unsaved {
            names: vec!["a.tmx".to_owned(), "b.tsx".to_owned()],
            purpose: Purpose::Close,
        });
        assert_eq!(two.title, "Save the changes to 2 files before closing?");
        assert_eq!(two.lines, ["• a.tmx", "• b.tsx"]);
    }

    #[test]
    fn the_quit_question_is_found() {
        let mut modals = Modals::default();
        assert!(!modals.is_open() && !modals.asks_about_quitting());
        modals.push(Modal::Decision(Decision {
            id: DecisionId(1),
            question: Question::Unsaved {
                names: vec!["a.tmx".to_owned()],
                purpose: Purpose::Quit,
            },
            choices: vec![Choice::Save, Choice::DontSave, Choice::Cancel],
        }));
        modals.push(Modal::Error(Notice::error("oops")));
        assert!(modals.is_open() && modals.asks_about_quitting());
    }
}
