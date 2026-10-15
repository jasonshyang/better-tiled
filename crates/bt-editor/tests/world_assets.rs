//! World positions share map history and the world's own save, session and recovery file.

#![cfg(test)]
mod common;

use bt_core::{FileRef, World, WorldMap};
use bt_editor::{ActionId, Intent};
use bt_test_data::Scratch;
use bt_utils::IRect;
use common::{apply, editor_in, write_map};
use glam::IVec2;

#[test]
fn a_world_opens_its_first_map_and_its_edits_undo_save_and_recover_with_that_map() {
    let scratch = Scratch::new("world-asset-history").unwrap();
    let map = write_map(scratch.path(), "map.tmx");
    let path = scratch.join("game.world");
    let world = World {
        maps: vec![WorldMap {
            file: FileRef::new("map.tmx", &map),
            rect: IRect::from_min_size(IVec2::ZERO, IVec2::new(48, 32)),
        }],
        ..World::default()
    };
    bt_tmx::WriteRequest::for_world(&world, &path)
        .write()
        .unwrap();
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path.clone()));
    let doc = editor.current().unwrap();
    assert_eq!(editor.path_of(doc), Some(map));
    let asset = editor.world_of(doc).unwrap();
    editor
        .transact(doc, "Move Map", |tx| {
            tx.world_mut(asset)?.maps[0].rect.x = 64;
            Ok::<_, bt_editor::EditError>(())
        })
        .unwrap();
    assert_eq!(editor.world_maps(asset)[0].rect.x, 64);
    assert_eq!(editor.session().worlds, vec![path.clone()]);
    assert!(
        editor
            .snapshots_for_unanswered_exit()
            .iter()
            .any(|snapshot| snapshot.meta.original == Some(path.clone())
                && snapshot.text.contains("\"x\": 64"))
    );
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert_eq!(editor.world_maps(asset)[0].rect.x, 0);
    apply(&mut editor, Intent::Run(ActionId::Redo));
    apply(&mut editor, Intent::Run(ActionId::Save));
    assert_eq!(
        bt_tmx::Loader::default()
            .load_world(&path)
            .unwrap()
            .value
            .maps[0]
            .rect
            .x,
        64
    );
    assert!(!editor.has_unsaved());
}

#[test]
fn resizing_an_isometric_map_moves_its_world_by_projected_bounds_and_undoes_both() {
    use bt_core::Orientation;
    use bt_test_data::fixtures::MapBuilder;
    use glam::UVec2;
    let scratch = Scratch::new("world-resize-isometric").unwrap();
    let map = MapBuilder::new(UVec2::new(32, 16), UVec2::splat(4))
        .orientation(Orientation::Isometric)
        .tile_layer("ground", ".")
        .build()
        .unwrap();
    let map_path = scratch.join("iso.tmx");
    bt_tmx::WriteRequest::for_map(&map, &map_path)
        .write()
        .unwrap();
    let path = scratch.join("iso.world");
    let rect = IRect::from_min_size(
        IVec2::new(100, 200),
        map.geometry().map_bounding_rect().size(),
    );
    let world = World {
        maps: vec![WorldMap {
            file: FileRef::new("iso.tmx", &map_path),
            rect,
        }],
        ..World::default()
    };
    bt_tmx::WriteRequest::for_world(&world, &path)
        .write()
        .unwrap();
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path));
    let doc = editor.current().unwrap();
    let world = editor.world_of(doc).unwrap();
    let size = UVec2::new(7, 6);
    let offset = IVec2::new(2, 1);
    let bounds = map
        .geometry()
        .bounding_rect(IRect::from_min_size(-offset, size.as_ivec2()));
    let expected = rect.min() + bounds.min() - map.geometry().map_bounding_rect().min();
    apply(
        &mut editor,
        Intent::ResizeMap {
            doc,
            size,
            offset,
            remove_objects_outside: false,
        },
    );
    assert_eq!(editor.world_maps(world)[0].rect.min(), expected);
    assert_eq!(
        editor.world_maps(world)[0].rect.size(),
        editor
            .map_of(doc)
            .unwrap()
            .geometry()
            .map_bounding_rect()
            .size()
    );
    assert_eq!(
        editor
            .document(doc)
            .unwrap()
            .history()
            .undoable()
            .unwrap()
            .changes()
            .len(),
        2
    );
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert_eq!(editor.world_maps(world)[0].rect, rect);
    assert_eq!(editor.map_of(doc).unwrap().width, 4);
    apply(&mut editor, Intent::Run(ActionId::Redo));
    assert_eq!(editor.world_maps(world)[0].rect.min(), expected);
}
