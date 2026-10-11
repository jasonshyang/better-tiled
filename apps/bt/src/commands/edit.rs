//! `bt edit` (`10-agents.md`, §8, §12, §13): a script run on a map, all or nothing. A map open
//! in the editor is edited there, as one undo step that is not saved, unless `--out` names
//! another file; otherwise the file is written, its previous bytes kept for `bt undo`, and on a
//! map with AutoMap layers what was painted kept in its project file. With `--do`, the
//! operations run on each map given in turn, and the first that fails stops the rest.
//! `--render` draws the result with the change outlined.

use std::io::{IsTerminal as _, Read as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, anyhow};
use bt_live::{EditRequest, MarkSpec, Request};
use bt_script::{
    EditOutcome, EditResult, EditToKeep, History, Outcome, Saved, Script, parse_script, run_script,
    split_do, to_json_pretty,
};
use bt_tmx::WriteRequest;
use bt_utils::atomic_write::{WriteMode, write_file};
use bt_utils::paths::display_name;

use crate::cli::{EditArgs, ViewBy};
use crate::commands::render::{device, draw_change};
use crate::exit::{Printed, Status};
use crate::link::OpenMap;
use crate::load::{LoadedMap, MapCopy, load_copy, loader_for};
use crate::record::{put_back_record, record_json, write_layers_record};
use crate::run::Run;
use crate::script_input::{definitions_text, read_definitions, script_input};

/// What `bt edit` runs, read once for every map.
struct EditJob {
    /// The maps, as given.
    maps: Vec<PathBuf>,
    /// The script's text.
    text: String,
    /// The script.
    script: Script,
    /// The text of the `--with` file; empty without one.
    prelude: String,
}

/// Runs `bt edit`.
pub(crate) fn edit(run: &Run, args: &EditArgs) -> anyhow::Result<Printed> {
    if let Some(problem) = usage_problem(args) {
        return Ok(Printed::stopped("", Status::Usage, problem));
    }
    if args.render.is_some() {
        // Before anything is edited: a picture that can't be drawn fails the command.
        device()?;
    }
    let job = read_job(args)?;
    let mut done: Vec<EditOutcome> = Vec::new();
    for map in &job.maps {
        match edit_map(run, args, &job, map) {
            Ok(outcome) => done.push(outcome),
            Err(error) if done.is_empty() => return Err(error),
            Err(error) => {
                let edited: Vec<String> = done.iter().map(|o| o.map.clone()).collect();
                let failure = format!(
                    "{}: {error:#}; it and the maps after it were left as they were ({} edited)",
                    map.display(),
                    edited.join(", ")
                );
                return Ok(Printed::stopped(
                    printed_text(&done, run.cli.json)?,
                    Status::Failed,
                    failure,
                ));
            }
        }
    }
    Ok(Printed::done(printed_text(&done, run.cli.json)?))
}

/// Why the arguments cannot be run, if they cannot.
fn usage_problem(args: &EditArgs) -> Option<String> {
    let several = args.paths.len() > 1;
    if args.ops.is_none() && args.paths.len() > 2 {
        return Some(
            "several maps take their operations from --do: bt edit A.tmx B.tmx --do \"…\"".into(),
        );
    }
    if args.ops.is_some()
        && several
        && (args.out.is_some() || args.render.is_some() || args.mark.is_some())
    {
        return Some("--out, --render and --mark take one map".into());
    }
    let from_input = args.ops.is_none()
        && args
            .paths
            .get(1)
            .is_none_or(|script| script.as_os_str() == "-");
    if from_input && std::io::stdin().is_terminal() {
        return Some(
            "give the script: bt edit MAP SCRIPT, bt edit MAP - with the script on standard \
             input, or bt edit MAP --do \"…\""
                .into(),
        );
    }
    None
}

/// The maps and the script the arguments name, read and parsed.
fn read_job(args: &EditArgs) -> anyhow::Result<EditJob> {
    let maps = match &args.ops {
        Some(_) => args.paths.clone(),
        None => args.paths.iter().take(1).cloned().collect(),
    };
    let text = match (&args.ops, args.paths.get(1)) {
        (Some(ops), _) => split_do(ops),
        (None, Some(file)) if file.as_os_str() != "-" => std::fs::read_to_string(file)
            .with_context(|| format!("the script {} can't be read", file.display()))?,
        (None, _) => {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .context("the script can't be read from standard input")?;
            text
        }
    };
    let script = parse_script(&text).map_err(|error| anyhow!("{error}"))?;
    let prelude = match &args.with {
        Some(file) => {
            read_definitions(file)?;
            definitions_text(file)?
        }
        None => String::new(),
    };
    Ok(EditJob {
        maps,
        text,
        script,
        prelude,
    })
}

