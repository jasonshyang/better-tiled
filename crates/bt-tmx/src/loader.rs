//! The loader: reads maps, tilesets and templates with the files they refer to, caching the
//! shared ones so that maps loaded through one loader share their tilesets.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use bt_core::{CustomTypes, Map, ObjectTemplate, Tileset};
use glam::UVec2;

use crate::automap_tiles::builtin_automap_tiles;
use crate::error::Error;
use crate::finish::Finisher;
use crate::format::{Format, unwrap_javascript};
use crate::paths::resolve_reference;
use crate::raw::{RawMap, RawTemplate, RawTileset};
use crate::text::{line_ending, read_text};
use crate::warning::{Warning, Warnings};
use crate::{json, xml};

/// A value read from a file, with the problems found in it and in the files it refers to.
#[derive(Clone, Debug)]
pub struct Loaded<T> {
    /// What was read.
    pub value: T,
    /// The problems that did not stop it, each once, naming the file it was found in.
    pub warnings: Vec<Warning>,
}

/// A cached file.
#[derive(Debug)]
struct Cached<T> {
    value: T,
    warnings: Vec<Warning>,
    /// The file's modification time when it was read; once it changes, the file is read again.
    modified: Option<SystemTime>,
}

/// Sets up a [`Loader`].
#[derive(Debug, Default)]
pub struct LoaderBuilder {
    custom_types: Arc<CustomTypes>,
    automap_tiles: Option<Arc<Tileset>>,
}

impl LoaderBuilder {
    /// The custom types property values are decoded with: the project's.
    pub fn custom_types(mut self, custom_types: Arc<CustomTypes>) -> LoaderBuilder {
        self.custom_types = custom_types;
        self
    }

    /// The tileset `:/automap-tiles.tsx` stands for; the built-in one by default.
    pub fn automap_tiles(mut self, tileset: Arc<Tileset>) -> LoaderBuilder {
        self.automap_tiles = Some(tileset);
        self
    }

    /// The loader.
    pub fn build(self) -> Loader {
        Loader {
            custom_types: self.custom_types,
            automap_tiles: self
                .automap_tiles
                .unwrap_or_else(|| Arc::new(builtin_automap_tiles())),
            tilesets: HashMap::new(),
            templates: HashMap::new(),
            canonical: HashMap::new(),
            reading: Vec::new(),
        }
    }
}

/// Reads Tiled's files. External tilesets and templates are cached by their canonical path
/// and modification time, so maps read through one loader share their `Arc<Tileset>`s until a
/// file changes. A loader holds no process-wide state; loaders with different custom types
/// coexist.
#[derive(Debug)]
pub struct Loader {
    custom_types: Arc<CustomTypes>,
    automap_tiles: Arc<Tileset>,
    tilesets: HashMap<PathBuf, Cached<Arc<Tileset>>>,
    templates: HashMap<PathBuf, Cached<Arc<ObjectTemplate>>>,
    /// The canonical path of each path files were asked for by, so that writers, which read no
    /// files, find cached templates.
    canonical: HashMap<PathBuf, PathBuf>,
    /// Tileset and template files being read, to refuse one that refers to itself.
    reading: Vec<PathBuf>,
}

impl Default for Loader {
    fn default() -> Loader {
        Loader::builder().build()
    }
}

impl Loader {
    /// Sets up a loader.
    pub fn builder() -> LoaderBuilder {
        LoaderBuilder::default()
    }

    /// The custom types property values are decoded with.
    pub fn custom_types(&self) -> &Arc<CustomTypes> {
        &self.custom_types
    }

    /// The tileset `:/automap-tiles.tsx` stands for.
    pub fn automap_tiles(&self) -> &Arc<Tileset> {
        &self.automap_tiles
    }

