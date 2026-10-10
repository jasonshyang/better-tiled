//! Recovery. While a map or a tileset file has unsaved changes, a snapshot of what Save would
//! write is kept in `<data>/recovery/<pid>-<asset>.snapshot`, described by a `.meta.json` beside
//! it: two seconds after its last change, or every thirty seconds while it keeps changing. A
//! saved or closed asset's snapshot is deleted. Each run touches `<pid>.alive` in the same folder
//! every few seconds; at start, snapshots of runs whose heartbeat stopped are offered back (a
//! tileset's snapshot is told apart by its file's extension).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bt_utils::atomic_write::{WriteMode, write_file};
use serde::{Deserialize, Serialize};

use crate::store::{AssetId, AssetKind, AssetStore};

/// Quiet time after the last change before a snapshot.
const QUIET_MS: u64 = 2_000;
/// The longest a changing map goes without a snapshot.
const OVERDUE_MS: u64 = 30_000;
/// How long a heartbeat counts as a live run.
const HEARTBEAT_LIFE: Duration = Duration::from_secs(60);

/// Which unsaved maps and tilesets need a snapshot, and when they last changed.
#[derive(Clone, Debug)]
pub(crate) struct RecoveryPlan {
    folder: PathBuf,
    pid: u32,
    tracked: HashMap<AssetId, Tracked>,
}

/// What the plan knows of an unsaved asset.
#[derive(Copy, Clone, Debug)]
struct Tracked {
    seen_version: u64,
    changed_at_ms: u64,
    snapshot_version: Option<u64>,
    snapshot_at_ms: Option<u64>,
    since_ms: u64,
}

/// An unsaved asset and its version.
struct UnsavedAsset {
    id: AssetId,
    version: u64,
}

/// What the schedule asks for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Due {
    /// Assets to snapshot now.
    pub(crate) snapshot: Vec<AssetId>,
    /// Assets no longer unsaved (or gone) whose snapshots go.
    pub(crate) delete: Vec<AssetId>,
}

impl RecoveryPlan {
    pub(crate) fn new(folder: PathBuf, pid: u32) -> RecoveryPlan {
        RecoveryPlan {
            folder,
            pid,
            tracked: HashMap::new(),
        }
    }

    /// The snapshots due at `now_ms`.
    pub(crate) fn due(&mut self, store: &AssetStore, now_ms: u64) -> Due {
        let mut due = Due::default();
        let unsaved = unsaved_assets(store);
        let gone: Vec<AssetId> = self
            .tracked
            .keys()
            .filter(|id| !unsaved.iter().any(|asset| asset.id == **id))
            .copied()
            .collect();
        for id in gone {
            self.tracked.remove(&id);
            due.delete.push(id);
        }
        for UnsavedAsset { id, version } in unsaved {
            let entry = self.tracked.entry(id).or_insert(Tracked {
                seen_version: version,
                changed_at_ms: now_ms,
                snapshot_version: None,
                snapshot_at_ms: None,
                since_ms: now_ms,
            });
            if entry.seen_version != version {
                entry.seen_version = version;
                entry.changed_at_ms = now_ms;
            }
            if entry.snapshot_version == Some(version) {
                continue;
            }
            let quiet = now_ms.saturating_sub(entry.changed_at_ms) >= QUIET_MS;
            let last = entry.snapshot_at_ms.unwrap_or(entry.since_ms);
            let overdue = now_ms.saturating_sub(last) >= OVERDUE_MS;
            if quiet || overdue {
                entry.snapshot_version = Some(version);
                entry.snapshot_at_ms = Some(now_ms);
                due.snapshot.push(id);
            }
        }
        due
    }

    /// Every unsaved asset whose latest version has no snapshot, now counted as snapshotted
    /// (the run is ending and writes them at once).
    pub(crate) fn take_unsnapshotted(&mut self, store: &AssetStore) -> Vec<AssetId> {
        let mut ids = Vec::new();
        for UnsavedAsset { id, version } in unsaved_assets(store) {
            let entry = self.tracked.entry(id).or_insert(Tracked {
                seen_version: version,
                changed_at_ms: 0,
                snapshot_version: None,
                snapshot_at_ms: None,
                since_ms: 0,
            });
            entry.seen_version = version;
            if entry.snapshot_version != Some(version) {
                entry.snapshot_version = Some(version);
                ids.push(id);
            }
        }
        ids
    }

    /// Stops tracking an asset (saved or closed).
    pub(crate) fn forget(&mut self, asset: AssetId) {
        self.tracked.remove(&asset);
    }

    /// The assets with snapshots.
    pub(crate) fn tracked(&self) -> Vec<AssetId> {
        self.tracked.keys().copied().collect()
    }

