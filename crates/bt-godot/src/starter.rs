//! The starter level scene (docs/design/12-godot-export.md): a scene that inherits a map's export, for the user's
//! own nodes. Every export replaces the generated scene, so Godot work goes in a scene built on
//! it. better-tiled writes the starter once, when the Godot setup asks for one, and never again:
//! from then on it is the user's, and no export reads or writes it. Writing it ourselves also
//! means Godot's first save of it is a load and save, which writes the `parent_id_path`s that keep
//! overrides on nodes the map renames.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::read::{self, uid_to_text};
use super::{contract, project_root, project_scan, res_path, text};

/// The starter level scene of the generated scene `scene`: `<stem>_level.tscn` beside it.
pub fn path(scene: &Path) -> PathBuf {
    let stem = scene.file_stem().unwrap_or_default().to_string_lossy();
    scene.with_file_name(format!("{stem}_level.tscn"))
}

/// How the starter's text begins (after `; `), which tells a starter from other scenes.
pub(super) const FIRST_WORDS: &str = "A level scene for ";

/// Writes the starter level scene of the generated scene `scene`, unless a file is already in its
/// place. Returns its path when it was written; `None` when a scene inheriting `scene` is already
/// there. Another file there (the export of a map called `<stem>_level`, say) is an error.
pub fn write(scene: &Path) -> Result<Option<PathBuf>, String> {
    // Through the file system, as the project's own path is (a link such as macOS's /var included).
    let scene = &super::resolved_path(&std::path::absolute(scene).map_err(|e| e.to_string())?);
    let path = path(scene);
    let generated = std::fs::read_to_string(scene)
        .map_err(|e| format!("couldn't read {}: {e}", scene.display()))?;
    let resource = read::parse(&generated).map_err(|e| format!("{}: {e}", scene.display()))?;
    let root = resource
        .root()
        .map(|(node, _)| node.name)
        .ok_or_else(|| format!("{}: the scene has no root node", scene.display()))?;
    let project = project_root(scene)?;
    let (Some(base), Some(own)) = (res_path(&project, scene), res_path(&project, &path)) else {
        return Err(format!(
            "{} isn't inside the Godot project {}",
            scene.display(),
            project.display()
        ));
    };
    let name = scene
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    if path.exists() || path.is_symlink() {
        if inherits(&path, &base, resource.uid().as_deref()) {
            return Ok(None);
        }
        let what = match contract::marker_in(&path) {
            Some(map) => format!(
                "the export of {}",
                Path::new(&map)
                    .file_name()
                    .map_or(map.clone(), |n| n.to_string_lossy().into_owned())
            ),
            None => "a file of its own".to_owned(),
        };
        return Err(format!(
            "{own} is already there, {what}, not a scene that inherits {name}"
        ));
    }
    // A uid of its own, derived from its path as the export's are, unless another file has it.
    let uid = contract::derived_uid(&own, 0);
    let scan = project_scan::scan(&project);
    if !scan.errors.is_empty() {
        return Err(scan.errors.join("; "));
    }
    let uid = scan
        .carrying(uid, &path)
        .next()
        .is_none()
        .then(|| uid_to_text(uid));
    let text = starter(
        &name,
        &base,
        resource.uid().as_deref(),
        uid.as_deref(),
        &root,
    );
    // Never over a file, even one that appeared just now.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| format!("couldn't write {}: {e}", path.display()))?;
    file.write_all(text.as_bytes())
        .map_err(|e| format!("couldn't write {}: {e}", path.display()))?;
    Ok(Some(path))
}

/// Whether the scene at `path` inherits the scene at the `res://` path `base` (whose uid is
/// `uid`): its root instances it.
fn inherits(path: &Path, base: &str, uid: Option<&str>) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(resource) = read::parse(&text) else {
        return false;
    };
    let Some(id) = resource.root().and_then(|(root, _)| root.instance) else {
        return false;
    };
    resource
        .ext_resources()
        .any(|e| e.id == id && (e.path == base || (uid.is_some() && e.uid.as_deref() == uid)))
}

/// The starter's text: guidance (Godot drops the `;` lines when it saves the scene), and a root
/// that instances the generated scene `base` (named `name`), whose root is `root`.
fn starter(
    name: &str,
    base: &str,
    base_uid: Option<&str>,
    uid: Option<&str>,
    root: &str,
) -> String {
    let uid_attr =
        |uid: Option<&str>| uid.map_or_else(String::new, |u| format!(" uid={}", text::string(u)));
    format!(
        "; {FIRST_WORDS}{name}, written once by better-tiled and never again: it is yours.\n\
         ; Every export of the map replaces {name}, so put the player, the camera and scripts here,\n\
         ; at the root or under a generated layer. Don't paint on the generated layers here (this scene\n\
         ; would keep its own copy of their tiles, hiding later exports): add a TileMapLayer of your own.\n\
         [gd_scene format=3{}]\n\n\
         [ext_resource type=\"PackedScene\"{} path={} id=\"1\"]\n\n\
         [node name={} instance=ExtResource(\"1\")]\n",
        uid_attr(uid),
        uid_attr(base_uid),
        text::string(base),
        text::string(root),
    )
}
