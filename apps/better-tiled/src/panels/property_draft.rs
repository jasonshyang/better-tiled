//! The shared custom-property rows in a Cancel/OK draft, for project and world dialogs.

use super::{CustomEdit, CustomRows, Fields, RowsSetup};
use bt_core::{CustomTypes, Properties};
use bt_editor::{PropertySet, Target};

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PropertyDraft {
    rows: CustomRows,
    fields: Fields,
    problem: Option<String>,
}

impl PropertyDraft {
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        properties: &mut Properties,
        types: &CustomTypes,
    ) {
        let set = PropertySet::of(vec![Target::properties(properties)], types);
        let rows = set.rows();
        let setup = RowsSetup {
            scope: ui.id().with("draft-properties"),
            types,
            noun: "Property",
            pick_files: false,
            clipboard: true,
        };
        let mut edits = Vec::new();
        self.rows
            .show(ui, &mut self.fields, &rows, &setup, &mut edits);
        for edit in edits {
            if let CustomEdit::Change { change, .. } = edit {
                let before = properties.clone();
                let set = PropertySet::of(vec![Target::properties(&before)], types);
                self.problem = set
                    .apply_to_properties(properties, &change)
                    .err()
                    .map(|error| error.to_string());
            }
        }
        if let Some(problem) = &self.problem {
            ui.colored_label(ui.visuals().error_fg_color, problem);
        }
    }
}
