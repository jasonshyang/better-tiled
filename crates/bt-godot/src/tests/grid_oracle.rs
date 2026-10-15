//! Godot itself measures exported cell centres, including negative coordinates.
use super::support::{godot, run_gdscript};
use bt_core::{MapGeometry, Orientation, StaggerAxis, StaggerIndex};
use bt_test_data::{
    Scratch,
    fixtures::{Atlas, MapBuilder},
};
use glam::{IVec2, UVec2};
use serde_json::json;

#[test]
fn godot_cell_centres_match_tiled_on_every_grid() {
    let Some(godot) = godot() else {
        return;
    };
    let scratch = Scratch::new("godot-grid-oracle").unwrap();
    std::fs::write(scratch.join("project.godot"), "config_version=5\n").unwrap();
    let atlas = Atlas::builder().build_in(scratch.path()).unwrap();
    let mut cases = Vec::new();
    for orientation in [
        Orientation::Orthogonal,
        Orientation::Isometric,
        Orientation::Staggered,
        Orientation::Hexagonal,
    ] {
        for axis in [StaggerAxis::X, StaggerAxis::Y] {
            for index in [StaggerIndex::Even, StaggerIndex::Odd] {
                let map = MapBuilder::new(UVec2::new(60, 60), UVec2::new(7, 10))
                    .orientation(orientation)
                    .stagger(axis, index)
                    .hex_side_length(30)
                    .tileset(atlas.tileset.clone())
                    .tile_layer("ground", "0 1")
                    .build()
                    .unwrap();
                let name = format!("map{}.tscn", cases.len());
                crate::export_map(
                    &map,
                    &scratch.join("map.tmx"),
                    &bt_core::CustomTypes::default(),
                    &scratch.join(&name),
                    &crate::Options::default(),
                )
                .unwrap();
                let geometry = MapGeometry::from_map(&map);
                let grid = crate::grid::Grid::of(&map).unwrap();
                let points: Vec<_> = [
                    IVec2::ZERO,
                    IVec2::new(-3, -2),
                    IVec2::new(4, 7),
                    IVec2::new(-1, 1),
                ]
                .into_iter()
                .map(|cell| {
                    let centre = crate::grid::tiled_centre(&geometry, cell) - grid.shift();
                    json!({"cell":[cell.x,cell.y],"centre":[centre.x,centre.y]})
                })
                .collect();
                cases.push(json!({"scene":format!("res://{name}"),"points":points}));
            }
        }
    }
    std::fs::write(
        scratch.join("expected.json"),
        serde_json::to_vec(&cases).unwrap(),
    )
    .unwrap();
    // Import first so PNGs are available to the scene loader.
    let output = std::process::Command::new(&godot)
        .args(["--headless", "--path"])
        .arg(scratch.path())
        .arg("--import")
        .output()
        .unwrap();
    assert!(output.status.success());
    let found = run_gdscript(&godot, scratch.path(), CHECK, &[]);
    assert_eq!(found["failures"], json!([]));
    assert_eq!(found["checked"], 64);
}
const CHECK: &str = r#"extends SceneTree
func _init():
    var failures = []
    var checked = 0
    for case in JSON.parse_string(FileAccess.get_file_as_string("res://expected.json")):
        var root = load(case.scene).instantiate()
        var layer = root.find_children("*", "TileMapLayer", true, false)[0]
        for point in case.points:
            var cell = Vector2i(point.cell[0],point.cell[1])
            var actual = layer.map_to_local(cell)
            var expected = Vector2(point.centre[0],point.centre[1])
            if actual.distance_to(expected)>0.01:
                failures.append({"scene":case.scene,"cell":point.cell,"actual":str(actual),"expected":str(expected)})
            checked += 1
        root.free()
    print("BT-JSON-BEGIN", JSON.stringify({"failures":failures,"checked":checked}), "BT-JSON-END")
    quit()
"#;
