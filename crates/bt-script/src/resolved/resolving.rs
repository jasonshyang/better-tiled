//! What every part of the resolved JSON is made from: the map (its `file` properties already
//! relative to the JSON file), its grid and names, the project's custom types, the JSON file's
//! folder, which tilesets are AutoMap's rule tiles, and the warnings gathered on the way.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use bt_core::{CustomTypes, ImageRef, ImageSource, Map, MapGeometry, TileId, TilesetSlot};
use bt_tmx::builtin_automap_tiles;
use bt_utils::paths::{clean_path, relative_path};

use super::error::ResolvedWarning;
use crate::names::Names;

/// The map being resolved and what is learnt on the way.
pub(crate) struct Resolving<'m> {
    /// The map, its `file` properties relative to the JSON file.
    pub(crate) map: &'m Map,
    /// Its grid.
    pub(crate) geometry: MapGeometry,
    /// Its names, as bt gives them.
    pub(crate) names: Names,
    /// The custom types classes resolve with.
    pub(crate) types: &'m CustomTypes,
    /// Where the JSON file and the map file are.
    folders: Folders,
    /// Which tilesets are AutoMap's rule tiles, by slot.
    rule_slots: Vec<bool>,
    /// What was left out of each rule tileset, by slot.
    rule_tiles_left_out: BTreeMap<usize, LeftOut>,
    /// The tiles shown without an image, by their tileset's slot.
    without_image: BTreeMap<usize, BTreeSet<u32>>,
    /// The warnings so far, each once.
    warnings: Vec<ResolvedWarning>,
}

/// The folders the JSON's paths are worked out from, both absolute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Folders {
    /// The JSON file's folder: every path written is relative to it.
    pub(crate) json: PathBuf,
    /// The map file's folder: what the map's own `file` properties, and any path the map left
    /// relative, are relative to.
    pub(crate) map: PathBuf,
}

/// What was left out of a rule tileset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct LeftOut {
    cells: usize,
    objects: usize,
}

impl<'m> Resolving<'m> {
    /// The resolving of `map` for a JSON file in `folders.json`, its classes resolved with
    /// `types`.
    pub(crate) fn new(map: &'m Map, types: &'m CustomTypes, folders: Folders) -> Resolving<'m> {
        let builtin = builtin_automap_tiles();
        let rule_slots = map
            .tilesets
            .iter()
            .map(|used| *used.tileset == builtin)
            .collect();
        Resolving {
            map,
            geometry: map.geometry(),
            names: Names::of(map),
            types,
            folders,
            rule_slots,
            rule_tiles_left_out: BTreeMap::new(),
            without_image: BTreeMap::new(),
            warnings: Vec::new(),
        }
    }

    /// Whether the tileset in `slot` is AutoMap's rule tiles (Tiled's built-in rule tileset,
    /// `bt_tmx::builtin_automap_tiles`).
    pub(crate) fn is_rule_tileset(&self, slot: TilesetSlot) -> bool {
        self.rule_slots.get(slot.index()).copied().unwrap_or(false)
    }

    /// Counts a cell of a rule tileset left empty.
    pub(crate) fn record_rule_cell(&mut self, slot: TilesetSlot) {
        self.rule_tiles_left_out
            .entry(slot.index())
            .or_default()
            .cells += 1;
    }

    /// Counts an object showing a rule tile left out.
    pub(crate) fn record_rule_object(&mut self, slot: TilesetSlot) {
        self.rule_tiles_left_out
            .entry(slot.index())
            .or_default()
            .objects += 1;
    }

    /// Notes a tile shown without an image, for one warning per tileset.
    pub(crate) fn record_tile_without_image(&mut self, slot: TilesetSlot, tile: TileId) {
        self.without_image
            .entry(slot.index())
            .or_default()
            .insert(tile.0);
    }

    /// Adds a warning, unless it was given already.
    pub(crate) fn record_warning(&mut self, warning: ResolvedWarning) {
        if !self.warnings.contains(&warning) {
            self.warnings.push(warning);
        }
    }

    /// `path` relative to the JSON file's folder, with `/` between its parts; absolute when no
    /// relative path leads there (another Windows drive). A path the map left relative is the
    /// map file's folder's.
    pub(crate) fn relative(&self, path: &Path) -> String {
        let absolute = if path.is_absolute() {
            clean_path(path)
        } else {
            clean_path(&self.folders.map.join(path))
        };
        relative_path(&self.folders.json, &absolute)
            .unwrap_or_else(|| absolute.to_string_lossy().replace('\\', "/"))
    }

    /// An image's path relative to the JSON file; `None` for an image the map file embeds, with
    /// a warning naming `what` shows it.
    pub(crate) fn image_path(
        &mut self,
        image: &ImageRef,
        what: impl FnOnce() -> String,
    ) -> Option<String> {
        match &image.source {
            ImageSource::File(file) => Some(self.relative(&file.path)),
            ImageSource::Embedded(_) => {
                self.record_warning(ResolvedWarning::EmbeddedImage { what: what() });
                None
            }
        }
    }

    /// The warnings: what was said on the way, the tiles without an image, then what each rule
    /// tileset left out.
    pub(crate) fn into_warnings(self) -> Vec<ResolvedWarning> {
        let mut warnings = self.warnings;
        let tileset_name = |slot: usize| {
            self.map
                .tilesets
                .get(slot)
                .map_or_else(String::new, |used| used.tileset.name.clone())
        };
        for (slot, tiles) in &self.without_image {
            warnings.push(ResolvedWarning::NoImage {
                tileset: tileset_name(*slot),
                tiles: tiles.iter().copied().collect(),
            });
        }
        for (slot, ruled) in self.rule_slots.iter().enumerate() {
            if !ruled {
                continue;
            }
            let left_out = self
                .rule_tiles_left_out
                .get(&slot)
                .copied()
                .unwrap_or_default();
            warnings.push(ResolvedWarning::RuleTilesLeftOut {
                tileset: tileset_name(slot),
                cells: left_out.cells,
                objects: left_out.objects,
            });
        }
        warnings
    }
}
