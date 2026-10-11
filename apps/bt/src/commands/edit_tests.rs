//! `bt edit` on files: the result written and the previous version kept for `bt undo`; dry runs
//! and edits that change nothing write nothing; a failing line writes nothing and says which;
//! `--out`; several maps with `--do`; `--with` and `--view`; a read-only file; a map with
//! AutoMap layers, whose project file keeps what was painted.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_core::{Tileset, WangSetType};
use bt_test_data::Scratch;
use bt_test_data::fixtures::{Atlas, MapBuilder, TerrainSetBuilder, combination_tile};
use bt_tmx::WriteRequest;
use glam::UVec2;

use super::*;
use crate::cli::Command;
use crate::folders::Folders;

/// Folders of a test: history and locks in the scratch folder, no home.
fn folders(scratch: &Scratch) -> Folders {
    std::fs::create_dir_all(scratch.join("locks")).unwrap();
    Folders {
        home: None,
        data: Some(scratch.join("data")),
        locks: Some(scratch.join("locks")),
        recovery: None,
    }
}

/// Runs `bt` with `args` offline.
fn bt(args: &[&str], folders: &Folders) -> anyhow::Result<Printed> {
    let run = Run::of_args(args, folders.clone());
    match &run.cli.command {
        Command::Edit(edit_args) => edit(&run, edit_args),
        Command::Undo(undo_args) => crate::commands::undo::undo(&run, undo_args),
        _ => panic!("an edit or an undo"),
    }
}

/// An 8×6 map of grass on `ground`, its tiles labelled with grass and water at their corners,
/// written as `name` in the scratch folder.
fn grass_map(scratch: &Scratch, name: &str) -> PathBuf {
    let set = TerrainSetBuilder::new(WangSetType::Corner)
        .name("Land")
        .terrains(&["Grass", "Water"])
        .every_combination();
    let atlas = Atlas::builder()
        .name("ground")
        .grid(4, 4)
        .build_in(scratch.path())
        .unwrap();
    let tileset = set.added_to(Tileset::clone(&atlas.tileset));
    let grass = combination_tile(2, [1, 1, 1, 1]).unwrap().0.to_string();
    let row = [grass.as_str(); 8].join(" ");
    let rows = vec![row; 6].join("\n");
    let map = MapBuilder::new(UVec2::splat(16), UVec2::new(8, 6))
        .tileset(Arc::new(tileset))
        .tile_layer("ground", &rows)
        .build()
        .unwrap();
    let path = scratch.join(name);
    WriteRequest::for_map(&map, &path).write().unwrap();
    path
}

fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn an_edit_writes_the_file_keeps_the_last_version_and_bt_undo_puts_it_back() {
    let scratch = Scratch::new("bt-edit-write").unwrap();
    let folders = folders(&scratch);
    let map = grass_map(&scratch, "a.tmx");
    let original = std::fs::read(&map).unwrap();
    let printed = bt(
        &["edit", text(&map), "--do", "paint water rect 2,2 3x2"],
        &folders,
    )
    .unwrap();
    assert_eq!(printed.status, Status::Done);
    let report = &printed.text;
    assert!(
        report.starts_with("a.tmx: 1 operation\n  ground: "),
        "{report}"
    );
    assert!(
        report.contains("no new seams or hidden edges, and no objects on other ground"),
        "{report}"
    );
    assert!(
        report.ends_with(&format!(
            "saved {}; bt undo {} puts it back\n",
            map.display(),
            map.display()
        )),
        "{report}"
    );
    assert_ne!(std::fs::read(&map).unwrap(), original);
    let undone = bt(&["undo", text(&map)], &folders).unwrap();
    assert_eq!(undone.status, Status::Done, "{}", undone.text);
    assert_eq!(std::fs::read(&map).unwrap(), original);
}

#[test]
fn a_dry_run_and_an_edit_that_changes_nothing_write_nothing() {
    let scratch = Scratch::new("bt-edit-nothing").unwrap();
    let folders = folders(&scratch);
    let map = grass_map(&scratch, "a.tmx");
    let original = std::fs::read(&map).unwrap();
    let dry = bt(
        &[
            "edit",
            text(&map),
            "--dry-run",
            "--do",
            "paint water rect 2,2 3x2",
        ],
        &folders,
    )
    .unwrap();
    assert!(dry.text.contains("ground: "), "{}", dry.text);
    assert!(
        dry.text.ends_with("dry run: nothing saved\n"),
        "{}",
        dry.text
    );
    let same = bt(&["edit", text(&map), "--do", "paint grass all"], &folders).unwrap();
    assert!(same.text.ends_with("nothing to save\n"), "{}", same.text);
    assert_eq!(std::fs::read(&map).unwrap(), original);
    let nothing = bt(&["undo", text(&map)], &folders).unwrap();
    assert_eq!(nothing.status, Status::Failed, "nothing was kept to undo");
}

