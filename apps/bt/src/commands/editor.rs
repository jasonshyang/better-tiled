//! The live link's commands (`10-agents.md`, §13; `11-live-link.md`, §5, §10, §11): what the
//! editor is doing (`bt editor`), outlines on its canvas (`bt mark`), and opening and saving maps
//! there (`bt open`, `bt save`).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context as _, bail};
use bt_live::{
    DocStatus, KeyboardFocus, MarkSpec, Request, Status, cells_from_runs, runs_from_cells,
};
use bt_script::{
    Context, Line, Outcome, Report, Section, area_cells, capped_list, cell_text, parse_area,
    region_text,
};
use bt_utils::IRect;
use bt_utils::paths::display_name;
use serde::{Deserialize, Serialize};

use crate::cli::{EditorArgs, MapArg, MarkArgs};
use crate::exit::{Printed, Status as ExitStatus};
use crate::link::{Link, OpenMap};
use crate::output::printed;
use crate::run::Run;

/// How long `bt open` waits for an editor it started to answer.
const START_WAIT: Duration = Duration::from_secs(20);

/// What `bt editor` reports.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct EditorOutcome {
    /// The editors that answered, the focused one first.
    pub(crate) editors: Vec<Status>,
    /// The map asked about, when one was.
    pub(crate) map: Option<PathBuf>,
    /// Why some editors did not answer.
    pub(crate) problems: Vec<String>,
    /// The window's picture asked for: where it went, or why there is none.
    pub(crate) screenshot: Option<Screenshot>,
}

/// The editor's picture `bt editor --screenshot` asked for.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub(crate) enum Screenshot {
    /// Written there.
    Written {
        /// The PNG.
        path: PathBuf,
    },
    /// Not written, and why.
    Failed {
        /// Why.
        reason: String,
    },
}

/// What the editor did for `bt mark`, `bt open` or `bt save`, in its words.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct DoneOutcome {
    /// The map.
    pub(crate) map: PathBuf,
    /// What was done.
    pub(crate) message: String,
}

impl Outcome for DoneOutcome {
    const SCHEMA: &'static str = "bt.done/1";

    fn report(&self) -> Report {
        let mut report = Report::new();
        report.add_line(Line::fact(self.message.clone()));
        report
    }
}

impl Outcome for EditorOutcome {
    const SCHEMA: &'static str = "bt.editor/1";

    fn report(&self) -> Report {
        let mut report = Report::new();
        for problem in &self.problems {
            report.add_line(Line::warning(problem.clone()));
        }
        if self.editors.is_empty() {
            report.add_line(Line::fact(
                "no editor is running (bt open MAP starts one with the map)",
            ));
        }
        for status in &self.editors {
            let mut section = Section::headed(format!(
                "editor {}{}",
                status.pid,
                if status.focused { ", in front" } else { "" }
            ));
            let documents: Vec<&DocStatus> = status
                .documents
                .iter()
                .filter(|doc| {
                    self.map
                        .as_deref()
                        .is_none_or(|map| doc.path.as_deref() == Some(map))
                })
                .collect();
            if documents.is_empty() {
                section.add_line(Line::fact(match &self.map {
                    Some(map) => format!("{} is not open there", display_name(map)),
                    None => "no map is open".to_owned(),
                }));
            }
            for doc in documents {
                section.add_line(document_line(doc));
            }
            report.add_section(section);
        }
        match &self.screenshot {
            Some(Screenshot::Written { path }) => report.add_line(Line::fact(format!(
                "the editor's window as the person sees it: {}",
                path.display()
            ))),
            Some(Screenshot::Failed { reason }) => {
                report.add_line(Line::note(format!("no picture of the window: {reason}")));
            }
            None => {}
        }
        report
    }
}

