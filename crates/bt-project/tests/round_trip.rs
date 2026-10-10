//! Reading and writing project files (`09-project-file.md`, §5, §6, §7 and §9): unknown keys
//! survive at every level, the output is sorted and byte-stable, an emptied entry disappears, a
//! missing file reads as empty, an unreadable one is an error naming it, and a write that would
//! change nothing does not touch the file. `BT_FUZZ_SEED` and `BT_FUZZ_CASES` set the seed and
//! count of the randomised test.

#![cfg(test)]

mod common;

use std::fs;
use std::path::Path;

use bt_project::{AutoMapRecord, Document, Error, FILE_NAME, GodotSetup, Note, ProjectFile};
use bt_test_data::{Fuzz, Scratch};
use bt_utils::Refused;
use bt_utils::rng::Rng;
use serde_json::{Map, Value, json};

use common::{lock_folder, record, write};

const EMPTY: &str = "{\n  \"betterTiled\": 1,\n  \"maps\": {}\n}\n";

/// A file with keys this version does not know at every level, as this version writes it.
const UNKNOWN_EVERYWHERE: &str = r##"{
  "betterTiled": 1,
  "godot": {
    "levels": "res://levels/",
    "physicsLayers": [
      {
        "collisionLayer": 2,
        "uOneWay": true
      }
    ],
    "uExport": {
      "mode": "fast"
    }
  },
  "maps": {
    "levels/a.tmx": {
      "automap": {
        "painted": [
          {
            "checksum": "00000000000000ff",
            "layer": 3,
            "name": "walls",
            "rows": {
              "-1": "0:4*2",
              "2": ". 1:7/hd"
            },
            "tilesets": [
              "../tiles/walls.tsx",
              "#Decor"
            ],
            "uRegions": [
              1,
              2
            ]
          }
        ],
        "rules": "../rules/rules.txt",
        "runs": [
          {
            "layers": [
              {
                "layer": 3,
                "name": "walls",
                "rules": "walls",
                "uWeight": 0.5
              }
            ],
            "uMode": "all"
          }
        ],
        "seed": 42,
        "uLater": null
      },
      "uZones": [
        "north"
      ]
    }
  },
  "uTop": {
    "nested": {
      "deep": [
        true,
        -1.25,
        "é \"quoted\""
      ]
    }
  }
}
"##;

#[test]
fn unknown_keys_at_every_level_survive_a_change_and_a_save() {
    let scratch = Scratch::new("bt-project-unknown-survive").unwrap();
    let locks = lock_folder(&scratch);
    let file = scratch.join(FILE_NAME);
    write(&file, UNKNOWN_EVERYWHERE);
    let document = ProjectFile::load(&file).unwrap();
    assert_eq!(
        document.to_json().unwrap(),
        UNKNOWN_EVERYWHERE,
        "read as written"
    );

    ProjectFile::edit(&file, &locks, |document| {
        document.set_automap("levels/b.tmx", record("b.txt"));
    })
    .unwrap();
    let written = fs::read_to_string(&file).unwrap();
    let mut expected: Value = serde_json::from_str(UNKNOWN_EVERYWHERE).unwrap();
    expected["maps"]["levels/b.tmx"] =
        json!({"automap": {"rules": "b.txt", "seed": 7, "runs": []}});
    assert_eq!(serde_json::from_str::<Value>(&written).unwrap(), expected);
    let inserted = ",\n    \"levels/b.tmx\": {\n      \"automap\": {\n        \"rules\": \"b.txt\",\
                    \n        \"runs\": [],\n        \"seed\": 7\n      }\n    }";
    assert!(written.contains(inserted), "{written}");
    assert_eq!(
        written.replace(inserted, ""),
        UNKNOWN_EVERYWHERE,
        "everything else byte for byte"
    );
}

