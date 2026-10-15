//! A map's Godot setup (docs/design/12-godot-export.md): the `godot` section of its better-tiled.json, made
//! once per project by the editor's setup sheet (or `bt godot setup`). It names the Godot project
//! the maps go into, the folder their scenes go in, where copies of images from outside the
//! project go, and whether a first export also writes a level scene.

use std::path::{Path, PathBuf};

use bt_project::{GodotSetup, ProjectFile};

/// Where scenes go when the setup doesn't say.
pub const LEVELS: &str = "res://levels/";
/// Where copies of images from outside the project go when the setup doesn't say.
pub const IMAGES: &str = "res://images/";
/// Where shared TileSets go when the setup doesn't say.
pub const TILE_SETS: &str = "res://tilesets/";

/// The setup a project file holds.
#[derive(Clone, Debug)]
pub struct Setup {
    /// The project file (better-tiled.json) it is in.
    pub file: PathBuf,
    /// The stored Godot export settings.
    pub godot: GodotSetup,
}

impl Setup {
    /// The setup covering the map at `map_path`: the `godot` section of its project file, when it
    /// names a Godot project.
    pub fn of(map_path: &Path) -> Result<Option<Setup>, String> {
        let Some(file) = ProjectFile::find(map_path, None).map_err(|e| e.to_string())? else {
            return Ok(None);
        };
        let data = ProjectFile::load(&file).map_err(|e| e.to_string())?;
        let Some(godot) = data.godot().cloned() else {
            return Ok(None);
        };
        validate(&godot, &file)?;
        Ok(godot.project.is_some().then_some(Setup { file, godot }))
    }

    /// The Godot project's folder.
    pub fn project(&self) -> Option<PathBuf> {
        self.godot
            .project
            .as_deref()
            .map(|p| bt_project::paths::resolve(&self.file, p))
    }

    /// The scene the map at `map_path` goes to: `<map stem>.tscn` in the levels folder.
    pub fn scene(&self, map_path: &Path) -> Option<PathBuf> {
        let levels = in_project(
            &self.project()?,
            self.godot.levels.as_deref().unwrap_or(LEVELS),
        )?;
        Some(levels.join(format!("{}.tscn", map_path.file_stem()?.to_string_lossy())))
    }

    /// Where copies of images from outside the project go.
    pub fn images(&self) -> Option<PathBuf> {
        in_project(
            &self.project()?,
            self.godot.images.as_deref().unwrap_or(IMAGES),
        )
    }

    /// Whether a map's first export also writes its starter level scene.
    pub fn level_scene(&self) -> bool {
        self.godot.level_scene.unwrap_or(false)
    }
}

/// The folder of the setup covering the map at `map_path` that copies of its images go to, when
/// the setup's project is `project` (the one the scene is in).
pub(super) fn images_for(map_path: &Path, project: &Path) -> Result<Option<PathBuf>, String> {
    let Some(setup) = Setup::of(map_path)? else {
        return Ok(None);
    };
    let same = |a: &Path, b: &Path| {
        a.canonicalize()
            .ok()
            .zip(b.canonicalize().ok())
            .is_some_and(|(a, b)| a == b)
    };
    Ok(setup
        .project()
        .filter(|p| same(p, project))
        .and_then(|_| setup.images()))
}

/// A `res://` folder inside the project at `project`; `None` for anything else.
pub fn in_project(project: &Path, res: &str) -> Option<PathBuf> {
    let relative = res.strip_prefix("res://")?;
    if relative.split('/').any(|c| c == "..") {
        return None;
    }
    Some(
        relative
            .split('/')
            .filter(|c| !c.is_empty())
            .fold(project.to_path_buf(), |dir, c| dir.join(c)),
    )
}

/// The Godot project at or above `path`'s folder: the folder holding `project.godot`.
pub fn find_project(path: &Path) -> Option<PathBuf> {
    super::project_root(&std::path::absolute(path).ok()?).ok()
}

/// The Godot version a project names in its project.godot (`config/features`), as (major,
/// minor): from 4.6 on its scenes get node `unique_id`s.
pub fn version(project: &Path) -> Option<(u32, u32)> {
    super::version::features(&std::fs::read_to_string(project.join("project.godot")).ok()?)
}

/// Whether a project of that version gets node `unique_id`s.
pub fn writes_unique_ids(version: (u32, u32)) -> bool {
    version >= super::version::UNIQUE_IDS
}

/// Writes `godot` as the `godot` section of the project file at `file`, keeping everything else
/// in the file (and in the section, keys this version doesn't know).
pub fn write(file: &Path, godot: GodotSetup, lock_folder: &Path) -> Result<(), String> {
    validate(&godot, file)?;
    ProjectFile::edit(file, lock_folder, |data| {
        data.set_godot(godot);
    })
    .map_err(|e| e.to_string())
}

/// Checks stored export settings before they are used or written.
pub fn validate(settings: &GodotSetup, file: &Path) -> Result<(), String> {
    if let Some(mode) = &settings.tile_sets
        && !matches!(mode.as_str(), "shared" | "embedded")
    {
        return Err(format!(
            "{}: unknown Godot tileSets mode {mode:?}; use shared or embedded",
            file.display()
        ));
    }
    let project = settings
        .project
        .as_deref()
        .map(|p| bt_project::paths::resolve(file, p));
    if let Some(project) = &project
        && !project.join("project.godot").is_file()
    {
        return Err(format!(
            "{}: Godot project {} has no project.godot",
            file.display(),
            project.display()
        ));
    }
    for (name, path) in [
        ("levels", &settings.levels),
        ("images", &settings.images),
        ("tileSetFolder", &settings.tile_set_folder),
    ] {
        if let Some(path) = path
            && in_project(Path::new("/"), path).is_none()
        {
            return Err(format!(
                "{}: godot.{name} must be a res:// folder inside the project",
                file.display()
            ));
        }
    }
    Ok(())
}
