//! What a script did (`10-agents.md`, §9): the typed outcome of a run, the same whether `bt` ran
//! it on a file or the editor through the live link. The runner fills what it knows: the edit's
//! label, the number of operations, the map's changes (on a map with AutoMap layers, as the
//! rules make them, with what the script painted under them), the cells no tile fits, the notes
//! line by line, the changed cells and their bounds. The caller fills the rest when it has it:
//! warnings from reading and writing, the problems the edit made (`bt check`'s `made_by`), the
//! changed area as text, and where the edit went.

use bt_utils::IRect;
use bt_utils::text::plural;
use glam::IVec2;
use serde::{Deserialize, Serialize};

use crate::changes::MapChanges;
use crate::report::{Line, LineKind, Outcome, Report, Section};
use crate::words::{cells_text, rect_option_json};

/// The most cells no tile fits that the text lists.
const UNFILLED_LISTED: usize = 12;

/// What an edit script did.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EditOutcome {
    /// The map, as reports name it (its file's name).
    pub map: String,
    /// The edit's name in the editor's history: the script's first-line comment, else its first
    /// operation that changes the map, with how many more follow.
    pub label: String,
    /// How many operations the script has.
    pub operations: usize,
    /// What changed on the map: on a map with AutoMap layers, as the rules make it.
    pub changes: MapChanges,
    /// On a map with AutoMap layers (before or after the script), what the script changed in
    /// what was painted.
    pub painted: Option<MapChanges>,
    /// The cells no terrain tile fits, left as they were, row by row.
    pub unfilled: Vec<IVec2>,
    /// How operations were carried out, line by line.
    pub notes: Vec<LineNote>,
    /// The cells that changed (and where objects that changed stand), row by row.
    pub cells: Vec<IVec2>,
    /// The rectangle holding them; none when none changed.
    #[serde(with = "rect_option_json")]
    pub area: Option<IRect>,
    /// Whether the edit changed anything: the map, what was painted, or its AutoMap layers.
    pub changed: bool,
    /// What reading the map or writing the result warned about, as the caller says it.
    pub warnings: Vec<String>,
    /// The problems the edit made that `bt check` would list (new seams, new hidden edges,
    /// objects that stood on one ground and stand on another now); empty when it made none;
    /// none when they were not looked for.
    pub made: Option<Vec<String>>,
    /// The changed area as text, when asked for (`--view`).
    pub view: Option<String>,
    /// Where the edit went, once the caller knows.
    pub saved: Option<Saved>,
}

/// A note on how an operation was carried out.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineNote {
    /// The operation's line, from 1.
    pub line: usize,
    /// Whether the line is one of the `--with` file's.
    pub in_definitions: bool,
    /// [`LineKind::Note`] for how it was carried out, [`LineKind::Warning`] for what may keep
    /// the result from being what it seems.
    pub kind: LineKind,
    /// What happened.
    pub text: String,
}

/// Where an edit went.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "to", rename_all = "snake_case")]
pub enum Saved {
    /// The file was written.
    File {
        /// The file, as reports name it.
        file: String,
        /// The project file that keeps what was painted, on a map with AutoMap layers.
        painted_in: Option<String>,
        /// The command that takes the edit back, when the previous version was kept.
        undo: Option<String>,
    },
    /// The edit is in the editor as one undo step, not saved yet.
    InEditor {
        /// The map, as commands name it.
        map: String,
    },
    /// A dry run: nothing was written or applied.
    DryRun {
        /// Whether the map is open in the editor.
        in_editor: bool,
    },
    /// Nothing changed, so nothing was written or applied.
    Unchanged {
        /// Whether the map is open in the editor.
        in_editor: bool,
    },
}

