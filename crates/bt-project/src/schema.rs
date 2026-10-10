//! The file's document: its format version, the entry of each map it covers, and the keys this
//! version does not know, kept at every level and written back unchanged.
//!
//! The file is JSON, written pretty with a two-space indent and a final newline. Every object's
//! keys are written sorted (rows by number, every other key by its text), whichever version of
//! better-tiled knows them, so the same content always gives the same bytes and a diff in
//! version control shows only what changed.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;

use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

/// The format version this program writes. It changes only when a key's meaning changes:
/// adding a key never bumps it, since unknown keys are kept and defaults are explicit.
pub(crate) const FORMAT_VERSION: u32 = 1;

/// The keys of an object that this version does not know, with their values as read.
pub(crate) type UnknownKeys = BTreeMap<String, Value>;

/// The contents of a `better-tiled.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    /// The format version the file was last written with; 0 when it names none.
    #[serde(default)]
    better_tiled: u32,
    /// What the file says about each map, by key (`crate::paths::key`).
    #[serde(default)]
    maps: BTreeMap<String, MapEntry>,
    /// Keys this version does not know, kept as they were.
    #[serde(flatten)]
    unknown: UnknownKeys,
}

/// What a project file says about one map.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapEntry {
    /// Keys this version does not know: records a newer version added, kept as they were.
    #[serde(flatten)]
    unknown: UnknownKeys,
}

/// What reading a project file noticed without failing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Note {
    /// The file was written by a newer better-tiled, in a format version this one does not
    /// know: what this version understands was read, and the rest is kept and written back as
    /// it was.
    NewerVersion {
        /// The file's format version.
        version: u32,
    },
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Note::NewerVersion { version } => write!(
                f,
                "written by a newer better-tiled (format {version}; this one knows format \
                 {FORMAT_VERSION}): what it understands was read, and the rest is kept as it is"
            ),
        }
    }
}

impl Document {
    /// The document of `text`, the contents of a project file: a key this version does not know
    /// is kept, and a known key with a value of the wrong kind is an error. Files are read with
    /// `crate::ProjectFile::load`, which names the file in its errors.
    pub fn from_json(text: &str) -> Result<Document, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// The text of the file: pretty JSON with a two-space indent, every object's keys sorted, a
    /// final newline, and `betterTiled` the greater of the file's version and this program's.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        let mut value = serde_json::to_value(self)?;
        if let Value::Object(object) = &mut value {
            object.insert(
                "betterTiled".to_owned(),
                Value::from(self.better_tiled.max(FORMAT_VERSION)),
            );
        }
        let mut text = serde_json::to_string_pretty(&Sorted(&value))?;
        text.push('\n');
        Ok(text)
    }

    /// The format version the file was last written with; 0 for a file that names none or
    /// does not exist yet.
    pub fn version(&self) -> u32 {
        self.better_tiled
    }

    /// The note a reader gives once about this document: that it was written by a newer
    /// version, when it was.
    pub fn version_note(&self) -> Option<Note> {
        (self.better_tiled > FORMAT_VERSION).then_some(Note::NewerVersion {
            version: self.better_tiled,
        })
    }

    /// The entry of the map with `key`.
    pub fn entry(&self, key: &str) -> Option<&MapEntry> {
        self.maps.get(key)
    }

    /// The keys of the maps the file has entries for, sorted.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.maps.keys().map(String::as_str)
    }
}

impl MapEntry {
    /// Whether the entry holds nothing, not even a record this version does not know; such an
    /// entry is removed from the file.
    pub fn is_empty(&self) -> bool {
        self.unknown.is_empty()
    }
}

/// A JSON value written with every object's keys in [`key_order`].
struct Sorted<'a>(&'a Value);

impl Serialize for Sorted<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Value::Object(object) => {
                let mut entries: Vec<(&String, &Value)> = object.iter().collect();
                entries.sort_by(|a, b| key_order(a.0, b.0));
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    map.serialize_entry(key, &Sorted(value))?;
                }
                map.end()
            }
            Value::Array(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(&Sorted(item))?;
                }
                seq.end()
            }
            other => other.serialize(serializer),
        }
    }
}

