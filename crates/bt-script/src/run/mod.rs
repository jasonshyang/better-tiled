//! Running a script (`10-agents.md`, §8): the operations of a parsed [`Script`] applied in order
//! to a copy of a map, all or nothing. `bt` and the editor's live link both call
//! [`run_script`]: the input says what the script runs on ([`RunInput`]), the result holds the
//! maps after it and the report ([`EditResult`]), and a failure names its line, the operation as
//! written and what is wrong ([`RunError`]), leaving the caller's map as it was.

mod choices;
mod choose;
mod definitions;
mod draw;
mod error;
mod input;
mod map_ops;
mod objects;
mod pieces;
mod result;
mod rules;
mod runner;
mod sprites;
mod terrain;
mod tiles;
mod values;

pub use error::{RunError, RunFailure};
pub use input::{DEFAULT_SEED, RunInput, RunInputBuilder};
pub use result::EditResult;

use bt_tmx::Loader;

use crate::names::Names;
use crate::script::Script;

/// Runs `script` on a copy of `input`'s map, reading the maps and rules it names through
/// `loader` (the map's own, so tilesets are shared). The `--with` definitions run first. On
/// success the result holds the maps after the script and what it did; on failure the error
/// names the line, and nothing the script did is kept.
pub fn run_script(
    script: &Script,
    input: &RunInput,
    loader: &mut Loader,
) -> Result<EditResult, RunError> {
    runner::Runner::new(input, loader).run(script, input)
}

/// The names of `input`'s map once its `--with` definitions have run: what `bt areas`, `reach`,
/// `mark` and `render` read shapes with. Nothing the definitions did to the map is kept.
pub fn defined_names(input: &RunInput, loader: &mut Loader) -> Result<Names, RunError> {
    let mut runner = runner::Runner::new(input, loader);
    runner.run_definitions(input)?;
    Ok(runner.into_names())
}
