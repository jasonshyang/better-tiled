//! What names, shapes and feet fail with (`10-agents.md`, §5, §6): one typed error whose
//! messages are the words an agent reads, each naming what was asked for and, where there is
//! one, the way to what was meant.

use bt_core::{LayerId, ObjectId};
use glam::{IVec2, UVec2};

/// The most names an unknown name's message lists in full; with more, it says where the full
/// list is.
const LISTED: usize = 12;

/// Why a name, a shape or a foot could not be resolved. Nothing is changed by a lookup that
/// fails.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum Error {
    /// No name of the kind asked for matches; the closest are suggested.
    #[error("{}", unknown_text(*.kind, .query, .closest, .listed, *.count))]
    Unknown {
        /// What was looked for.
        kind: NameKind,
        /// The name as written.
        query: String,
        /// Up to three names closest to it.
        closest: Vec<String>,
        /// Every name of the kind, when there are few; empty when there are many.
        listed: Vec<String>,
        /// How many names of the kind there are.
        count: usize,
    },
    /// An unqualified terrain name that is the last part of several qualified names.
    #[error("“{query}” is ambiguous: {}", options.join(", "))]
    Ambiguous {
        /// The name as written.
        query: String,
        /// The qualified names it could be.
        options: Vec<String>,
    },
    /// A name of one kind where another is required: tiles where a terrain is, a terrain where
    /// a sprite is.
    #[error("{name} is {}, not {}{}", found.with_article(), wanted.with_article(), way_text(name, *.found, *.wanted))]
    WrongKind {
        /// The name as written.
        name: String,
        /// What it is.
        found: NameKind,
        /// What the operation needs.
        wanted: NameKind,
    },
    /// `TILESET:ID` naming a tile that is not a sprite.
    #[error(
        "{query} is not a sprite: sprites are the tiles of image-collection tilesets, less the frames of animations"
    )]
    NotASprite {
        /// The tile as written.
        query: String,
    },
    /// A tile id the tileset lacks.
    #[error("{tileset} has no tile {id} ({range})")]
    NoSuchTile {
        /// The tileset's name.
        tileset: String,
        /// The id asked for.
        id: u32,
        /// The ids it has, as words: "its tiles are 0-31".
        range: String,
    },
    /// A range of tile ids written from the higher id to the lower.
    #[error("{tileset}:{first}-{last}: a range of tile ids goes from the lower id to the higher")]
    BackwardRange {
        /// The tileset as written.
        tileset: String,
        /// The first id written.
        first: u32,
        /// The last id written.
        last: u32,
    },
    /// Tile names put together that go on different layers.
    #[error(
        "{first} goes on layer {first_layer} and {second} on layer {second_layer}: tiles placed together go on one layer"
    )]
    LayersDisagree {
        /// The first tile name.
        first: String,
        /// The layer it goes on.
        first_layer: String,
        /// The tile name that disagrees.
        second: String,
        /// The layer it goes on.
        second_layer: String,
    },
    /// Several layers of the kind asked for have the name.
    #[error("several layers are called “{name}”: name one by id ({})", ids_text(.ids.iter().map(|id| id.0)))]
    SeveralLayers {
        /// The name as written.
        name: String,
        /// Their ids.
        ids: Vec<LayerId>,
    },
    /// `#ID` naming no layer.
    #[error("the map has no layer #{id} (bt info MAP lists the layers)")]
    NoSuchLayer {
        /// The id asked for.
        id: u32,
    },
    /// A layer of another kind than the operation needs.
    #[error("{layer} is {is}, not {wanted}")]
    WrongLayerKind {
        /// The layer's name.
        layer: String,
        /// What it is: "a tile layer", "an object layer", "an image layer", "a group".
        is: &'static str,
        /// What the operation needs.
        wanted: &'static str,
    },
    /// `#ID` naming no object.
    #[error("the map has no object #{id} (bt objects MAP lists them)")]
    NoSuchObject {
        /// The id asked for.
        id: u32,
    },
    /// Several objects have the name.
    #[error("several objects are called “{name}”: name one by id ({})", ids_text(.ids.iter().map(|id| id.0)))]
    SeveralObjects {
        /// The name as written.
        name: String,
        /// Their ids.
        ids: Vec<ObjectId>,
    },
    /// No object has the name and several have it as their type.
    #[error("there are {} {name} objects: name one by id ({})", ids.len(), ids_text(.ids.iter().map(|id| id.0)))]
    SeveralOfType {
        /// The type as written.
        name: String,
        /// Their ids.
        ids: Vec<ObjectId>,
    },
    /// `selection` or `selected` when the map is not open in the editor.
    #[error(
        "`{word}` stands for what is selected in the editor, but the map isn't open in the editor (bt editor shows what is)"
    )]
    NotInEditor {
        /// `selection` or `selected`.
        word: &'static str,
    },
    /// `selection` or `selected` when nothing of the kind is selected.
    #[error("`{word}`: no {what} are selected in the editor")]
    NothingSelected {
        /// `selection` or `selected`.
        word: &'static str,
        /// "cells" or "objects".
        what: &'static str,
    },
    /// `last` before any operation made objects.
    #[error("`last` stands for the objects the operation before made, and none has made any")]
    NothingLast,
    /// A definition whose name the map already gives to something else.
    #[error("“{name}” is already {} of the map: give the definition another name", by.with_article())]
    NameTaken {
        /// The name defined.
        name: String,
        /// What the map calls by it.
        by: NameKind,
    },
    /// A terrain set the names were built from that the map no longer has.
    #[error("the terrain set of {terrain} can't be read: {error}")]
    TerrainSet {
        /// The terrain's name.
        terrain: String,
        /// What reading it said.
        error: bt_terrain::TerrainError,
    },
    /// A tile of a tileset the map does not have.
    #[error("tile {tile} is of a tileset the map does not have")]
    NoTileset {
        /// The tile's id.
        tile: u32,
    },
    /// An object that is not a tile object, where only those stand by their feet.
    #[error("object #{id} is not a sprite or another tile object, so it has no foot")]
    NotATileObject {
        /// The object.
        id: u32,
    },
    /// A tile without an image, whose foot cannot be measured.
    #[error("{tile} has no image to measure its foot on")]
    NoImage {
        /// The tile, as words: "tile 3 of “trees”".
        tile: String,
    },
    /// A tile's image that cannot be read.
    #[error("the image of {tile} can't be read: {error}")]
    Image {
        /// The tile, as words.
        tile: String,
        /// What decoding it said.
        error: bt_image::Error,
    },
    /// A tile whose image has no visible pixel.
    #[error("the image of {tile} has no visible pixel, so it has no foot")]
    Invisible {
        /// The tile, as words.
        tile: String,
    },
    /// `containing X,Y` naming a cell the shape does not have.
    #[error(
        "containing {},{}: that cell isn't one of the shape's cells ({count} of them, before `containing`)",
        cell.x,
        cell.y
    )]
    NotInShape {
        /// The cell.
        cell: IVec2,
        /// How many cells the shape had.
        count: usize,
    },
    /// A shape reaching over more cells than a shape may.
    #[error("the shape reaches over {cells} cells; a shape may reach over at most {limit}")]
    TooLarge {
        /// The cells it reaches over.
        cells: u64,
        /// The most there may be.
        limit: u64,
    },
    /// A polygon or a path with too few points.
    #[error("{shape} needs at least {needed} points, and has {found}")]
    TooFewPoints {
        /// `poly` or `path`.
        shape: &'static str,
        /// How many it needs.
        needed: usize,
        /// How many it has.
        found: usize,
    },
    /// An ellipse or circle with a radius that is not positive.
    #[error("an ellipse's radii must be more than 0, and these are {radii}")]
    NotPositiveRadius {
        /// The radii written, as text: `2x0`.
        radii: String,
    },
    /// A path whose width is not positive.
    #[error("a path's width must be more than 0, and it is {width}")]
    NotPositiveWidth {
        /// The width written, as text.
        width: String,
    },
    /// A shape `reach` starts or ends at that has no cells on the map.
    #[error("{written}: no cells on the map")]
    NoCells {
        /// The shape as written.
        written: String,
    },

    // What the Look and Know commands refuse.
    /// A view one character a square of no cells.
    #[error("--step is a whole number above 0")]
    ZeroStep,
    /// A view too large to write a number or the corners of each of its cells.
    #[error(
        "{what} shows at most {}x{} cells, and this is {}x{}: narrow it with --region X,Y WxH",
        most.x,
        most.y,
        size.x,
        size.y
    )]
    ViewTooLarge {
        /// The option: `--ids`, `--corners`.
        what: &'static str,
        /// The most cells across and down it shows.
        most: IVec2,
        /// The cells asked for.
        size: IVec2,
    },
    /// `--corners` on a grid whose cells do not share corners as squares do.
    #[error(
        "--corners reads the corners squares share, and this map is {orientation}: bt view MAP and bt at MAP X,Y show its terrains"
    )]
    CornersNotSquare {
        /// The grid: `hexagonal`, `staggered`.
        orientation: &'static str,
    },
    /// `--run N` on a map without AutoMap layers.
    #[error(
        "--run {run} goes with maps that have AutoMap layers, and this one has none (bt info MAP says)"
    )]
    RunWithoutLayers {
        /// The run asked for.
        run: usize,
    },
    /// `--run N` past the AutoMap layers' runs.
    #[error("--run {run}: the map's AutoMap layers have {runs} runs, 1 to {runs}")]
    NoSuchRun {
        /// The run asked for.
        run: usize,
        /// How many runs there are.
        runs: usize,
    },
    /// A new map, or its tiles, of no size.
    #[error("{what} {}x{}: it must be at least 1x1", size.x, size.y)]
    ZeroSize {
        /// The option: `--size`, `--tile`.
        what: &'static str,
        /// The size given.
        size: UVec2,
    },
    /// A layer of `bt new --layers` that is not `NAME:tiles` or `NAME:objects`.
    #[error("--layers {spec}: a layer is NAME:tiles or NAME:objects")]
    LayerSpec {
        /// The layer as written.
        spec: String,
    },
    /// A new map that could not be put together.
    #[error("the new map can't be made: {why}")]
    NotCreated {
        /// Why.
        why: String,
    },
    /// Tiles written for `bt catalog MAP tiles` that are not tiles.
    #[error("tiles {written}: {why}")]
    NotTiles {
        /// The tiles as written.
        written: String,
        /// What is wrong with them.
        why: String,
    },
    /// A sheet scale out of range.
    #[error("--scale for a sheet is a whole number from 1 to 8, and it is {scale}")]
    SheetScale {
        /// The scale given.
        scale: u32,
    },
    /// A sheet larger than is drawn.
    #[error(
        "the sheet would be {}x{} px: name one tileset (bt catalog MAP tiles TILESET --sheet OUT.png) or narrow the list with --find",
        size.x,
        size.y
    )]
    SheetTooLarge {
        /// Its size in pixels.
        size: UVec2,
    },
    /// A sheet of nothing.
    #[error("nothing to draw: the list has no terrain, sprite or tile with a picture")]
    NothingToDraw,
    /// A picture's scale out of range.
    #[error("--scale is a number above 0 and at most 8, and it is {scale}")]
    RenderScale {
        /// The scale given, as written.
        scale: String,
    },
    /// A picture that could not be drawn.
    #[error("the picture can't be drawn: {why}")]
    NoPicture {
        /// Why: what the renderer said.
        why: String,
    },
}

