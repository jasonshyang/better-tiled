//! The regeneration contract (docs/design/12-godot-export.md): an export replaces only a file it can show it wrote
//! and nobody changed since, and otherwise refuses, writing nothing at all.
//!
//! - **The header.** Every generated scene and TileSet starts with three `;` lines (Godot loads
//!   them, and drops them when it saves the file again). The last holds an FNV-1a 64 hash of
//!   everything from the `[gd_scene` (or `[gd_resource`) line on, so a file still as written is
//!   told from one changed since. The root (a scene's root node, a TileSet's `[resource]`) also
//!   gets `metadata/bt_source`, naming the map: it survives Godot's re-save.
//! - **The uid.** A file keeps the uid it has; a new one gets a uid derived from its `res://`
//!   path, so exporting again after deleting it gives the same one. Another file carrying that
//!   uid is refused: this export moved in Godot, a copy of it made outside Godot, or a file Godot
//!   would mix it up with.
//! - **The order.** Every output is built in memory, the project scanned once and every output
//!   classified ([`Found`]); only when nothing is refused is anything written, each file
//!   atomically: image copies, TileSets, the scene, then `images/.sources.json`. Files already
//!   as the export would write them are left alone.
//! - **Image copies** (images.rs) are replaced while they are as they were copied. One changed in
//!   Godot is kept, and refused once its image changed too.
//! - **Replace** (`--replace`) writes over a refused file after keeping it beside it as
//!   `<name>.bak` (`<name>.2.bak`, … never over an earlier backup), with its uid taken out so the
//!   backup can't become a second file with that uid.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use super::Options;
use super::images::{hex, not_copied};
use super::plan::{Kind, Output, Plan};
use super::project_scan::{HEADER_MARK, Scan};
use super::read::{self, Resource, fnv1a64, uid_to_text};
use super::report::{File, Note, Report, State};
use super::{res_path, text};
use bt_utils::paths::{canonical_or_clean as normalized, reference_path as relative_path};

/// The version of the header's format, written after [`HEADER_MARK`].
const HEADER_VERSION: &str = "1";
/// The root's metadata naming the map a file was generated from.
pub(super) const MARKER: &str = "metadata/bt_source";

/// What an export found where it would write a file (docs/design/12-godot-export.md).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Found {
    /// Nothing yet.
    New,
    /// The very bytes the export would write: left alone.
    UpToDate,
    /// The export's own file, unchanged since it was written: replaced, keeping its uid.
    Ours,
    /// The export of another map (the map its marker names), unchanged since.
    OtherMap(String),
    /// The export's file, changed since (by hand or by a merge).
    Edited,
    /// No header but the marker: Godot saved the export again.
    ResavedByGodot,
    /// Written by Tiled's Godot exporter.
    TiledExport,
    /// No header: an earlier better-tiled, Tiled or a person wrote it.
    NoHeader,
    /// An image copy changed in Godot whose image didn't change: kept as it is.
    CopyChanged,
    /// An image copy changed in Godot whose image changed too.
    CopyConflict,
    /// The uid the export would give a new file is that of this file elsewhere, its own export
    /// moved in Godot.
    Moved(PathBuf),
    /// The uid the export would give a new file is another file's.
    UidInUse(PathBuf),
    /// Another file carries the uid this file has: a copy of it made outside Godot (Godot's
    /// Duplicate gives a copy a uid of its own). The copy is what needs another uid.
    Duplicate(PathBuf),
    /// A level scene: the user's own scene, inheriting the generated scene at this `res://` path.
    LevelScene(String),
    /// A shared TileSet that isn't there, used by these other generated scenes: a new one would
    /// number its sources afresh, under their cells.
    Missing(Vec<PathBuf>),
}

impl Found {
    /// Not written without Replace.
    pub fn refused(&self) -> bool {
        !matches!(
            self,
            Found::New | Found::UpToDate | Found::Ours | Found::CopyChanged
        )
    }

    /// Refused even with Replace: the way out is another uid, exporting where the scene went, the
    /// TileSet back, the copy fixed, or another scene than the user's level scene.
    fn beyond_replace(&self) -> bool {
        matches!(
            self,
            Found::Moved(_)
                | Found::UidInUse(_)
                | Found::Duplicate(_)
                | Found::Missing(_)
                | Found::LevelScene(_)
        )
    }

    /// Whether Replace can resolve this refusal while keeping a backup.
    pub fn can_replace(&self) -> bool {
        self.refused() && !self.beyond_replace()
    }

    /// Changed in Godot (or by hand) since the export wrote it.
    fn changed(&self) -> bool {
        matches!(
            self,
            Found::Edited | Found::ResavedByGodot | Found::CopyConflict
        )
    }
}

