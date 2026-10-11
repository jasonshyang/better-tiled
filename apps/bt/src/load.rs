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
use bt_core::{CustomTypes, Map};
use bt_live::Request;
use bt_tmx::{Loader, Warning};

use crate::link::OpenMap;
use crate::run::Run;

/// The extension of Tiled's project files.
const PROJECT_EXTENSION: &str = "tiled-project";

/// What every command's map comes with.
#[expect(
    dead_code,
    reason = "bt info and bt edit read the rest when they are wired"
)]
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
#[expect(dead_code, reason = "bt edit reads them when it is wired")]
pub(crate) struct PaintedLayers {
    /// The map as painted.
    pub(crate) painted: Map,
    /// The setup.
    pub(crate) setup: Setup,
    /// Why some of what was painted could not be put back.
    pub(crate) notes: Vec<String>,
}

/// Reads the map at `path` as the module's doc says, with the custom types of `--project` (else
/// the nearest project above the map), and its AutoMap layers from the project file found
/// upward from it, stopping below the home folder.
pub(crate) fn load_map(path: &Path, run: &Run) -> anyhow::Result<LoadedMap> {
    let canonical =
        std::fs::canonicalize(path).with_context(|| format!("{} can't be read", path.display()))?;
    let home = run.folders.home.as_deref();
    let custom_types = custom_types(&canonical, run.cli.project.as_deref(), home)?;
    let link = run.link();
    link.refuse_unanswered(&canonical, run.folders.recovery.as_deref())?;
    let mut loader = Loader::builder().custom_types(custom_types).build();
    if let Some(open) = link.open_map(&canonical).filter(|open| open.document.dirty) {
        return read_from_editor(&canonical, &open, loader);
    }
    let loaded = loader
        .load_map(&canonical)
        .with_context(|| format!("{} can't be read", path.display()))?;
    let layers = read_from_project(&canonical, home, &loaded.value)
        .with_context(|| format!("the project file of {} can't be read", path.display()))?
        .map(|restored| PaintedLayers {
            painted: restored.painted,
            setup: restored.setup,
            notes: restored
                .notes
                .iter()
                .map(|note| note.text.clone())
                .collect(),
        });
    Ok(LoadedMap {
        path: canonical,
        map: loaded.value,
        layers,
        warnings: loaded.warnings,
        loader,
        source: MapSource::File,
    })
}

/// The map as the editor that has it open shows it, with its unsaved tilesets.
fn read_from_editor(
    path: &Path,
    open: &OpenMap<'_>,
    mut loader: Loader,
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
    })
}

/// The custom types that apply to the map at `map`: those of `project`, else of the nearest
/// `.tiled-project` above it (stopping at the repository's top and below `home`), else none.
fn custom_types(
    map: &Path,
    project: Option<&Path>,
    home: Option<&Path>,
) -> anyhow::Result<Arc<CustomTypes>> {
    let file = match project {
        Some(file) => Some(file.to_path_buf()),
        None => nearest_project(map, home)?,
    };
    let Some(file) = file else {
        return Ok(Arc::new(CustomTypes::default()));
    };
    let loaded = bt_tmx::load_project(&file)
        .with_context(|| format!("the project {} can't be read", file.display()))?;
    Ok(Arc::new(loaded.value.custom_types))
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
