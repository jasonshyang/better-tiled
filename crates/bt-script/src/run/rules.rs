//! Rules (`10-agents.md`, §8.6, §8.7). `automap` reads the rules file given, else the `rules.txt`
//! beside the map, through the map's loader (or takes the rules the caller read from that file),
//! binds `RULE-LAYER=MAP-LAYER` pairs and applies the rules once as Tiled would. Before it runs
//! it says what would keep the rules from doing anything: rule maps' errors and warnings, rule
//! maps whose tilesets are copies of the map's, input layers the map lacks (with a near-miss
//! name, since names must match exactly) or has empty; after, how many matches it applied. On a
//! map with AutoMap layers the step is a note: the rules run anyway.
//!
//! `automap layers` starts (or starts over), adds a run, re-seeds or stops the map's AutoMap
//! layers; `off` bakes their result into what was painted. Output layers the map lacks are made
//! and named.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_automap::layers::{AutoLayers, RuleLayers, Setup};
use bt_automap::{LayerNames, RuleSet, RunRequest, Severity};
use bt_core::{Layer, LayerId, LayerKind, Map};
use bt_edit::{LayerType, new_layer};
use bt_utils::text::{and_list, plural, slug};

use super::error::RunFailure;
use super::runner::{AutoState, Runner};
use crate::script::{Binding, LayersOp};

