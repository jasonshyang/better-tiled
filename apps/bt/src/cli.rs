//! Every command and option of `bt`, declared once (`10-agents.md`, §4.1): the guide is checked
//! against this table, and `bt help COMMAND` prints a command's options from it.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// `bt`: Tiled maps for agents and scripts. `bt guide` is the documentation.
#[derive(Debug, Parser)]
#[command(
    name = "bt",
    version,
    about = "Read, edit and check Tiled maps from the command line (bt guide explains how)",
    disable_help_subcommand = false,
    propagate_version = true
)]
pub(crate) struct Cli {
    /// The `.tiled-project` whose custom types apply; else the nearest one above the map.
    #[arg(long, global = true, value_name = "FILE")]
    pub(crate) project: Option<PathBuf>,
    /// Print the outcome as JSON, with a `schema` field naming its shape.
    #[arg(long, global = true)]
    pub(crate) json: bool,
    /// The command.
    #[command(subcommand)]
    pub(crate) command: Command,
}

/// The commands.
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// How to work with maps from the command line: start here.
    Guide,
    /// Size, layers, tilesets, terrains, objects and properties.
    Info(MapArg),
    /// The map as text, one character per cell.
    View(ViewArgs),
    /// Everything in one cell, layer by layer.
    At(AtArgs),
    /// Objects with where they stand, filtered.
    Objects(ObjectsArgs),
    /// What the map can be built with, by name, and a numbered picture.
    Catalog(CatalogArgs),
    /// The connected areas of a shape's cells, largest first.
    Areas(AreasArgs),
    /// Whether a way leads from one cell to another through a shape's cells.
    Reach(ReachArgs),
    /// What an AutoMap run does at a cell, and why.
    Explain(ExplainArgs),
    /// A picture of the map with its cells numbered along the edges.
    Render(RenderArgs),
    /// A new empty map, like another or from a size.
    New(NewArgs),
    /// Run an edit script (from a file, standard input or --do): all or nothing.
    Edit(EditArgs),
    /// Take back the last bt edit (in the editor, when the map is open there).
    Undo(UndoArgs),
    /// Problems first, then what objects stand on; exits 5 when it lists a problem.
    Check(MapArg),
    /// What differs between two maps; with one, its unsaved changes in the editor.
    Diff(DiffArgs),
    /// What the editor has open, selected and in view.
    Editor(EditorArgs),
    /// Outline cells in the editor, to show them to the person using it.
    Mark(MarkArgs),
    /// Open a map in the editor, starting it when none runs.
    Open(MapArg),
    /// Save a map open in the editor.
    Save(MapArg),
    /// The map in another format: Tiled's, a picture, or resolved JSON for other programs.
    Export(ExportArgs),
}

/// A command that takes a map and nothing else.
#[derive(Debug, Args)]
pub(crate) struct MapArg {
    /// The map.
    pub(crate) map: PathBuf,
}

/// `bt view`.
#[derive(Debug, Args)]
pub(crate) struct ViewArgs {
    /// The map.
    pub(crate) map: PathBuf,
    /// Only these cells: X,Y WxH, X,Y,WxH or X1,Y1..X2,Y2.
    #[arg(long, num_args = 1..=2, value_name = "R")]
    pub(crate) region: Vec<String>,
    /// Only these layers, by name.
    #[arg(long, value_delimiter = ',', value_name = "L,…")]
    pub(crate) layers: Vec<String>,
    /// What a cell's character stands for.
    #[arg(long, value_enum)]
    pub(crate) by: Option<ViewBy>,
    /// Each row's runs after it (`~14-29`).
    #[arg(long)]
    pub(crate) runs: bool,
    /// Leave objects out.
    #[arg(long)]
    pub(crate) no_objects: bool,
    /// One character per square of N cells a side.
    #[arg(long, value_name = "N")]
    pub(crate) step: Option<u32>,
    /// Each cell's tile id, layer by layer.
    #[arg(long, conflicts_with_all = ["by", "runs", "no_objects", "step", "corners"])]
    pub(crate) ids: bool,
    /// The terrain at each corner of the cells.
    #[arg(long, conflicts_with_all = ["by", "runs", "no_objects", "step"])]
    pub(crate) corners: bool,
    /// What was painted, on a map with AutoMap layers.
    #[arg(long)]
    pub(crate) painted: bool,
}

