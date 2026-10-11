//! `bt edit` in the editor (`11-live-link.md`, §9.2): the script runs on the connection's
//! thread, on the base the window hands out, through the same `bt-script` a file edit uses; its
//! result goes back to the window to land as one undo step while the map is still at the base's
//! version. When the person edited meanwhile, the script runs again on the new base, up to three
//! times. A dry run, or a script that changed nothing, lands nothing. A script that panics is
//! reported, never taking the window down.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_core::Map;
use bt_editor::{ExternalBase, ExternalEdit, ExternalEditError};
use bt_live::{EditRequest, Edited, Response, runs_from_cells};
use bt_script::{
    EditResult, RunInput, Saved, Script, Selection, ViewBy, parse_definitions, parse_script,
    run_script,
};
use bt_tmx::{Loader, WriteRequest};

/// How many times a script runs before the map is taken to keep changing under it.
const RUNS: usize = 3;

/// What a script needs from the window: the base of the map at a path, and a result landed.
pub(super) trait Window {
    /// What a script runs on, for the map at `path`.
    fn prepare(&self, path: &Path) -> Result<ExternalBase, String>;
    /// The result landed as one undo step, or why not.
    fn apply(&self, edit: ExternalEdit) -> Result<Result<(), ExternalEditError>, String>;
}

/// The script `request` asks for, run in the editor as the module's doc says.
pub(super) fn edit_in_editor(
    window: &impl Window,
    request: &EditRequest,
) -> Result<Response, String> {
    let path = bt_live::canonical_path(&request.path);
    let script = parse_script(&request.script).map_err(|error| error.to_string())?;
    let definitions = if request.prelude.trim().is_empty() {
        None
    } else {
        Some(parse_definitions(&request.prelude).map_err(|error| format!("--with file, {error}"))?)
    };
    let by = view_by(request.by.as_deref())?;
    let job = ScriptJob {
        path,
        script,
        definitions,
        by,
        request,
    };
    for _ in 0..RUNS {
        let base = window.prepare(&job.path)?;
        let result = job.run_on(&base)?;
        if request.dry_run || !result.outcome.changed {
            let saved = if request.dry_run {
                Saved::DryRun { in_editor: true }
            } else {
                Saved::Unchanged { in_editor: true }
            };
            return job.answer(result, saved, false);
        }
        let edit = ExternalEdit {
            doc: base.doc,
            base_version: base.version,
            painted: Map::clone(&result.painted),
            setup: result.setup.clone(),
            label: result.outcome.label.clone(),
        };
        match window.apply(edit)? {
            Ok(()) => {
                let saved = Saved::InEditor {
                    map: job.path.display().to_string(),
                };
                return job.answer(result, saved, true);
            }
            Err(ExternalEditError::Conflict) => {}
            Err(other) => return Err(other.to_string()),
        }
    }
    Err("the map kept changing in the editor: run it again".to_owned())
}

/// A script to run on the editor's base, parsed once for every run.
struct ScriptJob<'r> {
    /// The map's canonical path.
    path: PathBuf,
    /// The script.
    script: Script,
    /// The `--with` definitions.
    definitions: Option<Script>,
    /// What the changed area's view is drawn by, when one is asked for; none for the view's
    /// own choice.
    by: Option<ViewBy>,
    /// The request.
    request: &'r EditRequest,
}

impl ScriptJob<'_> {
    /// Runs the script on a copy of `base`.
    fn run_on(&self, base: &ExternalBase) -> Result<EditResult, String> {
        let mut loader = Loader::builder()
            .custom_types(Arc::clone(&base.custom_types))
            .build();
        for known in &base.tilesets {
            loader
                .insert_tileset(&known.path, Arc::clone(&known.tileset))
                .map_err(|error| error.to_string())?;
        }
        let mut input =
            RunInput::builder(Arc::clone(&base.painted), &self.path).selection(Selection {
                cells: base.selected_cells.iter().collect(),
                objects: base.selected_objects.clone(),
            });
        if let Some(seed) = self.request.seed {
            input = input.seed(seed);
        }
        if let Some(rules) = &base.rules {
            input = input.rules_read(&rules.file, Arc::clone(&rules.rules));
        }
        if let Some(setup) = &base.setup {
            let rules = match &base.rules {
                Some(read) if read.file == setup.rules => Arc::clone(&read.rules),
                _ => Arc::new(
                    bt_automap::files::load(&mut loader, &setup.rules, Some(&self.path)).map_err(
                        |error| format!("the map's AutoMap rules can't be read: {error}"),
                    )?,
                ),
            };
            input = input.automap_layers(setup.clone(), rules, Arc::clone(&base.shown));
        }
        if let Some(definitions) = &self.definitions {
            input = input.definitions(definitions.clone());
        }
        if self.request.view {
            input = input.view(self.by, self.request.runs);
        }
        let input = input.build();
        let ran = catch_unwind(AssertUnwindSafe(|| {
            run_script(&self.script, &input, &mut loader)
        }));
        match ran {
            Ok(result) => result.map_err(|error| error.to_string()),
            Err(_) => Err("the script crashed inside the editor".to_owned()),
        }
    }

    /// The answer for `bt`: the report with where the edit went, and what the request asked for
    /// besides.
    fn answer(&self, result: EditResult, saved: Saved, applied: bool) -> Result<Response, String> {
        let outcome = result.outcome.clone().with_saved(saved);
        let map = if self.request.want_map {
            let rendered = WriteRequest::for_map(&result.shown, &self.path)
                .render()
                .map_err(|error| format!("the edited map can't be written as text: {error}"))?;
            Some(rendered.text)
        } else {
            None
        };
        let cells = self
            .request
            .want_cells
            .then(|| runs_from_cells(outcome.cells.iter().copied()));
        let report = serde_json::to_value(&outcome)
            .map_err(|error| format!("the edit's report can't be written: {error}"))?;
        Ok(Response::Edited(Edited {
            report,
            area: outcome.area,
            applied,
            map,
            cells,
        }))
    }
}

