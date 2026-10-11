//! `bt new` (`10-agents.md`, §10.7): an empty map like another (its tilesets and layers,
//! emptied, its properties cleared, the file properties of what it keeps rebased for the new
//! map's folder, and a hint for each layer the other filled with one tile), or from a size, a
//! tile size, tilesets and `name:kind` layers. Reading the other map and the tilesets, and
//! writing the new file, are the caller's.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_core::{
    Cell, Layer, LayerDataFormat, LayerId, LayerKind, Map, TileLayer, TilesetRef, TilesetSlot,
};
use bt_edit::{LayerType, add_tileset, new_layer};
use glam::UVec2;
use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::layer_tree::layers_top_first;
use crate::names::Names;
use crate::property_text::quoted;
use crate::report::{Line, Outcome, Report, Section};

/// The share of the map's cells one tile must fill for a layer to have been filled with it, in
/// tenths.
const FILLED_TENTHS: usize = 9;

/// What `bt new` makes.
pub struct NewRequest<'a> {
    file: String,
    source: Source<'a>,
    size: Option<UVec2>,
    tilesets: Vec<TilesetRef>,
    layers: Vec<String>,
}

/// What a new map starts from.
enum Source<'a> {
    /// Another map, emptied.
    Like {
        other: &'a Map,
        from_dir: PathBuf,
        to_dir: PathBuf,
    },
    /// A tile size.
    Fresh { tile: UVec2 },
}

impl<'a> NewRequest<'a> {
    /// A map called `file`, in the folder `new_dir`, like `other`, which is in `other_dir`.
    pub fn like(
        file: impl Into<String>,
        other: &'a Map,
        other_dir: &Path,
        new_dir: &Path,
    ) -> NewRequest<'a> {
        NewRequest {
            file: file.into(),
            source: Source::Like {
                other,
                from_dir: other_dir.to_path_buf(),
                to_dir: new_dir.to_path_buf(),
            },
            size: None,
            tilesets: Vec::new(),
            layers: Vec::new(),
        }
    }

    /// A map called `file`, `size` cells of `tile` pixels.
    pub fn sized(file: impl Into<String>, size: UVec2, tile: UVec2) -> NewRequest<'a> {
        NewRequest {
            file: file.into(),
            source: Source::Fresh { tile },
            size: Some(size),
            tilesets: Vec::new(),
            layers: Vec::new(),
        }
    }

    /// The size in cells (a map like another keeps the other's unless told).
    pub fn size(mut self, size: UVec2) -> NewRequest<'a> {
        self.size = Some(size);
        self
    }

    /// Tilesets the new map uses, read by the caller.
    pub fn tilesets(mut self, tilesets: Vec<TilesetRef>) -> NewRequest<'a> {
        self.tilesets = tilesets;
        self
    }

    /// Layers as `name:tiles` or `name:objects` (`name` alone is tiles), the first at the bottom.
    pub fn layers(mut self, layers: Vec<String>) -> NewRequest<'a> {
        self.layers = layers;
        self
    }
}

/// A new map and what `bt new` says of it.
#[derive(Clone, Debug)]
pub struct Created {
    /// The map, to write.
    pub map: Map,
    /// What `bt new` reports.
    pub outcome: NewOutcome,
}

/// `bt new MAP`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewOutcome {
    /// The new map's file name.
    pub file: String,
    /// Its size in cells.
    pub size: UVec2,
    /// Its tile size in pixels.
    pub tile_size: UVec2,
    /// Whether it is infinite.
    pub infinite: bool,
    /// Its layers, the top one first: `props (objects)`.
    pub layers: Vec<String>,
    /// Its tilesets, by name.
    pub tilesets: Vec<String>,
    /// The layers the other map filled with one tile, emptied too.
    pub refill: Vec<Refill>,
}

/// A layer the other map filled with one tile: a backdrop to fill again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refill {
    /// The layer.
    pub layer: String,
    /// The tile, as scripts name it.
    pub tile: String,
}

