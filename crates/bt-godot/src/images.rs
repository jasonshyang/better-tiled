//! The images a scene uses, as `res://` paths. Images inside the Godot project are used where
//! they are; the others are copied next to the scene. The copies are planned here and written
//! with the scene (contract.rs), never during the build.
//!
//! `images/.sources.json` says, for each copy, where it came from, the hash of what was written
//! and which generated files use it (docs/design/12-godot-export.md): so a copy changed in Godot can be told from
//! one whose image changed, and a copy nothing uses any more can be removed.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::plan::{Kind, Plan};
use super::project_scan::Scan;
use super::read::fnv1a64;
use super::report::{Note, Severity};
use super::res_path;
use bt_utils::paths::{canonical_or_clean as normalized, reference_path as relative_path};

/// The file in the copies' folder saying where each copy came from (Godot skips dot files).
const SOURCES: &str = ".sources.json";

/// A copy to make: where it goes, its bytes, the image it is a copy of, and the hash recorded for
/// what was copied there before.
struct Copy {
    target: PathBuf,
    bytes: Vec<u8>,
    source: PathBuf,
    recorded: Option<u64>,
}

/// One copy in `.sources.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Copied {
    /// The image, relative to the copies' folder.
    pub source: String,
    /// FNV-1a 64 of the bytes written, in hex. Lists written before there was a hash have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// The generated files (`res://` paths) that use the copy.
    #[serde(default, rename = "usedBy")]
    pub used_by: BTreeSet<String>,
    /// The users that have the copy from another image with the same bytes (two maps' own
    /// `tiles.png` in sibling folders), each with that image; the others have it from `source`.
    #[serde(
        default,
        rename = "otherSources",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub other_sources: BTreeMap<String, String>,
}

impl Copied {
    fn hash(&self) -> Option<u64> {
        self.hash
            .as_deref()
            .and_then(|h| u64::from_str_radix(h, 16).ok())
    }

    /// The image `user` has the copy from.
    fn source_of(&self, user: &str) -> &str {
        self.other_sources.get(user).unwrap_or(&self.source)
    }

    /// Records that `user` has the copy from `origin`: its source, when no other file uses it.
    fn set_source(&mut self, user: &str, origin: String) {
        if self.used_by.iter().all(|u| u == user) {
            (self.source, self.other_sources) = (origin, BTreeMap::new());
        } else if origin == self.source {
            self.other_sources.remove(user);
        } else {
            self.other_sources.insert(user.to_owned(), origin);
        }
    }

    /// Whether every file using the copy has it from `origin` (with none, whether its source is
    /// `origin`): only then may a change to that image replace it in place.
    fn only_from(&self, origin: &str) -> bool {
        match self.used_by.is_empty() {
            true => self.source == origin,
            false => self.used_by.iter().all(|u| self.source_of(u) == origin),
        }
    }

    /// Forgets the sources of files that no longer use the copy; when those left all have it from
    /// one other image, that image is its source.
    fn settle(&mut self) {
        let used_by = &self.used_by;
        self.other_sources.retain(|user, _| used_by.contains(user));
        let mut others = self.other_sources.values();
        if let Some(first) = others.next()
            && self.other_sources.len() == self.used_by.len()
            && others.all(|o| o == first)
        {
            self.source = first.clone();
            self.other_sources.clear();
        }
    }
}

/// An entry as read: the list's first format was copy → source.
#[derive(Deserialize)]
#[serde(untagged)]
enum Stored {
    Source(String),
    Copied(Copied),
}

/// Reads the copy registry; only a missing registry means it is empty.
fn read_sources(path: &Path) -> Result<BTreeMap<String, Copied>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(e.to_string()),
    };
    let stored: BTreeMap<String, Stored> =
        serde_json::from_str(&text).map_err(|e| format!("invalid image registry: {e}"))?;
    Ok(stored
        .into_iter()
        .map(|(key, s)| {
            (
                key,
                match s {
                    Stored::Source(source) => Copied {
                        source,
                        ..Copied::default()
                    },
                    Stored::Copied(c) => c,
                },
            )
        })
        .collect())
}