/// What a character of the changed area's view stands for, from `bt`'s word; none for the
/// view's own choice.
fn view_by(word: Option<&str>) -> Result<Option<ViewBy>, String> {
    match word {
        None => Ok(None),
        Some("terrain") => Ok(Some(ViewBy::Terrain)),
        Some("tile") => Ok(Some(ViewBy::Tile)),
        Some("layer") => Ok(Some(ViewBy::Layer)),
        Some(other) => Err(format!(
            "the view is by terrain, tile or layer, not “{other}”"
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use bt_core::{FileRef, TilesetRef};
    use bt_test_data::Scratch;
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::UVec2;

    use super::*;
    use crate::test_window::TestWindow;

    /// A window that hands out one base and answers each apply as told, in turn.
    struct Scripted {
        base: ExternalBase,
        applies: RefCell<Vec<Result<(), ExternalEditError>>>,
        prepared: Cell<usize>,
    }

    impl Window for Scripted {
        fn prepare(&self, _path: &Path) -> Result<ExternalBase, String> {
            self.prepared.set(self.prepared.get() + 1);
            Ok(self.base.clone())
        }

        fn apply(&self, _edit: ExternalEdit) -> Result<Result<(), ExternalEditError>, String> {
            let mut applies = self.applies.borrow_mut();
            if applies.is_empty() {
                return Err("no more answers".to_owned());
            }
            Ok(applies.remove(0))
        }
    }

    /// A window as told, with the base of a 4×3 map open in a real window; the map's path, and
    /// the folder it is in, kept while the test runs.
    struct Setup {
        window: Scripted,
        path: PathBuf,
        _scratch: Scratch,
    }

    fn scripted(name: &str, applies: Vec<Result<(), ExternalEditError>>) -> Setup {
        let scratch = Scratch::new(name).unwrap();
        let atlas = Atlas::builder()
            .name("tiles")
            .build_in(scratch.path())
            .unwrap();
        let tileset_path = scratch.join("tiles.tsx");
        WriteRequest::for_tileset(&atlas.tileset, &tileset_path)
            .write()
            .unwrap();
        let mut map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(4, 3))
            .tileset(atlas.tileset.clone())
            .tile_layer("ground", "0 1 2 3\n0 1 2 3\n0 1 2 3")
            .build()
            .unwrap();
        map.tilesets[0] = TilesetRef {
            file: Some(FileRef::new("tiles.tsx", &tileset_path)),
            first_gid: Some(1),
            tileset: atlas.tileset.clone(),
        };
        let path = scratch.join("a.tmx");
        WriteRequest::for_map(&map, &path).write().unwrap();
        let path = std::fs::canonicalize(path).unwrap();
        let mut window = TestWindow::with_data(&scratch.join("data"));
        window.drop_file(&path);
        let editor = window.editor();
        let doc = editor.document_at(&path).unwrap();
        let base = editor.prepare_external_edit(doc).unwrap();
        Setup {
            window: Scripted {
                base,
                applies: RefCell::new(applies),
                prepared: Cell::new(0),
            },
            path,
            _scratch: scratch,
        }
    }

    #[test]
    fn a_conflict_runs_the_script_again_on_the_new_base() {
        let Setup {
            window,
            path,
            _scratch: _kept,
        } = scripted(
            "live-edit-conflict",
            vec![
                Err(ExternalEditError::Conflict),
                Err(ExternalEditError::Conflict),
                Ok(()),
            ],
        );
        let request = EditRequest::new(&path, "tile tiles:5 0,0 layer ground");
        let edited = edit_in_editor(&window, &request)
            .unwrap()
            .into_edited()
            .unwrap();
        assert!(edited.applied);
        assert_eq!(window.prepared.get(), 3, "a base for every run");
    }

    #[test]
    fn a_map_that_keeps_changing_stops_after_three_runs() {
        let Setup {
            window,
            path,
            _scratch: _kept,
        } = scripted(
            "live-edit-keeps-changing",
            vec![Err(ExternalEditError::Conflict); 3],
        );
        let request = EditRequest::new(&path, "tile tiles:5 0,0 layer ground");
        let error = edit_in_editor(&window, &request).err().unwrap();
        assert_eq!(error, "the map kept changing in the editor: run it again");
        assert_eq!(window.prepared.get(), 3);
    }

    #[test]
    fn a_dry_run_and_a_bad_view_land_nothing() {
        let Setup {
            window,
            path,
            _scratch: _kept,
        } = scripted("live-edit-dry-run", Vec::new());
        let mut request = EditRequest::new(&path, "tile tiles:5 0,0 layer ground");
        request.dry_run = true;
        let edited = edit_in_editor(&window, &request)
            .unwrap()
            .into_edited()
            .unwrap();
        assert!(!edited.applied);
        request.view = true;
        request.by = Some("colour".to_owned());
        let error = edit_in_editor(&window, &request).err().unwrap();
        assert!(error.contains("terrain, tile or layer"), "{error}");
    }
}
