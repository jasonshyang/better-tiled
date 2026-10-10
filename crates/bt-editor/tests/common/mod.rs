//! What the editor's integration tests share: maps written to scratch folders, an editor
//! whose loads run as the window's workers run them, and finding the question asked.

#![allow(dead_code, reason = "each test file uses some of these")]

use std::path::{Path, PathBuf};

use bt_core::{FileRef, TilesetRef};
use bt_editor::{Editor, Effect, Intent};
use bt_test_data::Scratch;
use bt_test_data::fixtures::{Atlas, MapBuilder};
use bt_tmx::WriteRequest;
use glam::UVec2;

/// A map with an external tileset, written in `folder`.
pub fn write_map(folder: &Path, name: &str) -> PathBuf {
    let atlas = Atlas::builder().name("tiles").build_in(folder).unwrap();
    let tileset_path = folder.join("tiles.tsx");
    WriteRequest::for_tileset(&atlas.tileset, &tileset_path)
        .write()
        .unwrap();
    let mut map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(3, 2))
        .tileset(atlas.tileset.clone())
        .tile_layer("ground", "0 1 2\n3 4 5")
        .tile_layer("top", ". 6 .\n. . .")
        .build()
        .unwrap();
    map.tilesets[0] = TilesetRef {
        file: Some(FileRef::new("tiles.tsx", &tileset_path)),
        first_gid: Some(1),
        tileset: atlas.tileset.clone(),
    };
    let path = folder.join(name);
    WriteRequest::for_map(&map, &path).write().unwrap();
    path
}

/// Applies an intent and carries out loads and reads of rules as a worker would, returning the
/// other effects.
pub fn apply(editor: &mut Editor, intent: Intent) -> Vec<Effect> {
    let mut pending = vec![intent];
    let mut out = Vec::new();
    while let Some(intent) = pending.pop() {
        for effect in editor.apply(intent) {
            match effect {
                Effect::Load(job) => pending.push(Intent::Loaded(Box::new(job.run()))),
                Effect::LoadRules(job) => pending.push(Intent::RulesLoaded(Box::new(job.run()))),
                other => out.push(other),
            }
        }
    }
    out
}

pub fn editor_in(scratch: &Scratch) -> Editor {
    std::fs::create_dir_all(scratch.join("locks")).unwrap();
    Editor::builder()
        .recovery_folder(scratch.join("recovery"))
        .pid(1000)
        .home(Some(scratch.path().to_path_buf()))
        .lock_folder(scratch.join("locks"))
        .build()
        .unwrap()
}

pub fn ask(effects: &[Effect]) -> &bt_editor::Decision {
    effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Ask(decision) => Some(decision),
            _ => None,
        })
        .expect("a question")
}
