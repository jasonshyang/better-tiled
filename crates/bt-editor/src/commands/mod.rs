//! Project commands: edits belong to the project asset; a run snapshots its placeholders after
//! any required save, then hands process work to the window.

mod job;
pub use job::CommandJob;

use crate::{Chord, DocumentId, Editor, Effect, Notice, Severity};
use bt_core::Command;

impl Editor {
    /// The open project's external commands.
    pub fn project_commands(&self) -> &[Command] {
        self.files
            .project
            .and_then(|id| self.store.project(id))
            .map_or(&[], |project| project.commands.as_slice())
    }

    pub(crate) fn set_project_commands(
        &mut self,
        doc: DocumentId,
        commands: Vec<Command>,
    ) -> Vec<Effect> {
        for command in &commands {
            let problem = if command.name.trim().is_empty() {
                Some("Give each command a name")
            } else if command.command.trim().is_empty() {
                Some("Give each command an executable")
            } else if !command.shortcut.is_empty() && Chord::from_qt(&command.shortcut).is_none() {
                Some("A command shortcut is not a supported Qt key chord")
            } else {
                None
            };
            if let Some(problem) = problem {
                return vec![Effect::Notify(Notice::error(problem))];
            }
        }
        if self.document(doc).is_none() {
            return vec![Effect::Notify(Notice::error(
                "The document that opened Edit Commands is no longer open",
            ))];
        }
        let project = self.project_for_edits();
        let result = self.transact(doc, "Edit Commands", |tx| {
            let target = &mut tx.project_mut(project)?.commands;
            if *target == commands {
                return Ok(false);
            }
            *target = commands;
            Ok::<_, crate::EditError>(true)
        });
        result
            .err()
            .map(|error| Effect::Notify(Notice::error(error.to_string())))
            .into_iter()
            .collect()
    }

    pub(crate) fn run_project_command(&mut self, index: usize) -> Vec<Effect> {
        let Some(command) = self.project_commands().get(index).cloned() else {
            return vec![Effect::Notify(Notice::error(
                "That project command no longer exists",
            ))];
        };
        if !command.enabled {
            return vec![Effect::Notify(Notice::error(
                "That project command is disabled",
            ))];
        }
        let Some(doc) = self.current() else {
            return vec![Effect::Notify(Notice::error(
                "Open a document before running a project command",
            ))];
        };
        let mut effects = Vec::new();
        if command.save_before_execute {
            if self.path_of(doc).is_none() {
                return vec![Effect::Notify(Notice::error(
                    "Save the document to a file before running this command",
                ))];
            }
            effects.extend(self.save(doc));
            if effects.iter().any(|effect| {
                matches!(effect, Effect::Notify(notice) if notice.severity == Severity::Error)
                    || matches!(effect, Effect::PickFile(_))
            }) {
                effects.push(Effect::Notify(Notice::error(
                    "The command did not run because its required save failed",
                )));
                return effects;
            }
        }
        match job::prepare(self, doc, &command) {
            Ok(job) => effects.push(Effect::RunCommand(job)),
            Err(error) => effects.push(Effect::Notify(Notice::error(error))),
        }
        effects
    }

    /// The enabled project command bound to a chord. Built-in actions get first choice in the
    /// window; disabled commands never consume a key.
    pub fn project_command_for(&self, chord: Chord) -> Option<usize> {
        self.project_commands()
            .iter()
            .position(|command| command.enabled && Chord::from_qt(&command.shortcut) == Some(chord))
    }
}
