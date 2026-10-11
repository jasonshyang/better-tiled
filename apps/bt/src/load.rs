//! A command's map (`10-agents.md`, §4.3): the file, read through a loader built with the
//! project's custom types (`--project FILE`, else the nearest `.tiled-project` above the map),
//! with its reading warnings, and on a map with AutoMap layers what was painted, rebuilt from
//! the project file. A map open in an editor with unsaved changes is read from the editor
//! instead, with its unsaved tilesets; a file an editor that does not answer may hold unsaved
//! changes to is not read.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, bail};
use bt_automap::layers::{Setup, read_from_project};
use bt_core::Map;
use bt_live::Request;
use bt_tmx::{Loader, Warning};

use crate::link::OpenMap;
use crate::run::Run;

/// The extension of Tiled's project files.
const PROJECT_EXTENSION: &str = "tiled-project";

/// What every command's map comes with.
pub(crate) struct LoadedMap {
    /// The map's canonical path.
    pub(crate) path: PathBuf,
    /// The map as its file holds it: with AutoMap layers, what the rules make.
    pub(crate) map: Map,
    /// With AutoMap layers, what was painted and the layers' setup.
    pub(crate) layers: Option<PaintedLayers>,
    /// What reading the map warned about.
    pub(crate) warnings: Vec<Warning>,
    /// The loader the map was read with: maps and rules read later share its tilesets.
    pub(crate) loader: Loader,
    /// Where it was read from.
    pub(crate) source: MapSource,
    /// The project that applies to it, if any.
    pub(crate) project: Option<Arc<bt_core::Project>>,
    /// That project's `.tiled-project` file: a map's project-file entry goes next to it when
    /// no `better-tiled.json` is found above the map.
    pub(crate) project_file: Option<PathBuf>,
}

/// The project that applies to a map, and its file.
struct FoundProject {
    /// The `.tiled-project` file.
    file: PathBuf,
    /// The project it holds.
    project: Arc<bt_core::Project>,
}

/// Where a command's map was read from.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum MapSource {
    /// Its file.
    File,
    /// The editor with this process id, which has unsaved changes to it.
    Editor {
        /// The editor's process id.
        pid: u32,
    },
}

/// What was painted on a map with AutoMap layers, and the layers' setup.
pub(crate) struct PaintedLayers {
    /// The map as painted.
    pub(crate) painted: Map,
    /// The setup.
    pub(crate) setup: Setup,
    /// Why some of what was painted could not be put back.
    pub(crate) notes: Vec<bt_automap::Problem>,
}

/// Which copy of a map a command reads.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum MapCopy {
    /// The editor's when it has unsaved changes to the map, else the file.
    Shown,
    /// The file, whatever an editor holds.
    File,
    /// The editor's, which must have the map open.
    Editor,
}

/// Reads the map at `path` as the module's doc says, with the custom types of `--project` (else
/// the nearest project above the map), and its AutoMap layers from the project file found
/// upward from it, stopping below the home folder.
pub(crate) fn load_map(path: &Path, run: &Run) -> anyhow::Result<LoadedMap> {
    load_copy(path, run, MapCopy::Shown)
}

/// Reads the copy `copy` of the map at `path`.
pub(crate) fn load_copy(path: &Path, run: &Run, copy: MapCopy) -> anyhow::Result<LoadedMap> {
    let canonical =
        std::fs::canonicalize(path).with_context(|| format!("{} can't be read", path.display()))?;
    let home = run.folders.home.as_deref();
    let found = project_of(&canonical, run.cli.project.as_deref(), home)?;
    let custom_types = Arc::new(
        found
            .as_ref()
            .map(|p| p.project.custom_types.clone())
            .unwrap_or_default(),
    );
    let project_file = found.as_ref().map(|p| p.file.clone());
    let project = found.map(|p| p.project);
    let link = run.link();
    let mut loader = Loader::builder().custom_types(custom_types).build();
    let open = link.open_map(&canonical);
    match (copy, open) {
        (MapCopy::Editor, Some(open)) => {
            return read_from_editor(&canonical, &open, loader, project, project_file);
        }
        (MapCopy::Editor, None) => bail!(
            "{} is not open in the editor",
            bt_utils::paths::display_name(&canonical)
        ),
        (MapCopy::Shown, Some(open)) if open.document.dirty => {
            return read_from_editor(&canonical, &open, loader, project, project_file);
        }
        (MapCopy::Shown, _) => {
            link.refuse_unanswered(&canonical, run.folders.recovery.as_deref())?;
        }
        (MapCopy::File, _) => {}
    }
    let loaded = loader
        .load_map(&canonical)
        .with_context(|| format!("{} can't be read", path.display()))?;
    let layers = read_from_project(&canonical, home, &loaded.value)
        .with_context(|| format!("the project file of {} can't be read", path.display()))?
        .map(|restored| PaintedLayers {
            painted: restored.painted,
            setup: restored.setup,
            notes: restored.notes,
        });
    Ok(LoadedMap {
        path: canonical,
        map: loaded.value,
        layers,
        warnings: loaded.warnings,
        loader,
        source: MapSource::File,
        project,
        project_file,
    })
}

