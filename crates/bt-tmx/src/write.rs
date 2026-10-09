//! Writing maps, tilesets and templates: a request names the value, the file and how; it
//! renders to text or writes the file atomically.

use std::path::{Path, PathBuf};

use bt_core::{CompatibilityVersion, Map, ObjectTemplate, Project, Tileset, World};
use bt_utils::atomic_write::{WriteMode, write_file};

use crate::error::Error;
use crate::export::{ExportOptions, exported_map, exported_tileset};
use crate::format::Format;
use crate::json;
use crate::loader::{Loader, absolute};
use crate::project::project_text;
use crate::stamp::{Stamp, stamp_text};
use crate::text::with_line_ending;
use crate::unfinish::Unfinisher;
use crate::warning::Warning;
use crate::world::world_text;
use crate::xml::XmlPrinter;

/// What a write request writes.
#[derive(Copy, Clone, Debug)]
enum Value<'a> {
    Map(&'a Map),
    Tileset(&'a Tileset),
    Template(&'a ObjectTemplate),
    World(&'a World),
    Project(&'a Project),
    Stamp(&'a Stamp),
}

/// A file's text and the problems found while making it.
#[derive(Clone, Debug)]
pub struct Rendered {
    /// The text, as it would be written.
    pub text: String,
    /// Problems that did not stop it (a renumbered first gid, say).
    pub warnings: Vec<Warning>,
}

/// A map, tileset or template to write to a file, and how. Writing reads no files; the
/// templates instances are diffed against come from the loader given.
#[derive(Clone, Debug)]
pub struct WriteRequest<'a> {
    value: Value<'a>,
    path: PathBuf,
    format: Option<Format>,
    loader: Option<&'a Loader>,
    export: ExportOptions,
    safe_writing: bool,
    compatibility: CompatibilityVersion,
}

impl<'a> WriteRequest<'a> {
    fn new(value: Value<'a>, path: &Path) -> WriteRequest<'a> {
        WriteRequest {
            value,
            path: path.to_path_buf(),
            format: None,
            loader: None,
            export: ExportOptions::default(),
            safe_writing: true,
            compatibility: CompatibilityVersion::Latest,
        }
    }

    /// Writes `map` to `path`.
    pub fn for_map(map: &'a Map, path: &Path) -> WriteRequest<'a> {
        WriteRequest::new(Value::Map(map), path)
    }

    /// Writes `tileset` to `path`, as a tileset file.
    pub fn for_tileset(tileset: &'a Tileset, path: &Path) -> WriteRequest<'a> {
        WriteRequest::new(Value::Tileset(tileset), path)
    }

    /// Writes `template` to `path`.
    pub fn for_template(template: &'a ObjectTemplate, path: &Path) -> WriteRequest<'a> {
        WriteRequest::new(Value::Template(template), path)
    }

    /// Writes `world` to `path`, in the layout Tiled writes worlds in.
    pub fn for_world(world: &'a World, path: &Path) -> WriteRequest<'a> {
        WriteRequest::new(Value::World(world), path)
    }

    /// Writes `project` to `path`, in the layout Tiled writes projects in.
    pub fn for_project(project: &'a Project, path: &Path) -> WriteRequest<'a> {
        WriteRequest::new(Value::Project(project), path)
    }

    /// Writes `stamp` to `path`, as Tiled writes stamps.
    pub fn for_stamp(stamp: &'a Stamp, path: &Path) -> WriteRequest<'a> {
        WriteRequest::new(Value::Stamp(stamp), path)
    }

    /// The syntax; by default the one the file name's extension names, else XML.
    pub fn format(mut self, format: Format) -> WriteRequest<'a> {
        self.format = Some(format);
        self
    }

    /// The loader the value was read with: template instances are written as their difference
    /// from the templates it holds, and export options resolve with its custom types.
    pub fn loader(mut self, loader: &'a Loader) -> WriteRequest<'a> {
        self.loader = Some(loader);
        self
    }

    /// File ▸ Export's options, applied to a copy of the value.
    pub fn export(mut self, options: ExportOptions) -> WriteRequest<'a> {
        self.export = options;
        self
    }

    /// Whether the file is written to a temporary file and renamed over the target (the
    /// default); without, the target is written in place, for folders where renaming misbehaves.
    pub fn safe_writing(mut self, safe: bool) -> WriteRequest<'a> {
        self.safe_writing = safe;
        self
    }

    /// The format version a value that has none is written in (latest by default); 1.9 spells
    /// classes of tiles and objects `class`.
    pub fn compatibility_version(mut self, version: CompatibilityVersion) -> WriteRequest<'a> {
        self.compatibility = version;
        self
    }

    /// The text that would be written.
    pub fn render(&self) -> Result<Rendered, Error> {
        let path = absolute(&self.path)?;
        let format = self
            .format
            .or_else(|| Format::from_path(&path))
            .unwrap_or(Format::Xml);
        let mut unfinisher = Unfinisher::new(&path, self.loader);
        let new_version = self.compatibility.version_string();
        let minimize = self.export.minimize;
        let types = self.loader.map(|loader| loader.custom_types().as_ref());
        let text = match self.value {
            Value::Map(original) => {
                let exported = exported_map(original, &self.export, types, self.loader, &path);
                let map = exported.as_ref().unwrap_or(original);
                let raw = unfinisher.map_of(map, new_version);
                let text = match format {
                    Format::Xml => XmlPrinter::new(minimize).print_map(&raw),
                    Format::Json | Format::JavaScript => {
                        let value = json::map_json(&raw);
                        let wrap = if map.infinite {
                            map.file.output_chunk_size.x
                        } else {
                            map.width
                        };
                        let text = if minimize {
                            json::minimized_layout(&value)
                        } else {
                            json::tiled_layout(&value, usize::try_from(wrap).unwrap_or(0))
                        };
                        if format == Format::JavaScript {
                            javascript_wrapper(&text, &path)
                        } else {
                            text
                        }
                    }
                };
                with_line_ending(text, map.file.line_ending)
            }
            Value::Tileset(original) => {
                let exported = exported_tileset(original, &self.export, types);
                let tileset = exported.as_ref().unwrap_or(original);
                let raw = unfinisher.tileset_file_of(tileset, new_version);
                let text = match format {
                    Format::Xml => XmlPrinter::new(minimize).print_tileset_file(&raw),
                    Format::Json => {
                        let value = json::tileset_json(&raw);
                        if minimize {
                            json::minimized_layout(&value)
                        } else {
                            json::tiled_layout(&value, 0)
                        }
                    }
                    Format::JavaScript => {
                        return Err(Error::format(
                            &path,
                            "tilesets cannot be written as JavaScript",
                        ));
                    }
                };
                with_line_ending(text, tileset.file.line_ending)
            }
            Value::World(world) => world_text(world, &path),
            Value::Project(project) => project_text(project, &path),
            Value::Stamp(stamp) => {
                return Ok(stamp_text(stamp, &path, self.loader));
            }
            Value::Template(template) => {
                let raw = unfinisher.template_of(template);
                let text = match format {
                    Format::Xml => XmlPrinter::new(minimize).print_template(&raw),
                    Format::Json => json::tiled_layout(&json::template_json(&raw), 0),
                    Format::JavaScript => {
                        return Err(Error::format(
                            &path,
                            "templates cannot be written as JavaScript",
                        ));
                    }
                };
                with_line_ending(text, template.file.line_ending)
            }
        };
        Ok(Rendered {
            text,
            warnings: unfinisher.warnings.into_list(),
        })
    }

    /// Writes the file, atomically unless safe writing is off; refuses read-only targets
    /// before writing anything (see [`Error::refused`]).
    pub fn write(&self) -> Result<Vec<Warning>, Error> {
        let rendered = self.render()?;
        let path = absolute(&self.path)?;
        let mode = if self.safe_writing {
            WriteMode::Atomic
        } else {
            WriteMode::InPlace
        };
        write_file(&path, rendered.text.as_bytes(), mode).map_err(|source| Error::Write {
            path: path.clone(),
            source,
        })?;
        Ok(rendered.warnings)
    }
}

/// The wrapper of Tiled's JavaScript maps: the map goes to `onTileMapLoaded` when it is defined,
/// else into `TileMaps`, and is a CommonJS module's export.
fn javascript_wrapper(json: &str, path: &Path) -> String {
    // The file name up to its first dot (`QFileInfo::baseName`).
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let base_name = file_name.split('.').next().unwrap_or("").to_owned();
    let name = json::tiled_layout(&serde_json::Value::String(base_name), 0);
    format!(
        "(function(name,data){{\n if(typeof onTileMapLoaded === 'undefined') {{\n  if(typeof TileMaps === 'undefined') TileMaps = {{}};\n  TileMaps[name] = data;\n }} else {{\n  onTileMapLoaded(name,data);\n }}\n if(typeof module === 'object' && module && module.exports) {{\n  module.exports = data;\n }}}})({name},\n{json});"
    )
}
