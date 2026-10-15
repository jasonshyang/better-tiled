//! The images a scene uses (docs/design/12-godot-export.md): an image that can't be read fails the export, writing
//! nothing; a copy two maps make from images of the same bytes serves both, and settles.

use std::path::{Path, PathBuf};

use super::super::contract::Summary;
use super::super::*;
use super::support::*;

/// A map of 32×32 cells painted with `tileset` (a TMX line or element) in its first `cells`.
fn map(tileset: &str, cells: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="2" height="2" tilewidth="32" tileheight="32" infinite="0" nextlayerid="2" nextobjectid="1">
{tileset}
 <layer id="1" name="ground" width="2" height="2">
  <data encoding="csv">
{cells}
</data>
 </layer>
</map>
"#
    )
}

/// An atlas tileset in the map, of `image`.
fn atlas(image: &str) -> String {
    format!(
        r#" <tileset firstgid="1" name="art" tilewidth="32" tileheight="32" tilecount="4" columns="2">
  <image source="{image}" width="64" height="64"/>
 </tileset>"#
    )
}

fn failed(result: Result<Report, Error>) -> String {
    match result {
        Err(Error::Failed(message)) => message,
        Err(e) => panic!("refused instead: {e}"),
        Ok(report) => panic!("exported instead: {:?}", report.files),
    }
}

fn run(dir: &Path, map: &str, target: &Path) -> Result<Report, Error> {
    export(
        &dir.join("maps").join(map),
        &mut loader(dir),
        target,
        &Options::default(),
    )
}

/// Every file under `dir`.
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            if entry.path().is_dir() {
                stack.push(entry.path())
            } else {
                out.push(entry.path())
            }
        }
    }
    out.sort();
    out
}

#[test]
fn an_image_that_cant_be_read_fails_the_export_and_writes_nothing() {
    let dir = project("image-missing");
    // Inside the project, used where it is.
    std::fs::write(
        dir.join("maps/typo.tmx"),
        map(&atlas("typo.png"), "1,0,\n0,0"),
    )
    .unwrap();
    let target = dir.join("levels/typo.tscn");
    let message = failed(run(&dir, "typo.tmx", &target));
    let typo = dir.join("maps/typo.png");
    assert_eq!(
        message,
        format!(
            "{}: the image {} can't be read ({}): restore it, or fix the path that names it",
            dir.join("maps/typo.tmx").display(),
            typo.display(),
            std::fs::metadata(&typo).unwrap_err()
        )
    );
    assert!(!dir.join("levels").exists());

    // Outside it, copied: nothing is copied either, and the copy of another map's image of that
    // name isn't taken for it.
    let game = dir.join("game");
    std::fs::create_dir_all(&game).unwrap();
    std::fs::write(game.join("project.godot"), "config_version=5\n").unwrap();
    std::fs::write(
        dir.join("maps/good.tmx"),
        map(&atlas("tiles.png"), "1,0,\n0,0"),
    )
    .unwrap();
    run(&dir, "good.tmx", &game.join("levels/good.tscn")).unwrap();
    let before = files(&game);
    std::fs::create_dir_all(dir.join("other")).unwrap();
    std::fs::write(
        dir.join("other/typo.tmx"),
        map(&atlas("tiles.png"), "1,0,\n0,0"),
    )
    .unwrap();
    let result = export(
        &dir.join("other/typo.tmx"),
        &mut loader(&dir),
        &game.join("levels/typo.tscn"),
        &Options::default(),
    );
    let message = failed(result);
    assert!(
        message.contains(&format!(
            "the image {} can't be read (",
            dir.join("other/tiles.png").display()
        )),
        "{message}"
    );
    assert_eq!(files(&game), before, "nothing written");

    // An image collection's tile no cell uses, with an image that is gone: Godot would refuse
    // the TileSet all the same.
    let collection = r#" <tileset firstgid="1" name="props" tilewidth="64" tileheight="64" tilecount="2" columns="0">
  <grid orientation="orthogonal" width="1" height="1"/>
  <tile id="0">
   <image source="tiles.png" width="64" height="64"/>
  </tile>
  <tile id="1">
   <image source="gone.png" width="64" height="64"/>
  </tile>
 </tileset>"#;
    std::fs::write(dir.join("maps/props.tmx"), map(collection, "1,0,\n0,0")).unwrap();
    let message = failed(run(&dir, "props.tmx", &dir.join("levels/props.tscn")));
    assert!(
        message.contains(&format!(
            "the image {} can't be read",
            dir.join("maps/gone.png").display()
        )),
        "{message}"
    );
    // Two of them: both are named.
    std::fs::write(
        dir.join("maps/props.tmx"),
        map(&collection.replace("tiles.png", "lost.png"), "1,0,\n0,0"),
    )
    .unwrap();
    let message = failed(run(&dir, "props.tmx", &dir.join("levels/props.tscn")));
    assert!(
        message.contains(": 2 images can't be read: ")
            && message.contains("lost.png (")
            && message.contains("gone.png (")
            && message.ends_with("; restore them, or fix the paths that name them"),
        "{message}"
    );
    assert!(!dir.join("levels").exists());
    std::fs::remove_dir_all(&dir).ok();
}

