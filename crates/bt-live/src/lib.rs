//! The live link between `bt` and a running better-tiled (`docs/design/11-live-link.md`): the
//! private folder editors listen in, one line of JSON each way over a Unix domain socket, the
//! requests and answers with their version, the client that finds editors and asks them, and
//! the server that accepts connections and hands work to the window's thread with a deadline.
//!
//! It knows no maps. The editor's handler answers requests, running scripts through
//! `bt-script` and asking the editor core for the map; `bt` decides when to work through an
//! editor and when with the files. Both find the folder with [`live_dir`]; the editor listens
//! with [`Server::start`] and reaches its window through [`Jobs`]; `bt` finds editors with
//! [`editors`] and asks one with [`Editor::ask`].
//!
//! Windows has no live link: the crate builds there, [`Server::start`] says so and [`editors`]
//! finds none.

mod client;
mod dir;
mod error;
mod jobs;
mod protocol;
mod server;
#[cfg(test)]
mod testing;
#[cfg(unix)]
mod wire;

pub use client::{Editor, Editors, Problem, editors};
pub use dir::{LIVE_DIR_VAR, live_dir};
pub use error::Error;
pub use jobs::{JOB_WAIT, Job, Jobs, LONG_JOB_WAIT, Reply};
pub use protocol::{
    DocStatus, Done, EditRequest, Edited, KeyboardFocus, MARK_PALETTE, MapText, MarkSpec, PROTOCOL,
    Request, Response, Run, Status, TilesetText, canonical_path, cells_from_runs, runs_from_cells,
};
pub use server::{Handler, SOCKET_CHECK_INTERVAL, Server};
