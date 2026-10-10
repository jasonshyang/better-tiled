//! A command's map (`10-agents.md`, §4.3): the file, read through a loader built with the
//! project's custom types (`--project FILE`, else the nearest `.tiled-project` above the map),
//! with its reading warnings, and on a map with AutoMap layers what was painted, rebuilt from
//! the project file.
#![expect(
    dead_code,
    reason = "the commands that read maps are wired as their library parts land"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, bail};
use bt_automap::layers::{Setup, read_from_project};
use bt_core::{CustomTypes, Map};
use bt_tmx::{Loader, Warning};

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
}

/// What was painted on a map with AutoMap layers, and the layers' setup.
pub(crate) struct PaintedLayers {
    /// The map as painted.
    pub(crate) painted: Map,
    /// The setup.
    pub(crate) setup: Setup,
    /// Why some of what was painted could not be put back.
    pub(crate) notes: Vec<String>,
}

/// Reads the map at `path` with the custom types of `project` (else the nearest project above
/// the map), and its AutoMap layers from the project file found upward from it, stopping below
/// `home`.
pub(crate) fn load_map(
    path: &Path,
    project: Option<&Path>,
    home: Option<&Path>,
) -> anyhow::Result<LoadedMap> {
    let canonical =
        std::fs::canonicalize(path).with_context(|| format!("{} can't be read", path.display()))?;
    let custom_types = custom_types(&canonical, project, home)?;
    let mut loader = Loader::builder().custom_types(custom_types).build();
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
        let loaded = load_map(&map, None, None).unwrap();
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
        let error = load_map(&map, None, None).err().unwrap();
        assert!(error.to_string().contains("several projects"), "{error}");
        let chosen = scratch.join("a.tiled-project");
        assert!(load_map(&map, Some(&chosen), None).is_ok());
    }
}
