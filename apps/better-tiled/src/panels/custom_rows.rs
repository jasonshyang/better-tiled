//! Custom properties as rows (Tiled's `VariantMapProperty`, `ClassProperty` and
//! `VariantListProperty`, `reference/tiled/src/tiled/variantmapproperty.cpp`, with the
//! operations of `propertieswidget.cpp`): a field for each value by its type, class values
//! unfolding to their members and lists to their items, inherited values and unset members
//! dimmed until set with a button that resets them, and the operations Add, Remove, Rename,
//! Convert To, Copy and Paste. The Properties panel shows a thing's properties with them, and
//! the Custom Types editor a class's members.

use bt_core::{CustomTypes, ObjectId, Properties, Property, Value};
use bt_editor::{Change, ListEdit, Shown, Step, ValueType, conversions, value_types};

use super::value_fields::{
    Edited, Field, Fields, name_cell, number_text, parse_integer, parse_number,
};

/// How wide the names are, as a share of the rows' width.
const NAME_SHARE: f32 = 0.45;

/// What the rows asked for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CustomEdit {
    /// A change of the properties; the changes of one run of typing share `merge`.
    Change {
        /// The change.
        change: Change,
        /// The run, if it is part of one.
        merge: Option<u64>,
    },
    /// A file to choose for the value at a path.
    PickFile(Vec<Step>),
}

/// What the rows are shown with.
#[derive(Copy, Clone, Debug)]
pub(crate) struct RowsSetup<'a> {
    /// What the rows' widgets are kept apart by, from frame to frame.
    pub(crate) scope: egui::Id,
    /// The project's custom types.
    pub(crate) types: &'a CustomTypes,
    /// What a property is called: "Property", or "Member" for a class's members.
    pub(crate) noun: &'static str,
    /// Whether file values offer a file dialog.
    pub(crate) pick_files: bool,
    /// Whether Copy and Paste are offered.
    pub(crate) clipboard: bool,
}

/// A property being renamed in place.
#[derive(Clone, Debug)]
struct Renaming {
    name: String,
    text: String,
    focused: bool,
}

/// The Add form: the new property's name and type.
#[derive(Clone, Debug)]
struct Adding {
    name: String,
    value_type: ValueType,
    focused: bool,
}

/// The rows' own state: a rename under way, the Add form, the properties copied.
#[derive(Debug, Default)]
pub(crate) struct CustomRows {
    renaming: Option<Renaming>,
    adding: Option<Adding>,
    copied: Option<Properties>,
}

impl CustomRows {
    /// Forgets a rename and the Add form (what the rows show changed).
    pub(crate) fn forget_editing(&mut self) {
        self.renaming = None;
        self.adding = None;
    }

