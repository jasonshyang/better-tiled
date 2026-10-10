//! Finding a map's project file and where a new one goes (`09-project-file.md`, §4.1 and §4.2),
//! and keys and paths kept in the form the map's path has, through symbolic links (§4.3).

#![cfg(test)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use bt_project::{EntryPlace, FILE_NAME, ProjectFile, paths};
use bt_test_data::Scratch;

use common::write;

/// The project file in `folder`.
fn file_in(folder: &Path) -> PathBuf {
    folder.join(FILE_NAME)
}

#[test]
fn the_nearest_file_wins() {
    let scratch = Scratch::new("bt-project-nearest").unwrap();
    let home = Some(scratch.path());
    let game = scratch.join("game");
    let map = game.join("levels/caves/deep.tmx");
    write(&map, "");
    assert_eq!(ProjectFile::find(&map, home).unwrap(), None);
    write(&file_in(&game), "{}");
    assert_eq!(ProjectFile::find(&map, home).unwrap(), Some(file_in(&game)));
    write(&file_in(&game.join("levels")), "{}");
    assert_eq!(
        ProjectFile::find(&map, home).unwrap(),
        Some(file_in(&game.join("levels")))
    );
    write(&file_in(&game.join("levels/caves")), "{}");
    assert_eq!(
        ProjectFile::find(&map, home).unwrap(),
        Some(file_in(&game.join("levels/caves"))),
        "the map's own folder first"
    );
    fs::create_dir_all(game.join("levels/caves/deep")).unwrap();
    assert_eq!(
        ProjectFile::find(&game.join("levels/caves/deep/x.tmx"), home).unwrap(),
        Some(file_in(&game.join("levels/caves"))),
        "from a folder further down"
    );
}

/// The table of `09-project-file.md`, §4.2.
#[test]
fn a_new_file_goes_next_to_the_open_project_only_when_the_maps_search_reaches_it() {
    let scratch = Scratch::new("bt-project-location").unwrap();
    let home = Some(scratch.path());
    let game = scratch.join("game");
    let map = game.join("levels/caves/deep.tmx");
    write(&map, "");
    let caves = file_in(&game.join("levels/caves"));
    let open_game = game.join("game.tiled-project");
    let open_other = scratch.join("other/x.tiled-project");
    write(&open_game, "{}");
    write(&open_other, "{}");

    assert_eq!(ProjectFile::location(&map, None, home).unwrap(), caves);
    assert_eq!(
        ProjectFile::location(&map, Some(&open_game), home).unwrap(),
        file_in(&game),
        "next to the open project the map is under"
    );
    assert_eq!(
        ProjectFile::location(&map, Some(&open_other), home).unwrap(),
        caves,
        "not next to a project the map is not under"
    );
    let levels = file_in(&game.join("levels"));
    write(&levels, "{}");
    for open in [None, Some(open_game.as_path()), Some(open_other.as_path())] {
        assert_eq!(
            ProjectFile::location(&map, open, home).unwrap(),
            levels,
            "an existing file, whatever is open: {open:?}"
        );
    }
    let place = EntryPlace::for_map(&map, Some(&open_game), home).unwrap();
    assert_eq!(
        place,
        EntryPlace {
            file: levels,
            key: "caves/deep.tmx".to_owned()
        }
    );
}

/// The last row of the table: `~/better-tiled.json` and `~/games/new/.git`, with the map in
/// `~/games/new/maps/`.
#[test]
fn the_search_stops_at_the_repository_top_and_below_home() {
    let scratch = Scratch::new("bt-project-search-stops").unwrap();
    let home = scratch.join("home");
    let repository = home.join("games/new");
    let map = repository.join("maps/level.tmx");
    write(&map, "");
    fs::create_dir_all(repository.join(".git")).unwrap();
    write(&file_in(&home), "{}");
    write(&file_in(&home.join("games")), "{}");
    let find = |map: &Path| ProjectFile::find(map, Some(&home)).unwrap();
    let location = |open: Option<&Path>| ProjectFile::location(&map, open, Some(&home)).unwrap();

    assert_eq!(find(&map), None, "nothing above the repository");
    let next_to_map = file_in(&repository.join("maps"));
    assert_eq!(location(None), next_to_map);
    assert_eq!(
        location(Some(&home.join("games/old.tiled-project"))),
        next_to_map,
        "not next to a project the search does not reach"
    );
    assert_eq!(
        location(Some(&repository.join("new.tiled-project"))),
        file_in(&repository),
        "the repository's own project"
    );
    write(&file_in(&repository), "{}");
    assert_eq!(
        find(&map),
        Some(file_in(&repository)),
        "the repository's top"
    );

    // Without a repository the search goes up to the home folder, not into it.
    fs::remove_dir_all(repository.join(".git")).unwrap();
    fs::remove_file(file_in(&repository)).unwrap();
    assert_eq!(find(&map), Some(file_in(&home.join("games"))));
    fs::remove_file(file_in(&home.join("games"))).unwrap();
    assert_eq!(find(&map), None, "not the home folder's own");
    assert_eq!(
        location(Some(&home.join("stray.tiled-project"))),
        next_to_map,
        "nor next to a project in the home folder"
    );
}

