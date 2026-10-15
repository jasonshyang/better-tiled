//! A configured export of the map currently held by the caller.
use crate::{Error, Exporter, Options, Report};
use bt_core::{CustomTypes, Map};
use std::path::Path;

/// Builds an export without reading or writing until `run` is called.
pub struct ExportRequest<'a> {
    map: &'a Map,
    path: &'a Path,
    target: &'a Path,
    types: Option<&'a CustomTypes>,
    options: Options,
}
impl<'a> ExportRequest<'a> {
    /// Starts an export of `map`, stored at `path`, to the scene `target`.
    pub fn builder(map: &'a Map, path: &'a Path, target: &'a Path) -> Self {
        Self {
            map,
            path,
            target,
            types: None,
            options: Options::default(),
        }
    }
    /// The project's custom types; an empty registry is valid.
    pub fn custom_types(mut self, types: &'a CustomTypes) -> Self {
        self.types = Some(types);
        self
    }
    /// The regeneration options for this export.
    pub fn options(mut self, options: Options) -> Self {
        self.options = options;
        self
    }
    /// Plans, checks and writes the export through the caller's cached exporter.
    pub fn run(self, exporter: &mut Exporter) -> Result<Report, Error> {
        let types = self.types.ok_or_else(|| {
            Error::Failed(
                "an export needs its custom-type registry (an empty registry is valid)".to_owned(),
            )
        })?;
        exporter.export_map(self.map, self.path, types, self.target, &self.options)
    }
}
