//! One pass over the Godot project an export writes into, before anything is written (docs/design/12-godot-export.md
//! §3.3, §3.5): the uid every file carries, which files an export generated, and the files each
//! scene and resource refers to. It reads only the start of each `.tscn` and `.tres` (the header
//! and the `ext_resource` lines, which Godot writes first), `*.uid` files and the `uid=` line of
//! `*.import` files. It skips `.godot/`, hidden folders and folders holding a `.gdignore`, as
//! Godot does.
//!
//! What a file held is kept by its path, size and modification time, so the next scan (the
//! editor exports on every save) reads only the files that changed since.

use std::collections::{HashMap, HashSet};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::read;

/// The `;` line that starts with this is the export's own (contract.rs writes it).
pub(super) const HEADER_MARK: &str = "better-tiled-export ";

pub(super) struct Scan {
    pub project: PathBuf,
    /// The text resources, and the other files that carry a uid, in path order.
    pub entries: Vec<Entry>,
    /// Every file Godot would see, for [`Scan::mentioning`].
    files: Vec<PathBuf>,
    pub errors: Vec<String>,
}

/// A text resource, or another file with a uid.
#[derive(Clone)]
pub(super) struct Entry {
    /// The file the uid belongs to: the `.tscn` or `.tres`, or the file of a `.uid` or
    /// `.import` file.
    pub path: PathBuf,
    pub uid: Option<u64>,
    /// A `.tscn` or `.tres` that starts with the export's header.
    pub generated: bool,
    /// The `ext_resource`s of a `.tscn` or `.tres`: `(res path, uid)`.
    pub references: Vec<(String, Option<u64>)>,
}

/// Scans the project at `project` (its folder holding `project.godot`).
pub(super) fn scan(project: &Path) -> Scan {
    ScanCache::default().scan(project)
}

/// Caller-owned cache of Godot project resource headers.
#[derive(Default)]
pub struct ScanCache {
    entries: HashMap<PathBuf, Cached>,
}

impl ScanCache {
    /// Scans a project, rereading files whose size or timestamp changed.
    pub(crate) fn scan(&mut self, project: &Path) -> Scan {
        let mut scan = Scan {
            project: project.to_path_buf(),
            entries: Vec::new(),
            files: Vec::new(),
            errors: Vec::new(),
        };
        let mut folders = vec![project.to_path_buf()];
        while let Some(folder) = folders.pop() {
            if folder != project && folder.join(".gdignore").exists() {
                continue;
            }
            let read_dir = match std::fs::read_dir(&folder) {
                Ok(entries) => entries,
                Err(e) => {
                    scan.errors
                        .push(format!("couldn't scan {}: {e}", folder.display()));
                    continue;
                }
            };
            let mut children = Vec::new();
            for child in read_dir {
                match child {
                    Ok(entry) if !entry.file_name().to_string_lossy().starts_with('.') => {
                        match entry.file_type() {
                            Ok(kind) => children.push((entry.path(), kind.is_dir())),
                            Err(e) => scan
                                .errors
                                .push(format!("couldn't inspect {}: {e}", entry.path().display())),
                        }
                    }
                    Ok(_) => {}
                    Err(e) => scan
                        .errors
                        .push(format!("couldn't scan {}: {e}", folder.display())),
                }
            }
            children.sort();
            for (path, is_dir) in children.into_iter().rev() {
                if is_dir {
                    folders.push(path);
                } else {
                    scan.files.push(path);
                }
            }
        }
        scan.files.sort();
        let cache = &mut self.entries;
        let mut seen = HashSet::new();
        for file in &scan.files {
            if file
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e, "tscn" | "tres" | "uid" | "import"))
                && let Err(e) = std::fs::File::open(file)
            {
                scan.errors
                    .push(format!("couldn't read {}: {e}", file.display()));
                continue;
            }
            if let Some(entry) = cached_entry(cache, file) {
                scan.entries.push(entry);
            }
            seen.insert(file.as_path());
        }
        // Files gone since (or moved) are forgotten.
        cache.retain(|path, _| !path.starts_with(project) || seen.contains(path.as_path()));
        scan
    }
}

/// What a file held when it was last read, and the size and time it had then.
struct Cached {
    len: u64,
    modified: SystemTime,
    entry: Option<Entry>,
}

/// A file changed this recently could change again within its time's resolution (a second on
/// some file systems) without its time changing, so it isn't kept: it is read again next time.
const SETTLED: Duration = Duration::from_secs(2);