/// What a name stands for, in the messages of [`Error`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NameKind {
    /// One colour of one terrain set.
    Terrain,
    /// Terrains painted together on several layers.
    Material,
    /// A tile of an image-collection tileset, placed by its foot.
    Sprite,
    /// Sprites placed together at offsets.
    Group,
    /// Tiles and objects copied from a map, placed by a foot cell.
    Piece,
    /// Tiles: a tile group or tiles by id.
    Tiles,
    /// A defined area.
    Area,
    /// What `where`, `on`, `inside`, `avoid` and `near` take: a terrain, tiles or an area.
    Ground,
    /// A tileset.
    Tileset,
    /// A layer.
    Layer,
    /// An object.
    Object,
}

impl NameKind {
    /// The kind as a noun: "terrain", "tile".
    fn noun(self) -> &'static str {
        match self {
            NameKind::Terrain => "terrain",
            NameKind::Material => "material",
            NameKind::Sprite => "sprite",
            NameKind::Group => "group of sprites",
            NameKind::Piece => "piece",
            NameKind::Tiles => "tile",
            NameKind::Area => "area",
            NameKind::Ground => "terrain or tile",
            NameKind::Tileset => "tileset",
            NameKind::Layer => "layer",
            NameKind::Object => "object",
        }
    }

    /// The kind in many: "terrains", "tiles".
    fn plural(self) -> &'static str {
        match self {
            NameKind::Terrain => "terrains",
            NameKind::Material => "materials",
            NameKind::Sprite => "sprites",
            NameKind::Group => "groups of sprites",
            NameKind::Piece => "pieces",
            NameKind::Tiles => "tile names",
            NameKind::Area => "areas",
            NameKind::Ground => "terrains and tile names",
            NameKind::Tileset => "tilesets",
            NameKind::Layer => "layers",
            NameKind::Object => "named objects and types",
        }
    }

    /// The kind with its article: "a terrain", "tiles", "an area".
    fn with_article(self) -> &'static str {
        match self {
            NameKind::Terrain => "a terrain",
            NameKind::Material => "a material",
            NameKind::Sprite => "a sprite",
            NameKind::Group => "a group of sprites",
            NameKind::Piece => "a piece",
            NameKind::Tiles => "tiles",
            NameKind::Area => "an area",
            NameKind::Ground => "a terrain or tiles",
            NameKind::Tileset => "a tileset",
            NameKind::Layer => "a layer",
            NameKind::Object => "an object",
        }
    }

    /// The command that lists every name of the kind.
    fn listing(self) -> &'static str {
        match self {
            NameKind::Terrain | NameKind::Material | NameKind::Ground => "bt catalog MAP terrains",
            NameKind::Sprite | NameKind::Group | NameKind::Piece => "bt catalog MAP sprites",
            NameKind::Tiles => "bt catalog MAP tiles",
            NameKind::Area | NameKind::Tileset | NameKind::Layer => "bt info MAP",
            NameKind::Object => "bt objects MAP",
        }
    }
}

