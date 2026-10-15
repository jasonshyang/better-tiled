//! Real stamp controls, rendered previews, quick-stamp keys and the session file.

use crate::test_window::TestWindow;
use bt_core::{TileId, TilesetSlot};
use bt_editor::{FileRequest, Intent, StampEdit};
use bt_test_data::{
    Scratch,
    fixtures::{Atlas, MapBuilder},
};
use glam::{IVec2, UVec2};

#[test]
fn saved_stamps_are_usable_from_the_panel_and_quick_stamps_survive_restart() {
    let scratch = Scratch::new("window-stamps").unwrap();
    let atlas = Atlas::builder().build_in(scratch.path()).unwrap();
    let map = MapBuilder::new(UVec2::splat(16), UVec2::new(3, 1))
        .tileset(atlas.tileset)
        .tile_layer("ground", "1 2 .")
        .build()
        .unwrap();
    let path = scratch.join("map.tmx");
    bt_tmx::WriteRequest::for_map(&map, &path).write().unwrap();
    let data = scratch.join("data");
    let mut window = TestWindow::with_gpu(&data);
    window.drop_file(&path);
    let doc = window.editor().current().unwrap();
    window.apply(Intent::PickFromMap {
        doc,
        at: IVec2::ZERO,
    });
    window.press(egui::Modifiers::COMMAND, egui::Key::Num1);
    assert!(window.editor().tools().quick_stamp(0).is_some());
    window.apply(Intent::PickFromMap { doc, at: IVec2::X });
    window.press(egui::Modifiers::NONE, egui::Key::Num1);
    assert_eq!(
        window.editor().tools().brush().unwrap().variations()[0]
            .clip
            .layers[0]
            .cells
            .cell(IVec2::ZERO)
            .tile(),
        TileId(1)
    );
    window.click("Stamps");
    window.focus("Stamp name");
    window.type_text("Hill");
    window.click("Save Current Stamp");
    assert_eq!(window.editor().stamps().entries().len(), 1);
    assert!(data.join("stamps/Hill.stamp").is_file());
    assert!(!window.shows("Preview failed"));
    if let Some(path) = std::env::var_os("BT_STAMPS_SCREENSHOT") {
        window.save_picture(std::path::Path::new(&path));
    }
    window.click("Add Variation");
    assert_eq!(
        window.editor().stamps().entries()[0].stamp.variations.len(),
        2
    );
    window.click_holding("Hill", egui::Modifiers::NONE);
    assert_eq!(
        window.editor().tools().brush().unwrap().variations().len(),
        2
    );
    window.press(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::Num1,
    );
    assert_eq!(
        window
            .editor()
            .tools()
            .quick_stamp(0)
            .unwrap()
            .variations
            .len(),
        3
    );
    window.drag_cells(IVec2::new(2, 0), IVec2::new(2, 0), egui::Modifiers::NONE);
    assert_eq!(
        window.editor().map_of(doc).unwrap().layers[0]
            .as_tile_layer()
            .unwrap()
            .cell(IVec2::new(2, 0))
            .slot(),
        TilesetSlot::new(0)
    );
    window.apply(Intent::Run(bt_editor::ActionId::Undo));
    window.exit();
    let mut reopened = TestWindow::with_data(&data);
    assert_eq!(
        reopened
            .editor()
            .tools()
            .quick_stamp(0)
            .unwrap()
            .variations
            .len(),
        3
    );
    reopened.click("Stamps");
    reopened.click("Set Folder…");
    let folder = scratch.join("other");
    std::fs::create_dir(&folder).unwrap();
    reopened.answer_file_dialog(FileRequest::StampsFolder, &folder);
    assert!(reopened.editor().stamps().entries().is_empty());
    assert_eq!(reopened.editor().stamps().folder(), folder);
    assert_eq!(
        reopened.editor().settings().general.stamps_folder,
        Some(folder)
    );
    reopened.apply(Intent::Stamp(StampEdit::ChooseQuick(0)));
    assert_eq!(
        reopened
            .editor()
            .tools()
            .brush()
            .unwrap()
            .variations()
            .len(),
        3
    );
}
