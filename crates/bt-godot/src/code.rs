//! Stable diagnostic codes shared by reports, the editor and JSON.
use serde::Serialize;

/// A diagnostic emitted by the Godot exporter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Code {
    /// `godot/animation-layout`.
    #[serde(rename = "godot/animation-layout")]
    AnimationLayout,
    /// `godot/blend-mode`.
    #[serde(rename = "godot/blend-mode")]
    BlendMode,
    /// `godot/body-no-shape`.
    #[serde(rename = "godot/body-no-shape")]
    BodyNoShape,
    /// `godot/copy-changed`.
    #[serde(rename = "godot/copy-changed")]
    CopyChanged,
    /// `godot/copy-kept`.
    #[serde(rename = "godot/copy-kept")]
    CopyKept,
    /// `godot/custom-data-class`.
    #[serde(rename = "godot/custom-data-class")]
    CustomDataClass,
    /// `godot/custom-data-file`.
    #[serde(rename = "godot/custom-data-file")]
    CustomDataFile,
    /// `godot/custom-data-type`.
    #[serde(rename = "godot/custom-data-type")]
    CustomDataType,
    /// `godot/differs-from-tiled`.
    #[serde(rename = "godot/differs-from-tiled")]
    DiffersFromTiled,
    /// `godot/export-undeclared`.
    #[serde(rename = "godot/export-undeclared")]
    ExportUndeclared,
    /// `godot/grid-shift`.
    #[serde(rename = "godot/grid-shift")]
    GridShift,
    /// `godot/half-pixel-origin`.
    #[serde(rename = "godot/half-pixel-origin")]
    HalfPixelOrigin,
    /// `godot/hex-fallback`.
    #[serde(rename = "godot/hex-fallback")]
    HexFallback,
    /// `godot/hex-rotation`.
    #[serde(rename = "godot/hex-rotation")]
    HexRotation,
    /// `godot/image-not-copied`.
    #[serde(rename = "godot/image-not-copied")]
    ImageNotCopied,
    /// `godot/image-sources`.
    #[serde(rename = "godot/image-sources")]
    ImageSources,
    /// `godot/layer-ids-unsaved`.
    #[serde(rename = "godot/layer-ids-unsaved")]
    LayerIdsUnsaved,
    /// `godot/left-out`.
    #[serde(rename = "godot/left-out")]
    LeftOut,
    /// `godot/level-scene-automap`.
    #[serde(rename = "godot/level-scene-automap")]
    LevelSceneAutomap,
    /// `godot/level-scene-connection`.
    #[serde(rename = "godot/level-scene-connection")]
    LevelSceneConnection,
    /// `godot/level-scene-dropped`.
    #[serde(rename = "godot/level-scene-dropped")]
    LevelSceneDropped,
    /// `godot/level-scene-follows`.
    #[serde(rename = "godot/level-scene-follows")]
    LevelSceneFollows,
    /// `godot/level-scene-lost`.
    #[serde(rename = "godot/level-scene-lost")]
    LevelSceneLost,
    /// `godot/level-scene-moved-to-root`.
    #[serde(rename = "godot/level-scene-moved-to-root")]
    LevelSceneMovedToRoot,
    /// `godot/level-scene-renamed`.
    #[serde(rename = "godot/level-scene-renamed")]
    LevelSceneRenamed,
    /// `godot/level-scene-retargeted`.
    #[serde(rename = "godot/level-scene-retargeted")]
    LevelSceneRetargeted,
    /// `godot/level-scene-tiles`.
    #[serde(rename = "godot/level-scene-tiles")]
    LevelSceneTiles,
    /// `godot/level-scene-type`.
    #[serde(rename = "godot/level-scene-type")]
    LevelSceneType,
    /// `godot/level-scene-unreadable`.
    #[serde(rename = "godot/level-scene-unreadable")]
    LevelSceneUnreadable,
    /// `godot/map-loading`.
    #[serde(rename = "godot/map-loading")]
    MapLoading,
    /// `godot/metadata-key`.
    #[serde(rename = "godot/metadata-key")]
    MetadataKey,
    /// `godot/navigation-layer-unset`.
    #[serde(rename = "godot/navigation-layer-unset")]
    NavigationLayerUnset,
    /// `godot/navigation-shape`.
    #[serde(rename = "godot/navigation-shape")]
    NavigationShape,
    /// `godot/navigation-split`.
    #[serde(rename = "godot/navigation-split")]
    NavigationSplit,
    /// `godot/node-type`.
    #[serde(rename = "godot/node-type")]
    NodeType,
    /// `godot/occlusion-layer-unset`.
    #[serde(rename = "godot/occlusion-layer-unset")]
    OcclusionLayerUnset,
    /// `godot/physics-layer-unset`.
    #[serde(rename = "godot/physics-layer-unset")]
    PhysicsLayerUnset,
    /// `godot/physics-quadrant-size`.
    #[serde(rename = "godot/physics-quadrant-size")]
    PhysicsQuadrantSize,
    /// `godot/property-type`.
    #[serde(rename = "godot/property-type")]
    PropertyType,
    /// `godot/reserved-name`.
    #[serde(rename = "godot/reserved-name")]
    ReservedName,
    /// `godot/respath-missing`.
    #[serde(rename = "godot/respath-missing")]
    RespathMissing,
    /// `godot/respath-rotated`.
    #[serde(rename = "godot/respath-rotated")]
    RespathRotated,
    /// `godot/rule-tiles`.
    #[serde(rename = "godot/rule-tiles")]
    RuleTiles,
    /// `godot/setting-type`.
    #[serde(rename = "godot/setting-type")]
    SettingType,
    /// `godot/shared-source-kept`.
    #[serde(rename = "godot/shared-source-kept")]
    SharedSourceKept,
    /// `godot/shared-tileset-missing`.
    #[serde(rename = "godot/shared-tileset-missing")]
    SharedTilesetMissing,
    /// `godot/tall-tiles-no-ysort`.
    #[serde(rename = "godot/tall-tiles-no-ysort")]
    TallTilesNoYsort,
    /// `godot/tile-collision-kind`.
    #[serde(rename = "godot/tile-collision-kind")]
    TileCollisionKind,
    /// `godot/tile-image-missing`.
    #[serde(rename = "godot/tile-image-missing")]
    TileImageMissing,
    /// `godot/tile-render-size`.
    #[serde(rename = "godot/tile-render-size")]
    TileRenderSize,
    /// `godot/tileset-missing`.
    #[serde(rename = "godot/tileset-missing")]
    TilesetMissing,
    /// `godot/uid-duplicated`.
    #[serde(rename = "godot/uid-duplicated")]
    UidDuplicated,
    /// `godot/wang-flipped`.
    #[serde(rename = "godot/wang-flipped")]
    WangFlipped,
    /// `godot/wang-hex-corners`.
    #[serde(rename = "godot/wang-hex-corners")]
    WangHexCorners,
    /// `godot/wang-hex-sides`.
    #[serde(rename = "godot/wang-hex-sides")]
    WangHexSides,
    /// `godot/wang-several-sets`.
    #[serde(rename = "godot/wang-several-sets")]
    WangSeveralSets,
    /// `godot/ysort-false`.
    #[serde(rename = "godot/ysort-false")]
    YsortFalse,
    /// `godot/ysort-top-down`.
    #[serde(rename = "godot/ysort-top-down")]
    YsortTopDown,
    /// `godot/zindex-clamped`.
    #[serde(rename = "godot/zindex-clamped")]
    ZindexClamped,
}
impl Code {
    /// Every diagnostic code, in textual order.
    pub const ALL: &[Code] = &[
        Code::AnimationLayout,
        Code::BlendMode,
        Code::BodyNoShape,
        Code::CopyChanged,
        Code::CopyKept,
        Code::CustomDataClass,
        Code::CustomDataFile,
        Code::CustomDataType,
        Code::DiffersFromTiled,
        Code::ExportUndeclared,
        Code::GridShift,
        Code::HalfPixelOrigin,
        Code::HexFallback,
        Code::HexRotation,
        Code::ImageNotCopied,
        Code::ImageSources,
        Code::LayerIdsUnsaved,
        Code::LeftOut,
        Code::LevelSceneAutomap,
        Code::LevelSceneConnection,
        Code::LevelSceneDropped,
        Code::LevelSceneFollows,
        Code::LevelSceneLost,
        Code::LevelSceneMovedToRoot,
        Code::LevelSceneRenamed,
        Code::LevelSceneRetargeted,
        Code::LevelSceneTiles,
        Code::LevelSceneType,
        Code::LevelSceneUnreadable,
        Code::MapLoading,
        Code::MetadataKey,
        Code::NavigationLayerUnset,
        Code::NavigationShape,
        Code::NavigationSplit,
        Code::NodeType,
        Code::OcclusionLayerUnset,
        Code::PhysicsLayerUnset,
        Code::PhysicsQuadrantSize,
        Code::PropertyType,
        Code::ReservedName,
        Code::RespathMissing,
        Code::RespathRotated,
        Code::RuleTiles,
        Code::SettingType,
        Code::SharedSourceKept,
        Code::SharedTilesetMissing,
        Code::TallTilesNoYsort,
        Code::TileCollisionKind,
        Code::TileImageMissing,
        Code::TileRenderSize,
        Code::TilesetMissing,
        Code::UidDuplicated,
        Code::WangFlipped,
        Code::WangHexCorners,
        Code::WangHexSides,
        Code::WangSeveralSets,
        Code::YsortFalse,
        Code::YsortTopDown,
        Code::ZindexClamped,
    ];
    /// The stable name used by reports and tools.
    pub const fn as_str(self) -> &'static str {
        match self {
            Code::AnimationLayout => "godot/animation-layout",
            Code::BlendMode => "godot/blend-mode",
            Code::BodyNoShape => "godot/body-no-shape",
            Code::CopyChanged => "godot/copy-changed",
            Code::CopyKept => "godot/copy-kept",
            Code::CustomDataClass => "godot/custom-data-class",
            Code::CustomDataFile => "godot/custom-data-file",
            Code::CustomDataType => "godot/custom-data-type",
            Code::DiffersFromTiled => "godot/differs-from-tiled",
            Code::ExportUndeclared => "godot/export-undeclared",
            Code::GridShift => "godot/grid-shift",
            Code::HalfPixelOrigin => "godot/half-pixel-origin",
            Code::HexFallback => "godot/hex-fallback",
            Code::HexRotation => "godot/hex-rotation",
            Code::ImageNotCopied => "godot/image-not-copied",
            Code::ImageSources => "godot/image-sources",
            Code::LayerIdsUnsaved => "godot/layer-ids-unsaved",
            Code::LeftOut => "godot/left-out",
            Code::LevelSceneAutomap => "godot/level-scene-automap",
            Code::LevelSceneConnection => "godot/level-scene-connection",
            Code::LevelSceneDropped => "godot/level-scene-dropped",
            Code::LevelSceneFollows => "godot/level-scene-follows",
            Code::LevelSceneLost => "godot/level-scene-lost",
            Code::LevelSceneMovedToRoot => "godot/level-scene-moved-to-root",
            Code::LevelSceneRenamed => "godot/level-scene-renamed",
            Code::LevelSceneRetargeted => "godot/level-scene-retargeted",
            Code::LevelSceneTiles => "godot/level-scene-tiles",
            Code::LevelSceneType => "godot/level-scene-type",
            Code::LevelSceneUnreadable => "godot/level-scene-unreadable",
            Code::MapLoading => "godot/map-loading",
            Code::MetadataKey => "godot/metadata-key",
            Code::NavigationLayerUnset => "godot/navigation-layer-unset",
            Code::NavigationShape => "godot/navigation-shape",
            Code::NavigationSplit => "godot/navigation-split",
            Code::NodeType => "godot/node-type",
            Code::OcclusionLayerUnset => "godot/occlusion-layer-unset",
            Code::PhysicsLayerUnset => "godot/physics-layer-unset",
            Code::PhysicsQuadrantSize => "godot/physics-quadrant-size",
            Code::PropertyType => "godot/property-type",
            Code::ReservedName => "godot/reserved-name",
            Code::RespathMissing => "godot/respath-missing",
            Code::RespathRotated => "godot/respath-rotated",
            Code::RuleTiles => "godot/rule-tiles",
            Code::SettingType => "godot/setting-type",
            Code::SharedSourceKept => "godot/shared-source-kept",
            Code::SharedTilesetMissing => "godot/shared-tileset-missing",
            Code::TallTilesNoYsort => "godot/tall-tiles-no-ysort",
            Code::TileCollisionKind => "godot/tile-collision-kind",
            Code::TileImageMissing => "godot/tile-image-missing",
            Code::TileRenderSize => "godot/tile-render-size",
            Code::TilesetMissing => "godot/tileset-missing",
            Code::UidDuplicated => "godot/uid-duplicated",
            Code::WangFlipped => "godot/wang-flipped",
            Code::WangHexCorners => "godot/wang-hex-corners",
            Code::WangHexSides => "godot/wang-hex-sides",
            Code::WangSeveralSets => "godot/wang-several-sets",
            Code::YsortFalse => "godot/ysort-false",
            Code::YsortTopDown => "godot/ysort-top-down",
            Code::ZindexClamped => "godot/zindex-clamped",
        }
    }
}
impl std::fmt::Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl PartialEq<&str> for Code {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_diagnostic_enum_json_and_documented_codes_agree() {
        let doc = include_str!("../../../docs/design/12-godot-export.md");
        let documented: std::collections::BTreeSet<&str> = doc
            .split('`')
            .filter(|part| part.starts_with("godot/") && !part.ends_with('*'))
            .collect();
        let actual: std::collections::BTreeSet<&str> =
            super::Code::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(actual, documented);
        for code in super::Code::ALL {
            assert_eq!(serde_json::to_value(code).unwrap(), code.as_str());
        }
    }
}