/// In a shared TileSet, a map's missing image would break the scenes of every map of the grid:
/// the export fails, and the TileSet stays as it was.
#[test]
fn a_missing_image_leaves_the_shared_tileset_alone() {
    let dir = project("image-missing-shared");
    std::fs::write(
        dir.join("better-tiled.json"),
        r#"{"betterTiled": 1, "godot": {"project": "."}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("maps/good.tmx"),
        map(
            r#" <tileset firstgid="1" source="tiles.tsx"/>"#,
            "1,2,\n0,0",
        ),
    )
    .unwrap();
    run(&dir, "good.tmx", &dir.join("levels/good.tscn")).unwrap();
    let tres = dir.join("tilesets/ortho-32x32.tres");
    let before = files(&dir)
        .into_iter()
        .map(|f| (std::fs::read(&f).unwrap(), f))
        .collect::<Vec<_>>();
    std::fs::write(
        dir.join("maps/broken.tmx"),
        map(&atlas("typo.png"), "1,0,\n0,0"),
    )
    .unwrap();
    let message = failed(run(&dir, "broken.tmx", &dir.join("levels/broken.tscn")));
    assert!(message.contains("typo.png"), "{message}");
    let after = files(&dir)
        .into_iter()
        .filter(|f| !f.ends_with("broken.tmx"))
        .map(|f| (std::fs::read(&f).unwrap(), f))
        .collect::<Vec<_>>();
    assert_eq!(
        after,
        before,
        "nothing written, {} included",
        tres.display()
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// A tileset that can't be loaded has no images: the cells painted with it get one warning,
/// whatever the number of tiles.
#[test]
fn tiles_without_an_image_get_one_warning_for_their_tileset() {
    let dir = project("no-image");
    std::fs::write(
        dir.join("maps/lost.tmx"),
        map(r#" <tileset firstgid="1" source="lost.tsx"/>"#, "1,2,\n3,3"),
    )
    .unwrap();
    let report = run(&dir, "lost.tmx", &dir.join("levels/lost.tscn")).unwrap();
    let notes: Vec<&Note> = report
        .notes
        .iter()
        .filter(|n| n.code.as_str() == "godot/tile-image-missing")
        .collect();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0]
            .message
            .ends_with(": 3 tiles have no image (0–2); the cells showing them are left out"),
        "{}",
        notes[0].message
    );
    assert_eq!(
        notes[0].elements,
        [
            Element::Tile { tileset: 0, id: 0 },
            Element::Tile { tileset: 0, id: 1 },
            Element::Tile { tileset: 0, id: 2 }
        ]
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Two maps in sibling folders, each with its own `tiles.png` of the same bytes, exported to
/// scenes side by side: both images go to one copy.
fn siblings(name: &str) -> (PathBuf, PathBuf) {
    let dir = project(name);
    let game = dir.join("game");
    std::fs::create_dir_all(&game).unwrap();
    std::fs::write(game.join("project.godot"), "config_version=5\n").unwrap();
    for side in ["a", "b"] {
        std::fs::create_dir_all(dir.join(side)).unwrap();
        std::fs::copy(dir.join("maps/tiles.png"), dir.join(side).join("tiles.png")).unwrap();
        std::fs::write(
            dir.join(side).join(format!("{side}.tmx")),
            map(&atlas("tiles.png"), "1,2,\n0,0"),
        )
        .unwrap();
    }
    (dir, game)
}

fn side(dir: &Path, game: &Path, side: &str) -> (PathBuf, PathBuf) {
    (
        dir.join(side).join(format!("{side}.tmx")),
        game.join(format!("levels/{side}.tscn")),
    )
}

fn summary(dir: &Path, game: &Path, which: &str) -> Summary {
    let (map, scene) = side(dir, game, which);
    status(&map, &mut bt_tmx::Loader::builder().build(), &scene)
        .unwrap()
        .summary()
}

fn export_side(dir: &Path, game: &Path, which: &str) -> Report {
    let (map, scene) = side(dir, game, which);
    export(
        &map,
        &mut bt_tmx::Loader::builder().build(),
        &scene,
        &Options::default(),
    )
    .unwrap()
}

#[test]
fn a_copy_of_two_maps_images_of_the_same_bytes_settles_whichever_is_exported() {
    for order in [["a", "b"], ["b", "a"]] {
        let (dir, game) = siblings(&format!("copy-shared-{}", order[0]));
        for which in order {
            export_side(&dir, &game, which);
        }
        assert_eq!(
            files(&game.join("levels/images")).len(),
            2,
            "one copy and its list"
        );
        for which in ["a", "b", "a", "b"] {
            assert_eq!(
                summary(&dir, &game, which),
                Summary::UpToDate,
                "{which} after {order:?}"
            );
            assert!(
                export_side(&dir, &game, which).up_to_date(),
                "{which} after {order:?}"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[test]
fn an_image_edited_for_one_map_leaves_the_copy_the_other_uses() {
    let (dir, game) = siblings("copy-shared-edit");
    // b's export, then a's: a was the last to record the copy.
    export_side(&dir, &game, "b");
    export_side(&dir, &game, "a");
    let copy = game.join("levels/images/tiles.png");
    let b = std::fs::read(dir.join("b/tiles.png")).unwrap();

    let mut edited = image::open(dir.join("a/tiles.png")).unwrap().to_rgba8();
    edited.put_pixel(0, 0, image::Rgba([1, 2, 3, 255]));
    edited.save(dir.join("a/tiles.png")).unwrap();
    let report = export_side(&dir, &game, "a");
    assert_eq!(report.copied(), 1);
    assert_eq!(
        std::fs::read(&copy).unwrap(),
        b,
        "b's scene still shows b's image"
    );
    assert_eq!(
        std::fs::read(game.join("levels/images/tiles-2.png")).unwrap(),
        std::fs::read(dir.join("a/tiles.png")).unwrap()
    );
    let scene =
        |which: &str| std::fs::read_to_string(game.join(format!("levels/{which}.tscn"))).unwrap();
    assert!(
        scene("a").contains("path=\"res://levels/images/tiles-2.png\"")
            && scene("b").contains("path=\"res://levels/images/tiles.png\"")
    );
    assert_eq!(summary(&dir, &game, "b"), Summary::UpToDate);
    assert_eq!(summary(&dir, &game, "a"), Summary::UpToDate);
    std::fs::remove_dir_all(&dir).ok();
}
