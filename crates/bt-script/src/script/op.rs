//! A script's operations as parsed (`10-agents.md`, §7): one variant per operation, with typed
//! parts. Names stay as written; the runner resolves them.

use bt_utils::IRect;
use glam::{DVec2, IVec2};

use super::area::{Area, Shape};
use super::refs::{LayerRef, Named, ObjectRef};

/// One line of a script: where it is, what it says and what it asks.
#[derive(Clone, Debug, PartialEq)]
pub struct Located {
    /// The line number, from 1.
    pub line: usize,
    /// The operation as written, for reports.
    pub text: String,
    /// The operation.
    pub op: Op,
    /// `seed N` at the end of the operation: its own randomness.
    pub seed: Option<u64>,
}

/// A parsed script.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Script {
    /// The comment on the first line, which names the edit in the editor's history.
    pub label: Option<String>,
    /// The operations in order.
    pub ops: Vec<Located>,
}

/// An operation.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// `seed N` on its own line: the randomness of every following operation.
    Seed(u64),
    /// `define NAME = TERRAIN [layer L] + …`: terrains painted together on several layers.
    DefineMaterial {
        /// The name defined.
        name: String,
        /// The terrains, painted in order.
        parts: Vec<TerrainPart>,
    },
    /// `define NAME = sprite S [layer L] [+ sprite S2 at DX,DY [layer L]] …`: sprites placed
    /// together, each at its offset from the first one's foot.
    DefineGroup {
        /// The name defined.
        name: String,
        /// The sprites, the first standing at the foot.
        parts: Vec<SpritePart>,
    },
    /// `define NAME = tile TILE,… [layer L]`: a name for tiles.
    DefineTiles {
        /// The name defined.
        name: String,
        /// The tiles.
        tiles: Vec<Named>,
        /// The layer they go on.
        layer: Option<LayerRef>,
    },
    /// `define NAME = area SHAPE`: a name for the cells as they are on that line.
    DefineArea {
        /// The name defined.
        name: String,
        /// The cells.
        area: Area,
    },
    /// `define NAME = copy MAP REGION [minus SHAPE] [layers L,…] [objects] [foot X,Y]`: a piece
    /// of a map, placed by its foot cell like a sprite.
    DefinePiece {
        /// The name defined.
        name: String,
        /// What is copied.
        source: CopySource,
        /// Cells of the region left out.
        minus: Vec<Shape>,
        /// The foot cell, in the map copied from; by default the middle of the region's bottom
        /// row.
        foot: Option<IVec2>,
    },
    /// `paint TERRAIN SHAPE [layer L]`: the cells become the terrain (or material), the tiles
    /// around them fitting.
    Paint {
        /// The terrain or material.
        terrain: String,
        /// The cells.
        area: Area,
        /// The layer.
        layer: Option<LayerRef>,
    },
    /// `erase TERRAIN SHAPE [layer L]`: the terrain is taken off the cells, the tiles around
    /// them fitting.
    Erase {
        /// The terrain or material.
        terrain: String,
        /// The cells.
        area: Area,
        /// The layer.
        layer: Option<LayerRef>,
    },
    /// `tile TILE,… SHAPE [layer L]`: the tiles as they are, picked at random when several.
    Tile {
        /// The tiles.
        tiles: Vec<Named>,
        /// The cells.
        area: Area,
        /// The layer.
        layer: Option<LayerRef>,
    },
    /// `clear SHAPE layer L,…|all`: the cells become empty, nothing fitted.
    Clear {
        /// The cells.
        area: Area,
        /// The layers emptied.
        layers: Layers,
    },
    /// `copy MAP REGION [layers L,…] to X,Y [objects]`: tiles (and objects) from a map, as they
    /// are.
    Copy {
        /// What is copied.
        source: CopySource,
        /// The top-left cell it goes to.
        to: IVec2,
    },
    /// `place NAME,… at X,Y … | in SHAPE | rect X,Y WxH …`: sprites, groups or pieces.
    Place {
        /// What is placed, picked at random when several.
        names: Vec<Named>,
        /// Where.
        placement: Placement,
        /// Whether placed mirrored.
        flip: Flip,
        /// How far a foot may move from the middle of a cell's bottom edge, in cells (`in`).
        jitter: f64,
        /// The objects' name, layer, model and properties.
        object: ObjectOptions,
    },
    /// `scatter NAME,… in SHAPE [density D|count N] [spacing S] [jitter J] [flip random] [layer
    /// L] [like OBJECT]`.
    Scatter {
        /// What is scattered, picked at random.
        names: Vec<Named>,
        /// The cells feet may stand in.
        area: Area,
        /// How many.
        amount: Amount,
        /// The least distance between feet, in cells.
        spacing: f64,
        /// How far a foot may move from the middle of its cell's bottom edge, in cells.
        jitter: f64,
        /// Whether placed mirrored.
        flip: Flip,
        /// The objects' layer and model.
        object: ObjectOptions,
    },
    /// `object TYPE at X,Y | rect | ellipse | poly | path … | in SHAPE …`: typed objects.
    Object {
        /// The type.
        kind: String,
        /// Its shape and place.
        geometry: Geometry,
        /// The objects' name, layer, model and properties.
        object: ObjectOptions,
    },
    /// `set OBJECT … key=value…`: fields (`name`, `type`, `visible`, `rotation`, `size`) and
    /// properties; an empty value removes a property.
    Set {
        /// The objects.
        targets: Vec<ObjectRef>,
        /// What is set.
        assignments: Vec<Assignment>,
    },
    /// `move OBJECT to X,Y`: one object by its foot, its point or its top-left.
    MoveTo {
        /// The object.
        target: ObjectRef,
        /// Where, in tile units.
        to: DVec2,
    },
    /// `move OBJECT … by DX,DY`.
    MoveBy {
        /// The objects.
        targets: Vec<ObjectRef>,
        /// How far, in tile units.
        by: DVec2,
    },
    /// `delete OBJECT …`.
    Delete {
        /// The objects.
        targets: Vec<ObjectRef>,
    },
    /// `delete objects in SHAPE [type T] [layer L]` or `delete objects on TERRAIN,… [type T]
    /// [layer L]`.
    DeleteObjects {
        /// Which objects by where they stand.
        place: ObjectsWhere,
        /// Only objects of this type.
        kind: Option<String>,
        /// Only objects of this layer.
        layer: Option<LayerRef>,
    },
    /// `automap [RULES] [RULE-LAYER=MAP-LAYER …]`: the rules applied once.
    Automap {
        /// The rules file; by default the `rules.txt` beside the map.
        rules: Option<String>,
        /// Names of the rules bound to other layers of the map.
        bindings: Vec<Binding>,
    },
    /// `automap layers …`: the map's AutoMap layers.
    AutomapLayers(LayersOp),
    /// `property key=value …`: the map's properties.
    Property {
        /// What is set.
        assignments: Vec<Assignment>,
    },
    /// `layer …`: one layer operation.
    Layer(LayerOp),
    /// `resize WxH [shift DX,DY]`.
    Resize {
        /// The new size in cells.
        size: IVec2,
        /// How far the content moves.
        shift: IVec2,
    },
    /// `draw X,Y` … `end`: characters on a grid, each standing for what its legend says.
    Draw(Drawing),
}

