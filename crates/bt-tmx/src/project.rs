//! Projects (`.tiled-project`): folders, settings, commands, and the custom types maps use.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use bt_core::{
    ClassType, ClassUsage, Color, Command, CompatibilityVersion, CustomType, CustomTypeId,
    CustomTypeKind, CustomTypes, EnumStorage, EnumType, Project,
};
use serde_json::{Map as JsonObject, Value as Json};

use crate::error::Error;
use crate::finish::PropertyDecoder;
use crate::json::{JsonReader, properties_json, qt_layout, typed_property};
use crate::loader::{Loaded, absolute};
use crate::object_types::load_object_types;
use crate::paths::{file_ref, written_reference};
use crate::raw::RawProperty;
use crate::text::read_text;
use crate::unfinish::raw_properties;
use crate::warning::{WarningKind, Warnings};

/// The keys of a project file this version reads.
const KNOWN_KEYS: [&str; 8] = [
    "automappingRulesFile",
    "commands",
    "compatibilityVersion",
    "extensionsPath",
    "folders",
    "objectTypesFile",
    "properties",
    "propertyTypes",
];

/// Reads a project. A legacy object types file it names is imported into its custom types and
/// the reference cleared.
pub fn load_project(path: &Path) -> Result<Loaded<Project>, Error> {
    let path = absolute(path)?;
    let text = read_text(&path)?;
    parse_project(&text, &path)
}

/// Reads a project from its text; `path` is where it is.
pub fn parse_project(text: &str, path: &Path) -> Result<Loaded<Project>, Error> {
    let path = absolute(path)?;
    let value = crate::json::parse_document(text, &path)?;
    let object = value
        .as_object()
        .ok_or_else(|| Error::format(&path, "not a Tiled project: not a JSON object"))?;
    let dir = path.parent().unwrap_or(Path::new("")).to_path_buf();
    let mut warnings = Warnings::new(&path);
    let mut project = Project::default();
    {
        let mut json = JsonReader {
            warnings: &mut warnings,
        };
        let types = json.objects(object, "propertyTypes");
        project.custom_types = custom_types_of(&types, &mut json);
        for folder in json.array(object, "folders") {
            if let Some(folder) = folder.as_str() {
                project.folders.push(file_ref(&dir, folder));
            }
        }
        let extensions = json
            .scalar(object, "extensionsPath")
            .map_or_else(|| "extensions".to_owned(), |v| v.to_text());
        project.extensions_path = file_ref(&dir, &extensions);
        let rules = json.text_of(object, "automappingRulesFile");
        project.automapping_rules_file = (!rules.is_empty()).then(|| file_ref(&dir, &rules));
        let object_types = json.text_of(object, "objectTypesFile");
        project.object_types_file =
            (!object_types.is_empty()).then(|| file_ref(&dir, &object_types));
        for command in json.objects(object, "commands") {
            project.commands.push(command_of(command));
        }
        project.compatibility_version = json
            .scalar(object, "compatibilityVersion")
            .and_then(|v| v.to_int())
            .map_or(
                CompatibilityVersion::Latest,
                CompatibilityVersion::from_number,
            );
        let raw = json.properties_of(object);
        project.properties =
            PropertyDecoder::new(&project.custom_types, json.warnings).properties_of(raw);
    }
    for (key, value) in object {
        if !KNOWN_KEYS.contains(&key.as_str()) {
            project.unknown_keys.insert(key.clone(), value.to_string());
        }
    }
    if let Some(file) = project.object_types_file.clone() {
        match load_object_types(&file.path) {
            Ok(loaded) => {
                warnings.record_all(loaded.warnings);
                project.custom_types.merge(loaded.value);
                project.object_types_file = None;
                warnings.record(
                    WarningKind::LegacyShape,
                    format!(
                        "the object types of “{}” become classes of the project",
                        file.source
                    ),
                );
            }
            Err(error) => warnings.record(
                WarningKind::MissingFile,
                format!(
                    "the object types file “{}” cannot be read ({error}); it stays named",
                    file.source
                ),
            ),
        }
    }
    Ok(Loaded {
        value: project,
        warnings: warnings.into_list(),
    })
}

