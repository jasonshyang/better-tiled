//! The command palette's entries: the commands, the recent files, the map's layers and a go-to,
//! ranked by Tiled's search (`bt_utils::text::search_score`) with a bonus for what was used
//! recently and a penalty for what cannot run now. The window only draws the list.

use std::path::PathBuf;

use bt_core::{LayerId, LayerKind};
use bt_utils::text::search_score;
use glam::IVec2;

use super::enabled::Enabled;
use super::table::{ACTIONS, ActionId};
use crate::editor::Editor;
use crate::effect::{Effect, ViewRequest};

/// What a palette entry does.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PaletteTarget {
    /// Runs a project command.
    ProjectCommand(usize),
    /// Runs a command.
    Action(ActionId),
    /// Opens a file.
    File(PathBuf),
    /// Makes a layer of the current map current.
    Layer(LayerId),
    /// Centres the view on a cell.
    GoTo(IVec2),
}

/// An entry of the palette, as the window shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteEntry {
    /// What it does.
    pub target: PaletteTarget,
    /// What it is called.
    pub label: String,
    /// What is shown beside it: a key, a folder, a layer's kind.
    pub detail: String,
    /// Whether it can run now.
    pub enabled: bool,
    /// The characters of the label the search matched.
    pub matched: Vec<usize>,
}

/// What the palette remembers: the entries run last, most recent first.
#[derive(Clone, Debug, Default)]
pub(crate) struct PaletteState {
    recent: Vec<PaletteTarget>,
}

/// How many recently run entries get a bonus.
const RECENT: usize = 8;

/// A candidate before ranking.
struct Candidate {
    target: PaletteTarget,
    label: String,
    detail: String,
    enabled: bool,
}

/// An entry with its rank.
struct Ranked {
    rank: i64,
    entry: PaletteEntry,
}

impl Editor {
    /// The palette's entries for `query`, best first, at most `limit` of them. A query of two
    /// numbers ("12, 40") offers to go to that cell.
    pub fn palette(&self, query: &str, limit: usize) -> Vec<PaletteEntry> {
        if let Some(cell) = cell_in(query) {
            return self
                .situation()
                .map
                .map(|_| PaletteEntry {
                    target: PaletteTarget::GoTo(cell),
                    label: format!("Go to {}, {}", cell.x, cell.y),
                    detail: "cell".to_owned(),
                    enabled: true,
                    matched: Vec::new(),
                })
                .into_iter()
                .collect();
        }
        let words: Vec<&str> = query.split_whitespace().collect();
        let mut candidates = Vec::new();
        for (index, command) in self.project_commands().iter().enumerate() {
            candidates.push(Candidate {
                target: PaletteTarget::ProjectCommand(index),
                label: command.name.clone(),
                detail: command.shortcut.clone(),
                enabled: command.enabled && self.current().is_some(),
            });
        }
        for spec in ACTIONS.iter().filter(|s| s.id != ActionId::SearchActions) {
            candidates.push(Candidate {
                target: PaletteTarget::Action(spec.id),
                label: spec.label.trim_end_matches('…').to_owned(),
                detail: self.keymap.key_text(spec.id).unwrap_or_default(),
                enabled: self.enabled(spec.id) == Enabled::Enabled,
            });
        }
        for path in &self.session.recent_files {
            candidates.push(Candidate {
                target: PaletteTarget::File(path.clone()),
                label: bt_utils::paths::display_name(path),
                detail: path
                    .parent()
                    .map(|folder| folder.display().to_string())
                    .unwrap_or_default(),
                enabled: true,
            });
        }
        fn project_files(
            entries: &[crate::ProjectEntry],
            candidates: &mut Vec<Candidate>,
            known: &mut std::collections::HashSet<PathBuf>,
        ) {
            for entry in entries {
                if entry.openable() && known.insert(entry.path.clone()) {
                    candidates.push(Candidate {
                        target: PaletteTarget::File(entry.path.clone()),
                        label: bt_utils::paths::display_name(&entry.path),
                        detail: entry
                            .path
                            .parent()
                            .map_or_else(String::new, |folder| folder.display().to_string()),
                        enabled: true,
                    });
                }
                project_files(&entry.children, candidates, known);
            }
        }
        let mut known = self.session.recent_files.iter().cloned().collect();
        project_files(self.project_entries(), &mut candidates, &mut known);
        if let Some(map) = self.situation().map {
            map.for_each_layer(|layer, _| {
                let kind = match layer.kind {
                    LayerKind::Tile(_) => "tile layer",
                    LayerKind::Object(_) => "object layer",
                    LayerKind::Image(_) => "image layer",
                    LayerKind::Group(_) => "group",
                };
                candidates.push(Candidate {
                    target: PaletteTarget::Layer(layer.id),
                    label: layer.name.clone(),
                    detail: kind.to_owned(),
                    enabled: true,
                });
            });
        }
        let mut ranked: Vec<Ranked> = candidates
            .into_iter()
            .filter_map(|candidate| {
                let found = search_score(&words, &candidate.label)?;
                let recency = self
                    .palette
                    .recent
                    .iter()
                    .position(|t| *t == candidate.target)
                    .map_or(0, |i| (RECENT - i.min(RECENT)) as i64);
                let mut rank = i64::from(found.score) * 4;
                if !candidate.enabled {
                    rank /= 2;
                }
                rank += recency * 8;
                Some(Ranked {
                    rank,
                    entry: PaletteEntry {
                        target: candidate.target,
                        label: candidate.label,
                        detail: candidate.detail,
                        enabled: candidate.enabled,
                        matched: found.matched,
                    },
                })
            })
            .collect();
        // Equal scores put the shorter label first, the one the words cover more of: "snap to
        // grid" finds Snap to Grid before Snap to Fine Grid.
        ranked.sort_by(|a, b| {
            b.rank
                .cmp(&a.rank)
                .then_with(|| {
                    let length = |r: &Ranked| r.entry.label.chars().count();
                    length(a).cmp(&length(b))
                })
                .then_with(|| bt_utils::text::natural_cmp(&a.entry.label, &b.entry.label))
        });
        ranked.into_iter().take(limit).map(|r| r.entry).collect()
    }

