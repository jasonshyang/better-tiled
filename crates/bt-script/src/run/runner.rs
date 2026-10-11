//! The runner's state and its loop (`10-agents.md`, §8): a copy of the map as painted, the
//! names (rebuilt when a script adds tilesets, keeping what it defined), the feet measured, the
//! context of `selection`, `selected` and `last`, the seeds, the notes and the AutoMap layers.
//! Each operation draws from its own generator: the script's seed mixed with the operation's
//! ordinal (counted from the start or the last `seed N` line), or its own `seed N`.
//!
//! On a map with AutoMap layers the runner edits what was painted and reads shapes on the map as
//! it shows, which it brings up to date before an operation reads it, as the editor's tools do.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_automap::layers::{AutoLayers, Setup};
use bt_automap::{RuleSet, Severity};
use bt_core::{LayerId, Map, ObjectId};
use bt_tmx::Loader;
use bt_utils::IRect;
use bt_utils::rng::Rng;
use glam::{IVec2, UVec2};

use super::error::{RunError, RunFailure};
use super::input::{ChangedView, RulesRead, RunInput};
use super::result::EditResult;
use crate::changes::{MapChanges, Resized, changed_cells};
use crate::check::made_by;
use crate::context::Context;
use crate::edit_report::{EditOutcome, LineNote};
use crate::feet::Feet;
use crate::grid::sorted_cells;
use crate::names::Names;
use crate::report::LineKind;
use crate::script::{Area, Located, ObjectRef, Op, Script};
use crate::shapes::area_cells;
use crate::view::{ViewOutcome, ViewRequest};

/// The most characters of an operation a label holds.
const LABEL_LENGTH: usize = 70;

/// A script running on a copy of a map.
pub(crate) struct Runner<'l> {
    /// What was painted, as the script has left it so far.
    pub(super) map: Map,
    /// The map's file.
    pub(super) path: PathBuf,
    /// Reads maps and rules the script names, sharing the map's tilesets.
    pub(super) loader: &'l mut Loader,
    /// The map's names and what the script defined.
    pub(super) names: Names,
    /// Feet measured so far.
    pub(super) feet: Feet,
    /// `selection`, `selected`, `last` and the operation's seed.
    pub(super) context: Context,
    /// The seed the operations after the last `seed N` line draw from.
    script_seed: u64,
    /// The operations since the start or the last `seed N` line, this one included.
    ordinal: u64,
    /// The running operation's own `seed N`.
    own_seed: Option<u64>,
    /// The running operation's line.
    pub(super) line: usize,
    /// Whether the running operation is one of the `--with` file's.
    in_definitions: bool,
    /// How operations were carried out.
    pub(super) notes: Vec<LineNote>,
    /// What may keep the result from being what it seems, not of one line.
    pub(super) warnings: Vec<String>,
    /// The cells no terrain tile fitted.
    pub(super) unfilled: Vec<IVec2>,
    /// The resizes so far, in order.
    pub(super) resizes: Vec<ResizeStep>,
    /// The new cells of resizes, to look at again when the script ends.
    pub(super) strips: Vec<NewStrips>,
    /// The AutoMap layers, while the map has them.
    pub(super) auto: Option<AutoState>,
    /// Whether the script started, changed or stopped the AutoMap layers.
    pub(super) auto_changed: bool,
    /// Rules the caller read for the map.
    pub(super) rules_read: Option<RulesRead>,
    /// Whether what was painted may have changed since the AutoMap layers' result was made.
    shown_stale: bool,
}

/// AutoMap layers while a script runs.
pub(super) struct AutoState {
    /// Their setup.
    pub(super) setup: Setup,
    /// The rules they run.
    pub(super) rules: Arc<RuleSet>,
    /// What was painted and the rules' result, brought up to date before it is read.
    pub(super) layers: AutoLayers,
}

/// A resize: the new size and how far the old cells moved.
#[derive(Clone, Copy, Debug)]
pub(super) struct ResizeStep {
    /// The new size in cells.
    pub(super) size: UVec2,
    /// How far the old cells moved.
    pub(super) shift: IVec2,
}

/// The new cells of a resize, to say at the end of the script which layers are still empty
/// there that had a tile in every cell before.
#[derive(Clone, Debug)]
pub(super) struct NewStrips {
    /// The resize's line.
    pub(super) line: usize,
    /// The new strips of cells.
    pub(super) strips: Vec<IRect>,
    /// The tile layers that had a tile in every cell before the resize.
    pub(super) full: Vec<LayerId>,
}

