//! Tilesets back into the raw tree.

use bt_core::{
    GridOrientation, ImageRef, ImageSource, Tileset, TilesetImage, Transformations, WangSet,
};
use bt_utils::IRect;
use glam::IVec2;

use super::map::Unfinisher;
use super::property::raw_properties;
use crate::data::encode_base64_bytes;
use crate::names;
use crate::raw::{
    RawExport, RawFrame, RawGrid, RawImage, RawImageData, RawLayer, RawLayerKind, RawPoint,
    RawTile, RawTileset, RawTransformations, RawWangColor, RawWangId, RawWangSet, RawWangTile,
    Scalar, WangColorRole,
};

impl Unfinisher<'_> {
    /// A tileset file; `new_version` is the format version for one that has none.
    pub(crate) fn tileset_file_of(&mut self, tileset: &Tileset, new_version: &str) -> RawTileset {
        let version = if tileset.file.version.is_empty() {
            new_version.to_owned()
        } else {
            tileset.file.version.clone()
        };
        self.spell_classes_for(&version);
        let mut raw = self.tileset_contents_of(tileset);
        raw.version = Some(Scalar::Text(version));
        raw.tiled_version = Some(Scalar::text(if tileset.file.tiled_version.is_empty() {
            names::TILED_VERSION
        } else {
            &tileset.file.tiled_version
        }));
        if !tileset.file.export.is_empty() {
            raw.export = Some(RawExport {
                target: Some(Scalar::Text(
                    tileset
                        .file
                        .export
                        .target
                        .as_ref()
                        .map(|t| self.reference(t))
                        .unwrap_or_default(),
                )),
                format: Some(Scalar::text(&tileset.file.export.format)),
            });
        }
        raw
    }

    /// A tileset's contents, for a tileset file or a tileset embedded in a map.
    pub(crate) fn tileset_contents_of(&mut self, tileset: &Tileset) -> RawTileset {
        let counts = written_counts(tileset);
        let listing = tile_listing(tileset, counts.grid);
        RawTileset {
            name: Some(Scalar::text(&tileset.name)),
            class: Some(Scalar::text(&tileset.class)),
            tile_width: Some(Scalar::int(tileset.tile_width)),
            tile_height: Some(Scalar::int(tileset.tile_height)),
            spacing: Some(Scalar::int(tileset.spacing)),
            margin: Some(Scalar::int(tileset.margin)),
            tile_count: Some(Scalar::int(counts.tile_count)),
            columns: Some(Scalar::int(counts.columns)),
            background_color: tileset
                .background_color
                .map(|c| Scalar::Text(c.to_tiled_text())),
            object_alignment: Some(Scalar::text(names::alignment_name(
                tileset.object_alignment,
            ))),
            tile_render_size: Some(Scalar::text(names::tile_render_size_name(
                tileset.tile_render_size,
            ))),
            fill_mode: Some(Scalar::text(names::fill_mode_name(tileset.fill_mode))),
            tile_offset: (tileset.tile_offset != IVec2::ZERO).then(|| RawPoint {
                x: Some(Scalar::int(tileset.tile_offset.x)),
                y: Some(Scalar::int(tileset.tile_offset.y)),
            }),
            grid: tileset
                .grid
                .filter(|grid| {
                    grid.orientation != GridOrientation::Orthogonal
                        || grid.size != tileset.tile_size()
                })
                .map(|grid| RawGrid {
                    orientation: Some(Scalar::text(names::grid_orientation_name(grid.orientation))),
                    width: Some(Scalar::int(grid.size.x)),
                    height: Some(Scalar::int(grid.size.y)),
                }),
            transformations: (!tileset.transformations.is_empty()).then(|| {
                let flag = |flag| Some(Scalar::Bool(tileset.transformations.contains(flag)));
                RawTransformations {
                    hflip: flag(Transformations::HFLIP),
                    vflip: flag(Transformations::VFLIP),
                    rotate: flag(Transformations::ROTATE),
                    prefer_untransformed: flag(Transformations::PREFER_UNTRANSFORMED),
                }
            }),
            properties: raw_properties(&tileset.properties),
            image: match &tileset.image {
                TilesetImage::Atlas { image, .. } => Some(self.image_of(image, true)),
                TilesetImage::Collection => None,
            },
            tiles: self.tiles_of(tileset, &listing),
            wang_sets: tileset.wang_sets.iter().map(wang_set_of).collect(),
            lists_every_tile: listing.every_tile,
            ..RawTileset::default()
        }
    }

    /// An image reference; `with_trans` keeps the transparent colour (not on collection tiles).
    pub(crate) fn image_of(&self, image: &ImageRef, with_trans: bool) -> RawImage {
        let mut raw = RawImage {
            trans: image
                .transparent
                .filter(|_| with_trans)
                .map(|c| Scalar::Text(format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b))),
            width: Some(Scalar::int(image.declared_size.x)),
            height: Some(Scalar::int(image.declared_size.y)),
            ..RawImage::default()
        };
        match &image.source {
            ImageSource::File(file) => raw.source = Some(Scalar::Text(self.reference(file))),
            ImageSource::Embedded(embedded) => {
                raw.format = Some(Scalar::text(&embedded.format));
                raw.data = Some(RawImageData {
                    encoding: Some(Scalar::text("base64")),
                    text: encode_base64_bytes(&embedded.bytes),
                });
            }
        }
        raw
    }

    /// The tile entries in the order Tiled lists them.
    fn tiles_of(&mut self, tileset: &Tileset, listing: &TileListing) -> Vec<RawTile> {
        let collection = tileset.is_collection();
        let empty = bt_core::TileData::default();
        let mut out = Vec::with_capacity(listing.ids.len());
        for &id in &listing.ids {
            let tile = tileset.tiles.get(&id).unwrap_or(&empty);
            let class = Some(Scalar::text(&tile.class));
            let rect = tile
                .rect
                .filter(|rect| collection && !rect.is_empty())
                .filter(|rect| {
                    let full = tile.image.as_ref().map(|image| {
                        IRect::from_min_size(IVec2::ZERO, image.declared_size.as_ivec2())
                    });
                    full != Some(*rect)
                });
            out.push(RawTile {
                id: Some(Scalar::int(id.0)),
                class: if self.class_as_type {
                    None
                } else {
                    class.clone()
                },
                class_as_type: if self.class_as_type { class } else { None },
                probability: Some(Scalar::Real(tile.probability)),
                x: rect.map(|r| Scalar::int(r.x)),
                y: rect.map(|r| Scalar::int(r.y)),
                width: rect.map(|r| Scalar::int(r.w)),
                height: rect.map(|r| Scalar::int(r.h)),
                terrain: None,
                properties: raw_properties(&tile.properties),
                image: tile
                    .image
                    .as_ref()
                    .filter(|_| collection)
                    .map(|image| self.image_of(image, false)),
                collision: tile.collision.as_ref().map(|group| {
                    let mut layer = RawLayer {
                        kind: RawLayerKind::Object,
                        id: Some(Scalar::int(group.id.0)),
                        ..RawLayer::default()
                    };
                    self.object_layer_into(&group.layer, &mut layer, None);
                    layer
                }),
                animation: (!tile.animation.is_empty()).then(|| {
                    tile.animation
                        .iter()
                        .map(|frame| RawFrame {
                            tile_id: Some(Scalar::int(frame.tile.0)),
                            duration: Some(Scalar::int(frame.duration_ms)),
                        })
                        .collect()
                }),
            });
        }
        out
    }
}