/// A terrain on a layer: a part of a material or of a legend.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainPart {
    /// The terrain (or a material).
    pub terrain: String,
    /// The layer.
    pub layer: Option<LayerRef>,
}

/// A sprite of a group.
#[derive(Clone, Debug, PartialEq)]
pub struct SpritePart {
    /// The sprite.
    pub sprite: Named,
    /// Its foot's offset from the first sprite's foot, in tile units.
    pub offset: DVec2,
    /// The layer.
    pub layer: Option<LayerRef>,
}

/// What `copy` and `define … = copy` take from a map.
#[derive(Clone, Debug, PartialEq)]
pub struct CopySource {
    /// The map, relative to the edited one, or `.` for the edited map itself.
    pub map: String,
    /// The region copied.
    pub region: IRect,
    /// The layers copied.
    pub layers: Layers,
    /// Whether the objects standing in the region come along.
    pub objects: bool,
}

/// Layers an operation takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Layers {
    /// All of them.
    All,
    /// These.
    Only(Vec<LayerRef>),
}

/// Where `place` puts things.
#[derive(Clone, Debug, PartialEq)]
pub enum Placement {
    /// Feet at these points, in tile units.
    At(Vec<DVec2>),
    /// One sprite stretched to a box (orthogonal maps).
    Rect {
        /// The top-left, in tile units.
        min: DVec2,
        /// The size, in tile units.
        size: DVec2,
    },
    /// One per cell, standing at the middle of the cell's bottom edge.
    In(Area),
}

/// Whether placed things are mirrored.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Flip {
    /// Not mirrored.
    #[default]
    None,
    /// Mirrored.
    Flipped,
    /// Each one mirrored or not at random.
    Random,
}

/// How many `scatter` places.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Amount {
    /// This many.
    Count(u32),
    /// This part of the shape's cells.
    Density(f64),
}

/// An object's shape and place, in tile units.
#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    /// A point.
    Point(DVec2),
    /// A rectangle.
    Rect {
        /// The top-left.
        min: DVec2,
        /// The size.
        size: DVec2,
    },
    /// An ellipse.
    Ellipse {
        /// The centre.
        center: DVec2,
        /// The radii.
        radii: DVec2,
    },
    /// A polygon.
    Polygon(Vec<DVec2>),
    /// A path (Tiled's polyline).
    Path(Vec<DVec2>),
    /// Points at the centres of cells picked at random in an area, `spacing` cells apart.
    In {
        /// The cells.
        area: Area,
        /// How many.
        count: u32,
        /// The least distance between them, in cells.
        spacing: f64,
    },
}

