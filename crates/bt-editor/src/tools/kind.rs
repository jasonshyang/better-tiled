//! The tools: which there are, and which layers each works on.

/// A tool of the canvas.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ToolKind {
    /// Paints the brush.
    Brush,
    /// Paints terrain.
    Terrain,
    /// Erases tiles.
    Eraser,
    /// Fills an area of like tiles.
    BucketFill,
    /// Paints a rectangle or an ellipse.
    ShapeFill,
    /// Takes the tile under the pointer as the brush.
    Picker,
    /// Selects a rectangle of tiles.
    RectSelect,
    /// Selects connected like tiles.
    MagicWand,
    /// Selects every like tile.
    SameTile,
    /// Moves and resizes maps in a world.
    World,
    /// Moves layers' offsets.
    OffsetLayers,
    /// Selects, moves, resizes and turns objects.
    SelectObjects,
    /// Draws rectangles.
    InsertRectangle,
    /// Draws ellipses.
    InsertEllipse,
    /// Draws capsules.
    InsertCapsule,
    /// Places points.
    InsertPoint,
    /// Draws polygons, or polylines when not closed.
    InsertPolygon,
    /// Places tile objects.
    InsertTile,
    /// Places text.
    InsertText,
    /// Edits the points of polygons and polylines.
    EditPolygons,
    /// Explains what the AutoMap rules did at a cell.
    Explain,
}

/// The layers a tool works on.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ToolFamily {
    /// Tile layers.
    Tile,
    /// Object layers.
    Object,
    /// Any layer.
    Any,
}

impl ToolKind {
    /// The layers it works on.
    pub fn family(self) -> ToolFamily {
        match self {
            ToolKind::Brush
            | ToolKind::Terrain
            | ToolKind::Eraser
            | ToolKind::BucketFill
            | ToolKind::ShapeFill
            | ToolKind::Picker
            | ToolKind::RectSelect
            | ToolKind::MagicWand
            | ToolKind::SameTile => ToolFamily::Tile,
            ToolKind::World | ToolKind::OffsetLayers | ToolKind::Explain => ToolFamily::Any,
            ToolKind::SelectObjects
            | ToolKind::InsertRectangle
            | ToolKind::InsertEllipse
            | ToolKind::InsertCapsule
            | ToolKind::InsertPoint
            | ToolKind::InsertPolygon
            | ToolKind::InsertTile
            | ToolKind::InsertText
            | ToolKind::EditPolygons => ToolFamily::Object,
        }
    }

    /// Whether it selects tiles (Rectangular Select, the Magic Wand, Select Same Tile): with it,
    /// dragging inside the selection, X, Y and Z move, mirror and turn the selected tiles.
    pub fn selects_tiles(self) -> bool {
        matches!(
            self,
            ToolKind::RectSelect | ToolKind::MagicWand | ToolKind::SameTile
        )
    }
}
