//! The Godot setup: how the maps a project file covers go into a Godot project, as the editor's
//! setup sheet and `bt godot setup` write it. The keys are fixed here; what they mean is
//! `bt-godot`'s (`12-godot-export.md`).
//!
//! Every key is optional: what is not given is Godot's default or the exporter's, and is written
//! back as not given. Paths outside the Godot project are relative to the project file's folder
//! (`crate::paths::key`); the others are `res://` paths. Every object keeps the keys this version
//! does not know in `unknown`, written back as they were.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Godot's default for each layer mask of a TileSet's layers: the first layer only.
const FIRST_LAYER: u32 = 1;

/// The `godot` section of a project file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GodotSetup {
    /// The Godot project's folder, holding `project.godot`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Where scenes go (`res://levels/`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub levels: Option<String>,
    /// `shared` (one `.tres` per grid) or `embedded`: a string, so that a mode a newer version
    /// adds does not make the file unreadable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tile_sets: Option<String>,
    /// Where shared TileSets go (`res://tilesets/`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tile_set_folder: Option<String>,
    /// Where copies of images from outside the Godot project go (`res://images/`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<String>,
    /// Whether a first export also writes a starter scene that inherits the generated one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level_scene: Option<bool>,
    /// Whether saving a map that has been exported exports it again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_on_save: Option<bool>,
    /// The TileSet's physics layers, by `godotPhysicsLayer` number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physics_layers: Option<Vec<PhysicsLayerSetup>>,
    /// The TileSet's navigation layers, by `godotNavigationLayer` number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub navigation_layers: Option<Vec<NavigationLayerSetup>>,
    /// The TileSet's occlusion layers, by `godotOcclusionLayer` number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occlusion_layers: Option<Vec<OcclusionLayerSetup>>,
    /// Keys this version does not know, kept as they were.
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

/// A physics layer of the TileSet: its `collision_layer` and `collision_mask` bits.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicsLayerSetup {
    /// The layers the layer's shapes are in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collision_layer: Option<u32>,
    /// The layers the layer's shapes collide with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collision_mask: Option<u32>,
    /// Keys this version does not know, kept as they were.
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

/// A navigation layer of the TileSet: its `layers` bits.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigationLayerSetup {
    /// The navigation layers the layer's polygons are in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layers: Option<u32>,
    /// Keys this version does not know, kept as they were.
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

/// An occlusion layer of the TileSet: its `light_mask` bits.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcclusionLayerSetup {
    /// The lights the layer's occluders cast shadows for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light_mask: Option<u32>,
    /// Keys this version does not know, kept as they were.
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

impl PhysicsLayerSetup {
    /// The collision layer bits as given, else Godot's default, 1 (the first layer).
    pub fn collision_layer_or_default(&self) -> u32 {
        self.collision_layer.unwrap_or(FIRST_LAYER)
    }

    /// The collision mask bits as given, else Godot's default, 1 (the first layer).
    pub fn collision_mask_or_default(&self) -> u32 {
        self.collision_mask.unwrap_or(FIRST_LAYER)
    }
}

impl NavigationLayerSetup {
    /// The navigation layer bits as given, else Godot's default, 1 (the first layer).
    pub fn layers_or_default(&self) -> u32 {
        self.layers.unwrap_or(FIRST_LAYER)
    }
}

impl OcclusionLayerSetup {
    /// The light mask bits as given, else Godot's default, 1 (the first layer).
    pub fn light_mask_or_default(&self) -> u32 {
        self.light_mask.unwrap_or(FIRST_LAYER)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Document;

    const SECTION: &str = r#"{
    "exportOnSave": true,
    "images": "res://images/",
    "levelScene": false,
    "levels": "res://levels/",
    "navigationLayers": [
      {
        "layers": 3
      }
    ],
    "occlusionLayers": [
      {
        "lightMask": 2
      },
      {}
    ],
    "physicsLayers": [
      {
        "collisionLayer": 1,
        "collisionMask": 3
      },
      {
        "collisionLayer": 4,
        "later": {
          "oneWay": true
        }
      }
    ],
    "project": "../game",
    "tileSetFolder": "res://tilesets/",
    "tileSets": "shared",
    "zones": "newer"
  }"#;

    fn file_with(section: &str) -> String {
        format!("{{\n  \"betterTiled\": 1,\n  \"godot\": {section},\n  \"maps\": {{}}\n}}\n")
    }

