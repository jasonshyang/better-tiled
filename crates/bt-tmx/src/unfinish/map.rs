//! Maps back into the raw tree.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bt_core::{FileRef, LayerKind, Map, MapObject, ObjectOverrides};
use glam::UVec2;

use super::gids::{FirstGids, TilesetUse};
use super::property::raw_properties;
use crate::loader::Loader;
use crate::names;
use crate::paths::written_reference;
use crate::raw::{RawExport, RawMap, RawSize, RawTileset, Scalar};
use crate::warning::{Target, WarningKind, Warnings};

/// Turns values back into the raw tree for one file.
pub(crate) struct Unfinisher<'a> {
    /// The folder of the file being written.
    pub(crate) dir: PathBuf,
    /// The problems found.
    pub(crate) warnings: Warnings,
    /// The loader the value was read with: template instances are written as their difference
    /// from the template it holds.
    templates: Option<&'a Loader>,
    /// Whether tiles' and objects' classes are spelled `type` (all versions but 1.9).
    pub(crate) class_as_type: bool,
    /// The global ids of the map or template being written.
    pub(crate) gids: FirstGids,
    /// What instances of each template take, by template file.
    bases: HashMap<PathBuf, Option<MapObject>>,
    /// Whether references are written as absolute paths (the clipboard's fragments).
    absolute_references: bool,
}