#[test]
fn a_failing_line_writes_nothing_and_says_which() {
    let scratch = Scratch::new("bt-edit-failing").unwrap();
    let folders = folders(&scratch);
    let map = grass_map(&scratch, "a.tmx");
    let original = std::fs::read(&map).unwrap();
    let error = bt(
        &[
            "edit",
            text(&map),
            "--do",
            "paint water rect 0,0 2x2; paint lava all",
        ],
        &folders,
    )
    .err()
    .unwrap();
    assert!(
        format!("{error:#}").starts_with("line 2 (paint lava all): "),
        "{error:#}"
    );
    assert_eq!(std::fs::read(&map).unwrap(), original);
    let script_error = bt(&["edit", text(&map), "--do", "paint"], &folders)
        .err()
        .unwrap();
    assert!(
        format!("{script_error:#}").starts_with("line 1: "),
        "{script_error:#}"
    );
}

#[test]
fn out_writes_another_file_and_leaves_the_map_as_it_was() {
    let scratch = Scratch::new("bt-edit-out").unwrap();
    let folders = folders(&scratch);
    let map = grass_map(&scratch, "a.tmx");
    let original = std::fs::read(&map).unwrap();
    let copy = scratch.join("try.tmx");
    let printed = bt(
        &[
            "edit",
            text(&map),
            "--out",
            text(&copy),
            "--do",
            "paint water rect 2,2 3x2",
        ],
        &folders,
    )
    .unwrap();
    assert!(
        printed
            .text
            .ends_with(&format!("saved {}\n", copy.display())),
        "a new file has no earlier version to put back: {}",
        printed.text
    );
    assert_eq!(std::fs::read(&map).unwrap(), original);
    assert!(std::fs::read_to_string(&copy).unwrap().contains("<map"));
    let again = bt(
        &[
            "edit",
            text(&map),
            "--out",
            text(&copy),
            "--do",
            "paint water rect 0,0 2x2",
        ],
        &folders,
    )
    .unwrap();
    assert!(
        again
            .text
            .ends_with(&format!("bt undo {} puts it back\n", copy.display())),
        "{}",
        again.text
    );
}

#[test]
fn several_maps_take_the_operations_in_turn_and_the_first_failure_stops_the_rest() {
    let scratch = Scratch::new("bt-edit-several").unwrap();
    let folders = folders(&scratch);
    let a = grass_map(&scratch, "a.tmx");
    let b = grass_map(&scratch, "b.tmx");
    let printed = bt(
        &[
            "--json",
            "edit",
            text(&a),
            text(&b),
            "--do",
            "layer add sky tiles",
        ],
        &folders,
    )
    .unwrap();
    let both: serde_json::Value = serde_json::from_str(&printed.text).unwrap();
    assert_eq!(both.as_array().map(Vec::len), Some(2), "{}", printed.text);
    assert_eq!(both[0]["schema"], "bt.edit/1");

    let plain = scratch.join("plain.tmx");
    let empty = MapBuilder::new(UVec2::splat(16), UVec2::new(4, 4))
        .build()
        .unwrap();
    WriteRequest::for_map(&empty, &plain).write().unwrap();
    let c = grass_map(&scratch, "c.tmx");
    let c_before = std::fs::read(&c).unwrap();
    let stopped = bt(
        &[
            "edit",
            text(&a),
            text(&plain),
            text(&c),
            "--do",
            "paint water rect 1,1 2x2",
        ],
        &folders,
    )
    .unwrap();
    assert_eq!(stopped.status, Status::Failed);
    assert!(
        stopped.text.starts_with("a.tmx: 1 operation"),
        "{}",
        stopped.text
    );
    let failure = stopped.failure.unwrap();
    assert!(failure.contains("plain.tmx: line 1"), "{failure}");
    assert!(failure.ends_with("(a.tmx edited)"), "{failure}");
    assert_eq!(
        std::fs::read(&c).unwrap(),
        c_before,
        "c.tmx was not reached"
    );
}