impl Runner<'_> {
    /// `automap [RULES] [RULE-LAYER=MAP-LAYER …]`.
    pub(super) fn automap(
        &mut self,
        rules: Option<&str>,
        bindings: &[Binding],
    ) -> Result<(), RunFailure> {
        if self.auto.is_some() {
            if rules.is_some() || !bindings.is_empty() {
                return Err(RunFailure::refused(
                    "the map has AutoMap layers, whose rules run after every edit on what was \
                     painted: to run them on other layers too, automap layers add \
                     RULE-LAYER=MAP-LAYER …",
                ));
            }
            self.note(
                "automap: the map has AutoMap layers, whose rules run after every edit anyway",
            );
            return Ok(());
        }
        let (_, set) = self.rules_for(rules)?;
        let rule_layers = set.rule_layers();
        let mut names = LayerNames::builder();
        for binding in bindings {
            let bound = self.bound(&rule_layers, binding)?;
            names = if bound.objects {
                names.objects(binding.rule_layer.clone(), bound.layer)
            } else {
                names.tiles(binding.rule_layer.clone(), bound.layer)
            };
        }
        let names = names.build();
        self.note_rule_problems(&set, "automap");
        let way = match rules {
            Some(rules) => format!("automap {rules}"),
            None => "automap".to_owned(),
        };
        self.note_inputs(&set, &rule_layers, &names, &way);
        let request = RunRequest::builder(self.op_seed())
            .layer_names(names)
            .build();
        let report = set.run(&mut self.map, &request);
        for problem in &report.problems {
            self.note(format!("automap: {}", problem.text));
        }
        let made: Vec<String> = report
            .created_layers
            .iter()
            .filter_map(|&id| self.map.layer(id))
            .map(|layer| layer.name.clone())
            .collect();
        if !made.is_empty() {
            self.note(format!(
                "automap: new {} for the rules' outputs: {}",
                if made.len() == 1 { "layer" } else { "layers" },
                and_list(&made)
            ));
        }
        self.note(format!(
            "automap: {}",
            plural(report.matches, "rule match", "rule matches")
        ));
        Ok(())
    }

    /// `automap layers …`.
    pub(super) fn automap_layers(&mut self, op: &LayersOp) -> Result<(), RunFailure> {
        let none = || {
            RunFailure::refused(
                "the map has no AutoMap layers: start them with automap layers [RULES] \
                 [RULE-LAYER=MAP-LAYER …]",
            )
        };
        match op {
            LayersOp::Start { rules, bindings } => {
                let (path, set) = self.rules_for(rules.as_deref())?;
                let rule_layers = set.rule_layers();
                if rule_layers.inputs.is_empty()
                    && rule_layers.tile_outputs.is_empty()
                    && rule_layers.object_outputs.is_empty()
                {
                    return Err(RunFailure::refused(format!(
                        "{} has no rules that run",
                        bt_utils::paths::display_name(&path)
                    )));
                }
                self.note_rule_problems(&set, "automap layers");
                let run = self.layers_run(&rule_layers, bindings)?;
                let restarting = self.auto.is_some();
                let seed = match (&self.auto, self.own_seed()) {
                    (_, Some(seed)) => seed,
                    (Some(auto), None) => auto.setup.seed,
                    (None, None) => self.op_seed(),
                };
                let setup = Setup {
                    rules: path,
                    runs: vec![run],
                    seed,
                };
                self.refresh_shown();
                let previous = self.auto.take().map(|auto| auto.layers);
                let layers =
                    AutoLayers::update(previous.as_ref(), Arc::new(self.map.clone()), &setup, &set);
                self.auto = Some(AutoState {
                    setup,
                    rules: set,
                    layers,
                });
                self.auto_changed = true;
                if restarting {
                    self.note("automap layers: started over, with one run");
                }
                let runs = self.runs_text();
                self.note(format!(
                    "automap layers: on ({runs}); the rules run after every edit, on what was \
                     painted, and the file gets what they make"
                ));
            }
            LayersOp::Add { bindings } => {
                let rule_layers = self.auto.as_ref().ok_or_else(none)?.rules.rule_layers();
                let run = self.layers_run(&rule_layers, bindings)?;
                self.refresh_shown();
                if let Some(auto) = &mut self.auto {
                    auto.setup.runs.push(run);
                    auto.layers = AutoLayers::update(
                        Some(&auto.layers),
                        Arc::new(self.map.clone()),
                        &auto.setup,
                        &auto.rules,
                    );
                }
                self.auto_changed = true;
                let runs = self.runs_text();
                self.note(format!("automap layers: {runs}"));
            }
            LayersOp::Seed(seed) => {
                self.refresh_shown();
                let auto = self.auto.as_mut().ok_or_else(none)?;
                auto.setup.seed = *seed;
                auto.layers = AutoLayers::update(
                    Some(&auto.layers),
                    Arc::new(self.map.clone()),
                    &auto.setup,
                    &auto.rules,
                );
                self.auto_changed = true;
            }
            LayersOp::Off => {
                self.refresh_shown();
                let auto = self.auto.take().ok_or_else(none)?;
                self.map = Map::clone(auto.layers.result());
                self.auto_changed = true;
                self.names.rebuild_tables(&self.map);
                self.note("automap layers: off; what the rules made stays, as painted tiles");
            }
        }
        Ok(())
    }

    /// The rules a step names (from the map's folder), else the `rules.txt` beside the map: the
    /// rules the caller read from that file, or read through the map's loader.
    fn rules_for(&mut self, written: Option<&str>) -> Result<(PathBuf, Arc<RuleSet>), RunFailure> {
        let path = match written {
            Some(written) => self.resolve(written),
            None => {
                let beside = self.folder().join("rules.txt");
                if !beside.is_file() {
                    return Err(RunFailure::refused(
                        "there is no rules.txt beside the map: name the rules, e.g. automap \
                         ../Rules/rules.txt",
                    ));
                }
                beside
            }
        };
        if !path.exists() {
            return Err(RunFailure::refused(format!(
                "there are no rules at {}",
                path.display()
            )));
        }
        if let Some(read) = &self.rules_read
            && same_file(&read.path, &path)
        {
            return Ok((path, Arc::clone(&read.rules)));
        }
        let set = bt_automap::files::load(self.loader, &path, Some(&self.path))
            .map_err(|error| RunFailure::Rules(error.to_string()))?;
        Ok((path, Arc::new(set)))
    }

    /// A binding checked: the rules use the name, and the map's layer is of the kind they use it
    /// as.
    fn bound(&self, rule_layers: &RuleLayers, binding: &Binding) -> Result<Bound, RunFailure> {
        let layer = self.names.layer(&self.map, &binding.map_layer)?;
        let objects = match self.map.layer(layer).map(|l| &l.kind) {
            Some(LayerKind::Tile(_)) => false,
            Some(LayerKind::Object(_)) => true,
            _ => {
                return Err(RunFailure::refused(format!(
                    "{}: the rules read and write tile and object layers",
                    layer_text(&self.map, layer)
                )));
            }
        };
        let name = binding.rule_layer.as_str();
        let as_tiles = rule_layers.inputs.iter().any(|n| n == name)
            || rule_layers.tile_outputs.iter().any(|n| n == name);
        let as_objects = rule_layers.object_outputs.iter().any(|n| n == name);
        if !as_tiles && !as_objects {
            let mut known: Vec<&str> = rule_layers
                .inputs
                .iter()
                .chain(&rule_layers.tile_outputs)
                .chain(&rule_layers.object_outputs)
                .map(String::as_str)
                .collect();
            known.sort_unstable();
            known.dedup();
            return Err(RunFailure::refused(format!(
                "the rules don't name a layer “{name}” (they name {})",
                known.join(", ")
            )));
        }
        if objects && !as_objects || !objects && !as_tiles {
            return Err(RunFailure::refused(format!(
                "{} is {}, and the rules' “{name}” is {}",
                layer_text(&self.map, layer),
                if objects {
                    "an object layer"
                } else {
                    "a tile layer"
                },
                if as_objects {
                    "an object layer"
                } else {
                    "a tile layer"
                }
            )));
        }
        Ok(Bound { layer, objects })
    }

    /// A run of AutoMap layers: every name the rules use bound to the map's layer of that name,
    /// the bindings given in place of those, and the output layers no layer stands for made on
    /// top, named in a note.
    fn layers_run(
        &mut self,
        rule_layers: &RuleLayers,
        bindings: &[Binding],
    ) -> Result<LayerNames, RunFailure> {
        let mut bound = Vec::new();
        for binding in bindings {
            bound.push((
                binding.rule_layer.clone(),
                self.bound(rule_layers, binding)?,
            ));
        }
        let is_bound = |name: &str, objects: bool| {
            bound
                .iter()
                .any(|(rule, b)| rule == name && b.objects == objects)
        };
        let mut made = Vec::new();
        for (names, kind, objects) in [
            (&rule_layers.tile_outputs, LayerType::Tile, false),
            (&rule_layers.object_outputs, LayerType::Object, true),
        ] {
            for name in names {
                if is_bound(name, objects) || layer_of_name(&self.map, name, objects).is_some() {
                    continue;
                }
                let top = self.map.layers.last().map(|layer| layer.id);
                let id = new_layer(&mut self.map, kind, top)?;
                if let Some(layer) = self.map.layer_mut(id) {
                    layer.name = name.clone();
                }
                made.push(name.clone());
            }
        }
        if !made.is_empty() {
            self.note(format!(
                "automap layers: new {} for the rules to write: {}",
                if made.len() == 1 { "layer" } else { "layers" },
                and_list(&made)
            ));
        }
        let mut names = LayerNames::found_in(&self.map, rule_layers);
        for (rule, b) in &bound {
            names = names.rebound(rule, b.objects, Some(b.layer));
        }
        let loose: Vec<String> = rule_layers
            .inputs
            .iter()
            .filter(|name| !rule_layers.made.contains(name) && names.tile_layer(name).is_none())
            .cloned()
            .collect();
        for name in loose {
            self.note(format!(
                "automap layers: no layer stands for “{name}”, which the rules read: it reads as \
                 empty (say which: automap layers {name}=LAYER)"
            ));
        }
        Ok(names)
    }

    /// The AutoMap layers' runs in words: `2 runs: walls→plat1; walls→plat2`.
    fn runs_text(&self) -> String {
        let Some(auto) = &self.auto else {
            return String::new();
        };
        let map = &self.map;
        let runs: Vec<String> = auto
            .setup
            .runs
            .iter()
            .map(|run| {
                let bound: Vec<String> = run
                    .tile_bindings()
                    .iter()
                    .chain(run.object_bindings())
                    .filter_map(|binding| {
                        let layer = map.layer(binding.layer)?;
                        (layer.name != binding.name)
                            .then(|| format!("{}→{}", binding.name, layer.name))
                    })
                    .collect();
                if bound.is_empty() {
                    "on the layers of the rules' names".to_owned()
                } else {
                    bound.join(", ")
                }
            })
            .collect();
        format!("{}: {}", plural(runs.len(), "run", "runs"), runs.join("; "))
    }

    /// Notes the rule maps' errors and warnings, and a set with nothing to run.
    fn note_rule_problems(&mut self, set: &RuleSet, verb: &str) {
        let texts: Vec<String> = set
            .problems()
            .iter()
            .chain(set.rule_maps().iter().flat_map(|r| r.problems()))
            .filter(|p| p.severity() != Severity::Note)
            .map(|p| p.text.clone())
            .collect();
        for text in texts {
            self.note(format!("{verb}: {text}"));
        }
        if set.rule_count() == 0 {
            self.note(format!(
                "{verb}: nothing to run: the rules file lists no rule map with rules for this map \
                 (or its filters leave them all out)"
            ));
        }
        for copy in set.tileset_copies(&self.map) {
            self.note(format!("{verb}: {}", copy.problem().text));
        }
    }

    /// Notes the inputs the map lacks (with a near miss) or has empty, which keep rules from
    /// matching. `way` is how the step was written, for the way out.
    fn note_inputs(
        &mut self,
        set: &RuleSet,
        rule_layers: &RuleLayers,
        names: &LayerNames,
        way: &str,
    ) {
        let missing: Vec<String> = set
            .missing_layers(&self.map)
            .into_iter()
            .map(|m| m.name)
            .filter(|name| names.tile_layer(name).is_none())
            .collect();
        let read: Vec<&String> = rule_layers
            .inputs
            .iter()
            .filter(|name| !rule_layers.made.contains(name))
            .collect();
        if !missing.is_empty() && missing.len() == read.len() {
            let (layers, which) = if missing.len() == 1 {
                ("layer", "it")
            } else {
                ("layers", "each")
            };
            let first = missing.first().map_or("", String::as_str);
            self.note(format!(
                "automap: the rules read the {layers} {}, which this map doesn't have (say which \
                 layer {which} stands for: {way} {first}=L)",
                and_list(&missing)
            ));
        } else {
            for name in &missing {
                self.note(format!(
                    "automap: the rules read the layer {name}, which this map doesn't have: it \
                     reads as empty (say which layer it stands for: {way} {name}=L)"
                ));
            }
        }
        for name in &missing {
            if let Some(other) = near_miss(&self.map, name) {
                self.note(format!(
                    "automap: the map has a layer {other}, not {name} (names must match exactly)"
                ));
            }
        }
        for name in read {
            if missing.contains(name) {
                continue;
            }
            let layer = names
                .tile_layer(name)
                .or_else(|| layer_of_name(&self.map, name, false));
            let Some(id) = layer else {
                continue;
            };
            let empty = self
                .map
                .layer(id)
                .and_then(Layer::as_tile_layer)
                .is_some_and(|tiles| tiles.cells().next().is_none());
            if empty {
                let shown = layer_text(&self.map, id);
                let which = if shown == *name {
                    format!("the layer {name}")
                } else {
                    format!("{shown} (for {name})")
                };
                self.note(format!("automap: the rules read {which}, which is empty"));
            }
        }
    }
}

