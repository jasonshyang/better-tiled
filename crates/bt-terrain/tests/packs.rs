//! The purchased packs' terrain sets (`assets/`, never in the repository): every set fills an
//! empty layer with each of its terrains and paints each other terrain over that, the tiles placed
//! together agreeing and every unfilled cell's need naming a pattern the set lacks; and a dab
//! previews in under a millisecond on the largest set, in optimised builds.

#![cfg(test)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bt_core::{Map, Tileset, WangSetType};
use bt_terrain::{
    BrushTarget, FillRequest, PaintRequest, Terrain, TerrainColor, preview_fill, preview_paint,
};
use bt_test_data::fixtures::MapBuilder;
use glam::{IVec2, UVec2};

use common::*;

/// Every tileset file under `folder` with a terrain set.
fn tilesets_with_terrain(folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            tilesets_with_terrain(&path, found);
        } else if path.extension().is_some_and(|e| e == "tsx")
            && std::fs::read_to_string(&path).is_ok_and(|text| text.contains("<wangset"))
        {
            found.push(path);
        }
    }
}

fn pack_tilesets() -> Vec<(PathBuf, Arc<Tileset>)> {
    let Some(assets) = bt_test_data::test_data("assets") else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    tilesets_with_terrain(&assets, &mut paths);
    assert!(!paths.is_empty(), "the packs have terrain sets");
    paths
        .into_iter()
        .map(|path| {
            let tileset = bt_tmx::Loader::default().load_tileset(&path).unwrap().value;
            (path, tileset)
        })
        .collect()
}

fn map_of(tileset: &Arc<Tileset>, size: u32) -> Map {
    MapBuilder::new(UVec2::splat(16), UVec2::splat(size))
        .tileset(Arc::clone(tileset))
        .tile_layer("ground", "")
        .build()
        .unwrap()
}

fn every_cell(size: i32) -> Vec<IVec2> {
    bt_utils::IRect::from_min_size(IVec2::ZERO, IVec2::splat(size))
        .points()
        .collect()
}

#[test]
fn the_packs_sets_fill_and_paint_consistently() {
    let mut sets = 0;
    for (path, tileset) in pack_tilesets() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        for set_index in 0..tileset.wang_sets.len() {
            let empty = map_of(&tileset, 12);
            let terrain = Terrain::new(&empty, slot(), set_index).unwrap();
            let colors = terrain.color_count() as u8;
            sets += 1;
            for color in 1..=colors {
                let what = format!("{name} set {set_index}, fill with {color}");
                let fill = FillRequest::new(GROUND, &terrain, TerrainColor(color))
                    .cells(every_cell(12))
                    .seed(u64::from(color));
                let filled_preview = preview_fill(&empty, &fill).unwrap();
                assert_preview_consistent(&filled_preview, &terrain, &what);
                assert_needs_are_missing(&terrain, &filled_preview.needed);

                let mut filled = empty.clone();
                bt_terrain::fill(&mut filled, &fill).unwrap();
                for other in (0..=colors).filter(|&c| c != color) {
                    let kind = terrain.brush_kind(TerrainColor(other));
                    let mut targets = vec![BrushTarget::Tile(IVec2::splat(5))];
                    match kind {
                        WangSetType::Corner => targets.push(BrushTarget::Corner(IVec2::splat(8))),
                        WangSetType::Edge => targets.push(BrushTarget::Edge {
                            cell: IVec2::splat(8),
                            side: bt_core::Lane::Bottom,
                        }),
                        WangSetType::Mixed => {}
                    }
                    for target in targets {
                        let what = format!("{what}, then {target:?} with {other}");
                        let paint = PaintRequest::new(GROUND, &terrain, TerrainColor(other))
                            .targets(vec![target])
                            .seed(9);
                        let preview = preview_paint(&filled, &paint).unwrap();
                        assert_preview_consistent(&preview, &terrain, &what);
                        assert_needs_are_missing(&terrain, &preview.needed);
                    }
                }
            }
        }
    }
    if bt_test_data::test_data("assets").is_some() {
        assert!(sets > 20, "{sets} sets");
    }
}

#[test]
fn a_dab_previews_in_under_a_millisecond() {
    // The set with the most tiles to choose from.
    let Some((tileset, set_index)) = pack_tilesets()
        .into_iter()
        .flat_map(|(_, tileset)| {
            (0..tileset.wang_sets.len()).map(move |i| (Arc::clone(&tileset), i))
        })
        .max_by_key(|(tileset, i)| tileset.wang_sets[*i].tiles.len())
    else {
        return;
    };
    let empty = map_of(&tileset, 64);
    let terrain = Terrain::new(&empty, slot(), set_index).unwrap();
    let mut map = empty.clone();
    bt_terrain::fill(
        &mut map,
        &FillRequest::new(GROUND, &terrain, TerrainColor(1)).cells(every_cell(64)),
    )
    .unwrap();
    let color = TerrainColor(terrain.color_count().min(2) as u8);
    let mut times: Vec<Duration> = (0..21)
        .map(|seed| {
            let request = PaintRequest::new(GROUND, &terrain, color)
                .targets(vec![BrushTarget::Corner(IVec2::splat(32))])
                .seed(seed);
            let start = Instant::now();
            let preview = preview_paint(&map, &request).unwrap();
            let took = start.elapsed();
            assert!(!preview.cells.is_empty());
            took
        })
        .collect();
    times.sort();
    let median = times[times.len() / 2];
    println!("a dab on {} set {set_index}: {median:?}", tileset.name);
    if !cfg!(debug_assertions) {
        assert!(median < Duration::from_millis(1), "{median:?}");
    }
}
