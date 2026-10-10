//! M3's measure (`00-high-level-design.md`, §17): a pack's `rules.txt` and terrain sets give
//! Tiled's results, with every failure explained. On the Highlands pack (purchased, never in the
//! repository: the test is skipped without it), driven through the window: the pack's rules do
//! nothing on its own sample scene, and AutoMap's notice and the Explain tool say why (they read
//! `walls`, which the scene leaves empty); bound to the layer the scene paints its platforms on,
//! Live AutoMap rebuilds those platforms as the pack draws them; and the Terrain tool paints with
//! a terrain set of the pack, every tile meeting its neighbours. `bt-automap`'s and
//! `bt-terrain`'s pack tests hold the exact results these rest on.

use bt_core::{Cell, Layer, LayerId, Map, TilesetSlot};
use bt_test_data::Scratch;
use egui::{Key, Modifiers, PointerButton};
use glam::{DVec2, IVec2};

use crate::test_window::TestWindow;

/// The Highlands pack's Tiled folder, as the packs sit beside the repository's code.
const HIGHLANDS: &str = "assets/Epic RPG World - Highlands V1.4.1/TiledMap Editor";

fn layer_id(map: &Map, name: &str) -> LayerId {
    let mut found = None;
    map.for_each_layer(|layer, _| {
        if found.is_none() && layer.name == name {
            found = Some(layer.id);
        }
    });
    found.unwrap_or_else(|| panic!("the scene has a layer {name}"))
}

fn cells(map: &Map, layer: LayerId) -> Vec<(IVec2, Cell)> {
    let mut cells: Vec<(IVec2, Cell)> = map
        .layer(layer)
        .and_then(Layer::as_tile_layer)
        .unwrap()
        .cells()
        .map(|placed| (placed.at, placed.cell))
        .collect();
    cells.sort_by_key(|(at, _)| (at.y, at.x));
    cells
}

#[test]
fn a_packs_rules_and_terrain_sets_give_tileds_results_with_every_failure_explained() {
    let Some(pack) = bt_test_data::test_data(HIGHLANDS) else {
        return;
    };
    let scratch = Scratch::new("window-m3-measure").unwrap();
    let mut window = TestWindow::with_data(&scratch.join("data"));
    // Opened where it is and never saved: nothing is written into the pack.
    window.drop_file(&pack.join("Sample scene.tmx"));
    let doc = window.editor().current().unwrap();
    let scene = Map::clone(window.editor().map_of(doc).unwrap());
    let plat1 = layer_id(&scene, "plat1");
    let platforms = cells(&scene, plat1);
    assert!(!platforms.is_empty());

    // The rules do nothing on the pack's own scene, and say why.
    window.press(Modifiers::COMMAND, Key::M);
    assert!(
        window.shows("AutoMap did nothing: the rules read walls, which is empty"),
        "AutoMap's notice says why"
    );
    assert_eq!(
        **window.editor().map_of(doc).unwrap(),
        scene,
        "nothing changed"
    );

    // At a platform cell, Explain says the same: every rule that could write it needs tiles on
    // walls, and walls is empty there.
    window.press(Modifiers::NONE, Key::A);
    let at = platforms[platforms.len() / 2].0;
    window.click_cell(at, PointerButton::Primary, Modifiers::NONE);
    let explanations = window.editor().cell_explanations(doc);
    let [explained] = explanations else {
        panic!("one explanation: {explanations:?}");
    };
    let failed: Vec<_> = explained
        .explanation
        .attempts
        .iter()
        .flat_map(|attempt| &attempt.checks)
        .filter(|check| !check.holds)
        .collect();
    assert!(!failed.is_empty());
    assert!(
        failed
            .iter()
            .all(|check| check.layer == "walls" && check.found.is_empty()),
        "every rule fails for want of a tile on walls"
    );

    // Bound to plat1, the rules rebuild the platforms the scene paints there, as the pack draws
    // them: the same cells, each tile the scene's or another variation from the same tileset.
    window.click("Live AutoMap");
    window.click_last("reads and writes walls");
    window.click_last("plat1");
    let shown = Map::clone(window.editor().shown_map(doc).unwrap());
    let rebuilt = cells(&shown, plat1);
    assert_eq!(
        rebuilt.iter().map(|(at, _)| *at).collect::<Vec<_>>(),
        platforms.iter().map(|(at, _)| *at).collect::<Vec<_>>(),
        "the same cells"
    );
    let same = rebuilt
        .iter()
        .zip(&platforms)
        .filter(|((_, got), (_, want))| got == want)
        .count();
    assert!(
        rebuilt
            .iter()
            .zip(&platforms)
            .all(|((_, got), (_, want))| got.slot() == want.slot()),
        "the platforms' tilesets"
    );
    assert!(
        same * 2 > platforms.len(),
        "{same} of {} tiles are the scene's own; the rest are variations the rules picked",
        platforms.len()
    );

    // The Terrain tool paints a terrain set of the pack on the empty walls layer: every tile
    // meets its neighbours, corner for corner.
    window.click("Live AutoMap");
    window.click("Layers");
    window.click("walls");
    window.press(Modifiers::NONE, Key::T);
    window.click("Terrains");
    window.click("Terrain - Highlands");
    window.click("Terrain 1");
    let tile = f64::from(scene.tile_width);
    window.drag_world(
        DVec2::new(10.0 * tile, 10.0 * tile),
        DVec2::new(14.0 * tile, 10.0 * tile),
        Modifiers::NONE,
    );
    let map = Map::clone(window.editor().map_of(doc).unwrap());
    let walls = layer_id(&map, "walls");
    let painted = cells(&map, walls);
    assert!(!painted.is_empty(), "the stroke painted");
    let choice = window.editor().tools().terrain().unwrap();
    let slot = map
        .tilesets
        .iter()
        .position(|t| t.tileset.name == "Terrain - Highlands")
        .and_then(TilesetSlot::new)
        .unwrap();
    let terrain = bt_terrain::Terrain::new(&map, slot, choice.set).unwrap();
    let tiles = map.layer(walls).and_then(Layer::as_tile_layer).unwrap();
    let lanes = |at: IVec2| terrain.wang_of_cell(tiles.cell(at)).lanes();
    for (at, _) in &painted {
        let here = lanes(*at);
        let right = lanes(*at + IVec2::X);
        let below = lanes(*at + IVec2::Y);
        // Corner lanes: 1 top-right, 3 bottom-right, 5 bottom-left, 7 top-left.
        let left = lanes(*at - IVec2::X);
        let above = lanes(*at - IVec2::Y);
        assert_eq!(
            [here[1], here[3]],
            [right[7], right[5]],
            "{at} meets its right"
        );
        assert_eq!(
            [here[5], here[3]],
            [below[7], below[1]],
            "{at} meets the one below"
        );
        assert_eq!(
            [here[7], here[5]],
            [left[1], left[3]],
            "{at} meets its left"
        );
        assert_eq!(
            [here[7], here[1]],
            [above[5], above[3]],
            "{at} meets the one above"
        );
    }
}
