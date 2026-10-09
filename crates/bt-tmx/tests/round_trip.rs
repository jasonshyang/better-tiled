//! Every corpus file Tiled 1.0.1 or later saved (it records its `tiledversion`) is written
//! back byte for byte, apart from the files listed in [`EXCEPTIONS`], which even Tiled would
//! rewrite; every other file reads back to the same value, apart from what the writer
//! normalises.

mod corpus;

use std::path::Path;

use bt_core::{LayerDataFormat, Map};
use bt_tmx::{Format, Loader, WriteRequest, load_project, parse_project};
use corpus::{CorpusFile, DIRS, Kind, corpus};

/// Files Tiled would rewrite too, with why (path prefixes, as shown).
const EXCEPTIONS: &[(&str, &str)] = &[
    (
        "assets/Epic RPG World - Highlands V1.4.1/Highlands Biome Map/",
        "written by the asset pack's own tool: properties and animations on one line",
    ),
    (
        "reference/tiled/examples/forest/forest.tsx",
        "an <image> with its attributes in an order Tiled does not write",
    ),
    (
        "reference/tiled/examples/sticker-knight/map/objs.tsx",
        "the same",
    ),
    (
        "reference/tiled/examples/sticker-knight/ui/title.json",
        "saved by Tiled 1.8; the JSON writer of Tiled 1.12 adds every object's opacity",
    ),
    (
        "reference/tiled/src/tiled/resources/automap-tiles.tsx",
        "edited by hand: its tiles are listed out of order, and Tiled lists an atlas's tiles by id",
    ),
];

/// Whether Tiled saved the file: its XML or JSON records a `tiledversion`.
fn saved_by_tiled(original: &str) -> bool {
    original.contains("tiledversion=") || original.contains("\"tiledversion\":")
}

fn must_be_identical(file: &CorpusFile, original: &str) -> bool {
    saved_by_tiled(original)
        && !EXCEPTIONS
            .iter()
            .any(|(prefix, _)| file.shown.starts_with(prefix))
}

/// The first line that differs, with both versions.
fn first_difference(original: &str, written: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (
        original.split('\n').collect(),
        written.split('\n').collect(),
    );
    let line = a
        .iter()
        .zip(&b)
        .position(|(x, y)| x != y)
        .unwrap_or(a.len().min(b.len()));
    let clip = |s: &str| {
        s.chars()
            .take(200)
            .collect::<String>()
            .escape_debug()
            .to_string()
    };
    format!(
        "line {}:\n    original: {}\n    written:  {}\n    ({} lines, written {})",
        line + 1,
        a.get(line).map_or("<end>".into(), |s| clip(s)),
        b.get(line).map_or("<end>".into(), |s| clip(s)),
        a.len(),
        b.len()
    )
}

/// A map with what the writer normalises set as it writes it: a version and a Tiled version,
/// first gids, CSV for `<tile>` elements.
fn normalised(mut map: Map, written: &Map) -> Map {
    if map.file.version.is_empty() {
        map.file.version = written.file.version.clone();
    }
    if map.file.tiled_version.is_empty() {
        map.file.tiled_version = written.file.tiled_version.clone();
    }
    if map.file.layer_data == LayerDataFormat::Xml {
        map.file.layer_data = LayerDataFormat::Csv;
    }
    for (ours, theirs) in map.tilesets.iter_mut().zip(&written.tilesets) {
        ours.first_gid = theirs.first_gid;
    }
    map
}

/// Writes a corpus file back; checks a rewritten one reads back to the same value.
fn written_back(file: &CorpusFile) -> Result<String, bt_tmx::Error> {
    let mut loader = Loader::default();
    let path = &file.path;
    let format =
        Format::from_path(path).unwrap_or(if path.extension().is_some_and(|e| e == "json") {
            Format::Json
        } else {
            Format::Xml
        });
    match file.kind {
        Kind::Map => {
            let map = loader.load_map(path)?.value;
            let text = WriteRequest::for_map(&map, path)
                .format(format)
                .loader(&loader)
                .render()?
                .text;
            let again = loader.parse_map(&text, path)?.value;
            assert!(
                normalised(map.clone(), &again) == again,
                "{}: the written map reads back differently",
                file.shown
            );
            Ok(text)
        }
        Kind::Tileset => {
            let tileset = loader.load_tileset(path)?.value;
            let text = WriteRequest::for_tileset(&tileset, path)
                .format(format)
                .render()?
                .text;
            let again = loader.parse_tileset(&text, path)?.value;
            let mut expected = (*tileset).clone();
            if expected.file.version.is_empty() {
                expected.file.version = again.file.version.clone();
                expected.file.tiled_version = again.file.tiled_version.clone();
            }
            assert!(
                expected == *again,
                "{}: the written tileset reads back differently",
                file.shown
            );
            Ok(text)
        }
        Kind::Template => {
            let template = loader.load_template(path)?.value;
            let text = WriteRequest::for_template(&template, path)
                .format(format)
                .render()?
                .text;
            let again = loader.parse_template(&text, path)?.value;
            assert!(
                *template == *again,
                "{}: the written template reads back differently",
                file.shown
            );
            Ok(text)
        }
        Kind::World => {
            let world = loader.load_world(path)?.value;
            let text = WriteRequest::for_world(&world, path).render()?.text;
            assert_eq!(
                loader.parse_world(&text, path)?.value,
                world,
                "{}",
                file.shown
            );
            Ok(text)
        }
        Kind::Project => {
            let project = load_project(path)?.value;
            let text = WriteRequest::for_project(&project, path).render()?.text;
            assert_eq!(
                parse_project(&text, Path::new(path))?.value,
                project,
                "{}",
                file.shown
            );
            Ok(text)
        }
    }
}

#[test]
fn tiled_files_are_written_back_byte_for_byte() {
    let mut identical = 0;
    let mut total = 0;
    let mut unexpected = Vec::new();
    for file in corpus(&DIRS) {
        let original = std::fs::read_to_string(&file.path).unwrap();
        let written = written_back(&file).unwrap();
        total += 1;
        if written == original {
            identical += 1;
            continue;
        }
        let difference = format!("{}: {}", file.shown, first_difference(&original, &written));
        if must_be_identical(&file, &original) {
            unexpected.push(difference);
        } else {
            println!("rewritten (allowed): {difference}");
        }
    }
    println!("{identical} of {total} files written back byte for byte");
    assert!(
        unexpected.is_empty(),
        "{} files Tiled saved changed when written back:\n{}",
        unexpected.len(),
        unexpected.join("\n")
    );
}