/// The columns and tile count written: for an atlas, those of the image size the file states
/// (its own count, as Tiled counts the image it loaded, plus listed tiles past it); for a
/// collection, no columns and its tiles.
struct WrittenCounts {
    columns: u32,
    tile_count: u32,
    /// The tiles of an atlas's image grid.
    grid: u32,
}

/// The tiles Tiled lists, in its order.
struct TileListing {
    ids: Vec<bt_core::TileId>,
    every_tile: bool,
}

/// Tiled lists a collection's every tile in display order; an atlas's tiles with data by id,
/// unless the ids of its tiles (the grid's, then those past it) leave a gap, when it lists them
/// all.
fn tile_listing(tileset: &Tileset, grid: u32) -> TileListing {
    if tileset.is_collection() {
        return TileListing {
            ids: tileset.tile_ids(),
            every_tile: true,
        };
    }
    let beyond: Vec<bt_core::TileId> = tileset
        .tiles
        .keys()
        .copied()
        .filter(|id| id.0 >= grid)
        .collect();
    let gap = beyond
        .iter()
        .zip(grid..)
        .any(|(id, expected)| id.0 != expected);
    if gap {
        TileListing {
            ids: (0..grid).map(bt_core::TileId).chain(beyond).collect(),
            every_tile: true,
        }
    } else {
        TileListing {
            ids: tileset.tiles.keys().copied().collect(),
            every_tile: false,
        }
    }
}

fn written_counts(tileset: &Tileset) -> WrittenCounts {
    match &tileset.image {
        TilesetImage::Collection => WrittenCounts {
            columns: 0,
            tile_count: u32::try_from(tileset.tiles.len()).unwrap_or(u32::MAX),
            grid: 0,
        },
        TilesetImage::Atlas {
            image,
            columns,
            tile_count,
        } => {
            let declared = image.declared_size;
            let from_declared = declared.x > 0
                && declared.y > 0
                && tileset.tile_width > 0
                && tileset.tile_height > 0;
            let columns = if from_declared {
                tileset.columns_for_width(declared.x)
            } else {
                *columns
            };
            let grid = if from_declared {
                columns.saturating_mul(tileset.rows_for_height(declared.y))
            } else {
                *tile_count
            };
            let beyond =
                u32::try_from(tileset.tiles.keys().filter(|id| id.0 >= grid).count()).unwrap_or(0);
            WrittenCounts {
                columns,
                tile_count: grid.saturating_add(beyond),
                grid,
            }
        }
    }
}

/// A terrain set.
fn wang_set_of(set: &WangSet) -> RawWangSet {
    RawWangSet {
        name: Some(Scalar::text(&set.name)),
        class: Some(Scalar::text(&set.class)),
        kind: Some(Scalar::text(names::wang_set_type_name(set.kind))),
        tile: Some(set.tile.map_or(Scalar::Int(-1), |t| Scalar::int(t.0))),
        properties: raw_properties(&set.properties),
        colors: set
            .colors
            .iter()
            .map(|color| RawWangColor {
                role: WangColorRole::Any,
                name: Some(Scalar::text(&color.name)),
                class: Some(Scalar::text(&color.class)),
                color: Some(Scalar::Text(color.color.to_tiled_text())),
                tile: Some(color.tile.map_or(Scalar::Int(-1), |t| Scalar::int(t.0))),
                probability: Some(Scalar::Real(color.probability)),
                properties: raw_properties(&color.properties),
            })
            .collect(),
        tiles: set
            .tiles
            .iter()
            .map(|(tile, id)| RawWangTile {
                tile_id: Some(Scalar::int(tile.0)),
                wang_id: Some(RawWangId::Lanes(
                    id.lanes().iter().map(|&lane| Scalar::int(lane)).collect(),
                )),
            })
            .collect(),
    }
}