    /// Reads a map (XML, JSON or JavaScript-wrapped JSON, told apart by the contents) with its
    /// tilesets and templates.
    pub fn load_map(&mut self, path: &Path) -> Result<Loaded<Map>, Error> {
        let path = absolute(path)?;
        let text = read_text(&path)?;
        self.parse_map(&text, &path)
    }

    /// Reads a map from text; `path` is where the map is considered to be, so its references
    /// resolve against its folder.
    pub fn parse_map(&mut self, text: &str, path: &Path) -> Result<Loaded<Map>, Error> {
        let path = absolute(path)?;
        let mut warnings = Warnings::new(&path);
        let raw = read_raw_map(text, &path, &mut warnings)?;
        let mut finisher = Finisher::new(self, &path);
        finisher.warnings.record_all(warnings.into_list());
        let mut map = finisher.map_of(raw);
        map.file.line_ending = line_ending(text);
        Ok(Loaded {
            value: map,
            warnings: finisher.into_warnings(),
        })
    }

    /// Reads a world, decoding its properties with the loader's custom types.
    pub fn load_world(&mut self, path: &Path) -> Result<Loaded<bt_core::World>, Error> {
        let path = absolute(path)?;
        let text = read_text(&path)?;
        self.parse_world(&text, &path)
    }

    /// Reads a world from text; `path` is where it is.
    pub fn parse_world(
        &mut self,
        text: &str,
        path: &Path,
    ) -> Result<Loaded<bt_core::World>, Error> {
        let path = absolute(path)?;
        crate::world::parse_world(text, &path, &self.custom_types)
    }

    /// Reads a tileset file (XML or JSON), or takes it from the cache when it did not change.
    pub fn load_tileset(&mut self, path: &Path) -> Result<Loaded<Arc<Tileset>>, Error> {
        let path = absolute(path)?;
        self.load_tileset_cached(&path)
    }

    /// Reads a tileset from text, without caching it; `path` is where it is considered to be.
    pub fn parse_tileset(
        &mut self,
        text: &str,
        path: &Path,
    ) -> Result<Loaded<Arc<Tileset>>, Error> {
        let path = absolute(path)?;
        self.parse_tileset_document(text, &path)
    }

    /// Reads a template file (XML or JSON), or takes it from the cache when it did not change.
    pub fn load_template(&mut self, path: &Path) -> Result<Loaded<Arc<ObjectTemplate>>, Error> {
        let path = absolute(path)?;
        self.load_template_cached(&path)
    }

    /// Reads a template from text, without caching it; `path` is where it is considered to be.
    pub fn parse_template(
        &mut self,
        text: &str,
        path: &Path,
    ) -> Result<Loaded<Arc<ObjectTemplate>>, Error> {
        let path = absolute(path)?;
        let loaded = self.parse_template_document(text, &path)?;
        Ok(Loaded {
            value: Arc::new(loaded.value),
            warnings: loaded.warnings,
        })
    }

    /// Uses `tileset` for the tileset file at `path` (a version with unsaved changes) until the
    /// file changes.
    pub fn insert_tileset(&mut self, path: &Path, tileset: Arc<Tileset>) -> Result<(), Error> {
        let path = absolute(path)?;
        let key = self.canonical_key(&path)?;
        let modified = modified(&key);
        self.tilesets.insert(
            key,
            Cached {
                value: tileset,
                warnings: Vec::new(),
                modified,
            },
        );
        Ok(())
    }

    /// Forgets a cached tileset or template file, so that it is read again when next used.
    pub fn invalidate(&mut self, path: &Path) {
        let path = absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let key = self
            .canonical
            .get(&path)
            .cloned()
            .or_else(|| std::fs::canonicalize(&path).ok())
            .unwrap_or(path);
        self.tilesets.remove(&key);
        self.templates.remove(&key);
    }

    /// Forgets every cached tileset and template.
    pub fn clear_cache(&mut self) {
        self.tilesets.clear();
        self.templates.clear();
        self.canonical.clear();
    }