#[test]
fn definitions_and_the_changed_area_as_text() {
    let scratch = Scratch::new("bt-edit-with").unwrap();
    let folders = folders(&scratch);
    let map = grass_map(&scratch, "a.tmx");
    let with = scratch.join("names.txt");
    std::fs::write(&with, "define pond = area rect 2,2 3x2\n").unwrap();
    let printed = bt(
        &[
            "edit",
            text(&map),
            "--with",
            text(&with),
            "--view",
            "--do",
            "paint water where pond",
        ],
        &folders,
    )
    .unwrap();
    assert!(
        printed.text.contains("the changed area"),
        "{}",
        printed.text
    );
    std::fs::write(&with, "paint water all\n").unwrap();
    let error = bt(
        &[
            "edit",
            text(&map),
            "--with",
            text(&with),
            "--do",
            "paint grass all",
        ],
        &folders,
    )
    .err()
    .unwrap();
    assert!(
        format!("{error:#}").starts_with("--with names.txt, line 1"),
        "{error:#}"
    );
}

#[test]
fn the_editor_s_options_fail_without_an_editor_and_usage_is_said() {
    let scratch = Scratch::new("bt-edit-usage").unwrap();
    let folders = folders(&scratch);
    let map = grass_map(&scratch, "a.tmx");
    let original = std::fs::read(&map).unwrap();
    for option in [["--in-editor", ""], ["--mark", "plan"]] {
        let mut args = vec!["edit", text(&map)];
        args.extend(option.iter().filter(|a| !a.is_empty()));
        args.extend(["--do", "paint water rect 2,2 3x2"]);
        let error = bt(&args, &folders).err().unwrap();
        assert!(
            format!("{error:#}").contains("is not open in the editor"),
            "{error:#}"
        );
    }
    assert_eq!(std::fs::read(&map).unwrap(), original);
    let script = scratch.join("s.txt");
    std::fs::write(&script, "paint water all\n").unwrap();
    let three = bt(
        &["edit", text(&map), text(&script), text(&script)],
        &folders,
    )
    .unwrap();
    assert_eq!(three.status, Status::Usage);
    assert!(three.failure.unwrap().contains("--do"));
    let out = bt(
        &[
            "edit",
            text(&map),
            text(&map),
            "--out",
            "x.tmx",
            "--do",
            "paint water all",
        ],
        &folders,
    )
    .unwrap();
    assert_eq!(out.status, Status::Usage);
}

#[cfg(unix)]
#[test]
fn a_read_only_file_is_refused_and_nothing_is_kept() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("bt-edit-read-only").unwrap();
    let folders = folders(&scratch);
    let map = grass_map(&scratch, "a.tmx");
    std::fs::set_permissions(&map, std::fs::Permissions::from_mode(0o444)).unwrap();
    let error = bt(
        &["edit", text(&map), "--do", "paint water rect 2,2 3x2"],
        &folders,
    )
    .err()
    .unwrap();
    std::fs::set_permissions(&map, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(format!("{error:#}").contains("read-only"), "{error:#}");
    let history = History::new(folders.history().unwrap());
    assert_eq!(history.kept_edits(&map).unwrap(), 0);
}

#[test]
fn a_map_with_automap_layers_keeps_what_was_painted_in_its_project_file() {
    let Some(examples) = bt_test_data::test_data("reference/tiled/examples") else {
        return;
    };
    let scratch = Scratch::new("bt-edit-automap-layers").unwrap();
    let folders = folders(&scratch);
    let folder = scratch
        .mirror(&examples.join("sewer_automap"), "sewer_automap")
        .unwrap();
    let map = folder.join("sewers.tmx");
    let original = std::fs::read(&map).unwrap();
    let started = bt(&["edit", text(&map), "--do", "automap layers"], &folders).unwrap();
    assert!(
        started.text.contains(&format!(
            "saved {}, and what was painted in better-tiled.json",
            map.display()
        )),
        "{}",
        started.text
    );
    let project = folder.join("better-tiled.json");
    let record = std::fs::read_to_string(&project).unwrap();
    assert!(record.contains("sewers.tmx"), "{record}");

    // The next edit reads what was painted back from the project file.
    let cleared = bt(
        &["edit", text(&map), "--do", "clear rect 0,0 31x31 layer set"],
        &folders,
    )
    .unwrap();
    assert!(
        cleared.text.contains("what was painted (the changes above"),
        "{}",
        cleared.text
    );
    bt(&["undo", text(&map)], &folders).unwrap();
    bt(&["undo", text(&map)], &folders).unwrap();
    assert_eq!(std::fs::read(&map).unwrap(), original);
    let after = std::fs::read_to_string(&project).unwrap();
    assert!(
        !after.contains("sewers.tmx"),
        "the record went with the edit: {after}"
    );
}
