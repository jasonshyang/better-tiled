//! Auto-labelling from one tile: on sheets drawn in code whose labels are known, on Tiled's blob
//! set, and on the purchased packs' hand-made labels.

#![cfg(test)]

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use bt_core::{TileId, Tileset, TilesetImage, WangId, WangSetType};
use bt_image::DecodedImage;
use bt_terrain::TerrainColor;
use bt_terrain::autolabel::{Method, label_with};
use bt_terrain::layouts::{LAYOUTS, Layout};
use bt_utils::IRect;
use glam::UVec2;

use common::pictures::*;

fn layout(name: &str) -> &'static Layout {
    LAYOUTS.iter().find(|l| l.name == name).unwrap()
}

/// Every tile's rectangle in a tileset's image.
fn rects(tileset: &Tileset) -> BTreeMap<TileId, IRect> {
    tileset
        .tile_ids()
        .into_iter()
        .filter_map(|tile| Some((tile, tileset.tile_image(tile)?.rect)))
        .collect()
}

/// The lanes of `label` that are `color`, the others no terrain; `None` when none is.
fn only(label: WangId, color: u8, kind: WangSetType) -> Option<WangId> {
    let lanes: Vec<usize> = match kind {
        WangSetType::Corner => vec![1, 3, 5, 7],
        WangSetType::Edge => vec![0, 2, 4, 6],
        WangSetType::Mixed => (0..8).collect(),
    };
    let mut out = [0; 8];
    for lane in lanes {
        if label.lanes()[lane] == color {
            out[lane] = color;
        }
    }
    let out = WangId::from_lanes(out);
    (!out.is_empty()).then_some(out)
}

/// A layout drawn at the top-left of a sheet, terrain 1 over `under` (or nothing), labelled
/// from its plain tile: the proposal is the layout's own labels of terrain 1.
#[test]
fn labels_from_one_tile_on_a_known_sheet() {
    let grass = rgb(60, 170, 60);
    let dirt = rgb(150, 100, 50);
    for name in [
        "Blob 7×7",
        "5×5 cross island, diagonals",
        "3×3 island",
        "Godot 3×3 sides",
    ] {
        let l = layout(name);
        let size = l.size();
        for (under, method) in [(Some(dirt), Method::Learned), (None, Method::Transparency)] {
            let mut picture = blank(size.x, size.y, 24);
            draw_layout(&mut picture, l, UVec2::ZERO, 24, grass, under);
            let tileset = atlas_for(size.x, size.y, 24);
            let labels = l.labels(UVec2::ZERO, size.x, TerrainColor(1), TerrainColor(0));
            // The plain tile: all terrain lanes, with its middle the terrain.
            let full = l
                .slots()
                .iter()
                .position(|s| {
                    s.is_some_and(|s| {
                        s.middle
                            && (0..8).all(|lane| {
                                s.terrain & (1 << lane) != 0
                                    || !matches!(
                                        (l.kind, lane % 2),
                                        (WangSetType::Mixed, _)
                                            | (WangSetType::Corner, 1)
                                            | (WangSetType::Edge, 0)
                                    )
                            })
                    })
                })
                .unwrap() as u32;
            let reference = tileset.tile_image(TileId(full)).unwrap().rect;
            let proposal = label_with(
                Method::Auto,
                &picture,
                l.kind,
                tileset.tile_size(),
                reference,
                TerrainColor(1),
                &rects(&tileset),
            );
            assert_eq!(proposal.method, method, "{name} over {under:?}");
            let expected: BTreeMap<TileId, WangId> = labels
                .into_iter()
                .filter_map(|(tile, label)| Some((tile, only(label?, 1, l.kind)?)))
                .collect();
            assert_eq!(proposal.tiles, expected, "{name} over {under:?}");
        }
    }
}

