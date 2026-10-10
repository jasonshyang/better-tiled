//! The patterns a set needs: for a blob, for islands that meet only their ground (Tiled's desert
//! among them), for terrains that never meet, for terrains drawn over nothing, and with the
//! flipped and turned tiles the tileset allows.

#![cfg(test)]

mod common;

use std::collections::HashSet;

use bt_core::{TileId, Tileset, Transformations, WangId, WangSetType};
use bt_terrain::layouts::LAYOUTS;
use bt_terrain::patterns::{Patterns, needed};
use bt_terrain::{Terrain, TerrainColor};
use glam::UVec2;

use common::*;

/// The patterns of set 0 of `tileset`.
fn patterns_of(tileset: &Tileset) -> Patterns {
    needed(&Terrain::from_tileset(tileset, slot(), 0).unwrap())
}

/// A set of `kind` with terrains `names` and the given labels, tile `i` labelled `labels[i]`.
fn set_with(kind: WangSetType, names: &[&str], labels: &[[u8; 8]]) -> Tileset {
    tileset(kind, names, labels)
}

/// The labels of the Blob 7×7 layout of terrain 1 over terrain 2, by tile.
fn blob_labels() -> Vec<(TileId, WangId)> {
    let blob = LAYOUTS.iter().find(|l| l.name == "Blob 7×7").unwrap();
    blob.labels(UVec2::ZERO, 7, TerrainColor(1), TerrainColor(2))
        .into_iter()
        .filter_map(|(tile, label)| Some((tile, label?)))
        .collect()
}

/// A mixed set of grass (1) over sand (2) labelled with `labels`.
fn blob_set(labels: &[(TileId, WangId)]) -> Tileset {
    let mut tileset = set_with(WangSetType::Mixed, &["Grass", "Sand"], &[]);
    tileset.wang_sets[0].tiles = labels.iter().copied().collect();
    tileset
}

/// How many of the patterns the set needs its tiles have, and how many it needs.
fn counted(patterns: &Patterns) -> (usize, u64) {
    (patterns.present(), patterns.total)
}

#[test]
fn a_terrain_over_nothing_needs_its_patterns_with_nothing() {
    let corner = patterns_of(&set_with(WangSetType::Corner, &["Grass"], &[]));
    assert_eq!(
        (corner.listed.len() as u64, corner.total),
        (15, 15),
        "grass and nothing on four corners, but nothing at all"
    );
    let blob = patterns_of(&set_with(WangSetType::Mixed, &["Grass"], &[]));
    assert_eq!(blob.total, 46, "a blob's 47 tiles but the empty one");
}

#[test]
fn terrains_whose_labels_say_no_more_need_every_combination_in_tileds_order() {
    let patterns = patterns_of(&set_with(
        WangSetType::Corner,
        &["Grass", "Sand"],
        &[corners([1, 1, 0, 0])],
    ));
    assert_eq!((patterns.listed.len(), patterns.total), (16, 16));
    assert_eq!(
        patterns.listed[1].pattern,
        WangId::from_lanes([0, 2, 0, 1, 0, 1, 0, 1]),
        "the top-right corner counts fastest"
    );
}

#[test]
fn a_blob_needs_47_patterns_not_256() {
    let labels = blob_labels();
    let patterns = patterns_of(&blob_set(&labels));
    assert_eq!(patterns.total, 47);
    assert_eq!(patterns.present(), 47, "the blob's tiles are its patterns");
    let labelled: HashSet<WangId> = labels.iter().map(|(_, label)| *label).collect();
    assert!(
        patterns
            .listed
            .iter()
            .all(|p| labelled.contains(&p.pattern))
    );
    let plain_sand = WangId::from_lanes([2; 8]);
    let sand = patterns
        .listed
        .iter()
        .find(|p| p.pattern == plain_sand)
        .unwrap();
    assert_eq!(sand.tiles, 3, "the blob draws plain sand three times");
}

#[test]
fn a_blob_with_a_stray_label_still_needs_47() {
    let mut labels = blob_labels();
    let stray = labels
        .iter()
        .position(|(_, label)| label.lanes() == [1, 1, 1, 1, 1, 1, 1, 2])
        .unwrap();
    labels[stray].1 = WangId::from_lanes([1, 1, 1, 1, 1, 1, 2, 2]);
    let patterns = patterns_of(&blob_set(&labels));
    assert_eq!(
        counted(&patterns),
        (46, 47),
        "the mislabelled tile's pattern is missing"
    );
    let missing: Vec<WangId> = patterns.missing().map(|p| p.pattern).collect();
    assert_eq!(missing, [WangId::from_lanes([1, 1, 1, 1, 1, 1, 1, 2])]);
}

