//! Image layer edits and PNG export through the real window file-dialog and worker paths.

use bt_core::{Color, ImageLayer, ImageSource};
use bt_editor::{ActionId, Attribute, AttributeValue, FileRequest, Inspected, Intent};
use bt_image::{DecodedImage, decode_file, write_png};
use bt_test_data::{
    Scratch,
    fixtures::{Atlas, MapBuilder},
};
use glam::UVec2;

use crate::test_window::TestWindow;

#[test]
fn image_layer_chooser_rebases_undoes_and_preserves_repetition() {
    let scratch = Scratch::new("window-layer-image").unwrap();
    std::fs::create_dir(scratch.join("art")).unwrap();
    let image_path = scratch.join("art/backdrop.png");
    write_png(
        &image_path,
        &DecodedImage {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 0, 255, 255],
        },
    )
    .unwrap();
    let map = MapBuilder::new(UVec2::splat(16), UVec2::new(2, 1))
        .image_layer(
            "backdrop",
            ImageLayer {
                repeat_x: true,
                repeat_y: true,
                ..ImageLayer::default()
            },
        )
        .build()
        .unwrap();
    let path = scratch.join("map.tmx");
    bt_tmx::WriteRequest::for_map(&map, &path).write().unwrap();
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    let doc = window.editor().current().unwrap();
    let layer = window.editor().map_of(doc).unwrap().layers[0].id;
    window.apply(Intent::SelectLayer { doc, layer });
    let inspected = Inspected::Layers(vec![layer]);
    let image_layer = |window: &TestWindow| {
        window
            .editor()
            .map_of(doc)
            .unwrap()
            .layer(layer)
            .unwrap()
            .as_image_layer()
            .unwrap()
            .clone()
    };
    window.click("Choose Image…");
    window.answer_file_dialog(
        FileRequest::AttributeImage {
            doc,
            inspected: inspected.clone(),
        },
        &image_path,
    );
    let chosen = image_layer(&window);
    assert!(chosen.repeat_x && chosen.repeat_y);
    let image = chosen.image.as_ref().unwrap();
    assert_eq!(image.declared_size, UVec2::new(2, 1));
    let ImageSource::File(file) = &image.source else {
        panic!("expected file image")
    };
    assert_eq!(file.source, "art/backdrop.png");
    assert_eq!(file.path, image_path);
    window.apply(Intent::Run(ActionId::Undo));
    assert!(image_layer(&window).image.is_none());
    window.apply(Intent::Run(ActionId::Redo));
    assert_eq!(image_layer(&window), chosen);
    window.click("Choose Image…");
    window.answer_file_dialog(
        FileRequest::AttributeImage { doc, inspected },
        &scratch.join("missing.png"),
    );
    assert_eq!(
        image_layer(&window),
        chosen,
        "a failed probe changes nothing"
    );
    window.apply(Intent::Run(ActionId::Save));
    let restored = bt_tmx::Loader::builder()
        .build()
        .load_map(&path)
        .unwrap()
        .value;
    assert_eq!(restored.layers[0].as_image_layer().unwrap(), &chosen);
    window.click("Clear Image");
    assert!(image_layer(&window).image.is_none());
    window.apply(Intent::Run(ActionId::Undo));
    assert_eq!(image_layer(&window), chosen);
}

#[test]
fn png_export_uses_unsaved_edits_scale_transparency_and_safe_replacement() {
    let scratch = Scratch::new("window-image-export").unwrap();
    let atlas = Atlas::builder().build_in(scratch.path()).unwrap();
    let map = MapBuilder::new(UVec2::splat(16), UVec2::new(2, 1))
        .tileset(atlas.tileset)
        .tile_layer("ground", "0 .")
        .build()
        .unwrap();
    let path = scratch.join("map.tmx");
    bt_tmx::WriteRequest::for_map(&map, &path).write().unwrap();
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    let doc = window.editor().current().unwrap();
    window.apply(Intent::SetAttribute {
        doc,
        inspected: Inspected::Map,
        attribute: Attribute::BackgroundColor,
        value: AttributeValue::Color(Some(Color::rgb(12, 34, 56))),
        merge: None,
    });
    let output = scratch.join("export.png");
    window.apply(Intent::Run(ActionId::ExportAsImage));
    window.click("OK");
    window.answer_file_dialog(
        FileRequest::ExportImage {
            doc,
            scale_percent: 100,
            background: true,
        },
        &output,
    );
    let first = decode_file(&output, None).unwrap();
    assert_eq!((first.width, first.height), (32, 16));
    assert_eq!(&first.rgba[(24 * 4)..(25 * 4)], &[12, 34, 56, 255]);
    assert!(
        window.editor().has_unsaved(),
        "export does not save the map"
    );
    window.apply(Intent::Run(ActionId::ExportAsImage));
    window.click("Include map background colour");
    window.focus("Scale");
    window.press(egui::Modifiers::COMMAND, egui::Key::A);
    window.type_text("200");
    window.press(egui::Modifiers::NONE, egui::Key::Enter);
    window.click("OK");
    window.answer_file_dialog(
        FileRequest::ExportImage {
            doc,
            scale_percent: 200,
            background: false,
        },
        &output,
    );
    let second = decode_file(&output, None).unwrap();
    assert_eq!((second.width, second.height), (64, 32));
    assert_eq!(&second.rgba[(48 * 4)..(49 * 4)], &[0, 0, 0, 0]);
    let mut permissions = std::fs::metadata(&output).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&output, permissions).unwrap();
    window.apply(Intent::Run(ActionId::ExportAsImage));
    window.click("OK");
    window.answer_file_dialog(
        FileRequest::ExportImage {
            doc,
            scale_percent: 100,
            background: true,
        },
        &output,
    );
    assert_eq!(decode_file(&output, None).unwrap(), second);
    assert!(window.shows(&format!(
        "Cannot export image: {}",
        bt_image::replace_png(&output, &first).unwrap_err()
    )));
}
