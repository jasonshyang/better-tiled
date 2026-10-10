//! The AutoMap panel: where the map's rules come from, whether they are being read and why a
//! read failed, what is wrong with them rule map by rule map, and what the last run did: the
//! matches of each rule map, the layers locks kept outputs out of, and its problems.

use std::path::Path;

use bt_automap::{Area, IdleRuleMap, Problem, RuleSet, Severity};
use bt_editor::{ActionId, AutoMapRules, DocumentId, Editor, Intent, LastRun};
use bt_utils::text::{and_list, plural};

use super::automap_live::show_live;
use super::problem_line::problem_line;

/// The AutoMap panel (it keeps nothing of its own: sections fold as egui remembers).
#[derive(Default)]
pub(crate) struct AutoMapPanel;

impl AutoMapPanel {
    /// Draws the rules of `doc`'s map; a run asked for goes to `out`.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        editor: &Editor,
        doc: DocumentId,
        out: &mut Vec<Intent>,
    ) {
        let mut run = egui::Button::new("Run AutoMap");
        if let Some(key) = editor.keymap().key_text(ActionId::AutoMap) {
            run = run.shortcut_text(key);
        }
        if ui
            .add(run)
            .on_hover_text("Run the rules over the selection, or the whole map")
            .clicked()
        {
            out.push(Intent::Run(ActionId::AutoMap));
        }
        ui.separator();
        let Some(rules) = editor.automap_rules(doc) else {
            ui.weak("Looking for the map's rules…");
            return;
        };
        egui::ScrollArea::vertical().show(ui, |ui| {
            show_explaining(ui, editor, doc);
            show_rules(ui, rules);
            if rules.rules().is_some() {
                ui.separator();
                show_live(ui, editor, doc, out);
            }
        });
    }
}

/// The cell the Explain tool chose, or how to choose one.
fn show_explaining(ui: &mut egui::Ui, editor: &Editor, doc: DocumentId) {
    match editor.explained_cell(doc) {
        Some(cell) => {
            ui.label(format!("Explaining cell {}, {}", cell.x, cell.y));
        }
        None => {
            let key = editor
                .keymap()
                .key_text(ActionId::ExplainTool)
                .map_or_else(String::new, |key| format!(" ({key})"));
            ui.weak(format!(
                "Click a cell with the Explain tool{key} to see what the rules did there"
            ));
        }
    }
    ui.separator();
}

fn show_rules(ui: &mut egui::Ui, rules: &AutoMapRules) {
    let Some(file) = rules.file() else {
        ui.label("No rules for this map.");
        ui.weak(
            "Put a rules.txt beside the map, or name a rules file in the project's properties.",
        );
        return;
    };
    ui.label(format!("Rules: {}", display_name(file)))
        .on_hover_text(file.display().to_string());
    if rules.is_reading() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.weak("Reading the rules…");
        });
    }
    if let Some(error) = rules.error() {
        let kept = if rules.rules().is_some() {
            " The rules read before stay in use until it is fixed."
        } else {
            ""
        };
        ui.colored_label(
            ui.visuals().error_fg_color,
            format!("Cannot read them: {error}.{kept}"),
        );
    }
    if let Some(set) = rules.rules() {
        let rules_dir = file.parent();
        ui.weak(format!(
            "{}, {}",
            plural(set.rule_maps().len(), "rule map", "rule maps"),
            plural(set.rule_count(), "rule", "rules")
        ));
        show_problems(ui, set, rules_dir);
        show_rule_maps(ui, set, rules_dir);
        if let Some(run) = rules.last_run() {
            show_last_run(ui, run, rules_dir);
        }
    }
}

/// What is wrong with the rules, the rule set's first, then each rule map's under its name;
/// open at first only when something is more than a note.
fn show_problems(ui: &mut egui::Ui, set: &RuleSet, rules_dir: Option<&Path>) {
    let all: Vec<&Problem> = set
        .problems()
        .iter()
        .chain(set.rule_maps().iter().flat_map(|r| r.problems()))
        .collect();
    if all.is_empty() {
        return;
    }
    let count = |severity: Severity, one: &str, many: &str| {
        let n = all.iter().filter(|p| p.severity() == severity).count();
        (n > 0).then(|| plural(n, one, many))
    };
    let counts: Vec<String> = [
        count(Severity::Error, "error", "errors"),
        count(Severity::Warning, "warning", "warnings"),
        count(Severity::Note, "note", "notes"),
    ]
    .into_iter()
    .flatten()
    .collect();
    egui::CollapsingHeader::new(format!("Problems: {}", and_list(&counts)))
        .id_salt("automap problems")
        .default_open(all.iter().any(|p| p.severity() != Severity::Note))
        .show(ui, |ui| {
            for problem in set.problems() {
                problem_line(ui, problem);
            }
            for rule_map in set.rule_maps() {
                if rule_map.problems().is_empty() {
                    continue;
                }
                ui.label(relative_name(rule_map.path(), rules_dir));
                ui.indent(rule_map.path(), |ui| {
                    for problem in rule_map.problems() {
                        problem_line(ui, problem);
                    }
                });
            }
        });
}

