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

#[test]
fn a_map_adopts_nearby_worlds_and_loads_visible_neighbours_once_without_tabs() {
    use bt_utils::DRect;
    use glam::DVec2;
    let scratch = Scratch::new("world-neighbour-cache").unwrap();
    let first = write_map(scratch.path(), "first.tmx");
    let second = write_map(scratch.path(), "second.tmx");
    let far = write_map(scratch.path(), "far.tmx");
    let world_path = scratch.join("game.world");
    let entry = |path: &std::path::Path, x| WorldMap {
        file: FileRef::new(path.file_name().unwrap().to_str().unwrap(), path),
        rect: IRect::from_min_size(IVec2::new(x, 0), IVec2::new(48, 32)),
    };
    let world = World {
        maps: vec![entry(&first, 0), entry(&second, 48), entry(&far, 4800)],
        ..World::default()
    };
    bt_tmx::WriteRequest::for_world(&world, &world_path)
        .write()
        .unwrap();
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(first.clone()));
    let doc = editor.current().unwrap();
    assert_eq!(editor.path_of(doc), Some(first));
    assert!(editor.world_of(doc).is_some());
    let visible = DRect::from_min_size(DVec2::new(-10.0, -10.0), DVec2::new(150.0, 100.0));
    assert!(editor.needs_world_maps(doc, visible));
    apply(&mut editor, Intent::LoadWorldMaps { doc, visible });
    assert_eq!(editor.documents().in_order().count(), 1);
    assert!(!editor.needs_world_maps(doc, visible));
    let cached = editor.store().by_path(&second).unwrap();
    assert!(
        editor.store().by_path(&far).is_none(),
        "offscreen maps load on demand"
    );
    apply(&mut editor, Intent::Open(second.clone()));
    assert_eq!(
        editor.document(editor.current().unwrap()).unwrap().asset(),
        cached
    );
    let world = editor.world_of(doc).unwrap();
    apply(
        &mut editor,
        Intent::MoveWorldMap {
            doc,
            world,
            path: second,
            position: IVec2::new(64, 0),
        },
    );
    apply(&mut editor, Intent::SelectDocument(doc));
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert_eq!(editor.world_maps(world)[1].rect.x, 48);
}

#[test]
fn create_add_properties_save_and_unload_keep_map_history_usable() {
    use bt_editor::{Choice, FileRequest};
    let scratch = Scratch::new("world-management").unwrap();
    let map = write_map(scratch.path(), "map.tmx");
    let second = write_map(scratch.path(), "second.tmx");
    let path = scratch.join("new.world");
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(map));
    let doc = editor.current().unwrap();
    apply(
        &mut editor,
        Intent::FilePicked {
            request: FileRequest::NewWorld(doc),
            path: Some(path.clone()),
        },
    );
    let world = editor.world_of(doc).unwrap();
    assert_eq!(
        bt_tmx::Loader::default()
            .load_world(&path)
            .unwrap()
            .value
            .maps
            .len(),
        1
    );
    apply(
        &mut editor,
        Intent::FilePicked {
            request: FileRequest::WorldMap { doc, world },
            path: Some(second),
        },
    );
    assert_eq!(editor.world_maps(world).len(), 2);
    let mut value = (**editor.store().world(world).unwrap()).clone();
    value.grid_size = IVec2::splat(8);
    apply(
        &mut editor,
        Intent::SetWorld {
            doc,
            world,
            value: Box::new(value),
        },
    );
    apply(
        &mut editor,
        Intent::ResizeMap {
            doc,
            size: glam::UVec2::new(4, 3),
            offset: IVec2::ZERO,
            remove_objects_outside: false,
        },
    );
    let effects = apply(&mut editor, Intent::Run(ActionId::UnloadWorld));
    let decision = common::ask(&effects).id;
    apply(
        &mut editor,
        Intent::Decided {
            decision,
            choice: Choice::Cancel,
        },
    );
    assert!(editor.world_of(doc).is_some());
    let effects = apply(&mut editor, Intent::Run(ActionId::UnloadWorld));
    let decision = common::ask(&effects).id;
    apply(
        &mut editor,
        Intent::Decided {
            decision,
            choice: Choice::Save,
        },
    );
    assert!(editor.world_of(doc).is_none());
    let saved = bt_tmx::Loader::default().load_world(&path).unwrap().value;
    assert_eq!(saved.grid_size, IVec2::splat(8));
    assert_eq!(saved.maps.len(), 2);
    assert!(
        editor.undo(doc).is_ok(),
        "unloading must not block undoing the map half of resize"
    );
    assert_eq!(editor.map_of(doc).unwrap().width, 3);
}

#[test]
fn pattern_edits_expand_current_unsaved_state_and_ignore_late_worker_results() {
    use bt_core::WorldPattern;
    let scratch = Scratch::new("world-pattern-jobs").unwrap();
    let map = write_map(scratch.path(), "map_0_0.tmx");
    write_map(scratch.path(), "map_1_0.tmx");
    let path = scratch.join("patterns.world");
    let world = World {
        maps: vec![WorldMap {
            file: FileRef::new("map_0_0.tmx", &map),
            rect: IRect::from_min_size(IVec2::ZERO, IVec2::new(48, 32)),
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
    let mut value = (**editor.store().world(world).unwrap()).clone();
    value.patterns.push(WorldPattern {
        multiplier: IVec2::new(48, 32),
        map_size: IVec2::new(48, 32),
        ..WorldPattern::new(r"map_(\d+)_(\d+)\.tmx")
    });
    let jobs = editor.apply(Intent::SetWorld {
        doc,
        world,
        value: Box::new(value),
    });
    let late = jobs
        .into_iter()
        .find_map(|effect| {
            if let bt_editor::Effect::Load(job) = effect {
                Some(job.run())
            } else {
                None
            }
        })
        .unwrap();
    apply(&mut editor, Intent::Run(ActionId::Undo));
    apply(&mut editor, Intent::Loaded(Box::new(late)));
    assert_eq!(
        editor.world_maps(world).len(),
        1,
        "old expansion must not revive an undone pattern"
    );
    apply(&mut editor, Intent::Run(ActionId::Redo));
    assert_eq!(editor.world_maps(world).len(), 2);
    assert!(
        editor.store().get(world).unwrap().is_dirty(),
        "expanding patterns must not replace or mark the definition saved"
    );
    assert_eq!(editor.world_maps(world)[1].rect.x, 48);
}