/// A file the export won't replace, and why.
#[derive(Debug, serde::Serialize)]
pub struct Refusal {
    /// The path of the file on disk.
    pub path: PathBuf,
    /// Found.
    pub found: Found,
    /// What is wrong and what exporting would undo, for people.
    pub reason: String,
    /// What the file has that the export lacks: "a script on 'desert'", "2 signal
    /// connections", …
    pub extras: Vec<String>,
}

impl Refusal {
    /// What to do about it, on the command line.
    pub fn way_out(&self) -> String {
        way_out(&self.path, &self.found)
    }
}

/// The scenes among `users` that aren't the export's: the user's own, whose cells only the
/// TileSet they were painted with keeps right. One the export wrote names its map, by its header
/// or, once Godot has saved it again (dropping the header), by its marker.
pub fn hand_made(users: &[PathBuf]) -> Vec<&PathBuf> {
    users.iter().filter(|p| !exported(p)).collect()
}

/// Whether the scene at `path` is one the export wrote: it has the header, or the marker.
pub(super) fn exported(path: &Path) -> bool {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let header = read::parse_header(&text).is_ok_and(|r| {
        r.comments
            .iter()
            .any(|c| c.trim_start().starts_with(HEADER_MARK))
    });
    header
        || read::parse(&text)
            .ok()
            .as_ref()
            .and_then(marker_of)
            .is_some()
}

/// What to do, on the command line, about the file at `path` that an export refuses as `found`
/// (nothing for a file it writes).
pub fn way_out(path: &Path, found: &Found) -> String {
    let tileset = path.extension().is_some_and(|e| e == "tres");
    let what = if tileset { "TileSet" } else { "scene" };
    match found {
        Found::New | Found::UpToDate | Found::Ours | Found::CopyChanged => String::new(),
        Found::Moved(to) if tileset => format!(
            "move it back, or make {} the map's tilesetResPath",
            shown(to)
        ),
        Found::Moved(to) => format!(
            "export to {} from now on, or add --new-uid to start a new scene here",
            shown(to)
        ),
        Found::Missing(users) => match hand_made(users).as_slice() {
            [] => "restore it, or export every map that uses it again with bt godot export --all"
                .to_owned(),
            own => {
                let names: Vec<String> = own.iter().map(|p| file_name(p)).collect();
                let paint = if own.len() == 1 { "paints" } else { "paint" };
                format!(
                    "restore it: {} {paint} with it, and a TileSet made afresh would number its tiles anew",
                    list(&names)
                )
            }
        },
        Found::UidInUse(_) => format!("add --new-uid to give this {what} a uid of its own"),
        Found::Duplicate(copy) => format!(
            "if {} is a copy of {} made outside Godot, delete it, or make it again with Godot's Duplicate…, which gives a copy a uid of its own",
            shown(copy),
            file_name(path)
        ),
        Found::LevelScene(_) => "it is yours: export this map to another scene".to_owned(),
        Found::OtherMap(_) => format!(
            "export this map to another {what}, or add --replace to make this one its export anyway, keeping it as a .bak first"
        ),
        Found::Edited
        | Found::ResavedByGodot
        | Found::TiledExport
        | Found::NoHeader
        | Found::CopyConflict => {
            "add --replace to replace it anyway, keeping it as a .bak first".to_owned()
        }
    }
}

/// A file's `res://` path in the Godot project it is in, else its path.
fn shown(path: &Path) -> String {
    super::project_root(path)
        .ok()
        .and_then(|project| res_path(&project, path))
        .unwrap_or_else(|| path.display().to_string())
}

/// The file and its classification before an export.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct FileStatus {
    /// The output file on disk.
    pub path: PathBuf,
    /// What the exporter found there.
    pub found: Found,
}

/// What the export would do to each file, without doing it (`godot::status`).
#[derive(Debug, serde::Serialize)]
pub struct Status {
    /// Each file the export writes, with what is there now.
    pub files: Vec<FileStatus>,
    /// Image copies the export would remove.
    pub pruned: Vec<PathBuf>,
    /// The scenes that inherit or instance the export, and what Godot will do with them.
    pub level_scenes: Vec<super::inheritors::LevelScene>,
}

/// The state of a map's export, as a word.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Summary {
    /// Neverexported.
    NeverExported,
    /// Every output already matches the map.
    UpToDate,
    /// At least one generated output would change.
    OutOfDate,
    /// A file the export would replace changed since it wrote it: exporting refuses.
    Changed,
    /// Exporting refuses for another reason: a file someone else's, a shared TileSet missing or
    /// moved, a uid another file's.
    Refused,
    /// The scene moved in Godot, to this file.
    Moved(PathBuf),
}

