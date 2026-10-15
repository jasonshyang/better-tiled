//! Planning and performing Godot exports.

use std::path::{Path, PathBuf};

use bt_core::{CustomTypes, Map};

use crate::plan::{Kind, Plan};
use crate::resolve::resolved;
use crate::{Build, Error, Note, Options, Report, Status};
use crate::{
    contract, grid, images, inheritors, project_scan, read, setup, shared, tiledata, tileset,
    version,
};
use bt_project::{GodotSetup, ProjectFile};
use bt_utils::paths::canonical_or_clean as normalized;
use grid::Grid;
use images::Images;

/// AutoMap's rule tiles (Empty, Ignore, …) only mean something in rule maps.
pub(crate) const RULE_TILES: &str =
    "the map uses AutoMap's rule tiles (Empty, Ignore, …): they are left out of the scene";

/// Writes the map at `map_path` as the Godot scene `target` (a `.tscn` inside a Godot project).
pub fn export(
    map_path: &Path,
    loader: &mut bt_tmx::Loader,
    target: &Path,
    options: &Options,
) -> Result<Report, Error> {
    export_from(map_path, loader, target, options, false)
}

/// [`export`]; `rebuilding` when every generated scene of the project is exported again
/// ([`export_all`]), which makes a missing shared TileSet afresh.
fn export_from(
    map_path: &Path,
    loader: &mut bt_tmx::Loader,
    target: &Path,
    options: &Options,
    rebuilding: bool,
) -> Result<Report, Error> {
    let map_path = normalized(&std::path::absolute(map_path).map_err(|e| e.to_string())?);
    scene_target(target)?;
    let loaded = loader
        .load_map(&map_path)
        .map_err(|e| format!("couldn't load {}: {e}", map_path.display()))?;
    let Planned {
        plan,
        scan,
        mut report,
    } = plan_export(
        &loaded.value,
        &map_path,
        loader.custom_types(),
        target,
        options,
        rebuilding,
    )?;
    contract::export(&plan, &scan, options, &mut report)?;
    let loading = loaded
        .warnings
        .into_iter()
        .map(|w| Note::warning(crate::Code::MapLoading, w.message, Vec::new()));
    report.notes.splice(0..0, loading);
    Ok(report)
}

/// Writes `map` (as loaded, or as the editor has it), whose file is `map_path`, as the Godot
/// scene `target`.
pub fn export_map(
    map: &Map,
    map_path: &Path,
    custom_types: &CustomTypes,
    target: &Path,
    options: &Options,
) -> Result<Report, Error> {
    let Planned {
        plan,
        scan,
        mut report,
    } = plan_export(map, map_path, custom_types, target, options, false)?;
    contract::export(&plan, &scan, options, &mut report)?;
    Ok(report)
}

/// How many images from outside the Godot project exporting `map` (whose file is `map_path`) to
/// `target` copies into it, as the export itself finds them; writes nothing.
pub fn images_copied(
    map: &Map,
    map_path: &Path,
    custom_types: &CustomTypes,
    target: &Path,
) -> Result<usize, Error> {
    plan_export(
        map,
        map_path,
        custom_types,
        target,
        &Options::default(),
        false,
    )
    .map(|planned| planned.report.counts.images)
}

/// What exporting the map at `map_path` to `target` would find and do, writing nothing: for each
/// file, whether it is up to date, the export's own, changed since, or someone else's.
pub fn status(
    map_path: &Path,
    loader: &mut bt_tmx::Loader,
    target: &Path,
) -> Result<Status, Error> {
    let map_path = normalized(&std::path::absolute(map_path).map_err(|e| e.to_string())?);
    scene_target(target)?;
    let loaded = loader
        .load_map(&map_path)
        .map_err(|e| format!("couldn't load {}: {e}", map_path.display()))?;
    let Planned { plan, scan, report } = plan_export(
        &loaded.value,
        &map_path,
        loader.custom_types(),
        target,
        &Options::default(),
        false,
    )?;
    let mut status = contract::status(&plan, &scan)?;
    status.level_scenes = report.level_scenes;
    Ok(status)
}

