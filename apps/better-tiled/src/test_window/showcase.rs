//! Opt-in construction timelapse through the actual application's editing intents and GPU.
//! Every captured addition changes an initially empty map; the saved result is checked.

use super::*;
use bt_core::{Cell, LayerId, Map, MapObject, ObjectId, PlacedCell, TileId};
use bt_editor::{ActionId, DocumentId, Intent, SelectionMode};
use glam::IVec2;
use std::collections::BTreeMap;

struct Recording {
    folder: PathBuf,
    frames: Vec<serde_json::Value>,
}

impl Recording {
    fn capture(&mut self, window: &mut TestWindow, caption: &str, ticks: u32) {
        let file = format!("{:04}.png", self.frames.len());
        window.harness.run_steps(6);
        window
            .harness
            .render()
            .unwrap()
            .save(self.folder.join(&file))
            .unwrap();
        self.frames.push(serde_json::json!({
            "file": file, "caption": caption, "ticks": ticks,
        }));
    }
}

impl TestWindow {
    fn for_recording(data: &Path) -> TestWindow {
        let mut startup = Startup::from_folder(Cli::default(), data).unwrap();
        startup.settings.general.restore_session = false;
        startup.settings.general.recovery = false;
        startup.settings.general.watch_files = false;
        startup.settings.interface.grid_color.0 = bt_core::Color::rgba(92, 100, 110, 180);
        startup.live_folder = None;
        let locks = data.join("locks");
        std::fs::create_dir_all(&locks).unwrap();
        startup.lock_folder = Some(locks);
        let harness = Harness::builder()
            .with_size(egui::vec2(1600.0, 1000.0))
            .with_step_dt(1.0 / 60.0)
            .with_render_every_step(true)
            .wgpu()
            .build_eframe(|cc| App::for_recording(cc, startup).unwrap());
        let mut window = TestWindow { harness };
        window.settle();
        window
    }

    /// Paints a stamp with a real canvas click, using the brush the Tiles panel picks.
    fn paint_stamp(&mut self, doc: DocumentId, cells: Vec<PlacedCell>) {
        if cells.is_empty() {
            return;
        }
        let min = cells.iter().map(|cell| cell.at).reduce(IVec2::min).unwrap();
        let max = cells.iter().map(|cell| cell.at).reduce(IVec2::max).unwrap();
        let center = min + (max - min + IVec2::ONE) / 2;
        self.apply(Intent::PickTiles { doc, cells });
        self.click_cell(center, egui::PointerButton::Primary, egui::Modifiers::NONE);
    }

    /// Inserts an undoable tile object, preserving the reference's visible overlap order.
    fn insert_from_reference(&mut self, object: &MapObject) -> ObjectId {
        let doc = self.editor().current().unwrap();
        self.apply(Intent::InsertObject {
            doc,
            object: Box::new(object.clone()),
        });
        self.editor()
            .documents()
            .get(doc)
            .unwrap()
            .selection()
            .objects[0]
    }
}

/// Grass first establishes the island footprint; later strokes replace it with the final
/// terrace edges, cliff faces and ramps. Both passes really paint the same tile layer.
fn flat_islands(reference: &Map) -> Vec<PlacedCell> {
    let terrain = reference
        .layer(LayerId(5))
        .unwrap()
        .as_tile_layer()
        .unwrap();
    terrain
        .cells()
        .map(|placed| {
            let at = placed.at;
            let left = !terrain.cell(at - IVec2::X).is_empty();
            let right = !terrain.cell(at + IVec2::X).is_empty();
            let above = !terrain.cell(at - IVec2::Y).is_empty();
            let below = !terrain.cell(at + IVec2::Y).is_empty();
            let column = match (left, right) {
                (false, false) => 3,
                (false, true) => 0,
                (true, false) => 2,
                (true, true) => 1,
            };
            let row = match (above, below) {
                (false, false) => 3,
                (false, true) => 0,
                (true, false) => 2,
                (true, true) => 1,
            };
            PlacedCell {
                at,
                cell: Cell::new(placed.cell.slot().unwrap(), TileId(row * 9 + column)),
            }
        })
        .collect()
}