/// The order of an object's keys in the file: keys that are whole numbers (painted rows) by
/// number and before the others, the others by their text; equal numbers by their text.
fn key_order(a: &str, b: &str) -> Ordering {
    match (a.parse::<i64>(), b.parse::<i64>()) {
        (Ok(x), Ok(y)) => x.cmp(&y).then_with(|| a.cmp(b)),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => a.cmp(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY: &str = "{\n  \"betterTiled\": 1,\n  \"maps\": {}\n}\n";

    #[test]
    fn the_empty_document_is_written_with_its_version_and_no_entries() {
        assert_eq!(Document::default().to_json().unwrap(), EMPTY);
        let read = Document::from_json("{}").unwrap();
        assert_eq!((read.version(), read.keys().count()), (0, 0));
        assert_eq!(read.to_json().unwrap(), EMPTY);
        assert_eq!(Document::from_json(EMPTY).unwrap().version(), 1);
    }

    #[test]
    fn unknown_keys_are_kept_at_every_level_and_written_back_sorted() {
        let text = r#"{"zeta": {"b": [1, {"y": 2, "x": 1}], "a": null}, "maps": {
            "levels/b.tmx": {"zones": {"count": 3}},
            "a.tmx": {"later": true, "automapNext": "kept"}
        }, "betterTiled": 1, "alpha": 1.5}"#;
        let document = Document::from_json(text).unwrap();
        assert_eq!(
            document.keys().collect::<Vec<_>>(),
            ["a.tmx", "levels/b.tmx"]
        );
        let written = document.to_json().unwrap();
        assert_eq!(
            written,
            r#"{
  "alpha": 1.5,
  "betterTiled": 1,
  "maps": {
    "a.tmx": {
      "automapNext": "kept",
      "later": true
    },
    "levels/b.tmx": {
      "zones": {
        "count": 3
      }
    }
  },
  "zeta": {
    "a": null,
    "b": [
      1,
      {
        "x": 1,
        "y": 2
      }
    ]
  }
}
"#
        );
        let again = Document::from_json(&written).unwrap();
        assert_eq!(again, document);
        assert_eq!(again.to_json().unwrap(), written, "byte-stable");
    }

    #[test]
    fn rows_and_other_numbered_keys_are_written_in_number_order() {
        let mut keys = ["10", "x", "-2", "3", "+3", "b", "-10"];
        keys.sort_by(|a, b| key_order(a, b));
        assert_eq!(keys, ["-10", "-2", "+3", "3", "10", "b", "x"]);
        let document = Document::from_json(r#"{"rows": {"10": "a", "9": "b", "-1": "c"}}"#);
        let written = document.unwrap().to_json().unwrap();
        let rows = written.find("\"rows\"").unwrap();
        let order: Vec<usize> = ["\"-1\"", "\"9\"", "\"10\""]
            .iter()
            .map(|key| written[rows..].find(key).unwrap())
            .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{written}");
    }

    #[test]
    fn a_newer_version_is_read_as_far_as_understood_kept_and_noted_once() {
        let document =
            Document::from_json(r#"{"betterTiled": 3, "maps": {"a.tmx": {"new": 1}}}"#).unwrap();
        assert_eq!(document.version(), 3);
        assert_eq!(
            document.version_note(),
            Some(Note::NewerVersion { version: 3 })
        );
        assert!(
            document
                .version_note()
                .unwrap()
                .to_string()
                .starts_with("written by a newer better-tiled (format 3; this one knows format 1)")
        );
        assert!(document.to_json().unwrap().contains("\"betterTiled\": 3"));
        assert_eq!(Document::default().version_note(), None);
        assert_eq!(
            Document::from_json(r#"{"betterTiled": 1}"#)
                .unwrap()
                .version_note(),
            None
        );
    }

    #[test]
    fn known_keys_of_the_wrong_kind_are_errors() {
        for text in [
            "",
            "[]",
            r#"{"betterTiled": "1"}"#,
            r#"{"betterTiled": -1}"#,
            r#"{"maps": []}"#,
            r#"{"maps": {"a.tmx": 3}}"#,
            r#"{"maps": {} "#,
        ] {
            assert!(Document::from_json(text).is_err(), "{text}");
        }
    }

    #[test]
    fn an_entry_with_nothing_in_it_is_empty() {
        let document =
            Document::from_json(r#"{"maps": {"a.tmx": {}, "b.tmx": {"new": 1}}}"#).unwrap();
        assert!(document.entry("a.tmx").unwrap().is_empty());
        assert!(!document.entry("b.tmx").unwrap().is_empty());
        assert!(document.entry("c.tmx").is_none());
    }
}