/// Edits one map: in the editor when it is open there and `--out` names no other file, else its
/// file.
fn edit_map(run: &Run, args: &EditArgs, job: &EditJob, map: &Path) -> anyhow::Result<EditOutcome> {
    let path =
        std::fs::canonicalize(map).with_context(|| format!("{} can't be read", map.display()))?;
    let link = run.link();
    if args.out.is_none()
        && let Some(open) = link.open_map(&path)
    {
        return edit_in_editor(run, args, job, &path, map, &open);
    }
    if args.in_editor || args.mark.is_some() {
        anyhow::bail!(
            "{} is not open in the editor: bt open {} opens it",
            display_name(&path),
            map.display()
        );
    }
    edit_file(run, args, job, &path, map)
}

/// Runs the script in the editor that has the map open: one undo step, not saved. `typed` is
/// the map as the command named it, which the report's next commands use.
fn edit_in_editor(
    run: &Run,
    args: &EditArgs,
    job: &EditJob,
    path: &Path,
    typed: &Path,
    open: &OpenMap<'_>,
) -> anyhow::Result<EditOutcome> {
    let request = EditRequest {
        path: path.to_path_buf(),
        script: job.text.clone(),
        prelude: job.prelude.clone(),
        seed: args.seed,
        dry_run: args.dry_run,
        view: args.view,
        by: args.by.map(|by| view_word(by).to_owned()),
        runs: args.runs,
        want_map: args.render.is_some(),
        want_cells: args.mark.is_some(),
    };
    let edited = open.ask(&Request::Edit(request))?.into_edited()?;
    let mut outcome: EditOutcome = serde_json::from_value(edited.report)
        .context("the editor's report of the edit can't be read")?;
    if let Some(Saved::InEditor { map }) = &mut outcome.saved {
        *map = typed.display().to_string();
    }
    if let Some(label) = &args.mark {
        let cells = edited.cells.unwrap_or_default();
        let marked = if cells.is_empty() {
            format!("nothing to outline as “{label}”: the edit changes no cells")
        } else {
            let done = open
                .ask(&Request::Mark {
                    path: path.to_path_buf(),
                    marks: vec![MarkSpec {
                        runs: cells,
                        label: label.clone(),
                    }],
                    add: true,
                    focus: true,
                })?
                .into_done()?;
            format!("outlined in the editor as “{label}”: {}", done.message)
        };
        outcome = outcome.with_marked(marked);
    }
    if let (Some(out), Some(text)) = (&args.render, &edited.map) {
        // The edit is in the editor already: a picture that fails is said, not a failure.
        let drawn = loader_for(path, run).and_then(|mut loader| {
            let map = loader
                .parse_map(text, path)
                .with_context(|| format!("the editor's {} can't be read", typed.display()))?
                .value;
            draw_change(
                &map,
                Arc::clone(loader.custom_types()),
                out,
                &args.region,
                args.scale,
                &outcome.cells,
            )
        });
        outcome = match drawn {
            Ok(picture) => outcome.with_picture(picture),
            Err(error) => outcome.with_warning(format!("the picture can't be drawn: {error:#}")),
        };
    }
    Ok(outcome)
}

/// Runs the script on the map's file and writes the result, unless it is a dry run or changed
/// nothing; `typed` is the map as the command named it.
fn edit_file(
    run: &Run,
    args: &EditArgs,
    job: &EditJob,
    path: &Path,
    typed: &Path,
) -> anyhow::Result<EditOutcome> {
    let link = run.link();
    if !args.force {
        link.refuse_unanswered(path, run.folders.recovery.as_deref())
            .map_err(|error| {
                anyhow!(
                    "{error:#}; --force writes it anyway, and the editor then asks the person \
                     whether to reload"
                )
            })?;
    }
    let copy = if args.force && link.open_map(path).is_none() {
        MapCopy::File
    } else {
        MapCopy::Shown
    };
    let mut loaded = load_copy(path, run, copy)?;
    let mut input = script_input(&mut loaded, args.with.as_deref(), args.seed)?;
    if args.view {
        input = input.view(args.by.map(view_by), args.runs);
    }
    let result = run_script(&job.script, &input.build(), &mut loaded.loader)
        .map_err(|error| anyhow!(error))?;
    let mut outcome = result.outcome.clone();
    for warning in &loaded.warnings {
        outcome = outcome.with_warning(format!("reading the map: {warning}"));
    }
    if let Some(layers) = &loaded.layers {
        for note in &layers.notes {
            outcome = outcome.with_warning(format!("AutoMap layers: {}", note.text));
        }
    }
    if let Some(out) = &args.render {
        let picture = draw_change(
            &result.shown,
            Arc::clone(loaded.loader.custom_types()),
            out,
            &args.region,
            args.scale,
            &outcome.cells,
        )?;
        outcome = outcome.with_picture(picture);
    }
    if args.dry_run {
        return Ok(outcome.with_saved(Saved::DryRun { in_editor: false }));
    }
    match &args.out {
        Some(out) => {
            let target = written_path(out)?;
            write_result(run, &loaded, &result, &target, out, outcome)
        }
        None if !outcome.changed => Ok(outcome.with_saved(Saved::Unchanged { in_editor: false })),
        None => write_result(run, &loaded, &result, path, typed, outcome),
    }
}