#[test]
fn the_maps_own_folder_is_searched_even_when_it_is_home() {
    let scratch = Scratch::new("bt-project-home-map").unwrap();
    let home = scratch.join("home");
    let map = home.join("top.tmx");
    write(&map, "");
    assert_eq!(ProjectFile::find(&map, Some(&home)).unwrap(), None);
    assert_eq!(
        ProjectFile::location(&map, None, Some(&home)).unwrap(),
        file_in(&home),
        "made next to the map, where it is found again"
    );
    write(&file_in(&home), "{}");
    assert_eq!(
        ProjectFile::find(&map, Some(&home)).unwrap(),
        Some(file_in(&home))
    );
}

#[test]
fn keys_are_relative_with_slashes() {
    let scratch = Scratch::new("bt-project-keys").unwrap();
    let game = scratch.join("game");
    write(&file_in(&game), "{}");
    let place = EntryPlace::for_map(
        &game.join("levels").join("caves").join("deep.tmx"),
        None,
        Some(scratch.path()),
    )
    .unwrap();
    assert_eq!(place.key, "levels/caves/deep.tmx", "/ on every platform");
    assert_eq!(
        paths::key(&place.file, &scratch.join("rules/walls.txt")).unwrap(),
        "../rules/walls.txt"
    );
    assert_eq!(
        paths::resolve(&place.file, "../rules/walls.txt"),
        scratch.join("rules").join("walls.txt")
    );
}

/// The file is found in the form the map's path has, through the same links, and gives paths
/// back in that form: a canonical path would be another path to the same file, and the rules'
/// tilesets would stop matching the map's.
#[cfg(unix)]
#[test]
fn files_keys_and_paths_keep_the_form_the_maps_path_has_through_a_link() {
    let scratch = Scratch::new("bt-project-link-form").unwrap();
    let home = Some(scratch.path());
    let real = scratch.join("real");
    let link = scratch.join("link");
    write(&real.join("game/levels/caves/deep.tmx"), "");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let levels_file = |root: &Path| file_in(&root.join("game/levels"));
    write(&levels_file(&real), "{}");

    for root in [&link, &real] {
        let map = root.join("game/levels/caves/deep.tmx");
        let found = ProjectFile::find(&map, home).unwrap().unwrap();
        assert_eq!(found, levels_file(root), "in the map's form");
        assert_eq!(paths::key(&found, &map).unwrap(), "caves/deep.tmx");
        assert_eq!(
            paths::resolve(&found, "../rules/walls.txt"),
            root.join("game/rules/walls.txt")
        );
    }

    // An open project named through the other form still takes a new file next to it, in the
    // map's form.
    fs::remove_file(levels_file(&real)).unwrap();
    let map = link.join("game/levels/caves/deep.tmx");
    let open = real.join("game/game.tiled-project");
    write(&open, "{}");
    assert_eq!(
        ProjectFile::location(&map, Some(&open), home).unwrap(),
        file_in(&link.join("game"))
    );

    // The system's temporary folder is reached through a link on macOS (/var is
    // /private/var), and Scratch gives its canonical form.
    let plain = std::env::temp_dir();
    let canonical = fs::canonicalize(&plain).unwrap();
    if let Ok(inside) = scratch.path().strip_prefix(&canonical) {
        let map = plain.join(inside).join("real/game/levels/caves/deep.tmx");
        write(&file_in(&real.join("game")), "{}");
        let found = ProjectFile::find(&map, None).unwrap().unwrap();
        assert_eq!(found, plain.join(inside).join("real/game").join(FILE_NAME));
        assert_eq!(paths::key(&found, &map).unwrap(), "levels/caves/deep.tmx");
    }
}