    /// The cached template of the file at `path`, as last read; reads no file.
    pub(crate) fn cached_template(&self, path: &Path) -> Option<&Arc<ObjectTemplate>> {
        let key = self.canonical.get(path).map_or(path, PathBuf::as_path);
        self.templates.get(key).map(|cached| &cached.value)
    }

    /// The size of an image file, from its header.
    pub(crate) fn image_size(&self, path: &Path) -> Result<UVec2, bt_image::Error> {
        bt_image::probe_size(path)
    }

    /// The canonical path of a file (following links), remembered for writers.
    fn canonical_key(&mut self, path: &Path) -> Result<PathBuf, Error> {
        let key = std::fs::canonicalize(path).map_err(|source| Error::from_io(path, source))?;
        self.canonical.insert(path.to_path_buf(), key.clone());
        Ok(key)
    }

    /// A tileset file, from the cache when it did not change.
    pub(crate) fn load_tileset_cached(
        &mut self,
        path: &Path,
    ) -> Result<Loaded<Arc<Tileset>>, Error> {
        let key = self.canonical_key(path)?;
        let modified = modified(&key);
        if let Some(cached) = self.tilesets.get(&key).filter(|c| c.modified == modified) {
            return Ok(Loaded {
                value: Arc::clone(&cached.value),
                warnings: cached.warnings.clone(),
            });
        }
        if self.reading.contains(&key) {
            return Err(Error::format(path, "the tileset refers to itself"));
        }
        self.reading.push(key.clone());
        let result = read_text(path).and_then(|text| self.parse_tileset_document(&text, path));
        self.reading.pop();
        let loaded = result?;
        self.tilesets.insert(
            key,
            Cached {
                value: Arc::clone(&loaded.value),
                warnings: loaded.warnings.clone(),
                modified,
            },
        );
        Ok(loaded)
    }

    fn parse_tileset_document(
        &mut self,
        text: &str,
        path: &Path,
    ) -> Result<Loaded<Arc<Tileset>>, Error> {
        let mut warnings = Warnings::new(path);
        let raw = read_raw_tileset(text, path, &mut warnings)?;
        let source = raw
            .source
            .as_ref()
            .map(crate::raw::Scalar::to_text)
            .unwrap_or_default();
        if !source.trim().is_empty() {
            // A tileset file whose root refers to another file stands for it, as in Tiled.
            let dir = path.parent().unwrap_or(Path::new(""));
            return self.load_tileset_cached(&resolve_reference(dir, &source));
        }
        let version = raw
            .version
            .as_ref()
            .map(crate::raw::Scalar::to_text)
            .unwrap_or_default();
        let tiled_version = raw
            .tiled_version
            .as_ref()
            .map(crate::raw::Scalar::to_text)
            .unwrap_or_default();
        let mut finisher = Finisher::new(self, path);
        finisher.warnings.record_all(warnings.into_list());
        let mut tileset = finisher.tileset_of(raw);
        tileset.file.version = version;
        tileset.file.tiled_version = tiled_version;
        tileset.file.line_ending = line_ending(text);
        Ok(Loaded {
            value: Arc::new(tileset),
            warnings: finisher.into_warnings(),
        })
    }

    /// A template file, from the cache when it did not change.
    pub(crate) fn load_template_cached(
        &mut self,
        path: &Path,
    ) -> Result<Loaded<Arc<ObjectTemplate>>, Error> {
        let key = self.canonical_key(path)?;
        let modified = modified(&key);
        if let Some(cached) = self.templates.get(&key).filter(|c| c.modified == modified) {
            return Ok(Loaded {
                value: Arc::clone(&cached.value),
                warnings: cached.warnings.clone(),
            });
        }
        if self.reading.contains(&key) {
            return Err(Error::format(path, "the template refers to itself"));
        }
        self.reading.push(key.clone());
        let result = read_text(path).and_then(|text| self.parse_template_document(&text, path));
        self.reading.pop();
        let loaded = result?;
        let value = Arc::new(loaded.value);
        self.templates.insert(
            key,
            Cached {
                value: Arc::clone(&value),
                warnings: loaded.warnings.clone(),
                modified,
            },
        );
        Ok(Loaded {
            value,
            warnings: loaded.warnings,
        })
    }