pub(super) struct Images {
    project: PathBuf,
    /// Where images from outside the project go.
    copies: PathBuf,
    map_dir: PathBuf,
    /// The generated file that uses the images (`res://`), for `usedBy`.
    user: String,
    /// Every generated file this export writes that uses images: `user`, and those of
    /// [`Images::res_image_for`].
    users: BTreeSet<String>,
    paths: HashMap<PathBuf, String>,
    converted: HashMap<u64, String>,
    planned: Vec<Copy>,
    /// `images/.sources.json` (keys are paths in the copies' folder), read when first needed.
    sources: Option<BTreeMap<String, Copied>>,
    /// The copies this export uses, and by which generated file.
    used: HashSet<(String, String)>,
    /// The images that can't be read, and why: the export fails, writing nothing.
    missing: Vec<(PathBuf, String)>,
}

impl Images {
    pub fn builder(project: PathBuf, map_dir: PathBuf, user: String) -> Images {
        let copies = project.join("images");
        let users = BTreeSet::from([user.clone()]);
        Images {
            project,
            copies,
            map_dir,
            user,
            users,
            paths: HashMap::new(),
            converted: HashMap::new(),
            planned: Vec::new(),
            sources: None,
            used: HashSet::new(),
            missing: Vec::new(),
        }
    }

    pub fn copies_folder(mut self, copies: PathBuf) -> Self {
        self.copies = copies;
        self
    }
    pub fn project(&self) -> &Path {
        &self.project
    }
    pub fn map_folder(&self) -> &Path {
        &self.map_dir
    }

    /// How many images from outside the project this export uses (each copied into it).
    pub fn copied(&self) -> usize {
        self.paths
            .keys()
            .filter(|source| {
                bt_image::is_map_file(source) || res_path(&self.project, source).is_none()
            })
            .count()
            + self.converted.len()
    }

    fn sources(&mut self) -> &mut BTreeMap<String, Copied> {
        if self.sources.is_none() {
            let manifest = self.copies.join(SOURCES);
            match read_sources(&manifest) {
                Ok(sources) => self.sources = Some(sources),
                Err(e) => {
                    self.missing_image(&manifest, e);
                    self.sources = Some(BTreeMap::new());
                }
            }
        }
        self.sources.get_or_insert_with(BTreeMap::new)
    }

    /// Records that this export uses the copy at `key` (its path in the copies' folder), made
    /// from `origin` with `bytes`.
    fn record(&mut self, key: String, origin: String, bytes: &[u8]) {
        let user = self.user.clone();
        let copied = self.sources().entry(key.clone()).or_default();
        copied.set_source(&user, origin);
        copied.hash = Some(hex(fnv1a64(bytes)));
        copied.used_by.insert(user.clone());
        self.used.insert((user, key));
    }

    /// Records that the generated file `user` uses the image at `res` as it is, if that is a copy
    /// listed here: a shared TileSet's kept sources name the copies they were made with.
    pub fn keep(&mut self, res: &str, user: &str) {
        // Reusing an image already in the project does not create a copy registry.
        if self.sources.is_none() {
            let manifest = self.copies.join(SOURCES);
            match manifest.try_exists() {
                Ok(false) => return,
                Ok(true) => {}
                Err(error) => {
                    self.missing_image(&manifest, error.to_string());
                    return;
                }
            }
        }
        let Some(file) = res.strip_prefix("res://").map(|r| self.project.join(r)) else {
            return;
        };
        let key = relative_path(&self.copies, &file);
        let Some(copied) = self.sources().get_mut(&key) else {
            return;
        };
        copied.used_by.insert(user.to_owned());
        self.users.insert(user.to_owned());
        self.used.insert((user.to_owned(), key));
    }