/// A scene an export generated, as [`generated_scenes`] finds it: where it is, and the map its
/// root's marker names (or why that can't be told).
#[derive(Debug, serde::Serialize)]
pub struct Generated {
    /// The generated scene on disk.
    pub scene: PathBuf,
    /// The source map, or why its marker cannot be read.
    pub map: Result<PathBuf, String>,
}
/// A duplicated generated scene that exports leave alone.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct SceneCopy {
    /// The copied scene.
    pub scene: PathBuf,
    /// Its original map.
    pub map: PathBuf,
}

/// The scenes of a Godot project an export generated ([`generated_scenes`]), and the copies of
/// them.
#[derive(Debug, Default)]
pub struct Scenes {
    /// Generated.
    pub generated: Vec<Generated>,
    /// Copies of generated scenes, each with its map, away from where that map exports to:
    /// scenes that keep only the marker (Godot's Duplicate keeps it), and scenes with the header
    /// whose uid the scene where the map exports to has too (a copy made outside Godot). No
    /// export reads or writes them.
    pub copies: Vec<SceneCopy>,
}

/// The scenes in the Godot project at `project` that an export generated, in path order: those
/// with its header, and those Godot saved again (which keep only its marker) where their map
/// exports to, each with the map it was made from.
pub fn generated_scenes(project: &Path) -> Scenes {
    let scan = project_scan::scan(&normalized(project));
    if !scan.errors.is_empty() {
        return Scenes {
            generated: vec![Generated {
                scene: project.to_path_buf(),
                map: Err(scan.errors.join("; ")),
            }],
            copies: Vec::new(),
        };
    }
    let mut found = Vec::new();
    for e in scan
        .entries
        .iter()
        .filter(|e| e.path.extension().is_some_and(|x| x == "tscn"))
    {
        let map = match std::fs::read_to_string(&e.path) {
            Ok(text) => source_map(&e.path, &text, e.generated),
            Err(error) => e
                .generated
                .then(|| Err(format!("couldn't read {}: {error}", e.path.display()))),
        };
        if let Some(map) = map {
            // Where the map exports to, as far as can be told: only for the scenes it is needed for.
            let home = match &map {
                Ok(map)
                    if !e.generated
                        || e.uid
                            .is_some_and(|uid| scan.carrying(uid, &e.path).next().is_some()) =>
                {
                    Some(exported_there(map, &e.path))
                }
                _ => None,
            };
            found.push((e, map, home));
        }
    }
    let mut scenes = Scenes::default();
    for (e, map, home) in &found {
        // Only its header says a scene is the export wherever it is, and only while no scene where
        // its map exports to has its uid too.
        let at_home = |uid: u64| {
            found.iter().any(|(other, _, home)| {
                other.path != e.path && other.uid == Some(uid) && *home == Some(true)
            })
        };
        let copy = *home == Some(false) && (!e.generated || e.uid.is_some_and(at_home));
        match map {
            Ok(map) if copy => scenes.copies.push(SceneCopy {
                scene: e.path.clone(),
                map: map.clone(),
            }),
            _ => scenes.generated.push(Generated {
                scene: e.path.clone(),
                map: map.clone(),
            }),
        }
    }
    scenes.generated.sort_by(|a, b| a.scene.cmp(&b.scene));
    scenes
}

/// Whether the map at `map_path` exports to `scene`, as far as can be told: its remembered
/// export, else its Godot setup's scene. A map that can't be read can't say otherwise.
fn exported_there(map_path: &Path, scene: &Path) -> bool {
    let Ok(loaded) = bt_tmx::Loader::builder().build().load_map(map_path) else {
        return true;
    };
    match scene_of(map_path, &loaded.value) {
        Ok(Some(target)) => resolved_path(&normalized(&target)) == resolved_path(scene),
        Ok(None) => false,
        Err(_) => true,
    }
}