impl Status {
    /// Summary.
    pub fn summary(&self) -> Summary {
        let scene = |p: &Path| p.extension().is_some_and(|e| e == "tscn");
        if let Some(to) = self.files.iter().find_map(|FileStatus { path, found }| {
            if let Found::Moved(to) = found {
                scene(path).then(|| to.clone())
            } else {
                None
            }
        }) {
            return Summary::Moved(to);
        }
        let refused: Vec<&Found> = self
            .files
            .iter()
            .map(|file| &file.found)
            .filter(|f| f.refused())
            .collect();
        if !refused.is_empty() {
            return if refused.iter().all(|f| f.changed()) {
                Summary::Changed
            } else {
                Summary::Refused
            };
        }
        let scene_new = self.files.iter().any(|file| {
            file.found == Found::New && file.path.extension().is_some_and(|e| e == "tscn")
        });
        if scene_new {
            return Summary::NeverExported;
        }
        if self.pruned.is_empty()
            && self
                .files
                .iter()
                .all(|file| matches!(file.found, Found::UpToDate | Found::CopyChanged))
        {
            Summary::UpToDate
        } else {
            Summary::OutOfDate
        }
    }
}

/// What `metadata/bt_source` and the header say a generated `file` was made from: `source`
/// relative to the file's folder.
pub(super) fn marker(file: &Path, source: &Path) -> String {
    relative_path(file.parent().unwrap_or(Path::new("")), source)
}

/// The decision for one planned output.
struct Decision<'p> {
    output: &'p Output,
    found: Found,
    /// What to write; `None` leaves the file as it is.
    bytes: Option<Cow<'p, [u8]>>,
    /// The file as it is, kept here before it is replaced.
    backup: Option<(PathBuf, Vec<u8>)>,
    reason: String,
    extras: Vec<String>,
}

/// Classifies every output of `plan` and, unless one is refused, writes them (or with
/// `options.check`, only says what it would write).
pub(super) fn export(
    plan: &Plan,
    scan: &Scan,
    options: &Options,
    report: &mut Report,
) -> Result<(), super::Error> {
    let decisions = decide(plan, scan, options)?;
    let refusals: Vec<Refusal> = decisions
        .iter()
        .filter(|d| d.found.refused() && (d.found.beyond_replace() || !options.replace))
        .map(|d| Refusal {
            path: d.output.path.clone(),
            found: d.found.clone(),
            reason: d.reason.clone(),
            extras: d.extras.clone(),
        })
        .collect();
    if !refusals.is_empty() {
        return Err(super::Error::Refused(refusals));
    }
    // A file up to date whose copy carries its uid: said, though nothing is written.
    for d in decisions
        .iter()
        .filter(|d| d.found == Found::UpToDate && !d.reason.is_empty())
    {
        report.notes.push(Note::warning(
            crate::Code::UidDuplicated,
            format!("{}: {}", d.output.path.display(), d.reason),
            Vec::new(),
        ));
    }
    report.checked = options.check;
    write(&decisions, &plan.prune, options.check, report)?;
    Ok(())
}

/// What the export would find and do, writing nothing.
pub(super) fn status(plan: &Plan, scan: &Scan) -> Result<Status, String> {
    let decisions = decide(plan, scan, &Options::default())?;
    Ok(Status {
        files: decisions
            .into_iter()
            .map(|d| FileStatus {
                path: d.output.path.clone(),
                found: d.found,
            })
            .collect(),
        pruned: plan.prune.clone(),
        level_scenes: Vec::new(),
    })
}

fn decide<'p>(plan: &'p Plan, scan: &Scan, options: &Options) -> Result<Vec<Decision<'p>>, String> {
    plan.outputs
        .iter()
        .map(|output| match output.kind {
            Kind::Scene | Kind::TileSet => generated(output, scan, options),
            Kind::Image => copy(output, options),
            Kind::Manifest => Ok(plain(output, existing(&output.path)?)),
        })
        .collect()
}

/// The bytes at `path`, if there is a file.
fn existing(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("couldn't read {}: {e}", path.display())),
    }
}

/// A file the export always owns: written when it differs.
fn plain(output: &Output, existing: Option<Vec<u8>>) -> Decision<'_> {
    let same = existing.as_deref() == Some(&output.bytes[..]);
    let found = if same {
        Found::UpToDate
    } else if existing.is_some() {
        Found::Ours
    } else {
        Found::New
    };
    Decision {
        output,
        found,
        bytes: (!same).then_some(Cow::Borrowed(&output.bytes[..])),
        backup: None,
        reason: String::new(),
        extras: Vec::new(),
    }
}

/// A file that exists but can't be written fails the export, as a save does.
fn writable(path: &Path) -> Result<(), String> {
    match std::fs::OpenOptions::new().write(true).open(path) {
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            Err(format!("{}: the file is read-only", path.display()))
        }
        _ => Ok(()),
    }
}

