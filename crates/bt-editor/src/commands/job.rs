//! Tiled command placeholders become literal argument values, so quotes in a filename or an
//! object name cannot introduce more arguments or shell syntax.

use crate::{DocumentId, Editor};
use bt_utils::process::{ProcessExit, ProcessJob, ProcessText};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};

/// A project command ready to run, detached from subsequent document or project edits.
#[derive(Clone, Debug)]
pub struct CommandJob {
    name: String,
    process: ProcessJob,
    directory: String,
    show_output: bool,
}

impl CommandJob {
    fn new(name: String, process: ProcessJob) -> Self {
        Self {
            name,
            process,
            directory: String::new(),
            show_output: true,
        }
    }
    fn directory(mut self, directory: String) -> Self {
        self.directory = directory;
        self
    }
    fn show_output(mut self, shown: bool) -> Self {
        self.show_output = shown;
        self
    }
    /// The name shown in the console.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Whether starting this command brings the console forward.
    pub fn shows_output(&self) -> bool {
        self.show_output
    }
    /// The literal process request, for previews and tests.
    pub fn process(&self) -> &ProcessJob {
        &self.process
    }

    /// Runs on a worker. `%executablepath` resolves there, where filesystem work belongs.
    pub fn run(
        &self,
        cancel: Arc<AtomicBool>,
        output: impl Fn(ProcessText) + Send + Sync + 'static,
    ) -> Result<ProcessExit, String> {
        let directory = if self.directory.trim().is_empty() {
            None
        } else {
            let mut directory = self.directory.clone();
            if directory.contains("%executablepath") {
                let program = bt_utils::process::executable(self.process.program())?;
                let parent = program
                    .parent()
                    .ok_or("The executable has no parent folder")?;
                directory = directory.replace("%executablepath", &parent.to_string_lossy());
            }
            Some(PathBuf::from(directory))
        };
        self.process
            .clone()
            .directory(directory)
            .run(cancel, output)
    }
}

pub(super) fn prepare(
    editor: &Editor,
    doc: DocumentId,
    command: &bt_core::Command,
) -> Result<CommandJob, String> {
    let document = editor
        .document(doc)
        .ok_or("The document is no longer open")?;
    let map = editor.map_of(doc);
    let file = editor.path_of(doc);
    let export = map
        .and_then(|map| map.file.export.target.as_ref())
        .map(|file| file.path.clone());
    let current_object = document
        .selection()
        .objects
        .first()
        .and_then(|id| map?.object(*id));
    let tile_ids = document.tileset_editing().map(|editing| {
        editing
            .selected_tiles()
            .iter()
            .map(|id| id.0.to_string())
            .collect::<Vec<_>>()
            .join(",")
    });
    let text =
        |path: Option<&std::path::Path>| path.map(|path| path.to_string_lossy().into_owned());
    let variables = BTreeMap::from([
        ("mapfile", text(file.as_deref())),
        (
            "mappath",
            text(file.as_deref().and_then(std::path::Path::parent)),
        ),
        ("exportfile", text(export.as_deref())),
        (
            "exportpath",
            text(export.as_deref().and_then(std::path::Path::parent)),
        ),
        (
            "projectpath",
            text(editor.project_path().and_then(std::path::Path::parent)),
        ),
        (
            "layername",
            document
                .selection()
                .current_layer
                .and_then(|id| map?.layer(id))
                .map(|layer| layer.name.clone()),
        ),
        ("tileid", tile_ids),
        (
            "objectid",
            current_object.map(|object| object.id.0.to_string()),
        ),
        (
            "objectclass",
            current_object.map(|object| object.class.clone()),
        ),
        (
            "objecttype",
            current_object.map(|object| object.class.clone()),
        ),
        ("worldfile", None),
    ]);
    let program = command.command.trim();
    let program = if program.starts_with('"') {
        let mut parts = bt_utils::command_line::split(program)?;
        if parts.len() != 1 {
            return Err(
                "The executable field must contain one program; use Arguments for its arguments"
                    .into(),
            );
        }
        parts.remove(0)
    } else {
        program.to_owned()
    };
    let program = substitute(&program, &variables)?;
    if program.is_empty() {
        return Err("The command has no executable".into());
    }
    let arguments = bt_utils::command_line::split(&command.arguments)?
        .iter()
        .map(|arg| substitute(arg, &variables))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CommandJob::new(
        command.name.clone(),
        ProcessJob::new(program).arguments(arguments),
    )
    .directory(substitute(&command.working_directory, &variables)?)
    .show_output(command.show_output))
}

fn substitute(
    template: &str,
    variables: &BTreeMap<&str, Option<String>>,
) -> Result<String, String> {
    let mut output = String::new();
    let mut rest = template;
    while let Some(index) = rest.find('%') {
        let prefix = rest.get(..index).ok_or("Invalid placeholder boundary")?;
        output.push_str(prefix);
        rest = rest
            .get(index + 1..)
            .ok_or("Invalid placeholder boundary")?;
        if let Some((name, value)) = variables.iter().find(|(name, _)| rest.starts_with(**name)) {
            output.push_str(
                value
                    .as_deref()
                    .ok_or_else(|| format!("%{name} is unavailable in the current document"))?,
            );
            rest = rest
                .get(name.len()..)
                .ok_or("Invalid placeholder boundary")?;
        } else {
            output.push('%');
        }
    }
    output.push_str(rest);
    Ok(output)
}
