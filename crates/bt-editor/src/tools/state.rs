//! The tools' editor-wide state: stored once, never copied between documents, with the rules
//! that keep the tool, the layer and the brush coherent.

use bt_edit::{Brush, BrushMode, FillShape, SelectionMode};
use bt_terrain::BrushTarget;
use bt_utils::rng::Rng;
use glam::IVec2;

use super::kind::{ToolFamily, ToolKind};
use super::terrain::TerrainChoice;

/// The current tool and its options.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolState {
    /// The tool in use.
    pub(crate) tool: ToolKind,
    /// The tool before it, for a tool's key pressed again.
    pub(crate) previous_tool: Option<ToolKind>,
    /// The tile tool to bring back when a tile layer becomes current.
    pub(crate) last_tile_tool: ToolKind,
    /// The object tool to bring back when an object layer becomes current.
    pub(crate) last_object_tool: ToolKind,
    /// The brush paints a random tile of itself in each cell.
    pub(crate) random: bool,
    /// The side of the square a one-tile or random brush and the eraser paint (1 is one cell).
    pub(crate) brush_size: u32,
    /// The selection tools' default mode (the toolbar's).
    pub(crate) selection_mode: SelectionMode,
    /// What Shape Fill paints.
    pub(crate) fill_shape: FillShape,
    /// What the painting tools paint.
    pub(crate) brush: Option<Brush>,
    /// Nine quick stamps, including the grid their variations were captured on.
    pub(crate) quick_stamps: [Option<bt_tmx::Stamp>; 9],
    pub(crate) quick_stamps_generation: u64,
    /// The seed the next placement, fill or random tile is drawn from; it moves on after each
    /// paint, so the next preview shows the next variation.
    pub(crate) seed: u64,
    /// Where the last brush stroke ended, where Shift's line starts.
    pub(crate) last_paint: Option<IVec2>,
    /// The terrain the Terrain tool paints and terrain fills prefer.
    pub(crate) terrain: Option<TerrainChoice>,
    /// Whether Bucket Fill and Shape Fill fill with the terrain rather than the brush; picking
    /// a brush ends it.
    pub(crate) terrain_fill: bool,
    /// The seed of the next terrain stroke or fill; it moves on after each, as the brush's does.
    pub(crate) terrain_seed: u64,
    /// Where the last terrain stroke ended, where Shift's line starts.
    pub(crate) last_terrain_target: Option<BrushTarget>,
}

impl Default for ToolState {
    fn default() -> ToolState {
        ToolState {
            tool: ToolKind::Brush,
            previous_tool: None,
            last_tile_tool: ToolKind::Brush,
            last_object_tool: ToolKind::SelectObjects,
            random: false,
            brush_size: 1,
            selection_mode: SelectionMode::Replace,
            fill_shape: FillShape::Rectangle,
            brush: None,
            quick_stamps: Default::default(),
            quick_stamps_generation: 0,
            seed: 0x5EED_0FB7_7E55,
            last_paint: None,
            terrain: None,
            terrain_fill: false,
            terrain_seed: 0x7E44_A1D5_EED0,
            last_terrain_target: None,
        }
    }
}

/// The largest brush side.
pub const MAX_BRUSH_SIZE: u32 = 64;

impl ToolState {
    /// The tool in use.
    pub fn tool(&self) -> ToolKind {
        self.tool
    }

    /// Whether the brush paints random tiles.
    pub fn random(&self) -> bool {
        self.random
    }

    /// The brush and eraser size.
    pub fn brush_size(&self) -> u32 {
        self.brush_size
    }

    /// The selection tools' default mode.
    pub fn selection_mode(&self) -> SelectionMode {
        self.selection_mode
    }

    /// What Shape Fill paints.
    pub fn fill_shape(&self) -> FillShape {
        self.fill_shape
    }

    /// The brush, if one was picked.
    pub fn brush(&self) -> Option<&Brush> {
        self.brush.as_ref()
    }

    /// The quick stamp stored in a slot (0–8), if any.
    pub fn quick_stamp(&self, slot: usize) -> Option<&bt_tmx::Stamp> {
        self.quick_stamps.get(slot).and_then(Option::as_ref)
    }

