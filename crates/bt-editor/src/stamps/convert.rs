//! Saved stamp maps and brushes share the clipboard's cells, layer names and tileset identity.

use bt_core::{Layer, LayerId, LayerKind, Map, TileLayer};
use bt_edit::{Brush, TileClip, TileRegion, Variation, copy};
use bt_tmx::{Stamp, StampVariation};
use glam::UVec2;

pub(super) fn brush_of(
    stamp: &Stamp,
    variation: Option<usize>,
    target: Option<&Map>,
) -> Result<Brush, String> {
    if variation.is_some_and(|index| index >= stamp.variations.len()) {
        return Err("The stamp no longer has that variation".into());
    }
    let mut variations = Vec::new();
    for (index, saved) in stamp.variations.iter().enumerate() {
        if variation.is_some_and(|chosen| chosen != index) {
            continue;
        }
        if !saved.probability.is_finite() || saved.probability < 0.0 {
            return Err("Stamp probabilities must be finite and non-negative".into());
        }
        let mut layers = Vec::new();
        saved.map.for_each_layer(|layer, _| {
            if layer.as_tile_layer().is_some() {
                layers.push(layer.id);
            }
        });
        let mut clip = copy(
            &saved.map,
            &layers,
            &TileRegion::from_rect(saved.map.tile_bounding_rect()),
        )
        .map_err(|error| error.to_string())?;
        if let Some(target) = target
            && saved.map.stagger_index != target.stagger_index
            && matches!(
                target.orientation,
                bt_core::Orientation::Hexagonal | bt_core::Orientation::Staggered
            )
        {
            clip.origin = match target.stagger_axis {
                bt_core::StaggerAxis::X => glam::IVec2::X,
                bt_core::StaggerAxis::Y => glam::IVec2::Y,
            };
        }
        variations.push(Variation {
            probability: if variation.is_some() {
                1.0
            } else {
                saved.probability
            },
            clip,
        });
    }
    if variations.is_empty()
        || !variations
            .iter()
            .any(|variation| variation.probability > 0.0 && !variation.clip.is_empty())
    {
        return Err("The stamp has no paintable variation".into());
    }
    Brush::from_variations(variations).map_err(|error| error.to_string())
}