/// An image copy: replaced while it is as the export copied it; kept when it was changed in
/// Godot and its image wasn't; refused when both changed.
fn copy<'p>(output: &'p Output, options: &Options) -> Result<Decision<'p>, String> {
    let Some(on_disk) = existing(&output.path)? else {
        return Ok(plain(output, None));
    };
    let mut decision = plain(output, Some(on_disk.clone()));
    let Some(recorded) = output.recorded.filter(|_| decision.found == Found::Ours) else {
        return Ok(decision);
    };
    if fnv1a64(&on_disk) == recorded {
        return Ok(decision);
    }
    if fnv1a64(&output.bytes) == recorded {
        decision.found = Found::CopyChanged;
        decision.bytes = None;
        return Ok(decision);
    }
    decision.found = Found::CopyConflict;
    let name = file_name(&output.path);
    let source = output
        .source
        .as_deref()
        .map_or_else(String::new, |s| format!(" ({})", s.display()));
    decision.reason = format!(
        "the image copy {name} was changed in Godot, and its image{source} changed too: copying it again would undo the Godot changes"
    );
    if options.replace {
        decision.backup = Some((backup_path(&output.path), on_disk));
    }
    Ok(decision)
}

/// A generated scene or TileSet: its uid decided, its header added, and what is there now
/// classified.
fn generated<'p>(
    output: &'p Output,
    scan: &Scan,
    options: &Options,
) -> Result<Decision<'p>, String> {
    let path = &output.path;
    let body = String::from_utf8_lossy(&output.bytes);
    let from = output
        .from
        .clone()
        .or_else(|| output.source.as_deref().map(|s| marker(path, s)))
        .unwrap_or_default();
    let on_disk = existing(path)?;
    let text = on_disk.as_deref().map(String::from_utf8_lossy);
    let kept = text
        .as_deref()
        .and_then(|t| read::parse_header(t).ok())
        .and_then(|head| head.uid());
    let clash = clash(path, kept.as_deref(), on_disk.is_some(), &from, scan);
    // --new-uid gives a new file a uid of its own where the one it would get is another file's:
    // a scene that moved (a new one starts here), or a file whose uid an unrelated file has.
    // Never a TileSet that moved (the scenes using it would be split between two), nor a file
    // whose copy has its uid (the copy is what needs another).
    let around = options.new_uid
        && clash.as_ref().is_some_and(|(found, _)| match found {
            Found::Moved(_) => output.kind == Kind::Scene,
            Found::UidInUse(_) => true,
            _ => false,
        });
    let uid = uid(output.kind, path, kept, scan, around);
    let body = with_uid(&body, &uid);
    let bytes = format!(
        "{}{body}",
        header(output.kind, &from, fnv1a64(body.as_bytes()))
    )
    .into_bytes();
    let mut decision = Decision {
        output,
        found: Found::New,
        bytes: None,
        backup: None,
        reason: String::new(),
        extras: Vec::new(),
    };
    if on_disk.as_deref() == Some(&bytes[..]) {
        decision.found = Found::UpToDate;
        // Nothing to write, but a copy carrying its uid is said (export() makes it a warning).
        if let Some((Found::Duplicate(_), reason)) = clash {
            decision.reason = reason;
        }
        return Ok(decision);
    }
    let name = file_name(path);
    if let Some((found, reason)) = clash.filter(|_| !around) {
        (decision.found, decision.reason) = (found, reason);
        return Ok(decision);
    }
    if on_disk.is_none() && !output.users.is_empty() {
        let users: Vec<String> = output
            .users
            .iter()
            .map(|p| res_path(&scan.project, p).unwrap_or_else(|| p.display().to_string()))
            .collect();
        decision.found = Found::Missing(output.users.clone());
        decision.reason = format!(
            "{name} is missing, and {} use it: a new one would number its sources afresh, and their cells would show other tiles",
            list(&users)
        );
        return Ok(decision);
    }
    decision.bytes = Some(Cow::Owned(bytes));
    let Some(text) = text.as_deref() else {
        return Ok(decision);
    };
    writable(path)?;
    (decision.found, decision.reason, decision.extras) =
        classify(text, &body, &from, output.kind, &name, scan);
    if decision.found.refused() && options.replace {
        decision.backup = Some((
            backup_path(path),
            without_uid(text).into_owned().into_bytes(),
        ));
    }
    Ok(decision)
}

/// Another file carrying the uid the file at `path` has (`kept`) or, new, would get: a copy of
/// it, the file itself moved in Godot (it carries the marker `from` too), or an unrelated file;
/// with why, for people.
fn clash(
    path: &Path,
    kept: Option<&str>,
    exists: bool,
    from: &str,
    scan: &Scan,
) -> Option<(Found, String)> {
    let res = res_path(&scan.project, path).unwrap_or_else(|| path.display().to_string());
    let uid = kept.map_or_else(|| uid_to_text(derived_uid(&res, 0)), str::to_owned);
    let other = read::text_to_uid(&uid).and_then(|id| scan.carrying(id, path).next())?;
    let res_other =
        res_path(&scan.project, &other.path).unwrap_or_else(|| other.path.display().to_string());
    let name = file_name(path);
    Some(if kept.is_some() {
        (
            Found::Duplicate(other.path.clone()),
            format!(
                "{res_other} carries the uid of {name} ({uid}), as a copy made outside Godot does: Godot would mix the two files up"
            ),
        )
    } else if !exists && marker_in(&other.path).is_some_and(|m| m == from) {
        (
            Found::Moved(other.path.clone()),
            format!(
                "{name} seems to have moved in Godot to {res_other}: that file carries this export's uid ({uid})"
            ),
        )
    } else {
        (
            Found::UidInUse(other.path.clone()),
            format!(
                "{res_other} already has the uid {uid} this export would give {name}: Godot would mix the two files up"
            ),
        )
    })
}

