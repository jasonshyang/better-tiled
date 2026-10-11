//! `bt new`: an empty map, like another (its tilesets and layers, emptied) or from a size, a tile
//! size, tilesets and layers (`10-agents.md`, §10.7). A file that is there is written over only
//! with `--force`; without, the answer is a refusal (exit 3) and nothing is written.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use bt_core::{FileRef, TilesetRef};
use bt_script::{Line, NewOutcome, NewRequest, Outcome, Report, parse_size};
use bt_tmx::{Loader, WriteRequest};
use bt_utils::paths::display_name;
use glam::UVec2;
use serde::{Deserialize, Serialize};

use crate::cli::NewArgs;
use crate::exit::{Printed, Status};
use crate::load::load_map;
use crate::output::printed;
use crate::run::Run;

/// What a command answers when it would write over a file it was not told to: refused (exit 3),
/// nothing written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RefusedOutcome {
    /// The file that is there.
    pub(crate) file: String,
    /// Why it was refused, and the way to do it anyway.
    pub(crate) reason: String,
}

impl Outcome for RefusedOutcome {
    const SCHEMA: &'static str = "bt.refused/1";

    fn report(&self) -> Report {
        let mut report = Report::new();
        report.add_line(Line::problem(format!("{}: {}", self.file, self.reason)));
        report
    }
}

/// Runs `bt new`. The map is made from what the options name before the file is looked at, so
/// a refusal means only that the file is there; it is written when the file isn't there, or
/// `--force` says so.
pub(crate) fn new_map(run: &Run, args: &NewArgs) -> anyhow::Result<Printed> {
    let path = new_map_path(&args.map)?;
    let size = args.size.as_deref().map(size_of).transpose()?;
    let tile = args.tile.as_deref().map(size_of).transpose()?;
    let other = args
        .like
        .as_deref()
        .map(|other| load_map(other, run))
        .transpose()?;
    let mut loader = Loader::builder().build();
    let request = match (&other, size, tile) {
        (Some(other), _, _) => {
            let request = NewRequest::like(&other.map, &other.path, &path);
            match size {
                Some(size) => request.size(size),
                None => request,
            }
        }
        (None, Some(size), Some(tile)) => {
            let tilesets = args
                .tilesets
                .iter()
                .map(|file| external_tileset(&mut loader, file))
                .collect::<anyhow::Result<Vec<TilesetRef>>>()?;
            NewRequest::sized(&path, size, tile)
                .tilesets(tilesets)
                .layers(args.layers.clone())
        }
        (None, _, _) => bail!(
            "bt new needs --like OTHER.tmx, or --size WxH with --tile WxH (the tiles' size in \
             pixels)"
        ),
    };
    let created = NewOutcome::from_request(&request)?;
    if !args.force && std::fs::symlink_metadata(&path).is_ok() {
        let refused = RefusedOutcome {
            file: display_name(&args.map),
            reason: "the file is there already: bt new writes over a file only with --force"
                .to_owned(),
        };
        return printed(&refused, run.cli.json, Status::Refused);
    }
    WriteRequest::for_map(&created.map, &path).write()?;
    printed(&created.outcome, run.cli.json, Status::Done)
}

/// A size option, `WxH` in positive whole numbers.
fn size_of(written: &str) -> anyhow::Result<UVec2> {
    Ok(parse_size(written).map_err(anyhow::Error::msg)?.as_uvec2())
}

/// Where the new map goes: in its folder as the system has it (absolute, links followed), so
/// the files the map refers to are written relative to the folder it is really in.
fn new_map_path(map: &Path) -> anyhow::Result<PathBuf> {
    let name = map
        .file_name()
        .with_context(|| format!("{} names no file to write", map.display()))?;
    let folder = match map.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let folder = std::fs::canonicalize(folder)
        .with_context(|| format!("the folder of {} isn't there", map.display()))?;
    Ok(folder.join(name))
}

/// The tileset file `file`, read with `loader`, as the new map refers to it: written relative
/// to the map's folder.
fn external_tileset(loader: &mut Loader, file: &Path) -> anyhow::Result<TilesetRef> {
    let unreadable = || format!("the tileset {} can't be read", file.display());
    let path = std::fs::canonicalize(file).with_context(unreadable)?;
    let loaded = loader.load_tileset(&path).with_context(unreadable)?;
    Ok(TilesetRef {
        file: Some(FileRef::new("", path)),
        first_gid: None,
        tileset: loaded.value,
    })
}

#[cfg(test)]
mod tests {
    use bt_test_data::Scratch;
    use bt_test_data::fixtures::Atlas;
    use bt_tmx::{Loader, WriteRequest};

    use crate::commands::testing::{arg, bt, examples, lock};
    use crate::exit::Status;

