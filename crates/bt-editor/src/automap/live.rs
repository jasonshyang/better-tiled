//! AutoMap layers in the editor (`05-editor-core.md`, §16): a document's setup is part of every
//! transaction, the result is derived from what was painted after each change and shown in its
//! place, and starting, stopping, re-rolling and binding are steps of their own.

use std::sync::Arc;

use bt_automap::LayerNames;
use bt_automap::RuleSet;
use bt_automap::layers::{AutoLayers, Setup};
use bt_core::{LayerId, Map};
use bt_edit::{LayerType, new_layer};
use bt_utils::rng::Rng;
use glam::IVec2;

use crate::document::DocumentId;
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;
use crate::notice::Notice;

/// A change to a document's AutoMap layers, each one step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveEdit {
    /// Start them: the rules found for the map, each name they use bound to the map's layer of
    /// that name, output layers the map lacks made on top. Right after AutoMap ran, what was
    /// painted is the map before that run.
    Start,
    /// Stop them: the result becomes what was painted.
    Stop,
    /// Draw the rules' random choices from a new seed.
    Reroll,
    /// Bind `name` (a tile layer name of the rules, or with `objects` an object output) in run
    /// `run` to `layer`, or with `None` leave it a temporary layer.
    Bind {
        /// The run.
        run: usize,
        /// The name the rules use.
        name: String,
        /// Whether the name is of the rules' object outputs.
        objects: bool,
        /// The map layer, or none.
        layer: Option<LayerId>,
    },
    /// Add a run, bound as the last one is, to bind on other layers.
    AddRun,
    /// Remove a run.
    RemoveRun(usize),
}

impl LiveEdit {
    /// Its name in the History panel.
    fn label(&self) -> &'static str {
        match self {
            LiveEdit::Start => "Start AutoMap Layers",
            LiveEdit::Stop => "Stop AutoMap Layers",
            LiveEdit::Reroll => "Re-roll AutoMap Layers",
            LiveEdit::Bind { .. } => "Bind AutoMap Layer",
            LiveEdit::AddRun => "Add AutoMap Run",
            LiveEdit::RemoveRun(_) => "Remove AutoMap Run",
        }
    }
}

/// AutoMap layers' result for a document, with what it was derived from.
#[derive(Clone, Debug)]
pub(crate) struct LiveResult {
    pub(crate) layers: AutoLayers,
    pub(crate) setup: Setup,
    pub(crate) rules: Arc<RuleSet>,
}

/// A result to derive again: the document, and what to derive it from.
struct Derivation {
    doc: DocumentId,
    painted: Arc<Map>,
    setup: Setup,
    rules: Arc<RuleSet>,
}

impl Editor {
    /// What the canvas, Save and the exports take of a document's map: AutoMap layers' result
    /// while they are on and "Show what I painted" is off, else the map as painted.
    pub fn shown_map(&self, doc: DocumentId) -> Option<&Arc<Map>> {
        let document = self.documents.get(doc)?;
        let live = self
            .automap
            .live
            .get(&doc)
            .filter(|_| document.automap.is_some() && !document.view.show_painted);
        match live {
            Some(live) => Some(live.layers.result()),
            None => self.store.map(document.asset()),
        }
    }

    /// The map the rules' result is saved as: AutoMap layers' result while they are on,
    /// whatever is shown.
    pub(crate) fn result_map(&self, doc: DocumentId) -> Option<&Arc<Map>> {
        let document = self.documents.get(doc)?;
        match self.automap.live.get(&doc) {
            Some(live) if document.automap.is_some() => Some(live.layers.result()),
            _ => self.store.map(document.asset()),
        }
    }

    /// Whether the tile at `cell` of the tile layer `layer` is one the AutoMap rules made, not
    /// what was painted there; `None` while AutoMap layers are off.
    pub fn made_by_rules(&self, doc: DocumentId, layer: LayerId, cell: IVec2) -> Option<bool> {
        let live = self.live_layers(doc)?;
        let at = |map: &Map| {
            map.layer(layer)
                .and_then(|l| l.as_tile_layer())
                .map(|tiles| tiles.cell(cell))
        };
        Some(at(live.result()) != at(live.painted()))
    }

