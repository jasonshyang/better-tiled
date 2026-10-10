//! The Custom Types editor (Tiled's `PropertyTypesEditor`,
//! `reference/tiled/src/tiled/propertytypeseditor.cpp`): the project's enums and classes in a
//! list with Add Enum, Add Class and Remove Type, and the selected type's details: its name; an
//! enum's values (added, renamed in place, removed), how they are saved and whether they are
//! flags; a class's colour, whether its objects are filled, what it may be the class of, and its
//! members with their types and defaults, edited as custom properties are. Every change is a
//! step on the project in the current document's history. With no project open, a line says
//! the types last until better-tiled quits. Escape in a field leaves the field; Escape
//! elsewhere, or Close, closes the editor.

use bt_core::{
    ClassType, ClassUsage, CustomType, CustomTypeId, CustomTypeKind, EnumStorage, EnumType,
};
use bt_editor::{Editor, Intent, TypeEdit};

use crate::panels::{CustomEdit, CustomRows, Field, Fields, RowsSetup};

/// How wide the list of types is.
const LIST_WIDTH: f32 = 190.0;
/// How wide the details are.
const DETAILS_WIDTH: f32 = 470.0;

/// What Tiled's "Use as" calls each usage.
const USAGE_LABELS: [UsageLabel; 9] = [
    UsageLabel {
        usage: ClassUsage::PROPERTY,
        label: "Property value",
    },
    UsageLabel {
        usage: ClassUsage::MAP,
        label: "Map",
    },
    UsageLabel {
        usage: ClassUsage::LAYER,
        label: "Layer",
    },
    UsageLabel {
        usage: ClassUsage::OBJECT,
        label: "Object",
    },
    UsageLabel {
        usage: ClassUsage::TILE,
        label: "Tile",
    },
    UsageLabel {
        usage: ClassUsage::TILESET,
        label: "Tileset",
    },
    UsageLabel {
        usage: ClassUsage::WANG_COLOR,
        label: "Terrain",
    },
    UsageLabel {
        usage: ClassUsage::WANG_SET,
        label: "Terrain Set",
    },
    UsageLabel {
        usage: ClassUsage::PROJECT,
        label: "Project",
    },
];

/// A usage and what the editor calls it.
#[derive(Copy, Clone, Debug)]
struct UsageLabel {
    usage: ClassUsage,
    label: &'static str,
}

/// A name being typed: a type's, or (with `index`) one of an enum's values.
#[derive(Clone, Debug, PartialEq)]
struct Naming {
    id: CustomTypeId,
    index: Option<usize>,
    text: String,
}

/// An edit the editor asked for, and the run it belongs to.
#[derive(Clone, Debug, PartialEq)]
struct Asked {
    edit: TypeEdit,
    merge: Option<u64>,
}

/// The Custom Types editor's state.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CustomTypesDialog {
    /// The type whose details are shown.
    selected: Option<CustomTypeId>,
    /// The types there were when one was added, to select the new one when it shows.
    adding_from: Option<Vec<CustomTypeId>>,
    naming: Option<Naming>,
    fields: Fields,
    members: CustomRows,
}

