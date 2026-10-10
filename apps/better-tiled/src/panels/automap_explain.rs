//! The AutoMap panel's Explain view: what the rules do at the cell the Explain tool chose. For
//! each run explained: every layer the rules write, before and after; the writes in order; the
//! rules applied there; the rules that came close (a condition naming the tiles here holds), each
//! with the conditions that failed; how many others could write the cell; and the tiles no rule
//! names, which the rules see only as "a tile".

use bt_automap::{
    AttemptOutcome, Condition, ConditionCheck, Explanation, MatchCell, RuleAttempt, RuleRef,
    RuleSet, Writer,
};
use bt_core::{Cell, Flip, Map};
use bt_editor::{CellExplanation, DocumentId, Editor, ExplainedRun};
use bt_utils::text::{and_list, plural};

use super::problem_line::problem_line;

/// Draws the explanations of `doc`'s chosen cell.
pub(crate) fn show_explanations(ui: &mut egui::Ui, editor: &Editor, doc: DocumentId) {
    let explanations = editor.cell_explanations(doc);
    if explanations.is_empty() {
        ui.weak("Nothing to explain until the map's rules are read.");
        return;
    }
    for (index, explained) in explanations.iter().enumerate() {
        let title = match explained.of {
            ExplainedRun::Live(run) => format!("Run {} of Live AutoMap", run + 1),
            ExplainedRun::LastRun => "The last AutoMap run".to_owned(),
            ExplainedRun::RunNow => "If AutoMap ran now".to_owned(),
        };
        egui::CollapsingHeader::new(title)
            .id_salt(("automap explanation", index))
            .default_open(true)
            .show(ui, |ui| show_explanation(ui, explained));
    }
}

fn show_explanation(ui: &mut egui::Ui, explained: &CellExplanation) {
    let words = Words {
        map: &explained.map,
        rules: &explained.rules,
        explanation: &explained.explanation,
    };
    let explanation = &explained.explanation;
    for problem in &explanation.problems {
        problem_line(ui, problem);
    }
    for layer in &explanation.layers {
        let gone = if layer.present {
            ""
        } else {
            " (made by the run)"
        };
        let changed = if layer.before == layer.after {
            "stays".to_owned()
        } else {
            format!("→ {}", words.cell(layer.after))
        };
        ui.label(format!(
            "{}{gone}: {} {changed}",
            layer.layer,
            words.cell(layer.before)
        ));
    }
    if !explanation.writes.is_empty() {
        ui.weak("Written, in order:");
        ui.indent("writes", |ui| {
            for write in &explanation.writes {
                let by = match write.by {
                    Writer::Rule { rule, position } => {
                        format!("{} at {}, {}", words.rule(rule), position.x, position.y)
                    }
                    Writer::DeleteTiles { rule_map } => {
                        format!("DeleteTiles of {}", words.rule_map(rule_map))
                    }
                };
                ui.label(format!(
                    "{}: {} → {} by {by}",
                    write.layer,
                    words.cell(write.before),
                    words.cell(write.after)
                ));
            }
        });
    }
    let applied: Vec<&RuleAttempt> = explanation
        .attempts
        .iter()
        .filter(|a| matches!(a.outcome, AttemptOutcome::Applied { .. }))
        .collect();
    let close: Vec<&RuleAttempt> = explanation
        .attempts
        .iter()
        .filter(|a| !a.matched && a.outcome == AttemptOutcome::NoMatch && a.holds_a_tile())
        .collect();
    let held_back: Vec<&RuleAttempt> = explanation
        .attempts
        .iter()
        .filter(|a| {
            a.matched
                && matches!(
                    a.outcome,
                    AttemptOutcome::SkippedByProbability | AttemptOutcome::Overlapping
                )
        })
        .collect();
    for attempt in &applied {
        ui.label(format!(
            "✔ {} applied at {}, {}",
            words.rule(attempt.rule),
            attempt.position.x,
            attempt.position.y
        ));
    }
    for attempt in &held_back {
        let why = match attempt.outcome {
            AttemptOutcome::SkippedByProbability => "its Probability skipped it",
            _ => "its output would overlap its own (NoOverlappingOutput)",
        };
        ui.label(format!(
            "◐ {} matched at {}, {}, but {why}",
            words.rule(attempt.rule),
            attempt.position.x,
            attempt.position.y
        ));
    }
    if !close.is_empty() {
        egui::CollapsingHeader::new(format!(
            "Came close: {}",
            plural(close.len(), "rule", "rules")
        ))
        .id_salt(("automap close", explanation.cell.x, explanation.cell.y))
        .default_open(applied.is_empty())
        .show(ui, |ui| {
            for attempt in &close {
                ui.label(format!(
                    "{} at {}, {}",
                    words.rule(attempt.rule),
                    attempt.position.x,
                    attempt.position.y
                ));
                let closest = attempt.closest_set();
                ui.indent(("close", attempt.rule.rule_map, attempt.rule.rule), |ui| {
                    for check in attempt
                        .checks
                        .iter()
                        .filter(|c| !c.holds && Some(c.input_set) == closest)
                    {
                        ui.colored_label(ui.visuals().warn_fg_color, words.failed(check));
                    }
                });
            }
        });
    }
    let others: Vec<&RuleAttempt> = explanation
        .attempts
        .iter()
        .filter(|a| {
            !matches!(a.outcome, AttemptOutcome::Applied { .. })
                && !close.iter().any(|c| std::ptr::eq(*c, *a))
                && !held_back.iter().any(|h| std::ptr::eq(*h, *a))
        })
        .collect();
    if !others.is_empty() {
        let rules = plural(others.len(), "other rule", "other rules");
        ui.weak(match nothing_to_read(&others) {
            Some(Unread { layers, missing }) => {
                let lacks = if missing {
                    "which the map lacks"
                } else if layers.len() == 1 {
                    "which is empty here"
                } else {
                    "which are empty here"
                };
                format!(
                    "{rules} could write here too, but they need tiles on {}, {lacks}",
                    and_list(&layers)
                )
            }
            None => format!(
                "{rules} could write here too, but nothing here is what {} for",
                if others.len() == 1 {
                    "it looks"
                } else {
                    "they look"
                }
            ),
        });
    }
    for unnamed in &explanation.unnamed {
        ui.weak(format!(
            "{}: {} is named by no rule, which see it only as a tile",
            unnamed.layer,
            words.cell(unnamed.tile)
        ));
    }
}

