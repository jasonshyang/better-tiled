//! How Tiled spells enumerated values, in both directions, and the version strings it writes.

use bt_core::{
    Alignment, BlendMode, DrawOrder, FillMode, GridOrientation, HAlign, LayerDataFormat,
    Orientation, RenderOrder, StaggerAxis, StaggerIndex, TileRenderSize, VAlign, WangSetType,
};

/// The Tiled release whose formats this crate writes, given as `tiledversion` to files that
/// have none.
pub(crate) const TILED_VERSION: &str = "1.12.2";

/// The orientation a name gives.
pub(crate) fn orientation_from_name(name: &str) -> Option<Orientation> {
    match name {
        "orthogonal" => Some(Orientation::Orthogonal),
        "isometric" => Some(Orientation::Isometric),
        "staggered" => Some(Orientation::Staggered),
        "hexagonal" => Some(Orientation::Hexagonal),
        "oblique" => Some(Orientation::Oblique),
        _ => None,
    }
}

/// An orientation's name.
pub(crate) fn orientation_name(orientation: Orientation) -> &'static str {
    match orientation {
        Orientation::Orthogonal => "orthogonal",
        Orientation::Isometric => "isometric",
        Orientation::Staggered => "staggered",
        Orientation::Hexagonal => "hexagonal",
        Orientation::Oblique => "oblique",
    }
}

/// The render order a name gives.
pub(crate) fn render_order_from_name(name: &str) -> Option<RenderOrder> {
    match name {
        "right-down" => Some(RenderOrder::RightDown),
        "right-up" => Some(RenderOrder::RightUp),
        "left-down" => Some(RenderOrder::LeftDown),
        "left-up" => Some(RenderOrder::LeftUp),
        _ => None,
    }
}

/// A render order's name.
pub(crate) fn render_order_name(order: RenderOrder) -> &'static str {
    match order {
        RenderOrder::RightDown => "right-down",
        RenderOrder::RightUp => "right-up",
        RenderOrder::LeftDown => "left-down",
        RenderOrder::LeftUp => "left-up",
    }
}

/// The stagger axis a name gives.
pub(crate) fn stagger_axis_from_name(name: &str) -> Option<StaggerAxis> {
    match name {
        "x" => Some(StaggerAxis::X),
        "y" => Some(StaggerAxis::Y),
        _ => None,
    }
}

/// A stagger axis's name.
pub(crate) fn stagger_axis_name(axis: StaggerAxis) -> &'static str {
    match axis {
        StaggerAxis::X => "x",
        StaggerAxis::Y => "y",
    }
}

/// The stagger index a name gives.
pub(crate) fn stagger_index_from_name(name: &str) -> Option<StaggerIndex> {
    match name {
        "odd" => Some(StaggerIndex::Odd),
        "even" => Some(StaggerIndex::Even),
        _ => None,
    }
}

/// A stagger index's name.
pub(crate) fn stagger_index_name(index: StaggerIndex) -> &'static str {
    match index {
        StaggerIndex::Odd => "odd",
        StaggerIndex::Even => "even",
    }
}

/// The draw order a name gives.
pub(crate) fn draw_order_from_name(name: &str) -> Option<DrawOrder> {
    match name {
        "topdown" => Some(DrawOrder::TopDown),
        "index" => Some(DrawOrder::Index),
        _ => None,
    }
}

/// A draw order's name.
pub(crate) fn draw_order_name(order: DrawOrder) -> &'static str {
    match order {
        DrawOrder::TopDown => "topdown",
        DrawOrder::Index => "index",
    }
}

/// The object alignment a name gives.
pub(crate) fn alignment_from_name(name: &str) -> Option<Alignment> {
    match name {
        "unspecified" => Some(Alignment::Unspecified),
        "topleft" => Some(Alignment::TopLeft),
        "top" => Some(Alignment::Top),
        "topright" => Some(Alignment::TopRight),
        "left" => Some(Alignment::Left),
        "center" => Some(Alignment::Center),
        "right" => Some(Alignment::Right),
        "bottomleft" => Some(Alignment::BottomLeft),
        "bottom" => Some(Alignment::Bottom),
        "bottomright" => Some(Alignment::BottomRight),
        _ => None,
    }
}