/// What objects an operation makes are given besides their shape.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjectOptions {
    /// `name N`.
    pub name: Option<String>,
    /// `layer L`.
    pub layer: Option<LayerRef>,
    /// `like OBJECT`: its type and properties, except what the line writes out.
    pub like: Option<ObjectRef>,
    /// `key=value …`: fields and properties as `set` takes them.
    pub assignments: Vec<Assignment>,
}

/// `key=value`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assignment {
    /// The key.
    pub key: String,
    /// The value as written; empty removes a property.
    pub value: String,
}

/// Which objects `delete objects` takes by where they stand.
#[derive(Clone, Debug, PartialEq)]
pub enum ObjectsWhere {
    /// `in SHAPE`: those standing in the cells.
    In(Area),
    /// `on TERRAIN,…`: those standing on the grounds, as `check` sees it.
    On(Vec<Named>),
}

/// `RULE-LAYER=MAP-LAYER`: a layer name of the rules bound to a layer of the map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    /// The name the rules use.
    pub rule_layer: String,
    /// The map's layer.
    pub map_layer: LayerRef,
}

/// What `automap layers` does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayersOp {
    /// `automap layers [RULES] [RULE-LAYER=MAP-LAYER …]`: start them, or start over, with one
    /// run.
    Start {
        /// The rules file; by default the `rules.txt` beside the map.
        rules: Option<String>,
        /// The run's bindings.
        bindings: Vec<Binding>,
    },
    /// `automap layers add RULE-LAYER=MAP-LAYER …`: one more run, on other layers.
    Add {
        /// The run's bindings.
        bindings: Vec<Binding>,
    },
    /// `automap layers seed N`: other random choices.
    Seed(u64),
    /// `automap layers off`: stop them; what the rules made stays as painted tiles.
    Off,
}

/// One layer operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayerOp {
    /// `layer add NAME tiles|objects [above L|below L]`.
    Add {
        /// The new layer's name.
        name: String,
        /// Whether it holds objects rather than tiles.
        objects: bool,
        /// Where it goes; on top by default.
        place: Option<LayerPlace>,
    },
    /// `layer rename L NAME`.
    Rename {
        /// The layer.
        layer: LayerRef,
        /// Its new name.
        name: String,
    },
    /// `layer delete L`.
    Delete {
        /// The layer.
        layer: LayerRef,
    },
    /// `layer hide L`, `layer show L`.
    SetVisible {
        /// The layer.
        layer: LayerRef,
        /// Whether shown.
        visible: bool,
    },
    /// `layer lock L`, `layer unlock L`.
    SetLocked {
        /// The layer.
        layer: LayerRef,
        /// Whether locked.
        locked: bool,
    },
    /// `layer move L above|below M`.
    Move {
        /// The layer.
        layer: LayerRef,
        /// Where it goes.
        place: LayerPlace,
    },
}

/// Above or below another layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerPlace {
    /// Whether above it rather than below.
    pub above: bool,
    /// The other layer.
    pub layer: LayerRef,
}

/// A `draw` block.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawing {
    /// The cell of the first row's first character.
    pub origin: IVec2,
    /// What the characters stand for.
    pub legend: Vec<LegendEntry>,
    /// The rows, without their leading `|`; `.` and spaces leave cells as they are.
    pub rows: Vec<String>,
    /// The line number of the first row.
    pub rows_line: usize,
}

/// `C = …`: what a character of a drawing stands for.
#[derive(Clone, Debug, PartialEq)]
pub struct LegendEntry {
    /// The character.
    pub symbol: char,
    /// What it stands for.
    pub meaning: Legend,
}

/// What a character of a drawing stands for.
#[derive(Clone, Debug, PartialEq)]
pub enum Legend {
    /// `C = TERRAIN [layer L] [+ …]`: terrains or materials, or a tile name's tiles.
    Paint(Vec<TerrainPart>),
    /// `C = tile TILE,… [layer L]`, or tiles written as ids on their own.
    Tiles {
        /// The tiles.
        tiles: Vec<Named>,
        /// The layer.
        layer: Option<LayerRef>,
    },
    /// `C = clear layer L,…|all`: the cells become empty.
    Clear(Layers),
    /// `C = sprite NAME,… [layer L]`: a sprite at the bottom middle of each cell.
    Sprites {
        /// The sprites, picked at random.
        sprites: Vec<Named>,
        /// The layer.
        layer: Option<LayerRef>,
    },
    /// `C = object TYPE [layer L] [key=value…]`: a point at the centre of each cell.
    Object {
        /// The type.
        kind: String,
        /// The layer.
        layer: Option<LayerRef>,
        /// Fields and properties.
        assignments: Vec<Assignment>,
    },
}