    /// Shows `rows`, then the Add form and the buttons below them; what they ask for goes to
    /// `out`.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        fields: &mut Fields,
        rows: &[Shown],
        setup: &RowsSetup<'_>,
        out: &mut Vec<CustomEdit>,
    ) {
        for row in rows {
            self.show_row(ui, fields, row, None, setup, out);
        }
        if rows.is_empty() {
            ui.weak(format!("No {}", plural(setup.noun)));
        }
        self.show_adding(ui, setup, out);
        ui.horizontal(|ui| {
            if self.adding.is_none() && ui.button(format!("Add {}", setup.noun)).clicked() {
                self.adding = Some(Adding {
                    name: String::new(),
                    value_type: ValueType::String,
                    focused: false,
                });
            }
            if setup.clipboard {
                if ui
                    .add_enabled(!rows.is_empty(), egui::Button::new("Copy All"))
                    .clicked()
                {
                    self.copied = Some(top_level(rows));
                }
                let paste = egui::Button::new("Paste");
                if ui.add_enabled(self.copied.is_some(), paste).clicked()
                    && let Some(copied) = &self.copied
                {
                    out.push(change(Change::paste(copied.clone())));
                }
            }
        });
    }

    /// One row, and its members or items below it. `list` is the length of the list a row is
    /// an item of.
    fn show_row(
        &mut self,
        ui: &mut egui::Ui,
        fields: &mut Fields,
        row: &Shown,
        list: Option<usize>,
        setup: &RowsSetup<'_>,
        out: &mut Vec<CustomEdit>,
    ) {
        let unfolds =
            !row.children.is_empty() || matches!(row.value.value, Value::Class(_) | Value::List(_));
        if !unfolds {
            ui.horizontal(|ui| self.show_line(ui, fields, row, list, setup, out));
            return;
        }
        let id = setup.scope.with("fold").with(&row.path);
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
            .show_header(ui, |ui| self.show_line(ui, fields, row, list, setup, out))
            .body(|ui| {
                let items = row.children.len();
                let is_list = matches!(row.value.value, Value::List(_));
                for child in &row.children {
                    self.show_row(ui, fields, child, is_list.then_some(items), setup, out);
                }
            });
    }

    /// A row's name, its field and its reset button.
    fn show_line(
        &mut self,
        ui: &mut egui::Ui,
        fields: &mut Fields,
        row: &Shown,
        list: Option<usize>,
        setup: &RowsSetup<'_>,
        out: &mut Vec<CustomEdit>,
    ) {
        let width = (ui.available_width() * NAME_SHARE).max(60.0);
        let top = row.path.len() == 1;
        let name = row_name(row);
        if top
            && let Some(renaming) = &mut self.renaming
            && renaming.name == name
        {
            let response = ui.add_sized(
                [width, ui.spacing().interact_size.y],
                egui::TextEdit::singleline(&mut renaming.text)
                    .id(setup.scope.with("rename").with(&name)),
            );
            if !renaming.focused {
                response.request_focus();
                renaming.focused = true;
            }
            if response.lost_focus() {
                let escaped = ui.input(|input| input.key_pressed(egui::Key::Escape));
                if !escaped && renaming.text != renaming.name {
                    out.push(change(Change::rename(
                        renaming.name.clone(),
                        renaming.text.clone(),
                    )));
                }
                self.renaming = None;
            }
            return;
        }
        let mut text = egui::RichText::new(&name);
        if row.holders.is_empty() {
            text = text.weak();
        }
        let label = name_cell(
            ui,
            width,
            egui::Label::new(text)
                .truncate()
                .sense(egui::Sense::click()),
        )
        .on_hover_text(row_hint(row));
        if top && label.double_clicked() {
            self.start_renaming(&name);
        }
        label.context_menu(|ui| {
            if top {
                self.top_menu(ui, row, setup, out);
            } else if let Some(length) = list {
                item_menu(ui, row, length, out);
            }
        });
        let field = Field {
            id: setup.scope.with("field").with(&row.path),
            label: label.id,
            mixed: row.mixed,
            enabled: true,
        };
        if let Some(edit) = value_field(ui, fields, field, row, setup) {
            out.push(edit);
        }
        if row.resettable
            && ui
                .small_button("⟲")
                .on_hover_text(format!("Reset {name}"))
                .clicked()
        {
            out.push(change(Change::reset(row.path.clone())));
        }
    }

    /// The menu of a property: Copy, Paste, Rename, Convert To, Reset, Remove.
    fn top_menu(
        &mut self,
        ui: &mut egui::Ui,
        row: &Shown,
        setup: &RowsSetup<'_>,
        out: &mut Vec<CustomEdit>,
    ) {
        let name = row_name(row);
        if setup.clipboard {
            if ui.button("Copy").clicked() {
                let mut copied = Properties::new();
                copied.insert(name.clone(), row.value.clone());
                self.copied = Some(copied);
                ui.close();
            }
            if ui
                .add_enabled(self.copied.is_some(), egui::Button::new("Paste"))
                .clicked()
                && let Some(copied) = &self.copied
            {
                out.push(change(Change::paste(copied.clone())));
                ui.close();
            }
            ui.separator();
        }
        if ui.button("Rename…").clicked() {
            self.start_renaming(&name);
            ui.close();
        }
        let targets = conversions(&row.value, setup.types);
        ui.add_enabled_ui(!targets.is_empty(), |ui| {
            ui.menu_button("Convert To", |ui| {
                for to in targets {
                    if ui.button(to.name()).clicked() {
                        out.push(change(Change::convert(name.clone(), to)));
                        ui.close();
                    }
                }
            });
        });
        if ui
            .add_enabled(row.resettable, egui::Button::new("Reset"))
            .clicked()
        {
            out.push(change(Change::reset(row.path.clone())));
            ui.close();
        }
        let owned = !row.holders.is_empty();
        if ui
            .add_enabled(owned, egui::Button::new(format!("Remove {}", setup.noun)))
            .clicked()
        {
            out.push(change(Change::remove(name)));
            ui.close();
        }
    }

    fn start_renaming(&mut self, name: &str) {
        self.renaming = Some(Renaming {
            name: name.to_owned(),
            text: name.to_owned(),
            focused: false,
        });
    }

    /// The Add form: a name, a type and Add; Enter adds, Escape or Cancel closes it.
    fn show_adding(&mut self, ui: &mut egui::Ui, setup: &RowsSetup<'_>, out: &mut Vec<CustomEdit>) {
        let Some(adding) = &mut self.adding else {
            return;
        };
        let mut add = false;
        let mut close = false;
        ui.horizontal(|ui| {
            let label = ui.label(format!("{} name", setup.noun));
            let response = ui
                .add(
                    egui::TextEdit::singleline(&mut adding.name)
                        .id(setup.scope.with("add-name"))
                        .desired_width(110.0),
                )
                .labelled_by(label.id);
            if !adding.focused {
                response.request_focus();
                adding.focused = true;
            }
            if response.lost_focus() {
                add |= ui.input(|input| input.key_pressed(egui::Key::Enter));
                close |= ui.input(|input| input.key_pressed(egui::Key::Escape));
            }
            let types = value_types(setup.types);
            let of_type = ui.label("of type");
            let combo = egui::ComboBox::from_id_salt(setup.scope.with("add-type"))
                .selected_text(adding.value_type.name())
                .show_ui(ui, |ui| {
                    for value_type in types {
                        let name = value_type.name().to_owned();
                        ui.selectable_value(&mut adding.value_type, value_type, name);
                    }
                });
            combo.response.labelled_by(of_type.id);
            add |= ui.button("Add").clicked();
            close |= ui.button("Cancel").clicked();
        });
        if add && !adding.name.trim().is_empty() {
            out.push(change(Change::add(
                adding.name.trim(),
                adding.value_type.clone(),
            )));
            close = true;
        }
        if close {
            self.adding = None;
        }
    }
}