    fn parse_template_document(
        &mut self,
        text: &str,
        path: &Path,
    ) -> Result<Loaded<ObjectTemplate>, Error> {
        let mut warnings = Warnings::new(path);
        let raw = read_raw_template(text, path, &mut warnings)?;
        let mut finisher = Finisher::new(self, path);
        finisher.warnings.record_all(warnings.into_list());
        let mut template = finisher.template_of(raw);
        template.file.line_ending = line_ending(text);
        Ok(Loaded {
            value: template,
            warnings: finisher.into_warnings(),
        })
    }
}

/// `path` made absolute and cleaned.
pub(crate) fn absolute(path: &Path) -> Result<PathBuf, Error> {
    bt_utils::paths::absolute_path(path).map_err(|error| Error::format(path, error.to_string()))
}

/// A file's modification time, when the system gives one.
fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The raw tree of a map's text, in whichever syntax it is.
pub(crate) fn read_raw_map(
    text: &str,
    path: &Path,
    warnings: &mut Warnings,
) -> Result<RawMap, Error> {
    match Format::of_file(text, path) {
        Format::Xml => {
            let document = xml::parse_document(text, path)?;
            let root = document.root_element();
            if !root.has_tag_name("map") {
                return Err(not_kind(path, "map", root.tag_name().name()));
            }
            record_doctype(text, warnings);
            Ok(xml::XmlReader::new(warnings).read_map(root))
        }
        Format::Json | Format::JavaScript => {
            let value = json::parse_document(unwrap_javascript(text), path)?;
            json::read_map(&value, path, warnings)
        }
    }
}

/// The raw tree of a tileset file's text.
fn read_raw_tileset(text: &str, path: &Path, warnings: &mut Warnings) -> Result<RawTileset, Error> {
    match Format::of_file(text, path) {
        Format::Xml => {
            let document = xml::parse_document(text, path)?;
            let root = document.root_element();
            if !root.has_tag_name("tileset") {
                return Err(not_kind(path, "tileset", root.tag_name().name()));
            }
            record_doctype(text, warnings);
            Ok(xml::XmlReader::new(warnings).read_tileset(root))
        }
        Format::Json | Format::JavaScript => {
            let value = json::parse_document(text, path)?;
            json::read_tileset_file(&value, path, warnings)
        }
    }
}

/// The raw tree of a template file's text.
fn read_raw_template(
    text: &str,
    path: &Path,
    warnings: &mut Warnings,
) -> Result<RawTemplate, Error> {
    match Format::of_file(text, path) {
        Format::Xml => {
            let document = xml::parse_document(text, path)?;
            let root = document.root_element();
            if !root.has_tag_name("template") {
                return Err(not_kind(path, "template", root.tag_name().name()));
            }
            Ok(xml::XmlReader::new(warnings).read_template(root))
        }
        Format::Json | Format::JavaScript => {
            let value = json::parse_document(text, path)?;
            json::read_template(&value, path, warnings)
        }
    }
}

/// Notes a `DOCTYPE` (written by Tiled before 1.0), which a save leaves out.
fn record_doctype(text: &str, warnings: &mut Warnings) {
    if text.contains("<!DOCTYPE") {
        warnings.record(
            crate::warning::WarningKind::LegacyShape,
            "the file has a DOCTYPE (Tiled before 1.0); it is left out when saved",
        );
    }
}

/// The error for an XML file whose root is not the element asked for.
fn not_kind(path: &Path, kind: &str, root: &str) -> Error {
    Error::format(
        path,
        format!("not a Tiled {kind}: its root element is <{root}>"),
    )
}