/// What a character of `bt view` stands for.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum ViewBy {
    /// The terrain covering more than half of the cell.
    Terrain,
    /// The kind of tile.
    Tile,
    /// The top layer with a tile there.
    Layer,
}

/// `bt at`.
#[derive(Debug, Args)]
pub(crate) struct AtArgs {
    /// The map.
    pub(crate) map: PathBuf,
    /// The cell, X,Y.
    pub(crate) cell: String,
    /// What was painted, on a map with AutoMap layers.
    #[arg(long)]
    pub(crate) painted: bool,
}

/// `bt objects`.
#[derive(Debug, Args)]
pub(crate) struct ObjectsArgs {
    /// The map.
    pub(crate) map: PathBuf,
    /// Only objects of this type.
    #[arg(long = "type", value_name = "T")]
    pub(crate) kind: Option<String>,
    /// Only objects of this layer.
    #[arg(long, value_name = "L")]
    pub(crate) layer: Option<String>,
    /// Only objects standing in this region.
    #[arg(long = "in", num_args = 1..=2, value_name = "R")]
    pub(crate) within: Vec<String>,
    /// Only objects of this name.
    #[arg(long, value_name = "N")]
    pub(crate) name: Option<String>,
}

/// `bt catalog`.
#[derive(Debug, Args)]
pub(crate) struct CatalogArgs {
    /// The map.
    pub(crate) map: PathBuf,
    /// What to list: terrains, sprites, tiles or types; all but tiles by default.
    pub(crate) what: Option<CatalogWhat>,
    /// With `tiles`: a tileset, or tiles TILESET:ID,… (up to twelve).
    pub(crate) tiles: Option<String>,
    /// Only items whose name or description holds this.
    #[arg(long, value_name = "TEXT")]
    pub(crate) find: Option<String>,
    /// A picture of the listed items, numbered (tiles: by id, laid out as in the tileset).
    #[arg(long, value_name = "OUT.png")]
    pub(crate) sheet: Option<PathBuf>,
    /// How much bigger the sheet's items are drawn.
    #[arg(long, value_name = "N", requires = "sheet")]
    pub(crate) scale: Option<u32>,
}

/// What `bt catalog` lists.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum CatalogWhat {
    /// Terrains, with what they border and where they are used.
    Terrains,
    /// Sprites, with their sizes, pairs and what their copies share.
    Sprites,
    /// Tiles by id, with where each is used.
    Tiles,
    /// Object types, with counts and properties.
    Types,
}

/// `bt areas`.
#[derive(Debug, Args)]
pub(crate) struct AreasArgs {
    /// The map.
    pub(crate) map: PathBuf,
    /// The shape, as in scripts (quote it when it has spaces).
    pub(crate) shape: String,
    /// List every area, not the first few.
    #[arg(long)]
    pub(crate) all: bool,
    /// A file of define lines the shape may use.
    #[arg(long, value_name = "FILE")]
    pub(crate) with: Option<PathBuf>,
    /// The seed of rough shapes.
    #[arg(long, value_name = "N")]
    pub(crate) seed: Option<u64>,
}

/// `bt reach`.
#[derive(Debug, Args)]
pub(crate) struct ReachArgs {
    /// The map.
    pub(crate) map: PathBuf,
    /// Where the way starts: a cell, or a shape (any of its cells).
    pub(crate) from: String,
    /// Where it ends: a cell, or a shape.
    pub(crate) to: String,
    /// The cells a way may pass through; every cell by default.
    #[arg(long, value_name = "SHAPE")]
    pub(crate) through: Option<String>,
    /// A file of define lines the shapes may use.
    #[arg(long, value_name = "FILE")]
    pub(crate) with: Option<PathBuf>,
    /// The seed of rough shapes.
    #[arg(long, value_name = "N")]
    pub(crate) seed: Option<u64>,
}

