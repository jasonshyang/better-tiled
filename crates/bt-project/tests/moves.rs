//! Keys that move (`09-project-file.md`, §8): Save As takes a map's entry along within a file
//! and across files, a copy written with `bt edit --out` leaves the original's entry alone, and
//! entries whose map is gone are listed as orphans, never dropped.

#![cfg(test)]

mod common;

use std::fs;
use std::path::Path;

use bt_project::{Document, EntryPlace, FILE_NAME, ProjectFile};
use bt_test_data::Scratch;

use common::{lock_folder, record, write};

/// The place of the map at `map`'s entry, with the scratch folder as home.
fn place(scratch: &Scratch, map: &Path) -> EntryPlace {
    EntryPlace::for_map(map, None, Some(scratch.path())).unwrap()
}

/// The keys the project file at `file` has entries for.
fn keys(file: &Path) -> Vec<String> {
    ProjectFile::load(file)
        .unwrap()
        .keys()
        .map(str::to_owned)
        .collect()
}

#[test]
fn save_as_moves_an_entry_within_a_file() {
    let scratch = Scratch::new("bt-project-move-within").unwrap();
    let locks = lock_folder(&scratch);
    let levels = scratch.join("game/levels");
    write(&levels.join("a.tmx"), "");
    write(&levels.join("keep.tmx"), "");
    let file = levels.join(FILE_NAME);
    ProjectFile::edit(&file, &locks, |document| {
        document.set_automap("a.tmx", record("a"));
        document.set_automap("keep.tmx", record("keep"));
    })
    .unwrap();
    let before = ProjectFile::load(&file).unwrap();

    let from = place(&scratch, &levels.join("a.tmx"));
    let to = place(&scratch, &levels.join("sub/b.tmx"));
    assert_eq!(
        (from.file.as_path(), to.file.as_path()),
        (file.as_path(), file.as_path())
    );
    assert_eq!(to.key, "sub/b.tmx");
    assert!(ProjectFile::move_entry(&from, &to, &locks).unwrap());
    let after = ProjectFile::load(&file).unwrap();
    assert_eq!(keys(&file), ["keep.tmx", "sub/b.tmx"]);
    assert_eq!(after.entry("sub/b.tmx"), before.entry("a.tmx"));
    assert_eq!(after.entry("keep.tmx"), before.entry("keep.tmx"));

    // Saved over a map that had an entry: the moved entry replaces it.
    assert!(
        ProjectFile::move_entry(&to, &place(&scratch, &levels.join("keep.tmx")), &locks).unwrap()
    );
    assert_eq!(keys(&file), ["keep.tmx"]);
    assert_eq!(
        ProjectFile::load(&file)
            .unwrap()
            .automap("keep.tmx")
            .unwrap()
            .rules,
        "a"
    );
}

#[test]
fn save_as_moves_an_entry_across_files() {
    let scratch = Scratch::new("bt-project-move-across").unwrap();
    let locks = lock_folder(&scratch);
    let (west, east) = (scratch.join("game/west"), scratch.join("game/east"));
    write(&west.join("x.tmx"), "");
    write(&west.join("stays.tmx"), "");
    fs::create_dir_all(&east).unwrap();
    let west_file = west.join(FILE_NAME);
    ProjectFile::edit(&west_file, &locks, |document| {
        document.set_automap("x.tmx", record("x"));
        document.set_automap("stays.tmx", record("stays"));
    })
    .unwrap();
    let moved = ProjectFile::load(&west_file)
        .unwrap()
        .entry("x.tmx")
        .cloned();

    let from = place(&scratch, &west.join("x.tmx"));
    let to = place(&scratch, &east.join("y.tmx"));
    assert_eq!(to.file, east.join(FILE_NAME), "a new file next to the map");
    assert!(ProjectFile::move_entry(&from, &to, &locks).unwrap());
    assert_eq!(keys(&west_file), ["stays.tmx"]);
    assert_eq!(keys(&to.file), ["y.tmx"]);
    assert_eq!(
        ProjectFile::load(&to.file).unwrap().entry("y.tmx").cloned(),
        moved
    );

    // And back, emptying the second file: it stays as the empty document.
    assert!(ProjectFile::move_entry(&to, &from, &locks).unwrap());
    assert_eq!(keys(&west_file), ["stays.tmx", "x.tmx"]);
    assert_eq!(keys(&to.file), Vec::<String>::new());
    assert!(to.file.is_file());
}

