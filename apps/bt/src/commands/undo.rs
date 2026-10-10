//! `bt undo`: the last `bt edit` of a map taken back (`10-agents.md`, §12).

use anyhow::Context;
use bt_project::{AutoMapRecord, EntryPlace, ProjectFile};
use bt_script::{History, UndoOutcome, Undone};
use bt_utils::atomic_write::{WriteMode, write_file};

use crate::cli::{Cli, UndoArgs};
use crate::exit::{Printed, Status};
use crate::folders::Folders;
use crate::output::printed;

/// Runs `bt undo` on the map's file: refused when the file was saved since the edit unless
/// `--force`; the kept bytes go back atomically, never over a read-only file, with the map's
/// AutoMap record as it was.
pub(crate) fn undo(cli: &Cli, args: &UndoArgs, folders: &Folders) -> anyhow::Result<Printed> {
    let path = std::fs::canonicalize(&args.map)
        .with_context(|| format!("{} can't be read", args.map.display()))?;
    let history = History::new(folders.history()?);
    let outcome = |undone| UndoOutcome {
        map: args.map.clone(),
        undone,
    };
    let Some(last) = history.last_edit(&path)? else {
        return printed(&outcome(Undone::Nothing), cli.json, Status::Failed);
    };
    let bytes =
        std::fs::read(&path).with_context(|| format!("{} can't be read", args.map.display()))?;
    let saved_since = !last.wrote(&bytes);
    if saved_since && !args.force {
        let refused = outcome(Undone::SavedSince { label: last.label });
        return printed(&refused, cli.json, Status::Refused);
    }
    write_file(&path, &last.before, WriteMode::Atomic)?;
    put_back_record(&path, last.entry.as_deref(), folders)?;
    let earlier = history.forget_last_edit(&path)?;
    let undone = Undone::File {
        label: last.label,
        earlier,
        forced: saved_since,
    };
    printed(&outcome(undone), cli.json, Status::Done)
}

/// Puts the map's AutoMap record back as it was before the edit: `entry` (its JSON), or none.
fn put_back_record(
    map: &std::path::Path,
    entry: Option<&str>,
    folders: &Folders,
) -> anyhow::Result<()> {
    let place = EntryPlace::for_map(map, None, folders.home.as_deref())?;
    let record: Option<AutoMapRecord> = entry
        .map(serde_json::from_str)
        .transpose()
        .context("the kept AutoMap record is damaged")?;
    if record.is_none() && !place.file.exists() {
        return Ok(());
    }
    let locks = folders.locks()?;
    ProjectFile::edit(&place.file, &locks, |document| match record {
        Some(record) => document.set_automap(&place.key, record),
        None => {
            document.remove_automap(&place.key);
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use bt_script::EditToKeep;
    use bt_test_data::Scratch;
    use clap::Parser;

    use super::*;

    fn folders(scratch: &Scratch) -> Folders {
        std::fs::create_dir_all(scratch.join("locks")).unwrap();
        Folders {
            home: None,
            data: Some(scratch.join("data")),
            locks: Some(scratch.join("locks")),
        }
    }

    fn run(args: &[&str], folders: &Folders) -> Printed {
        let cli = Cli::try_parse_from(std::iter::once("bt").chain(args.iter().copied())).unwrap();
        match &cli.command {
            crate::cli::Command::Undo(undo_args) => undo(&cli, undo_args, folders).unwrap(),
            _ => panic!("an undo"),
        }
    }

    /// `map` as `bt edit` would leave it: `after` written, `before` kept.
    fn edited(folders: &Folders, map: &std::path::Path, before: &str, after: &str) {
        let path = std::fs::canonicalize(map).unwrap();
        History::new(folders.history().unwrap())
            .keep_edit(
                &path,
                EditToKeep {
                    before: before.as_bytes(),
                    written: after.as_bytes(),
                    label: "paint water",
                    entry: None,
                },
            )
            .unwrap();
        std::fs::write(map, after).unwrap();
    }

    #[test]
    fn the_last_edit_is_taken_back_and_a_saved_file_is_refused_without_force() {
        let scratch = Scratch::new("bt-undo").unwrap();
        let folders = folders(&scratch);
        let map = scratch.join("a.tmx");
        std::fs::write(&map, "v0").unwrap();
        edited(&folders, &map, "v0", "v1");
        let map_arg = map.to_str().unwrap();
        let printed = run(&["undo", map_arg], &folders);
        assert_eq!(printed.status, Status::Done);
        assert!(
            printed.text.contains("took back “paint water”"),
            "{}",
            printed.text
        );
        assert_eq!(std::fs::read_to_string(&map).unwrap(), "v0");
        assert_eq!(run(&["undo", map_arg], &folders).status, Status::Failed);

        edited(&folders, &map, "v0", "v1");
        std::fs::write(&map, "saved in the editor").unwrap();
        let refused = run(&["undo", map_arg], &folders);
        assert_eq!(refused.status, Status::Refused);
        assert!(
            refused.text.starts_with("problem: a.tmx was saved since"),
            "{}",
            refused.text
        );
        assert_eq!(
            std::fs::read_to_string(&map).unwrap(),
            "saved in the editor"
        );
        let forced = run(&["undo", map_arg, "--force"], &folders);
        assert_eq!(forced.status, Status::Done);
        assert_eq!(std::fs::read_to_string(&map).unwrap(), "v0");
    }

    #[test]
    fn a_read_only_file_is_never_written_over() {
        let scratch = Scratch::new("bt-undo-read-only").unwrap();
        let folders = folders(&scratch);
        let map = scratch.join("a.tmx");
        std::fs::write(&map, "v0").unwrap();
        edited(&folders, &map, "v0", "v1");
        let mut permissions = std::fs::metadata(&map).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&map, permissions).unwrap();
        let cli = Cli::try_parse_from(["bt", "undo", map.to_str().unwrap()]).unwrap();
        let crate::cli::Command::Undo(args) = &cli.command else {
            panic!("an undo")
        };
        let error = undo(&cli, args, &folders).err().unwrap();
        assert!(format!("{error:#}").contains("read-only"), "{error:#}");
        assert_eq!(std::fs::read_to_string(&map).unwrap(), "v1");
        let history = History::new(folders.history().unwrap());
        let path = std::fs::canonicalize(&map).unwrap();
        assert_eq!(
            history.kept_edits(&path).unwrap(),
            1,
            "the edit is still kept"
        );
    }
}