/// The uid of a generated file at `path`: the one it has (`kept`), else one derived from its
/// `res://` path; with `new_uid`, a salted one no other file has. A shared TileSet, which other
/// scenes use by its uid, takes a new one only when another file has its own.
pub(super) fn uid(
    kind: Kind,
    path: &Path,
    kept: Option<String>,
    scan: &Scan,
    new_uid: bool,
) -> String {
    let res = res_path(&scan.project, path).unwrap_or_else(|| path.display().to_string());
    let in_use = || {
        let current = kept
            .clone()
            .unwrap_or_else(|| uid_to_text(derived_uid(&res, 0)));
        read::text_to_uid(&current).is_some_and(|id| scan.carrying(id, path).next().is_some())
    };
    let new_uid = new_uid && (kind != Kind::TileSet || in_use());
    match kept.clone().filter(|_| !new_uid) {
        Some(uid) => uid,
        None => uid_to_text(free_uid(&res, scan, path, new_uid, kept.as_deref())),
    }
}

/// A uid no other file has: derived from the `res://` path (salted with `--new-uid`, until it is
/// free and differs from `old`).
fn free_uid(res: &str, scan: &Scan, path: &Path, salted: bool, old: Option<&str>) -> u64 {
    if !salted {
        return derived_uid(res, 0);
    }
    let old = old.and_then(read::text_to_uid);
    (1..)
        .map(|salt| derived_uid(res, salt))
        .find(|&id| Some(id) != old && scan.carrying(id, path).next().is_none())
        .unwrap_or(1)
}

/// FNV-1a 64 of the `res://` path (and the salt), as 63 bits and never 0: Godot's ids are
/// positive.
pub(super) fn derived_uid(res: &str, salt: u32) -> u64 {
    let seed = if salt == 0 {
        Cow::Borrowed(res)
    } else {
        Cow::Owned(format!("{res}#{salt}"))
    };
    let id = fnv1a64(seed.as_bytes()) & (u64::MAX >> 1);
    id.max(1)
}

/// The three `;` lines starting a generated file whose rest has the hash `hash`.
fn header(kind: Kind, from: &str, hash: u64) -> String {
    // A line break in the path would end the comment.
    let from = from.replace(['\n', '\r'], " ");
    let advice = match kind {
        Kind::TileSet => {
            "Set tiles up in better-tiled:\n; changes made here are replaced by the next export."
        }
        _ => {
            "Edit the map, or a scene that\n; inherits this one: changes made here are replaced by the next export."
        }
    };
    format!(
        "; Generated by better-tiled from {from}. {advice}\n; {HEADER_MARK}{HEADER_VERSION} fnv1a64={}\n",
        hex(hash)
    )
}

/// `body` with ` uid="…"` added to its first line (`[gd_scene …]` or `[gd_resource …]`).
fn with_uid(body: &str, uid: &str) -> String {
    let line_end = body.find('\n').unwrap_or(body.len());
    let close = body[..line_end].rfind(']').unwrap_or(line_end);
    format!(
        "{} uid={}{}",
        &body[..close],
        text::string(uid),
        &body[close..]
    )
}

/// `text` with the uid taken out of its header line.
fn without_uid(text: &str) -> Cow<'_, str> {
    let Ok(resource) = read::parse_header(text) else {
        return Cow::Borrowed(text);
    };
    let Some(raw) = resource.header.attr("uid") else {
        return Cow::Borrowed(text);
    };
    let span = resource.header.span.clone();
    let attr = format!(" uid={raw}");
    match text[span.clone()].find(&attr) {
        Some(at) => Cow::Owned(format!(
            "{}{}",
            &text[..span.start + at],
            &text[span.start + at + attr.len()..]
        )),
        None => Cow::Borrowed(text),
    }
}

/// Where a file is kept before Replace writes over it: `<name>.bak`, else `<name>.2.bak`, …
fn backup_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    (1..)
        .map(|n| {
            path.with_file_name(if n == 1 {
                format!("{name}.bak")
            } else {
                format!("{name}.{n}.bak")
            })
        })
        .find(|p| !p.exists() && !p.is_symlink())
        .unwrap_or_else(|| path.with_file_name(format!("{name}.bak")))
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// The header's hash, if `resource` has the export's header.
fn header_hash(resource: &Resource) -> Option<Option<u64>> {
    let line = resource
        .comments
        .iter()
        .find_map(|c| c.trim_start().strip_prefix(HEADER_MARK))?;
    let mut words = line.split_whitespace();
    let version = words.next();
    let hash = words
        .find_map(|w| w.strip_prefix("fnv1a64="))
        .and_then(|h| u64::from_str_radix(h, 16).ok());
    Some(hash.filter(|_| version == Some(HEADER_VERSION)))
}

