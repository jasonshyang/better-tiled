//! `bt diff`: what differs between two maps, or the editor's unsaved changes to one
//! (`10-agents.md`, §10.7).

use anyhow::Context as _;
use bt_script::DiffOutcome;
use bt_utils::paths::display_name;

use crate::cli::DiffArgs;
use crate::exit::{Printed, Status};
use crate::load::{MapCopy, load_copy, load_map};
use crate::output::printed;
use crate::run::Run;

/// Runs `bt diff`.
pub(crate) fn diff(run: &Run, args: &DiffArgs) -> anyhow::Result<Printed> {
    let outcome = match &args.other {
        Some(other) => {
            let before = load_map(&args.map, run)?;
            let after = load_map(other, run)?;
            DiffOutcome::between(&before.map, &after.map)
                .named(display_name(&args.map), display_name(other))
        }
        None => {
            let editor = load_copy(&args.map, run, MapCopy::Editor).with_context(|| {
                "bt diff MAP lists the changes the editor has not saved; bt diff MAP OTHER \
                 compares two files"
            })?;
            let file = load_copy(&args.map, run, MapCopy::File)?;
            DiffOutcome::between(&file.map, &editor.map).unsaved_in_editor(display_name(&args.map))
        }
    };
    printed(&outcome, run.cli.json, Status::Done)
}

#[cfg(test)]
mod tests {
    use bt_test_data::Scratch;
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use bt_tmx::WriteRequest;
    use glam::UVec2;

    use super::*;
    use crate::cli::Command;
    use crate::folders::Folders;

    #[test]
    fn two_files_are_compared_layer_by_layer() {
        let scratch = Scratch::new("bt-diff").unwrap();
        let atlas = Atlas::builder()
            .name("tiles")
            .build_in(scratch.path())
            .unwrap();
        for (name, rows) in [("a.tmx", "0 1\n2 3"), ("b.tmx", "0 1\n2 .")] {
            let map = MapBuilder::new(UVec2::splat(16), UVec2::new(2, 2))
                .tileset(atlas.tileset.clone())
                .tile_layer("ground", rows)
                .build()
                .unwrap();
            WriteRequest::for_map(&map, &scratch.join(name))
                .write()
                .unwrap();
        }
        let a = scratch.join("a.tmx");
        let b = scratch.join("b.tmx");
        let run = Run::of_args(
            &["diff", a.to_str().unwrap(), b.to_str().unwrap()],
            Folders::default(),
        );
        let Command::Diff(args) = &run.cli.command else {
            panic!("a diff")
        };
        let printed = diff(&run, args).unwrap();
        assert!(
            printed.text.starts_with("a.tmx → b.tmx"),
            "{}",
            printed.text
        );
        assert!(printed.text.contains("ground"), "{}", printed.text);
    }
}