#[test]
fn output_is_sorted_and_byte_stable_and_an_unchanged_file_is_not_touched() {
    let scratch = Scratch::new("bt-project-byte-stable").unwrap();
    let locks = lock_folder(&scratch);
    let (first, second) = (scratch.join("first.json"), scratch.join("second.json"));
    let set = |path: &Path, key: &str| {
        ProjectFile::edit(path, &locks, |document| {
            document.set_automap(key, record(key))
        })
        .unwrap();
    };
    set(&first, "b.tmx");
    set(&first, "a.tmx");
    set(&second, "a.tmx");
    set(&second, "b.tmx");
    let text = fs::read_to_string(&first).unwrap();
    assert_eq!(
        text,
        fs::read_to_string(&second).unwrap(),
        "the same content, the same bytes"
    );
    assert!(text.find("\"a.tmx\"").unwrap() < text.find("\"b.tmx\"").unwrap());
    assert!(text.ends_with("}\n") && text.contains("\n  \"maps\": {\n    \"a.tmx\""));

    let modified = fs::metadata(&first).unwrap().modified().unwrap();
    set(&first, "a.tmx");
    assert_eq!(fs::read_to_string(&first).unwrap(), text);
    assert_eq!(
        fs::metadata(&first).unwrap().modified().unwrap(),
        modified,
        "a write that would change nothing does not touch the file"
    );

    // A file formatted by hand is kept as it is until something in it changes.
    let by_hand =
        r#"{"maps":{"a.tmx":{"automap":{"seed":7,"runs":[],"rules":"a.tmx"}}},"betterTiled":1}"#;
    fs::write(&first, by_hand).unwrap();
    set(&first, "a.tmx");
    assert_eq!(fs::read_to_string(&first).unwrap(), by_hand);
    set(&first, "c.tmx");
    assert!(
        fs::read_to_string(&first)
            .unwrap()
            .starts_with("{\n  \"betterTiled\": 1,\n")
    );
}

#[test]
fn an_emptied_entry_disappears_and_an_emptied_file_is_the_empty_document() {
    let scratch = Scratch::new("bt-project-emptied").unwrap();
    let locks = lock_folder(&scratch);
    let file = scratch.join(FILE_NAME);
    for key in ["a.tmx", "b.tmx"] {
        ProjectFile::edit(&file, &locks, |document| {
            document.set_automap(key, record(key))
        })
        .unwrap();
    }
    let removed =
        ProjectFile::edit(&file, &locks, |document| document.remove_automap("a.tmx")).unwrap();
    assert_eq!(removed.map(|record| record.rules), Some("a.tmx".to_owned()));
    let text = fs::read_to_string(&file).unwrap();
    assert!(!text.contains("a.tmx") && text.contains("b.tmx"), "{text}");
    ProjectFile::edit(&file, &locks, |document| document.remove_automap("b.tmx")).unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        EMPTY,
        "written as the empty document, not deleted"
    );
}

#[test]
fn a_missing_file_reads_as_empty_and_a_change_to_nothing_does_not_make_it() {
    let scratch = Scratch::new("bt-project-missing").unwrap();
    let locks = lock_folder(&scratch);
    let file = scratch.join(FILE_NAME);
    let document = ProjectFile::load(&file).unwrap();
    assert_eq!(document, Document::default());
    assert_eq!((document.version(), document.version_note()), (0, None));
    let removed =
        ProjectFile::edit(&file, &locks, |document| document.remove_automap("a.tmx")).unwrap();
    assert_eq!(removed, None);
    assert!(!file.exists(), "nothing changed, so nothing was made");
    ProjectFile::edit(&file, &locks, |document| {
        document.set_godot(GodotSetup::default())
    })
    .unwrap();
    assert!(fs::read_to_string(&file).unwrap().contains("\"godot\": {}"));
}

