//! `bt diff` (`10-agents.md`, §10.7): what differs between two maps, layer by layer, in the
//! words of an edit report; or, for a map open in the editor, the changes it has not saved.

use bt_core::Map;
use serde::{Deserialize, Serialize};

use crate::changes::MapChanges;
use crate::report::{Line, Outcome, Report, Section};

/// What differs between two maps: `bt diff A B`, or `bt diff MAP` for the editor's unsaved
/// changes to it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiffOutcome {
    /// The earlier map, as the command named it.
    pub before: String,
    /// The later map, as the command named it.
    pub after: String,
    /// Whether the later map is the editor's copy of the earlier one, so the changes are those
    /// not saved yet.
    pub unsaved: bool,
    /// What differs.
    pub changes: MapChanges,
}

impl DiffOutcome {
    /// What differs from `before` to `after`, named `before` and `after` until
    /// [`DiffOutcome::named`] says otherwise.
    pub fn between(before: &Map, after: &Map) -> DiffOutcome {
        DiffOutcome {
            before: "before".to_owned(),
            after: "after".to_owned(),
            unsaved: false,
            changes: MapChanges::between(before, after),
        }
    }

    /// This diff with the maps named as the command named them.
    pub fn named(mut self, before: impl Into<String>, after: impl Into<String>) -> DiffOutcome {
        self.before = before.into();
        self.after = after.into();
        self
    }

    /// This diff as the unsaved changes of the map `map`: its file was the earlier map and the
    /// editor's copy the later.
    pub fn unsaved_in_editor(mut self, map: impl Into<String>) -> DiffOutcome {
        let map = map.into();
        self.before = map.clone();
        self.after = map;
        self.unsaved = true;
        self
    }
}

impl Outcome for DiffOutcome {
    const SCHEMA: &'static str = "bt.diff/1";

    fn report(&self) -> Report {
        let mut report = Report::new();
        if self.unsaved && self.changes.is_empty() {
            report.add_line(Line::fact(format!(
                "{}: no unsaved changes in the editor",
                self.before
            )));
            return report;
        }
        let heading = if self.unsaved {
            format!(
                "{}: unsaved changes in the editor (the file → the editor):",
                self.before
            )
        } else {
            format!("{} → {}:", self.before, self.after)
        };
        let mut section = Section::headed(heading);
        if self.changes.is_empty() {
            section.add_line(Line::fact("the maps are the same"));
        }
        section.add_lines(self.changes.lines());
        report.add_section(section);
        report
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{LayerId, Property, Value};
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::{IVec2, UVec2};

    use super::*;
    use crate::report::{from_json, to_json};

    fn map() -> Map {
        let atlas = Atlas::builder().name("tiles").build_embedded().unwrap();
        MapBuilder::new(UVec2::new(16, 16), UVec2::new(4, 3))
            .tileset(atlas.tileset)
            .tile_layer("ground", "1 1 1 1\n1 1 1 1\n1 1 1 1")
            .build()
            .unwrap()
    }

    #[test]
    fn two_maps_differ_layer_by_layer_in_the_words_of_an_edit_report() {
        let before = map();
        let mut after = before.clone();
        after
            .layer_mut(LayerId(1))
            .unwrap()
            .as_tile_layer_mut()
            .unwrap()
            .set_cell(IVec2::new(2, 1), bt_core::Cell::EMPTY)
            .unwrap();
        after
            .properties
            .insert("level".into(), Property::new(Value::Int(2)));
        let diff = DiffOutcome::between(&before, &after).named("a.tmx", "b.tmx");
        assert_eq!(
            diff.report().text(),
            "a.tmx → b.tmx:\n  map property level = 2 (new)\n  ground: 1 cell changed in 2,1 \
             1x1: nothing ×1\n"
        );
        let json = to_json(&diff).unwrap();
        assert!(json.starts_with(r#"{"schema":"bt.diff/1""#));
        assert_eq!(from_json::<DiffOutcome>(&json).unwrap(), diff);
        let same = DiffOutcome::between(&before, &before).named("a.tmx", "a2.tmx");
        assert_eq!(
            same.report().text(),
            "a.tmx → a2.tmx:\n  the maps are the same\n"
        );
    }

    #[test]
    fn the_editors_unsaved_changes_are_from_the_file_to_the_editor() {
        let file = map();
        let none = DiffOutcome::between(&file, &file).unsaved_in_editor("level.tmx");
        assert_eq!(
            none.report().text(),
            "level.tmx: no unsaved changes in the editor\n"
        );
        let mut edited = file.clone();
        edited.layer_mut(LayerId(1)).unwrap().visible = false;
        let some = DiffOutcome::between(&file, &edited).unsaved_in_editor("level.tmx");
        assert_eq!(
            some.report().text(),
            "level.tmx: unsaved changes in the editor (the file → the editor):\n  ground: hidden\n"
        );
    }
}