/// The map a generated file names in `metadata/bt_source`.
fn marker_of(resource: &Resource) -> Option<String> {
    let root = match resource.header.kind {
        "gd_scene" => resource.root().map(|(_, section)| section),
        _ => resource.resource(),
    }?;
    root.first(MARKER).and_then(read::string)
}

/// The map the generated file at `file` names in `metadata/bt_source`.
pub(super) fn marker_in(file: &Path) -> Option<String> {
    let text = std::fs::read_to_string(file).ok()?;
    marker_of(&read::parse(&text).ok()?)
}

/// What the file `text` at a generated file's place is (docs/design/12-godot-export.md), and for a refusal, why
/// and what it has beyond the export's `body`.
fn classify(
    text: &str,
    body: &str,
    from: &str,
    kind: Kind,
    name: &str,
    scan: &Scan,
) -> (Found, String, Vec<String>) {
    let resource = read::parse(text);
    let head = read::parse_header(text).ok();
    let hash = head.as_ref().and_then(header_hash);
    // The new file is only read when there is something to compare it with.
    let beyond = |r: &Resource| {
        read::parse(body)
            .map(|ours| extras(r, &ours))
            .unwrap_or_default()
    };
    let said = |extras: &[String]| {
        if extras.is_empty() {
            String::new()
        } else {
            format!(" It has {}.", list(extras))
        }
    };
    let inherit = match kind {
        Kind::TileSet => {
            "Make TileSet settings in better-tiled (godot* properties) instead.".to_owned()
        }
        _ => format!("Keep Godot work in a scene that inherits {name}."),
    };
    if let (Some(hash), Some(head)) = (hash, head.as_ref()) {
        let start = head.header.span.start;
        if hash == Some(fnv1a64(text.as_bytes().get(start..).unwrap_or_default())) {
            let theirs = resource
                .as_ref()
                .ok()
                .and_then(marker_of)
                .unwrap_or_default();
            if theirs == from {
                return (Found::Ours, String::new(), Vec::new());
            }
            let what = if theirs.is_empty() {
                "another map".to_owned()
            } else {
                theirs.clone()
            };
            return (
                Found::OtherMap(theirs),
                format!("it is the export of {what}, not of {from}"),
                Vec::new(),
            );
        }
        let extras = resource.as_ref().map(beyond).unwrap_or_default();
        let reason = format!(
            "it changed after better-tiled exported it (by hand, or by a merge): exporting would undo that.{} {inherit}",
            said(&extras)
        );
        return (Found::Edited, reason, extras);
    }
    let Ok(resource) = resource else {
        return (
            Found::NoHeader,
            "it has no better-tiled header, and isn't a scene Godot can read".to_owned(),
            Vec::new(),
        );
    };
    if marker_of(&resource).is_some() {
        let extras = beyond(&resource);
        let reason = format!(
            "Godot saved it again after better-tiled exported it: exporting would undo the changes made there.{} {inherit}",
            said(&extras)
        );
        return (Found::ResavedByGodot, reason, extras);
    }
    if let Some(base) = level_scene_of(&resource, scan) {
        let base_name = base.rsplit('/').next().unwrap_or_default().to_owned();
        let reason = format!(
            "it is {base_name}'s level scene: a scene of yours that inherits {base}, which no export writes over"
        );
        return (Found::LevelScene(base), reason, Vec::new());
    }
    if tiled_export(&resource) {
        // Tiled's exporter writes nodes for the map's objects too, so only what it never writes
        // can be told as someone's work.
        let extras: Vec<String> = beyond(&resource)
            .into_iter()
            .filter(|e| {
                e.starts_with("a script on ")
                    || e.ends_with("signal connection")
                    || e.ends_with("signal connections")
            })
            .collect();
        let reason = match kind {
            Kind::TileSet => "Tiled's Godot exporter wrote it. Replacing it gives a TileSet that better-tiled writes from the map's tilesets.".to_owned(),
            _ => "Tiled's Godot exporter wrote it. Replacing it turns its TileMap into one TileMapLayer per layer, so scripts that use $TileMap need the layer names; objects become typed nodes, and resPath objects stay instances.".to_owned(),
        };
        return (
            Found::TiledExport,
            format!("{reason}{}", said(&extras)),
            extras,
        );
    }
    let extras = beyond(&resource);
    let reason = format!(
        "it has no better-tiled header: an earlier version of better-tiled, Tiled or a person wrote it.{}",
        said(&extras)
    );
    (Found::NoHeader, reason, extras)
}

