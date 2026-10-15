//! The level-scene check (docs/design/12-godot-export.md): what Godot will do with the scenes that inherit or
//! instance the export (level scenes) once the new export is there. It only reads them: each
//! `.tscn` the project scan lists as referring to the export is parsed, and Godot's loading of it
//! is replayed on the new export's nodes. Godot 4.7.2 does this (checked headless):
//!
//! - **A node's parent** is found by `parent_id_path` (the `unique_id`s down to it), else by its
//!   path. A parent found neither way is the root, and the node is called
//!   `<parent path, / as _>#<name>` there ("Parent path … has vanished").
//! - **An override** (a node without `type` or `instance`) applies to the node of its name under
//!   that parent, else to the node with its `unique_id` wherever that went (renamed as above when
//!   the parent wasn't found), else to nothing ("… has vanished"). So a name taken by another
//!   node wins over the id.
//! - **A signal connection** is found by its two paths, and dropped without a word when one is
//!   gone.
//! - **An override of a node whose type changed** (`godotNode` making a tile object a body)
//!   sets what the new type has, and drops the rest without a word.
//!
//! Which layer or object a node stands for comes from the `unique_id`s, else from the scene the
//! export replaces (its `metadata/layer_id` and `metadata/id`), so exports without ids (Godot
//! before 4.6) are understood too. What the check finds goes in the report, as notes and as
//! [`LevelScene`]s; it never writes a level scene.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use bt_core::{Layer, LayerId, Map};

use super::plan::{Kind, Plan};
use super::project_scan::Scan;
use super::read::{self, Resource};
use super::report::{Element, Note, Report, Severity};
use super::{contract, names, res_path};
use bt_project::ProjectFile;

/// Settings every node type the export writes has (CanvasItem's and Node2D's but `skew`, which
/// a Label lacks): an override of only these applies whatever the node's type becomes.
const SHARED: [&str; 18] = [
    "position",
    "rotation",
    "scale",
    "visible",
    "modulate",
    "self_modulate",
    "show_behind_parent",
    "top_level",
    "clip_children",
    "light_mask",
    "visibility_layer",
    "z_index",
    "z_as_relative",
    "y_sort_enabled",
    "texture_filter",
    "texture_repeat",
    "material",
    "use_parent_material",
];

/// A scene that inherits or instances the export, and what Godot will do with it.
#[derive(Clone, Debug, serde::Serialize)]
pub struct LevelScene {
    /// The path of the file on disk.
    pub path: PathBuf,
    /// What Godot will do with the scene when the export changes.
    pub findings: Vec<Finding>,
    /// Why it couldn't be checked: it isn't a scene Godot could read.
    pub unreadable: Option<String>,
    /// The findings in words, one note per kind (the export's notes have them too).
    pub notes: Vec<Note>,
}

/// One thing Godot will do with a level scene's node or connection.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Finding {
    pub fate: Fate,
    /// The node's path in the level scene, as written; for a connection
    /// `signal: from → to`.
    pub node: String,
    /// Where it is with the new export: the node Godot puts it on, or for a connection the two
    /// paths it would need now.
    pub now: Option<String>,
    /// The layer or object it stands for.
    pub element: Option<Element>,
    /// For [`Fate::TypeChanged`], how the node's type changed.
    pub types: Option<TypeChange>,
}

/// A node's type in the export a level scene was made against, and in the new one.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct TypeChange {
    pub was: String,
    pub now: String,
    /// The child that has the node's sprite settings now (a tile object `godotNode` made a
    /// body or area), as the level scene's path.
    pub sprite: Option<String>,
}

/// What happens to a node of a level scene (docs/design/12-godot-export.md's table).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub enum Fate {
    /// Godot finds the node again by its `unique_id` (or its parent's) where the export renamed
    /// or moved it.
    Follows,
    /// Found by its `unique_id`, but its parent isn't, so Godot renames it `A#B`.
    Renamed,
    /// An override Godot finds by path, which the export changed: it is lost.
    Lost,
    /// An override that now lands on another node, the one that took its name.
    Retargeted,
    /// An override of a node the export no longer has: Godot drops it.
    Dropped,
    /// A node the level adds under a node Godot can't find: moved to the root as `A#B`.
    MovedToRoot,
    /// A `tile_map_data` override on a generated layer: what the map paints there stops
    /// showing.
    FrozenTiles,
    /// An override of a node whose type the export changed (`godotNode` making a tile object a
    /// body): Godot ignores the settings the new type doesn't have.
    TypeChanged,
    /// A signal connection to or from a path that no longer exists: Godot drops it.
    Disconnected,
    /// An override of an object AutoMap placed: kept only while the rules place it the same way.
    AutoMapPlaced,
}