impl NewOutcome {
    /// The map `request` asks for.
    pub fn from_request(request: &NewRequest<'_>) -> Result<Created, Error> {
        if let Some(size) = request.size.filter(|s| s.x == 0 || s.y == 0) {
            return Err(Error::ZeroSize {
                what: "--size",
                size,
            });
        }
        let mut refill = Vec::new();
        let mut map = match &request.source {
            Source::Like {
                other,
                from_dir,
                to_dir,
            } => {
                refill = filled_layers(other);
                let mut map = Map::clone(other);
                map.properties.clear();
                map.file.export = bt_core::ExportSettings::default();
                let size = request.size.unwrap_or(UVec2::new(map.width, map.height));
                map.width = size.x;
                map.height = size.y;
                empty_layers(&mut map.layers, size);
                map.next_object_id = 1;
                bt_tmx::rebase_file_references(&mut map, from_dir, to_dir);
                map
            }
            Source::Fresh { tile } => {
                if tile.x == 0 || tile.y == 0 {
                    return Err(Error::ZeroSize {
                        what: "--tile",
                        size: *tile,
                    });
                }
                let size = request.size.unwrap_or(UVec2::ONE);
                let mut map = Map {
                    width: size.x,
                    height: size.y,
                    tile_width: tile.x,
                    tile_height: tile.y,
                    ..Map::default()
                };
                map.file.layer_data = LayerDataFormat::Csv;
                map
            }
        };
        for tileset in &request.tilesets {
            add_tileset(&mut map, tileset.clone()).map_err(|error| Error::NotCreated {
                why: error.to_string(),
            })?;
        }
        add_layers(&mut map, &request.layers)?;
        let names = Names::of(&map);
        let layers = layers_top_first(&map)
            .iter()
            .map(|at| {
                let kind = match at.layer.kind {
                    LayerKind::Tile(_) => "tiles",
                    LayerKind::Object(_) => "objects",
                    LayerKind::Image(_) => "image",
                    LayerKind::Group(_) => "group",
                };
                format!("{}{} ({kind})", "  ".repeat(at.depth), at.layer.name)
            })
            .collect();
        let tilesets = (0..map.tilesets.len())
            .filter_map(TilesetSlot::new)
            .filter_map(|slot| names.tileset_name(slot).map(str::to_owned))
            .collect();
        let outcome = NewOutcome {
            file: request.file.clone(),
            size: UVec2::new(map.width, map.height),
            tile_size: map.tile_size(),
            infinite: map.infinite,
            layers,
            tilesets,
            refill,
        };
        Ok(Created { map, outcome })
    }
}

/// The tile layers of `map` one tile fills nine tenths of, with the tile.
fn filled_layers(map: &Map) -> Vec<Refill> {
    let names = Names::of(map);
    let area = usize::try_from(u64::from(map.width) * u64::from(map.height)).unwrap_or(usize::MAX);
    let mut refill = Vec::new();
    for at in layers_top_first(map) {
        let Some(tiles) = at.layer.as_tile_layer() else {
            continue;
        };
        let mut counts: std::collections::HashMap<Cell, usize> = std::collections::HashMap::new();
        for placed in tiles.cells() {
            *counts.entry(placed.cell).or_default() += 1;
        }
        let most = counts
            .into_iter()
            .max_by_key(|(tile, count)| (*count, tile.tile(), tile.flip().bits()));
        if let Some((tile, count)) = most
            && area > 0
            && count * 10 >= area * FILLED_TENTHS
        {
            refill.push(Refill {
                layer: at.layer.name.clone(),
                tile: names.tile_text(tile),
            });
        }
    }
    refill
}

/// Every layer emptied: tile layers of `size` (or infinite, as they were), object layers
/// without objects; image layers and groups as they are.
fn empty_layers(layers: &mut [Layer], size: UVec2) {
    for layer in layers {
        match &mut layer.kind {
            LayerKind::Tile(tiles) => {
                *tiles = if tiles.is_infinite() {
                    TileLayer::infinite(size)
                } else {
                    TileLayer::finite(size)
                };
            }
            LayerKind::Object(objects) => objects.objects = Arc::new(Vec::new()),
            LayerKind::Group(children) => empty_layers(children, size),
            LayerKind::Image(_) => {}
        }
    }
}

/// Adds `name:kind` layers on top, in order; a name the map has is left as it is.
fn add_layers(map: &mut Map, specs: &[String]) -> Result<(), Error> {
    for spec in specs.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        let (name, kind) = spec.split_once(':').unwrap_or((spec, "tiles"));
        let kind = match kind.trim() {
            "tiles" | "tile" => LayerType::Tile,
            "objects" | "object" => LayerType::Object,
            _ => {
                return Err(Error::LayerSpec {
                    spec: spec.to_owned(),
                });
            }
        };
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::LayerSpec {
                spec: spec.to_owned(),
            });
        }
        if map.layers.iter().any(|layer| layer.name == name) {
            continue;
        }
        let top: Option<LayerId> = map.layers.last().map(|layer| layer.id);
        let id = new_layer(map, kind, top).map_err(|error| Error::NotCreated {
            why: error.to_string(),
        })?;
        if let Some(layer) = map.layer_mut(id) {
            layer.name = name.to_owned();
        }
    }
    Ok(())
}

impl Outcome for NewOutcome {
    const SCHEMA: &'static str = "bt.new/1";