impl CustomTypesDialog {
    /// Shows the editor; its changes go to `out`. Whether it closed.
    pub(crate) fn show(
        &mut self,
        ctx: &egui::Context,
        editor: &Editor,
        out: &mut Vec<Intent>,
    ) -> bool {
        let Some(doc) = editor.current() else {
            return true;
        };
        let types = editor.custom_types();
        self.follow_types(&types.types);
        let typing = ctx.memory(|memory| memory.focused().is_some());
        let mut asked = Vec::new();
        let mut close = false;
        egui::Modal::new(egui::Id::new("custom_types")).show(ctx, |ui| {
            ui.set_width(LIST_WIDTH + DETAILS_WIDTH + 24.0);
            ui.heading("Custom Types");
            match editor.project_path() {
                Some(path) => ui.weak(format!(
                    "Saved in {} when the map is saved",
                    bt_utils::paths::display_name(path)
                )),
                None => ui.weak("No project is open: these types last until better-tiled quits"),
            };
            ui.separator();
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(LIST_WIDTH);
                    self.show_list(ui, &types.types, &mut asked);
                });
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.set_width(DETAILS_WIDTH);
                    let selected = self
                        .selected
                        .and_then(|id| types.types.iter().find(|t| t.id == id));
                    match selected {
                        Some(found) => self.show_details(ui, editor, found, &mut asked),
                        None => {
                            ui.weak("Add an enum or a class, or choose one");
                        }
                    }
                });
            });
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = ui.button("Close").clicked();
            });
        });
        for Asked { edit, merge } in asked {
            out.push(Intent::EditCustomTypes { doc, edit, merge });
        }
        let escaped = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        close || (escaped && !typing)
    }

    /// Keeps the selection on a type that exists, on the one just added when there is one.
    fn follow_types(&mut self, types: &[CustomType]) {
        if let Some(before) = &self.adding_from
            && let Some(added) = types.iter().find(|t| !before.contains(&t.id))
        {
            self.selected = Some(added.id);
            self.adding_from = None;
        }
        if self
            .selected
            .is_some_and(|id| !types.iter().any(|t| t.id == id))
        {
            self.selected = None;
        }
        if self.selected.is_none() {
            self.selected = types.first().map(|t| t.id);
        }
    }

    /// The list of types and the buttons below it.
    fn show_list(&mut self, ui: &mut egui::Ui, types: &[CustomType], asked: &mut Vec<Asked>) {
        egui::ScrollArea::vertical()
            .id_salt("custom_types_list")
            .max_height(300.0)
            .show(ui, |ui| {
                for custom_type in types {
                    let kind = match custom_type.kind {
                        CustomTypeKind::Class(_) => "class",
                        CustomTypeKind::Enum(_) => "enum",
                    };
                    let chosen = self.selected == Some(custom_type.id);
                    let response = ui
                        .selectable_label(chosen, &custom_type.name)
                        .on_hover_text(kind);
                    if response.clicked() && !chosen {
                        self.selected = Some(custom_type.id);
                        self.forget_editing();
                    }
                }
                if types.is_empty() {
                    ui.weak("No types");
                }
            });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let mut add = |edit: TypeEdit| {
                self.adding_from = Some(types.iter().map(|t| t.id).collect());
                self.forget_editing();
                asked.push(Asked { edit, merge: None });
            };
            if ui.button("Add Enum").clicked() {
                add(TypeEdit::AddEnum);
            }
            if ui.button("Add Class").clicked() {
                add(TypeEdit::AddClass);
            }
        });
        let remove = egui::Button::new("Remove Type");
        if ui.add_enabled(self.selected.is_some(), remove).clicked()
            && let Some(id) = self.selected
        {
            self.forget_editing();
            asked.push(Asked {
                edit: TypeEdit::Remove(id),
                merge: None,
            });
        }
    }

    /// Forgets what was being typed.
    fn forget_editing(&mut self) {
        self.naming = None;
        self.fields.forget_typing();
        self.members.forget_editing();
    }

    /// The selected type's name and, by its kind, an enum's or a class's details.
    fn show_details(
        &mut self,
        ui: &mut egui::Ui,
        editor: &Editor,
        found: &CustomType,
        asked: &mut Vec<Asked>,
    ) {
        let id = found.id;
        ui.horizontal(|ui| {
            let label = ui.label("Name");
            if let Some(name) = self.name_field(ui, label.id, id, None, &found.name) {
                asked.push(Asked {
                    edit: TypeEdit::Rename { id, name },
                    merge: None,
                });
            }
        });
        ui.add_space(4.0);
        match &found.kind {
            CustomTypeKind::Enum(enumeration) => self.show_enum(ui, id, enumeration, asked),
            CustomTypeKind::Class(class) => self.show_class(ui, editor, id, class, asked),
        }
    }

    /// A name typed in place: what was typed when the field is left with Enter or a click
    /// elsewhere and differs from `current`; Escape drops it.
    fn name_field(
        &mut self,
        ui: &mut egui::Ui,
        label: egui::Id,
        id: CustomTypeId,
        index: Option<usize>,
        current: &str,
    ) -> Option<String> {
        let mine = |naming: &&mut Naming| naming.id == id && naming.index == index;
        let mut text = self
            .naming
            .as_mut()
            .filter(mine)
            .map_or_else(|| current.to_owned(), |naming| naming.text.clone());
        let field_id = egui::Id::new("custom_type_name").with(id).with(index);
        let response = ui
            .add(
                egui::TextEdit::singleline(&mut text)
                    .id(field_id)
                    .desired_width(220.0),
            )
            .labelled_by(label);
        if response.has_focus() {
            self.naming = Some(Naming {
                id,
                index,
                text: text.clone(),
            });
        }
        if !response.lost_focus() {
            return None;
        }
        self.naming = None;
        let escaped = ui.input(|input| input.key_pressed(egui::Key::Escape));
        (!escaped && text.trim() != current).then_some(text)
    }

    /// An enum's details: how its values are saved, whether they are flags, and the values.
    fn show_enum(
        &mut self,
        ui: &mut egui::Ui,
        id: CustomTypeId,
        enumeration: &EnumType,
        asked: &mut Vec<Asked>,
    ) {
        ui.horizontal(|ui| {
            let label = ui.label("Save as");
            let mut storage = enumeration.storage;
            let combo = egui::ComboBox::from_id_salt(egui::Id::new("enum_storage").with(id))
                .selected_text(storage_label(storage))
                .show_ui(ui, |ui| {
                    for option in [EnumStorage::String, EnumStorage::Int] {
                        ui.selectable_value(&mut storage, option, storage_label(option));
                    }
                });
            combo.response.labelled_by(label.id);
            if storage != enumeration.storage {
                asked.push(Asked {
                    edit: TypeEdit::SetStorage { id, storage },
                    merge: None,
                });
            }
        });
        let mut flags = enumeration.values_as_flags;
        if ui
            .checkbox(&mut flags, "Allow multiple values (flags)")
            .changed()
        {
            asked.push(Asked {
                edit: TypeEdit::SetValuesAsFlags { id, flags },
                merge: None,
            });
        }
        ui.add_space(4.0);
        ui.strong("Values");
        for (index, value) in enumeration.values.iter().enumerate() {
            ui.horizontal(|ui| {
                let label = ui.weak(format!("{index}"));
                if let Some(name) = self.name_field(ui, label.id, id, Some(index), value) {
                    asked.push(Asked {
                        edit: TypeEdit::RenameValue { id, index, name },
                        merge: None,
                    });
                }
                let remove = ui
                    .small_button("×")
                    .on_hover_text(format!("Remove {value}"));
                if remove.clicked() {
                    self.forget_editing();
                    asked.push(Asked {
                        edit: TypeEdit::RemoveValue { id, index },
                        merge: None,
                    });
                }
            });
        }
        if ui.button("Add Value").clicked() {
            asked.push(Asked {
                edit: TypeEdit::AddValue(id),
                merge: None,
            });
        }
    }

    /// A class's details: its colour, fill, what it may be the class of, and its members.
    fn show_class(
        &mut self,
        ui: &mut egui::Ui,
        editor: &Editor,
        id: CustomTypeId,
        class: &ClassType,
        asked: &mut Vec<Asked>,
    ) {
        ui.horizontal(|ui| {
            let label = ui.label("Color");
            let field = Field {
                id: egui::Id::new("class_color").with(id),
                label: label.id,
                mixed: false,
                enabled: true,
            };
            if let Some(edited) = self.fields.color(ui, field, Some(class.color), false)
                && let Some(color) = edited.value
            {
                asked.push(Asked {
                    edit: TypeEdit::SetColor { id, color },
                    merge: edited.merge,
                });
            }
            let mut fill = class.draw_fill;
            if ui.checkbox(&mut fill, "Draw fill").changed() {
                asked.push(Asked {
                    edit: TypeEdit::SetDrawFill { id, fill },
                    merge: None,
                });
            }
        });
        ui.add_space(4.0);
        ui.strong("Use as");
        ui.horizontal_wrapped(|ui| {
            for usage in USAGE_LABELS {
                let mut on = class.use_as.contains(usage.usage);
                if ui.checkbox(&mut on, usage.label).changed() {
                    let mut use_as = class.use_as;
                    use_as.set(usage.usage, on);
                    asked.push(Asked {
                        edit: TypeEdit::SetUseAs { id, usage: use_as },
                        merge: None,
                    });
                }
            }
        });
        ui.add_space(4.0);
        ui.strong("Members");
        let rows = match editor.member_rows(id) {
            Ok(rows) => rows,
            Err(error) => {
                ui.weak(error.to_string());
                return;
            }
        };
        let setup = RowsSetup {
            scope: egui::Id::new("class_members").with(id),
            types: editor.custom_types(),
            noun: "Member",
            pick_files: false,
            clipboard: false,
        };
        let mut edits = Vec::new();
        egui::ScrollArea::vertical()
            .id_salt("class_members")
            .max_height(220.0)
            .show(ui, |ui| {
                self.members
                    .show(ui, &mut self.fields, &rows, &setup, &mut edits);
            });
        for edit in edits {
            if let CustomEdit::Change { change, merge } = edit {
                asked.push(Asked {
                    edit: TypeEdit::Members { id, change },
                    merge,
                });
            }
        }
    }
}

/// What the "Save as" box calls a way of saving an enum's values.
fn storage_label(storage: EnumStorage) -> &'static str {
    match storage {
        EnumStorage::String => "String",
        EnumStorage::Int => "Number",
    }
}

#[cfg(test)]
mod tests {
    use bt_core::CustomTypes;

    use super::*;

    #[test]
    fn the_selection_follows_the_types() {
        let mut types = CustomTypes::new();
        types.add(CustomType::class("Enemy", ClassType::default()));
        let mut dialog = CustomTypesDialog::default();
        dialog.follow_types(&types.types);
        assert_eq!(dialog.selected, Some(types.types[0].id));
        dialog.adding_from = Some(vec![types.types[0].id]);
        types.add(CustomType::enumeration("Kind", EnumType::default()));
        dialog.follow_types(&types.types);
        assert_eq!(
            dialog.selected,
            Some(types.types[1].id),
            "the one just added"
        );
        types.types.clear();
        dialog.follow_types(&types.types);
        assert_eq!(dialog.selected, None);
        assert_eq!(storage_label(EnumStorage::Int), "Number");
    }
}
