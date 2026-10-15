//! An asset: a loaded map, tileset or project, with where it lives and whether it is saved.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_core::{Map, ObjectTemplate, Project, Tileset, World};
use bt_tmx::Warning;

/// An asset's identity for the life of the process; never reused.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AssetId(pub u64);

impl fmt::Display for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "asset {}", self.0)
    }
}

/// What an asset holds: replaced, never changed in place.
#[derive(Clone, Debug, PartialEq)]
pub enum Root {
    /// A world file.
    World(Arc<World>),
    /// An object template file.
    Template(Arc<ObjectTemplate>),
    /// A map.
    Map(Arc<Map>),
    /// A tileset in a file of its own.
    Tileset(Arc<Tileset>),
    /// A project.
    Project(Arc<Project>),
}

impl Root {
    /// A serialization request for this root; callers provide any owning loader.
    pub(crate) fn write_request(&self, path: &Path) -> bt_tmx::WriteRequest<'_> {
        match self {
            Root::Map(map) => bt_tmx::WriteRequest::for_map(map, path),
            Root::Tileset(tileset) => bt_tmx::WriteRequest::for_tileset(tileset, path),
            Root::Template(template) => bt_tmx::WriteRequest::for_template(template, path),
            Root::World(world) => bt_tmx::WriteRequest::for_world(world, path),
            Root::Project(project) => bt_tmx::WriteRequest::for_project(project, path),
        }
    }
    /// The kind of asset.
    pub fn kind(&self) -> AssetKind {
        match self {
            Root::World(_) => AssetKind::World,
            Root::Template(_) => AssetKind::Template,
            Root::Map(_) => AssetKind::Map,
            Root::Tileset(_) => AssetKind::Tileset,
            Root::Project(_) => AssetKind::Project,
        }
    }

    /// Whether two roots are the same value, cheaply when they are the same `Arc`.
    pub(crate) fn same_as(&self, other: &Root) -> bool {
        match self {
            Root::World(a) => matches!(other, Root::World(b) if Arc::ptr_eq(a, b) || a == b),
            Root::Template(a) => matches!(other, Root::Template(b) if Arc::ptr_eq(a, b) || a == b),
            Root::Map(a) => matches!(other, Root::Map(b) if Arc::ptr_eq(a, b) || a == b),
            Root::Tileset(a) => matches!(other, Root::Tileset(b) if Arc::ptr_eq(a, b) || a == b),
            Root::Project(a) => matches!(other, Root::Project(b) if Arc::ptr_eq(a, b) || a == b),
        }
    }
}

/// The kinds of assets.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum AssetKind {
    /// A world.
    World,
    /// An object template.
    Template,
    /// A map.
    Map,
    /// A tileset.
    Tileset,
    /// A project.
    Project,
}

impl fmt::Display for AssetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            AssetKind::World => "world",
            AssetKind::Template => "template",
            AssetKind::Map => "map",
            AssetKind::Tileset => "tileset",
            AssetKind::Project => "project",
        })
    }
}

/// What is on disk for an asset: the version that was saved or loaded, and the file's content.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OnDisk {
    /// The asset version the file holds.
    pub version: u64,
    /// The FNV-1a hash of the file's bytes, which tells our own writes from others'.
    pub content_hash: u64,
}

/// A loaded asset.
#[derive(Clone, Debug)]
pub struct Asset {
    pub(crate) id: AssetId,
    pub(crate) path: Option<PathBuf>,
    pub(crate) root: Root,
    pub(crate) version: u64,
    /// The highest version the asset has had, so a new change never reuses an undone one's.
    pub(crate) high_water: u64,
    pub(crate) on_disk: Option<OnDisk>,
    pub(crate) missing: bool,
    pub(crate) warnings: Vec<Warning>,
}

impl Asset {
    /// Its identity.
    pub fn id(&self) -> AssetId {
        self.id
    }

    /// Its file; `None` for a new asset never saved.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// What it holds.
    pub fn root(&self) -> &Root {
        &self.root
    }

    /// Bumped by every change of the root.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// What is on disk, when it is known.
    pub fn on_disk(&self) -> Option<OnDisk> {
        self.on_disk
    }

    /// Whether its file was deleted or moved away (the asset is then the only copy).
    pub fn is_missing(&self) -> bool {
        self.missing
    }

    /// The warnings of its load.
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    /// The kind of asset.
    pub fn kind(&self) -> AssetKind {
        self.root.kind()
    }

    /// Whether it has changes the file lacks: never saved, changed since, or its file gone.
    pub fn is_dirty(&self) -> bool {
        self.missing || self.on_disk.is_none_or(|disk| disk.version != self.version)
    }

    /// The name shown for it: its file's name, or “untitled” and its kind.
    pub fn display_name(&self) -> String {
        match &self.path {
            Some(path) => bt_utils::paths::display_name(path),
            None => format!("untitled {}", self.kind()),
        }
    }
}