impl Saved {
    /// Where the edit went, in words.
    fn text(&self) -> String {
        match self {
            Saved::File {
                file,
                painted_in,
                undo,
            } => {
                let mut text = format!("saved {file}");
                if let Some(project) = painted_in {
                    text.push_str(&format!(", and what was painted in {project}"));
                }
                if let Some(undo) = undo {
                    text.push_str(&format!("; {undo} puts it back"));
                }
                text
            }
            Saved::InEditor { map } => format!(
                "applied in the editor as one undo step (bt undo {map} takes it back); not saved \
                 yet: bt save {map} writes the file"
            ),
            Saved::DryRun { in_editor: false } => "dry run: nothing saved".to_owned(),
            Saved::DryRun { in_editor: true } => {
                "dry run: the map in the editor is unchanged".to_owned()
            }
            Saved::Unchanged { in_editor: false } => "nothing to save".to_owned(),
            Saved::Unchanged { in_editor: true } => "nothing to change in the editor".to_owned(),
        }
    }
}

impl LineNote {
    /// The note as a line: `note: line 3: …`.
    fn line(&self) -> Line {
        let place = if self.in_definitions {
            format!("--with file, line {}", self.line)
        } else {
            format!("line {}", self.line)
        };
        Line::new(self.kind, format!("{place}: {}", self.text))
    }
}

impl EditOutcome {
    /// This outcome with the problems the edit made, as `bt check`'s `made_by` gives them.
    pub fn with_made(mut self, problems: Vec<String>) -> EditOutcome {
        self.made = Some(problems);
        self
    }

    /// This outcome with the changed area as text.
    pub fn with_view(mut self, view: String) -> EditOutcome {
        self.view = Some(view);
        self
    }

    /// This outcome with where the edit went.
    pub fn with_saved(mut self, saved: Saved) -> EditOutcome {
        self.saved = Some(saved);
        self
    }

    /// This outcome with a warning from reading the map or writing the result.
    pub fn with_warning(mut self, warning: impl Into<String>) -> EditOutcome {
        self.warnings.push(warning.into());
        self
    }
}

impl Outcome for EditOutcome {
    const SCHEMA: &'static str = "bt.edit/1";

    fn report(&self) -> Report {
        let mut report = Report::new();
        for warning in &self.warnings {
            report.add_line(Line::warning(warning.clone()));
        }
        if let Some(made) = &self.made
            && !made.is_empty()
        {
            let mut section = Section::new();
            section.add_lines(made.iter().map(|problem| Line::problem(problem.clone())));
            report.add_section(section);
        }
        let mut changes = Section::headed(format!(
            "{}: {}",
            self.map,
            plural(self.operations, "operation", "operations")
        ));
        let painted_changed = self.painted.as_ref().is_some_and(|p| !p.is_empty());
        if self.changes.is_empty() {
            changes.add_line(Line::fact(if painted_changed {
                "nothing changed on the map: the rules make the same of what was painted"
            } else {
                "nothing changed"
            }));
        }
        changes.add_lines(self.changes.lines());
        if let Some(painted) = self.painted.as_ref().filter(|p| !p.is_empty()) {
            changes.add_line(
                Line::fact(
                    "what was painted (the changes above are what the AutoMap rules made of it):",
                )
                .with_details(painted.lines()),
            );
        }
        report.add_section(changes);
        if !self.unfilled.is_empty() {
            report.add_line(Line::warning(format!(
                "no tile fits {}, left as they were: {}",
                plural(self.unfilled.len(), "cell", "cells"),
                cells_text(&self.unfilled, UNFILLED_LISTED)
            )));
        }
        let mut notes = Section::new();
        notes.add_lines(self.notes.iter().map(LineNote::line));
        report.add_section(notes);
        if self.made.as_ref().is_some_and(Vec::is_empty) && self.changed {
            report.add_line(Line::fact(
                "no new seams or hidden edges, and no objects on other ground",
            ));
        }
        if let Some(view) = &self.view {
            let mut section = Section::new();
            section.add_lines(view.lines().map(Line::fact));
            report.add_section(section);
        }
        if let Some(saved) = &self.saved {
            report.add_line(Line::fact(saved.text()));
        }
        report
    }
}