    /// Where the last brush stroke ended.
    pub fn last_paint(&self) -> Option<IVec2> {
        self.last_paint
    }

    /// The terrain chosen, if any.
    pub fn terrain(&self) -> Option<TerrainChoice> {
        self.terrain
    }

    /// Whether the fill tools fill with the terrain.
    pub fn terrain_fill(&self) -> bool {
        self.terrain_fill
    }

    /// Where the last terrain stroke ended.
    pub fn last_terrain_target(&self) -> Option<BrushTarget> {
        self.last_terrain_target
    }

    /// Moves the terrain seed on, for the next stroke or fill.
    pub(crate) fn reseed_terrain(&mut self) {
        self.terrain_seed = Rng::new(self.terrain_seed).next_u64();
    }

    /// Moves the seed on, for the next paint.
    pub(crate) fn reseed(&mut self) {
        self.seed = Rng::new(self.seed).next_u64();
    }

    /// Switches random mode, the brush following.
    pub(crate) fn set_random(&mut self, random: bool) {
        self.random = random;
        let mode = if random {
            BrushMode::Random
        } else {
            BrushMode::Stamp
        };
        self.brush = self.brush.take().map(|brush| brush.with_mode(mode));
    }

    /// Makes `tool` current, remembering it as its family's last tool.
    pub(crate) fn set_tool(&mut self, tool: ToolKind) {
        if tool != self.tool {
            self.previous_tool = Some(self.tool);
            self.tool = tool;
        }
        match tool.family() {
            ToolFamily::Tile => self.last_tile_tool = tool,
            ToolFamily::Object => self.last_object_tool = tool,
            ToolFamily::Any => {}
        }
    }

    /// Follows the current layer: a tile layer brings back the last tile tool and an object
    /// layer the last object tool, unless the tool in use works on any layer.
    pub(crate) fn follow_layer(&mut self, family: Option<ToolFamily>) {
        let wanted = match family {
            Some(ToolFamily::Tile) => self.last_tile_tool,
            Some(ToolFamily::Object) => self.last_object_tool,
            Some(ToolFamily::Any) | None => return,
        };
        let fits =
            matches!(self.tool.family(), ToolFamily::Any) || Some(self.tool.family()) == family;
        if !fits {
            self.previous_tool = Some(self.tool);
            self.tool = wanted;
        }
    }

    /// Sets the brush and eraser size, within 1 and [`MAX_BRUSH_SIZE`].
    pub(crate) fn set_brush_size(&mut self, size: u32) {
        self.brush_size = size.clamp(1, MAX_BRUSH_SIZE);
        let size = self.brush_size;
        self.brush = self.brush.take().map(|brush| brush.with_size(size));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_layer_family_brings_back_its_last_tool() {
        let mut tools = ToolState::default();
        tools.set_tool(ToolKind::Eraser);
        tools.follow_layer(Some(ToolFamily::Object));
        assert_eq!(tools.tool(), ToolKind::SelectObjects);
        tools.set_tool(ToolKind::InsertPoint);
        tools.follow_layer(Some(ToolFamily::Tile));
        assert_eq!(tools.tool(), ToolKind::Eraser, "the last tile tool");
        tools.follow_layer(Some(ToolFamily::Object));
        assert_eq!(tools.tool(), ToolKind::InsertPoint);
        tools.set_tool(ToolKind::OffsetLayers);
        tools.follow_layer(Some(ToolFamily::Tile));
        assert_eq!(
            tools.tool(),
            ToolKind::OffsetLayers,
            "it works on any layer"
        );
        tools.follow_layer(None);
        assert_eq!(tools.tool(), ToolKind::OffsetLayers);
    }

    #[test]
    fn brush_sizes_stay_in_range() {
        let mut tools = ToolState::default();
        tools.set_brush_size(0);
        assert_eq!(tools.brush_size(), 1);
        tools.set_brush_size(1000);
        assert_eq!(tools.brush_size(), MAX_BRUSH_SIZE);
    }
}
