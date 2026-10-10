//! The Properties panel (Tiled's `PropertiesDock` and `PropertiesWidget`,
//! `reference/tiled/src/tiled/propertiesdock.cpp` and `propertieswidget.cpp`): what the
//! current document inspects (`Editor::inspected`: the map, the selected layers or objects, or
//! the tileset editor's tiles or tileset), in two sections: the built-in attributes, with
//! Tiled's labels, and the custom properties. Values the targets do not share show as mixed,
//! read-only attributes are greyed out, and what is typed applies as it is typed, one step per
//! run of typing.

use bt_editor::{
    Attribute, AttributeRow, AttributeValue, DocumentId, Editor, FileRequest, Inspected, Intent,
};

use super::custom_rows::{CustomEdit, CustomRows, RowsSetup};
use super::value_fields::{
    Edited, Field, Fields, name_cell, number_text, parse_integer, parse_number,
};

/// How wide the attributes' names are, as a share of the panel's width.
const NAME_SHARE: f32 = 0.45;

/// What the panel showed: a document and what it inspected.
#[derive(Clone, Debug, PartialEq)]
struct Subject {
    doc: DocumentId,
    inspected: Inspected,
}

/// The panel's own state: its fields, the custom properties' rows, what it showed last and the
/// files it asks for.
#[derive(Debug, Default)]
pub(crate) struct PropertiesPanel {
    fields: Fields,
    custom: CustomRows,
    shown: Option<Subject>,
    file_requests: Vec<FileRequest>,
}

impl PropertiesPanel {
    /// Shows what `doc` inspects; changes go to `out`.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        editor: &Editor,
        doc: DocumentId,
        out: &mut Vec<Intent>,
    ) {
        ui.heading("Properties");
        ui.separator();
        let Some(inspected) = editor.inspected(doc) else {
            return;
        };
        let subject = Subject {
            doc,
            inspected: inspected.clone(),
        };
        if self.shown.as_ref() != Some(&subject) {
            self.fields.forget_typing();
            self.custom.forget_editing();
            self.shown = Some(subject.clone());
        }
        if inspected.is_empty() {
            ui.weak("Nothing is selected");
            return;
        }
        let attributes = editor.attribute_rows(doc, &inspected);
        let properties = editor.property_rows(doc, &inspected);
        let scope = egui::Id::new("properties").with(doc).with(&inspected);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::CollapsingHeader::new(title(&inspected))
                    .id_salt(scope.with("attributes"))
                    .default_open(true)
                    .show(ui, |ui| match &attributes {
                        Ok(rows) => {
                            for row in rows {
                                self.show_attribute(ui, scope, row, &subject, out);
                            }
                        }
                        Err(error) => {
                            ui.weak(error.to_string());
                        }
                    });
                egui::CollapsingHeader::new("Custom Properties")
                    .id_salt(scope.with("custom"))
                    .default_open(true)
                    .show(ui, |ui| match &properties {
                        Ok(rows) => {
                            let setup = RowsSetup {
                                scope,
                                types: editor.custom_types(),
                                noun: "Property",
                                pick_files: true,
                                clipboard: true,
                            };
                            let mut edits = Vec::new();
                            self.custom
                                .show(ui, &mut self.fields, rows, &setup, &mut edits);
                            for edit in edits {
                                self.take_custom_edit(edit, &subject, out);
                            }
                        }
                        Err(error) => {
                            ui.weak(error.to_string());
                        }
                    });
            });
    }

    /// The files the panel asked for since the last call, for the window to ask for.
    pub(crate) fn take_file_requests(&mut self) -> Vec<FileRequest> {
        std::mem::take(&mut self.file_requests)
    }

    /// A built-in attribute's row: its name and a field by its kind.
    fn show_attribute(
        &mut self,
        ui: &mut egui::Ui,
        scope: egui::Id,
        row: &AttributeRow,
        subject: &Subject,
        out: &mut Vec<Intent>,
    ) {
        ui.horizontal(|ui| {
            let width = (ui.available_width() * NAME_SHARE).max(60.0);
            let mut text = egui::RichText::new(row.attribute.label());
            if row.from_template {
                text = text.weak();
            }
            let label = name_cell(ui, width, egui::Label::new(text).truncate());
            let label_id = label.id;
            if row.from_template {
                label.on_hover_text("From the template: setting it overrides the template");
            }
            let field = Field {
                id: scope.with("attribute").with(row.attribute),
                label: label_id,
                mixed: row.mixed,
                enabled: !row.read_only,
            };
            if let Some(edited) = attribute_field(ui, &mut self.fields, field, row) {
                out.push(Intent::SetAttribute {
                    doc: subject.doc,
                    inspected: subject.inspected.clone(),
                    attribute: row.attribute,
                    value: edited.value,
                    merge: edited.merge,
                });
            }
        });
    }

    /// What the custom properties' rows asked for, as intents or file requests.
    fn take_custom_edit(&mut self, edit: CustomEdit, subject: &Subject, out: &mut Vec<Intent>) {
        match edit {
            CustomEdit::Change { change, merge } => out.push(Intent::ChangeProperties {
                doc: subject.doc,
                inspected: subject.inspected.clone(),
                change,
                merge,
            }),
            CustomEdit::PickFile(path) => self.file_requests.push(FileRequest::PropertyFile {
                doc: subject.doc,
                inspected: subject.inspected.clone(),
                path,
            }),
        }
    }
}

