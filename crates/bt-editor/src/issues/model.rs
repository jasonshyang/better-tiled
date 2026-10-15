//! Typed diagnostics shared by collection workers and the window. Identity excludes display
//! text, so two render/loader reports of the same resource remain one actionable issue.

use crate::{AssetId, ProjectProblem, Severity, TilesetHome};
use bt_core::{LayerId, ObjectId, TileId, TilesetSlot, Violation};
use std::{collections::HashSet, path::PathBuf};

/// The subsystem that reported a diagnostic.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum IssueSource {
    /// Model invariants.
    Map,
    /// Referenced files and format warnings.
    Files,
    /// Rule compilation and execution.
    AutoMap,
    /// The last Godot export or verification.
    Godot,
}

/// A resource read by a map or its dependencies.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ResourceKind {
    /// A tileset file.
    Tileset,
    /// A template file.
    Template,
    /// A raster image or map used as an image.
    Image,
    /// A map placed in a world.
    Map,
    /// The asset's own file or a project configuration file.
    File,
}

/// Stable diagnostic classification, independent of the sentence shown.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum IssueKind {
    /// A file-format warning without a more specific live resource diagnostic.
    Format(bt_tmx::WarningKind),
    /// A referenced resource cannot be used.
    Resource {
        /// Type of resource.
        kind: ResourceKind,
        /// Resolved file.
        path: PathBuf,
    },
    /// A failure to read pixels embedded in a Tiled asset.
    EmbeddedImage(bt_image::ImageKey),
    /// A model invariant violation.
    Map(Violation),
    /// A typed rule compiler or engine problem.
    AutoMap(bt_automap::ProblemKind),
    /// The rules could not be read.
    AutoMapRead,
    /// A project browser scan problem.
    Project(ProjectProblem),
    /// A stable exporter diagnostic code, or its failure category.
    Godot(String),
    /// A diagnostics worker could not run.
    Worker,
}

/// A place an Issues row can navigate to.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum IssueTarget {
    /// A loaded asset's editor or settings.
    Asset(AssetId),
    /// A layer in a map.
    Layer {
        /// Map asset.
        map: AssetId,
        /// Layer identity.
        layer: LayerId,
    },
    /// An object in a map.
    Object {
        /// Map asset.
        map: AssetId,
        /// Object identity.
        object: ObjectId,
    },
    /// A tileset or one of its tiles.
    Tileset {
        /// Owning asset or embedded slot.
        home: TilesetHome,
        /// Specific tile, when any.
        tile: Option<TileId>,
    },
    /// A collision shape of one tileset tile.
    CollisionObject {
        /// Owning tileset.
        home: TilesetHome,
        /// Collision tile.
        tile: TileId,
        /// Shape identity within that tile.
        object: ObjectId,
    },
    /// A rule map or another source file.
    File(PathBuf),
}

/// An exact reference that Locate File can replace, captured before its asynchronous picker.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum LocateTarget {
    /// External tileset reference of a map or template editing view.
    Tileset {
        /// Owner asset.
        owner: AssetId,
        /// Reference slot.
        slot: TilesetSlot,
    },
    /// An atlas image, or a tile's image in a collection.
    TilesetImage {
        /// Owning tileset.
        home: TilesetHome,
        /// Tile for an image collection; none for an atlas.
        tile: Option<TileId>,
    },
    /// Image of an image layer.
    LayerImage {
        /// Map asset.
        map: AssetId,
        /// Image layer identity.
        layer: LayerId,
    },
    /// Template link of a map object.
    Template {
        /// Map asset.
        map: AssetId,
        /// Instance identity.
        object: ObjectId,
    },
    /// Template link of a tile's collision shape.
    CollisionTemplate {
        /// Owning tileset.
        home: TilesetHome,
        /// Collision tile.
        tile: TileId,
        /// Shape identity.
        object: ObjectId,
    },
    /// Explicit world-map reference.
    WorldMap {
        /// World asset.
        world: AssetId,
        /// Entry index.
        index: usize,
    },
}
impl LocateTarget {
    /// The asset whose reference must still be unchanged when the file arrives.
    pub fn asset(self) -> AssetId {
        match self {
            Self::Tileset { owner, .. } => owner,
            Self::TilesetImage { home, .. } | Self::CollisionTemplate { home, .. } => home.asset(),
            Self::LayerImage { map, .. } | Self::Template { map, .. } => map,
            Self::WorldMap { world, .. } => world,
        }
    }
}

/// A problem with its source, severity, navigation targets and optional file repair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    /// Stable category.
    pub kind: IssueKind,
    /// Reporting subsystem.
    pub source: IssueSource,
    /// How serious it is.
    pub severity: Severity,
    /// Human-readable detail.
    pub text: String,
    /// A problem may concern several objects or layers.
    pub targets: Vec<IssueTarget>,
    /// Exact resource reference to repair.
    pub fix: Option<LocateTarget>,
}
impl Issue {
    /// A warning; builders specify severity and source where they differ from its category.
    pub fn new(kind: IssueKind, text: impl Into<String>) -> Self {
        let source = match &kind {
            IssueKind::Map(_) => IssueSource::Map,
            IssueKind::AutoMap(_) | IssueKind::AutoMapRead => IssueSource::AutoMap,
            IssueKind::Godot(_) => IssueSource::Godot,
            _ => IssueSource::Files,
        };
        Self {
            kind,
            source,
            severity: Severity::Warning,
            text: text.into(),
            targets: Vec::new(),
            fix: None,
        }
    }
    /// Sets the severity.
    pub fn severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }
    /// Adds a navigation target, once.
    pub fn target(mut self, target: IssueTarget) -> Self {
        if !self.targets.contains(&target) {
            self.targets.push(target);
        }
        self
    }
    /// Attaches a captured reference for Locate File.
    pub fn locate(mut self, target: LocateTarget) -> Self {
        self.fix = Some(target);
        self
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Identity {
    kind: IssueKind,
    source: IssueSource,
    targets: Vec<IssueTarget>,
    fix: Option<LocateTarget>,
    detail: Option<String>,
}

/// Keeps the first diagnostic for each typed identity; display wording is never interpreted.
pub fn deduplicate_issues(issues: Vec<Issue>) -> Vec<Issue> {
    let mut seen = HashSet::new();
    issues
        .into_iter()
        .filter(|issue| {
            seen.insert(Identity {
                kind: issue.kind.clone(),
                source: issue.source,
                targets: issue.targets.clone(),
                fix: issue.fix,
                detail: (!matches!(
                    issue.kind,
                    IssueKind::Resource { .. } | IssueKind::EmbeddedImage(_)
                ))
                .then(|| issue.text.clone()),
            })
        })
        .collect()
}