/// Writes the map an edit left to `target` (named `typed` on the command line): the previous
/// bytes kept for `bt undo` first, then the map, then what was painted under its AutoMap layers
/// in its project file. A step that fails takes back the ones before it.
fn write_result(
    run: &Run,
    loaded: &LoadedMap,
    result: &EditResult,
    target: &Path,
    typed: &Path,
    mut outcome: EditOutcome,
) -> anyhow::Result<EditOutcome> {
    let folders = &run.folders;
    let rendered = WriteRequest::for_map(&result.shown, target)
        .loader(&loaded.loader)
        .render()
        .with_context(|| format!("{} can't be written", typed.display()))?;
    for warning in &rendered.warnings {
        outcome = outcome.with_warning(format!("writing the map: {warning}"));
    }
    let bytes = rendered.text.into_bytes();
    let before = match std::fs::read(target) {
        Ok(before) => Some(before),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(anyhow!(error).context(format!("{} can't be read", typed.display())));
        }
    };
    let entry = record_json(target, folders)?;
    let history = History::new(folders.history()?);
    if let Some(before) = &before {
        history.keep_edit(
            target,
            EditToKeep {
                before,
                written: &bytes,
                label: &outcome.label,
                entry: entry.as_deref(),
            },
        )?;
    }
    let undo_keeping = |error: anyhow::Error| -> anyhow::Error {
        if before.is_none() {
            return error;
        }
        match history.forget_last_edit(target) {
            Ok(_) => error,
            Err(forget) => error.context(format!(
                "and the version kept for bt undo could not be forgotten: {forget}"
            )),
        }
    };
    if let Err(error) = write_file(target, &bytes, WriteMode::Atomic) {
        return Err(undo_keeping(anyhow!(error)));
    }
    let painted_in = match write_record(loaded, result, target, entry.is_some(), run) {
        Ok(painted_in) => painted_in,
        Err(error) => {
            let put_back = match &before {
                Some(before) => write_file(target, before, WriteMode::Atomic),
                None => std::fs::remove_file(target).map_err(|source| bt_utils::Error::Write {
                    path: target.to_path_buf(),
                    source,
                }),
            };
            let error = match put_back {
                Ok(()) => error,
                Err(put_back) => error.context(format!(
                    "and {} could not be put back as it was: {put_back}",
                    typed.display()
                )),
            };
            return Err(undo_keeping(error));
        }
    };
    let saved = Saved::File {
        file: typed.display().to_string(),
        painted_in: painted_in.map(|file| display_name(&file)),
        undo: before.map(|_| format!("bt undo {}", typed.display())),
    };
    Ok(outcome.with_saved(saved))
}

/// Writes what was painted under the map's AutoMap layers after the edit into the project file
/// of `target`, or removes `target`'s record (`had_record`) when the map has none now; the
/// project file written for the layers.
fn write_record(
    loaded: &LoadedMap,
    result: &EditResult,
    target: &Path,
    had_record: bool,
    run: &Run,
) -> anyhow::Result<Option<PathBuf>> {
    match (&result.setup, &result.layers) {
        (Some(setup), Some(layers)) => write_layers_record(
            target,
            setup,
            layers,
            loaded.project_file.as_deref(),
            &run.folders,
        )
        .map(Some),
        _ if had_record => put_back_record(target, None, &run.folders).map(|()| None),
        _ => Ok(None),
    }
}

/// The file `--out` names, as `bt undo` will find it again: its canonical path when it is
/// there, else its absolute path.
fn written_path(out: &Path) -> anyhow::Result<PathBuf> {
    match std::fs::canonicalize(out) {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::path::absolute(out).with_context(|| format!("{} can't be written", out.display()))
        }
        Err(error) => Err(anyhow!(error).context(format!("{} can't be written", out.display()))),
    }
}

/// The outcomes as `--json` asks: one map's outcome, or an array of several.
fn printed_text(outcomes: &[EditOutcome], json: bool) -> anyhow::Result<String> {
    if !json {
        let texts: Vec<String> = outcomes.iter().map(|o| o.report().text()).collect();
        return Ok(texts.join("\n"));
    }
    let objects = outcomes
        .iter()
        .map(to_json_pretty)
        .collect::<Result<Vec<String>, _>>()?;
    let mut text = match objects.as_slice() {
        [one] => one.clone(),
        several => format!("[\n{}\n]", several.join(",\n")),
    };
    text.push('\n');
    Ok(text)
}

/// The view's word for what a character stands for, as the live link carries it.
fn view_word(by: ViewBy) -> &'static str {
    match by {
        ViewBy::Terrain => "terrain",
        ViewBy::Tile => "tile",
        ViewBy::Layer => "layer",
    }
}

/// What a character of the changed area's view stands for.
fn view_by(by: ViewBy) -> bt_script::ViewBy {
    match by {
        ViewBy::Terrain => bt_script::ViewBy::Terrain,
        ViewBy::Tile => bt_script::ViewBy::Tile,
        ViewBy::Layer => bt_script::ViewBy::Layer,
    }
}

#[cfg(test)]
#[path = "edit_tests.rs"]
mod tests;