impl Fate {
    pub fn code(self) -> crate::Code {
        match self {
            Fate::Follows => crate::Code::LevelSceneFollows,
            Fate::Renamed => crate::Code::LevelSceneRenamed,
            Fate::Lost => crate::Code::LevelSceneLost,
            Fate::Retargeted => crate::Code::LevelSceneRetargeted,
            Fate::Dropped => crate::Code::LevelSceneDropped,
            Fate::MovedToRoot => crate::Code::LevelSceneMovedToRoot,
            Fate::FrozenTiles => crate::Code::LevelSceneTiles,
            Fate::TypeChanged => crate::Code::LevelSceneType,
            Fate::Disconnected => crate::Code::LevelSceneConnection,
            Fate::AutoMapPlaced => crate::Code::LevelSceneAutomap,
        }
    }

    pub fn severity(self) -> Severity {
        match self {
            Fate::Follows | Fate::AutoMapPlaced => Severity::Info,
            _ => Severity::Warning,
        }
    }
}

/// Checks the level scenes of the export to `target` (the scene `plan` holds for it) and adds
/// what Godot will do with them to `report`. `map` (as loaded, its tilesets not yet embedded)
/// and `map_path` say which objects AutoMap placed.
pub(super) fn check(
    scan: &Scan,
    target: &Path,
    plan: &Plan,
    map: &Map,
    map_path: &Path,
    report: &mut Report,
) {
    let Some(res) = res_path(&scan.project, target) else {
        return;
    };
    // The uid levels know the export by: the one it keeps (the scan read it), or the one a new
    // file gets.
    let kept = scan
        .entries
        .iter()
        .find(|e| e.path == target)
        .and_then(|e| e.uid);
    let uid = kept.unwrap_or_else(|| contract::derived_uid(&res, 0));
    let tscn = |p: &Path| p.extension().is_some_and(|e| e == "tscn");
    let levels: Vec<PathBuf> = scan
        .referencing(&res, Some(uid))
        .filter(|e| !e.generated && e.path != target && tscn(&e.path))
        .map(|e| e.path.clone())
        .collect();
    if levels.is_empty() {
        return;
    }
    let Some(scene) = plan
        .outputs
        .iter()
        .find(|o| o.kind == Kind::Scene && o.path == target)
    else {
        return;
    };
    let Ok(new) = read::parse(&String::from_utf8_lossy(&scene.bytes)).map(|r| Base::of(&r)) else {
        return;
    };
    // The export being replaced says which layer or object a path stood for.
    let old_text = std::fs::read_to_string(target).ok();
    let old = old_text
        .as_deref()
        .and_then(|t| read::parse(t).ok())
        .filter(|r| r.header.kind == "gd_scene")
        .map(|r| Base::of(&r));
    let placed = automap_placed(map, map_path);
    let base_name = file_name(target);
    for path in levels {
        let compared = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| compare(&text, &res, uid, old.as_ref(), &new, &placed));
        let level = match compared {
            Ok(Some(findings)) => LevelScene {
                path,
                findings,
                unreadable: None,
                notes: Vec::new(),
            },
            // It refers to the export without inheriting or instancing it (a property holding it).
            Ok(None) => continue,
            Err(e) => {
                let message = format!(
                    "couldn't read {} to see what Godot will do with it once {base_name} changes: {e}",
                    path.display()
                );
                LevelScene {
                    path,
                    findings: Vec::new(),
                    unreadable: Some(e),
                    notes: vec![Note::warning(
                        crate::Code::LevelSceneUnreadable,
                        message,
                        Vec::new(),
                    )],
                }
            }
        };
        let mut level = level;
        level.notes.extend(notes(&level, &base_name, new.ids));
        report.notes.extend(level.notes.iter().cloned());
        report.level_scenes.push(level);
    }
}

