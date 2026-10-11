//! `bt check`: problems first, then what objects stand on (`10-agents.md`, §11). It exits 5
//! when it lists a problem, so scripts tell "problems found" from "could not run".

use bt_script::CheckOutcome;
use bt_utils::paths::display_name;

use crate::cli::MapArg;
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
    let outcome = CheckOutcome::of(
        &loaded.map,
        display_name(&args.map),
        &warnings,
        loaded.loader.custom_types(),
        project_file.as_deref(),
    );
    let status = if outcome.has_problems() {
        Status::Problems
    } else {
        Status::Done
    };
    printed(&outcome, run.cli.json, status)
}