/// A project's custom types, read in two passes as Tiled reads them: every type first, then
/// the classes' member values against all of them, a class before the classes that hold it. A
/// member that would make its class contain itself, given the members accepted so far, is
/// dropped, so of a cycle the member found last goes, as in Tiled.
fn custom_types_of(
    entries: &[&JsonObject<String, Json>],
    json: &mut JsonReader<'_>,
) -> CustomTypes {
    let mut types = CustomTypes::new();
    let mut raw_members: Vec<Vec<RawProperty>> = Vec::new();
    for entry in entries {
        json.check_keys(
            entry,
            "a custom type",
            &[
                "type",
                "id",
                "name",
                "storageType",
                "values",
                "valuesAsFlags",
                "members",
                "color",
                "drawFill",
                "useAs",
            ],
        );
        let name = json.text_of(entry, "name");
        let id = json
            .scalar(entry, "id")
            .and_then(|v| v.to_int())
            .and_then(|id| u32::try_from(id).ok())
            .unwrap_or(0);
        let kind = match json.text_of(entry, "type").as_str() {
            "" | "enum" => CustomTypeKind::Enum(EnumType {
                storage: if json.text_of(entry, "storageType") == "int" {
                    EnumStorage::Int
                } else {
                    EnumStorage::String
                },
                values: json
                    .array(entry, "values")
                    .iter()
                    .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned))
                    .collect(),
                values_as_flags: json
                    .scalar(entry, "valuesAsFlags")
                    .and_then(|v| v.to_bool())
                    .unwrap_or(false),
            }),
            "class" => {
                let defaults = ClassType::default();
                let color = json.text_of(entry, "color");
                let use_as = match entry.get("useAs").and_then(Json::as_array) {
                    Some(names) => names
                        .iter()
                        .filter_map(Json::as_str)
                        .filter_map(ClassUsage::from_use_as)
                        .fold(ClassUsage::empty(), |all, usage| all | usage),
                    // Before `useAs`, classes were property values only.
                    None => ClassUsage::PROPERTY,
                };
                CustomTypeKind::Class(ClassType {
                    members: Default::default(),
                    color: Color::from_tiled_text(&color).unwrap_or(defaults.color),
                    draw_fill: json
                        .scalar(entry, "drawFill")
                        .and_then(|v| v.to_bool())
                        .unwrap_or(defaults.draw_fill),
                    use_as,
                })
            }
            other => {
                json.warnings.record(
                    WarningKind::UnknownPropertyType,
                    format!(
                        "the custom type “{name}” is of the unknown kind “{other}”; it is dropped"
                    ),
                );
                continue;
            }
        };
        let members = json
            .objects(entry, "members")
            .into_iter()
            .map(|member| typed_property(member, "propertyType"))
            .collect();
        raw_members.push(members);
        types.types.push(CustomType {
            id: CustomTypeId(id),
            name,
            kind,
        });
    }
    let mut decoded = HashSet::new();
    let mut in_progress = Vec::new();
    for index in 0..types.types.len() {
        decode_members(
            index,
            &mut types,
            &mut raw_members,
            &mut decoded,
            &mut in_progress,
            json.warnings,
        );
    }
    types
}

/// Decodes the members of the class at `index`, after those of the classes its members are of.
fn decode_members(
    index: usize,
    types: &mut CustomTypes,
    raw_members: &mut [Vec<RawProperty>],
    decoded: &mut HashSet<usize>,
    in_progress: &mut Vec<usize>,
    warnings: &mut Warnings,
) {
    if decoded.contains(&index) || in_progress.contains(&index) {
        return;
    }
    in_progress.push(index);
    let members = raw_members
        .get_mut(index)
        .map(std::mem::take)
        .unwrap_or_default();
    for member in &members {
        let dependency = member.custom_type.as_deref().and_then(|name| {
            types.types.iter().position(|t| {
                t.name == name && t.as_class().is_some() && t.usage().contains(ClassUsage::PROPERTY)
            })
        });
        if let Some(dependency) = dependency {
            decode_members(
                dependency,
                types,
                raw_members,
                decoded,
                in_progress,
                warnings,
            );
        }
    }
    let mut out = BTreeMap::new();
    for member in members {
        let name = member.name.clone();
        let property = PropertyDecoder::new(types, warnings).property_of(member);
        out.insert(name, property);
    }
    if let Some(CustomTypeKind::Class(class)) = types.types.get_mut(index).map(|t| &mut t.kind) {
        class.members = out;
    }
    let name = types
        .types
        .get(index)
        .map(|t| t.name.clone())
        .unwrap_or_default();
    for circular in types
        .circular_members()
        .into_iter()
        .filter(|c| c.class == name)
    {
        if let Some(CustomTypeKind::Class(class)) = types.types.get_mut(index).map(|t| &mut t.kind)
        {
            class.members.remove(&circular.member);
        }
        warnings.record(
            WarningKind::CircularClass,
            format!(
                "member “{}” of class “{}” would make it contain itself; it is dropped",
                circular.member, circular.class
            ),
        );
    }
    in_progress.retain(|i| *i != index);
    decoded.insert(index);
}