/// An object alignment's name.
pub(crate) fn alignment_name(alignment: Alignment) -> &'static str {
    match alignment {
        Alignment::Unspecified => "unspecified",
        Alignment::TopLeft => "topleft",
        Alignment::Top => "top",
        Alignment::TopRight => "topright",
        Alignment::Left => "left",
        Alignment::Center => "center",
        Alignment::Right => "right",
        Alignment::BottomLeft => "bottomleft",
        Alignment::Bottom => "bottom",
        Alignment::BottomRight => "bottomright",
    }
}

/// The blend mode a name gives.
pub(crate) fn blend_mode_from_name(name: &str) -> Option<BlendMode> {
    match name {
        "normal" => Some(BlendMode::Normal),
        "add" => Some(BlendMode::Add),
        "multiply" => Some(BlendMode::Multiply),
        "screen" => Some(BlendMode::Screen),
        "overlay" => Some(BlendMode::Overlay),
        "darken" => Some(BlendMode::Darken),
        "lighten" => Some(BlendMode::Lighten),
        "color-dodge" => Some(BlendMode::ColorDodge),
        "color-burn" => Some(BlendMode::ColorBurn),
        "hard-light" => Some(BlendMode::HardLight),
        "soft-light" => Some(BlendMode::SoftLight),
        "difference" => Some(BlendMode::Difference),
        "exclusion" => Some(BlendMode::Exclusion),
        _ => None,
    }
}

/// A blend mode's name.
pub(crate) fn blend_mode_name(mode: BlendMode) -> &'static str {
    match mode {
        BlendMode::Normal => "normal",
        BlendMode::Add => "add",
        BlendMode::Multiply => "multiply",
        BlendMode::Screen => "screen",
        BlendMode::Overlay => "overlay",
        BlendMode::Darken => "darken",
        BlendMode::Lighten => "lighten",
        BlendMode::ColorDodge => "color-dodge",
        BlendMode::ColorBurn => "color-burn",
        BlendMode::HardLight => "hard-light",
        BlendMode::SoftLight => "soft-light",
        BlendMode::Difference => "difference",
        BlendMode::Exclusion => "exclusion",
    }
}

/// The terrain set kind a name gives.
pub(crate) fn wang_set_type_from_name(name: &str) -> Option<WangSetType> {
    match name {
        "corner" => Some(WangSetType::Corner),
        "edge" => Some(WangSetType::Edge),
        "mixed" => Some(WangSetType::Mixed),
        _ => None,
    }
}

/// A terrain set kind's name.
pub(crate) fn wang_set_type_name(kind: WangSetType) -> &'static str {
    match kind {
        WangSetType::Corner => "corner",
        WangSetType::Edge => "edge",
        WangSetType::Mixed => "mixed",
    }
}

/// The tile render size a name gives.
pub(crate) fn tile_render_size_from_name(name: &str) -> Option<TileRenderSize> {
    match name {
        "tile" => Some(TileRenderSize::Tile),
        "grid" => Some(TileRenderSize::Grid),
        _ => None,
    }
}

/// A tile render size's name.
pub(crate) fn tile_render_size_name(size: TileRenderSize) -> &'static str {
    match size {
        TileRenderSize::Tile => "tile",
        TileRenderSize::Grid => "grid",
    }
}

/// The fill mode a name gives.
pub(crate) fn fill_mode_from_name(name: &str) -> Option<FillMode> {
    match name {
        "stretch" => Some(FillMode::Stretch),
        "preserve-aspect-fit" => Some(FillMode::PreserveAspectFit),
        _ => None,
    }
}

/// A fill mode's name.
pub(crate) fn fill_mode_name(mode: FillMode) -> &'static str {
    match mode {
        FillMode::Stretch => "stretch",
        FillMode::PreserveAspectFit => "preserve-aspect-fit",
    }
}

/// The tileset grid orientation a name gives.
pub(crate) fn grid_orientation_from_name(name: &str) -> Option<GridOrientation> {
    match name {
        "orthogonal" => Some(GridOrientation::Orthogonal),
        "isometric" => Some(GridOrientation::Isometric),
        _ => None,
    }
}

/// A tileset grid orientation's name.
pub(crate) fn grid_orientation_name(orientation: GridOrientation) -> &'static str {
    match orientation {
        GridOrientation::Orthogonal => "orthogonal",
        GridOrientation::Isometric => "isometric",
    }
}