/// What Godot will do with the level scene `text` once the export at `res` (carrying `uid`)
/// is `new` instead of `old`; `None` when the scene neither inherits nor instances it.
fn compare(
    text: &str,
    res: &str,
    uid: u64,
    old: Option<&Base>,
    new: &Base,
    placed: &HashSet<u32>,
) -> Result<Option<Vec<Finding>>, String> {
    let level = read::parse(text)?;
    let ours: HashSet<String> = level
        .ext_resources()
        .filter(|e| e.path == res || e.uid.as_deref().and_then(read::text_to_uid) == Some(uid))
        .map(|e| e.id)
        .collect();
    let Some((root, _)) = level.root() else {
        return Ok(None);
    };
    let mut tree = Tree::new(new, root.unique_id);
    match &root.instance {
        Some(id) if ours.contains(id) => tree.mount(0, "."),
        Some(_) => {
            if let Some(root) = tree.nodes.get_mut(0) {
                root.opaque = true;
            }
        }
        None => {}
    }
    let mut seen = Vec::new();
    // Where each node the level names went, and what each override was meant for, by its path
    // as written.
    let mut placed_at: HashMap<String, Option<usize>> = HashMap::new();
    let mut meant_for: HashMap<String, Option<Key>> = HashMap::new();
    for (node, section) in level.nodes() {
        let Some(written_parent) = node.parent.clone() else {
            continue;
        };
        let written = node.path();
        let by_ids = (!node.parent_id_path.is_empty())
            .then(|| tree.by_ids(&node.parent_id_path))
            .flatten();
        let (parent, parent_lost) =
            match by_ids.map_or_else(|| tree.by_path(&written_parent), Found::Node) {
                Found::Node(p) => (p, false),
                // Inside another scene's instance: nothing of the export's.
                Found::Opaque => continue,
                Found::Missing => (0, true),
            };
        let recovered = format!(
            "{}#{}",
            written_parent.trim_start_matches("./").replace('/', "_"),
            node.name
        );
        let mount = if parent_lost {
            tree.mount_by_path(&written_parent)
        } else {
            tree.mount_of(parent)
        };
        if node.class.is_some() || node.instance.is_some() {
            let name = if parent_lost {
                recovered
            } else {
                node.name.clone()
            };
            let added = tree.add(parent, name, node.unique_id);
            match &node.instance {
                Some(id) if ours.contains(id) => tree.mount(added, &written),
                Some(_) => {
                    if let Some(node) = tree.nodes.get_mut(added) {
                        node.opaque = true;
                    }
                }
                None => {}
            }
            placed_at.insert(written.clone(), Some(added));
            if let Some(m) = mount {
                seen.push(Seen {
                    written,
                    written_parent,
                    mount: m,
                    parent_lost,
                    what: What::Added {
                        node: added,
                        parent,
                    },
                });
            }
            continue;
        }
        let Some(m) = mount else { continue };
        let by_name = tree.child(parent, &node.name);
        let by_uid = by_name
            .is_none()
            .then(|| {
                node.unique_id
                    .and_then(|u| tree.mounts.get(m).and_then(|m| m.by_uid.get(&u)).copied())
            })
            .flatten();
        if let (Some(t), true) = (by_uid, parent_lost)
            && let Some(node) = tree.nodes.get_mut(t)
        {
            node.name = recovered;
        }
        let found = by_name.map(|t| (t, false)).or(by_uid.map(|t| (t, true)));
        placed_at.insert(written.clone(), found.map(|(t, _)| t));
        let intended = tree
            .mounts
            .get(m)
            .and_then(|m| intended(&node.unique_id, &written, m, old, new));
        meant_for.insert(written.clone(), intended.clone());
        let tiles = section.get("tile_map_data").is_some();
        let typed = section
            .props
            .iter()
            .any(|(key, _)| !(SHARED.contains(&key.as_ref()) || key.starts_with("metadata/")));
        seen.push(Seen {
            written,
            written_parent,
            mount: m,
            parent_lost,
            what: What::Override {
                found,
                intended,
                tiles,
                typed,
            },
        });
    }
    if tree.mounts.is_empty() {
        return Ok(None);
    }
    let mut findings = Vec::new();
    for s in &seen {
        let parent = || meant(&s.written_parent, &meant_for, &tree, old, new);
        findings.extend(s.findings(&tree, old, new, placed, parent));
    }
    for c in level.connections() {
        let ends = [c.from.as_str(), c.to.as_str()];
        let missing: Vec<&str> = ends
            .iter()
            .copied()
            .filter(|p| matches!(tree.by_path(p), Found::Missing))
            .collect();
        // Only what the export is part of; the level's own nodes don't change with it.
        if missing.is_empty() || missing.iter().all(|p| tree.mount_by_path(p).is_none()) {
            continue;
        }
        let now_of = |p: &str| -> Option<String> {
            if !missing.contains(&p) {
                return Some(p.to_owned());
            }
            // A node the level names, else (an override lost, say) the node it was meant for.
            if let Some(&Some(t)) = placed_at.get(p) {
                return Some(tree.path(t));
            }
            let key = meant(p, &meant_for, &tree, old, new)?;
            tree.of_key(tree.mount_by_path(p)?, &key)
                .map(|t| tree.path(t))
        };
        let now = match (now_of(&c.from), now_of(&c.to)) {
            (Some(from), Some(to)) => Some(format!("{from} → {to}")),
            _ => None,
        };
        let element = missing
            .iter()
            .find_map(|p| meant(p, &meant_for, &tree, old, new)?.element());
        findings.push(Finding {
            fate: Fate::Disconnected,
            node: format!("{}: {} → {}", c.signal, c.from, c.to),
            now,
            element,
            types: None,
        });
    }
    Ok(Some(findings))
}

/// A level node that is part of the export, as Godot loads it.
struct Seen {
    written: String,
    written_parent: String,
    mount: usize,
    /// Godot found its parent neither by id nor by path.
    parent_lost: bool,
    what: What,
}