    fn report(&self) -> Report {
        let size = if self.infinite {
            "infinite".to_owned()
        } else {
            format!("{}x{} cells", self.size.x, self.size.y)
        };
        let mut section = Section::headed(format!(
            "created {}: {size} of {}x{} px",
            self.file, self.tile_size.x, self.tile_size.y
        ));
        section.add_line(Line::fact(format!(
            "layers (top first): {}",
            if self.layers.is_empty() {
                "none (add some with the edit step `layer add NAME tiles`)".to_owned()
            } else {
                self.layers
                    .iter()
                    .map(|l| l.trim_start().to_owned())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        )));
        section.add_line(Line::fact(format!(
            "tilesets: {}",
            if self.tilesets.is_empty() {
                "none".to_owned()
            } else {
                self.tilesets.join(", ")
            }
        )));
        for refill in &self.refill {
            section.add_line(Line::fact(format!(
                "{} was all {} in the original: fill it again with `tile {} all layer {}`",
                refill.layer,
                refill.tile,
                refill.tile,
                quoted(&refill.layer)
            )));
        }
        let mut report = Report::new();
        report.add_section(section);
        report
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{MapObject, Property, Value};
    use bt_test_data::fixtures::{Atlas, MapBuilder};

    use super::*;
    use crate::report::{from_json, to_json};

    #[test]
    fn a_map_like_another_keeps_its_tilesets_and_layers_emptied() {
        let atlas = Atlas::builder().name("ground").build_embedded().unwrap();
        let mut other = MapBuilder::new(UVec2::splat(16), UVec2::new(4, 3))
            .tileset(Arc::clone(&atlas.tileset))
            .tile_layer("sky", "5 5 5 5\n5 5 5 5\n5 5 5 5")
            .tile_layer("ground", "1 2 . .\n. . . .\n. . . .")
            .object_layer("props", vec![MapObject::default()])
            .build()
            .unwrap();
        other.properties.insert(
            "intent".into(),
            Property::new(Value::String("a pond".into())),
        );
        let request = NewRequest::like("copy.tmx", &other, Path::new("/maps"), Path::new("/maps"))
            .size(UVec2::new(8, 6));
        let created = NewOutcome::from_request(&request).unwrap();
        let map = &created.map;
        assert!(map.properties.is_empty());
        assert_eq!((map.width, map.height), (8, 6));
        assert!(map.layers.iter().all(|layer| match &layer.kind {
            LayerKind::Tile(tiles) =>
                tiles.cells().next().is_none() && tiles.size() == UVec2::new(8, 6),
            LayerKind::Object(objects) => objects.objects.is_empty(),
            _ => true,
        }));
        assert_eq!(
            created.outcome.report().text(),
            "created copy.tmx: 8x6 cells of 16x16 px\n  \
             layers (top first): props (objects), ground (tiles), sky (tiles)\n  \
             tilesets: ground\n  \
             sky was all ground:5 in the original: fill it again with `tile ground:5 all layer sky`\n"
        );
        assert_eq!(
            from_json::<NewOutcome>(&to_json(&created.outcome).unwrap()).unwrap(),
            created.outcome
        );
    }

    #[test]
    fn a_map_like_tileds_desert_keeps_its_tileset_file() {
        let Some(examples) = bt_test_data::test_data("reference/tiled/examples") else {
            return;
        };
        let mut loader = bt_tmx::Loader::default();
        let desert = loader.load_map(&examples.join("desert.tmx")).unwrap().value;
        let elsewhere = examples.join("elsewhere");
        let created = NewOutcome::from_request(&NewRequest::like(
            "oasis.tmx",
            &desert,
            &examples,
            &elsewhere,
        ))
        .unwrap();
        assert_eq!(created.outcome.tilesets, ["desert"]);
        assert_eq!(created.outcome.layers, ["Ground (tiles)"]);
        assert!(
            created.outcome.refill.is_empty(),
            "the desert is not all one tile"
        );
        assert_eq!(
            created.map.tilesets[0].file, desert.tilesets[0].file,
            "the tileset is still its file"
        );
    }

    #[test]
    fn a_map_from_a_size_has_its_tilesets_and_layers_the_first_at_the_bottom() {
        let atlas = Atlas::builder()
            .name("Ground Tiles")
            .build_embedded()
            .unwrap();
        let request = NewRequest::sized("fresh.tmx", UVec2::new(10, 5), UVec2::splat(16))
            .tilesets(vec![TilesetRef::embedded(Arc::clone(&atlas.tileset))])
            .layers(vec!["floor:tiles".into(), "things:objects".into()]);
        let created = NewOutcome::from_request(&request).unwrap();
        assert_eq!(created.map.file.layer_data, LayerDataFormat::Csv);
        assert_eq!(
            created.outcome.layers,
            ["things (objects)", "floor (tiles)"]
        );
        assert_eq!(created.outcome.tilesets, ["ground-tiles"]);
        let floor = created
            .map
            .layers
            .iter()
            .find(|l| l.name == "floor")
            .unwrap();
        assert_eq!(floor.as_tile_layer().unwrap().size(), UVec2::new(10, 5));
        assert_eq!(
            NewOutcome::from_request(
                &NewRequest::sized("x.tmx", UVec2::new(2, 2), UVec2::splat(16))
                    .layers(vec!["floor:water".into()])
            )
            .unwrap_err()
            .to_string(),
            "--layers floor:water: a layer is NAME:tiles or NAME:objects"
        );
        assert!(matches!(
            NewOutcome::from_request(&NewRequest::sized(
                "x.tmx",
                UVec2::new(0, 2),
                UVec2::splat(16)
            )),
            Err(Error::ZeroSize { what: "--size", .. })
        ));
    }
}
