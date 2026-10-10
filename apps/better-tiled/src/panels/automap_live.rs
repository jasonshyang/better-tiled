//! The AutoMap panel's Live section: AutoMap layers on or off, the seed and Re-roll, Show What I
//! Painted, and each run's bindings of the names the rules use to the map's layers. A name bound
//! to an empty layer is marked: rules reading it match nothing, the commonest reason a pack's
//! rules do nothing on a map that paints elsewhere.

use bt_automap::LayerNames;
use bt_automap::layers::RuleLayers;
use bt_core::{LayerId, LayerKind, Map};
use bt_editor::{ActionId, DocumentId, Editor, Intent, LiveEdit};

use super::problem_line::problem_line;

/// What a temporary layer is called in the choosers.
const TEMPORARY: &str = "(temporary)";

/// One name of the rules and what they do with it.
struct RuleName<'r> {
    name: &'r str,
    /// The rules read it.
    read: bool,
    /// The rules write tiles to it.
    written: bool,
    /// The rules place objects on it.
    objects: bool,
    /// An earlier rule map writes what a later one reads.
    made: bool,
}

/// Draws the Live section for `doc`; changes go to `out`.
pub(crate) fn show_live(
    ui: &mut egui::Ui,
    editor: &Editor,
    doc: DocumentId,
    out: &mut Vec<Intent>,
) {
    let Some(document) = editor.document(doc) else {
        return;
    };
    let mut on = document.automap().is_some();
    if ui
        .checkbox(&mut on, "Live AutoMap")
        .on_hover_text(
            "Keep the rules' result apart from what you paint, made again after every change",
        )
        .changed()
    {
        out.push(Intent::Run(ActionId::LiveAutoMap));
    }
    let (Some(setup), Some(map)) = (document.automap(), editor.map_of(doc)) else {
        return;
    };
    let rule_layers = editor
        .automap_rules(doc)
        .and_then(|rules| rules.rules())
        .map(|rules| rules.rule_layers())
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.weak(format!("Seed {}", setup.seed));
        if ui
            .button("Re-roll")
            .on_hover_text("Draw the rules' random choices again")
            .clicked()
        {
            out.push(live(doc, LiveEdit::Reroll));
        }
        let mut painted = document.view().show_painted;
        if ui.checkbox(&mut painted, "Show what I painted").changed() {
            out.push(Intent::Run(ActionId::ShowPainted));
        }
    });
    let names = rule_names(&rule_layers);
    for (run, bound) in setup.runs.iter().enumerate() {
        egui::CollapsingHeader::new(format!("Run {}", run + 1))
            .id_salt(("automap run", run))
            .default_open(true)
            .show(ui, |ui| {
                for name in &names {
                    binding_row(ui, map, doc, run, bound, name, out);
                }
                if setup.runs.len() > 1 && ui.button("Remove this run").clicked() {
                    out.push(live(doc, LiveEdit::RemoveRun(run)));
                }
            });
    }
    if ui
        .button("Add a run")
        .on_hover_text("Run the rules again, bound to other layers")
        .clicked()
    {
        out.push(live(doc, LiveEdit::AddRun));
    }
    if let Some(layers) = editor.live_layers(doc) {
        for problem in layers.problems() {
            problem_line(ui, problem);
        }
    }
}

/// The names the rules use, each once: tile layers read or written, then object outputs.
fn rule_names(layers: &RuleLayers) -> Vec<RuleName<'_>> {
    let mut names: Vec<RuleName<'_>> = Vec::new();
    for name in layers.inputs.iter().chain(&layers.tile_outputs) {
        if names.iter().any(|n| n.name == name && !n.objects) {
            continue;
        }
        names.push(RuleName {
            name,
            read: layers.inputs.contains(name),
            written: layers.tile_outputs.contains(name),
            objects: false,
            made: layers.made.contains(name),
        });
    }
    names.extend(layers.object_outputs.iter().map(|name| RuleName {
        name,
        read: false,
        written: false,
        objects: true,
        made: false,
    }));
    names
}

/// One name's binding in run `run`, with a chooser of the map's layers it can bind to.
fn binding_row(
    ui: &mut egui::Ui,
    map: &Map,
    doc: DocumentId,
    run: usize,
    bound: &LayerNames,
    name: &RuleName<'_>,
    out: &mut Vec<Intent>,
) {
    let layer = if name.objects {
        bound.object_layer(name.name)
    } else {
        bound.tile_layer(name.name)
    };
    let role = match (name.objects, name.read, name.written) {
        (true, _, _) => "places objects on",
        (false, true, true) => "reads and writes",
        (false, true, false) => "reads",
        (false, false, _) => "writes",
    };
    ui.horizontal(|ui| {
        let mut label = ui.label(format!("{role} {}", name.name));
        if name.made {
            label = label.on_hover_text(
                "An earlier rule map makes it: left temporary, the rules still read it",
            );
        }
        let shown = match layer.and_then(|id| map.layer(id)) {
            Some(found) => found.name.clone(),
            None if layer.is_some() => "(gone)".to_owned(),
            None => TEMPORARY.to_owned(),
        };
        let combo = egui::ComboBox::from_id_salt(("automap binding", run, name.name, name.objects))
            .selected_text(shown)
            .show_ui(ui, |ui| {
                if ui.selectable_label(layer.is_none(), TEMPORARY).clicked() {
                    out.push(bind(doc, run, name, None));
                }
                map.for_each_layer(|candidate, _| {
                    let fits = match candidate.kind {
                        LayerKind::Tile(_) => !name.objects,
                        LayerKind::Object(_) => name.objects,
                        LayerKind::Image(_) | LayerKind::Group(_) => false,
                    };
                    if fits
                        && ui
                            .selectable_label(layer == Some(candidate.id), &candidate.name)
                            .clicked()
                    {
                        out.push(bind(doc, run, name, Some(candidate.id)));
                    }
                });
            });
        combo.response.labelled_by(label.id);
        let empty = name.read
            && layer
                .and_then(|id| map.layer(id))
                .and_then(|found| found.as_tile_layer())
                .is_some_and(|tiles| tiles.cells().next().is_none());
        if empty {
            ui.colored_label(ui.visuals().warn_fg_color, "empty")
                .on_hover_text("Rules reading it match nothing: bind the layer you paint on");
        }
    });
}

fn bind(doc: DocumentId, run: usize, name: &RuleName<'_>, layer: Option<LayerId>) -> Intent {
    live(
        doc,
        LiveEdit::Bind {
            run,
            name: name.name.to_owned(),
            objects: name.objects,
            layer,
        },
    )
}

fn live(doc: DocumentId, edit: LiveEdit) -> Intent {
    Intent::EditLive { doc, edit }
}
