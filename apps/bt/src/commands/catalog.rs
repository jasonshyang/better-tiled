//! `bt catalog`: what a map can be built with, by name (its terrains, sprites, tiles and object
//! types), and with `--sheet` a numbered picture of what it lists (`10-agents.md`, §10.4).

use std::sync::Arc;

use anyhow::bail;
use bt_script::{CatalogOutcome, CatalogPart, CatalogRequest, Names};

use crate::cli::{CatalogArgs, CatalogWhat};
use crate::exit::{Printed, Status};
use crate::load::load_map;
use crate::output::printed;
use crate::picture_file::write_picture;
use crate::run::Run;

/// Runs `bt catalog`.
pub(crate) fn catalog(run: &Run, args: &CatalogArgs) -> anyhow::Result<Printed> {
    let loaded = load_map(&args.map, run)?;
    let map = &loaded.map;
    let names = Names::of(map);
    let mut request =
        CatalogRequest::new(map, &names).custom_types(Arc::clone(loaded.loader.custom_types()));
    request = match (args.what, &args.tiles) {
        (Some(CatalogWhat::Tiles), Some(tiles)) => request.tiles_of(tiles.clone()),
        (Some(what), None) => request.part(part_of(what)),
        (None, None) => request,
        (_, Some(tiles)) => bail!(
            "“{tiles}”: a tileset or tiles come after `tiles` alone (bt catalog MAP tiles \
             TILESET, or tiles TILESET:ID,…)"
        ),
    };
    if let Some(find) = &args.find {
        request = request.find(find.clone());
    }
    if let Some(sheet) = &args.sheet {
        request = request.sheet(sheet.display().to_string(), args.scale);
    }
    let cataloged = CatalogOutcome::from_request(&request)?;
    if let (Some(out), Some(sheet)) = (&args.sheet, &cataloged.sheet) {
        write_picture(out, sheet)?;
    }
    printed(&cataloged.outcome, run.cli.json, Status::Done)
}

/// What `bt catalog` is asked to list, as the catalog takes it.
fn part_of(what: CatalogWhat) -> CatalogPart {
    match what {
        CatalogWhat::Terrains => CatalogPart::Terrains,
        CatalogWhat::Sprites => CatalogPart::Sprites,
        CatalogWhat::Tiles => CatalogPart::Tiles,
        CatalogWhat::Types => CatalogPart::Types,
    }
}

#[cfg(test)]
mod tests {
    use bt_image::decode_file;
    use bt_test_data::Scratch;
    use glam::UVec2;

    use crate::commands::testing::{arg, bt, examples, lock, pond};

    #[test]
    fn the_catalog_names_what_a_map_is_built_with() {
        let scratch = Scratch::new("bt-catalog").unwrap();
        let map = pond(scratch.path());
        let map = arg(&map);
        let all = bt(&["catalog", map]).unwrap();
        assert_eq!(
            all.text,
            "terrains (paint with them; the tiles around fit themselves):\n  \
             grass: corner set of ground (16 tiles); borders water; on ground (52 cells)\n  \
             water: corner set of ground (16 tiles); borders grass; on ground (8 cells)\n\
             sprites (place them by their feet; 0 sprites):\n  \
             none\n\
             tiles: 32 tiles in 2 tilesets, 3 used (bt catalog MAP tiles lists them by id, with where each is used)\n\
             object types (object TYPE at X,Y key=value…):\n  \
             spawn (1 in this map)\n"
        );
        let found = bt(&["catalog", map, "terrains", "--find", "WAT"]).unwrap();
        assert!(found.text.contains("  water: "), "{}", found.text);
        assert!(!found.text.contains("grass:"), "{}", found.text);
        let tile = bt(&["catalog", map, "tiles", "things:2"]).unwrap();
        assert!(tile.text.contains("things:2: on deco ×2"), "{}", tile.text);
        let json = bt(&["catalog", map, "types", "--json"]).unwrap();
        assert!(
            json.text.starts_with("{\n  \"schema\": \"bt.catalog/1\","),
            "{}",
            json.text
        );
        let misplaced = bt(&["catalog", map, "terrains", "ground"]).unwrap_err();
        assert!(
            format!("{misplaced:#}").starts_with("“ground”: a tileset or tiles come after `tiles`"),
            "{misplaced:#}"
        );
        let unknown = bt(&["catalog", map, "tiles", "grund"]).unwrap_err();
        assert!(
            format!("{unknown:#}").contains("unknown tileset “grund”"),
            "{unknown:#}"
        );
    }

    #[test]
    fn a_sheet_replaces_the_picture_there_and_never_a_read_only_one() {
        let scratch = Scratch::new("bt-catalog-sheet").unwrap();
        let map = pond(scratch.path());
        let map = arg(&map);
        let out = scratch.join("tiles.png");
        let sheet = bt(&[
            "catalog",
            map,
            "tiles",
            "ground",
            "--sheet",
            arg(&out),
            "--scale",
            "2",
        ])
        .unwrap();
        assert!(
            sheet.text.contains(&format!("wrote {} (", out.display())),
            "{}",
            sheet.text
        );
        let first = decode_file(&out, None).unwrap();
        let again = bt(&["catalog", map, "tiles", "things", "--sheet", arg(&out)]).unwrap();
        assert!(
            again.text.contains("every tile with its id under it"),
            "{}",
            again.text
        );
        let second = decode_file(&out, None).unwrap();
        assert_ne!(
            UVec2::new(first.width, first.height),
            UVec2::new(second.width, second.height),
            "the picture there was replaced"
        );
        lock(&out);
        let refused = bt(&["catalog", map, "tiles", "--sheet", arg(&out)]).unwrap_err();
        assert!(
            format!("{refused:#}").ends_with("tiles.png: the file is read-only"),
            "{refused:#}"
        );
        assert_eq!(decode_file(&out, None).unwrap(), second, "left as it was");
        let scale = bt(&[
            "catalog",
            map,
            "--sheet",
            arg(&scratch.join("x.png")),
            "--scale",
            "9",
        ])
        .unwrap_err();
        assert!(
            format!("{scale:#}").contains("--scale for a sheet is a whole number from 1 to 8"),
            "{scale:#}"
        );
        assert!(!scratch.join("x.png").exists(), "nothing written");
    }

    #[test]
    fn tileds_examples_catalog_their_terrains_and_sprites() {
        let Some(examples) = examples() else {
            return;
        };
        let desert = bt(&["catalog", arg(&examples.join("desert.tmx")), "terrains"]).unwrap();
        let terrains: Vec<&str> = desert
            .text
            .lines()
            .filter(|line| line.starts_with("  "))
            .collect();
        assert_eq!(terrains.len(), 4, "{}", desert.text);
        let scratch = Scratch::new("bt-catalog-knight").unwrap();
        let out = scratch.join("sprites.png");
        let knight = bt(&[
            "catalog",
            arg(&examples.join("sticker-knight/map/sandbox.tmx")),
            "sprites",
            "--find",
            "shadow",
            "--sheet",
            arg(&out),
        ])
        .unwrap();
        assert!(
            knight
                .text
                .contains("the numbered pictures of the list above"),
            "{}",
            knight.text
        );
        assert!(decode_file(&out, None).is_ok());
    }
}