#[test]
fn an_unreadable_file_is_an_error_naming_it_and_is_never_written_over() {
    let scratch = Scratch::new("bt-project-unreadable").unwrap();
    let locks = lock_folder(&scratch);
    let file = scratch.join(FILE_NAME);
    for (text, at) in [
        ("{\"maps\": {", "line 1"),
        ("", "line 1"),
        (
            "{\n  \"maps\": {\n    \"a.tmx\": {\"automap\": {\"rules\": 3}}\n  }\n}",
            "line 3",
        ),
        ("{\"betterTiled\": \"one\"}", "line 1"),
    ] {
        fs::write(&file, text).unwrap();
        let error = ProjectFile::load(&file).unwrap_err();
        assert!(matches!(error, Error::Parse { .. }), "{error}");
        let message = error.to_string();
        assert!(
            message.contains(&file.display().to_string()) && message.contains(at),
            "{message}"
        );
        let mut changed = false;
        let edited = ProjectFile::edit(&file, &locks, |document| {
            changed = true;
            document.set_automap("a.tmx", record("a.txt"));
        });
        assert!(
            edited.is_err() && !changed,
            "{text}: stopped before the change"
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), text, "and left alone");
    }

    fs::remove_file(&file).unwrap();
    fs::create_dir(&file).unwrap();
    let error = ProjectFile::load(&file).unwrap_err();
    assert!(matches!(error, Error::Read { .. }), "{error}");
    assert!(error.to_string().contains(&file.display().to_string()));
}

#[test]
fn a_read_only_file_is_refused_and_left_as_it_was() {
    let scratch = Scratch::new("bt-project-read-only").unwrap();
    let locks = lock_folder(&scratch);
    let file = scratch.join(FILE_NAME);
    ProjectFile::edit(&file, &locks, |document| {
        document.set_automap("a.tmx", record("a"))
    })
    .unwrap();
    let before = fs::read_to_string(&file).unwrap();
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions.clone()).unwrap();
    let error = ProjectFile::edit(&file, &locks, |document| {
        document.set_automap("b.tmx", record("b"));
    })
    .unwrap_err();
    #[allow(
        clippy::permissions_set_readonly_false,
        reason = "letting the scratch folder be removed"
    )]
    permissions.set_readonly(false);
    fs::set_permissions(&file, permissions).unwrap();
    assert_eq!(error.refused(), Some(Refused::ReadOnlyFile), "{error}");
    assert!(error.to_string().contains(FILE_NAME), "{error}");
    assert_eq!(fs::read_to_string(&file).unwrap(), before);
}

#[test]
fn a_newer_version_is_kept_with_what_it_added() {
    let scratch = Scratch::new("bt-project-newer").unwrap();
    let locks = lock_folder(&scratch);
    let file = scratch.join(FILE_NAME);
    write(
        &file,
        r#"{"betterTiled": 2, "regions": {"a": 1}, "maps": {"a.tmx": {"automap": {"rules": "r", "seed": 1, "runs": [], "mode": "new"}}}}"#,
    );
    let document = ProjectFile::load(&file).unwrap();
    assert_eq!(
        document.version_note(),
        Some(Note::NewerVersion { version: 2 })
    );
    ProjectFile::edit(&file, &locks, |document| {
        let mut changed = document.automap("a.tmx").cloned().unwrap();
        changed.seed = 9;
        document.set_automap("a.tmx", changed);
    })
    .unwrap();
    let written: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(written["betterTiled"], 2, "the greater version");
    assert_eq!(written["regions"], json!({"a": 1}));
    assert_eq!(written["maps"]["a.tmx"]["automap"]["mode"], "new");
    assert_eq!(written["maps"]["a.tmx"]["automap"]["seed"], 9);
}

/// The keys a record knows at its top level: the model below keeps the rest of a replaced
/// record, as `Document::set_automap` and `Document::set_godot` do.
const AUTOMAP_KEYS: [&str; 5] = ["nextObjectId", "painted", "rules", "runs", "seed"];
const GODOT_KEYS: [&str; 10] = [
    "exportOnSave",
    "images",
    "levelScene",
    "levels",
    "navigationLayers",
    "occlusionLayers",
    "physicsLayers",
    "project",
    "tileSetFolder",
    "tileSets",
];
const MAP_KEYS: [&str; 5] = [
    "a.tmx",
    "levels/b.tmx",
    "levels/deep/c.tmx",
    "../up.tmx",
    "d e.tmx",
];

/// Random documents and records, with keys this version does not know at every level.
struct Generator {
    rng: Rng,
}