impl<'a> Unfinisher<'a> {
    /// Writes the file at `path` (absolute); `templates` is the loader the value was read with.
    pub(crate) fn new(path: &Path, templates: Option<&'a Loader>) -> Unfinisher<'a> {
        Unfinisher {
            dir: path.parent().map(Path::to_path_buf).unwrap_or_default(),
            warnings: Warnings::new(path),
            templates,
            class_as_type: true,
            gids: FirstGids::only(0, 0),
            bases: HashMap::new(),
            absolute_references: false,
        }
    }

    /// Writes text with absolute references, as the clipboard holds maps.
    pub(crate) fn with_absolute_references(templates: Option<&'a Loader>) -> Unfinisher<'a> {
        Unfinisher {
            absolute_references: true,
            ..Unfinisher::new(Path::new("/clipboard.tmx"), templates)
        }
    }

    /// A reference as written from the file's folder.
    pub(crate) fn reference(&self, file: &FileRef) -> String {
        if self.absolute_references && file.path.is_absolute() {
            return file.path.to_string_lossy().replace('\\', "/");
        }
        written_reference(&self.dir, file)
    }

    /// Spells classes of tiles and objects for the version written.
    pub(crate) fn spell_classes_for(&mut self, version: &str) {
        self.class_as_type = version != "1.9";
    }

    /// A map; `new_version` is the format version for a map that has none.
    pub(crate) fn map_of(&mut self, map: &Map, new_version: &str) -> RawMap {
        let version = if map.file.version.is_empty() {
            new_version.to_owned()
        } else {
            map.file.version.clone()
        };
        self.spell_classes_for(&version);
        let uses = self.tileset_uses(map);
        self.gids = FirstGids::assign(&map.tilesets, &uses);
        for change in std::mem::take(&mut self.gids.renumbered) {
            self.warnings.record_about(
                WarningKind::Renumbered,
                Some(Target::Tileset(change.name.clone())),
                format!(
                    "tileset “{}” starts at global id {} instead of {}, so that its tiles keep their own range",
                    change.name, change.to, change.from
                ),
            );
        }
        let mut raw = RawMap {
            tiled_version: Some(Scalar::text(if map.file.tiled_version.is_empty() {
                names::TILED_VERSION
            } else {
                &map.file.tiled_version
            })),
            version: Some(Scalar::Text(version)),
            class: Some(Scalar::text(&map.class)),
            orientation: Some(Scalar::text(names::orientation_name(map.orientation))),
            render_order: Some(Scalar::text(names::render_order_name(map.render_order))),
            compression_level: map.file.compression_level.map(Scalar::int),
            width: Some(Scalar::int(map.width)),
            height: Some(Scalar::int(map.height)),
            tile_width: Some(Scalar::int(map.tile_width)),
            tile_height: Some(Scalar::int(map.tile_height)),
            infinite: Some(Scalar::Bool(map.infinite)),
            hex_side_length: Some(Scalar::int(map.hex_side_length)),
            stagger_axis: Some(Scalar::text(names::stagger_axis_name(map.stagger_axis))),
            stagger_index: Some(Scalar::text(names::stagger_index_name(map.stagger_index))),
            skew_x: Some(Scalar::int(map.skew.x)),
            skew_y: Some(Scalar::int(map.skew.y)),
            parallax_origin_x: Some(Scalar::Real(map.parallax_origin.x)),
            parallax_origin_y: Some(Scalar::Real(map.parallax_origin.y)),
            background_color: map
                .background_color
                .map(|c| Scalar::Text(c.to_tiled_text())),
            next_layer_id: Some(Scalar::int(map.next_layer_id)),
            next_object_id: Some(Scalar::int(map.next_object_id)),
            chunk_size: (map.file.output_chunk_size != UVec2::splat(16)).then(|| RawSize {
                width: Some(Scalar::int(map.file.output_chunk_size.x)),
                height: Some(Scalar::int(map.file.output_chunk_size.y)),
            }),
            export: (!map.file.export.is_empty()).then(|| RawExport {
                target: map
                    .file
                    .export
                    .target
                    .as_ref()
                    .map(|t| Scalar::Text(self.reference(t))),
                format: Some(Scalar::text(&map.file.export.format)),
            }),
            properties: raw_properties(&map.properties),
            tilesets: Vec::new(),
            layers: Vec::new(),
        };
        for (index, tileset) in map.tilesets.iter().enumerate() {
            let Some(first) = self.gids.first_of(index) else {
                continue;
            };
            let mut raw_tileset = match &tileset.file {
                Some(file) => RawTileset {
                    source: Some(Scalar::Text(self.reference(file))),
                    ..RawTileset::default()
                },
                None => self.tileset_contents_of(&tileset.tileset),
            };
            raw_tileset.first_gid = Some(Scalar::int(first));
            raw.tilesets.push(raw_tileset);
        }
        raw.layers = self.layers_of(&map.layers, map);
        if self.gids.unnamed > 0 {
            self.warnings.record(
                WarningKind::UnknownGid,
                format!(
                    "{} tiles belong to no tileset of the map; they are written as empty",
                    self.gids.unnamed
                ),
            );
        }
        raw
    }

    /// What the map's written data uses of each tileset.
    fn tileset_uses(&mut self, map: &Map) -> Vec<TilesetUse> {
        let mut uses = vec![TilesetUse::default(); map.tilesets.len()];
        let mut objects = Vec::new();
        map.for_each_layer(|layer, _| match &layer.kind {
            LayerKind::Tile(tiles) => {
                for placed in tiles.cells() {
                    note_tile(&mut uses, placed.cell);
                }
            }
            LayerKind::Object(group) => {
                objects.extend(group.objects.iter().filter(|o| o.tile.is_some()))
            }
            LayerKind::Image(_) | LayerKind::Group(_) => {}
        });
        for object in objects {
            let Some(cell) = object.tile else {
                continue;
            };
            if object.template.is_none() || self.instance_tile_is_own(object, map) {
                note_tile(&mut uses, cell);
            } else if let Some(use_) = cell.slot().and_then(|slot| uses.get_mut(slot.index())) {
                use_.inherited = true;
            }
        }
        uses
    }

    fn instance_tile_is_own(&mut self, object: &MapObject, map: &Map) -> bool {
        if object.overrides.contains(ObjectOverrides::TILE) {
            return true;
        }
        let base = object
            .template
            .as_ref()
            .and_then(|t| self.instance_base(t, Some(map)));
        base.is_some_and(|base| base.tile != object.tile)
    }

    /// What an instance of `template` takes, with its tile in `map`'s tilesets (none for a
    /// collision shape) and sized as the loader sizes tile objects; `None` when the loader does
    /// not hold the template.
    pub(crate) fn instance_base(
        &mut self,
        template: &FileRef,
        map: Option<&Map>,
    ) -> Option<MapObject> {
        let key = template.path.clone();
        if map.is_none() {
            // Collision shapes are few; their bases are not cached separately.
            return self.base_from_loader(template, None);
        }
        if let Some(base) = self.bases.get(&key) {
            return base.clone();
        }
        let base = self.base_from_loader(template, map);
        self.bases.insert(key, base.clone());
        base
    }

    fn base_from_loader(&self, template: &FileRef, map: Option<&Map>) -> Option<MapObject> {
        let loaded = self.templates?.cached_template(&template.path)?;
        let mut object = loaded.object.clone()?;
        let Some(map) = map else {
            object.tile = None;
            return Some(object);
        };
        object.tile = object.tile.and_then(|cell| {
            let tileset = loaded.tilesets.get(cell.slot()?.index())?;
            let index = map.tilesets.iter().position(|own| {
                std::sync::Arc::ptr_eq(&own.tileset, &tileset.tileset)
                    || (own.file.is_some()
                        && own.file.as_ref().map(|f| &f.path)
                            == tileset.file.as_ref().map(|f| &f.path))
            })?;
            Some(
                bt_core::Cell::new(bt_core::TilesetSlot::new(index)?, cell.tile())
                    .with_flip(cell.flip()),
            )
        });
        if let Some(cell) = object.tile
            && let Some(tileset) = map.tileset_of(cell)
        {
            let size = tileset.tile_pixel_size(cell.tile()).as_dvec2();
            if object.size.x == 0.0 {
                object.size.x = size.x;
            }
            if object.size.y == 0.0 {
                object.size.y = size.y;
            }
        }
        Some(object)
    }
}

/// Notes a cell's tile in the uses of its tileset.
fn note_tile(uses: &mut [TilesetUse], cell: bt_core::Cell) {
    if let Some(use_) = cell.slot().and_then(|slot| uses.get_mut(slot.index())) {
        use_.highest_tile = Some(
            use_.highest_tile
                .map_or(cell.tile(), |t| t.max(cell.tile())),
        );
    }
}