enum What {
    /// A node the level adds, and where it went.
    Added { node: usize, parent: usize },
    /// An override: the node it applies to (and whether that was found by `unique_id` rather
    /// than by name), what it was meant for, whether it sets `tile_map_data`, and whether it
    /// sets anything a node of another type might not have (beyond [`SHARED`] and metadata).
    Override {
        found: Option<(usize, bool)>,
        intended: Option<Key>,
        tiles: bool,
        typed: bool,
    },
}

impl Seen {
    /// What Godot does with it, with `new` in place of `old`; `meant_parent` says what its
    /// parent (as written) stands for.
    fn findings(
        &self,
        tree: &Tree,
        old: Option<&Base>,
        new: &Base,
        placed: &HashSet<u32>,
        meant_parent: impl Fn() -> Option<Key>,
    ) -> Vec<Finding> {
        let finding = |fate, now: Option<String>, element| Finding {
            fate,
            node: self.written.clone(),
            now,
            element,
            types: None,
        };
        let mut out = Vec::new();
        match &self.what {
            &What::Added { node, parent } => {
                let now = tree.path(node);
                if self.parent_lost {
                    out.push(finding(
                        Fate::MovedToRoot,
                        Some(now),
                        meant_parent().and_then(|k| k.element()),
                    ));
                } else if tree.path(parent) != self.written_parent {
                    out.push(finding(
                        Fate::Follows,
                        Some(now),
                        tree.key(parent).and_then(Key::element),
                    ));
                }
            }
            What::Override {
                found,
                intended,
                tiles,
                typed,
            } => {
                let element = intended.as_ref().and_then(Key::element);
                match *found {
                    Some((t, true)) if self.parent_lost => {
                        out.push(finding(Fate::Renamed, Some(tree.path(t)), element))
                    }
                    Some((t, true)) if tree.path(t) != self.written => {
                        out.push(finding(Fate::Follows, Some(tree.path(t)), element))
                    }
                    Some((t, false)) => {
                        let landed = tree.key(t);
                        if intended.is_some() && landed.is_some() && intended.as_ref() != landed {
                            out.push(finding(Fate::Retargeted, Some(tree.path(t)), element));
                        } else if tree.path(t) != self.written {
                            out.push(finding(
                                Fate::Follows,
                                Some(tree.path(t)),
                                landed.and_then(Key::element),
                            ));
                        }
                    }
                    Some(_) => {}
                    None => match intended.as_ref().and_then(|k| tree.of_key(self.mount, k)) {
                        Some(t) => out.push(finding(Fate::Lost, Some(tree.path(t)), element)),
                        None => out.push(finding(Fate::Dropped, None, element)),
                    },
                }
                if let Some((t, _)) = *found {
                    let base = tree
                        .nodes
                        .get(t)
                        .and_then(|n| n.base)
                        .and_then(|b| new.nodes.get(b));
                    if *tiles && base.is_some_and(|b| b.tiles) {
                        out.push(finding(
                            Fate::FrozenTiles,
                            Some(tree.path(t)),
                            base.and_then(|b| b.key.element()),
                        ));
                    }
                    // A retargeted override is on another node altogether, which says enough.
                    let retargeted = out.iter().any(|f| f.fate == Fate::Retargeted);
                    if *typed
                        && !retargeted
                        && let Some(b) = tree.nodes.get(t).and_then(|n| n.base)
                        && let Some(types) = type_change(old, new, b, &tree.path(t))
                    {
                        out.push(Finding {
                            types: Some(types),
                            ..finding(
                                Fate::TypeChanged,
                                Some(tree.path(t)),
                                new.nodes.get(b).and_then(|n| n.key.element()),
                            )
                        });
                    }
                    // An object's shape goes with it.
                    if let Some(Element::Object(id)) = base.and_then(|b| b.key.element())
                        && placed.contains(&id)
                    {
                        out.push(finding(
                            Fate::AutoMapPlaced,
                            Some(tree.path(t)),
                            Some(Element::Object(id)),
                        ));
                    }
                }
            }
        }
        out
    }
}

/// How the type of the new export's node `b` (at `at` in the level scene) differs from that of
/// the node standing for the same in `old`, where both have one (neither is an instance).
fn type_change(old: Option<&Base>, new: &Base, b: usize, at: &str) -> Option<TypeChange> {
    let old = old?;
    let node = new.nodes.get(b)?;
    let was = old
        .by_key
        .get(&node.key)
        .and_then(|&i| old.nodes.get(i).and_then(|n| n.kind.clone()))?;
    let now = node.kind.clone().filter(|now| *now != was)?;
    let sprite_type = |kind: Option<&str>| matches!(kind, Some("Sprite2D" | "AnimatedSprite2D"));
    // A tile object's node made a body keeps its sprite as the child `Sprite` (objects.rs).
    let sprite = sprite_type(Some(&was))
        && !sprite_type(Some(&now))
        && new
            .nodes
            .iter()
            .any(|n| n.parent == Some(b) && n.name == "Sprite" && sprite_type(n.kind.as_deref()));
    Some(TypeChange {
        was,
        now,
        sprite: sprite.then(|| format!("{at}/Sprite")),
    })
}

