//! `bt info`, `bt at` and `bt objects`: what a map is made of (`10-agents.md`, §10.1).

use anyhow::Context as _;
use bt_automap::RuleSet;
use bt_core::{LayerId, Map, ObjectId};
use bt_script::{
    CellOutcome, EditorCopy, InfoOutcome, InfoRequest, MapRules, Names, ObjectsOutcome,
    ObjectsRequest, parse_cell, parse_region,
};
use bt_utils::paths::display_name;

use crate::cli::{AtArgs, MapArg, ObjectsArgs};
use crate::commands::layers::layer_ref;
use crate::exit::{Printed, Status};
use crate::load::{LoadedMap, MapSource, load_map};
use crate::output::printed;
use crate::run::Run;

/// Runs `bt info`.
pub(crate) fn info(run: &Run, args: &MapArg) -> anyhow::Result<Printed> {
    let mut loaded = load_map(&args.map, run)?;
    let names = Names::of(&loaded.map);
    let editor = match loaded.source {
        MapSource::Editor { .. } => Some(EditorCopy::Unsaved),
        MapSource::File => run.link().open_map(&loaded.path).map(|_| EditorCopy::Saved),
    };
    let warnings: Vec<String> = loaded.warnings.iter().map(ToString::to_string).collect();
    let rules = read_rules(&mut loaded);
    let mut request = InfoRequest::new(&loaded.map, &names, display_name(&args.map))
        .warnings(warnings)
        .custom_types(std::sync::Arc::clone(loaded.loader.custom_types()));
    if let Some(copy) = editor {
        request = request.editor(copy);
    }
    let rules_ref = rules.as_ref().map(|found| match &found.rules {
        Ok(set) => Ok(set),
        Err(reason) => Err(reason.clone()),
    });
    match (&loaded.layers, &rules, rules_ref) {
        (Some(layers), _, Some(rules)) => {
            request = request.rules(MapRules::Layers {
                setup: &layers.setup,
                rules,
                notes: &layers.notes,
            });
        }
        (None, Some(found), Some(rules)) => {
            request = request.rules(MapRules::File {
                path: &found.path,
                rules,
            });
        }
        _ => {}
    }
    let outcome = InfoOutcome::from_request(&request);
    printed(&outcome, run.cli.json, Status::Done)
}

/// The rules that apply to a map: its AutoMap layers', else the rules file Tiled would find.
pub(crate) struct FoundRules {
    /// The rules file.
    pub(crate) path: std::path::PathBuf,
    /// Its rules, or why they can't be read.
    pub(crate) rules: Result<RuleSet, String>,
}

/// Reads the rules of the loaded map, through its loader so tilesets are shared; none when the
/// map has no AutoMap layers, no `rules.txt` beside it and no project naming a rules file.
pub(crate) fn read_rules(loaded: &mut LoadedMap) -> Option<FoundRules> {
    let chosen = loaded
        .layers
        .as_ref()
        .map(|layers| layers.setup.rules.clone());
    let path = bt_automap::files::find_rules(
        Some(&loaded.path),
        chosen.as_deref(),
        loaded.project.as_deref(),
    )?;
    let rules = bt_automap::files::load(&mut loaded.loader, &path, Some(&loaded.path))
        .map_err(|error| error.to_string());
    Some(FoundRules { path, rules })
}

/// The map a command reads cells of: what was painted with `--painted` on a map with AutoMap
/// layers, else the map as shown.
pub(crate) fn shown_or_painted(loaded: &LoadedMap, painted: bool) -> anyhow::Result<&Map> {
    if !painted {
        return Ok(&loaded.map);
    }
    if let MapSource::Editor { .. } = loaded.source {
        anyhow::bail!(
            "{} has unsaved changes in the editor: bt save MAP first, then --painted reads what \
             was painted",
            display_name(&loaded.path)
        );
    }
    loaded
        .layers
        .as_ref()
        .map(|layers| &layers.painted)
        .with_context(|| {
            format!(
                "{} has no AutoMap layers: what was painted is what it shows",
                display_name(&loaded.path)
            )
        })
}

/// Runs `bt at`.
pub(crate) fn at(run: &Run, args: &AtArgs) -> anyhow::Result<Printed> {
    let loaded = load_map(&args.map, run)?;
    let map = shown_or_painted(&loaded, args.painted)?;
    let cell = parse_cell(&args.cell).map_err(anyhow::Error::msg)?;
    let names = Names::of(map);
    let outcome = CellOutcome::at(map, &names, cell);
    printed(&outcome, run.cli.json, Status::Done)
}

/// Runs `bt objects`.
pub(crate) fn objects(run: &Run, args: &ObjectsArgs) -> anyhow::Result<Printed> {
    let loaded = load_map(&args.map, run)?;
    let map = &loaded.map;
    let names = Names::of(map);
    let mut request = ObjectsRequest::new(map, &names);
    if let Some(kind) = &args.kind {
        request = request.of_type(kind.clone());
    }
    if let Some(layer) = &args.layer {
        let id: LayerId = names.object_layer(map, &layer_ref(layer))?;
        request = request.on_layer(id);
    }
    if !args.within.is_empty() {
        let region = parse_region(&args.within.join(" ")).map_err(anyhow::Error::msg)?;
        request = request.within(region);
    }
    if let Some(name) = &args.name {
        request = request.named(name.clone());
    }
    if let Some(layers) = &loaded.layers {
        request = request.placed_by_rules(placed_by_rules(&layers.painted, map));
    }
    let outcome = ObjectsOutcome::from_request(&request);
    printed(&outcome, run.cli.json, Status::Done)
}

/// The objects of `shown` that were not painted: the AutoMap rules placed them.
fn placed_by_rules(painted: &Map, shown: &Map) -> Vec<ObjectId> {
    let mut painted_ids = std::collections::HashSet::new();
    painted.for_each_layer(|layer, _| {
        if let Some(objects) = layer.as_object_layer() {
            painted_ids.extend(objects.objects.iter().map(|o| o.id));
        }
    });
    let mut placed = Vec::new();
    shown.for_each_layer(|layer, _| {
        if let Some(objects) = layer.as_object_layer() {
            placed.extend(
                objects
                    .objects
                    .iter()
                    .map(|o| o.id)
                    .filter(|id| !painted_ids.contains(id)),
            );
        }
    });
    placed
}