#[test]
fn nothing_to_move_writes_nothing() {
    let scratch = Scratch::new("bt-project-move-nothing").unwrap();
    let locks = lock_folder(&scratch);
    let from = place(&scratch, &scratch.join("west/x.tmx"));
    let to = place(&scratch, &scratch.join("east/y.tmx"));
    fs::create_dir_all(scratch.join("east")).unwrap();
    assert!(!ProjectFile::move_entry(&from, &to, &locks).unwrap());
    assert!(!ProjectFile::move_entry(&from, &from, &locks).unwrap());
    assert!(!from.file.exists() && !to.file.exists());
}

#[cfg(unix)]
#[test]
fn a_move_within_one_file_reached_through_a_link_loses_nothing() {
    let scratch = Scratch::new("bt-project-move-link").unwrap();
    let locks = lock_folder(&scratch);
    let real = scratch.join("real");
    write(&real.join("a.tmx"), "");
    std::os::unix::fs::symlink(&real, scratch.join("link")).unwrap();
    let file = real.join(FILE_NAME);
    ProjectFile::edit(&file, &locks, |document| {
        document.set_automap("a.tmx", record("a"))
    })
    .unwrap();
    let through_link = EntryPlace {
        file: scratch.join("link").join(FILE_NAME),
        key: "a.tmx".to_owned(),
    };
    let direct = EntryPlace {
        file: file.clone(),
        key: "a.tmx".to_owned(),
    };
    assert!(ProjectFile::move_entry(&through_link, &direct, &locks).unwrap());
    assert_eq!(keys(&file), ["a.tmx"], "the same file and key: kept");
    let renamed = EntryPlace {
        key: "b.tmx".to_owned(),
        ..direct.clone()
    };
    assert!(ProjectFile::move_entry(&through_link, &renamed, &locks).unwrap());
    assert_eq!(keys(&file), ["b.tmx"]);
}

#[test]
fn a_copy_written_with_out_leaves_the_originals_entry_alone() {
    let scratch = Scratch::new("bt-project-out").unwrap();
    let locks = lock_folder(&scratch);
    let maps = scratch.join("maps");
    write(&maps.join("level.tmx"), "");
    let original = place(&scratch, &maps.join("level.tmx"));
    ProjectFile::edit(&original.file, &locks, |document| {
        document.set_automap(&original.key, record("original"));
    })
    .unwrap();
    // `bt edit --out` writes the copy's record under the copy's key.
    let copy = place(&scratch, &maps.join("copies/level-2.tmx"));
    assert_eq!(copy.file, original.file);
    ProjectFile::edit(&copy.file, &locks, |document| {
        document.set_automap(&copy.key, record("copy"));
    })
    .unwrap();
    let document = ProjectFile::load(&original.file).unwrap();
    assert_eq!(document.automap("level.tmx").unwrap().rules, "original");
    assert_eq!(
        document.automap("copies/level-2.tmx").unwrap().rules,
        "copy"
    );
}

#[test]
fn orphans_are_listed_and_never_dropped() {
    let scratch = Scratch::new("bt-project-orphans-listed").unwrap();
    let locks = lock_folder(&scratch);
    write(&scratch.join("here.tmx"), "");
    let file = scratch.join(FILE_NAME);
    ProjectFile::edit(&file, &locks, |document| {
        for key in ["here.tmx", "gone.tmx", "moved/old.tmx"] {
            document.set_automap(key, record(key));
        }
    })
    .unwrap();
    let document = ProjectFile::load(&file).unwrap();
    assert_eq!(document.orphans(&file), ["gone.tmx", "moved/old.tmx"]);

    // Editing another entry keeps them; removing one is the person's choice.
    ProjectFile::edit(&file, &locks, |document| {
        document.set_automap("here.tmx", record("changed"));
    })
    .unwrap();
    assert_eq!(keys(&file), ["gone.tmx", "here.tmx", "moved/old.tmx"]);
    let removed = ProjectFile::edit(&file, &locks, |document: &mut Document| {
        document.remove_entry("gone.tmx")
    })
    .unwrap();
    assert_eq!(
        removed.and_then(|entry| entry.automap().cloned()),
        Some(record("gone.tmx"))
    );
    let document = ProjectFile::load(&file).unwrap();
    assert_eq!(document.orphans(&file), ["moved/old.tmx"]);

    // A map moved back finds its entry again.
    write(&scratch.join("moved/old.tmx"), "");
    assert_eq!(document.orphans(&file), Vec::<&str>::new());
}
