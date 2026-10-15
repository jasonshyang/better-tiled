//! The corpora: Tiled's sources under `reference/` (examples, test data, resources) and the
//! purchased packs under `assets/` (both fetched, not committed; see `bt-test-data`).

use std::path::{Path, PathBuf};

/// The corpus folders.
pub const DIRS: [&str; 2] = ["reference", "assets"];

/// What a corpus file is.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Map,
    Tileset,
    Template,
    World,
    Project,
}

/// A corpus file.
pub struct CorpusFile {
    /// Its path as shown (relative to the workspace, with forward slashes).
    pub shown: String,
    /// Where it is.
    pub path: PathBuf,
    /// What it is.
    pub kind: Kind,
}

/// Every Tiled file under `dirs` (relative to the workspace), sorted. JSON files count when
/// they say they are a map, tileset or template. A missing folder is a loud skip.
pub fn corpus(dirs: &[&str]) -> Vec<CorpusFile> {
    let mut out = Vec::new();
    for dir in dirs {
        let Some(found) = bt_test_data::test_data(dir) else {
            bt_test_data::missing(dir, "a corpus folder; see 13-testing.md");
            continue;
        };
        let mut paths = Vec::new();
        collect(&found, &mut paths);
        for path in paths {
            let Some(kind) = kind_of(&path) else {
                continue;
            };
            let relative = path
                .strip_prefix(&found)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push(CorpusFile {
                shown: format!("{dir}/{relative}"),
                path,
                kind,
            });
        }
    }
    out
}

fn kind_of(path: &Path) -> Option<Kind> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "tmx" | "tmj" => Some(Kind::Map),
        "tsx" | "tsj" => Some(Kind::Tileset),
        "tx" | "tj" => Some(Kind::Template),
        "world" => Some(Kind::World),
        "tiled-project" => Some(Kind::Project),
        "json" => {
            let text = std::fs::read_to_string(path).ok()?;
            let value: serde_json::Value = serde_json::from_str(&text).ok()?;
            match value.get("type")?.as_str()? {
                "map" => Some(Kind::Map),
                "tileset" => Some(Kind::Tileset),
                "template" => Some(Kind::Template),
                _ => None,
            }
        }
        _ => None,
    }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect(&path, out);
        } else {
            out.push(path);
        }
    }
}