/// The generated scene the scene `resource` inherits, as its `res://` path, when it is a level
/// scene: its root instances a scene the export generated, or it is a starter (which says so).
fn level_scene_of(resource: &Resource, scan: &Scan) -> Option<String> {
    let id = resource.root()?.0.instance?;
    let base = resource
        .ext_resources()
        .find(|e| e.id == id && e.class == "PackedScene")?;
    let starter = resource
        .comments
        .iter()
        .any(|c| c.trim_start().starts_with(super::starter::FIRST_WORDS));
    let uid = base.uid.as_deref().and_then(read::text_to_uid);
    let generated = || {
        scan.entries.iter().any(|e| {
            e.generated
                && (res_path(&scan.project, &e.path).as_deref() == Some(base.path.as_str())
                    || (uid.is_some() && e.uid == uid))
        })
    };
    (starter || generated()).then_some(base.path)
}

/// Tiled's exporter writes one `TileMap` node with `format = 2` using its `TileSet_0`, or a
/// TileSet `.tres` of sources numbered `TileSetAtlasSource_<n>`. Godot writes both kinds of file
/// too, with ids of random letters.
fn tiled_export(resource: &Resource) -> bool {
    let maps: Vec<_> = resource
        .nodes()
        .filter(|(node, _)| node.class.as_deref() == Some("TileMap"))
        .collect();
    if let [(_, section)] = &maps[..] {
        let tile_set = section
            .get("tile_set")
            .and_then(|v| read::sub_resource(v).or_else(|| read::ext_resource(v)));
        return section.get("format") == Some("2") && tile_set.as_deref() == Some("TileSet_0");
    }
    let numbered = |id: &str| {
        id.strip_prefix("TileSetAtlasSource_")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    };
    let atlases: Vec<String> = resource
        .sub_resources()
        .filter(|(sub, _)| sub.class == "TileSetAtlasSource")
        .map(|(sub, _)| sub.id)
        .collect();
    resource.class().as_deref() == Some("TileSet")
        && !atlases.is_empty()
        && atlases.iter().all(|id| numbered(id))
}

/// What `theirs` has that the export's `ours` lacks, as phrases: scripts, nodes, signal
/// connections, groups, `unique_id`s and other properties.
fn extras(theirs: &Resource, ours: &Resource) -> Vec<String> {
    let ours_nodes: std::collections::HashMap<_, _> =
        ours.nodes().map(|(n, s)| (n.path(), (n, s))).collect();
    let root_name = theirs.root().map(|(n, _)| n.name).unwrap_or_default();
    let shown = |path: &str| {
        if path == "." {
            root_name.clone()
        } else {
            path.to_owned()
        }
    };
    let (mut scripts, mut added, mut grouped, mut ids, mut props) =
        (Vec::new(), Vec::new(), Vec::new(), 0, Vec::new());
    for (node, section) in theirs.nodes() {
        let path = node.path();
        if section.get("script").is_some() {
            scripts.push(format!("a script on '{}'", shown(&path)));
        }
        let Some((our_node, our_section)) = ours_nodes.get(&path) else {
            let what = node
                .class
                .clone()
                .or(node.instance.as_ref().map(|_| "instance".to_owned()))
                .unwrap_or_else(|| "node".to_owned());
            added.push(format!("{} {what} '{}'", article(&what), shown(&path)));
            continue;
        };
        if node.groups.iter().any(|g| !our_node.groups.contains(g)) {
            grouped.push(format!("'{}'", shown(&path)));
        }
        if node.unique_id.is_some() && node.unique_id != our_node.unique_id {
            ids += 1;
        }
        let new: Vec<&str> = section
            .props
            .iter()
            .map(|(k, _)| k.as_ref())
            .filter(|k| *k != "script" && our_section.get(k).is_none())
            .collect();
        if !new.is_empty() {
            props.push(format!("{} on '{}'", new.join(", "), shown(&path)));
        }
    }
    let mut out = scripts;
    const SHOWN: usize = 5;
    if added.len() > SHOWN {
        let more = added.len() - SHOWN;
        added.truncate(SHOWN);
        added.push(format!("{more} more nodes"));
    }
    out.extend(added);
    let connections = theirs
        .connections()
        .count()
        .saturating_sub(ours.connections().count());
    match connections {
        0 => {}
        1 => out.push("a signal connection".to_owned()),
        n => out.push(format!("{n} signal connections")),
    }
    if !grouped.is_empty() {
        out.push(format!("groups on {}", grouped.join(", ")));
    }
    out.extend(props);
    match ids {
        0 => {}
        1 => out.push("a unique_id the export didn't write".to_owned()),
        n => out.push(format!(
            "unique_ids on {n} nodes that the export didn't write"
        )),
    }
    let our_subs: Vec<String> = ours.sub_resources().map(|(s, _)| s.id).collect();
    let subs = theirs
        .sub_resources()
        .filter(|(s, _)| !our_subs.contains(&s.id))
        .count();
    if subs > 0 && theirs.header.kind == "gd_resource" {
        out.push(format!("{subs} resources the export doesn't write"));
    }
    out
}