fn assert_finished_map(actual: &Map, reference: &Map) {
    assert_eq!(actual.layers.len(), reference.layers.len());
    for expected in &reference.layers {
        let actual = actual.layer(expected.id).unwrap();
        if let Some(tiles) = expected.as_tile_layer() {
            let painted = actual.as_tile_layer().unwrap();
            for y in 0..reference.height {
                for x in 0..reference.width {
                    let at = IVec2::new(x as i32, y as i32);
                    assert_eq!(
                        painted.cell(at),
                        tiles.cell(at),
                        "{} at {at}",
                        expected.name
                    );
                }
            }
        } else {
            let objects = &actual.as_object_layer().unwrap().objects;
            let expected_objects = &expected.as_object_layer().unwrap().objects;
            assert_eq!(objects.len(), expected_objects.len(), "{}", expected.name);
            for (actual, expected) in objects.iter().zip(expected_objects.iter()) {
                let mut expected = expected.clone();
                expected.id = actual.id;
                assert_eq!(actual, &expected, "object content and draw order");
            }
        }
    }
}

#[test]
#[ignore = "requires a GPU; run scripts/record-showcase.py"]
fn record_showcase() {
    let output = PathBuf::from(std::env::var_os("BT_SHOWCASE_DIR").expect("BT_SHOWCASE_DIR"));
    let map = PathBuf::from(std::env::var_os("BT_SHOWCASE_MAP").expect("BT_SHOWCASE_MAP"));
    let reference = bt_tmx::Loader::builder()
        .build()
        .load_map(&map.with_file_name("reference.tmx"))
        .unwrap()
        .value;
    std::fs::create_dir_all(&output).unwrap();
    let mut recording = Recording {
        folder: output.clone(),
        frames: Vec::new(),
    };
    let mut window = TestWindow::for_recording(&output.join("data"));
    window.drop_file(&map);
    let doc = window.editor().current().unwrap();
    for layer in &window.editor().shown_map(doc).unwrap().layers {
        if let Some(tiles) = layer.as_tile_layer() {
            assert_eq!(tiles.cells().count(), 0, "recording must start empty");
        } else {
            assert!(layer.as_object_layer().unwrap().objects.is_empty());
        }
    }
    window.apply(Intent::Run(ActionId::FitInView));
    window.apply(Intent::SelectLayer {
        doc,
        layer: LayerId(1),
    });
    window.apply(Intent::Run(ActionId::BrushTool));
    window.click("Water");
    recording.capture(&mut window, "Start with an empty map", 5);

    // A real Bucket Fill creates the ocean in one click.
    window.apply(Intent::SelectLayer {
        doc,
        layer: LayerId(1),
    });
    window.apply(Intent::PickTiles {
        doc,
        cells: vec![PlacedCell {
            at: IVec2::ZERO,
            cell: reference
                .layer(LayerId(1))
                .unwrap()
                .as_tile_layer()
                .unwrap()
                .cell(IVec2::ZERO),
        }],
    });
    window.apply(Intent::Run(ActionId::BucketFillTool));
    window.click_cell(
        IVec2::new(12, 9),
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    window.apply(Intent::Run(ActionId::ShowGrid));
    recording.capture(&mut window, "Fill the ocean", 4);

    window.apply(Intent::SelectLayer {
        doc,
        layer: LayerId(5),
    });
    window.click("Terrain 1");
    let islands = flat_islands(&reference);
    for y in 0..reference.height as i32 {
        let mut row: Vec<_> = islands
            .iter()
            .copied()
            .filter(|cell| cell.at.y == y)
            .collect();
        row.sort_by_key(|cell| cell.at.x);
        for cells in row.chunks(8) {
            window.paint_stamp(doc, cells.to_vec());
            recording.capture(&mut window, "Paint the islands", 1);
        }
    }

    // Paint the ground and shadows beneath the grass before transparent cliff tiles go on.
    for layer in [LayerId(3), LayerId(4)] {
        window.apply(Intent::SelectLayer { doc, layer });
        let cells = reference
            .layer(layer)
            .unwrap()
            .as_tile_layer()
            .unwrap()
            .cells()
            .collect();
        window.paint_stamp(doc, cells);
    }
    window.apply(Intent::SelectLayer {
        doc,
        layer: LayerId(5),
    });
    window.click("Terrain 2");
    let terrain = reference
        .layer(LayerId(5))
        .unwrap()
        .as_tile_layer()
        .unwrap();
    for y in 0..reference.height as i32 {
        let cells: Vec<_> = terrain.cells().filter(|cell| cell.at.y == y).collect();
        if !cells.is_empty() {
            window.paint_stamp(doc, cells);
            recording.capture(&mut window, "Build the cliffs and connect the ramps", 2);
        }
    }

    window.apply(Intent::SelectLayer {
        doc,
        layer: LayerId(7),
    });
    window.click("Village");
    let zoom = window
        .harness
        .get_all_by_label("Zoom")
        .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Slider)
        .unwrap()
        .rect();
    window.click_on_screen(
        zoom.left_center() + egui::vec2(4.0, 0.0),
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    window.apply(Intent::Run(ActionId::InsertTileTool));
    // Keep the pointer outside the canvas so the insertion preview does not cover the build.
    window.pointer(egui::Event::PointerMoved(egui::pos2(10.0, 10.0)));
    let objects = &reference
        .layer(LayerId(7))
        .unwrap()
        .as_object_layer()
        .unwrap()
        .objects;
    let mut inserted = BTreeMap::new();
    for stage in 0..3 {
        if stage == 2 {
            // Finish the coast after the buildings and trees cover their foundations.
            window.apply(Intent::SelectLayer {
                doc,
                layer: LayerId(2),
            });
            for object in reference
                .layer(LayerId(2))
                .unwrap()
                .as_object_layer()
                .unwrap()
                .objects
                .iter()
            {
                window.insert_from_reference(object);
            }
            window.apply(Intent::SelectLayer {
                doc,
                layer: LayerId(7),
            });
        }
        let mut count = 0;
        for (index, object) in objects.iter().enumerate() {
            let object_stage = match object.name.as_str() {
                "castle" | "tower" | "house1" | "house2" | "house3" => 0,
                name if name.starts_with("tree") => 1,
                _ => 2,
            };
            if object_stage != stage {
                continue;
            }
            let id = window.insert_from_reference(object);
            let index = objects
                .iter()
                .take(index)
                .filter(|o| inserted.contains_key(&o.id))
                .count();
            window.apply(Intent::ReorderObjects {
                doc,
                objects: vec![id],
                layer: LayerId(7),
                index,
            });
            inserted.insert(object.id, id);
            window.apply(Intent::SelectObjects {
                doc,
                ids: Vec::new(),
                mode: SelectionMode::Replace,
            });
            count += 1;
            let caption = match stage {
                0 => "Place the castle, towers and village",
                1 => "Plant the trees",
                _ => "Add characters, sheep and details",
            };
            if stage < 2 || count % 2 == 0 {
                recording.capture(&mut window, caption, if stage == 0 { 2 } else { 1 });
            }
        }
    }
    window.apply(Intent::Run(ActionId::SelectObjectsTool));
    assert_finished_map(window.editor().shown_map(doc).unwrap(), &reference);
    window.apply(Intent::Run(ActionId::Save));
    let saved = bt_tmx::Loader::builder()
        .build()
        .load_map(&map)
        .unwrap()
        .value;
    assert_finished_map(&saved, &reference);
    for _ in 0..30 {
        recording.capture(&mut window, "Built in Better Tiled", 1);
    }
    std::fs::write(
        output.join("frames.json"),
        serde_json::to_vec_pretty(&recording.frames).unwrap(),
    )
    .unwrap();
    std::fs::write(output.join("construction-check.txt"), format!(
        "Started with every tile and object layer empty.\nRecorded {} editing frames.\nThe saved map matches every reference tile, sprite, position, flip and object draw order.\n",
        recording.frames.len()
    )).unwrap();
    window.exit();
}