/// `bt explain`.
#[derive(Debug, Args)]
pub(crate) struct ExplainArgs {
    /// The map.
    pub(crate) map: PathBuf,
    /// The cell, X,Y.
    pub(crate) cell: String,
    /// The rules file, then RULE-LAYER=MAP-LAYER bindings; the rules.txt beside the map by
    /// default.
    pub(crate) rules_and_bindings: Vec<String>,
    /// The seed of the rules' random choices.
    #[arg(long, value_name = "N")]
    pub(crate) seed: Option<u64>,
    /// On a map with AutoMap layers, only this run (from 1).
    #[arg(long, value_name = "N")]
    pub(crate) run: Option<usize>,
    /// List every rule that could write the cell, not only those that came close.
    #[arg(long)]
    pub(crate) all: bool,
}

/// `bt render`.
#[derive(Debug, Args)]
pub(crate) struct RenderArgs {
    /// The map.
    pub(crate) map: PathBuf,
    /// The picture to write.
    pub(crate) out: PathBuf,
    /// Only these cells.
    #[arg(long, num_args = 1..=2, value_name = "R")]
    pub(crate) region: Vec<String>,
    /// How many pixels a map pixel takes (up to 8).
    #[arg(long, value_name = "S")]
    pub(crate) scale: Option<f64>,
    /// A grid line around every cell.
    #[arg(long, conflicts_with = "clean")]
    pub(crate) grid: bool,
    /// No grid lines and no numbers.
    #[arg(long)]
    pub(crate) clean: bool,
    /// Outline these cells in red: a region or a shape.
    #[arg(long, value_name = "SHAPE")]
    pub(crate) highlight: Option<String>,
    /// Only these layers.
    #[arg(long, value_delimiter = ',', value_name = "L,…")]
    pub(crate) layers: Vec<String>,
    /// The editor's marks on this map, numbered in their colours.
    #[arg(long)]
    pub(crate) marks: bool,
    /// A file of define lines the highlight may use.
    #[arg(long, value_name = "FILE")]
    pub(crate) with: Option<PathBuf>,
    /// What was painted, on a map with AutoMap layers.
    #[arg(long)]
    pub(crate) painted: bool,
}

/// `bt new`.
#[derive(Debug, Args)]
pub(crate) struct NewArgs {
    /// The map to write.
    pub(crate) map: PathBuf,
    /// A map whose tilesets and layers the new one takes, emptied.
    #[arg(long, value_name = "OTHER.tmx")]
    pub(crate) like: Option<PathBuf>,
    /// The size in cells.
    #[arg(long, value_name = "WxH")]
    pub(crate) size: Option<String>,
    /// The tile size in pixels (without --like).
    #[arg(long, value_name = "WxH", conflicts_with = "like")]
    pub(crate) tile: Option<String>,
    /// External tilesets to use (without --like).
    #[arg(
        long,
        value_delimiter = ',',
        value_name = "A,B",
        conflicts_with = "like"
    )]
    pub(crate) tilesets: Vec<PathBuf>,
    /// Layers as name:tiles or name:objects (without --like).
    #[arg(
        long,
        value_delimiter = ',',
        value_name = "name:kind,…",
        conflicts_with = "like"
    )]
    pub(crate) layers: Vec<String>,
    /// Write over a file that is there.
    #[arg(long)]
    pub(crate) force: bool,
}

