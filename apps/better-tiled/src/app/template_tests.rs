//! Template bases use the object Properties panel and the real canvas renderer.

use crate::test_window::TestWindow;
use bt_core::{MapObject, ObjectTemplate, Shape};
use bt_editor::{ActionId, Intent};
use bt_test_data::Scratch;
use egui::{Key, Modifiers};
use glam::DVec2;

#[test]
fn template_properties_edit_save_and_undo_the_definition() {
    let scratch = Scratch::new("window-template-base").unwrap();
    let path = scratch.join("chest.tx");
    let template = ObjectTemplate {
        object: Some(MapObject {
            name: "Chest".into(),
            class: "Treasure".into(),
            size: DVec2::new(64.0, 48.0),
            shape: Shape::Ellipse,
            ..MapObject::default()
        }),
        ..ObjectTemplate::default()
    };
    bt_tmx::WriteRequest::for_template(&template, &path)
        .write()
        .unwrap();
    let mut window = TestWindow::with_gpu(&scratch.join("data"));
    window.drop_file(&path);
    assert!(window.shows("Template Base"));
    assert!(!window.shows("This map has no tilesets"));
    window.focus("Name");
    window.press(Modifiers::COMMAND, Key::A);
    window.type_text("Golden Chest");
    window.press(Modifiers::NONE, Key::Enter);
    let doc = window.editor().current().unwrap();
    assert_eq!(
        window
            .editor()
            .template_of(doc)
            .unwrap()
            .object
            .as_ref()
            .unwrap()
            .name,
        "Golden Chest"
    );
    if let Some(path) = std::env::var_os("BT_TEMPLATE_SCREENSHOT") {
        window.save_picture(std::path::Path::new(&path));
    }
    window.apply(Intent::Run(ActionId::Save));
    assert_eq!(
        bt_tmx::Loader::default()
            .load_template(&path)
            .unwrap()
            .value
            .object
            .as_ref()
            .unwrap()
            .name,
        "Golden Chest"
    );
    window.apply(Intent::Run(ActionId::Undo));
    assert_eq!(
        window
            .editor()
            .template_of(doc)
            .unwrap()
            .object
            .as_ref()
            .unwrap()
            .name,
        "Chest"
    );
    window.exit();
}

#[test]
fn template_picker_previews_real_tiles_places_and_edits_the_shared_base() {
    use bt_core::{Cell, FileRef, ObjectId, TileId, TilesetRef, TilesetSlot};
    use bt_editor::FileRequest;
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use egui::PointerButton;
    use glam::UVec2;
    let scratch = Scratch::new("window-template-placement").unwrap();
    let atlas = Atlas::builder().build_in(scratch.path()).unwrap();
    let tileset = scratch.join("tiles.tsx");
    bt_tmx::WriteRequest::for_tileset(&atlas.tileset, &tileset)
        .write()
        .unwrap();
    let path = scratch.join("chest.tx");
    let template = ObjectTemplate {
        tilesets: vec![TilesetRef {
            file: Some(FileRef::new("tiles.tsx", &tileset)),
            first_gid: Some(1),
            tileset: atlas.tileset,
        }],
        object: Some(MapObject {
            name: "Chest".into(),
            tile: Some(Cell::new(TilesetSlot::new(0).unwrap(), TileId(3))),
            ..MapObject::default()
        }),
        ..ObjectTemplate::default()
    };
    bt_tmx::WriteRequest::for_template(&template, &path)
        .write()
        .unwrap();
    let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(8))
        .object_layer("Objects", Vec::new())
        .build()
        .unwrap();
    let map_path = scratch.join("map.tmx");
    bt_tmx::WriteRequest::for_map(&map, &map_path)
        .write()
        .unwrap();
    let mut window = TestWindow::with_gpu(&scratch.join("data"));
    window.drop_file(&map_path);
    let doc = window.editor().current().unwrap();
    window.press(Modifiers::NONE, Key::V);
    window.answer_file_dialog(FileRequest::ObjectTemplate(doc), &path);
    window.hover_world(DVec2::new(48.0, 48.0));
    assert!(window.editor().map_of(doc).unwrap().tilesets.is_empty());
    if let Some(path) = std::env::var_os("BT_TEMPLATE_GHOST_SCREENSHOT") {
        window.save_picture(std::path::Path::new(&path));
    }
    window.click_world(
        DVec2::new(48.0, 48.0),
        PointerButton::Primary,
        Modifiers::NONE,
    );
    assert_eq!(window.editor().map_of(doc).unwrap().tilesets.len(), 1);
    assert_eq!(
        window
            .editor()
            .map_of(doc)
            .unwrap()
            .object(ObjectId(1))
            .unwrap()
            .name,
        "Chest"
    );
    window.apply(Intent::Run(ActionId::EditTemplate));
    let template_doc = window.editor().current().unwrap();
    assert!(window.editor().template_of(template_doc).is_some());
    window.focus("Name");
    window.press(Modifiers::COMMAND, Key::A);
    window.type_text("Treasure Chest");
    window.press(Modifiers::NONE, Key::Enter);
    assert_eq!(
        window
            .editor()
            .map_of(doc)
            .unwrap()
            .object(ObjectId(1))
            .unwrap()
            .name,
        "Treasure Chest"
    );
    window.apply(Intent::Run(ActionId::Save));
    window.apply(Intent::SelectDocument(doc));
    window.apply(Intent::Run(ActionId::DetachTemplate));
    assert!(
        window
            .editor()
            .map_of(doc)
            .unwrap()
            .object(ObjectId(1))
            .unwrap()
            .template
            .is_none()
    );
    window.apply(Intent::Run(ActionId::Undo));
    assert!(
        window
            .editor()
            .map_of(doc)
            .unwrap()
            .object(ObjectId(1))
            .unwrap()
            .template
            .is_some()
    );
    window.apply(Intent::Run(ActionId::SaveAll));
    window.exit();
}

