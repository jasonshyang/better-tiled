//! World neighbours, movement previews, resize and switching through the real GPU canvas.

use crate::test_window::TestWindow;
use bt_core::{FileRef, World, WorldMap};
use bt_editor::{ActionId, Intent};
use bt_test_data::{
    Scratch,
    fixtures::{Atlas, MapBuilder},
};
use bt_utils::IRect;
use egui::{Key, Modifiers, PointerButton};
use glam::{DVec2, IVec2, UVec2};

#[test]
fn world_neighbours_move_resize_cancel_and_open_without_duplicate_assets() {
    let scratch = Scratch::new("window-world-tool").unwrap();
    let atlas = Atlas::builder().build_in(scratch.path()).unwrap();
    let first = scratch.join("first.tmx");
    let second = scratch.join("second.tmx");
    for path in [&first, &second] {
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(3, 2))
            .tileset(atlas.tileset.clone())
            .tile_layer("ground", "0 1 2\n3 4 5")
            .build()
            .unwrap();
        bt_tmx::WriteRequest::for_map(&map, path).write().unwrap();
    }
    let world_path = scratch.join("demo.world");
    let entry = |path: &std::path::Path, x| WorldMap {
        file: FileRef::new(path.file_name().unwrap().to_str().unwrap(), path),
        rect: IRect::from_min_size(IVec2::new(x, 0), IVec2::new(48, 32)),
    };
    let world = World {
        maps: vec![entry(&first, 0), entry(&second, 48)],
        ..World::default()
    };
    bt_tmx::WriteRequest::for_world(&world, &world_path)
        .write()
        .unwrap();
    let mut window = TestWindow::with_gpu(&scratch.join("data"));
    window.drop_file(&first);
    window.apply(Intent::Run(ActionId::ZoomNormal));
    for _ in 0..3 {
        window.apply(Intent::Run(ActionId::ZoomIn));
    }
    let doc = window.editor().current().unwrap();
    let world = window.editor().world_of(doc).unwrap();
    let cached = window.editor().store().by_path(&second).unwrap();
    window.press(Modifiers::NONE, Key::N);
    window.drag_world_then_escape(DVec2::new(72.0, 16.0), DVec2::new(88.0, 16.0));
    assert_eq!(window.editor().world_maps(world)[1].rect.x, 48);
    window.drag_world(
        DVec2::new(72.0, 16.0),
        DVec2::new(120.0, 16.0),
        Modifiers::NONE,
    );
    assert_eq!(window.editor().world_maps(world)[1].rect.x, 96);
    window.drag_world(
        DVec2::new(48.0, 32.0),
        DVec2::new(64.0, 48.0),
        Modifiers::NONE,
    );
    assert_eq!(window.editor().map_of(doc).unwrap().width, 4);
    assert_eq!(window.editor().map_of(doc).unwrap().height, 3);
    window.press(Modifiers::NONE, Key::ArrowRight);
    assert_eq!(window.editor().world_maps(world)[0].rect.x, 16);
    window.apply(Intent::Run(ActionId::Undo));
    assert_eq!(window.editor().world_maps(world)[0].rect.x, 0);
    if let Some(path) = std::env::var_os("BT_WORLD_SCREENSHOT") {
        window.save_picture(std::path::Path::new(&path));
    }
    window.click_world(
        DVec2::new(112.0, 16.0),
        PointerButton::Primary,
        Modifiers::NONE,
    );
    let second_doc = window.editor().current().unwrap();
    assert_ne!(doc, second_doc);
    assert_eq!(
        window.editor().document(second_doc).unwrap().asset(),
        cached
    );
    window.apply(Intent::Run(ActionId::SaveAll));
    window.exit();
}