/// The field of an attribute, by its kind: a choice box for a choice, a text field for text and
/// numbers, a checkbox, a colour; images and files are shown by their paths.
fn attribute_field(
    ui: &mut egui::Ui,
    fields: &mut Fields,
    field: Field,
    row: &AttributeRow,
) -> Option<Edited<AttributeValue>> {
    let choices = row.attribute.choices();
    if !choices.is_empty() {
        let names: Vec<String> = choices.iter().map(ToString::to_string).collect();
        let selected = choices.iter().position(|choice| *choice == row.value);
        let edited = fields.choice(ui, field, selected, &row.value.to_string(), &names)?;
        return Some(Edited {
            value: choices.get(edited.value)?.clone(),
            merge: edited.merge,
        });
    }
    match &row.value {
        AttributeValue::Text(text) => fields.text(ui, field, text, |typed| {
            Some(AttributeValue::Text(typed.to_owned()))
        }),
        AttributeValue::Integer(number) => fields.text(ui, field, &number.to_string(), |text| {
            parse_integer(text).map(AttributeValue::Integer)
        }),
        AttributeValue::Number(number) => fields.text(ui, field, &number_text(*number), |text| {
            parse_number(text).map(AttributeValue::Number)
        }),
        AttributeValue::Bool(value) => {
            let edited = fields.checkbox(ui, field, *value)?;
            Some(Edited {
                value: AttributeValue::Bool(edited.value),
                merge: edited.merge,
            })
        }
        AttributeValue::Color(color) => {
            let can_clear = row.attribute != Attribute::TextColor;
            let edited = fields.color(ui, field, *color, can_clear)?;
            Some(Edited {
                value: AttributeValue::Color(edited.value),
                merge: edited.merge,
            })
        }
        other => {
            ui.add_enabled(false, egui::Label::new(other.to_string()).truncate());
            None
        }
    }
}

/// The attributes section's title: what is inspected.
fn title(inspected: &Inspected) -> String {
    let counted = |count: usize, one: &str, many: &str| {
        if count == 1 {
            one.to_owned()
        } else {
            format!("{count} {many}")
        }
    };
    match inspected {
        Inspected::Map => "Map".to_owned(),
        Inspected::Tileset => "Tileset".to_owned(),
        Inspected::Layers(layers) => counted(layers.len(), "Layer", "Layers"),
        Inspected::Objects(objects) => counted(objects.len(), "Object", "Objects"),
        Inspected::Tiles(tiles) => match tiles.as_slice() {
            [tile] => format!("Tile {tile}"),
            _ => format!("{} Tiles", tiles.len()),
        },
        Inspected::TerrainSet(_) => "Terrain Set".to_owned(),
        Inspected::Terrain { .. } => "Terrain".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{LayerId, TileId};

    use super::*;

    #[test]
    fn titles_say_what_is_inspected() {
        assert_eq!(title(&Inspected::Map), "Map");
        assert_eq!(title(&Inspected::Layers(vec![LayerId(1)])), "Layer");
        assert_eq!(
            title(&Inspected::Layers(vec![LayerId(1), LayerId(2)])),
            "2 Layers"
        );
        assert_eq!(title(&Inspected::Tiles(vec![TileId(4)])), "Tile 4");
    }
}