pub(super) fn stamp_of(name: String, brush: &Brush, source: &Map) -> Result<Stamp, String> {
    let variations = brush
        .variations()
        .iter()
        .map(|variation| {
            Ok(StampVariation {
                probability: variation.probability,
                map: map_of(&variation.clip, source)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let stamp = Stamp {
        name,
        quick_stamp_index: None,
        variations,
    };
    brush_of(&stamp, None, None)?;
    Ok(stamp)
}

fn map_of(clip: &TileClip, source: &Map) -> Result<Map, String> {
    let size = UVec2::new(
        u32::try_from(clip.size.x).map_err(|error| error.to_string())?,
        u32::try_from(clip.size.y).map_err(|error| error.to_string())?,
    );
    let mut map = source.clone();
    map.width = size.x;
    map.height = size.y;
    let stagger_offset = match source.stagger_axis {
        bt_core::StaggerAxis::X => clip.origin.x,
        bt_core::StaggerAxis::Y => clip.origin.y,
    };
    if stagger_offset.rem_euclid(2) == 1 {
        map.stagger_index = match source.stagger_index {
            bt_core::StaggerIndex::Odd => bt_core::StaggerIndex::Even,
            bt_core::StaggerIndex::Even => bt_core::StaggerIndex::Odd,
        };
    }
    map.infinite = false;
    map.tilesets = clip.tilesets.clone();
    map.layers.clear();
    map.properties.clear();
    map.class.clear();
    map.background_color = None;
    map.file.export = Default::default();
    map.next_object_id = 1;
    for part in &clip.layers {
        let mut cells = TileLayer::finite(size);
        for placed in part.cells.cells() {
            cells
                .set_cell(placed.at, placed.cell)
                .map_err(|error| error.to_string())?;
        }
        let id = u32::try_from(map.layers.len() + 1).map_err(|error| error.to_string())?;
        let mut layer = Layer::new(LayerId(id), &part.name, LayerKind::Tile(cells));
        layer.opacity = part.opacity;
        layer.tint = part.tint;
        map.layers.push(layer);
    }
    map.next_layer_id = u32::try_from(map.layers.len() + 1).map_err(|error| error.to_string())?;
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use bt_utils::IRect;
    use glam::IVec2;

    #[test]
    fn stamps_captured_on_odd_rows_or_columns_keep_their_staggered_shape() {
        let atlas = Atlas::builder().build_embedded().unwrap();
        for orientation in [
            bt_core::Orientation::Hexagonal,
            bt_core::Orientation::Staggered,
        ] {
            for axis in [bt_core::StaggerAxis::X, bt_core::StaggerAxis::Y] {
                for index in [bt_core::StaggerIndex::Odd, bt_core::StaggerIndex::Even] {
                    let mut map = MapBuilder::new(UVec2::splat(16), UVec2::splat(4))
                        .tileset(atlas.tileset.clone())
                        .tile_layer("ground", "1 1 1 1\n1 2 3 1\n1 4 5 1\n1 1 1 1")
                        .build()
                        .unwrap();
                    map.orientation = orientation;
                    map.stagger_axis = axis;
                    map.stagger_index = index;
                    map.hex_side_length = 8;
                    let brush = Brush::capture(
                        &map,
                        &[map.layers[0].id],
                        IRect::from_min_size(IVec2::ONE, IVec2::splat(2)),
                    )
                    .unwrap();
                    let stamp = stamp_of("odd".into(), &brush, &map).unwrap();
                    let restored = brush_of(&stamp, None, Some(&map)).unwrap();
                    for point in [IVec2::splat(2), IVec2::splat(3)] {
                        let first = brush
                            .placement(&[point], &map.geometry(), &mut bt_utils::rng::Rng::new(1))
                            .unwrap();
                        let second = restored
                            .placement(&[point], &map.geometry(), &mut bt_utils::rng::Rng::new(1))
                            .unwrap();
                        assert_eq!(
                            first.layers, second.layers,
                            "{orientation:?} {axis:?} {index:?} {point}"
                        );
                        assert_eq!(first.origin, second.origin);
                    }
                }
            }
        }
    }

    #[test]
    fn weighted_multilayer_brushes_round_trip_as_tiled_stamps() {
        let atlas = Atlas::builder().build_embedded().unwrap();
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(2, 1))
            .tileset(atlas.tileset)
            .tile_layer("ground", "1 .")
            .tile_layer("detail", ". 2h")
            .build()
            .unwrap();
        let layers = map.layers.iter().map(|layer| layer.id).collect::<Vec<_>>();
        let clip = copy(
            &map,
            &layers,
            &TileRegion::from_rect(IRect::from_min_size(IVec2::ZERO, IVec2::new(2, 1))),
        )
        .unwrap();
        let brush = Brush::from_variations(vec![
            Variation {
                clip: clip.clone(),
                probability: 0.25,
            },
            Variation {
                clip: clip.clone(),
                probability: 2.5,
            },
        ])
        .unwrap();
        let stamp = stamp_of("hill".into(), &brush, &map).unwrap();
        let json = bt_tmx::WriteRequest::for_stamp(&stamp, std::path::Path::new("/tmp/hill.stamp"))
            .render()
            .unwrap()
            .text;
        let read = bt_tmx::Loader::default()
            .parse_stamp(&json, std::path::Path::new("/tmp/hill.stamp"))
            .unwrap()
            .value;
        let restored = brush_of(&read, None, Some(&map)).unwrap();
        assert_eq!(
            restored
                .variations()
                .iter()
                .map(|v| v.probability)
                .collect::<Vec<_>>(),
            vec![0.25, 2.5]
        );
        let restored = &restored.variations()[0].clip;
        assert_eq!(restored.size, IVec2::new(2, 1));
        assert_eq!(restored.layers[0].name, "ground");
        assert_eq!(restored.layers[1].name, "detail");
        assert_eq!(
            restored.layers[1].cells.cell(IVec2::new(1, 0)),
            clip.layers[1].cells.cell(IVec2::new(1, 0))
        );
        let mut zero_weight = read;
        zero_weight.variations[0].probability = 0.0;
        assert_eq!(
            brush_of(&zero_weight, Some(0), Some(&map))
                .unwrap()
                .variations()[0]
                .probability,
            1.0,
            "choosing one variation directly makes it paintable regardless of its library weight"
        );
    }
}