    /// AutoMap layers of a document, while they are on and their rules have run.
    pub fn live_layers(&self, doc: DocumentId) -> Option<&AutoLayers> {
        self.documents.get(doc)?.automap.as_ref()?;
        self.automap.live.get(&doc).map(|live| &live.layers)
    }

    /// Changes a document's AutoMap layers as one step.
    pub(crate) fn edit_live(&mut self, doc: DocumentId, edit: LiveEdit) -> Vec<Effect> {
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        if self.store.map(asset).is_none() {
            return Vec::new();
        }
        let label = edit.label();
        let result = match edit {
            LiveEdit::Start => return self.start_live(doc),
            LiveEdit::Stop => {
                let result = self.result_map(doc).cloned();
                self.transact(doc, label, |tx| {
                    if let Some(result) = result {
                        *tx.map_mut(asset)? = Map::clone(&result);
                    }
                    tx.automap = None;
                    Ok::<_, EditError>(())
                })
            }
            LiveEdit::Reroll => self.transact(doc, label, |tx| {
                if let Some(setup) = &mut tx.automap {
                    setup.seed = Rng::new(setup.seed).next_u64();
                }
                Ok::<_, EditError>(())
            }),
            LiveEdit::Bind {
                run,
                name,
                objects,
                layer,
            } => self.transact(doc, label, |tx| {
                if let Some(names) = tx.automap.as_mut().and_then(|s| s.runs.get_mut(run)) {
                    *names = names.rebound(&name, objects, layer);
                }
                Ok::<_, EditError>(())
            }),
            LiveEdit::AddRun => self.transact(doc, label, |tx| {
                if let Some(setup) = &mut tx.automap {
                    let names = setup
                        .runs
                        .last()
                        .cloned()
                        .unwrap_or_else(LayerNames::temporary);
                    setup.runs.push(names);
                }
                Ok::<_, EditError>(())
            }),
            LiveEdit::RemoveRun(run) => self.transact(doc, label, |tx| {
                if let Some(setup) = &mut tx.automap
                    && run < setup.runs.len()
                {
                    setup.runs.remove(run);
                }
                Ok::<_, EditError>(())
            }),
        };
        result.err().map(warn).into_iter().collect()
    }

    /// Starts AutoMap layers on a document's map, once its rules are read.
    pub(crate) fn start_live(&mut self, doc: DocumentId) -> Vec<Effect> {
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let mut effects = self.find_rules(asset, false);
        let Some(rules) = self.automap.maps.get_mut(&asset) else {
            return effects;
        };
        let Some(file) = rules.file.clone() else {
            effects.push(Effect::Notify(Notice::warning(
                "AutoMap layers need rules: put a rules.txt beside the map, or name a rules file \
                 in the project's properties",
            )));
            return effects;
        };
        let Some(set) = rules.rules.clone().filter(|_| rules.reading.is_none()) else {
            rules.waiting_start = true;
            return effects;
        };
        // Right after AutoMap ran, what was painted is the map before that run.
        let current = self.store.map(asset).cloned();
        let before_run = rules
            .last_run
            .as_ref()
            .filter(|run| {
                current
                    .as_ref()
                    .is_some_and(|map| Arc::ptr_eq(map, &run.after))
            })
            .map(|run| Arc::clone(&run.before));
        let seed = self.automap.seed;
        self.automap.seed = Rng::new(seed).next_u64();
        let rule_layers = set.rule_layers();
        let result = self.transact(doc, LiveEdit::Start.label(), |tx| {
            let map = tx.map_mut(asset)?;
            if let Some(before) = before_run {
                *map = Map::clone(&before);
            }
            add_missing_outputs(map, &rule_layers.tile_outputs, LayerType::Tile)?;
            add_missing_outputs(map, &rule_layers.object_outputs, LayerType::Object)?;
            tx.automap = Some(Setup {
                rules: file,
                runs: vec![LayerNames::found_in(tx.map(asset)?, &rule_layers)],
                seed,
            });
            Ok::<_, EditError>(())
        });
        effects.extend(result.err().map(warn));
        effects
    }