/// Autotiles each drawn over nothing and never together, as an RPG Maker import makes them:
/// each one's own blob patterns, not every autotile with every other (4⁸).
#[test]
fn terrains_that_never_meet_are_kept_apart() {
    let apart = [
        [1; 8],
        [1, 0, 0, 0, 0, 0, 0, 0],
        [2; 8],
        [0, 0, 3, 0, 0, 0, 0, 0],
    ];
    let patterns = patterns_of(&set_with(WangSetType::Mixed, &["A", "B", "C"], &apart));
    assert_eq!(patterns.total, 3 * 46);
}

/// Islands that meet only their ground: brick and cobblestone each on sand, water not drawn yet
/// (it goes on the sand too).
#[test]
fn islands_meet_only_their_ground() {
    let pair = |c: u8| {
        (0..16_u8).map(move |i| {
            let corner = |bit: u8| if i & bit == 0 { 1 } else { c };
            corners([corner(1), corner(2), corner(4), corner(8)])
        })
    };
    let islands: Vec<[u8; 8]> = pair(2).chain(pair(3).skip(1)).collect();
    let patterns = patterns_of(&set_with(
        WangSetType::Corner,
        &["Sand", "Brick", "Cobblestone", "Water"],
        &islands,
    ));
    assert_eq!(counted(&patterns), (31, 46));
}

/// Grass and water, with each other and over nothing: nothing is one of the terrains, so a
/// missing patch of grass on nothing is missing.
#[test]
fn nothing_is_a_terrain_when_two_are_drawn_over_it() {
    let all: Vec<[u8; 4]> = (0..81_u32)
        .map(|i| {
            [
                (i % 3) as u8,
                (i / 3 % 3) as u8,
                (i / 9 % 3) as u8,
                (i / 27) as u8,
            ]
        })
        .filter(|c| *c != [0; 4])
        .collect();
    let lacking = [0, 0, 0, 1];
    let labels: Vec<[u8; 8]> = all
        .iter()
        .filter(|c| **c != lacking)
        .map(|c| corners(*c))
        .collect();
    let patterns = patterns_of(&set_with(WangSetType::Corner, &["Grass", "Water"], &labels));
    assert_eq!(counted(&patterns), (79, 80));
    let missing: Vec<WangId> = patterns.missing().map(|p| p.pattern).collect();
    assert_eq!(missing, [WangId::from_lanes(corners(lacking))]);
}

/// Tiled's desert: brick, cobblestone and dirt each meet the sand and nothing else, so it needs
/// 46 patterns, 6 of them missing (two corners of a terrain facing each other).
#[test]
fn tileds_desert_needs_its_islands_on_sand() {
    let Some(path) = bt_test_data::test_data("reference/tiled/examples/desert.tsx") else {
        return;
    };
    let tileset = bt_tmx::Loader::default().load_tileset(&path).unwrap().value;
    assert_eq!(counted(&patterns_of(&tileset)), (40, 46));
}

#[test]
fn flipped_and_turned_tiles_count_as_present() {
    // Sand on the top-right corner only.
    let mut tileset = set_with(
        WangSetType::Corner,
        &["Grass", "Sand"],
        &[corners([2, 1, 1, 1])],
    );
    assert_eq!(patterns_of(&tileset).present(), 1);
    // With turns allowed, the other three corners are covered too.
    tileset.transformations = Transformations::ROTATE;
    assert_eq!(patterns_of(&tileset).present(), 4);
}

/// Six terrains on the eight lanes of a mixed set, with no label to tell them apart: 6⁸
/// combinations, counted but listed only as far as Tiled lists them.
#[test]
fn a_group_too_big_to_list_is_counted() {
    let patterns = patterns_of(&set_with(
        WangSetType::Mixed,
        &["A", "B", "C", "D", "E", "F"],
        &[],
    ));
    assert_eq!(patterns.total, 6_u64.pow(8));
    assert_eq!(patterns.listed.len(), bt_terrain::patterns::MAX_LISTED);
    assert_eq!(patterns.present(), 0);
}

#[test]
fn a_set_without_terrains_needs_nothing() {
    let patterns = patterns_of(&set_with(WangSetType::Corner, &[], &[]));
    assert_eq!(patterns, Patterns::default());
}
