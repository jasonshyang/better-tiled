//! An edit's report as the runner gives it (`10-agents.md`, §9): the outcome as values, its
//! JSON round trip, and the words of a few edits pinned.

#![cfg(test)]

mod common;

use bt_script::{EditOutcome, Outcome, Saved, from_json, to_json};
use common::*;
use glam::IVec2;

#[test]
fn a_pond_and_a_tree_are_reported_layer_by_layer_from_the_top() {
    let level = level("bt-script-report-pond");
    let result = ran(
        &level,
        "# a pond and a tree\npaint water rect 3,2 4x3\nplace tree at 9.5,6 type=pine",
    );
    let outcome = result
        .outcome
        .clone()
        .with_made(Vec::new())
        .with_saved(Saved::File {
            file: "level.tmx".into(),
            painted_in: None,
            undo: Some("bt undo level.tmx".into()),
        });
    assert_eq!(outcome.label, "a pond and a tree");
    assert_eq!(outcome.operations, 2);
    assert_eq!(
        outcome.report().text(),
        "level.tmx: 2 operations\n  props: +1 object #1: #1 tree [pine] at 9.5,6\n  ground: 30 cells \
         changed in 2,1 6x5: grass/water edges ×18, water ×12; grass 96 → 70 cells, water 0 → 12 \
         cells\nno new seams or hidden edges, and no objects on other ground\nsaved level.tmx; bt \
         undo level.tmx puts it back\n"
    );
    assert_eq!(
        outcome.cells.len(),
        31,
        "the pond's cells and where the tree stands"
    );
    assert!(outcome.cells.contains(&IVec2::new(9, 5)));
    let json = to_json(&outcome).unwrap();
    assert_eq!(from_json::<EditOutcome>(&json).unwrap(), outcome);
}

#[test]
fn an_edit_that_changes_nothing_says_so() {
    let level = level("bt-script-report-nothing");
    let result = ran(&level, "paint grass all");
    assert!(!result.outcome.changed);
    assert!(result.outcome.cells.is_empty() && result.outcome.area.is_none());
    let outcome = result
        .outcome
        .with_saved(Saved::Unchanged { in_editor: false });
    assert_eq!(
        outcome.report().text(),
        "level.tmx: 1 operation\n  nothing changed\nnothing to save\n"
    );
}

#[test]
fn objects_moved_and_changed_are_said_with_their_places() {
    let level = level("bt-script-report-objects");
    let first = ran(
        &level,
        "object spawn at 1.5,1.5 wave=1\nobject spawn at 4.5,1.5",
    );
    let script = bt_script::parse_script("move #1 by 2,0\nset #1 wave=2\ndelete #2").unwrap();
    let input = bt_script::RunInput::builder(first.painted.clone(), &level.path).build();
    let result = bt_script::run_script(&script, &input, &mut bt_tmx::Loader::default()).unwrap();
    let text = result.outcome.report().text();
    assert_eq!(
        text,
        "level.tmx: 3 operations\n  design: -1 object: #2 spawn at 4.5,1.5\n  design: moved 1 \
         object: #1 spawn 1.5,1.5 → 3.5,1.5\n  design: changed 1 object: #1 spawn wave=2\n"
    );
}