/// Layers the failing rules needed tiles on and found nothing: the reason they fail.
struct Unread {
    /// The layers, each once.
    layers: Vec<String>,
    /// Whether the map lacks every one of them.
    missing: bool,
}

/// When every condition that failed in `attempts` asked for a tile and found the cell empty:
/// the layers they read. `None` when some failed on a tile that was there.
fn nothing_to_read(attempts: &[&RuleAttempt]) -> Option<Unread> {
    let mut layers: Vec<String> = Vec::new();
    let mut missing = true;
    let mut any = false;
    for check in attempts.iter().flat_map(|a| &a.checks).filter(|c| !c.holds) {
        if !check.found.is_empty() || check.condition != Condition::AnyOf {
            return None;
        }
        any = true;
        missing &= !check.layer_present;
        if !layers.contains(&check.layer) {
            layers.push(check.layer.clone());
        }
    }
    any.then_some(Unread { layers, missing })
}

/// How cells, tiles and rules of an explanation are put in words.
struct Words<'e> {
    map: &'e Map,
    rules: &'e RuleSet,
    explanation: &'e Explanation,
}

impl Words<'_> {
    /// A cell of the run, its tileset by name: the map's slots, then the tilesets the run added.
    fn cell(&self, cell: Cell) -> String {
        let Some(slot) = cell.slot() else {
            return "empty".to_owned();
        };
        let index = slot.index();
        let tileset = match self.map.tilesets.get(index) {
            Some(reference) => Some(reference.tileset.name.clone()),
            None => index
                .checked_sub(self.map.tilesets.len())
                .and_then(|added| self.explanation.added_tilesets.get(added))
                .and_then(|table| self.rules.tileset(*table))
                .map(|reference| reference.tileset.name.clone()),
        };
        format!(
            "{} #{}{}",
            tileset.unwrap_or_else(|| "a tileset".to_owned()),
            cell.tile().0,
            flips(cell.flip())
        )
    }

    /// A tile a condition names, its tileset by the rules' name for it.
    fn match_cell(&self, cell: MatchCell) -> String {
        let Some(tile) = cell.tile else {
            return "empty".to_owned();
        };
        let tileset = self
            .rules
            .tileset(tile.tileset)
            .map_or_else(|| "a tileset".to_owned(), |t| t.tileset.name.clone());
        format!("{tileset} #{}{}", tile.tile.0, flips(tile.flip))
    }

    fn rule(&self, rule: RuleRef) -> String {
        format!("rule {} of {}", rule.rule + 1, self.rule_map(rule.rule_map))
    }

    fn rule_map(&self, index: usize) -> String {
        self.rules
            .rule_maps()
            .get(index)
            .and_then(|r| r.path().file_name())
            .map_or_else(
                || format!("rule map {}", index + 1),
                |name| name.to_string_lossy().into_owned(),
            )
    }

    /// A failed condition: where, what it asks, what is there.
    fn failed(&self, check: &ConditionCheck) -> String {
        let asks = match check.condition {
            Condition::AnyOf => "needs one of",
            Condition::NoneOf => "needs none of",
        };
        let tiles: Vec<String> = check.tiles.iter().map(|t| self.match_cell(*t)).collect();
        let missing = if check.layer_present {
            ""
        } else {
            " (the map has no such layer)"
        };
        format!(
            "{}{missing} at {}, {} {asks} {}; it holds {}",
            check.layer,
            check.at.x,
            check.at.y,
            tiles.join(", "),
            self.cell(check.found)
        )
    }
}

/// A flip and the letter Tiled writes it with.
struct FlipLetter {
    flip: Flip,
    letter: char,
}

/// The flips in the order their letters are written.
const FLIP_LETTERS: [FlipLetter; 4] = [
    FlipLetter {
        flip: Flip::H,
        letter: 'h',
    },
    FlipLetter {
        flip: Flip::V,
        letter: 'v',
    },
    FlipLetter {
        flip: Flip::D,
        letter: 'd',
    },
    FlipLetter {
        flip: Flip::HEX120,
        letter: 'x',
    },
];

/// A tile's flips as Tiled's letters, after a slash; nothing for none.
fn flips(flip: Flip) -> String {
    let letters: String = FLIP_LETTERS
        .iter()
        .filter(|f| flip.contains(f.flip))
        .map(|f| f.letter)
        .collect();
    if letters.is_empty() {
        String::new()
    } else {
        format!("/{letters}")
    }
}