/// `bt edit`.
#[derive(Debug, Args)]
pub(crate) struct EditArgs {
    /// The map, then the script file (`-` or none: standard input); with --do, the maps.
    #[arg(required = true, value_name = "MAP [SCRIPT|-]")]
    pub(crate) paths: Vec<PathBuf>,
    /// Operations instead of a script, split by `;`.
    #[arg(long = "do", value_name = "OPS")]
    pub(crate) ops: Option<String>,
    /// A file of define lines run before the script.
    #[arg(long, value_name = "FILE")]
    pub(crate) with: Option<PathBuf>,
    /// The seed of the script's random choices.
    #[arg(long, value_name = "N")]
    pub(crate) seed: Option<u64>,
    /// Report the result without writing it.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Add the changed area as text.
    #[arg(long)]
    pub(crate) view: bool,
    /// With --view: what a character stands for.
    #[arg(long, value_enum, requires = "view")]
    pub(crate) by: Option<ViewBy>,
    /// With --view: each row's runs.
    #[arg(long, requires = "view")]
    pub(crate) runs: bool,
    /// Add a picture with the change outlined.
    #[arg(long, value_name = "OUT.png")]
    pub(crate) render: Option<PathBuf>,
    /// With --render: only these cells.
    #[arg(long, num_args = 1..=2, value_name = "R", requires = "render")]
    pub(crate) region: Vec<String>,
    /// With --render: the scale.
    #[arg(long, value_name = "S", requires = "render")]
    pub(crate) scale: Option<f64>,
    /// Write the result to this file instead.
    #[arg(long, value_name = "OTHER.tmx")]
    pub(crate) out: Option<PathBuf>,
    /// Fail instead of writing the file when the map is not open in the editor.
    #[arg(long, conflicts_with = "out")]
    pub(crate) in_editor: bool,
    /// Outline in the editor the cells the edit changed (with --dry-run: would change).
    #[arg(long, value_name = "LABEL", conflicts_with = "out")]
    pub(crate) mark: Option<String>,
    /// Write a map an editor holds unsaved changes to that bt cannot reach.
    #[arg(long)]
    pub(crate) force: bool,
}

/// `bt undo`.
#[derive(Debug, Args)]
pub(crate) struct UndoArgs {
    /// The map.
    pub(crate) map: PathBuf,
    /// Take the edit back even though the file was saved since.
    #[arg(long)]
    pub(crate) force: bool,
}

/// `bt diff`.
#[derive(Debug, Args)]
pub(crate) struct DiffArgs {
    /// The map.
    pub(crate) map: PathBuf,
    /// The map to compare it with; without, its unsaved changes in the editor.
    pub(crate) other: Option<PathBuf>,
}

/// `bt editor`.
#[derive(Debug, Args)]
pub(crate) struct EditorArgs {
    /// Only this map.
    pub(crate) map: Option<PathBuf>,
    /// Also a picture of the editor's window, as the person sees it.
    #[arg(long, value_name = "OUT.png")]
    pub(crate) screenshot: Option<PathBuf>,
}

/// `bt mark`.
#[derive(Debug, Args)]
pub(crate) struct MarkArgs {
    /// The map.
    pub(crate) map: PathBuf,
    /// The cells, as a shape (quote it when it has spaces).
    #[arg(required_unless_present = "clear")]
    pub(crate) shape: Option<String>,
    /// The mark's label.
    #[arg(long, value_name = "TEXT")]
    pub(crate) label: Option<String>,
    /// Keep the map's other marks.
    #[arg(long)]
    pub(crate) add: bool,
    /// Leave the view where it is.
    #[arg(long)]
    pub(crate) no_focus: bool,
    /// A file of define lines the shape may use.
    #[arg(long, value_name = "FILE")]
    pub(crate) with: Option<PathBuf>,
    /// Take the map's marks away.
    #[arg(long, conflicts_with_all = ["shape", "label", "add", "no_focus", "with"])]
    pub(crate) clear: bool,
}

/// `bt export`.
#[derive(Debug, Args)]
pub(crate) struct ExportArgs {
    /// The map or tileset.
    #[arg(required_unless_present = "schema")]
    pub(crate) input: Option<PathBuf>,
    /// The file to write; its extension names the format.
    #[arg(required_unless_present = "schema")]
    pub(crate) out: Option<PathBuf>,
    /// Write the map's tilesets into it.
    #[arg(long)]
    pub(crate) embed_tilesets: bool,
    /// Write template instances as plain objects.
    #[arg(long)]
    pub(crate) detach_templates: bool,
    /// Write the properties types give, not only those set.
    #[arg(long)]
    pub(crate) resolve_types: bool,
    /// Leave out what Tiled leaves out when minimizing.
    #[arg(long)]
    pub(crate) minimize: bool,
    /// Plain JSON for other programs (OUT.json).
    #[arg(long, conflicts_with_all = ["embed_tilesets", "detach_templates", "resolve_types", "minimize"])]
    pub(crate) resolved: bool,
    /// Print the resolved JSON's schema.
    #[arg(long, conflicts_with_all = ["input", "out"])]
    pub(crate) schema: bool,
}