    #[test]
    fn a_map_from_a_size_refers_to_its_tilesets_from_its_folder() {
        let scratch = Scratch::new("bt-new").unwrap();
        let atlas = Atlas::builder()
            .name("Ground Tiles")
            .build_in(scratch.path())
            .unwrap();
        let tsx = scratch.join("ground.tsx");
        WriteRequest::for_tileset(&atlas.tileset, &tsx)
            .write()
            .unwrap();
        std::fs::create_dir(scratch.join("maps")).unwrap();
        let map = scratch.join("maps/fresh.tmx");
        let created = bt(&[
            "new",
            arg(&map),
            "--size",
            "10x5",
            "--tile",
            "16x16",
            "--tilesets",
            arg(&tsx),
            "--layers",
            "ground:tiles,props:objects",
        ])
        .unwrap();
        assert_eq!(created.status, Status::Done);
        assert_eq!(
            created.text,
            "created fresh.tmx: 10x5 cells of 16x16 px\n  \
             layers (top first): props (objects), ground (tiles)\n  \
             tilesets: ground-tiles\n"
        );
        let read = Loader::default().load_map(&map).unwrap().value;
        assert_eq!((read.width, read.height), (10, 5));
        assert_eq!(
            read.tilesets[0].file.as_ref().unwrap().source,
            "../ground.tsx",
            "written from the map's folder"
        );
        let json = bt(&[
            "new",
            arg(&scratch.join("other.tmx")),
            "--size",
            "2x2",
            "--tile",
            "8x8",
            "--json",
        ])
        .unwrap();
        assert!(
            json.text.starts_with("{\n  \"schema\": \"bt.new/1\","),
            "{}",
            json.text
        );
    }

    #[test]
    fn a_file_that_is_there_is_written_over_only_with_force() {
        let scratch = Scratch::new("bt-new-force").unwrap();
        let taken = scratch.join("taken.tmx");
        std::fs::write(&taken, "mine").unwrap();
        let new = |more: &[&str]| {
            let mut all = vec!["new", arg(&taken), "--size", "3x2", "--tile", "16x16"];
            all.extend_from_slice(more);
            bt(&all)
        };
        let refused = new(&[]).unwrap();
        assert_eq!(refused.status, Status::Refused);
        assert_eq!(
            refused.text,
            "problem: taken.tmx: the file is there already: bt new writes over a file only with \
             --force\n"
        );
        assert_eq!(std::fs::read_to_string(&taken).unwrap(), "mine");
        let json = new(&["--json"]).unwrap();
        assert_eq!(json.status, Status::Refused);
        assert!(
            json.text.starts_with("{\n  \"schema\": \"bt.refused/1\","),
            "{}",
            json.text
        );
        assert_eq!(std::fs::read_to_string(&taken).unwrap(), "mine");
        lock(&taken);
        let locked = new(&["--force"]).unwrap_err();
        assert!(
            format!("{locked:#}").ends_with("taken.tmx: the file is read-only"),
            "{locked:#}"
        );
        assert_eq!(std::fs::read_to_string(&taken).unwrap(), "mine");
        let mut permissions = std::fs::metadata(&taken).unwrap().permissions();
        #[allow(
            clippy::permissions_set_readonly_false,
            reason = "the test unlocks the file it locked"
        )]
        permissions.set_readonly(false);
        std::fs::set_permissions(&taken, permissions).unwrap();
        let forced = new(&["--force"]).unwrap();
        assert_eq!(forced.status, Status::Done);
        let read = Loader::default().load_map(&taken).unwrap().value;
        assert_eq!((read.width, read.height), (3, 2));
    }

    #[test]
    fn what_a_new_map_lacks_is_said_and_nothing_is_written() {
        let scratch = Scratch::new("bt-new-mistakes").unwrap();
        let map = scratch.join("x.tmx");
        let error = |more: &[&str]| {
            let mut all = vec!["new", arg(&map)];
            all.extend_from_slice(more);
            format!("{:#}", bt(&all).unwrap_err())
        };
        assert!(
            error(&[]).starts_with("bt new needs --like OTHER.tmx, or --size WxH with --tile WxH"),
            "{}",
            error(&[])
        );
        assert!(
            error(&["--size", "4x4"]).starts_with("bt new needs"),
            "{}",
            error(&["--size", "4x4"])
        );
        assert_eq!(
            error(&["--size", "0x4", "--tile", "16x16"]),
            "“0x4” is not a size: W and H are positive whole numbers"
        );
        assert_eq!(
            error(&[
                "--size",
                "4x4",
                "--tile",
                "16x16",
                "--layers",
                "floor:water"
            ]),
            "--layers floor:water: a layer is NAME:tiles or NAME:objects"
        );
        assert!(
            error(&["--size", "4x4", "--tile", "16x16", "--tilesets", "none.tsx"])
                .starts_with("the tileset none.tsx can't be read"),
            "{}",
            error(&["--size", "4x4", "--tile", "16x16", "--tilesets", "none.tsx"])
        );
        assert!(
            format!(
                "{:#}",
                bt(&[
                    "new",
                    arg(&scratch.join("no/x.tmx")),
                    "--size",
                    "1x1",
                    "--tile",
                    "8x8"
                ])
                .unwrap_err()
            )
            .starts_with("the folder of "),
        );
        assert!(!map.exists(), "nothing was written");
    }

    #[test]
    fn a_map_like_tileds_desert_keeps_its_tileset_and_layer_emptied() {
        let Some(examples) = examples() else {
            return;
        };
        let scratch = Scratch::new("bt-new-like").unwrap();
        let oasis = scratch.join("oasis.tmx");
        let created = bt(&[
            "new",
            arg(&oasis),
            "--like",
            arg(&examples.join("desert.tmx")),
            "--size",
            "20x10",
        ])
        .unwrap();
        assert_eq!(
            created.text,
            "created oasis.tmx: 20x10 cells of 32x32 px\n  \
             layers (top first): Ground (tiles)\n  \
             tilesets: desert\n"
        );
        let read = Loader::default().load_map(&oasis).unwrap().value;
        assert_eq!(read.tilesets[0].tileset.name, "Desert");
        let ground = read.layers[0].as_tile_layer().unwrap();
        assert_eq!(ground.cells().count(), 0, "emptied");
    }
}