/// A loader for maps of the folder of `path` (an existing map), with the custom types of the
/// project that applies to it: for maps `bt` gets as text, from the editor.
pub(crate) fn loader_for(path: &Path, run: &Run) -> anyhow::Result<Loader> {
    let found = project_of(
        path,
        run.cli.project.as_deref(),
        run.folders.home.as_deref(),
    )?;
    let custom_types = Arc::new(
        found
            .map(|p| p.project.custom_types.clone())
            .unwrap_or_default(),
    );
    Ok(Loader::builder().custom_types(custom_types).build())
}

/// The map as the editor that has it open shows it, with its unsaved tilesets.
fn read_from_editor(
    path: &Path,
    open: &OpenMap<'_>,
    mut loader: Loader,
    project: Option<Arc<bt_core::Project>>,
    project_file: Option<PathBuf>,
) -> anyhow::Result<LoadedMap> {
    let text = open
        .ask(&Request::Snapshot {
            path: path.to_path_buf(),
        })?
        .into_map_text()?;
    for tileset in &text.tilesets {
        let parsed = loader
            .parse_tileset(&tileset.tsx, &tileset.path)
            .with_context(|| format!("the editor's {} can't be read", tileset.path.display()))?;
        loader.insert_tileset(&tileset.path, parsed.value)?;
    }
    let loaded = loader
        .parse_map(&text.map, path)
        .with_context(|| format!("the editor's {} can't be read", path.display()))?;
    Ok(LoadedMap {
        path: path.to_path_buf(),
        map: loaded.value,
        layers: None,
        warnings: loaded.warnings,
        loader,
        source: MapSource::Editor {
            pid: open.editor.pid(),
        },
        project,
        project_file,
    })
}

/// The project that applies to the map at `map`: `project`, else the nearest `.tiled-project`
/// above it (stopping at the repository's top and below `home`), else none.
fn project_of(
    map: &Path,
    project: Option<&Path>,
    home: Option<&Path>,
) -> anyhow::Result<Option<FoundProject>> {
    let file = match project {
        Some(file) => Some(file.to_path_buf()),
        None => nearest_project(map, home)?,
    };
    let Some(file) = file else {
        return Ok(None);
    };
    let loaded = bt_tmx::load_project(&file)
        .with_context(|| format!("the project {} can't be read", file.display()))?;
    Ok(Some(FoundProject {
        file,
        project: Arc::new(loaded.value),
    }))
}

/// The nearest `.tiled-project` above `map`. Two in one folder are refused: which one applies
/// would be a guess, and `--project` says.
fn nearest_project(map: &Path, home: Option<&Path>) -> anyhow::Result<Option<PathBuf>> {
    let start = map.parent().unwrap_or(map);
    let mut refused: Option<anyhow::Error> = None;
    let found = bt_utils::paths::search_up(start, home, |dir| {
        let mut projects: Vec<PathBuf> = std::fs::read_dir(dir)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == PROJECT_EXTENSION))
            .collect();
        projects.sort();
        match projects.as_slice() {
            [] => None,
            [one] => Some(one.clone()),
            several => {
                let names: Vec<String> = several
                    .iter()
                    .map(|p| bt_utils::paths::display_name(p))
                    .collect();
                refused = Some(anyhow::anyhow!(
                    "{} holds several projects ({}): say which with --project FILE",
                    dir.display(),
                    names.join(", ")
                ));
                Some(PathBuf::new())
            }
        }
    });
    if let Some(error) = refused {
        bail!(error);
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_test_data::Scratch;
    use bt_test_data::fixtures::MapBuilder;
    use bt_tmx::WriteRequest;
    use glam::UVec2;

    fn write_map(folder: &Path) -> PathBuf {
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(3, 2))
            .build()
            .unwrap();
        let path = folder.join("a.tmx");
        WriteRequest::for_map(&map, &path).write().unwrap();
        path
    }

    #[test]
    fn a_map_is_read_with_the_nearest_projects_types() {
        let scratch = Scratch::new("bt-load-project").unwrap();
        std::fs::create_dir_all(scratch.join("maps")).unwrap();
        let map = write_map(&scratch.join("maps"));
        std::fs::write(
            scratch.join("game.tiled-project"),
            r#"{"propertyTypes": [{"id": 1, "name": "Door", "type": "class", "members": [], "useAs": ["object"]}]}"#,
        )
        .unwrap();
        let loaded = load_map(&map, &Run::for_test(None)).unwrap();
        assert_eq!(loaded.loader.custom_types().types.len(), 1);
        assert_eq!(loaded.path, std::fs::canonicalize(&map).unwrap());
        assert!(loaded.layers.is_none());
    }

    #[test]
    fn two_projects_in_one_folder_are_refused_and_project_chooses() {
        let scratch = Scratch::new("bt-load-two-projects").unwrap();
        let map = write_map(scratch.path());
        for name in ["a", "b"] {
            std::fs::write(
                scratch.join(format!("{name}.tiled-project")),
                r#"{"propertyTypes": []}"#,
            )
            .unwrap();
        }
        let error = load_map(&map, &Run::for_test(None)).err().unwrap();
        assert!(error.to_string().contains("several projects"), "{error}");
        let chosen = scratch.join("a.tiled-project");
        assert!(load_map(&map, &Run::for_test(Some(&chosen))).is_ok());
    }
}