/// The horizontal text alignment a name gives.
pub(crate) fn halign_from_name(name: &str) -> Option<HAlign> {
    match name {
        "left" => Some(HAlign::Left),
        "center" => Some(HAlign::Center),
        "right" => Some(HAlign::Right),
        "justify" => Some(HAlign::Justify),
        _ => None,
    }
}

/// A horizontal text alignment's name.
pub(crate) fn halign_name(align: HAlign) -> &'static str {
    match align {
        HAlign::Left => "left",
        HAlign::Center => "center",
        HAlign::Right => "right",
        HAlign::Justify => "justify",
    }
}

/// The vertical text alignment a name gives.
pub(crate) fn valign_from_name(name: &str) -> Option<VAlign> {
    match name {
        "top" => Some(VAlign::Top),
        "center" => Some(VAlign::Center),
        "bottom" => Some(VAlign::Bottom),
        _ => None,
    }
}

/// A vertical text alignment's name.
pub(crate) fn valign_name(align: VAlign) -> &'static str {
    match align {
        VAlign::Top => "top",
        VAlign::Center => "center",
        VAlign::Bottom => "bottom",
    }
}

/// The layer data format an encoding and compression give (`""` for absent); `None` for one
/// Tiled does not write.
pub(crate) fn layer_data_format_from_names(
    encoding: &str,
    compression: &str,
) -> Option<LayerDataFormat> {
    match (encoding, compression) {
        ("", _) => Some(LayerDataFormat::Xml),
        ("csv", _) => Some(LayerDataFormat::Csv),
        ("base64", "") => Some(LayerDataFormat::Base64),
        ("base64", "zlib") => Some(LayerDataFormat::Base64Zlib),
        ("base64", "gzip") => Some(LayerDataFormat::Base64Gzip),
        ("base64", "zstd") => Some(LayerDataFormat::Base64Zstd),
        _ => None,
    }
}

/// The encoding attribute of a layer data format (`None` for XML's `<tile>` elements).
pub(crate) fn encoding_name(format: LayerDataFormat) -> Option<&'static str> {
    match format {
        LayerDataFormat::Xml => None,
        LayerDataFormat::Csv => Some("csv"),
        LayerDataFormat::Base64
        | LayerDataFormat::Base64Zlib
        | LayerDataFormat::Base64Gzip
        | LayerDataFormat::Base64Zstd => Some("base64"),
    }
}

/// The compression attribute of a layer data format (`""` for uncompressed base64, `None`
/// where there is no compression attribute).
pub(crate) fn compression_name(format: LayerDataFormat) -> Option<&'static str> {
    match format {
        LayerDataFormat::Xml | LayerDataFormat::Csv => None,
        LayerDataFormat::Base64 => Some(""),
        LayerDataFormat::Base64Zlib => Some("zlib"),
        LayerDataFormat::Base64Gzip => Some("gzip"),
        LayerDataFormat::Base64Zstd => Some("zstd"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_go_both_ways() {
        let modes = [
            BlendMode::Normal,
            BlendMode::Add,
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::Overlay,
            BlendMode::Darken,
            BlendMode::Lighten,
            BlendMode::ColorDodge,
            BlendMode::ColorBurn,
            BlendMode::HardLight,
            BlendMode::SoftLight,
            BlendMode::Difference,
            BlendMode::Exclusion,
        ];
        for mode in modes {
            assert_eq!(blend_mode_from_name(blend_mode_name(mode)), Some(mode));
        }
        assert_eq!(blend_mode_from_name("glow"), None);
        assert_eq!(
            orientation_from_name(orientation_name(Orientation::Oblique)),
            Some(Orientation::Oblique)
        );
        assert_eq!(
            alignment_from_name("bottomright"),
            Some(Alignment::BottomRight)
        );
        assert_eq!(
            layer_data_format_from_names("base64", "zstd"),
            Some(LayerDataFormat::Base64Zstd)
        );
        assert_eq!(layer_data_format_from_names("base64", "lzma"), None);
        assert_eq!(layer_data_format_from_names("hex", ""), None);
        assert_eq!(compression_name(LayerDataFormat::Base64), Some(""));
        assert_eq!(encoding_name(LayerDataFormat::Xml), None);
    }
}
