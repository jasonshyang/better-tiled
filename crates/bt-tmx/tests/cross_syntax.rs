//! A map written in the other syntax reads back to the same map: XML maps through JSON, and
//! JSON maps, once in XML's six significant digits, through JSON again.

#![cfg(test)]

mod corpus;

use std::path::Path;

use bt_core::{ImageSource, Layer, LayerDataFormat, LayerKind, Map, TileLayer, TilesetImage};
use bt_tmx::{Format, Loader, WriteRequest};
use corpus::{DIRS, Kind, corpus};
use glam::UVec2;

/// Whether a map holds an image embedded in its file, which JSON cannot hold.
fn embeds_images(map: &Map) -> bool {
    let embedded = |source: &ImageSource| matches!(source, ImageSource::Embedded(_));
    let mut found = map.tilesets.iter().filter(|t| t.file.is_none()).any(|t| {
        matches!(&t.tileset.image, TilesetImage::Atlas { image, .. } if embedded(&image.source))
            || t.tileset
                .tiles
                .values()
                .any(|tile| tile.image.as_ref().is_some_and(|i| embedded(&i.source)))
    });
    map.for_each_layer(|layer, _| {
        if let LayerKind::Image(image) = &layer.kind {
            found |= image.image.as_ref().is_some_and(|i| embedded(&i.source));
        }
    });
    found
}

/// `map` written in `format` and read back.
fn through(map: &Map, path: &Path, format: Format, loader: &mut Loader) -> Map {
    let text = WriteRequest::for_map(map, path)
        .format(format)
        .loader(loader)
        .render()
        .unwrap()
        .text;
    loader.parse_map(&text, path).unwrap().value
}

/// The map without what one syntax cannot say as the other does: infinite layers' declared
/// size (JSON states the bounds of their chunks there, as Tiled's does), `<tile>` elements
/// (written as CSV), and first gids (renumbered when they no longer fit).
fn comparable(mut map: Map) -> Map {
    if map.file.layer_data == LayerDataFormat::Xml {
        map.file.layer_data = LayerDataFormat::Csv;
    }
    for tileset in &mut map.tilesets {
        tileset.first_gid = None;
    }
    fn strip(layers: &mut [Layer]) {
        for layer in layers {
            match &mut layer.kind {
                LayerKind::Tile(tiles) if tiles.is_infinite() => {
                    let mut stripped = TileLayer::infinite(UVec2::ZERO);
                    for placed in tiles.cells() {
                        stripped.set_cell(placed.at, placed.cell).unwrap();
                    }
                    *tiles = stripped;
                }
                LayerKind::Group(children) => strip(children),
                _ => {}
            }
        }
    }
    strip(&mut map.layers);
    map
}

#[test]
fn maps_survive_the_other_syntax() {
    let mut checked = 0;
    let mut failures = Vec::new();
    for file in corpus(&DIRS).into_iter().filter(|f| f.kind == Kind::Map) {
        let mut loader = Loader::default();
        let map = loader.load_map(&file.path).unwrap().value;
        if embeds_images(&map) {
            continue;
        }
        let in_xml = if Format::from_path(&file.path) == Some(Format::Xml) {
            map
        } else {
            through(&map, &file.path, Format::Xml, &mut loader)
        };
        let in_json = through(&in_xml, &file.path, Format::Json, &mut loader);
        let mut expected = in_xml.clone();
        if expected.file.version.is_empty() {
            expected.file.version = in_json.file.version.clone();
        }
        if expected.file.tiled_version.is_empty() {
            expected.file.tiled_version = in_json.file.tiled_version.clone();
        }
        checked += 1;
        if comparable(expected) != comparable(in_json) {
            failures.push(file.shown.clone());
        }
    }
    println!("{checked} maps checked through the other syntax");
    assert!(
        failures.is_empty(),
        "maps that read back differently:\n{}",
        failures.join("\n")
    );
}
