//! The AutoMap record of a map entry: the rules, their runs with the map layers bound to the
//! names the rules use, and what was painted on the layers the rules write. The shapes are fixed
//! here; what they mean is `bt-automap`'s, which produces the record when a map is saved and
//! rebuilds the painted map from it when the map is opened (`08-automap.md`, §10).
//!
//! Every record keeps the keys this version does not know in `unknown`, written back as they
//! were. A record that replaces another keeps the replaced record's unknown keys it does not set
//! itself (`crate::Document::set_automap`); the unknown keys of nested records (runs, bindings,
//! painted entries) are the caller's to carry over, since only the caller knows which new run or
//! entry stands for which old one.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A map's AutoMap layers: which rules run on which of its layers, with which seed, and what
/// was painted on the layers the rules write, so the painted map can be rebuilt from the map as
/// saved.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoMapRecord {
    /// The rules file or rule map, relative to the project file's folder (`crate::paths::key`).
    pub rules: String,
    /// The seed of the rules' random choices.
    pub seed: u64,
    /// The runs of the rules, in order.
    pub runs: Vec<RunRecord>,
    /// One entry per written layer whose painted cells (or objects) differ from the map as
    /// saved.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub painted: Vec<PaintedRecord>,
    /// The painted map's next object id, when the objects the rules placed took ids after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_object_id: Option<u32>,
    /// Keys this version does not know, kept as they were.
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

/// One run of the rules: the map layers bound to the names the rules use.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    /// The bindings, one per name the run binds.
    pub layers: Vec<BindingRecord>,
    /// Keys this version does not know, kept as they were.
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

/// A map layer bound to a name the rules use.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingRecord {
    /// The name the rules use: `walls` for `input_walls` and `output_walls`.
    pub rules: String,
    /// Whether the binding is the rules' object output of that name rather than their tile
    /// layer.
    #[serde(default, skip_serializing_if = "is_false")]
    pub objects: bool,
    /// The map layer's id.
    pub layer: u32,
    /// The map layer's name when saved, for people reading the file.
    pub name: String,
    /// Keys this version does not know, kept as they were.
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