/// What a level node at `written` (with `unique_id`) stands for in the export mounted at
/// `mount`: by its id in the new export or the old one, else by its path in the old one (or the
/// new one, where the old one is gone).
fn intended(
    unique_id: &Option<i32>,
    written: &str,
    mount: &Mount,
    old: Option<&Base>,
    new: &Base,
) -> Option<Key> {
    let bases = [Some(new), old];
    if let Some(u) = unique_id
        && let Some(key) = bases.iter().flatten().find_map(|b| {
            b.by_uid
                .get(u)
                .and_then(|&i| b.nodes.get(i).map(|n| n.key.clone()))
        })
    {
        return Some(key);
    }
    let by_path = mount.base_path(written).and_then(|path| {
        [old, Some(new)].iter().flatten().find_map(|b| {
            b.by_path
                .get(&path)
                .and_then(|&i| b.nodes.get(i).map(|n| n.key.clone()))
        })
    });
    // A layer's or object's id says which it is, even once the export that had it is replaced
    // (`godot::status` after an export). Only where the export writes ids: a level saved against
    // one without them carries ids Godot made up.
    by_path.or_else(
        || match unique_id.filter(|_| new.ids).and_then(names::element) {
            Some(Element::Layer(id)) => Some(Key::Layer(id)),
            Some(Element::Object(id)) => Some(Key::Object(id)),
            _ => None,
        },
    )
}

/// What the level's node at `written` was meant for: an override's own record, else what the
/// export has there.
fn meant(
    written: &str,
    meant: &HashMap<String, Option<Key>>,
    tree: &Tree,
    old: Option<&Base>,
    new: &Base,
) -> Option<Key> {
    if let Some(key) = meant.get(written) {
        return key.clone();
    }
    let m = tree.mount_by_path(written)?;
    intended(&None, written, tree.mounts.get(m)?, old, new)
}

/// What a node of an export stands for: a layer or object by its Tiled id, or a node under one.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize)]
enum Key {
    Root,
    Layer(u32),
    Object(u32),
    /// The Parallax2D holding a layer.
    Holder(u32),
    /// A node with no id of its own (an area's shape), by its name under its parent.
    Child(Box<Key>, String),
    /// A node whose id is ambiguous (a property named `id`, say), by its path.
    Path(String),
}

impl Key {
    fn element(&self) -> Option<Element> {
        match self {
            Key::Root | Key::Path(_) => None,
            Key::Layer(id) | Key::Holder(id) => Some(Element::Layer(*id)),
            Key::Object(id) => Some(Element::Object(*id)),
            Key::Child(parent, _) => parent.element(),
        }
    }
}

/// An export's nodes, found by path, `unique_id` and what they stand for.
struct Base {
    nodes: Vec<BaseNode>,
    by_path: HashMap<String, usize>,
    by_uid: HashMap<i32, usize>,
    by_key: HashMap<Key, usize>,
    /// It gives its nodes `unique_id`s (Godot 4.6 and later).
    ids: bool,
}

struct BaseNode {
    name: String,
    parent: Option<usize>,
    uid: Option<i32>,
    key: Key,
    /// Its type, unless it is an instance.
    kind: Option<String>,
    /// A TileMapLayer.
    tiles: bool,
    /// An instance of another scene (`resPath`): what is under it isn't in the export.
    instance: bool,
}