/// [`entry`], from the cache while the file's size and time are as they were.
fn cached_entry(cache: &mut HashMap<PathBuf, Cached>, file: &Path) -> Option<Entry> {
    if !file
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e, "tscn" | "tres" | "uid" | "import"))
    {
        return None;
    }
    let Ok(meta) = std::fs::metadata(file) else {
        return entry(file);
    };
    let (len, modified) = (
        meta.len(),
        meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
    );
    if let Some(cached) = cache
        .get(file)
        .filter(|c| c.len == len && c.modified == modified)
    {
        return cached.entry.clone();
    }
    let found = entry(file);
    let settled = SystemTime::now()
        .duration_since(modified)
        .is_ok_and(|age| age >= SETTLED);
    if settled {
        cache.insert(
            file.to_path_buf(),
            Cached {
                len,
                modified,
                entry: found.clone(),
            },
        );
    } else {
        cache.remove(file);
    }
    found
}

/// What a text resource holds, or the uid a `.uid` or `.import` file gives its file.
fn entry(file: &Path) -> Option<Entry> {
    let extension = file.extension()?.to_str()?;
    let owner = || file.with_extension("");
    match extension {
        "tscn" | "tres" => {
            let text = resource_start(file)?;
            let resource = read::parse(&text).ok()?;
            let uid = resource.uid().as_deref().and_then(read::text_to_uid);
            let generated = resource
                .comments
                .iter()
                .any(|c| c.trim_start().starts_with(HEADER_MARK));
            let references = resource
                .ext_resources()
                .map(|e| (e.path, e.uid.as_deref().and_then(read::text_to_uid)))
                .collect();
            Some(Entry {
                path: file.to_path_buf(),
                uid,
                generated,
                references,
            })
        }
        "uid" => {
            let text = std::fs::read_to_string(file).ok()?;
            let uid = read::text_to_uid(text.trim())?;
            Some(Entry {
                path: owner(),
                uid: Some(uid),
                generated: false,
                references: Vec::new(),
            })
        }
        "import" => {
            let text = std::fs::read_to_string(file).ok()?;
            let raw = text.lines().find_map(|l| l.strip_prefix("uid="))?;
            let uid = read::text_to_uid(&read::string(raw.trim())?)?;
            Some(Entry {
                path: owner(),
                uid: Some(uid),
                generated: false,
                references: Vec::new(),
            })
        }
        _ => None,
    }
}

/// The start of a text resource: its comments, header and `ext_resource` lines, up to the first
/// other section (a scene's tile data can run to megabytes, and is never needed here).
fn resource_start(file: &Path) -> Option<String> {
    let mut reader = std::io::BufReader::new(std::fs::File::open(file).ok()?);
    let mut text = String::new();
    let mut header_seen = false;
    loop {
        let start = text.len();
        if reader.read_line(&mut text).ok()? == 0 {
            break;
        }
        let line = text
            .get(start..)
            .unwrap_or_default()
            .trim_start_matches('\u{feff}');
        if line.starts_with('[') {
            let ext = line.starts_with("[ext_resource");
            if header_seen && !ext {
                text.truncate(start);
                break;
            }
            header_seen = true;
        }
    }
    Some(text)
}

impl Scan {
    /// The files carrying `uid`, other than `except`.
    pub fn carrying(&self, uid: u64, except: &Path) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(move |e| e.uid == Some(uid) && e.path != except)
    }

    /// The scenes and resources that refer to the file at the `res://` path `res`, or to `uid`.
    pub fn referencing(&self, res: &str, uid: Option<u64>) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(move |e| {
            e.references
                .iter()
                .any(|(path, u)| path == res || (uid.is_some() && *u == uid))
        })
    }

    /// For each of the `res://` paths in `wanted`, the first file that mentions it, leaving out the
    /// files in `except` and backups (`.bak`, which Godot doesn't import). Reads every file, so
    /// it is only for the rare export that drops an image copy.
    pub fn mentioning(
        &self,
        wanted: &[String],
        except: &[PathBuf],
    ) -> Result<HashMap<String, PathBuf>, String> {
        let mut found = HashMap::new();
        if wanted.is_empty() {
            return Ok(found);
        }
        for file in &self.files {
            if except.contains(file) || file.extension().is_some_and(|e| e == "bak") {
                continue;
            }
            let bytes = std::fs::read(file).map_err(|e| {
                format!("couldn't check image references in {}: {e}", file.display())
            })?;
            let text = String::from_utf8_lossy(&bytes);
            for res in wanted {
                if !found.contains_key(res) && text.contains(res.as_str()) {
                    found.insert(res.clone(), file.clone());
                }
            }
            if found.len() == wanted.len() {
                break;
            }
        }
        Ok(found)
    }
}