impl Generator {
    fn below(&mut self, n: usize) -> usize {
        self.rng.below(n as u64).unwrap_or(0) as usize
    }

    fn chance(&mut self) -> bool {
        self.below(2) == 0
    }

    fn pick<'a>(&mut self, from: &[&'a str]) -> &'a str {
        from[self.below(from.len())]
    }

    fn number(&mut self) -> u64 {
        self.below(100) as u64
    }

    /// A value of any kind, nested a little.
    fn value(&mut self, depth: u32) -> Value {
        match self.below(if depth > 1 { 6 } else { 8 }) {
            0 => Value::Null,
            1 => json!(self.chance()),
            2 => json!(self.number()),
            3 => json!(self.pick(&["", "text", "é \"quoted\"\nline"])),
            4 => [
                json!(-1.25),
                json!(0.5),
                json!(3.0),
                json!(u64::MAX),
                json!(i64::MIN),
            ][self.below(5)]
            .clone(),
            5 => json!(i64::MIN + self.number() as i64),
            6 => Value::Array((0..self.below(3)).map(|_| self.value(depth + 1)).collect()),
            _ => {
                let mut object = Map::new();
                self.add_unknown(&mut object, depth + 1);
                Value::Object(object)
            }
        }
    }

    /// Adds up to two keys no version of the format knows (they start with `u`).
    fn add_unknown(&mut self, object: &mut Map<String, Value>, depth: u32) {
        for _ in 0..self.below(3) {
            let name = format!("u{}", self.pick(&["Later", "Zones", "Mode", "Weight", "x"]));
            let value = self.value(depth);
            object.insert(name, value);
        }
    }

    fn with_unknown(&mut self, value: Value) -> Value {
        let Value::Object(mut object) = value else {
            panic!("not an object: {value}")
        };
        self.add_unknown(&mut object, 0);
        Value::Object(object)
    }

    fn binding(&mut self) -> Value {
        let mut binding = json!({"rules": self.pick(&["walls", "floor"]), "layer": self.number(), "name": "plat"});
        if self.chance() {
            binding["objects"] = json!(true);
        }
        self.with_unknown(binding)
    }

    fn painted(&mut self) -> Value {
        let mut painted =
            json!({"layer": self.number(), "name": "walls", "checksum": "0123456789abcdef"});
        if self.chance() {
            painted["objects"] = json!(true);
            painted["generated"] = json!([self.number(), self.number()]);
        } else {
            painted["tilesets"] = json!(["../tiles/a.tsx", "#Decor"]);
            painted["x"] = json!(self.below(5) as i64 - 2);
            painted["rows"] = json!({"-3": "0:1*2", "0": ". 1:7/hvdx", "12": ".*3 0:2"});
        }
        self.with_unknown(painted)
    }

    fn automap(&mut self) -> Value {
        let runs: Vec<Value> = (0..self.below(3))
            .map(|_| {
                let layers: Vec<Value> = (0..self.below(3)).map(|_| self.binding()).collect();
                self.with_unknown(json!({ "layers": layers }))
            })
            .collect();
        let mut record = json!({"rules": "../rules.txt", "seed": self.number(), "runs": runs});
        if self.chance() {
            let painted: Vec<Value> = (0..1 + self.below(2)).map(|_| self.painted()).collect();
            record["painted"] = Value::Array(painted);
        }
        if self.chance() {
            record["nextObjectId"] = json!(self.number());
        }
        self.with_unknown(record)
    }

    fn godot(&mut self) -> Value {
        let mut godot = json!({});
        if self.chance() {
            godot["levels"] = json!("res://levels/");
        }
        if self.chance() {
            godot["tileSets"] = json!(self.pick(&["shared", "embedded", "future"]));
        }
        if self.chance() {
            godot["exportOnSave"] = json!(self.chance());
        }
        if self.chance() {
            let layers: Vec<Value> = (0..self.below(3))
                .map(|_| {
                    let layer = json!({"collisionMask": self.number()});
                    self.with_unknown(layer)
                })
                .collect();
            godot["physicsLayers"] = Value::Array(layers);
        }
        if self.chance() {
            let layer = self.with_unknown(json!({}));
            godot["occlusionLayers"] = json!([layer]);
        }
        self.with_unknown(godot)
    }

    fn document(&mut self) -> Value {
        let mut maps = Map::new();
        for _ in 0..self.below(4) {
            let mut entry = Map::new();
            if self.below(4) > 0 {
                entry.insert("automap".to_owned(), self.automap());
            }
            self.add_unknown(&mut entry, 0);
            maps.insert(self.pick(&MAP_KEYS).to_owned(), Value::Object(entry));
        }
        let mut document = json!({"betterTiled": 1 + self.below(2), "maps": maps});
        if self.chance() {
            document["godot"] = self.godot();
        }
        self.with_unknown(document)
    }
}