/// The scene the map `map` (at `map_path`) is exported to when none is named: its remembered
/// Godot export (Tiled's `format="tscn"` too), else `<map stem>.tscn` in the levels folder of the
/// Godot setup covering it. A setup that can't be read is an error, not none.
pub fn scene_of(map_path: &Path, map: &Map) -> Result<Option<PathBuf>, String> {
    let export = &map.file.export;
    if let Some(target) = export.target.as_ref().filter(|_| export.format == "tscn") {
        return Ok(Some(target.path.clone()));
    }
    setup(map_path)?;
    Ok(setup::Setup::of(map_path)?.and_then(|s| s.scene(map_path)))
}

/// The map the scene at `path` (its `text`) was made from, as its root's marker names it; `None`
/// for a scene without the marker that hasn't the export's header either: not a generated one.
fn source_map(path: &Path, text: &str, header: bool) -> Option<Result<PathBuf, String>> {
    // Only a scene that names the marker at all is parsed.
    if !header && !text.contains(contract::MARKER) {
        return None;
    }
    let parsed = read::parse(text);
    let source = parsed
        .as_ref()
        .ok()
        .and_then(|r| r.root())
        .and_then(|(_, root)| root.first(contract::MARKER))
        .and_then(read::string);
    match (source, parsed) {
        (Some(source), _) => Some(Ok(normalized(
            &path.parent().unwrap_or(Path::new("")).join(source),
        ))),
        (None, _) if !header => None,
        (None, Err(e)) => Some(Err(format!("{}: {e}", path.display()))),
        (None, Ok(_)) => Some(Err(format!(
            "{} doesn't name the map it was made from (its root has no {})",
            path.display(),
            contract::MARKER
        ))),
    }
}

/// A generated scene [`export_all`] exported again (or, when nothing was written, would have):
/// the scene, the map it is made from, and how that went.
#[derive(Debug)]
pub struct Exported {
    /// The scene path verified by Godot.
    pub scene: PathBuf,
    /// Map.
    pub map: Option<PathBuf>,
    /// Outcome.
    pub outcome: Result<Report, Error>,
}

/// Every generated scene of a Godot project exported again ([`export_all`]).
#[derive(Debug)]
pub struct AllExported {
    /// Scenes.
    pub scenes: Vec<Exported>,
    /// Whether the exports wrote (their outcomes are of the real export), or wrote nothing (of
    /// an export with `check`).
    pub written: bool,
    /// Copies of generated scenes, left alone ([`Scenes::copies`]).
    pub copies: Vec<SceneCopy>,
}

/// Exports every generated scene of the Godot project at `project` again, from the map its marker
/// names (each loaded with a loader from `loader`), all or nothing: first writing nothing, as
/// `check` does; then, unless `options.check` or one of them would be refused or fails, for
/// real. A missing shared TileSet is made afresh. Copies of generated scenes are left alone.
pub fn export_all(
    project: &Path,
    loader: &mut dyn FnMut(&Path) -> Result<bt_tmx::Loader, String>,
    options: &Options,
) -> Result<AllExported, Error> {
    let project = normalized(&std::path::absolute(project).map_err(|e| e.to_string())?);
    if !project.join("project.godot").is_file() {
        return Err(Error::Failed(format!(
            "{} isn't a Godot project: it has no project.godot",
            project.display()
        )));
    }
    let Scenes {
        generated: scenes,
        copies,
    } = generated_scenes(&project);
    let mut each = |check: bool| -> Vec<Exported> {
        let options = Options {
            check,
            ..options.clone()
        };
        scenes
            .iter()
            .map(|Generated { scene, map }| {
                let outcome = match map {
                    Ok(map) => loader(map).map_err(Error::Failed).and_then(|mut loader| {
                        export_from(map, &mut loader, scene, &options, true)
                    }),
                    Err(e) => Err(Error::Failed(e.clone())),
                };
                Exported {
                    scene: scene.clone(),
                    map: map.as_ref().ok().cloned(),
                    outcome,
                }
            })
            .collect()
    };
    let checked = each(true);
    if options.check || checked.iter().any(|e| e.outcome.is_err()) {
        return Ok(AllExported {
            scenes: checked,
            written: false,
            copies,
        });
    }
    Ok(AllExported {
        scenes: each(false),
        written: true,
        copies,
    })
}

