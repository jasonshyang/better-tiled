//! The Explain tool's side of the editor: the cell chosen in each document and what the rules do
//! there. With AutoMap layers on, each run is explained on the stage it read, in the map's layer
//! names; right after AutoMap ran, with nothing changed since, that run is explained on the map
//! before it; else a run now on the map as it is (`08-automap.md`, §8: which run is explained is
//! the editor's choice). Explanations are made again only when what they explain changed.

use std::sync::Arc;

use bt_automap::layers::Setup;
use bt_automap::{Explanation, RuleSet, RunRequest};
use bt_core::Map;
use glam::IVec2;

use crate::document::DocumentId;
use crate::editor::Editor;
use crate::effect::Effect;

/// Which run an explanation is of.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ExplainedRun {
    /// Run `run` of AutoMap layers (from 0), on the stage it read.
    Live(usize),
    /// The last AutoMap run, on the map before it: nothing changed since.
    LastRun,
    /// A run of the rules now, on the map as it is.
    RunNow,
}

/// What the rules do at the cell the Explain tool chose, in one run.
#[derive(Clone, Debug)]
pub struct CellExplanation {
    /// The run explained.
    pub of: ExplainedRun,
    /// What the rules do there.
    pub explanation: Explanation,
    /// The map the run read: its tilesets name the cells of the explanation.
    pub map: Arc<Map>,
    /// The rules that ran: their tilesets name the tiles of the conditions.
    pub rules: Arc<RuleSet>,
}

/// The explanations of a document's chosen cell, with what they were made from.
#[derive(Clone, Debug)]
pub(crate) struct Explained {
    cell: IVec2,
    made_from: Source,
    explanations: Vec<CellExplanation>,
}

/// What explanations are made from: when any of it changes they are made again.
#[derive(Clone, Debug)]
struct Source {
    painted: Option<Arc<Map>>,
    rules: Option<Arc<RuleSet>>,
    setup: Option<Setup>,
    last_after: Option<Arc<Map>>,
    seed: u64,
}

impl Source {
    fn same_as(&self, other: &Source) -> bool {
        fn same<T>(a: &Option<Arc<T>>, b: &Option<Arc<T>>) -> bool {
            match (a, b) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
        }
        same(&self.painted, &other.painted)
            && same(&self.rules, &other.rules)
            && same(&self.last_after, &other.last_after)
            && self.setup == other.setup
            && self.seed == other.seed
    }
}

impl Editor {
    /// The cell the Explain tool chose in a document.
    pub fn explained_cell(&self, doc: DocumentId) -> Option<IVec2> {
        self.automap.explaining.get(&doc).copied()
    }

    /// What the rules do at the cell the Explain tool chose: one explanation per run of AutoMap
    /// layers, or one of the last run or of a run now; none until the rules are read.
    pub fn cell_explanations(&self, doc: DocumentId) -> &[CellExplanation] {
        self.automap
            .explained
            .get(&doc)
            .map_or(&[], |explained| explained.explanations.as_slice())
    }

    /// The Explain tool chose `cell`: the AutoMap panel explains it, with the map's rules read
    /// first when they are not yet.
    pub(crate) fn explain_cell(&mut self, doc: DocumentId, cell: IVec2) -> Vec<Effect> {
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        self.automap.explaining.insert(doc, cell);
        self.find_rules(asset, false)
    }

    /// Makes again the explanations whose cell, map, rules or setup changed.
    pub(crate) fn sync_explanations(&mut self) {
        let docs: Vec<DocumentId> = self.automap.explaining.keys().copied().collect();
        self.automap.explained.retain(|doc, _| docs.contains(doc));
        for doc in docs {
            let Some(cell) = self.explained_cell(doc) else {
                continue;
            };
            let Some(source) = self.explain_source(doc) else {
                self.automap.explained.remove(&doc);
                continue;
            };
            let fresh = self
                .automap
                .explained
                .get(&doc)
                .is_some_and(|e| e.cell == cell && e.made_from.same_as(&source));
            if fresh {
                continue;
            }
            let explanations = self.explain_at(doc, cell);
            self.automap.explained.insert(
                doc,
                Explained {
                    cell,
                    made_from: source,
                    explanations,
                },
            );
        }
    }

    /// What a document's explanations are made from now; `None` when it shows no map.
    fn explain_source(&self, doc: DocumentId) -> Option<Source> {
        let document = self.documents.get(doc)?;
        let rules = self.automap.maps.get(&document.asset());
        Some(Source {
            painted: Some(Arc::clone(self.store.map(document.asset())?)),
            rules: rules.and_then(|r| r.rules.clone()),
            setup: document.automap.clone(),
            last_after: rules
                .and_then(|r| r.last_run.as_ref())
                .map(|run| Arc::clone(&run.after)),
            seed: self.automap.seed,
        })
    }

    /// The explanations of `cell` in a document, as the module's doc says which.
    fn explain_at(&self, doc: DocumentId, cell: IVec2) -> Vec<CellExplanation> {
        let Some(document) = self.documents.get(doc) else {
            return Vec::new();
        };
        let Some(painted) = self.store.map(document.asset()) else {
            return Vec::new();
        };
        let rules = self.automap.maps.get(&document.asset());
        if let (Some(setup), Some(live)) = (&document.automap, self.automap.live.get(&doc)) {
            return setup
                .runs
                .iter()
                .enumerate()
                .filter_map(|(run, names)| {
                    let input = live.layers.input_of(run)?;
                    let request = RunRequest::builder(setup.run_seed(run))
                        .layer_names(names.clone())
                        .build();
                    Some(CellExplanation {
                        of: ExplainedRun::Live(run),
                        explanation: live
                            .rules
                            .explain_cell_with(input, cell, &request)
                            .with_layer_names(names, input),
                        map: Arc::clone(input),
                        rules: Arc::clone(&live.rules),
                    })
                })
                .collect();
        }
        if let Some(last) = rules
            .and_then(|r| r.last_run.as_ref())
            .filter(|last| Arc::ptr_eq(&last.after, painted))
        {
            return vec![CellExplanation {
                of: ExplainedRun::LastRun,
                explanation: last.rules.explain_cell(&last.before, cell, last.seed),
                map: Arc::clone(&last.before),
                rules: Arc::clone(&last.rules),
            }];
        }
        let Some(set) = rules.and_then(|r| r.rules.clone()) else {
            return Vec::new();
        };
        vec![CellExplanation {
            of: ExplainedRun::RunNow,
            explanation: set.explain_cell(painted, cell, self.automap.seed),
            map: Arc::clone(painted),
            rules: set,
        }]
    }
}
