//! Tiled's automapping for better-tiled: rule maps compiled once and run on maps in memory with
//! the results Tiled 1.12 gives, randomness drawn from a seed, every attempt, check and write
//! reported to an observer the engine is generic over, and every problem typed.
//!
//! The crate ports Tiled's `AutoMapper` (`src/tiled/automapper.cpp`), `AutomappingManager`
//! (`src/tiled/automappingmanager.cpp`, with `src/tiled/automapperwrapper.cpp`) and
//! `automappingutils.cpp`, Copyright 2010-2026 Stefan Beller, Seanba, Thorbjørn Lindeijer and
//! Tiled's contributors, licensed under the GNU GPL version 2 or later and used here under
//! version 3 or later. `docs/design/08-automap.md` is the design; its "As built" section lists
//! every way the crate departs from Tiled and why.

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the engine, which comes next, uses the boxes")
)]
mod bx;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the engine, which comes next, uses the sets of cells"
    )
)]
mod cell_set;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the engine, which comes next, draws its random numbers here"
    )
)]
mod rng;
mod rule_ref;

pub use rule_ref::RuleRef;
