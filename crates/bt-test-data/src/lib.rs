//! Test support for the workspace: finding the corpora that are never committed (Tiled's sources
//! in `reference/`, the purchased packs in `assets/`), deciding what happens when one is missing,
//! and giving each test a folder of its own.
//!
//! A test that needs something missing fails and names it. Setting `BT_SKIP_MISSING_TEST_DATA=1`
//! turns that into a skip that says what it skipped, and `BT_SKIP_LOG` names a file every skip is
//! appended to, which continuous integration checks with the `check-skips` program.

mod corpus;
mod scratch;
mod skip;
mod skip_check;

pub use corpus::{test_data, workspace_root};
pub use scratch::Scratch;
pub use skip::{SKIP_LOG_VAR, SKIP_VAR, missing, skipping};
pub use skip_check::disallowed_skips;