/// A command, with the capitalised keys of projects before Tiled 1.4 accepted.
fn command_of(object: &JsonObject<String, Json>) -> Command {
    let read = |key: &str| {
        object.get(key).or_else(|| {
            let mut capitalised = key.to_owned();
            if let Some(first) = capitalised.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            object.get(&capitalised)
        })
    };
    let text = |key: &str| read(key).and_then(Json::as_str).unwrap_or("").to_owned();
    let flag = |key: &str| read(key).and_then(Json::as_bool).unwrap_or(false);
    Command {
        name: text("name"),
        command: text("command"),
        arguments: text("arguments"),
        working_directory: text("workingDirectory"),
        shortcut: text("shortcut"),
        enabled: flag("enabled"),
        save_before_execute: flag("saveBeforeExecute"),
        show_output: flag("showOutput"),
    }
}

/// A project's text, laid out as Tiled writes it (`QJsonDocument`).
pub(crate) fn project_text(project: &Project, path: &Path) -> String {
    let dir = path.parent().unwrap_or(Path::new(""));
    let mut document = JsonObject::new();
    for (key, value) in &project.unknown_keys {
        if let Ok(value) = serde_json::from_str::<Json>(value) {
            document.insert(key.clone(), value);
        }
    }
    document.insert(
        "automappingRulesFile".into(),
        Json::String(
            project
                .automapping_rules_file
                .as_ref()
                .map(|file| written_reference(dir, file))
                .unwrap_or_default(),
        ),
    );
    let commands = project
        .commands
        .iter()
        .map(|command| {
            let mut entry = JsonObject::new();
            entry.insert("arguments".into(), Json::String(command.arguments.clone()));
            entry.insert("command".into(), Json::String(command.command.clone()));
            entry.insert("enabled".into(), Json::Bool(command.enabled));
            entry.insert("name".into(), Json::String(command.name.clone()));
            entry.insert(
                "saveBeforeExecute".into(),
                Json::Bool(command.save_before_execute),
            );
            entry.insert("shortcut".into(), Json::String(command.shortcut.clone()));
            entry.insert("showOutput".into(), Json::Bool(command.show_output));
            entry.insert(
                "workingDirectory".into(),
                Json::String(command.working_directory.clone()),
            );
            Json::Object(entry)
        })
        .collect();
    document.insert("commands".into(), Json::Array(commands));
    if project.compatibility_version != CompatibilityVersion::Latest {
        document.insert(
            "compatibilityVersion".into(),
            Json::from(project.compatibility_version.number()),
        );
    }
    document.insert(
        "extensionsPath".into(),
        Json::String(written_reference(dir, &project.extensions_path)),
    );
    document.insert(
        "folders".into(),
        Json::Array(
            project
                .folders
                .iter()
                .map(|folder| Json::String(written_reference(dir, folder)))
                .collect(),
        ),
    );
    document.insert(
        "properties".into(),
        properties_json(&raw_properties(&project.properties), "propertytype"),
    );
    document.insert(
        "propertyTypes".into(),
        Json::Array(
            project
                .custom_types
                .types
                .iter()
                .map(custom_type_json)
                .collect(),
        ),
    );
    qt_layout(&Json::Object(document))
}

