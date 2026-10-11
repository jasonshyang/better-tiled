//! What a script runs on when `bt` runs it on a file (`10-agents.md`, §8): what was painted,
//! with the AutoMap layers' setup, their rules and the map as it shows when the map has them;
//! the definitions of a `--with` file; the seed. The commands that read shapes (`areas`,
//! `reach`, `mark`, `render`) read them with the names a `--with` file defines.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, anyhow};
use bt_script::{Names, RunInput, RunInputBuilder, Script, defined_names, parse_definitions};
use bt_utils::paths::display_name;

use crate::load::LoadedMap;

/// The input of a script on `loaded`'s map: what was painted, with its AutoMap layers when it
/// has them (their rules read through the map's loader); `with`'s definitions run first; the
/// seed as `--seed` gives it, else the default.
pub(crate) fn script_input(
    loaded: &mut LoadedMap,
    with: Option<&Path>,
    seed: Option<u64>,
) -> anyhow::Result<RunInputBuilder> {
    let shown = Arc::new(loaded.map.clone());
    let mut builder = match &loaded.layers {
        Some(layers) => {
            let rules = bt_automap::files::load(
                &mut loaded.loader,
                &layers.setup.rules,
                Some(&loaded.path),
            )
            .with_context(|| {
                format!(
                    "the AutoMap rules of {} can't be read",
                    display_name(&loaded.path)
                )
            })?;
            RunInput::builder(Arc::new(layers.painted.clone()), &loaded.path).automap_layers(
                layers.setup.clone(),
                Arc::new(rules),
                shown,
            )
        }
        None => RunInput::builder(shown, &loaded.path),
    };
    if let Some(seed) = seed {
        builder = builder.seed(seed);
    }
    if let Some(file) = with {
        builder = builder.definitions(read_definitions(file)?);
    }
    Ok(builder)
}

/// The names shapes on `loaded`'s map are read with: the map's own, and what `with` defines.
pub(crate) fn shape_names(
    loaded: &mut LoadedMap,
    with: Option<&Path>,
    seed: Option<u64>,
) -> anyhow::Result<Names> {
    if with.is_none() {
        return Ok(Names::of(&loaded.map));
    }
    let input = script_input(loaded, with, seed)?.build();
    defined_names(&input, &mut loaded.loader).map_err(|error| anyhow!(error))
}

/// The text of a `--with` file.
pub(crate) fn definitions_text(file: &Path) -> anyhow::Result<String> {
    std::fs::read_to_string(file).with_context(|| format!("{} can't be read", file.display()))
}

/// The definitions of a `--with` file: define lines and seeds only.
pub(crate) fn read_definitions(file: &Path) -> anyhow::Result<Script> {
    parse_definitions(&definitions_text(file)?)
        .map_err(|error| anyhow!("--with {}, {error}", display_name(file)))
}

#[cfg(test)]
mod tests {
    use bt_test_data::Scratch;
    use bt_test_data::fixtures::MapBuilder;
    use bt_tmx::WriteRequest;
    use glam::UVec2;

    use super::*;
    use crate::load::load_map;
    use crate::run::Run;

    #[test]
    fn definitions_name_areas_and_a_line_that_is_no_definition_is_refused() {
        let scratch = Scratch::new("bt-script-input").unwrap();
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(6, 4))
            .build()
            .unwrap();
        let path = scratch.join("a.tmx");
        WriteRequest::for_map(&map, &path).write().unwrap();
        let with = scratch.join("defs.txt");
        std::fs::write(&with, "# shared names\ndefine yard = area rect 1,1 2x2\n").unwrap();
        let mut loaded = load_map(&path, &Run::for_test(None)).unwrap();
        let names = shape_names(&mut loaded, Some(&with), None).unwrap();
        let area = bt_script::parse_area("where yard").unwrap();
        let cells =
            bt_script::area_cells(&loaded.map, &names, &bt_script::Context::new(1), &area).unwrap();
        assert_eq!(cells.len(), 4);
        std::fs::write(&with, "define yard = area rect 1,1 2x2\nclear all\n").unwrap();
        let error = shape_names(&mut loaded, Some(&with), None).err().unwrap();
        assert!(
            format!("{error:#}").starts_with("--with defs.txt, line 2"),
            "{error:#}"
        );
    }
}
