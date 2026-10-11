//! `bt check`: problems first, then what objects stand on (`10-agents.md`, §11). It exits 5
//! when it lists a problem, so scripts tell "problems found" from "could not run".

use bt_script::{CheckOutcome, CheckRequest};
use bt_utils::paths::display_name;

use crate::cli::MapArg;
use crate::commands::look::placed_by_rules;
use crate::exit::{Printed, Status};
use crate::load::load_map;
use crate::output::printed;
use crate::run::Run;

/// Runs `bt check`.
pub(crate) fn check(run: &Run, args: &MapArg) -> anyhow::Result<Printed> {
    let loaded = load_map(&args.map, run)?;
    let warnings: Vec<String> = loaded.warnings.iter().map(ToString::to_string).collect();
    let project_file = bt_project::ProjectFile::find(&loaded.path, run.folders.home.as_deref())
        .ok()
        .flatten();
    let mut request = CheckRequest::new(&loaded.map, display_name(&args.map))
        .warnings(warnings)
        .custom_types(loaded.loader.custom_types());
    if let Some(file) = &project_file {
        request = request.project_file(file);
    }
    if let Some(layers) = &loaded.layers {
        let placed = placed_by_rules(&layers.painted, &loaded.map);
        request = request.placed_by_rules(placed.iter().map(|id| id.0).collect());
    }
    let outcome = CheckOutcome::from_request(&request);
    let status = if outcome.has_problems() {
        Status::Problems
    } else {
        Status::Done
    };
    printed(&outcome, run.cli.json, status)
}

#[cfg(test)]
mod tests {
    use bt_test_data::Scratch;

    use crate::commands::testing::{arg, bt, sewers_with_automap_layers};
    use crate::exit::Status;

    #[test]
    fn objects_the_rules_place_are_named_as_theirs() {
        let scratch = Scratch::new("bt-check-rules-objects").unwrap();
        let Some(sewers) = sewers_with_automap_layers(&scratch) else {
            return;
        };
        // A project whose Trigger class lacks the property the rules give their triggers.
        std::fs::write(
            scratch.join("examples/game.tiled-project"),
            r#"{"propertyTypes": [{"id": 1, "name": "Trigger", "type": "class", "members": [], "useAs": ["object"]}]}"#,
        )
        .unwrap();
        let printed = bt(&["check", arg(&sewers)]).unwrap();
        assert_eq!(printed.status, Status::Problems, "{}", printed.text);
        assert!(
            printed.text.contains("Trigger has triggering"),
            "{}",
            printed.text
        );
        assert!(
            printed
                .text
                .contains("note: the map's AutoMap rules place #"),
            "{}",
            printed.text
        );
    }
}