fn article(word: &str) -> &'static str {
    if word.starts_with(['A', 'E', 'I', 'O', 'U', 'a', 'e', 'i', 'o', 'u']) {
        "an"
    } else {
        "a"
    }
}

/// "a, b and c".
fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Writes what was decided, each file atomically (Godot's editor may be watching): the image
/// copies, then TileSets, then the scene, so that what the scene uses is there when Godot reads
/// it, then the copies nothing uses any more go, then the list of where the copies came from. A
/// copy that can't be written is a warning; a scene or TileSet that can't be fails the export.
/// With `check`, only records what it would do.
fn write(
    decisions: &[Decision],
    prune: &[PathBuf],
    check: bool,
    report: &mut Report,
) -> Result<(), String> {
    let of = |kind: Kind| decisions.iter().filter(move |d| d.output.kind == kind);
    let (files, notes) = (&mut report.files, &mut report.notes);
    let mut record = |path: &Path, state: State| {
        files.push(File {
            path: path.to_path_buf(),
            state,
        })
    };
    // The copies that couldn't be written, with the hashes recorded for what is there.
    let mut unwritten = Vec::new();
    for d in of(Kind::Image) {
        if d.found == Found::CopyChanged {
            let message = format!(
                "the image copy {} was changed in Godot: it is kept, and shows instead of its image (which hasn't changed since)",
                d.output.path.display()
            );
            notes.push(Note::warning(crate::Code::CopyChanged, message, Vec::new()));
        }
        let Some(bytes) = d.bytes.as_deref().filter(|_| !check) else {
            record(
                &d.output.path,
                if d.bytes.is_some() {
                    State::Copied
                } else {
                    State::Unchanged
                },
            );
            continue;
        };
        let source = d.output.source.as_deref().unwrap_or(&d.output.path);
        let written = create_parent(&d.output.path)
            .map_err(|e| e.to_string())
            .and_then(|()| backup(d))
            .and_then(|()| atomically(&d.output.path, bytes));
        match written {
            Ok(()) => {
                if let Some((path, _)) = &d.backup {
                    record(path, State::BackedUp);
                }
                record(&d.output.path, State::Copied);
            }
            Err(e) => {
                notes.push(not_copied(source, &e));
                unwritten.push((d.output.path.clone(), d.output.recorded));
            }
        }
    }
    for d in of(Kind::TileSet).chain(of(Kind::Scene)) {
        let Some(bytes) = &d.bytes else {
            record(&d.output.path, State::Unchanged);
            continue;
        };
        if !check {
            create_parent(&d.output.path).map_err(|e| {
                format!(
                    "couldn't create {}: {e}",
                    normalized(d.output.path.parent().unwrap_or(Path::new(""))).display()
                )
            })?;
            backup(d)?;
            atomically(&d.output.path, bytes)?;
        }
        if let Some((path, _)) = &d.backup {
            record(path, State::BackedUp);
        }
        record(&d.output.path, State::Written);
    }
    for path in prune {
        match if check {
            Ok(())
        } else {
            std::fs::remove_file(path)
        } {
            Ok(()) => record(path, State::Pruned),
            Err(e) => notes.push(Note::warning(
                crate::Code::ImageSources,
                format!("couldn't remove {}: {e}", path.display()),
                Vec::new(),
            )),
        }
    }
    for d in of(Kind::Manifest) {
        // The list says what the copies are, so it keeps the old hashes of those not written.
        let as_written = (!unwritten.is_empty())
            .then(|| super::images::as_written(&d.output.path, &d.output.bytes, &unwritten))
            .transpose()?;
        let bytes = match &as_written {
            Some(bytes) => (existing(&d.output.path).ok().flatten().as_ref() != Some(bytes))
                .then_some(&bytes[..]),
            None => d.bytes.as_deref(),
        };
        let Some(bytes) = bytes else {
            record(&d.output.path, State::Unchanged);
            continue;
        };
        match if check {
            Ok(())
        } else {
            atomically(&d.output.path, bytes)
        } {
            Ok(()) => record(&d.output.path, State::Written),
            Err(e) => notes.push(Note::warning(crate::Code::ImageSources, e, Vec::new())),
        }
    }
    Ok(())
}

fn create_parent(path: &Path) -> std::io::Result<()> {
    path.parent().map_or(Ok(()), std::fs::create_dir_all)
}

/// Keeps the file being replaced, if Replace asked for it.
fn backup(d: &Decision) -> Result<(), String> {
    match &d.backup {
        Some((path, bytes)) => atomically(path, bytes),
        None => Ok(()),
    }
}

/// A read-only file is refused.
fn atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    bt_utils::atomic_write::write_file(path, bytes, bt_utils::atomic_write::WriteMode::Atomic)
        .map_err(|e| e.to_string())
}
