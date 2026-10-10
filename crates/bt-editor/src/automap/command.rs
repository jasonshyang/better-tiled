//! Map ▸ AutoMap: the map's rules run over the selected cells, or over the whole map with none
//! selected (Tiled's `AutomappingManager::autoMap`), as one step, after the rules are read when
//! they are still being read. The notice says what the rules did, or why they did nothing.

use std::sync::Arc;

use bt_automap::{Area, Report, RuleSet, RunRequest, Severity as ProblemSeverity};
use bt_core::Map;
use bt_edit::TileRegion;
use bt_utils::rng::Rng;
use bt_utils::text::{and_list, plural};

use super::rules::LastRun;
use crate::document::DocumentId;
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;
use crate::notice::Notice;
use crate::store::{AssetId, NewAsset, Root};
use crate::watch::Found;

impl Editor {
    /// AutoMap on a document's map: over its selected cells, or the whole map with none.
    pub(crate) fn automap(&mut self, doc: DocumentId) -> Vec<Effect> {
        let Some(document) = self.documents.get(doc) else {
            return Vec::new();
        };
        let asset = document.asset();
        let area = selection_area(&document.selection().tiles);
        if self.store.map(asset).is_none() {
            return Vec::new();
        }
        let mut effects = self.find_rules(asset, false);
        let Some(rules) = self.automap.maps.get_mut(&asset) else {
            return effects;
        };
        if rules.file.is_none() {
            let name = self
                .store
                .get(asset)
                .map_or_else(String::new, |a| a.display_name());
            effects.push(Effect::Notify(Notice::warning(format!(
                "AutoMap has no rules for {name}: put a rules.txt beside the map, or name a \
                 rules file in the project's properties"
            ))));
            return effects;
        }
        if rules.reading.is_some() || rules.rules.is_none() {
            rules.waiting = Some(area);
            return effects;
        }
        effects.extend(self.run_rules(doc, area));
        effects
    }

    /// Runs a document's map's rules over `area` as one step.
    pub(crate) fn run_rules(&mut self, doc: DocumentId, area: Area) -> Vec<Effect> {
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let Some(rules) = self.automap.maps.get(&asset).and_then(|r| r.rules.clone()) else {
            return Vec::new();
        };
        let Some(before) = self.store.map(asset).cloned() else {
            return Vec::new();
        };
        let seed = self.automap.seed;
        let request = RunRequest::builder(seed).area(area.clone()).build();
        let result = self.transact(doc, "AutoMap", |tx| {
            let map = tx.map_mut(asset)?;
            Ok::<_, EditError>(rules.run(map, &request))
        });
        let report = match result {
            Ok(report) => report,
            Err(error) => return vec![warn(error)],
        };
        self.automap.seed = Rng::new(seed).next_u64();
        self.add_rule_tilesets(asset, &report);
        let notice = run_notice(&rules, &before, &report);
        let after = self
            .store
            .map(asset)
            .cloned()
            .unwrap_or_else(|| Arc::clone(&before));
        if let Some(entry) = self.automap.maps.get_mut(&asset) {
            entry.last_run = Some(LastRun {
                rules,
                area,
                seed,
                before,
                after,
                report,
            });
        }
        vec![Effect::Notify(notice)]
    }

    /// Makes the tileset files a run added to a map assets of the store, as Add External
    /// Tileset does, so they are watched, shown and saved as the map's others are.
    fn add_rule_tilesets(&mut self, asset: AssetId, report: &Report) {
        let Some(map) = self.store.map(asset).cloned() else {
            return;
        };
        let read = self
            .automap
            .maps
            .get(&asset)
            .map(|r| r.files.clone())
            .unwrap_or_default();
        for slot in &report.tilesets_added {
            let Some(reference) = map.tilesets.get(slot.index()) else {
                continue;
            };
            let Some(file) = &reference.file else {
                continue;
            };
            if !file.path.is_absolute() || self.store.by_path(&file.path).is_some() {
                continue;
            }
            let content_hash =
                read.iter()
                    .find(|f| f.path == file.path)
                    .and_then(|f| match f.found {
                        Found::Present { content_hash } => Some(content_hash),
                        Found::Missing => None,
                    });
            self.store.insert(NewAsset {
                root: Root::Tileset(Arc::clone(&reference.tileset)),
                path: Some(file.path.clone()),
                content_hash,
                warnings: Vec::new(),
            });
        }
    }
}

/// What a run covers: the selected cells, or the whole map with none selected.
fn selection_area(tiles: &TileRegion) -> Area {
    if tiles.is_empty() {
        Area::Whole
    } else {
        Area::Selection(tiles.rects())
    }
}

/// What a run did, or why it did nothing, as the status bar says it.
fn run_notice(rules: &RuleSet, before: &Map, report: &Report) -> Notice {
    let mut detail: Vec<String> = report.problems.iter().map(|p| p.text.clone()).collect();
    if report.matches > 0 {
        let applied = plural(report.matches, "match", "matches");
        let mut text = match report.changed {
            Some(_) => format!("AutoMap applied {applied}"),
            None => format!("AutoMap applied {applied}; the map had what they write already"),
        };
        let locked: Vec<String> = report
            .locked_outputs
            .iter()
            .map(|locked| locked.name.clone())
            .collect();
        if locked.is_empty() {
            return with_detail(Notice::info(text), detail);
        }
        let layers = if locked.len() == 1 { "layer" } else { "layers" };
        text.push_str(&format!(
            "; outputs were kept out of the locked {layers} {}",
            and_list(&locked)
        ));
        return with_detail(Notice::warning(text), detail);
    }
    let unused: Vec<&str> = rules
        .problems()
        .iter()
        .chain(rules.rule_maps().iter().flat_map(|r| r.problems()))
        .filter(|p| p.severity() == ProblemSeverity::Error)
        .map(|p| p.text.as_str())
        .collect();
    detail.extend(unused.iter().map(|text| (*text).to_owned()));
    let text = if rules.rule_count() == 0 {
        "AutoMap did nothing: the rules file holds no rules that can run".to_owned()
    } else {
        let missing: Vec<String> = rules
            .missing_layers(before)
            .into_iter()
            .map(|m| m.name)
            .collect();
        if missing.is_empty() {
            "AutoMap did nothing: no rule matched".to_owned()
        } else {
            let layers = if missing.len() == 1 {
                "layer"
            } else {
                "layers"
            };
            format!(
                "AutoMap did nothing: the map has no {layers} {} that the rules read",
                and_list(&missing)
            )
        }
    };
    with_detail(Notice::warning(text), detail)
}

fn with_detail(notice: Notice, detail: Vec<String>) -> Notice {
    if detail.is_empty() {
        notice
    } else {
        notice.with_detail(detail.join("\n"))
    }
}