struct Planned {
    plan: Plan,
    scan: project_scan::Scan,
    report: Report,
}

/// Builds every file an export writes, in memory, and scans the Godot project they go into
/// (docs/design/12-godot-export.md).
fn plan_export(
    map: &Map,
    map_path: &Path,
    custom_types: &CustomTypes,
    target: &Path,
    options: &Options,
    rebuilding: bool,
) -> Result<Planned, Error> {
    plan_export_cached(
        map,
        map_path,
        custom_types,
        target,
        options,
        rebuilding,
        &mut project_scan::ScanCache::default(),
    )
}

fn plan_export_cached(
    map: &Map,
    map_path: &Path,
    custom_types: &CustomTypes,
    target: &Path,
    options: &Options,
    rebuilding: bool,
    cache: &mut project_scan::ScanCache,
) -> Result<Planned, Error> {
    let map_path = normalized(&std::path::absolute(map_path).map_err(|e| e.to_string())?);
    let target = scene_target(target)?;
    crate::validate::map(map)?;
    let grid = Grid::of(map).map_err(|e| format!("{}: {e}", map_path.display()))?;
    let project = project_root(&target)?;
    let scene_dir = normalized(target.parent().unwrap_or(&project));
    let scan = cache.scan(&project);
    if !scan.errors.is_empty() {
        return Err(Error::Failed(scan.errors.join("; ")));
    }
    let mut rule_slots = crate::resolve::rule_slots(map);
    let mut source_names = tileset::source_names(map, &map_path, &project);
    let mut tileset_dirs = tiledata::tileset_dirs(map, &map_path);
    // AutoMap's record (the level-scene check) names tilesets as the map file does.
    let as_loaded = map;
    let mut map = resolved(map, custom_types);
    crate::validate::map(&map)?;
    let mut shared = shared::Shared::of(
        &map, &map_path, &project, &target, &scan, options, rebuilding,
    )
    .map_err(|e| format!("{}: {e}", map_path.display()))?;
    if let Some(shared) = &mut shared {
        shared.extend(
            &mut map,
            &mut source_names,
            &mut tileset_dirs,
            &mut rule_slots,
            &project,
            custom_types,
        );
    }
    let map_dir = map_path
        .parent()
        .map_or_else(PathBuf::new, Path::to_path_buf);
    let user = res_path(&project, &target).unwrap_or_default();
    // A setup's own folder for copies, else `images/` next to the scene (or to a shared TileSet,
    // which every map of its grid uses alike).
    let near = shared.as_ref().map_or(scene_dir.as_path(), |s| s.folder());
    let copies = setup::images_for(&map_path, &project)?.unwrap_or_else(|| near.join("images"));
    let images = Images::builder(project.clone(), map_dir.clone(), user).copies_folder(copies);
    let mut build = Build::new(&map, grid, images);
    build.rule_slots = rule_slots;
    build.names.unique_ids = version::unique_ids(&build.project);
    build.tileset.names = source_names;
    build.tileset.data.dirs = tileset_dirs;
    build.tileset.shared = shared;
    build.setup = setup(&map_path)?;
    if build.tileset.shared.is_some() {
        // Its images are placed before the scene's own use them: the same whichever map it is.
        build.tileset_contents();
    }
    let name = map_path
        .file_stem()
        .map_or_else(|| "Map".to_owned(), |s| s.to_string_lossy().into_owned());
    build.build(&name, &contract::marker(&target, &map_path))?;
    if build.tileset.sources.iter().any(|source| {
        source.id > u16::MAX as usize
            || source
                .tiles
                .keys()
                .any(|(x, y)| *x < 0 || *y < 0 || *x > u16::MAX as i32 || *y > u16::MAX as i32)
    }) {
        return Err(Error::Failed(
            "TileSet source IDs or atlas coordinates exceed Godot cell limits".into(),
        ));
    }
    let renamed = build.identity(&map_path, &target);
    build.instance_uids(&scan);
    let mut plan = Plan::default();
    let body = build.write();
    if !build.failures.is_empty() {
        return Err(Error::Failed(build.failures.join("; ")));
    }
    plan.add(
        Kind::Scene,
        target.clone(),
        body.into_bytes(),
        Some(map_path.clone()),
    );
    if let Some((path, bytes, from, users)) = build.shared_tileset() {
        plan.tileset(path, bytes, from, users);
    }
    if let Some(missing) = build.images.missing() {
        return Err(Error::Failed(format!("{}: {missing}", map_path.display())));
    }
    let mut report = Report {
        counts: build.counts(),
        notes: std::mem::take(&mut build.notes),
        renamed,
        consumed: build.consumed.names(),
        ..Report::default()
    };
    build.images.plan(&mut plan, &scan, &mut report.notes);
    if let Some(missing) = build.images.missing() {
        return Err(Error::Failed(missing));
    }
    if !build.failures.is_empty() {
        return Err(Error::Failed(build.failures.join("; ")));
    }
    inheritors::check(&scan, &target, &plan, as_loaded, &map_path, &mut report);
    Ok(Planned { plan, scan, report })
}