/// A document as `bt editor` writes it, with what the person is doing in it.
fn document_line(doc: &DocStatus) -> Line {
    let mut states = Vec::new();
    if doc.active {
        states.push("shown".to_owned());
    }
    if doc.dirty {
        states.push("unsaved changes".to_owned());
    }
    if doc.tileset {
        states.push("a tileset".to_owned());
    }
    let title = match &doc.path {
        Some(path) => display_name(path),
        None => doc.title.clone(),
    };
    let heading = if states.is_empty() {
        title
    } else {
        format!("{title} ({})", states.join(", "))
    };
    let mut details = Vec::new();
    let mut tool = format!("tool {}", doc.tool);
    if let Some(brush) = &doc.brush {
        tool.push_str(&format!(" with {brush}"));
    }
    details.push(Line::fact(match &doc.layer {
        Some(layer) => format!("layer {layer}; {tool}"),
        None => tool,
    }));
    let selected = cells_from_runs(&doc.selection);
    if let Some(bounds) = IRect::bounding(selected.iter().copied()) {
        details.push(Line::fact(format!(
            "{} {} selected in {}",
            selected.len(),
            if selected.len() == 1 { "cell" } else { "cells" },
            region_text(bounds)
        )));
    }
    if !doc.objects.is_empty() {
        let ids: Vec<String> = doc.objects.iter().map(|id| format!("#{id}")).collect();
        details.push(Line::fact(format!(
            "objects selected: {}",
            capped_list(&ids, 12)
        )));
    }
    let mut view = Vec::new();
    if let Some(cells) = doc.view {
        view.push(format!("cells in view: {}", region_text(cells)));
    }
    if let Some(mouse) = doc.mouse {
        view.push(format!("the mouse is over {}", cell_text(mouse)));
    }
    if !view.is_empty() {
        details.push(Line::fact(view.join("; ")));
    }
    if doc.undo_steps > 0 {
        details.push(Line::fact(format!(
            "{} {} to undo, the last: {}",
            doc.undo_steps,
            if doc.undo_steps == 1 { "step" } else { "steps" },
            doc.history.join(", ")
        )));
    }
    if !doc.marks.is_empty() {
        let marks: Vec<String> = doc
            .marks
            .iter()
            .enumerate()
            .map(|(index, mark)| {
                let cells = cells_from_runs(&mark.runs);
                let bounds = IRect::bounding(cells.iter().copied())
                    .map_or_else(String::new, |b| format!(" in {}", region_text(b)));
                format!(
                    "{}. {} ({} cells{bounds})",
                    index + 1,
                    mark.label,
                    cells.len()
                )
            })
            .collect();
        details.push(Line::fact(format!("marks: {}", marks.join(", "))));
    }
    if doc.world_tool {
        details.push(Line::fact("the person is arranging a world's maps"));
    }
    if let Some(keyboard) = doc.keyboard {
        details.push(Line::fact(format!(
            "keys go to {}",
            match keyboard {
                KeyboardFocus::Map => "the map",
                KeyboardFocus::TextField => "a text field: the person is typing",
                KeyboardFocus::TilesetCanvas => "the tileset editor",
                KeyboardFocus::Dialog => "a dialog",
            }
        )));
    }
    Line::fact(heading).with_details(details)
}

/// Runs `bt editor`.
pub(crate) fn editor(run: &Run, args: &EditorArgs) -> anyhow::Result<Printed> {
    let link = run.link();
    let map = args.map.as_deref().map(bt_live::canonical_path);
    let screenshot = args
        .screenshot
        .as_ref()
        .map(|out| take_screenshot(link, out));
    let outcome = EditorOutcome {
        editors: link.editors.iter().map(|e| e.status().clone()).collect(),
        map,
        problems: link.problems.clone(),
        screenshot,
    };
    printed(&outcome, run.cli.json, ExitStatus::Done)
}

/// Asks the editor in front for its window's picture.
fn take_screenshot(link: &Link, out: &Path) -> Screenshot {
    let Some(editor) = link.editors.first() else {
        return Screenshot::Failed {
            reason: "no editor is running".to_owned(),
        };
    };
    let out = std::path::absolute(out).unwrap_or_else(|_| out.to_path_buf());
    match editor
        .ask(&Request::Screenshot { out: out.clone() })
        .map_err(anyhow::Error::from)
        .and_then(|response| Ok(response.into_done()?))
    {
        Ok(_) => Screenshot::Written { path: out },
        Err(error) => Screenshot::Failed {
            reason: format!("{error:#}"),
        },
    }
}