/// What was painted on one layer the rules write, where it differs from the map as saved.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaintedRecord {
    /// The layer's id.
    pub layer: u32,
    /// The layer's name when saved, for people reading the file.
    pub name: String,
    /// The checksum of the layer as saved in the map (`crate::checksum`): when the map's layer
    /// no longer has it (the map was changed elsewhere, in Tiled say), the entry does not apply.
    pub checksum: String,
    /// Whether the layer is an object layer, whose `generated` lists the objects the rules
    /// placed; otherwise a tile layer, whose `rows` hold the painted cells.
    #[serde(default, skip_serializing_if = "is_false")]
    pub objects: bool,
    /// Tile layers: the tilesets `rows` refer to by index, as tileset keys
    /// (`crate::paths::tileset_key`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tilesets: Vec<String>,
    /// Tile layers: the column the rows start at.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub x: i32,
    /// Tile layers: the painted cells of each row that has any, by row number, in the text of
    /// `crate::rows`. No rows: nothing was painted.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub rows: BTreeMap<i32, String>,
    /// Object layers: the ids of the objects the rules placed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generated: Vec<u32>,
    /// Keys this version does not know, kept as they were.
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_zero(value: &i32) -> bool {
    *value == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECORD: &str = r##"{
  "nextObjectId": 40,
  "painted": [
    {
      "checksum": "00ff00ff00ff00ff",
      "layer": 4,
      "name": "plat1",
      "rows": {
        "-2": "0:5*3",
        "3": ". 0:1/hv 1:7",
        "10": ".*4 0:2"
      },
      "tilesets": [
        "../tiles/ground.tsx",
        "#Decor"
      ],
      "x": -3
    },
    {
      "checksum": "0123456789abcdef",
      "generated": [
        12,
        13
      ],
      "layer": 6,
      "name": "spawns",
      "objects": true
    }
  ],
  "rules": "rules/rules.txt",
  "runs": [
    {
      "layers": [
        {
          "layer": 4,
          "name": "plat1",
          "rules": "walls"
        },
        {
          "layer": 6,
          "name": "spawns",
          "objects": true,
          "rules": "spawns"
        }
      ]
    }
  ],
  "seed": 7
}"##;

    #[test]
    fn a_record_reads_into_its_fields_and_writes_back_as_read() {
        let record: AutoMapRecord = serde_json::from_str(RECORD).unwrap();
        assert_eq!(
            (record.rules.as_str(), record.seed, record.next_object_id),
            ("rules/rules.txt", 7, Some(40))
        );
        let walls = &record.runs[0].layers[0];
        assert_eq!(
            (
                walls.rules.as_str(),
                walls.objects,
                walls.layer,
                walls.name.as_str()
            ),
            ("walls", false, 4, "plat1")
        );
        assert!(record.runs[0].layers[1].objects);
        let tiles = &record.painted[0];
        assert_eq!(
            tiles.rows.keys().copied().collect::<Vec<_>>(),
            [-2, 3, 10],
            "rows by number"
        );
        assert_eq!(
            (tiles.x, tiles.objects, tiles.generated.len()),
            (-3, false, 0)
        );
        assert_eq!(tiles.tilesets, ["../tiles/ground.tsx", "#Decor"]);
        let objects = &record.painted[1];
        assert_eq!(
            (objects.objects, objects.generated.as_slice(), objects.x),
            (true, [12, 13].as_slice(), 0)
        );
        assert!(objects.rows.is_empty() && objects.tilesets.is_empty());

        // In a file, the record is written back as it was read, six spaces in.
        let file = format!(r#"{{"maps": {{"a.tmx": {{"automap": {RECORD}}}}}}}"#);
        let written = crate::Document::from_json(&file)
            .unwrap()
            .to_json()
            .unwrap();
        let indented: Vec<String> = RECORD.lines().map(|line| format!("      {line}")).collect();
        let indented = indented.join("\n");
        assert!(
            written.contains(indented.trim_start()),
            "written back as read:\n{written}"
        );
    }

    #[test]
    fn unknown_keys_are_kept_in_every_record() {
        let text = r#"{"rules": "r.txt", "seed": 1, "later": {"a": 1},
            "runs": [{"layers": [{"rules": "w", "layer": 1, "name": "g", "weight": 2}], "mode": "x"}],
            "painted": [{"layer": 1, "name": "g", "checksum": "c", "regions": [1]}]}"#;
        let record: AutoMapRecord = serde_json::from_str(text).unwrap();
        assert_eq!(record.unknown["later"], serde_json::json!({"a": 1}));
        assert_eq!(record.runs[0].unknown["mode"], "x");
        assert_eq!(record.runs[0].layers[0].unknown["weight"], 2);
        assert_eq!(record.painted[0].unknown["regions"], serde_json::json!([1]));
        let again: AutoMapRecord =
            serde_json::from_str(&serde_json::to_string(&record).unwrap()).unwrap();
        assert_eq!(again, record);
    }

    #[test]
    fn missing_keys_without_defaults_are_errors() {
        for text in [
            r#"{"seed": 1, "runs": []}"#,
            r#"{"rules": "r", "runs": []}"#,
            r#"{"rules": "r", "seed": 1}"#,
            r#"{"rules": "r", "seed": -1, "runs": []}"#,
            r#"{"rules": "r", "seed": 1, "runs": [{"layers": [{"rules": "w", "name": "g"}]}]}"#,
            r#"{"rules": "r", "seed": 1, "runs": [], "painted": [{"layer": 1, "name": "g"}]}"#,
            r#"{"rules": "r", "seed": 1, "runs": [], "painted": [{"layer": 1, "name": "g", "checksum": "c", "rows": {"x": ""}}]}"#,
        ] {
            assert!(
                serde_json::from_str::<AutoMapRecord>(text).is_err(),
                "{text}"
            );
        }
    }
}
