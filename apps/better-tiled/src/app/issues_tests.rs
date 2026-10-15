//! Real window navigation, source-linked repairs and the non-map Issues entry point.
use crate::test_window::TestWindow;
use bt_editor::{ActionId, FileRequest, Intent, LocateTarget};
use bt_test_data::Scratch;
#[test]
fn issues_show_missing_nested_image_and_repair_through_the_file_picker() {
    let scratch = Scratch::new("window-unified-issues").unwrap();
    let path = scratch.join("map.tmx");
    std::fs::write(&path, r#"<map orientation="orthogonal" width="8" height="6" tilewidth="16" tileheight="16" nextlayerid="3"><group id="1" name="Scenery"><imagelayer id="2" name="Backdrop"><image source="missing.png" width="32" height="32"/></imagelayer></group></map>"#).unwrap();
    let image = scratch.join("backdrop.png");
    bt_image::replace_png(
        &image,
        &bt_image::DecodedImage {
            width: 32,
            height: 32,
            rgba: [58, 125, 95, 255].repeat(1024),
        },
    )
    .unwrap();
    let mut window = TestWindow::with_gpu(&scratch.join("data"));
    window.drop_file(&path);
    window.apply(Intent::Run(ActionId::ShowIssues));
    window.apply(Intent::RefreshIssues);
    assert!(window.shows("Files · Error"));
    assert!(window.shows("Locate File…"));
    window.click("Go to source");
    let doc = window.editor().current().unwrap();
    assert_eq!(
        window
            .editor()
            .document(doc)
            .unwrap()
            .selection()
            .current_layer,
        Some(bt_core::LayerId(2))
    );
    if let Some(path) = std::env::var_os("BT_ISSUES_SCREENSHOT") {
        window.save_picture(std::path::Path::new(&path));
    }
    window.click("Locate File…");
    let map = window.editor().document(doc).unwrap().asset();
    let version = window.editor().store().get(map).unwrap().version();
    window.answer_file_dialog(
        FileRequest::LocateFile {
            doc,
            target: LocateTarget::LayerImage {
                map,
                layer: bt_core::LayerId(2),
            },
            version,
        },
        &image,
    );
    window.apply(Intent::RefreshIssues);
    assert!(window.editor().issues(doc).is_empty());
    assert!(window.shows("No issues found."));
    window.apply(Intent::Run(ActionId::Undo));
    window.apply(Intent::RefreshIssues);
    assert!(window.shows("Locate File…"));
    window.exit();
}
#[test]
fn issues_are_available_in_a_standalone_tileset_document() {
    let scratch = Scratch::new("window-tileset-issues").unwrap();
    let path = scratch.join("tiles.tsx");
    std::fs::write(&path, r#"<tileset name="Missing atlas" tilewidth="16" tileheight="16" tilecount="4" columns="2"><image source="missing.png" width="32" height="32"/></tileset>"#).unwrap();
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    window.apply(Intent::Run(ActionId::ShowIssues));
    window.apply(Intent::RefreshIssues);
    assert!(window.shows("Files · Error"));
    assert!(window.shows("Locate File…"));
}