/// "unknown terrain “ice”: did you mean ice-lake? (terrains: grass, ice-lake, water)".
fn unknown_text(
    kind: NameKind,
    query: &str,
    closest: &[String],
    listed: &[String],
    count: usize,
) -> String {
    let mut text = format!("unknown {} “{query}”", kind.noun());
    if !closest.is_empty() {
        text.push_str(&format!(": did you mean {}?", closest.join(", ")));
    }
    let known = if count == 0 {
        format!("the map has no {}", kind.plural())
    } else if listed.is_empty() {
        format!("all {count} {}: {}", kind.plural(), kind.listing())
    } else {
        format!("{}: {}", kind.plural(), listed.join(", "))
    };
    let tiles = match kind {
        NameKind::Tiles => {
            "; tiles are also written as TILESET:ID or by a sprite's name, and bt catalog MAP tiles lists them"
        }
        NameKind::Ground => {
            "; tiles are written as TILESET:ID, and bt catalog MAP tiles lists them"
        }
        _ => "",
    };
    text.push_str(&format!(" ({known}{tiles})"));
    text
}

/// The way to what was meant when a name of one kind was used where another is required, with
/// the colon before it; empty for the kinds no name is found to be.
fn way_text(name: &str, found: NameKind, wanted: NameKind) -> String {
    match found {
        NameKind::Tiles => format!(": place tiles with `tile {name} SHAPE`"),
        NameKind::Terrain | NameKind::Material => format!(": paint it with `paint {name} SHAPE`"),
        NameKind::Sprite | NameKind::Group | NameKind::Piece if wanted == NameKind::Ground => {
            ": the cells under objects are `under LAYER` or `@NAME`".to_owned()
        }
        NameKind::Sprite | NameKind::Group | NameKind::Piece => {
            format!(": place it with `place {name} at X,Y`")
        }
        NameKind::Area => format!(": use it as a shape, `where {name}`"),
        NameKind::Ground | NameKind::Tileset | NameKind::Layer | NameKind::Object => String::new(),
    }
}

