//! What writing the resolved JSON fails with, and what it warns about while the JSON is still
//! written.

use std::path::PathBuf;

use bt_utils::text::plural;

use crate::words::capped_list;

/// The most tile ids a warning lists before saying how many more there are.
const TILES_LISTED: usize = 12;

/// Why the resolved JSON was not written. Nothing was written: the target is as it was.
#[derive(Debug, thiserror::Error)]
pub enum ResolvedError {
    /// The map is oblique: its sheared cells have no place in the format.
    #[error(
        "{} is an oblique map: the resolved JSON is written for orthogonal, isometric, staggered \
         and hexagonal maps",
        map.display()
    )]
    Oblique {
        /// The map's file.
        map: PathBuf,
    },
    /// The folder the JSON goes in cannot be found.
    #[error("the folder of {} can't be found: {source}", path.display())]
    Folder {
        /// The JSON file asked for.
        path: PathBuf,
        /// What the system said.
        #[source]
        source: std::io::Error,
    },
    /// The file could not be written: it is read-only, its folder can't be written to, or the
    /// system said no.
    #[error(transparent)]
    Write(#[from] bt_utils::Error),
    /// The JSON could not be made.
    #[error("the resolved JSON can't be made: {0}")]
    Json(#[from] serde_json::Error),
}

/// Something the resolved JSON leaves out or cannot say, written while it is still made.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ResolvedWarning {
    /// The map has AutoMap's rule tileset: its tiles mean something only in rule maps, so they
    /// are left out, and so is the tileset.
    #[error(
        "the map has AutoMap's rule tiles (tileset “{tileset}”), which mean something only in \
         rule maps: they are left out ({}, {})",
        plural(*.cells, "cell", "cells"),
        plural(*.objects, "object", "objects")
    )]
    RuleTilesLeftOut {
        /// The rule tileset's name.
        tileset: String,
        /// The cells left empty.
        cells: usize,
        /// The objects left out.
        objects: usize,
    },
    /// An image the map file embeds, which the JSON cannot point to.
    #[error(
        "{what}: its image is embedded in the map file, and the resolved JSON names only image files"
    )]
    EmbeddedImage {
        /// What shows the image: “tileset “trees””, “layer “sky””.
        what: String,
    },
    /// Tiles shown without an image: their tileset's file could not be read, or the tileset has
    /// no such tile.
    #[error(
        "tileset “{tileset}” has no image for {} {}: written without one",
        if .tiles.len() == 1 { "tile" } else { "tiles" },
        tiles_text(.tiles)
    )]
    NoImage {
        /// The tileset's name.
        tileset: String,
        /// The tiles' ids.
        tiles: Vec<u32>,
    },
    /// Cells of a layer holding tiles of a tileset the map does not have: they are written
    /// empty.
    #[error(
        "layer “{layer}”: {} of a tileset the map does not have, written empty",
        plural(*.cells, "cell", "cells")
    )]
    CellsWithoutTileset {
        /// The layer's name.
        layer: String,
        /// How many cells.
        cells: usize,
    },
    /// A tile object showing a tile of a tileset the map does not have: it is written without
    /// its tile.
    #[error(
        "object #{id} shows a tile of a tileset the map does not have: written without its tile"
    )]
    ObjectWithoutTileset {
        /// The object's id.
        id: u32,
    },
    /// A terrain set that cannot be read, so its tiles have no terrains.
    #[error(
        "the terrain set “{set}” of tileset “{tileset}” can't be read ({error}): its tiles are \
         written without terrains"
    )]
    TerrainSet {
        /// The tileset's name.
        tileset: String,
        /// The set's name.
        set: String,
        /// What reading it said.
        error: bt_terrain::TerrainError,
    },
}

/// Tile ids as a warning lists them: `3, 5, 9`, at most a dozen, then how many more.
fn tiles_text(tiles: &[u32]) -> String {
    let words: Vec<String> = tiles.iter().map(u32::to_string).collect();
    capped_list(&words, TILES_LISTED)
}