/// The map at `path` open in an editor, or why bt cannot reach it there.
fn open_map<'l>(link: &'l Link, path: &Path) -> anyhow::Result<OpenMap<'l>> {
    link.open_map(path).with_context(|| {
        format!(
            "{} is not open in the editor: bt open MAP opens it",
            display_name(path)
        )
    })
}

/// Runs `bt mark`.
pub(crate) fn mark(run: &Run, args: &MarkArgs) -> anyhow::Result<Printed> {
    let path = bt_live::canonical_path(&args.map);
    let link = run.link();
    let open = open_map(link, &path)?;
    let request = if args.clear {
        Request::Mark {
            path: path.clone(),
            marks: Vec::new(),
            add: false,
            focus: false,
        }
    } else {
        if args.shape.is_empty() {
            bail!("bt mark needs a shape, or --clear");
        }
        let written = &args.shape.join(" ");
        let mut loaded = crate::load::load_map(&path, run)?;
        let names = crate::script_input::shape_names(&mut loaded, args.with.as_deref(), None)?;
        let context =
            Context::new(crate::commands::know::DEFAULT_SEED).with_selection(open.selection());
        let area = parse_area(written)
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("“{written}” is not a shape"))?;
        let cells = area_cells(&loaded.map, &names, &context, &area)?;
        if cells.is_empty() {
            bail!("“{written}” holds no cells of {}", display_name(&path));
        }
        Request::Mark {
            path: path.clone(),
            marks: vec![MarkSpec {
                runs: runs_from_cells(cells),
                label: args.label.clone().unwrap_or_else(|| written.clone()),
                color: None,
            }],
            add: args.add,
            focus: !args.no_focus,
        }
    };
    let done = open.ask(&request)?.into_done()?;
    let outcome = DoneOutcome {
        map: path,
        message: done.message,
    };
    printed(&outcome, run.cli.json, ExitStatus::Done)
}

/// Runs `bt save`.
pub(crate) fn save(run: &Run, args: &MapArg) -> anyhow::Result<Printed> {
    let path = bt_live::canonical_path(&args.map);
    let link = run.link();
    let open = link.open_map(&path).with_context(|| {
        format!(
            "{} is not open in the editor (bt save saves maps open there; bt edit writes files \
             itself)",
            display_name(&path)
        )
    })?;
    let done = open
        .ask(&Request::Save { path: path.clone() })?
        .into_done()?;
    let outcome = DoneOutcome {
        map: path,
        message: done.message,
    };
    printed(&outcome, run.cli.json, ExitStatus::Done)
}

/// Runs `bt open`: the map opened in the editor in front, or in an editor `bt` starts.
pub(crate) fn open(run: &Run, args: &MapArg) -> anyhow::Result<Printed> {
    let path = std::fs::canonicalize(&args.map)
        .with_context(|| format!("{} can't be read", args.map.display()))?;
    let link = run.link();
    let message = match link.editors.first() {
        Some(editor) => {
            editor
                .ask(&Request::Open { path: path.clone() })?
                .into_done()?
                .message
        }
        None => start_editor(&path)?,
    };
    let outcome = DoneOutcome { map: path, message };
    printed(&outcome, run.cli.json, ExitStatus::Done)
}

/// Starts the editor with the map, detached in its own process group, and waits for it to
/// answer with the map open.
fn start_editor(path: &Path) -> anyhow::Result<String> {
    let program = editor_program()?;
    let mut command = std::process::Command::new(&program);
    command
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
        .spawn()
        .with_context(|| format!("the editor {} did not start", program.display()))?;
    let started = Instant::now();
    while started.elapsed() < START_WAIT {
        std::thread::sleep(Duration::from_millis(250));
        if Link::find().open_map(path).is_some() {
            return Ok(format!("started the editor with {}", display_name(path)));
        }
    }
    bail!(
        "the editor started but did not answer within {} seconds: its preference \"Let bt work \
         with the open maps\" may be off",
        START_WAIT.as_secs()
    )
}