    /// Derives AutoMap layers' result again for each document whose painted map, setup or rules
    /// changed; a document whose rules are being read keeps its last result until they arrive.
    /// A map opened with AutoMap layers whose first result is not what its file holds (the
    /// rules changed since) becomes unsaved, and says so.
    pub(crate) fn sync_live(&mut self) -> Vec<Effect> {
        let mut stale = Vec::new();
        for document in self.documents.in_order() {
            let Some(setup) = &document.automap else {
                continue;
            };
            let Some(painted) = self.store.map(document.asset()) else {
                continue;
            };
            let Some(rules) = self
                .automap
                .maps
                .get(&document.asset())
                .and_then(|r| r.rules.as_ref())
            else {
                continue;
            };
            let fresh = self.automap.live.get(&document.id()).is_some_and(|live| {
                Arc::ptr_eq(live.layers.painted(), painted)
                    && live.setup == *setup
                    && Arc::ptr_eq(&live.rules, rules)
            });
            if !fresh {
                stale.push(Derivation {
                    doc: document.id(),
                    painted: Arc::clone(painted),
                    setup: setup.clone(),
                    rules: Arc::clone(rules),
                });
            }
        }
        let on: Vec<DocumentId> = self
            .documents
            .in_order()
            .filter(|d| d.automap.is_some())
            .map(|d| d.id())
            .collect();
        self.automap.live.retain(|doc, _| on.contains(doc));
        self.automap.saved.retain(|doc, _| on.contains(doc));
        let mut effects = Vec::new();
        for derivation in stale {
            let previous = self
                .automap
                .live
                .get(&derivation.doc)
                .map(|live| &live.layers);
            let layers = AutoLayers::update(
                previous,
                derivation.painted,
                &derivation.setup,
                &derivation.rules,
            );
            if let Some(saved) = self.automap.saved.remove(&derivation.doc)
                && **layers.result() != *saved
            {
                effects.extend(self.mark_rules_changed(derivation.doc));
            }
            self.automap.live.insert(
                derivation.doc,
                LiveResult {
                    layers,
                    setup: derivation.setup,
                    rules: derivation.rules,
                },
            );
        }
        effects
    }

    /// The map of a document opened with AutoMap layers holds what its rules no longer make:
    /// it takes a new version, so it is unsaved, and the user is told.
    fn mark_rules_changed(&mut self, doc: DocumentId) -> Vec<Effect> {
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let Some(root) = self.store.get(asset).map(|a| a.root().clone()) else {
            return Vec::new();
        };
        let name = self
            .store
            .get(asset)
            .map_or_else(String::new, |a| a.display_name());
        let version = self.store.next_version(asset);
        self.store.set_root(asset, root, version);
        vec![Effect::Notify(Notice::info(format!(
            "The AutoMap rules now make something other than what {name} holds: saving keeps \
             what they make now"
        )))]
    }
}

/// Makes a layer on top of the map for each of `names` the map has no layer of `kind` for.
fn add_missing_outputs(map: &mut Map, names: &[String], kind: LayerType) -> Result<(), EditError> {
    for name in names {
        let mut exists = false;
        map.for_each_layer(|layer, _| {
            exists |= layer.name == *name && LayerType::of(layer) == kind;
        });
        if exists {
            continue;
        }
        let top = map.layers.last().map(|layer| layer.id);
        let id = new_layer(map, kind, top)?;
        if let Some(layer) = map.layer_mut(id) {
            layer.name = name.clone();
        }
    }
    Ok(())
}