/// The `godot` section of the map's better-tiled.json (docs/design/12-godot-export.md), if it has one. A file that
/// can't be read fails the export: without it, where the TileSet and the image copies go isn't
/// known.
fn setup(map_path: &Path) -> Result<GodotSetup, String> {
    let Some(file) = ProjectFile::find(map_path, None).map_err(|e| e.to_string())? else {
        return Ok(GodotSetup::default());
    };
    match ProjectFile::load(&file) {
        Ok(data) => {
            let godot = data.godot().cloned().unwrap_or_default();
            setup::validate(&godot, &file)?;
            Ok(godot)
        }
        Err(e) => Err(format!(
            "couldn't read the Godot setup ({e}): fix the file, and export again"
        )),
    }
}

/// The scene file `target` names, made absolute (through the file system as far as it exists,
/// so it lies inside the project's own path): it must be a `.tscn`.
pub(crate) fn scene_target(target: &Path) -> Result<PathBuf, String> {
    let target = std::path::absolute(target).map_err(|e| e.to_string())?;
    if target.extension().is_none_or(|e| e != "tscn") {
        return Err(format!(
            "{}: a Godot scene file ends in .tscn",
            target.display()
        ));
    }
    Ok(resolved_path(&target))
}

/// `path` with its longest existing start resolved through the file system.
pub(crate) fn resolved_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => resolved_path(parent).join(name),
        _ => normalized(path),
    }
}

/// The folder holding `project.godot`, at or above the scene's folder.
pub(crate) fn project_root(target: &Path) -> Result<PathBuf, String> {
    let mut dir = target.parent();
    while let Some(d) = dir {
        if d.join("project.godot").is_file() {
            return Ok(normalized(d));
        }
        dir = d.parent();
    }
    Err(format!(
        "{} isn't inside a Godot project (no project.godot in its folder or above)",
        target.display()
    ))
}

/// The `res://` path of a file inside the project.
pub(crate) fn res_path(project: &Path, file: &Path) -> Option<String> {
    let relative = normalized(file).strip_prefix(project).ok()?.to_path_buf();
    Some(format!(
        "res://{}",
        relative.to_string_lossy().replace('\\', "/")
    ))
}

/// An exporter whose resource-header cache survives repeat exports.
#[derive(Default)]
pub struct Exporter {
    cache: project_scan::ScanCache,
}
impl Exporter {
    /// An exporter with an empty project cache.
    pub fn new() -> Self {
        Self::default()
    }
    /// Exports an in-memory map, reusing the project scan from previous exports.
    pub fn export_map(
        &mut self,
        map: &Map,
        path: &Path,
        types: &CustomTypes,
        target: &Path,
        options: &Options,
    ) -> Result<Report, Error> {
        let Planned {
            plan,
            scan,
            mut report,
        } = plan_export_cached(map, path, types, target, options, false, &mut self.cache)?;
        contract::export(&plan, &scan, options, &mut report)?;
        Ok(report)
    }
}