/// The rule maps in the order they run, with their rule counts.
fn show_rule_maps(ui: &mut egui::Ui, set: &RuleSet, rules_dir: Option<&Path>) {
    egui::CollapsingHeader::new("Rule maps")
        .id_salt("automap rule maps")
        .show(ui, |ui| {
            for rule_map in set.rule_maps() {
                let name = relative_name(rule_map.path(), rules_dir);
                let rules = plural(rule_map.rule_count(), "rule", "rules");
                if rule_map.is_used() {
                    ui.label(format!("{name} — {rules}"))
                } else {
                    ui.weak(format!("{name} — not used"))
                }
                .on_hover_text(rule_map.path().display().to_string());
            }
        });
}

/// What the last run did.
fn show_last_run(ui: &mut egui::Ui, run: &LastRun, rules_dir: Option<&Path>) {
    egui::CollapsingHeader::new("Last run")
        .id_salt("automap last run")
        .default_open(true)
        .show(ui, |ui| {
            let over = match run.area {
                Area::Whole => "the whole map",
                Area::Selection(_) => "the selection",
                Area::Edit(_) => "an edit",
            };
            let report = &run.report;
            ui.label(format!(
                "{} over {over}",
                plural(report.matches, "match", "matches")
            ));
            match report.changed {
                Some(changed) => ui.weak(format!(
                    "Changed cells within {}×{} at {}, {}",
                    changed.w, changed.h, changed.x, changed.y
                )),
                None => ui.weak("No cell changed"),
            };
            let mut idle = Vec::new();
            for (index, (rule_map, matches)) in run
                .rules
                .rule_maps()
                .iter()
                .zip(&report.rule_map_matches)
                .enumerate()
            {
                let name = relative_name(rule_map.path(), rules_dir);
                if *matches == 0 {
                    let why = run
                        .idle
                        .iter()
                        .find(|i| i.rule_map == index)
                        .map(idle_reason);
                    idle.push(match why {
                        Some(why) => format!("{name}: {why}"),
                        None => name,
                    });
                } else {
                    ui.label(format!("{name}: {}", plural(*matches, "match", "matches")));
                }
            }
            if !idle.is_empty() {
                let header = if idle.len() == report.rule_map_matches.len() {
                    "No rule map matched".to_owned()
                } else {
                    format!(
                        "{} matched nothing",
                        plural(idle.len(), "rule map", "rule maps")
                    )
                };
                egui::CollapsingHeader::new(header)
                    .id_salt("automap idle rule maps")
                    .show(ui, |ui| {
                        for name in &idle {
                            ui.weak(name);
                        }
                    });
            }
            for locked in &report.locked_outputs {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    format!(
                        "{} kept out of the locked layer {}",
                        plural(locked.matches, "match was", "matches were"),
                        locked.name
                    ),
                );
            }
            for problem in &report.problems {
                problem_line(ui, problem);
            }
        });
}

/// Why a rule map matched nothing, as its inputs say.
fn idle_reason(idle: &IdleRuleMap) -> String {
    if !idle.missing_layers.is_empty() {
        format!(
            "reads {}, which the map lacks",
            and_list(&idle.missing_layers)
        )
    } else if !idle.empty_layers.is_empty() {
        let are = if idle.empty_layers.len() == 1 {
            "is"
        } else {
            "are"
        };
        format!("reads {}, which {are} empty", and_list(&idle.empty_layers))
    } else {
        "no rule matched".to_owned()
    }
}

/// A rule map's path from the rules file's folder, or its file name.
fn relative_name(path: &Path, rules_dir: Option<&Path>) -> String {
    rules_dir
        .and_then(|dir| path.strip_prefix(dir).ok())
        .map_or_else(
            || display_name(path),
            |relative| relative.display().to_string(),
        )
}

fn display_name(path: &Path) -> String {
    bt_utils::paths::display_name(path)
}
