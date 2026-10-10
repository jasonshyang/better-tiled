//! New Tileset from RPG Maker autotiles through the editor, on sheets drawn in a scratch folder:
//! the sheet is put together by the effect's job (as a worker runs it), the result intent adds
//! the tileset `bt-terrain` makes as one step, and a sheet that cannot be put together says why.

#![cfg(test)]

use std::path::{Path, PathBuf};

use bt_core::{TilesetImage, WangSetType};
use bt_editor::{
    ActionId, DocumentId, Editor, Effect, Intent, NewMap, NewTileset, NewTilesetSource,
};
use bt_image::DecodedImage;
use bt_terrain::rpg_maker;
use bt_test_data::Scratch;
use bt_test_data::fixtures::{blank_picture, paint_a2_autotile};
use glam::UVec2;

/// The tile size of the drawn sheets.
const TILE: u32 = 12;
const GRASS: [u8; 4] = [80, 160, 70, 255];

/// A sheet of `columns`×`rows` autotiles, those listed drawn, written as `name` in `folder`.
fn sheet(folder: &Path, name: &str, columns: u32, drawn: &[u32]) -> PathBuf {
    let mut picture = blank_picture(columns * 2 * TILE, 3 * TILE);
    for &column in drawn {
        paint_a2_autotile(&mut picture, UVec2::new(column * 2, 0), TILE, GRASS);
    }
    let image = DecodedImage {
        width: picture.width(),
        height: picture.height(),
        rgba: picture.into_raw(),
    };
    let path = folder.join(name);
    bt_image::write_png(&path, &image).unwrap();
    path
}

/// An editor with a new map open.
fn editor() -> (Editor, DocumentId) {
    let mut editor = Editor::new();
    editor.apply(Intent::CreateMap(NewMap::default()));
    let doc = editor.current().unwrap();
    (editor, doc)
}

fn autotiles(sheet: &Path, columns: u32) -> NewTileset {
    NewTileset {
        name: "Floors".into(),
        tile_size: UVec2::splat(TILE),
        source: NewTilesetSource::RpgMakerAutotiles {
            sheet: sheet.to_path_buf(),
            sheet_size: UVec2::new(columns * 2 * TILE, 3 * TILE),
        },
    }
}

/// Asks for the tileset; the job the editor hands the worker.
fn job_for(editor: &mut Editor, doc: DocumentId, new: NewTileset) -> bt_editor::AutotilesJob {
    let effects = editor.apply(Intent::CreateTileset { doc, tileset: new });
    match effects.as_slice() {
        [Effect::ExpandAutotiles(job)] => job.clone(),
        other => panic!("{other:?}"),
    }
}

fn notice_text(effects: &[Effect]) -> &str {
    match effects {
        [Effect::Notify(notice)] => &notice.text,
        other => panic!("{other:?}"),
    }
}

#[test]
fn autotiles_become_a_tileset_of_the_map_in_one_step() {
    let scratch = Scratch::new("editor-rpg-maker").unwrap();
    let path = sheet(scratch.path(), "Floors_A2.png", 2, &[0, 1]);
    let (mut editor, doc) = editor();

    let job = job_for(&mut editor, doc, autotiles(&path, 2));
    assert_eq!(job.doc(), doc);
    assert!(
        editor.map_of(doc).unwrap().tilesets.is_empty(),
        "nothing until the sheet is put together"
    );
    let expanded = job.run();
    let written = scratch.join("Floors_A2 blob.png");
    assert!(written.is_file(), "written beside the sheet");
    let effects = editor.apply(Intent::AutotilesExpanded(Box::new(expanded)));
    assert_eq!(
        notice_text(&effects),
        "Wrote “Floors_A2 blob.png” beside “Floors_A2.png”: one terrain of “Floors” per \
         autotile, 2 autotiles"
    );

    let map = editor.map_of(doc).unwrap();
    let [added] = map.tilesets.as_slice() else {
        panic!("one tileset: {:?}", map.tilesets.len());
    };
    assert!(added.file.is_none(), "embedded, as New Tileset makes them");
    let ours = &added.tileset;
    // What bt-terrain makes of the same sheet elsewhere.
    let elsewhere = scratch.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let copy = sheet(&elsewhere, "Floors_A2.png", 2, &[0, 1]);
    let theirs = rpg_maker::tileset(&rpg_maker::expand(&copy, TILE).unwrap(), "Floors");
    assert_eq!(ours.wang_sets, theirs.wang_sets);
    assert_eq!(
        (ours.name.as_str(), ours.tile_count(), ours.columns()),
        (theirs.name.as_str(), theirs.tile_count(), theirs.columns())
    );
    let TilesetImage::Atlas { image, .. } = &ours.image else {
        panic!("an atlas");
    };
    assert_eq!(image.path(), Some(written.as_path()));
    let set = &ours.wang_sets[0];
    assert_eq!((set.kind, set.colors.len()), (WangSetType::Mixed, 2));
    let steps: Vec<String> = editor
        .document(doc)
        .unwrap()
        .history()
        .entries()
        .into_iter()
        .map(|e| e.label)
        .collect();
    assert_eq!(steps, ["New Tileset"]);

    editor.apply(Intent::Run(ActionId::Undo));
    assert!(editor.map_of(doc).unwrap().tilesets.is_empty(), "one step");
    assert!(written.is_file(), "the image written stays");

    // Again: the image is written under a name of its own.
    let job = job_for(&mut editor, doc, autotiles(&path, 2));
    let effects = editor.apply(Intent::AutotilesExpanded(Box::new(job.run())));
    assert!(notice_text(&effects).starts_with("Wrote “Floors_A2 blob 2.png”"));
}

#[test]
fn a_sheet_that_cannot_be_put_together_says_why() {
    let scratch = Scratch::new("editor-rpg-maker-empty").unwrap();
    let empty = sheet(scratch.path(), "Empty_A2.png", 1, &[]);
    let (mut editor, doc) = editor();
    let job = job_for(&mut editor, doc, autotiles(&empty, 1));
    let effects = editor.apply(Intent::AutotilesExpanded(Box::new(job.run())));
    assert_eq!(
        notice_text(&effects),
        "Cannot make autotiles of “Empty_A2.png”: every autotile of the sheet is empty"
    );
    assert!(editor.map_of(doc).unwrap().tilesets.is_empty());
    assert!(!scratch.join("Empty_A2 blob.png").exists());

    // Refused before any work: an odd tile size.
    let odd = NewTileset {
        tile_size: UVec2::splat(TILE + 1),
        ..autotiles(&empty, 1)
    };
    let effects = editor.apply(Intent::CreateTileset { doc, tileset: odd });
    assert!(notice_text(&effects).contains("even"));
}