impl<'l> Runner<'l> {
    /// A runner on a copy of `input`'s map, reading through `loader`.
    pub(super) fn new(input: &RunInput, loader: &'l mut Loader) -> Runner<'l> {
        let map = Map::clone(&input.map);
        let names = Names::of(&map);
        let mut context = Context::new(input.seed);
        if let Some(selection) = &input.selection {
            context = context.with_selection(selection.clone());
        }
        let mut warnings = Vec::new();
        let auto = input.automap.as_ref().map(|auto| {
            let known = AutoLayers::known(Arc::clone(&input.map), Arc::clone(&auto.shown));
            let layers = AutoLayers::update(
                Some(&known),
                Arc::clone(&input.map),
                &auto.setup,
                &auto.rules,
            );
            if **layers.result() != *auto.shown {
                warnings.push(
                    "the map's AutoMap rules now make something other than it holds: the result \
                     is what they make now"
                        .to_owned(),
                );
            }
            AutoState {
                setup: auto.setup.clone(),
                rules: Arc::clone(&auto.rules),
                layers,
            }
        });
        Runner {
            map,
            path: input.path.clone(),
            loader,
            names,
            feet: Feet::new(),
            context,
            script_seed: input.seed,
            ordinal: 0,
            own_seed: None,
            line: 0,
            in_definitions: false,
            notes: Vec::new(),
            warnings,
            unfilled: Vec::new(),
            resizes: Vec::new(),
            strips: Vec::new(),
            auto,
            auto_changed: false,
            rules_read: input.rules_read.clone(),
            shown_stale: false,
        }
    }

    /// Runs the `--with` definitions, then the script, then says what it did.
    pub(super) fn run(mut self, script: &Script, input: &RunInput) -> Result<EditResult, RunError> {
        self.run_definitions(input)?;
        for located in &script.ops {
            self.step(located)?;
        }
        Ok(self.finish(script, input))
    }

    /// The names of the map and what the definitions defined.
    pub(super) fn into_names(self) -> Names {
        self.names
    }

    /// Runs the `--with` definitions, which hold define lines and seeds only.
    pub(super) fn run_definitions(&mut self, input: &RunInput) -> Result<(), RunError> {
        if let Some(definitions) = &input.definitions {
            self.in_definitions = true;
            for located in &definitions.ops {
                let defines = matches!(
                    located.op,
                    Op::Seed(_)
                        | Op::DefineMaterial { .. }
                        | Op::DefineGroup { .. }
                        | Op::DefineTiles { .. }
                        | Op::DefineArea { .. }
                        | Op::DefinePiece { .. }
                );
                if !defines {
                    return Err(self.error_at(
                        located,
                        RunFailure::refused("a definitions file holds define lines (and seed)"),
                    ));
                }
                self.step(located)?;
            }
            // The script's randomness does not depend on the definitions file's.
            self.in_definitions = false;
            self.script_seed = input.seed;
            self.ordinal = 0;
        }
        Ok(())
    }

    /// Runs one operation.
    fn step(&mut self, located: &Located) -> Result<(), RunError> {
        self.line = located.line;
        self.own_seed = located.seed;
        if let Op::Seed(seed) = located.op {
            self.script_seed = seed;
            self.ordinal = 0;
            return Ok(());
        }
        self.ordinal += 1;
        self.context.set_seed(self.op_seed());
        let next_id = self.map.next_object_id;
        let tilesets = self.map.tilesets.len();
        self.apply(&located.op)
            .map_err(|failure| self.error_at(located, failure))?;
        if self.map.tilesets.len() != tilesets {
            self.names.rebuild_tables(&self.map);
        }
        let made: Vec<ObjectId> = (next_id..self.map.next_object_id)
            .map(ObjectId)
            .filter(|&id| self.map.object(id).is_some())
            .collect();
        if !made.is_empty() {
            self.context.set_last(made);
        }
        if changes_the_map(&located.op) {
            self.shown_stale = true;
        }
        Ok(())
    }

    /// The error of a failure at `located`.
    fn error_at(&self, located: &Located, failure: RunFailure) -> RunError {
        RunError {
            line: located.line,
            operation: located.text.clone(),
            in_definitions: self.in_definitions,
            failure: Box::new(failure),
        }
    }

    /// Carries out one operation.
    fn apply(&mut self, op: &Op) -> Result<(), RunFailure> {
        match op {
            Op::Seed(_) => Ok(()),
            Op::DefineMaterial { name, parts } => self.define_material(name, parts),
            Op::DefineGroup { name, parts } => self.define_group(name, parts),
            Op::DefineTiles { name, tiles, layer } => {
                self.define_tiles(name, tiles, layer.as_ref())
            }
            Op::DefineArea { name, area } => self.define_area(name, area),
            Op::DefinePiece {
                name,
                source,
                minus,
                foot,
            } => self.define_piece(name, source, minus, *foot),
            Op::Paint {
                terrain,
                area,
                layer,
            } => self.paint_terrain(terrain, area, layer.as_ref(), false),
            Op::Erase {
                terrain,
                area,
                layer,
            } => self.paint_terrain(terrain, area, layer.as_ref(), true),
            Op::Tile { tiles, area, layer } => self.put_tiles(tiles, area, layer.as_ref()),
            Op::Clear { area, layers } => self.clear_cells(area, layers),
            Op::Copy { source, to } => self.copy_from(source, *to),
            Op::Place {
                names,
                placement,
                flip,
                jitter,
                object,
            } => self.place(names, placement, *flip, *jitter, object),
            Op::Scatter {
                names,
                area,
                amount,
                spacing,
                jitter,
                flip,
                object,
            } => self.scatter(names, area, *amount, *spacing, *jitter, *flip, object),
            Op::Object {
                kind,
                geometry,
                object,
            } => self.add_objects(kind, geometry, object),
            Op::Set {
                targets,
                assignments,
            } => self.set_objects(targets, assignments),
            Op::MoveTo { target, to } => self.move_to(target, *to),
            Op::MoveBy { targets, by } => self.move_by(targets, *by),
            Op::Delete { targets } => self.delete_named(targets),
            Op::DeleteObjects { place, kind, layer } => {
                self.delete_where(place, kind.as_deref(), layer.as_ref())
            }
            Op::Automap { rules, bindings } => self.automap(rules.as_deref(), bindings),
            Op::AutomapLayers(op) => self.automap_layers(op),
            Op::Property { assignments } => self.set_map_properties(assignments),
            Op::Layer(op) => self.layer_op(op),
            Op::Resize { size, shift } => self.resize(*size, *shift),
            Op::Draw(drawing) => self.draw(drawing),
        }
    }

    /// The running operation's seed.
    pub(super) fn op_seed(&self) -> u64 {
        match self.own_seed {
            Some(seed) => seed,
            None => mixed(self.script_seed, self.ordinal),
        }
    }

    /// The `seed N` the running operation ends with, if it has one.
    pub(super) fn own_seed(&self) -> Option<u64> {
        self.own_seed
    }

    /// A generator for the running operation's random choices.
    pub(super) fn rng(&self) -> Rng {
        Rng::new(self.op_seed())
    }

    /// Notes how the running operation was carried out.
    pub(super) fn note(&mut self, text: impl Into<String>) {
        self.notes.push(LineNote {
            line: self.line,
            in_definitions: self.in_definitions,
            kind: LineKind::Note,
            text: text.into(),
        });
    }

    /// Notes what may keep the running operation's result from being what it seems.
    pub(super) fn warn(&mut self, text: impl Into<String>) {
        self.notes.push(LineNote {
            line: self.line,
            in_definitions: self.in_definitions,
            kind: LineKind::Warning,
            text: text.into(),
        });
    }

    /// The map's folder, against which a script's paths resolve.
    pub(super) fn folder(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("."))
    }