    #[test]
    fn the_section_reads_into_its_fields_with_defaults() {
        let document = Document::from_json(&file_with(SECTION)).unwrap();
        let godot = document.godot().unwrap();
        assert_eq!(godot.project.as_deref(), Some("../game"));
        assert_eq!(godot.levels.as_deref(), Some("res://levels/"));
        assert_eq!(godot.tile_sets.as_deref(), Some("shared"));
        assert_eq!(godot.tile_set_folder.as_deref(), Some("res://tilesets/"));
        assert_eq!(godot.images.as_deref(), Some("res://images/"));
        assert_eq!(
            (godot.level_scene, godot.export_on_save),
            (Some(false), Some(true))
        );
        let physics = godot.physics_layers.as_deref().unwrap();
        let masks: Vec<[u32; 2]> = physics
            .iter()
            .map(|l| {
                [
                    l.collision_layer_or_default(),
                    l.collision_mask_or_default(),
                ]
            })
            .collect();
        assert_eq!(
            masks,
            [[1, 3], [4, 1]],
            "a mask not given is Godot's default"
        );
        assert_eq!(physics[1].collision_mask, None, "and stays not given");
        let navigation = godot.navigation_layers.as_deref().unwrap();
        assert_eq!(navigation[0].layers_or_default(), 3);
        let occlusion = godot.occlusion_layers.as_deref().unwrap();
        assert_eq!(
            occlusion
                .iter()
                .map(OcclusionLayerSetup::light_mask_or_default)
                .collect::<Vec<_>>(),
            [2, 1]
        );
        assert_eq!(NavigationLayerSetup::default().layers_or_default(), 1);
    }

    #[test]
    fn the_section_is_written_back_as_read_with_unknown_keys_in_nested_entries() {
        let text = file_with(SECTION);
        let document = Document::from_json(&text).unwrap();
        let godot = document.godot().unwrap();
        assert_eq!(godot.unknown["zones"], "newer");
        let physics = godot.physics_layers.as_deref().unwrap();
        assert_eq!(
            physics[1].unknown["later"],
            serde_json::json!({"oneWay": true})
        );
        assert_eq!(document.to_json().unwrap(), text);
    }

    #[test]
    fn an_empty_section_is_kept_and_a_missing_one_is_not_written() {
        let empty = file_with("{}");
        let document = Document::from_json(&empty).unwrap();
        assert_eq!(document.godot(), Some(&GodotSetup::default()));
        assert_eq!(document.to_json().unwrap(), empty);
        let mut none = Document::default();
        assert!(!none.to_json().unwrap().contains("godot"));
        none.set_godot(GodotSetup::default());
        assert_eq!(none.to_json().unwrap(), empty);
        let empty_lists = file_with("{\n    \"physicsLayers\": []\n  }");
        assert_eq!(
            Document::from_json(&empty_lists)
                .unwrap()
                .to_json()
                .unwrap(),
            empty_lists,
            "an empty list is not a missing one"
        );
    }

    #[test]
    fn setting_the_section_keeps_what_a_newer_version_added_and_removing_it_drops_it() {
        let mut document = Document::from_json(&file_with(SECTION)).unwrap();
        document.set_godot(GodotSetup {
            levels: Some("res://scenes/".to_owned()),
            ..GodotSetup::default()
        });
        let godot = document.godot().unwrap();
        assert_eq!(godot.levels.as_deref(), Some("res://scenes/"));
        assert_eq!(godot.project, None, "replaced, not merged");
        assert_eq!(
            godot.unknown["zones"], "newer",
            "a newer version's key stays"
        );
        assert!(document.remove_godot().is_some());
        assert_eq!(document.godot(), None);
        assert!(!document.to_json().unwrap().contains("godot"));
    }

    #[test]
    fn values_of_the_wrong_kind_are_errors() {
        for section in [
            r#"{"levels": 3}"#,
            r#"{"levelScene": "yes"}"#,
            r#"{"physicsLayers": {}}"#,
            r#"{"physicsLayers": [{"collisionMask": -1}]}"#,
            r#"{"occlusionLayers": [3]}"#,
        ] {
            assert!(
                Document::from_json(&file_with(section)).is_err(),
                "{section}"
            );
        }
    }
}