impl Base {
    fn of(scene: &Resource) -> Base {
        let mut base = Base {
            nodes: Vec::new(),
            by_path: HashMap::new(),
            by_uid: HashMap::new(),
            by_key: HashMap::new(),
            ids: false,
        };
        let mut own: Vec<Option<Key>> = Vec::new();
        let mut parallax: Vec<bool> = Vec::new();
        let mut paths: Vec<String> = Vec::new();
        let mut children: Vec<Vec<usize>> = Vec::new();
        for (node, section) in scene.nodes() {
            let parent = node
                .parent
                .as_ref()
                .and_then(|p| base.by_path.get(p).copied());
            // A node whose parent isn't there yet isn't a scene Godot would load.
            if node.parent.is_some() && parent.is_none() {
                continue;
            }
            let key = match node.parent {
                None => Some(Key::Root),
                Some(_) => match names::identity(section) {
                    Some(Element::Layer(id)) => Some(Key::Layer(id)),
                    Some(Element::Object(id)) => Some(Key::Object(id)),
                    _ => None,
                },
            };
            let index = base.nodes.len();
            if let Some(p) = parent
                && let Some(children) = children.get_mut(p)
            {
                children.push(index);
            }
            base.by_path.insert(node.path(), index);
            paths.push(node.path());
            parallax.push(node.class.as_deref() == Some("Parallax2D"));
            if let Some(u) = node.unique_id {
                base.by_uid.insert(u, index);
                base.ids |= parent.is_some();
            }
            let tiles = node.class.as_deref() == Some("TileMapLayer");
            base.nodes.push(BaseNode {
                name: node.name,
                parent,
                uid: node.unique_id,
                key: Key::Root,
                kind: node.class,
                tiles,
                instance: node.instance.is_some(),
            });
            own.push(key);
            children.push(Vec::new());
        }
        // A key found twice says nothing.
        let mut count: HashMap<&Key, usize> = HashMap::new();
        for key in own.iter().flatten() {
            *count.entry(key).or_default() += 1;
        }
        let twice: HashSet<Key> = count
            .into_iter()
            .filter(|(_, n)| *n > 1)
            .map(|(k, _)| k.clone())
            .collect();
        for i in 0..base.nodes.len() {
            // A Parallax2D holding a layer is named after it, so it is known by the layer.
            let held = || {
                children
                    .get(i)
                    .into_iter()
                    .flatten()
                    .find_map(|&c| match own.get(c) {
                        Some(Some(Key::Layer(l))) => Some(*l),
                        _ => None,
                    })
            };
            let key = match own.get(i).cloned().flatten() {
                Some(key) if twice.contains(&key) => {
                    Key::Path(paths.get(i).cloned().unwrap_or_default())
                }
                Some(key) => key,
                None => match parallax
                    .get(i)
                    .copied()
                    .unwrap_or(false)
                    .then(held)
                    .flatten()
                {
                    Some(layer) => Key::Holder(layer),
                    None => base.child_key(i),
                },
            };
            base.by_key.insert(key.clone(), i);
            if let Some(node) = base.nodes.get_mut(i) {
                node.key = key;
            }
        }
        base
    }

    /// The key of a node with no id of its own (its parent comes first, so has its key).
    fn child_key(&self, i: usize) -> Key {
        let Some(node) = self.nodes.get(i) else {
            return Key::Path(format!("missing node {i}"));
        };
        match node.parent.and_then(|p| self.nodes.get(p)) {
            Some(parent) => Key::Child(Box::new(parent.key.clone()), node.name.clone()),
            None => Key::Root,
        }
    }
}

/// The level scene's nodes as Godot builds them: the new export's nodes under the node that
/// inherits or instances it (a mount), and the level's own.
struct Tree<'b> {
    nodes: Vec<TreeNode>,
    mounts: Vec<Mount>,
    new: &'b Base,
}

struct TreeNode {
    name: String,
    parent: Option<usize>,
    children: Vec<usize>,
    uid: Option<i32>,
    /// A node of the export: its index there.
    base: Option<usize>,
    /// The mount it belongs to (or is).
    mount: Option<usize>,
    /// Another scene's instance, whose nodes aren't known.
    opaque: bool,
}

/// Where the export is in the level scene.
struct Mount {
    /// The level's path to it (`.` when the level inherits it).
    written: String,
    by_uid: HashMap<i32, usize>,
    /// The tree node of each export node.
    nodes: Vec<usize>,
}

impl Mount {
    /// The export's path to the node the level calls `written`.
    fn base_path(&self, written: &str) -> Option<String> {
        if self.written == "." {
            return Some(written.to_owned());
        }
        if written == self.written {
            return Some(".".to_owned());
        }
        written
            .strip_prefix(&self.written)?
            .strip_prefix('/')
            .map(str::to_owned)
    }
}

enum Found {
    Node(usize),
    /// Inside another scene's instance.
    Opaque,
    Missing,
}