    /// A path a script writes, from the map's folder unless absolute.
    pub(super) fn resolve(&self, written: &str) -> PathBuf {
        let path = Path::new(written);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            bt_utils::paths::clean_path(&self.folder().join(path))
        }
    }

    /// Brings the AutoMap layers' result up to date with what was painted, when the map has
    /// them and something may have changed.
    pub(super) fn refresh_shown(&mut self) {
        if !self.shown_stale {
            return;
        }
        self.shown_stale = false;
        if let Some(auto) = &mut self.auto {
            let painted = Arc::new(self.map.clone());
            auto.layers = auto
                .layers
                .after_painting(painted, &auto.setup, &auto.rules);
        }
    }

    /// The map as it shows, as [`Runner::refresh_shown`] last made it: with AutoMap layers the
    /// rules' result, else what was painted.
    pub(super) fn shown_now(&self) -> &Map {
        match &self.auto {
            Some(auto) => auto.layers.result(),
            None => &self.map,
        }
    }

    /// The cells of `area` on the map as it shows.
    pub(super) fn cells_of(&mut self, area: &Area) -> Result<Vec<IVec2>, RunFailure> {
        self.refresh_shown();
        Ok(area_cells(
            self.shown_now(),
            &self.names,
            &self.context,
            area,
        )?)
    }

    /// The cells of `area` on the map as it shows, noting for `verb` when there are none.
    pub(super) fn cells_noted(
        &mut self,
        area: &Area,
        verb: &str,
    ) -> Result<Vec<IVec2>, RunFailure> {
        let cells = self.cells_of(area)?;
        if cells.is_empty() {
            self.note(format!(
                "{verb}: the shape has no cells (outside the map, or nothing matched)"
            ));
        }
        Ok(cells)
    }

    /// The objects `target` stands for, of what was painted. On a map with AutoMap layers, an
    /// object the rules placed is refused with the ways out.
    pub(super) fn objects_of(&mut self, target: &ObjectRef) -> Result<Vec<ObjectId>, RunFailure> {
        match self.names.objects(&self.map, target, &self.context) {
            Ok(ids) => Ok(ids),
            Err(error) => {
                if self.auto.is_some() {
                    self.refresh_shown();
                    let shown = self.names.objects(self.shown_now(), target, &self.context);
                    if let Some(id) = shown
                        .ok()
                        .and_then(|ids| ids.into_iter().find(|&id| self.map.object(id).is_none()))
                    {
                        return Err(RunFailure::RuleMade { id: id.0 });
                    }
                }
                Err(error.into())
            }
        }
    }

    /// The objects several targets stand for, each once, in the order named.
    pub(super) fn objects_of_all(
        &mut self,
        targets: &[ObjectRef],
    ) -> Result<Vec<ObjectId>, RunFailure> {
        let mut ids: Vec<ObjectId> = Vec::new();
        for target in targets {
            for id in self.objects_of(target)? {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        Ok(ids)
    }

    /// Says what the script did.
    fn finish(mut self, script: &Script, input: &RunInput) -> EditResult {
        self.note_strips_still_empty();
        self.shown_stale = true;
        self.refresh_shown();
        if let Some(auto) = &self.auto {
            let problems: Vec<String> = auto
                .layers
                .problems()
                .filter(|problem| problem.severity() != Severity::Note)
                .map(|problem| format!("AutoMap layers: {}", problem.text))
                .collect();
            self.warnings.extend(problems);
        }
        let painted = Arc::new(self.map);
        let (shown, setup, layers) = match self.auto {
            Some(auto) => (
                Arc::clone(auto.layers.result()),
                Some(auto.setup),
                Some(auto.layers),
            ),
            None => (Arc::clone(&painted), None, None),
        };
        let shown_before: &Map = input
            .automap
            .as_ref()
            .map_or(&input.map, |auto| &auto.shown);
        let moved_shown = moved_by(shown_before, &self.resizes);
        let mut feet = self.feet;
        let mut changes = MapChanges::between_with(&moved_shown, &shown, &mut feet);
        changes.resized = resized(shown_before, &shown, &self.resizes);
        let mut cells = changed_cells(&moved_shown, &shown, &mut feet);
        let with_layers = input.automap.is_some() || layers.is_some();
        let painted_changes = with_layers.then(|| {
            let moved_painted = moved_by(&input.map, &self.resizes);
            cells.extend(changed_cells(&moved_painted, &painted, &mut feet));
            let mut painted_changes = MapChanges::between_with(&moved_painted, &painted, &mut feet);
            painted_changes.resized = None;
            painted_changes
        });
        let cells = sorted_cells(cells);
        let painted_changed = painted_changes.as_ref().is_some_and(|p| !p.is_empty());
        let changed = !changes.is_empty() || painted_changed || self.auto_changed;
        let made = made_by(&moved_shown, &shown, &cells, self.loader.custom_types())
            .lines()
            .into_iter()
            .map(|line| line.text)
            .collect();
        let view = match input.view.filter(|_| !cells.is_empty()) {
            Some(asked) => match changed_view(&shown, &self.names, &cells, asked) {
                Ok(view) => Some(view),
                Err(error) => {
                    self.warnings
                        .push(format!("the changed area can't be shown as text: {error}"));
                    None
                }
            },
            None => None,
        };
        let outcome = EditOutcome {
            map: bt_utils::paths::display_name(&self.path),
            label: label(script),
            operations: script.ops.len(),
            changes,
            painted: painted_changes,
            unfilled: sorted_cells(self.unfilled),
            notes: self.notes,
            area: IRect::bounding(cells.iter().copied()),
            cells,
            changed,
            warnings: self.warnings,
            made: Some(made),
            view,
            saved: None,
            marked: None,
            picture: None,
        };
        EditResult {
            painted,
            shown,
            setup,
            layers,
            setup_changed: self.auto_changed,
            outcome,
        }
    }

    /// Notes, at each resize's line, the tile layers that had a tile in every cell before it and
    /// are still empty somewhere in its new cells now the script has ended.
    fn note_strips_still_empty(&mut self) {
        for strips in std::mem::take(&mut self.strips) {
            let empty: Vec<String> = strips
                .full
                .iter()
                .filter_map(|&id| self.map.layer(id))
                .filter(|layer| {
                    layer.as_tile_layer().is_some_and(|tiles| {
                        strips
                            .strips
                            .iter()
                            .any(|strip| strip.points().any(|p| tiles.cell(p).is_empty()))
                    })
                })
                .map(|layer| layer.name.clone())
                .collect();
            if empty.is_empty() {
                continue;
            }
            let at: Vec<String> = strips
                .strips
                .iter()
                .map(|strip| crate::words::region_text(*strip))
                .collect();
            self.notes.push(LineNote {
                line: strips.line,
                in_definitions: false,
                kind: LineKind::Warning,
                text: format!(
                    "resize: the new cells ({}) are still empty on {}, which had a tile in every \
                     cell",
                    at.join(", "),
                    bt_utils::text::and_list(&empty)
                ),
            });
        }
    }
}

/// The area around `cells` on `shown` as text, as `asked`.
fn changed_view(
    shown: &Map,
    names: &Names,
    cells: &[IVec2],
    asked: ChangedView,
) -> Result<String, crate::error::Error> {
    let mut request = ViewRequest::new(shown, names)
        .around(cells)
        .runs(asked.runs);
    if let Some(by) = asked.by {
        request = request.by(by);
    }
    Ok(ViewOutcome::from_request(&request)?.text)
}

/// Whether an operation may change the map (definitions and seeds do not).
fn changes_the_map(op: &Op) -> bool {
    !matches!(
        op,
        Op::Seed(_)
            | Op::DefineMaterial { .. }
            | Op::DefineGroup { .. }
            | Op::DefineTiles { .. }
            | Op::DefineArea { .. }
            | Op::DefinePiece { .. }
    )
}

/// An operation's seed: the script's seed mixed with the operation's ordinal.
fn mixed(seed: u64, ordinal: u64) -> u64 {
    Rng::new(seed ^ ordinal.wrapping_mul(0xD1B5_4A32_D192_ED03)).next_u64()
}

/// `map` resized as the script resized it, so its cells are where the script's resizes moved
/// them, to compare the map after with.
fn moved_by(map: &Map, resizes: &[ResizeStep]) -> Map {
    let mut moved = map.clone();
    for step in resizes {
        // A resize the script made of this map's size cannot fail on a copy of it.
        if bt_edit::resize_map(&mut moved, step.size, step.shift, false).is_err() {
            break;
        }
    }
    moved
}

/// The size change and the shift of every resize together; none when there were none.
fn resized(before: &Map, after: &Map, resizes: &[ResizeStep]) -> Option<Resized> {
    let shift = resizes
        .iter()
        .fold(IVec2::ZERO, |sum, step| sum + step.shift);
    let size = |map: &Map| {
        IVec2::new(
            i32::try_from(map.width).unwrap_or(i32::MAX),
            i32::try_from(map.height).unwrap_or(i32::MAX),
        )
    };
    let (before, after) = (size(before), size(after));
    (before != after || shift != IVec2::ZERO).then_some(Resized {
        before,
        after,
        shift,
    })
}

/// The edit's label: the script's first-line comment, else its first operation that changes
/// the map and how many more follow, cut to a length a history list shows.
fn label(script: &Script) -> String {
    let name = match &script.label {
        Some(comment) => comment.clone(),
        None => {
            let first = script
                .ops
                .iter()
                .find(|located| changes_the_map(&located.op))
                .or(script.ops.first());
            let text = first.map_or("edit", |located| located.text.as_str());
            let others = script
                .ops
                .iter()
                .filter(|located| !matches!(located.op, Op::Seed(_)))
                .count()
                .saturating_sub(1);
            if others > 0 {
                format!("{text} (+{others} more)")
            } else {
                text.to_owned()
            }
        }
    };
    let mut short: String = name.chars().take(LABEL_LENGTH).collect();
    if name.chars().count() > LABEL_LENGTH {
        short.push('…');
    }
    short
}
