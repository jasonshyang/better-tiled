//! The AutoMap panel: where the map's rules come from, whether they are being read and why a
//! read failed, what is wrong with them rule map by rule map, and what the last run did: the
//! matches of each rule map, the layers locks kept outputs out of, and its problems.

use std::path::Path;

use bt_automap::{Area, Problem, RuleSet, Severity};
use bt_editor::{ActionId, AutoMapRules, DocumentId, Editor, Intent, LastRun};
use bt_utils::text::plural;

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
        egui::ScrollArea::vertical().show(ui, |ui| show_rules(ui, rules));
    }
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

/// What is wrong with the rules, the rule set's first, then each rule map's under its name.
fn show_problems(ui: &mut egui::Ui, set: &RuleSet, rules_dir: Option<&Path>) {
    let count = set.problems().len()
        + set
            .rule_maps()
            .iter()
            .map(|r| r.problems().len())
            .sum::<usize>();
    if count == 0 {
        return;
    }
    egui::CollapsingHeader::new(format!("Problems ({count})"))
        .id_salt("automap problems")
        .default_open(true)
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
            for (rule_map, matches) in run.rules.rule_maps().iter().zip(&report.rule_map_matches) {
                let name = relative_name(rule_map.path(), rules_dir);
                if *matches == 0 {
                    ui.weak(format!("{name}: matched nothing"));
                } else {
                    ui.label(format!("{name}: {}", plural(*matches, "match", "matches")));
                }
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

fn problem_line(ui: &mut egui::Ui, problem: &Problem) {
    let (mark, color) = match problem.severity() {
        Severity::Error => ("⛔", ui.visuals().error_fg_color),
        Severity::Warning => ("⚠", ui.visuals().warn_fg_color),
        Severity::Note => ("ℹ", ui.visuals().weak_text_color()),
    };
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(color, mark);
        ui.label(&problem.text);
    });
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