    /// What is wrong when images the export uses can't be read: Godot would refuse the scene (or
    /// a shared TileSet, and with it every scene of its grid), so the export fails.
    pub fn missing(&self) -> Option<String> {
        match &self.missing[..] {
            [] => None,
            [(path, why)] => Some(format!(
                "the image {} can't be read ({why}): restore it, or fix the path that names it",
                path.display()
            )),
            many => {
                let mut images: Vec<String> = many
                    .iter()
                    .map(|(path, why)| format!("{} ({why})", path.display()))
                    .collect();
                let last = images.pop().unwrap_or_default();
                Some(format!(
                    "{} images can't be read: {} and {last}; restore them, or fix the paths that name them",
                    many.len(),
                    images.join(", ")
                ))
            }
        }
    }

    fn missing_image(&mut self, source: &Path, why: String) {
        if !self.missing.iter().any(|(path, _)| path == source) {
            self.missing.push((source.to_path_buf(), why));
        }
    }

    /// Adds the planned copies to `plan`, the copies this export no longer uses that may go, and
    /// the list of where the copies came from. A copy no generated file uses any more is removed
    /// only when it is as it was copied and no other file in the project mentions it.
    pub fn plan(&mut self, plan: &mut Plan, scan: &Scan, notes: &mut Vec<Note>) {
        for Copy {
            target,
            bytes,
            source,
            recorded,
        } in std::mem::take(&mut self.planned)
        {
            plan.copy(target, bytes, source, recorded);
        }
        let manifest = self.copies.join(SOURCES);
        if self.sources.is_none() && !manifest.is_file() {
            return;
        }
        let (users, used) = (self.users.clone(), std::mem::take(&mut self.used));
        let sources = self.sources();
        let mut unused = Vec::new();
        for (key, copied) in sources.iter_mut() {
            let before = copied.used_by.len();
            copied.used_by.retain(|user| {
                !users.contains(user) || used.contains(&(user.clone(), key.clone()))
            });
            copied.settle();
            if copied.used_by.len() < before && copied.used_by.is_empty() {
                unused.push(key.clone());
            }
        }
        let mut candidates = Vec::new();
        for key in unused {
            let file = self.copies.join(&key);
            let res = res_path(&self.project, &file).unwrap_or_default();
            let recorded = self.sources().get(&key).and_then(Copied::hash);
            match std::fs::read(&file) {
                // Gone already: nothing to keep track of.
                Err(_) => {
                    self.sources().remove(&key);
                }
                Ok(bytes) if Some(fnv1a64(&bytes)) != recorded => {
                    notes.push(kept(&res, "it was changed in Godot"))
                }
                Ok(_) => candidates.push((key, file, res)),
            }
        }
        // Mentions in the files this export replaces don't count, nor the copy's own `.import`.
        let mut except: Vec<PathBuf> = plan
            .outputs
            .iter()
            .filter(|o| matches!(o.kind, Kind::Scene | Kind::TileSet))
            .map(|o| o.path.clone())
            .collect();
        except.push(manifest.clone());
        for (_, file, _) in &candidates {
            except.push(file.clone());
            except.push(PathBuf::from(format!("{}.import", file.display())));
        }
        let wanted: Vec<String> = candidates.iter().map(|(_, _, res)| res.clone()).collect();
        let mentioned = match scan.mentioning(&wanted, &except) {
            Ok(mentioned) => mentioned,
            Err(e) => {
                self.missing_image(&manifest, e);
                return;
            }
        };
        for (key, file, res) in candidates {
            match mentioned.get(&res) {
                Some(by) => notes.push(kept(
                    &res,
                    &format!(
                        "{} mentions it",
                        res_path(&self.project, by).unwrap_or_else(|| by.display().to_string())
                    ),
                )),
                None => {
                    self.sources().remove(&key);
                    plan.prune.push(file);
                }
            }
        }
        let bytes = match manifest_bytes(self.sources()) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.missing_image(&manifest, error);
                return;
            }
        };
        plan.add(Kind::Manifest, manifest, bytes, None);
    }

    /// Resolves embedded images and transparent color keys into tracked PNG copies.
    pub fn res_image(&mut self, image: &bt_core::ImageRef) -> String {
        if image.transparent.is_none()
            && let Some(path) = image.path()
        {
            return self.res(path);
        }
        let key = image.transparent.map(|c| bt_image::ColorKey {
            r: c.r,
            g: c.g,
            b: c.b,
        });
        let decoded = match &image.source {
            bt_core::ImageSource::File(file) => bt_image::decode_file(&file.path, key),
            bt_core::ImageSource::Embedded(embedded) => {
                bt_image::decode_bytes(&embedded.bytes, &embedded.format, key)
            }
        };
        let identity = image
            .path()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.map_dir.join("embedded.png"));
        let bytes = decoded.and_then(|decoded| bt_image::encode_png(&identity, &decoded));
        let bytes = match bytes {
            Ok(bytes) => bytes,
            Err(e) => {
                self.missing_image(&identity, e.to_string());
                return String::new();
            }
        };
        let content_hash = fnv1a64(&bytes);
        let hash = hex(content_hash);
        if let Some(res) = self.converted.get(&content_hash).cloned() {
            let user = self.user.clone();
            self.keep(&res, &user);
            return res;
        }
        let target = self.copies.join(format!("embedded-{hash}.png"));
        let registry_key = relative_path(&self.copies, &target);
        let recorded = self.sources().get(&registry_key).and_then(Copied::hash);
        self.record(registry_key, relative_path(&self.copies, &identity), &bytes);
        if self
            .existing(&target)
            .is_none_or(|b| b.as_ref() != bytes.as_slice())
        {
            self.planned.push(Copy {
                target: target.clone(),
                bytes,
                source: identity,
                recorded,
            });
        }
        let res = res_path(&self.project, &target).unwrap_or_default();
        self.converted.insert(content_hash, res.clone());
        res
    }
    /// Uses an image on behalf of a shared resource.
    pub fn res_image_for(&mut self, image: &bt_core::ImageRef, user: &str, base: &Path) -> String {
        self.users.insert(user.into());
        let previous_user = std::mem::replace(&mut self.user, user.into());
        let previous_dir = std::mem::replace(&mut self.map_dir, base.into());
        let res = self.res_image(image);
        self.user = previous_user;
        self.map_dir = previous_dir;
        res
    }

    pub fn res(&mut self, source: &Path) -> String {
        let source = normalized(source);
        if let Some(path) = self.paths.get(&source).cloned() {
            let user = self.user.clone();
            self.keep(&path, &user);
            return path;
        }
        let map_image = bt_image::is_map_file(&source);
        let file = match res_path(&self.project, &source).filter(|_| !map_image) {
            Some(res) => {
                if !source.is_file() {
                    let why = std::fs::metadata(&source)
                        .map_or_else(|e| e.to_string(), |_| "it isn't a file".to_owned());
                    self.missing_image(&source, why);
                }
                self.paths.insert(source, res.clone());
                return res;
            }
            None => self.copy(&source, map_image),
        };
        let res = res_path(&self.project, &file).unwrap_or_default();
        self.paths.insert(source, res.clone());
        res
    }

    /// What is at `target` once the planned copies are made.
    fn existing(&self, target: &Path) -> Option<Cow<'_, [u8]>> {
        match self.planned.iter().find(|c| c.target == target) {
            Some(copy) => Some(Cow::Borrowed(&copy.bytes)),
            None => std::fs::read(target).ok().map(Cow::Owned),
        }
    }

    /// Plans a copy of an image into the project: `images/` mirrors its path from the map's
    /// folder (without the leading `..`s), so maps of one asset pack share the copies. The copy of
    /// an image that changed since is replaced (unless it was changed in Godot: the contract
    /// decides); a different image already in that place gets a numbered name. A map used as an
    /// image is drawn.
    fn copy(&mut self, source: &Path, map_image: bool) -> PathBuf {
        let relative = relative_path(&self.map_dir, source);
        let tail: PathBuf = if relative.starts_with('/') || relative.contains(':') {
            PathBuf::from(source.file_name().unwrap_or_default())
        } else {
            relative
                .split('/')
                .filter(|c| !c.is_empty() && *c != ".." && *c != ".")
                .collect()
        };
        let mut target = self.copies.join(tail);
        if map_image {
            target.set_extension("png");
        }
        let bytes = if map_image {
            render_map_image(source)
        } else {
            std::fs::read(source).map_err(|e| e.to_string())
        };
        let bytes = match bytes {
            Ok(bytes) => bytes,
            // Nothing to copy: the export fails.
            Err(e) => {
                self.missing_image(source, e);
                return target;
            }
        };
        let (stem, extension) = (
            target
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            target.extension().map(|e| e.to_string_lossy().into_owned()),
        );
        let origin = relative_path(&self.copies, source);
        let key = |target: &Path, copies: &Path| relative_path(copies, target);
        let mut n = 2;
        let mut recorded = None;
        while let Some(same) = self
            .existing(&target)
            .map(|existing| existing == bytes.as_slice())
        {
            let key = key(&target, &self.copies);
            if same {
                self.record(key, origin, &bytes);
                return target;
            }
            // The copy of this very image, which changed since: replace it, unless a file has it
            // from another image (which didn't change).
            if let Some(copied) = self.sources().get(&key).filter(|c| c.only_from(&origin)) {
                recorded = copied.hash();
                break;
            }
            let name = match &extension {
                Some(e) => format!("{stem}-{n}.{e}"),
                None => format!("{stem}-{n}"),
            };
            target.set_file_name(name);
            n += 1;
        }
        let key = key(&target, &self.copies);
        self.record(key, origin, &bytes);
        self.planned.push(Copy {
            target: target.clone(),
            bytes,
            source: source.to_path_buf(),
            recorded,
        });
        target
    }
}