/// A custom type as Tiled writes it.
fn custom_type_json(custom_type: &CustomType) -> Json {
    let mut entry = JsonObject::new();
    entry.insert("id".into(), Json::from(custom_type.id.0));
    entry.insert("name".into(), Json::String(custom_type.name.clone()));
    match &custom_type.kind {
        CustomTypeKind::Enum(enumeration) => {
            entry.insert("type".into(), Json::String("enum".into()));
            entry.insert(
                "storageType".into(),
                Json::String(
                    match enumeration.storage {
                        EnumStorage::Int => "int",
                        EnumStorage::String => "string",
                    }
                    .into(),
                ),
            );
            entry.insert(
                "values".into(),
                Json::Array(
                    enumeration
                        .values
                        .iter()
                        .map(|v| Json::String(v.clone()))
                        .collect(),
                ),
            );
            entry.insert(
                "valuesAsFlags".into(),
                Json::Bool(enumeration.values_as_flags),
            );
        }
        CustomTypeKind::Class(class) => {
            entry.insert("type".into(), Json::String("class".into()));
            entry.insert(
                "members".into(),
                properties_json(&raw_properties(&class.members), "propertyType"),
            );
            let c = class.color;
            entry.insert(
                "color".into(),
                Json::String(format!("#{:02x}{:02x}{:02x}{:02x}", c.a, c.r, c.g, c.b)),
            );
            entry.insert("drawFill".into(), Json::Bool(class.draw_fill));
            entry.insert(
                "useAs".into(),
                Json::Array(
                    class
                        .use_as
                        .use_as_names()
                        .map(|n| Json::String(n.into()))
                        .collect(),
                ),
            );
        }
    }
    Json::Object(entry)
}

#[cfg(test)]
mod tests {
    use bt_core::{Property, Value};
    use bt_test_data::Scratch;

    use super::*;

    #[test]
    fn types_decode_against_each_other_and_cycles_are_dropped() {
        let text = r#"{
    "folders": ["."],
    "futureKey": {"kept": [1, 2]},
    "propertyTypes": [
        {"id": 1, "name": "Outer", "type": "class", "useAs": ["property"],
         "members": [{"name": "inner", "type": "class", "propertyType": "Inner", "value": {"hp": 3}}]},
        {"id": 2, "name": "Inner", "type": "class", "useAs": ["property"],
         "members": [{"name": "hp", "type": "float", "value": 1}, {"name": "back", "type": "class", "propertyType": "Outer", "value": {}}]},
        {"id": 3, "name": "Dir", "type": "enum", "storageType": "int", "values": ["Up", "Down"], "valuesAsFlags": false},
        {"id": 4, "name": "Holder", "type": "class", "useAs": ["property"],
         "members": [{"name": "inner", "type": "class", "propertyType": "Inner", "value": {"hp": 3}}]}
    ]
}"#;
        let scratch = Scratch::new("bt-tmx-project").unwrap();
        let path = scratch.join("p.tiled-project");
        let loaded = parse_project(text, &path).unwrap();
        let project = loaded.value;
        let class = |i: usize| project.custom_types.types[i].as_class().unwrap();
        // Inner is read first (Outer holds it) while Outer has no members yet, so Inner keeps
        // `back`; then Outer's `inner` closes the cycle and goes.
        assert!(!class(0).members.contains_key("inner"));
        assert!(class(1).members.contains_key("back"));
        let Value::Class(inner) = &class(3).members["inner"].value else {
            panic!()
        };
        assert_eq!(inner["hp"], Property::new(Value::Float(3.0)));
        assert_eq!(loaded.warnings.len(), 1);
        assert_eq!(loaded.warnings[0].kind, WarningKind::CircularClass);
        assert_eq!(project.folders[0].path, scratch.path());
        let written = project_text(&project, &path);
        assert!(written.contains("\"futureKey\": {\n        \"kept\": [\n"));
        let again = parse_project(&written, &path).unwrap().value;
        assert_eq!(again, project);
    }

    #[test]
    fn tileds_example_project_reads_and_writes_as_tiled_does() {
        let Some(path) = bt_test_data::test_data("reference/tiled/examples/examples.tiled-project")
        else {
            bt_test_data::missing(
                "reference/tiled/examples/examples.tiled-project",
                "run scripts/fetch-reference.sh",
            );
            return;
        };
        let original = std::fs::read_to_string(&path).unwrap();
        let project = load_project(&path).unwrap().value;
        // Tiled writes project properties even when there are none, and every class's
        // `drawFill`; the example predates both.
        let expected = original
            .replace(
                "    ],\n    \"propertyTypes\"",
                "    ],\n    \"properties\": [\n    ],\n    \"propertyTypes\"",
            )
            .replace(
                "\",\n            \"id\"",
                "\",\n            \"drawFill\": true,\n            \"id\"",
            );
        assert_eq!(project_text(&project, &path), expected);
    }
}