/// `record` in place of `replaced`, keeping the replaced record's keys outside `known` that
/// `record` does not set.
fn replaced_by(replaced: Option<Value>, record: &Value, known: &[&str]) -> Value {
    let mut record = record.clone();
    if let (Some(Value::Object(replaced)), Value::Object(new)) = (replaced, &mut record) {
        for (name, value) in replaced {
            if !known.contains(&name.as_str()) && !new.contains_key(&name) {
                new.insert(name, value);
            }
        }
    }
    record
}

/// Edits applied to a document and, by plain JSON operations, to a model of it.
#[test]
fn unknown_keys_survive_any_edit() {
    let fuzz = Fuzz::from_env(0x0B7_0009, 200);
    for case in 0..fuzz.cases {
        let mut generator = Generator {
            rng: Rng::new(fuzz.case_seed(case)),
        };
        let mut model = generator.document();
        let original = model.to_string();
        let mut document = Document::from_json(&original).unwrap();
        let mut edits = Vec::new();
        for _ in 0..1 + generator.below(6) {
            let maps = model["maps"].as_object_mut().unwrap();
            match generator.below(4) {
                0 => {
                    let key = generator.pick(&MAP_KEYS);
                    let record = generator.automap();
                    edits.push(format!("set_automap({key}, {record})"));
                    let parsed: AutoMapRecord = serde_json::from_value(record.clone()).unwrap();
                    document.set_automap(key, parsed);
                    let entry = maps.entry(key).or_insert_with(|| json!({}));
                    let replaced = entry.get("automap").cloned();
                    entry["automap"] = replaced_by(replaced, &record, &AUTOMAP_KEYS);
                }
                1 => {
                    let key = generator.pick(&MAP_KEYS);
                    edits.push(format!("remove_automap({key})"));
                    document.remove_automap(key);
                    if let Some(Value::Object(entry)) = maps.get_mut(key) {
                        entry.remove("automap");
                        if entry.is_empty() {
                            maps.remove(key);
                        }
                    }
                }
                2 => {
                    let (from, to) = (generator.pick(&MAP_KEYS), generator.pick(&MAP_KEYS));
                    edits.push(format!("move_entry({from}, {to})"));
                    document.move_entry(from, to);
                    if from != to
                        && let Some(entry) = maps.remove(from)
                    {
                        maps.insert(to.to_owned(), entry);
                    }
                }
                _ => {
                    let setup = generator.godot();
                    edits.push(format!("set_godot({setup})"));
                    document.set_godot(serde_json::from_value(setup.clone()).unwrap());
                    let replaced = model.get("godot").cloned();
                    model["godot"] = replaced_by(replaced, &setup, &GODOT_KEYS);
                }
            }
            // Every edit is saved and read back, and the text is stable.
            let text = document.to_json().unwrap();
            document = Document::from_json(&text).unwrap();
            assert_eq!(document.to_json().unwrap(), text);
        }
        let expected = Document::from_json(&model.to_string()).unwrap();
        assert_eq!(
            document.to_json().unwrap(),
            expected.to_json().unwrap(),
            "{} case {case}: {original}\nedits:\n{}",
            fuzz.reproduce(),
            edits.join("\n")
        );
    }
}