impl<'b> Tree<'b> {
    fn new(new: &'b Base, root_uid: Option<i32>) -> Tree<'b> {
        let root = TreeNode {
            name: String::new(),
            parent: None,
            children: Vec::new(),
            uid: root_uid,
            base: None,
            mount: None,
            opaque: false,
        };
        Tree {
            nodes: vec![root],
            mounts: Vec::new(),
            new,
        }
    }

    fn add(&mut self, parent: usize, name: String, uid: Option<i32>) -> usize {
        let index = self.nodes.len();
        self.nodes.push(TreeNode {
            name,
            parent: Some(parent),
            children: Vec::new(),
            uid,
            base: None,
            mount: None,
            opaque: false,
        });
        if let Some(parent) = self.nodes.get_mut(parent) {
            parent.children.push(index);
        }
        index
    }

    /// Puts the export's nodes under the node `at`, which the level calls `written`.
    fn mount(&mut self, at: usize, written: &str) {
        let m = self.mounts.len();
        let mut mount = Mount {
            written: written.to_owned(),
            by_uid: HashMap::new(),
            nodes: Vec::with_capacity(self.new.nodes.len()),
        };
        if let Some(node) = self.nodes.get_mut(at) {
            node.mount = Some(m);
        }
        for (i, node) in self.new.nodes.iter().enumerate() {
            let index = match node.parent {
                None => at,
                Some(p) => {
                    let Some(&parent) = mount.nodes.get(p) else {
                        continue;
                    };
                    let index = self.add(parent, node.name.clone(), node.uid);
                    if let Some(added) = self.nodes.get_mut(index) {
                        added.base = Some(i);
                        added.mount = Some(m);
                        added.opaque = node.instance;
                    }
                    if let Some(u) = node.uid {
                        mount.by_uid.insert(u, index);
                    }
                    index
                }
            };
            mount.nodes.push(index);
        }
        self.mounts.push(mount);
    }

    fn child(&self, parent: usize, name: &str) -> Option<usize> {
        self.nodes
            .get(parent)?
            .children
            .iter()
            .copied()
            .find(|&c| self.nodes.get(c).is_some_and(|n| n.name == name))
    }

    /// The node at `path` from the root.
    fn by_path(&self, path: &str) -> Found {
        let mut at = 0;
        for part in path
            .trim_start_matches("./")
            .split('/')
            .filter(|p| !p.is_empty() && *p != ".")
        {
            if self.nodes.get(at).is_some_and(|n| n.opaque) {
                return Found::Opaque;
            }
            match self.child(at, part) {
                Some(c) => at = c,
                None => return Found::Missing,
            }
        }
        Found::Node(at)
    }

    /// The node `parent_id_path` leads to: each id a node below the one before.
    fn by_ids(&self, ids: &[i32]) -> Option<usize> {
        let mut at = 0;
        for &id in ids {
            at = (1..self.nodes.len()).find(|&n| {
                self.nodes.get(n).is_some_and(|n| n.uid == Some(id)) && self.below(n, at)
            })?;
        }
        Some(at)
    }

    fn below(&self, mut node: usize, ancestor: usize) -> bool {
        while let Some(p) = self.nodes.get(node).and_then(|n| n.parent) {
            if p == ancestor {
                return true;
            }
            node = p;
        }
        false
    }

    /// The mount a node belongs to: its own or its nearest ancestor's.
    fn mount_of(&self, mut node: usize) -> Option<usize> {
        loop {
            if let Some(m) = self.nodes.get(node)?.mount {
                return Some(m);
            }
            node = self.nodes.get(node)?.parent?;
        }
    }

    /// The mount whose nodes the level's path `written` names.
    fn mount_by_path(&self, written: &str) -> Option<usize> {
        let written = written.trim_start_matches("./");
        self.mounts
            .iter()
            .enumerate()
            .filter(|(_, m)| m.base_path(written).is_some())
            .max_by_key(|(_, m)| m.written.len())
            .map(|(i, _)| i)
    }

    fn key(&self, node: usize) -> Option<&Key> {
        self.nodes
            .get(node)?
            .base
            .and_then(|b| self.new.nodes.get(b).map(|n| &n.key))
    }

    /// The tree node of the export node standing for `key` in mount `m`.
    fn of_key(&self, m: usize, key: &Key) -> Option<usize> {
        self.new
            .by_key
            .get(key)
            .and_then(|&b| self.mounts.get(m)?.nodes.get(b).copied())
    }

    /// A node's path from the root, as the level scene would write it.
    fn path(&self, node: usize) -> String {
        let Some(current) = self.nodes.get(node) else {
            return format!("missing node {node}");
        };
        match current.parent {
            None => ".".to_owned(),
            Some(0) => current.name.clone(),
            Some(p) => format!("{}/{}", self.path(p), current.name),
        }
    }
}

/// The ids of the objects AutoMap placed in `map`, from the project file's record of what was
/// painted (only for layers still as that record says).
fn automap_placed(map: &Map, map_path: &Path) -> HashSet<u32> {
    let Ok(Some(file)) = ProjectFile::find(map_path, None) else {
        return HashSet::new();
    };
    let Ok(data) = ProjectFile::load(&file) else {
        return HashSet::new();
    };
    let Ok(key) = bt_project::paths::key(&file, map_path) else {
        return HashSet::new();
    };
    let Some(entry) = data.automap(&key) else {
        return HashSet::new();
    };
    entry
        .painted
        .iter()
        .filter(|p| p.objects)
        .filter(|p| {
            map.layer(LayerId(p.layer))
                .and_then(Layer::as_object_layer)
                .is_some_and(|l| bt_project::checksum::objects(map, &l.objects) == p.checksum)
        })
        .flat_map(|p| p.generated.iter().copied())
        .collect()
}

/// The notes on one level scene: one per kind of finding, naming the nodes.
fn notes(level: &LevelScene, base: &str, ids: bool) -> Vec<Note> {
    let name = file_name(&level.path);
    let mut fates: Vec<Fate> = level.findings.iter().map(|f| f.fate).collect();
    fates.sort();
    fates.dedup();
    fates
        .into_iter()
        .map(|fate| {
            let findings: Vec<&Finding> = level.findings.iter().filter(|f| f.fate == fate).collect();
            let items = list(&findings, fate);
            let n = findings.len();
            let nodes = if n == 1 { "a node".to_owned() } else { format!("{n} nodes") };
            let message = match fate {
                Fate::Follows => format!("{name}: Godot finds {nodes} again by unique_id where this export renames or moves {}: {items}. Save {name} once in Godot so it keeps the new paths", them(n)),
                Fate::Renamed => format!("{name}: Godot finds {nodes} by unique_id but not by {} parent's path, so it keeps the override and renames {}: {items}. Scripts that find {} by path need the new name; an override of a node and of its parent in one level scene is saved by path, so avoid that", their(n), them(n), them(n)),
                Fate::Lost => {
                    let advice = if ids {
                        format!("Before Godot saves {name}, rename them back in the map and export again, save {name} once in Godot (it then finds nodes by unique_id), and rename them again")
                    } else {
                        "Godot projects before 4.6 find nodes by path only: rename them back, or move the overrides to the new paths".to_owned()
                    };
                    format!("{name} has overrides of {nodes} that Godot finds by path, which this export changes, so they will be lost: {items}. {advice}")
                }
                Fate::Retargeted => format!("{name} overrides {nodes} that this export no longer has where {name} looks, and Godot applies the overrides to the node that took {} name instead: {items}. Remove or move those overrides in {name}", their(n)),
                Fate::Dropped => format!("{name} overrides {nodes} that {base} no longer has, and Godot drops those overrides: {items}"),
                Fate::MovedToRoot => format!("{name} adds {nodes} under nodes Godot can't find by path in {base} any more, so Godot moves {} to the root: {items}. Put {} back in Godot", them(n), them(n)),
                Fate::FrozenTiles => format!("{name} paints its own tiles on {} of {base}: Godot shows those instead, so what the map paints there stops showing: {items}. Paint on a TileMapLayer of your own that uses the same TileSet, and remove the override", if n == 1 { "a layer".to_owned() } else { format!("{n} layers") }),
                Fate::TypeChanged => format!(
                    "{name} overrides {nodes} whose type this export changes, and Godot ignores the overridden settings the new type doesn't have: {items}. Move those overrides in {name} to the nodes that have the settings now"
                ),
                Fate::Disconnected => {
                    let connections = if n == 1 { "a signal connection".to_owned() } else { format!("{n} signal connections") };
                    format!("Godot drops {connections} of {name}, because a node at either end is renamed, moved or gone and connections are found by path: {items}. Connect {} again in Godot", if n == 1 { "it" } else { "them" })
                }
                Fate::AutoMapPlaced => format!("{name} overrides {nodes} that AutoMap placed: Godot keeps those overrides only while the rules place {} the same way: {items}", them(n)),
            };
            let mut elements: Vec<Element> = findings.iter().filter_map(|f| f.element).collect();
            elements.dedup();
            Note { code: fate.code(), severity: fate.severity(), message, elements }
        })
        .collect()
}

fn them(n: usize) -> &'static str {
    if n == 1 { "it" } else { "them" }
}