    /// Where an asset's snapshot goes.
    pub(crate) fn snapshot_path(&self, asset: AssetId) -> PathBuf {
        self.folder
            .join(format!("{}-{}.snapshot", self.pid, asset.0))
    }

    /// This run's heartbeat file.
    pub(crate) fn heartbeat_path(&self) -> PathBuf {
        heartbeat_path(&self.folder, self.pid)
    }

    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }
}

/// The unsaved maps of the store, and its unsaved tilesets that have files, with their
/// versions.
fn unsaved_assets(store: &AssetStore) -> Vec<UnsavedAsset> {
    store
        .assets()
        .filter(|asset| {
            let snapshotted = match asset.kind() {
                AssetKind::Map => true,
                AssetKind::Tileset => asset.path().is_some(),
                AssetKind::Project => false,
            };
            snapshotted && asset.is_dirty()
        })
        .map(|asset| UnsavedAsset {
            id: asset.id(),
            version: asset.version(),
        })
        .collect()
}

/// A run's heartbeat file.
fn heartbeat_path(folder: &Path, pid: u32) -> PathBuf {
    folder.join(format!("{pid}.alive"))
}

/// What a snapshot is of.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotMeta {
    /// The file the map was saved to, if any.
    pub original: Option<PathBuf>,
    /// The run that wrote it.
    pub pid: u32,
}

/// A snapshot to write: the text Save would write, and what it is of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// Where it goes.
    pub path: PathBuf,
    /// The map's text.
    pub text: String,
    /// What it is of.
    pub meta: SnapshotMeta,
}

impl Snapshot {
    /// Writes the snapshot and its description (on a worker).
    pub fn write(&self) -> Result<(), std::io::Error> {
        if let Some(folder) = self.path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let meta = serde_json::to_string_pretty(&self.meta).map_err(std::io::Error::other)?;
        write_file(&self.path, self.text.as_bytes(), WriteMode::Atomic)
            .map_err(std::io::Error::other)?;
        write_file(&meta_path(&self.path), meta.as_bytes(), WriteMode::Atomic)
            .map_err(std::io::Error::other)
    }
}

/// Deletes a snapshot and its description; one that is not there is no error.
pub fn delete_snapshot(path: &Path) -> Result<(), std::io::Error> {
    for file in [path.to_path_buf(), meta_path(path)] {
        match std::fs::remove_file(&file) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn meta_path(snapshot: &Path) -> PathBuf {
    let mut name = snapshot.as_os_str().to_owned();
    name.push(".meta.json");
    PathBuf::from(name)
}

/// Touches a run's heartbeat (every few seconds while it runs).
pub fn touch_heartbeat(folder: &Path, pid: u32) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(folder)?;
    std::fs::write(heartbeat_path(folder, pid), pid.to_string())
}

/// Removes a run's heartbeat (when it ends cleanly).
pub fn remove_heartbeat(folder: &Path, pid: u32) -> Result<(), std::io::Error> {
    match std::fs::remove_file(heartbeat_path(folder, pid)) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// A snapshot whose run stopped without saving or deleting it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Orphan {
    /// The snapshot file.
    pub snapshot: PathBuf,
    /// What it is of.
    pub meta: SnapshotMeta,
}

impl Orphan {
    /// The name shown for it.
    pub fn name(&self) -> String {
        self.meta.original.as_deref().map_or_else(
            || "an untitled map".to_owned(),
            bt_utils::paths::display_name,
        )
    }

    /// Where the recovered map is considered to be (its relative references resolve there).
    pub fn original_path(&self) -> PathBuf {
        self.meta.original.clone().unwrap_or_else(|| {
            self.snapshot.parent().map_or_else(
                || PathBuf::from("untitled.tmx"),
                |folder| folder.join("untitled.tmx"),
            )
        })
    }
}

/// The snapshots in `folder` whose runs are gone: not this run, and without a recent heartbeat.
pub fn orphans(folder: &Path, own_pid: u32, now: SystemTime) -> Vec<Orphan> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut found: Vec<Orphan> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "snapshot"))
        .filter_map(|snapshot| {
            let meta: SnapshotMeta =
                serde_json::from_str(&std::fs::read_to_string(meta_path(&snapshot)).ok()?).ok()?;
            Some(Orphan { snapshot, meta })
        })
        .filter(|orphan| orphan.meta.pid != own_pid && !is_alive(folder, orphan.meta.pid, now))
        .collect();
    found.sort_by(|a, b| a.snapshot.cmp(&b.snapshot));
    found
}

/// Whether a run's heartbeat is recent.
fn is_alive(folder: &Path, pid: u32, now: SystemTime) -> bool {
    std::fs::metadata(heartbeat_path(folder, pid))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|modified| now.duration_since(modified).ok())
        .is_some_and(|age| age < HEARTBEAT_LIFE)
}
