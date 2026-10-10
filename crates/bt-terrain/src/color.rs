//! A terrain of a set, as a typed value.

/// One terrain of a terrain set: a 1-based index into its colours, or none (0), which paints
/// "no terrain" and so erases.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TerrainColor(pub u8);

impl TerrainColor {
    /// No terrain: what painting erases with.
    pub const NONE: TerrainColor = TerrainColor(0);

    /// Whether this is "no terrain".
    pub fn is_none(self) -> bool {
        self.0 == 0
    }
}