/// A map layer a rules' name is bound to, and whether it holds objects.
struct Bound {
    layer: LayerId,
    objects: bool,
}

/// The bottom-most layer of the kind called exactly `name`, as Tiled finds layers.
fn layer_of_name(map: &Map, name: &str, objects: bool) -> Option<LayerId> {
    let mut found = None;
    map.for_each_layer(|layer, _| {
        let fits = match layer.kind {
            LayerKind::Tile(_) => !objects,
            LayerKind::Object(_) => objects,
            LayerKind::Image(_) | LayerKind::Group(_) => false,
        };
        if found.is_none() && fits && layer.name == name {
            found = Some(layer.id);
        }
    });
    found
}

/// A tile layer whose name differs from `name` only in case or separators.
fn near_miss(map: &Map, name: &str) -> Option<String> {
    let wanted = slug(name);
    let mut found = None;
    map.for_each_layer(|layer, _| {
        if found.is_none()
            && layer.as_tile_layer().is_some()
            && layer.name != name
            && slug(&layer.name) == wanted
        {
            found = Some(layer.name.clone());
        }
    });
    found
}

/// A layer's name.
fn layer_text(map: &Map, layer: LayerId) -> String {
    map.layer(layer)
        .map_or_else(String::new, |l| l.name.clone())
}

/// Whether two paths are one file.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => bt_utils::paths::clean_path(a) == bt_utils::paths::clean_path(b),
    }
}
