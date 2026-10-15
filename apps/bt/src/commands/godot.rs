//! Godot commands share the exporter's planning, refusal and verification rules.
use std::path::Path;

use anyhow::{Context, bail};
use bt_godot::{Error, Options, Report, Summary};
use serde::Serialize;
use serde_json::json;

use crate::cli::{GodotCommand, GodotExportArgs, GodotSetupArgs, GodotStatusArgs};
use crate::exit::{Printed, Status};
use crate::load::{MapCopy, load_copy, loader_for};
use crate::run::Run;

pub(crate) fn run(run: &Run, command: &GodotCommand) -> anyhow::Result<Printed> {
    match command {
        GodotCommand::Setup(args) => setup(run, args),
        GodotCommand::Export(args) => export(run, args),
        GodotCommand::Status(args) => status(run, args),
        GodotCommand::Verify { scene } => {
            let godot = bt_godot::verify::executable().context(bt_godot::verify::NO_GODOT)?;
            let verdict = bt_godot::verify::verify(&godot, scene).map_err(anyhow::Error::msg)?;
            output(
                run,
                "bt godot verify 1",
                &verdict,
                verdict.text(),
                if verdict.clean() {
                    Status::Done
                } else {
                    Status::Failed
                },
            )
        }
    }
}

fn setup(run: &Run, args: &GodotSetupArgs) -> anyhow::Result<Printed> {
    let loaded = load_copy(&args.map, run, MapCopy::File)?;
    let project = args
        .godot_project
        .clone()
        .or_else(|| bt_godot::setup::find_project(&loaded.path))
        .context("name a Godot project with --godot-project DIR (it must contain project.godot)")?;
    let project = std::fs::canonicalize(project)?;
    let file = bt_project::ProjectFile::location(
        &loaded.path,
        loaded.project_file.as_deref(),
        run.folders.home.as_deref(),
    )?;
    let settings = bt_project::GodotSetup {
        project: Some(bt_project::paths::key(&file, &project)?),
        levels: Some(args.levels.clone()),
        images: Some(args.images.clone()),
        tile_set_folder: Some(args.tilesets.clone()),
        tile_sets: Some("shared".into()),
        level_scene: Some(args.level_scene),
        export_on_save: Some(args.export_on_save),
        ..bt_project::GodotSetup::default()
    };
    bt_godot::setup::write(&file, settings.clone(), &run.folders.locks()?)
        .map_err(anyhow::Error::msg)?;
    output(
        run,
        "bt godot setup 1",
        &json!({"file":file,"godot":settings}),
        format!("Godot setup saved in {}\n", file.display()),
        Status::Done,
    )
}

fn export(run: &Run, args: &GodotExportArgs) -> anyhow::Result<Printed> {
    let options = Options {
        replace: args.replace,
        check: args.check,
        new_uid: args.new_uid,
    };
    if !args.all {
        return export_one(
            run,
            args.map.as_deref().context("name a map or --all")?,
            args.out.as_deref(),
            &options,
            args.remember,
        );
    }
    let project = match &args.godot_project {
        Some(p) => p.clone(),
        None => std::env::current_dir()?,
    };
    let mut loader = |p: &Path| loader_for(p, run).map_err(|e| format!("{e:#}"));
    match bt_godot::export_all(&project, &mut loader, &options) {
        Err(error) => failure(run, error),
        Ok(all) => {
            let mut text = String::new();
            let mut status = Status::Done;
            let mut scenes = Vec::new();
            for scene in all.scenes {
                match scene.outcome {
                    Ok(report) => {
                        let current = report_status(&report);
                        if current != Status::Done {
                            status = current;
                        }
                        text.push_str(&report_text(&report));
                        scenes.push(json!({"scene":scene.scene,"map":scene.map,"report":report}));
                    }
                    Err(error) => {
                        let printed = failure(run, error)?;
                        status = printed.status;
                        text.push_str(&printed.text);
                        scenes.push(json!({"scene":scene.scene,"error":printed.text}));
                    }
                }
            }
            output(
                run,
                "bt godot export all 1",
                &json!({"scenes":scenes,"written":all.written,"copies":all.copies}),
                text,
                status,
            )
        }
    }
}

