//! What an export writes, built in full in memory before anything is written (docs/design/12-godot-export.md).

use std::path::PathBuf;

/// What a file is, which decides when it is written (see contract.rs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Scene,
    TileSet,
    /// A copy of an image from outside the Godot project.
    Image,
    /// `images/.sources.json`, saying where each copy came from.
    Manifest,
}

pub(super) struct Output {
    pub kind: Kind,
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    /// What the output is made from: a scene's map, the image of a copy.
    pub source: Option<PathBuf>,
    /// An image copy: the hash `images/.sources.json` has for what the export last wrote there.
    pub recorded: Option<u64>,
    /// What a generated file's header and `metadata/bt_source` say it is made from when that is
    /// no one file (a shared TileSet's tilesets); otherwise `source` says it.
    pub from: Option<String>,
    /// A shared TileSet: the other generated scenes using it, which a new one would leave
    /// pointing at sources it numbers otherwise.
    pub users: Vec<PathBuf>,
}

/// The files an export writes, in the order they were planned.
#[derive(Default)]
pub(super) struct Plan {
    pub outputs: Vec<Output>,
    /// Image copies nothing uses any more, unchanged since they were copied: removed.
    pub prune: Vec<PathBuf>,
}

impl Plan {
    pub fn add(&mut self, kind: Kind, path: PathBuf, bytes: Vec<u8>, source: Option<PathBuf>) {
        self.outputs.push(Output {
            kind,
            path,
            bytes,
            source,
            recorded: None,
            from: None,
            users: Vec::new(),
        });
    }

    /// Adds a shared TileSet, made from what `from` says and used by the scenes `users` too.
    pub fn tileset(&mut self, path: PathBuf, bytes: Vec<u8>, from: String, users: Vec<PathBuf>) {
        self.outputs.push(Output {
            kind: Kind::TileSet,
            path,
            bytes,
            source: None,
            recorded: None,
            from: Some(from),
            users,
        });
    }

    /// Adds a copy of the image `source`, `recorded` being the hash of what was copied there
    /// before.
    pub fn copy(&mut self, path: PathBuf, bytes: Vec<u8>, source: PathBuf, recorded: Option<u64>) {
        self.outputs.push(Output {
            kind: Kind::Image,
            path,
            bytes,
            source: Some(source),
            recorded,
            from: None,
            users: Vec::new(),
        });
    }
}