/// The editor's program: next to `bt`'s own, then `$BETTER_TILED`, then on the `PATH`, then
/// (on macOS) in `/Applications`.
fn editor_program() -> anyhow::Result<PathBuf> {
    let name = if cfg!(windows) {
        "better-tiled.exe"
    } else {
        "better-tiled"
    };
    let beside = std::env::current_exe()
        .ok()
        .and_then(|bt| bt.parent().map(|dir| dir.join(name)));
    let named = std::env::var_os("BETTER_TILED").map(PathBuf::from);
    let on_path: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join(name))
                .collect()
        })
        .unwrap_or_default();
    let application = cfg!(target_os = "macos")
        .then(|| PathBuf::from("/Applications/better-tiled.app/Contents/MacOS/better-tiled"));
    beside
        .into_iter()
        .chain(named)
        .chain(on_path)
        .chain(application)
        .find(|candidate| candidate.is_file())
        .with_context(|| {
            "the editor (better-tiled) is not next to bt, named by $BETTER_TILED, on the PATH or \
             in /Applications"
                .to_owned()
        })
}

#[cfg(test)]
mod tests {
    use bt_live::Run as CellRun;
    use glam::IVec2;

    use super::*;

    fn document() -> DocStatus {
        DocStatus {
            path: Some(PathBuf::from("/maps/a.tmx")),
            title: "a.tmx".into(),
            active: true,
            dirty: true,
            tileset: false,
            layer: Some("ground".into()),
            tool: "Terrain Brush".into(),
            brush: Some("water".into()),
            selection: vec![CellRun { y: 1, x0: 2, x1: 5 }],
            objects: vec![5, 6],
            view: Some(IRect::from_min_size(IVec2::ZERO, IVec2::new(40, 30))),
            mouse: Some(IVec2::new(12, 7)),
            history: vec!["Paint Terrain".into(), "Erase".into()],
            undo_steps: 14,
            marks: vec![MarkSpec {
                runs: vec![CellRun { y: 3, x0: 3, x1: 5 }],
                label: "bridge".into(),
                color: None,
            }],
            world_tool: false,
            keyboard: Some(KeyboardFocus::Map),
        }
    }

    #[test]
    fn the_editor_report_says_what_the_person_is_doing() {
        let outcome = EditorOutcome {
            editors: vec![Status {
                protocol: bt_live::PROTOCOL,
                pid: 4321,
                focused: true,
                documents: vec![document()],
            }],
            map: None,
            problems: vec!["the editor (process 9) didn't answer in time".into()],
            screenshot: Some(Screenshot::Failed {
                reason: "the window is hidden".into(),
            }),
        };
        assert_eq!(
            outcome.report().text(),
            "warning: the editor (process 9) didn't answer in time\n\
             editor 4321, in front\n\
             \x20 a.tmx (shown, unsaved changes)\n\
             \x20   layer ground; tool Terrain Brush with water\n\
             \x20   3 cells selected in 2,1 3x1\n\
             \x20   objects selected: #5, #6\n\
             \x20   cells in view: 0,0 40x30; the mouse is over 12,7\n\
             \x20   14 steps to undo, the last: Paint Terrain, Erase\n\
             \x20   marks: 1. bridge (2 cells in 3,3 2x1)\n\
             \x20   keys go to the map\n\
             note: no picture of the window: the window is hidden\n"
        );
        let json = bt_script::to_json(&outcome).unwrap();
        assert!(json.starts_with(r#"{"schema":"bt.editor/1""#));
    }

    #[test]
    fn with_no_editor_running_it_says_how_to_start_one() {
        let outcome = EditorOutcome {
            editors: Vec::new(),
            map: None,
            problems: Vec::new(),
            screenshot: None,
        };
        assert_eq!(
            outcome.report().text(),
            "no editor is running (bt open MAP starts one with the map)\n"
        );
    }
}