pub(crate) fn export_one(
    run: &Run,
    map: &Path,
    out: Option<&Path>,
    options: &Options,
    remember: bool,
) -> anyhow::Result<Printed> {
    let loaded = load_copy(map, run, MapCopy::File)?;
    let target = out
        .map(Path::to_path_buf)
        .or(bt_godot::scene_of(&loaded.path, &loaded.map).map_err(anyhow::Error::msg)?)
        .context("this map has no Godot target: use bt godot setup, or name OUT.tscn")?;
    let result = bt_godot::export_map(
        &loaded.map,
        &loaded.path,
        loaded.loader.custom_types(),
        &target,
        options,
    );
    let mut report = match result {
        Ok(r) => r,
        Err(e) => return failure(run, e),
    };
    report.notes.splice(
        0..0,
        loaded.warnings.iter().map(|w| {
            bt_godot::Note::warning(bt_godot::Code::MapLoading, w.to_string(), Vec::new())
        }),
    );
    if run
        .link()
        .open_map(&loaded.path)
        .is_some_and(|o| o.document.dirty)
    {
        report.notes.push(bt_godot::Note::warning(bt_godot::Code::MapLoading,"Exported the saved file; the editor has unsaved changes. Save it first to include them.".into(),Vec::new()));
    }
    if !options.check {
        if bt_godot::setup::Setup::of(&loaded.path)
            .map_err(anyhow::Error::msg)?
            .is_some_and(|s| s.level_scene())
        {
            bt_godot::starter::write(&target).map_err(anyhow::Error::msg)?;
        }
        if remember {
            let target = std::path::absolute(&target)?;
            if let Some(open) = run.link().open_map(&loaded.path) {
                let _ = open.ask(&bt_live::Request::RememberExport {
                    path: loaded.path.clone(),
                    target,
                })?;
            } else {
                run.link()
                    .refuse_unanswered(&loaded.path, run.folders.recovery.as_deref())?;
                let mut map = loaded.map.clone();
                map.file.export.format = "tscn".into();
                map.file.export.target = Some(bt_core::FileRef::new(
                    bt_utils::paths::reference_path(
                        loaded.path.parent().context("map has no parent folder")?,
                        &target,
                    ),
                    target,
                ));
                bt_tmx::WriteRequest::for_map(&map, &loaded.path)
                    .loader(&loaded.loader)
                    .write()?;
            }
        }
    }
    let status = report_status(&report);
    output(
        run,
        "bt godot export 1",
        &report,
        report_text(&report),
        status,
    )
}

fn report_status(report: &Report) -> Status {
    if report.checked && !report.up_to_date() {
        Status::OutOfDate
    } else {
        Status::Done
    }
}
fn report_text(report: &Report) -> String {
    let mut text = String::new();
    for file in &report.files {
        text.push_str(&format!("{:?}: {}\n", file.state, file.path.display()));
    }
    text.push_str(&format!(
        "{} layers, {} cells, {} tiles, {} objects\n",
        report.counts.layers, report.counts.cells, report.counts.tiles, report.counts.objects
    ));
    for note in &report.notes {
        text.push_str(&format!("{}: {}\n", note.code, note.message));
    }
    text
}
fn status(run: &Run, args: &GodotStatusArgs) -> anyhow::Result<Printed> {
    let mut targets = Vec::new();
    if let Some(path) = &args.map {
        let loaded = load_copy(path, run, MapCopy::File)?;
        let target = bt_godot::scene_of(&loaded.path, &loaded.map)
            .map_err(anyhow::Error::msg)?
            .context("this map has no Godot export target")?;
        targets.push((loaded.path, target));
    } else {
        let project = match &args.godot_project {
            Some(p) => p.clone(),
            None => std::env::current_dir()?,
        };
        if !project.join("project.godot").is_file() {
            bail!("{} has no project.godot", project.display());
        }
        for generated in bt_godot::generated_scenes(&project).generated {
            targets.push((generated.map.map_err(anyhow::Error::msg)?, generated.scene));
        }
    }
    let mut text = String::new();
    let mut results = Vec::new();
    let mut code = Status::Done;
    for (map, scene) in targets {
        let mut loader = loader_for(&map, run)?;
        let status = match bt_godot::status(&map, &mut loader, &scene) {
            Ok(s) => s,
            Err(e) => return failure(run, e),
        };
        let summary = status.summary();
        match summary {
            Summary::UpToDate => {}
            Summary::Changed | Summary::Refused | Summary::Moved(_) => code = Status::Refused,
            _ => {
                if code == Status::Done {
                    code = Status::OutOfDate
                }
            }
        }
        text.push_str(&format!("{}: {summary:?}\n", map.display()));
        results.push(json!({"map":map,"scene":scene,"summary":summary,"status":status}));
    }
    output(run, "bt godot status 1", &results, text, code)
}
fn failure(run: &Run, error: Error) -> anyhow::Result<Printed> {
    let (status, data, text) = match error {
        Error::Failed(message) => (
            Status::Failed,
            json!({"error":message}),
            format!("{message}\n"),
        ),
        Error::Refused(refusals) => {
            let text = refusals
                .iter()
                .map(|r| format!("{}: {}\n{}\n", r.path.display(), r.reason, r.way_out()))
                .collect::<String>();
            (Status::Refused, json!({"refusals":refusals}), text)
        }
    };
    output(run, "bt godot error 1", &data, text, status)
}
fn output(
    run: &Run,
    schema: &str,
    value: &impl Serialize,
    text: String,
    status: Status,
) -> anyhow::Result<Printed> {
    let text = if run.cli.json {
        serde_json::to_string_pretty(&json!({"schema":schema,"result":value}))? + "\n"
    } else {
        text
    };
    Ok(Printed {
        text,
        status,
        failure: None,
    })
}