/// A change made at once.
fn change(change: Change) -> CustomEdit {
    CustomEdit::Change {
        change,
        merge: None,
    }
}

/// The menu of a list's item: remove, duplicate, move.
fn item_menu(ui: &mut egui::Ui, row: &Shown, length: usize, out: &mut Vec<CustomEdit>) {
    let Some((Step::Item(index), list)) = row.path.split_last() else {
        return;
    };
    let index = *index;
    let mut edit = |ui: &mut egui::Ui, text: &str, enabled: bool, list_edit: ListEdit| {
        if ui.add_enabled(enabled, egui::Button::new(text)).clicked() {
            out.push(change(Change::list(list.to_vec(), list_edit)));
            ui.close();
        }
    };
    edit(ui, "Remove Item", true, ListEdit::Remove(index));
    edit(ui, "Duplicate Item", true, ListEdit::Duplicate(index));
    edit(
        ui,
        "Move Up",
        index > 0,
        ListEdit::Move {
            from: index,
            to: index.saturating_sub(1),
        },
    );
    edit(
        ui,
        "Move Down",
        index + 1 < length,
        ListEdit::Move {
            from: index,
            to: index + 1,
        },
    );
}

/// The field of a row's value, by its type: an enum's choice box or flags, a class's name, a
/// list's length with a button that adds an item, else a built-in type's field.
fn value_field(
    ui: &mut egui::Ui,
    fields: &mut Fields,
    field: Field,
    row: &Shown,
    setup: &RowsSetup<'_>,
) -> Option<CustomEdit> {
    let set = |value: Property, merge: Option<u64>| CustomEdit::Change {
        change: Change::set(row.path.clone(), value),
        merge,
    };
    // A value of a type the project lacks keeps its type's name.
    let same_type = |value: Value| Property {
        value,
        custom_type: row.value.custom_type.clone(),
    };
    if let Some(name) = &row.value.custom_type
        && let Some(enumeration) = setup.types.enum_type(name)
    {
        let current = enumeration.to_int(&row.value.value).unwrap_or(0);
        let typed = |number: i64| {
            Property::typed(enumeration.to_storage(&Value::Int(number)), name.clone())
        };
        if enumeration.values_as_flags {
            let edited = fields.flags(ui, field, current, &enumeration.values)?;
            return Some(set(typed(edited.value), edited.merge));
        }
        let selected = enumeration
            .to_int(&row.value.value)
            .and_then(|index| usize::try_from(index).ok())
            .filter(|index| *index < enumeration.values.len());
        let shown = value_text(&row.value.value);
        let edited = fields.choice(ui, field, selected, &shown, &enumeration.values)?;
        let index = i64::try_from(edited.value).ok()?;
        return Some(set(typed(index), edited.merge));
    }
    match &row.value.value {
        Value::Class(_) => {
            let class = row.value.custom_type.as_deref().unwrap_or("class");
            ui.weak(class);
            None
        }
        Value::List(items) => {
            ui.weak(format!("{} items", items.len()));
            let mut pushed = None;
            ui.menu_button("+", |ui| {
                for value_type in value_types(setup.types) {
                    if ui.button(value_type.name()).clicked()
                        && let Ok(item) = value_type.default_value(setup.types)
                    {
                        pushed = Some(change(Change::list(row.path.clone(), ListEdit::Push(item))));
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text("Add an item");
            pushed
        }
        Value::String(text) => {
            let edited = fields.text(ui, field, text, |typed| Some(typed.to_owned()))?;
            Some(set(same_type(Value::String(edited.value)), edited.merge))
        }
        Value::Int(number) => {
            let edited = fields.text(ui, field, &number.to_string(), parse_integer)?;
            Some(set(same_type(Value::Int(edited.value)), edited.merge))
        }
        Value::Float(number) => {
            let edited = fields.text(ui, field, &number_text(*number), parse_number)?;
            Some(set(same_type(Value::Float(edited.value)), edited.merge))
        }
        Value::Bool(value) => {
            let edited = fields.checkbox(ui, field, *value)?;
            Some(set(same_type(Value::Bool(edited.value)), edited.merge))
        }
        Value::Color(color) => {
            let edited = fields.color(ui, field, *color, true)?;
            Some(set(same_type(Value::Color(edited.value)), edited.merge))
        }
        Value::Object(id) => {
            let edited: Edited<u32> = fields.text(ui, field, &id.0.to_string(), |typed| {
                typed.trim().parse().ok()
            })?;
            Some(set(
                same_type(Value::Object(ObjectId(edited.value))),
                edited.merge,
            ))
        }
        Value::File(file) => {
            let mut edit = None;
            ui.horizontal(|ui| {
                if setup.pick_files
                    && ui
                        .small_button("…")
                        .on_hover_text("Choose a file")
                        .clicked()
                {
                    edit = Some(CustomEdit::PickFile(row.path.clone()));
                }
                if let Some(edited) = fields.text(ui, field, file, |typed| Some(typed.to_owned())) {
                    edit = Some(set(same_type(Value::File(edited.value)), edited.merge));
                }
            });
            edit
        }
    }
}

/// What a row is called: its property's or member's name, or its place in its list.
fn row_name(row: &Shown) -> String {
    match row.path.last() {
        Some(Step::Member(name)) => name.clone(),
        Some(Step::Item(index)) => format!("[{index}]"),
        None => String::new(),
    }
}

/// What hovering a row's name says: its type, and where an inherited value comes from.
fn row_hint(row: &Shown) -> String {
    let type_name = ValueType::of(&row.value).map_or_else(
        || "class".to_owned(),
        |value_type| value_type.name().to_owned(),
    );
    match (&row.inherited_from, row.is_default) {
        (_, true) => format!("{type_name}, the class's default"),
        (Some(origin), _) if row.holders.is_empty() => format!("{type_name}, from {origin}"),
        _ => type_name,
    }
}

/// A value in words, for a choice box whose value is not among its options.
fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Int(number) => number.to_string(),
        other => other.type_name().to_owned(),
    }
}

/// The properties of the first target, from the rows (what Copy All takes).
fn top_level(rows: &[Shown]) -> Properties {
    let mut properties = Properties::new();
    for row in rows {
        if let [Step::Member(name)] = row.path.as_slice() {
            properties.insert(name.clone(), row.value.clone());
        }
    }
    properties
}

/// "Properties", "Members".
fn plural(noun: &str) -> String {
    match noun {
        "Property" => "properties".to_owned(),
        other => format!("{}s", other.to_lowercase()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(path: Vec<Step>, value: Value) -> Shown {
        Shown {
            path,
            value: Property::new(value),
            mixed: false,
            holders: vec![0],
            inherited_from: None,
            is_default: false,
            resettable: false,
            children: Vec::new(),
        }
    }

    #[test]
    fn rows_are_named_and_copied_by_their_top_level_properties() {
        let hp = shown(vec![Step::Member("hp".into())], Value::Int(3));
        let item = shown(
            vec![Step::Member("zones".into()), Step::Item(2)],
            Value::Int(1),
        );
        assert_eq!(row_name(&hp), "hp");
        assert_eq!(row_name(&item), "[2]");
        assert_eq!(row_hint(&hp), "int");
        let copied = top_level(&[hp, item]);
        assert_eq!(copied.len(), 1);
        assert_eq!(plural("Member"), "members");
    }
}