/// Tiled's own blob set: yellow over blue, labelled from the plain yellow tile, gives Tiled's
/// labels of yellow.
#[test]
fn labels_of_tileds_blob_set_from_its_yellow_tile() {
    let Some(path) = bt_test_data::test_data("reference/tiled/tests/wangtiles/wangblob.tsx") else {
        return;
    };
    let (tileset, image) = load(&path);
    let set = tileset.wang_sets.iter().find(|s| s.name == "Blob").unwrap();
    let yellow = set.colors[0].tile.unwrap();
    let proposal = label_with(
        Method::Auto,
        &image,
        set.kind,
        tileset.tile_size(),
        tileset.tile_image(yellow).unwrap().rect,
        TerrainColor(1),
        &rects(&tileset),
    );
    assert_eq!(proposal.method, Method::Learned);
    let expected: BTreeMap<TileId, WangId> = set
        .tiles
        .iter()
        .filter_map(|(&tile, &label)| Some((tile, only(label, 1, set.kind)?)))
        .collect();
    assert_eq!(proposal.tiles, expected);
}

fn load(path: &Path) -> (std::sync::Arc<Tileset>, DecodedImage) {
    let tileset = bt_tmx::Loader::default().load_tileset(path).unwrap().value;
    let TilesetImage::Atlas { image, .. } = &tileset.image else {
        panic!("{} is not an atlas", path.display());
    };
    let decoded = bt_image::decode_file(image.path().unwrap(), None).unwrap();
    (tileset, decoded)
}

/// Sets whose terrain no colour comparison can find: both sides share their colours, or the
/// terrain's top is see-through and only its rim shows.
const UNSEEABLE: [&str; 2] = [
    "rocky ground-to-rocky-ground",
    "Cliff1-1T-transparency-CTRL+M on walls layer to make it transparent ",
];

/// The packs' hand-made labels of single-terrain sets: auto-labelling each set's terrain from
/// one of its plain tiles, over the tiles the set labels, gives the same labels on nine tiles in
/// ten. (Loosely drawn terrains, scattered stones, do worst.)
#[test]
fn labels_of_the_packs_terrains_agree_with_their_hand_labels() {
    let mut total = 0;
    let mut same = 0;
    for file in [
        "assets/ERW - Ancient Ruins V 2.3/TiledMap Editor/Tilesets/Terrain - Ancient Ruins.tsx",
        "assets/Epic RPG World - Desert V1.6/tiledmap editor/Tilesets/Terrain-Desert.tsx",
    ] {
        let Some(path) = bt_test_data::test_data(file) else {
            return;
        };
        let (tileset, image) = load(&path);
        for set in &tileset.wang_sets {
            if set.colors.len() != 1 || UNSEEABLE.contains(&set.name.as_str()) {
                continue;
            }
            let plain = |label: &WangId| {
                only(*label, 1, set.kind) == Some(*label)
                    && label.lanes().iter().filter(|&&v| v == 1).count()
                        == if set.kind == WangSetType::Mixed { 8 } else { 4 }
            };
            let Some((&reference, _)) = set.tiles.iter().find(|(_, label)| plain(label)) else {
                continue;
            };
            let tiles: BTreeMap<TileId, IRect> = set
                .tiles
                .keys()
                .filter_map(|&tile| Some((tile, tileset.tile_image(tile)?.rect)))
                .collect();
            let proposal = label_with(
                Method::Auto,
                &image,
                set.kind,
                tileset.tile_size(),
                tileset.tile_image(reference).unwrap().rect,
                TerrainColor(1),
                &tiles,
            );
            let mut set_same = 0;
            for (tile, label) in &set.tiles {
                let ours = proposal.tiles.get(tile).copied();
                set_same += usize::from(ours == only(*label, 1, set.kind));
            }
            println!(
                "{}: {set_same}/{} ({:?})",
                set.name,
                set.tiles.len(),
                proposal.method
            );
            same += set_same;
            total += set.tiles.len();
        }
    }
    println!("overall {same}/{total}");
    assert!(same * 100 >= total * 90, "{same} of {total}");
}
