//! How a script refers to what a map holds (`10-agents.md`, §5): content by name or tiles by
//! id, layers by name or id, objects by id, name or type, or as the editor's selection or what
//! the previous operation made. These are syntax only; `crate::names` resolves them.

/// Map content as a script names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Named {
    /// By name, slugged when looked up: a terrain, a tile group, a sprite, a material, a group,
    /// a piece or a defined area. Which kinds count is the operation's.
    Name(String),
    /// Tiles by id, which together are one ground: any of them.
    Tiles(TileIds),
}

/// Tiles by id: `TILESET:ID`, `TILESET:A-B`, and after one of those more ids or ranges of the
/// same tileset (`terrain:1-4,9`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileIds {
    /// The tileset, by name, slugged when looked up.
    pub tileset: String,
    /// The ids, in the order written.
    pub ranges: Vec<IdRange>,
}

/// Tile ids from `first` to `last`, both included: `ID` is a range of one.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct IdRange {
    /// The first id.
    pub first: u32,
    /// The last id.
    pub last: u32,
}

/// A layer as a script names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayerRef {
    /// By name, slugged when looked up; several layers of one name must be told apart by id.
    Name(String),
    /// `#ID`.
    Id(u32),
}

/// Objects as a script names them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectRef {
    /// `#ID`.
    Id(u32),
    /// By name when one object has it, else by type when exactly one object has that type.
    Name(String),
    /// `selected`: the objects selected in the editor.
    Selected,
    /// `last`: the objects the previous operation that made objects made.
    Last,
}