fn their(n: usize) -> &'static str {
    if n == 1 { "its" } else { "their" }
}

/// The findings as a list, at most a few: "Objects/Door → Things/Door (object 1), …".
fn list(findings: &[&Finding], fate: Fate) -> String {
    const SHOWN: usize = 5;
    let mut items: Vec<String> = findings
        .iter()
        .take(SHOWN)
        .map(|f| {
            let what = match f.element {
                Some(Element::Layer(id)) => format!(" (layer {id})"),
                Some(Element::Object(id)) => format!(" (object {id})"),
                _ => String::new(),
            };
            if let Some(types) = &f.types {
                let sprite = types
                    .sprite
                    .as_ref()
                    .map_or_else(String::new, |s| format!(" (sprite settings on {s})"));
                return format!("{}{what}: {} → {}{sprite}", f.node, types.was, types.now);
            }
            match (&f.now, fate) {
                (Some(now), Fate::Disconnected) => format!("{}{what}, now {now}", f.node),
                (Some(now), _) if *now != f.node => format!("{} → {now}{what}", f.node),
                _ => format!("{}{what}", f.node),
            }
        })
        .collect();
    if findings.len() > SHOWN {
        items.push(format!("and {} more", findings.len() - SHOWN));
    }
    items.join(", ")
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}