    /// Runs a palette entry and remembers it.
    pub(crate) fn run_palette(&mut self, target: PaletteTarget) -> Vec<Effect> {
        self.palette.recent.retain(|t| *t != target);
        self.palette.recent.insert(0, target.clone());
        self.palette.recent.truncate(RECENT);
        match target {
            PaletteTarget::ProjectCommand(index) => self.run_project_command(index),
            PaletteTarget::Action(id) => self.run(id),
            PaletteTarget::File(path) => self.open(&path),
            PaletteTarget::Layer(layer) => {
                if let Some(doc) = self.documents.current() {
                    self.select_layer(doc, layer);
                }
                Vec::new()
            }
            PaletteTarget::GoTo(cell) => vec![Effect::View(ViewRequest::CenterOnCell(cell))],
        }
    }
}

/// The cell a query names: two whole numbers, separated by a comma, spaces or both.
fn cell_in(query: &str) -> Option<IVec2> {
    let mut numbers = query
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|part| !part.is_empty());
    let x = numbers.next()?.parse().ok()?;
    let y = numbers.next()?.parse().ok()?;
    numbers.next().is_none().then_some(IVec2::new(x, y))
}

#[cfg(test)]
mod tests {
    use bt_core::LayerId;
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::UVec2;

    use super::*;
    use crate::intent::Intent;

    fn editor() -> Editor {
        let atlas = Atlas::builder().build_embedded().unwrap();
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(4, 3))
            .tileset(atlas.tileset)
            .tile_layer("ground", "1 1 1 1\n1 1 1 1\n1 1 1 1")
            .object_layer("enemies", Vec::new())
            .build()
            .unwrap();
        let mut editor = Editor::new();
        editor.open_new_map(map);
        editor
    }

    #[test]
    fn commands_and_layers_rank_by_the_search() {
        let editor = editor();
        let found = editor.palette("save as", 5);
        assert_eq!(found[0].target, PaletteTarget::Action(ActionId::SaveAs));
        let layer = editor.palette("enem", 5);
        assert_eq!(layer[0].target, PaletteTarget::Layer(LayerId(2)));
        assert_eq!(layer[0].detail, "object layer");
        assert!(editor.palette("zzqq", 5).is_empty());
        // Words that score alike in both find the shorter label first.
        let snap = editor.palette("snap to grid", 2);
        assert_eq!(
            snap.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(),
            ["Snap to Grid", "Snap to Fine Grid"]
        );
        assert!(
            editor
                .palette("", 200)
                .iter()
                .all(|e| e.target != PaletteTarget::Action(ActionId::SearchActions)),
            "the palette does not list itself"
        );
    }

    #[test]
    fn a_pair_of_numbers_goes_to_a_cell() {
        let mut editor = editor();
        let found = editor.palette("12, 40", 5);
        assert_eq!(found[0].target, PaletteTarget::GoTo(IVec2::new(12, 40)));
        let effects = editor.apply(Intent::RunPalette(found[0].target.clone()));
        assert!(matches!(
            effects[..],
            [Effect::View(ViewRequest::CenterOnCell(cell))] if cell == IVec2::new(12, 40)
        ));
        assert_eq!(cell_in("3 -4"), Some(IVec2::new(3, -4)));
        assert_eq!(cell_in("3"), None);
        assert_eq!(cell_in("3, 4, 5"), None);
    }

    #[test]
    fn what_was_run_lately_comes_first() {
        let mut editor = editor();
        let ground = PaletteTarget::Layer(LayerId(1));
        assert_ne!(editor.palette("", 500)[0].target, ground);
        editor.apply(Intent::RunPalette(ground.clone()));
        assert_eq!(editor.palette("", 500)[0].target, ground);
    }
}