/// `#3, #5`.
fn ids_text(ids: impl Iterator<Item = u32>) -> String {
    let ids: Vec<String> = ids.map(|id| format!("#{id}")).collect();
    ids.join(", ")
}

impl Error {
    /// An unknown name of `kind`, with the names of the kind it could have been: the closest
    /// three suggested, all of them listed when there are few.
    pub(crate) fn unknown(kind: NameKind, query: &str, names: &[&str]) -> Error {
        let mut sorted: Vec<&str> = names.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        let wanted = query.rsplit('/').next().unwrap_or(query);
        let closest = bt_utils::text::fuzzy_match(wanted, &sorted)
            .into_iter()
            .map(str::to_owned)
            .collect();
        let listed = if sorted.len() <= LISTED {
            sorted.iter().map(|&name| name.to_owned()).collect()
        } else {
            Vec::new()
        };
        Error::Unknown {
            kind,
            query: query.to_owned(),
            closest,
            listed,
            count: sorted.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_names_suggest_the_closest_and_say_where_the_rest_are() {
        let few = Error::unknown(NameKind::Terrain, "snw", &["water", "snow", "grass"]);
        assert_eq!(
            few.to_string(),
            "unknown terrain “snw”: did you mean snow? (terrains: grass, snow, water)"
        );
        let names: Vec<String> = (0..20).map(|n| format!("rock-{n}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let many = Error::unknown(NameKind::Sprite, "tree", &names);
        assert_eq!(
            many.to_string(),
            "unknown sprite “tree” (all 20 sprites: bt catalog MAP sprites)"
        );
        let none = Error::unknown(NameKind::Layer, "ground", &[]);
        assert_eq!(
            none.to_string(),
            "unknown layer “ground” (the map has no layers)"
        );
        let ground = Error::unknown(NameKind::Ground, "pebbles", &["water"]);
        assert!(
            ground.to_string().ends_with(
                "(terrains and tile names: water; tiles are written as TILESET:ID, and bt catalog MAP tiles lists them)"
            ),
            "{ground}"
        );
    }

    #[test]
    fn a_name_of_another_kind_says_what_it_is_and_the_way() {
        let tiles = Error::WrongKind {
            name: "stone".into(),
            found: NameKind::Tiles,
            wanted: NameKind::Terrain,
        };
        assert_eq!(
            tiles.to_string(),
            "stone is tiles, not a terrain: place tiles with `tile stone SHAPE`"
        );
        let terrain = Error::WrongKind {
            name: "water".into(),
            found: NameKind::Terrain,
            wanted: NameKind::Sprite,
        };
        assert_eq!(
            terrain.to_string(),
            "water is a terrain, not a sprite: paint it with `paint water SHAPE`"
        );
    }
}