#[test]
fn collision_canvas_previews_and_places_shape_templates() {
    use bt_core::TileId;
    use bt_editor::{FileRequest, TilesetMode};
    let scratch = Scratch::new("window-collision-template").unwrap();
    let atlas = bt_test_data::fixtures::Atlas::builder()
        .tile_size(32, 32)
        .build_in(scratch.path())
        .unwrap();
    let tileset_path = scratch.join("tiles.tsx");
    bt_tmx::WriteRequest::for_tileset(&atlas.tileset, &tileset_path)
        .write()
        .unwrap();
    let template = ObjectTemplate {
        object: Some(MapObject {
            name: "Solid".into(),
            size: DVec2::new(12.0, 8.0),
            shape: Shape::Ellipse,
            ..MapObject::default()
        }),
        ..ObjectTemplate::default()
    };
    let path = scratch.join("solid.tx");
    bt_tmx::WriteRequest::for_template(&template, &path)
        .write()
        .unwrap();
    let mut window = TestWindow::with_gpu(&scratch.join("data"));
    window.drop_file(&tileset_path);
    let doc = window.editor().current().unwrap();
    window.apply(Intent::SelectTilesetTiles {
        doc,
        tiles: vec![TileId(0)],
        current: Some(TileId(0)),
    });
    window.apply(Intent::SetTilesetMode {
        doc,
        mode: TilesetMode::Collision,
    });
    window.press(Modifiers::NONE, Key::V);
    window.answer_file_dialog(FileRequest::ObjectTemplate(doc), &path);
    let at = window.collision_point(DVec2::new(16.0, 16.0));
    window.hover_on_screen(at);
    if let Some(path) = std::env::var_os("BT_COLLISION_TEMPLATE_SCREENSHOT") {
        window.save_picture(std::path::Path::new(&path));
    }
    window.click_on_screen(at, egui::PointerButton::Primary, Modifiers::NONE);
    let asset = window.editor().document(doc).unwrap().asset();
    let objects = &window
        .editor()
        .store()
        .tileset(asset)
        .unwrap()
        .tile(TileId(0))
        .unwrap()
        .collision
        .as_ref()
        .unwrap()
        .layer
        .objects;
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].name, "Solid");
    assert!(objects[0].template.is_some());
    window.apply(Intent::Run(ActionId::Undo));
    assert!(
        window
            .editor()
            .store()
            .tileset(asset)
            .unwrap()
            .tile(TileId(0))
            .is_none_or(|tile| tile.collision.is_none())
    );
    window.exit();
}
