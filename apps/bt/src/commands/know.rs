//! `bt areas` and `bt reach`: how a map's cells connect (`10-agents.md`, §10.5).

use anyhow::Context as _;
use bt_core::Map;
use bt_script::{
    AreasOutcome, Context, Names, ReachOutcome, ReachRequest, ShapeCells, area_cells, parse_area,
};

use crate::cli::{AreasArgs, ReachArgs};
use crate::exit::{Printed, Status};
use crate::load::load_map;
use crate::output::printed;
use crate::run::Run;
use crate::script_input::shape_names;

/// The seed of rough shapes when `--seed` gives none: one fixed seed, so a command run twice
/// gives the same cells.
pub(crate) const DEFAULT_SEED: u64 = 1;

/// Runs `bt areas`.
pub(crate) fn areas(run: &Run, args: &AreasArgs) -> anyhow::Result<Printed> {
    let mut loaded = load_map(&args.map, run)?;
    let names = shape_names(&mut loaded, args.with.as_deref(), args.seed)?;
    let context = Context::new(args.seed.unwrap_or(DEFAULT_SEED));
    let shape = shape_cells(&loaded.map, &names, &context, &args.shape)?;
    let outcome = AreasOutcome::from_cells(&loaded.map, &shape, args.all);
    printed(&outcome, run.cli.json, Status::Done)
}

/// Runs `bt reach`.
pub(crate) fn reach(run: &Run, args: &ReachArgs) -> anyhow::Result<Printed> {
    let mut loaded = load_map(&args.map, run)?;
    let names = shape_names(&mut loaded, args.with.as_deref(), args.seed)?;
    let context = Context::new(args.seed.unwrap_or(DEFAULT_SEED));
    let cells = |text: &str| shape_cells(&loaded.map, &names, &context, text);
    let mut request = ReachRequest::new(cells(&args.from)?, cells(&args.to)?);
    if let Some(through) = &args.through {
        request = request.through(cells(through)?);
    }
    let outcome = ReachOutcome::from_request(&loaded.map, &names, &request)?;
    printed(&outcome, run.cli.json, Status::Done)
}

/// The cells of a shape written on the command line, as in scripts.
pub(crate) fn shape_cells(
    map: &Map,
    names: &Names,
    context: &Context,
    written: &str,
) -> anyhow::Result<ShapeCells> {
    let area = parse_area(written)
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("“{written}” is not a shape"))?;
    let cells = area_cells(map, names, context, &area)?;
    Ok(ShapeCells {
        written: written.to_owned(),
        cells,
    })
}

#[cfg(test)]
mod tests {
    use bt_test_data::Scratch;
    use bt_test_data::fixtures::MapBuilder;
    use bt_tmx::WriteRequest;
    use glam::UVec2;

    use super::*;
    use crate::cli::Command;

    fn run(args: &[&str]) -> anyhow::Result<Printed> {
        let run = Run::of_args(args, crate::folders::Folders::default());
        match &run.cli.command {
            Command::Areas(areas_args) => areas(&run, areas_args),
            Command::Reach(reach_args) => reach(&run, reach_args),
            _ => panic!("areas or reach"),
        }
    }

    #[test]
    fn areas_and_reach_read_shapes_as_scripts_do() {
        let scratch = Scratch::new("bt-know").unwrap();
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(6, 3))
            .build()
            .unwrap();
        let path = scratch.join("a.tmx");
        WriteRequest::for_map(&map, &path).write().unwrap();
        let path = path.to_str().unwrap();
        let areas = run(&["areas", path, "rect 0,0 2x3 minus 1,1"]).unwrap();
        assert!(
            areas
                .text
                .starts_with("rect 0,0 2x3 minus 1,1: 5 cells in 1 area"),
            "{}",
            areas.text
        );
        let reach = run(&[
            "reach",
            path,
            "0,0",
            "5,2",
            "--through",
            "all minus rect 2,0 1x2",
        ])
        .unwrap();
        assert!(reach.text.contains("yes:"), "{}", reach.text);
        let blocked = run(&[
            "reach",
            path,
            "0,0",
            "5,2",
            "--through",
            "all minus rect 2,0 1x3",
        ])
        .unwrap();
        assert!(!blocked.text.contains("yes:"), "{}", blocked.text);
        let error = run(&["areas", path, "lake"]).err().unwrap();
        assert!(format!("{error:#}").contains("is not a shape"), "{error:#}");
    }
}