fn manifest_bytes(sources: &BTreeMap<String, Copied>) -> Result<Vec<u8>, String> {
    Ok((serde_json::to_string_pretty(sources).map_err(|e| e.to_string())? + "\n").into_bytes())
}

/// The planned `.sources.json` at `manifest` (`bytes`) once the copies `unwritten` couldn't be
/// written: each keeps the hash recorded before (with each copy), being still what was copied
/// then, so the next export replaces it rather than taking it for one changed in Godot.
pub(super) fn as_written(
    manifest: &Path,
    bytes: &[u8],
    unwritten: &[(PathBuf, Option<u64>)],
) -> Result<Vec<u8>, String> {
    let mut sources = serde_json::from_slice::<BTreeMap<String, Copied>>(bytes)
        .map_err(|e| format!("invalid planned image registry: {e}"))?;
    let copies = manifest.parent().unwrap_or(Path::new(""));
    for (copy, recorded) in unwritten {
        if let Some(copied) = sources.get_mut(&relative_path(copies, copy)) {
            copied.hash = recorded.map(hex);
        }
    }
    manifest_bytes(&sources)
}

/// A hash as `.sources.json` and the export's header write it.
pub(super) fn hex(hash: u64) -> String {
    format!("{hash:016x}")
}

/// The warning for an image that couldn't be copied.
pub(super) fn not_copied(source: &Path, error: &str) -> Note {
    Note::warning(
        crate::Code::ImageNotCopied,
        format!("image {} not copied: {error}", source.display()),
        Vec::new(),
    )
}

/// The note for a copy nothing uses any more that stays.
fn kept(res: &str, why: &str) -> Note {
    Note {
        code: crate::Code::CopyKept,
        severity: Severity::Info,
        message: format!("{res} is no longer used by the export but stays: {why}"),
        elements: Vec::new(),
    }
}

fn render_map_image(source: &Path) -> Result<Vec<u8>, String> {
    let loaded = bt_tmx::Loader::builder()
        .build()
        .load_map(source)
        .map_err(|e| e.to_string())?;
    let device = bt_render::HeadlessDevice::acquire().map_err(|e| e.to_string())?;
    let picture = bt_render::Render::of(std::sync::Arc::new(loaded.value))
        .run(device)
        .map_err(|e| e.to_string())?;
    bt_image::encode_png(
        source,
        &bt_image::DecodedImage {
            width: picture.width,
            height: picture.height,
            rgba: picture.rgba,
        },
    )
    .map_err(|e| e.to_string())
}
