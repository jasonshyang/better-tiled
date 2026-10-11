//! `bt view`: the map as text, one character per cell, or with `--ids` each cell's tile id and
//! with `--corners` the terrain at each corner of the cells (`10-agents.md`, §10.2).

use bt_script::{Names, ViewOutcome, ViewRequest, parse_region};
use bt_utils::paths::display_name;

use crate::cli::{ViewArgs, ViewBy};
use crate::commands::layers::named_layers;
use crate::commands::look::shown_or_painted;
use crate::exit::{Printed, Status};
use crate::load::load_map;
use crate::output::printed;
use crate::run::Run;

/// Runs `bt view`.
pub(crate) fn view(run: &Run, args: &ViewArgs) -> anyhow::Result<Printed> {
    let loaded = load_map(&args.map, run)?;
    let map = shown_or_painted(&loaded, args.painted)?;
    let names = Names::of(map);
    let mut request = ViewRequest::new(map, &names)
        .title(display_name(&args.map))
        .runs(args.runs);
    if !args.region.is_empty() {
        let region = parse_region(&args.region.join(" ")).map_err(anyhow::Error::msg)?;
        request = request.region(region);
    }
    if !args.layers.is_empty() {
        request = request.layers(named_layers(map, &names, &args.layers)?);
    }
    if let Some(by) = args.by {
        request = request.by(script_view_by(by));
    }
    if args.no_objects {
        request = request.without_objects();
    }
    if let Some(step) = args.step {
        request = request.step(step);
    }
    let outcome = if args.ids {
        ViewOutcome::ids(&request)?
    } else if args.corners {
        ViewOutcome::corners(&request)?
    } else {
        ViewOutcome::from_request(&request)?
    };
    printed(&outcome, run.cli.json, Status::Done)
}

/// What `--by` names, as the view takes it.
pub(crate) fn script_view_by(by: ViewBy) -> bt_script::ViewBy {
    match by {
        ViewBy::Terrain => bt_script::ViewBy::Terrain,
        ViewBy::Tile => bt_script::ViewBy::Tile,
        ViewBy::Layer => bt_script::ViewBy::Layer,
    }
}

#[cfg(test)]
mod tests {
    use bt_test_data::Scratch;

    use crate::commands::testing::{arg, bt, examples, pond, sewers_with_automap_layers};

    #[test]
    fn a_map_reads_as_text_by_terrain_tile_or_layer_with_its_ids_and_corners() {
        let scratch = Scratch::new("bt-view").unwrap();
        let map = pond(scratch.path());
        let map = arg(&map);
        let view = bt(&["view", map]).unwrap();
        assert_eq!(
            view.text,
            "pond.tmx: cells 0,0 10x6; layers deco, ground\n    \
             0123456789\n \
             0  \"d\"\"\"\"\"\"\"\"\n \
             1  \"d\"\"\"\"\"\"\"\"\n \
             2  \"\"\"~~~~\"\"\"\n \
             3  \"\"\"~~~~\"\"\"\n \
             4  \"\"\"\"\"\"\"\"\"\"\n \
             5  \"\"\"\"\"\"\"\"S\"\n\
             legend:\n  \
             .  nothing, or only the edge of a terrain\n  \
             \"  grass (terrain on ground): 50 cells\n  \
             ~  water (terrain on ground): 8 cells\n  \
             d  tiles of deco that are no terrain's (things:2 ×2): 2 cells\n  \
             S  spawn on design: #1 8.5,5.5\n"
        );
        let narrow = bt(&[
            "view",
            map,
            "--layers",
            "ground",
            "--by",
            "tile",
            "--runs",
            "--region",
            "2,2",
            "3x1",
            "--no-objects",
        ])
        .unwrap();
        assert_eq!(
            narrow.text,
            "pond.tmx: cells 2,2 3x1; layers ground\n    \
             234\n \
             2  app   a2 p3-4\n\
             legend:\n  \
             .  nothing\n  \
             p  ground:15 (on ground): 2 cells\n  \
             a  ground:0 (on ground): 1 cell\n"
        );
        let by_layer = bt(&["view", map, "--by", "layer", "--step", "2"]).unwrap();
        assert!(
            by_layer.text.starts_with(
                "pond.tmx: cells 0,0 10x6, one character per 2x2 cells; layers deco, ground\n"
            ),
            "{}",
            by_layer.text
        );
        let ids = bt(&[
            "view", map, "--ids", "--region", "0,0,3x2", "--layers", "deco",
        ])
        .unwrap();
        assert!(
            ids.text.ends_with(
                "deco (tiles of things):\n   \
                 0 1 2\n \
                 0 . 2 .\n \
                 1 . 2 .\n"
            ),
            "{}",
            ids.text
        );
        let corners = bt(&["view", map, "--corners", "--region", "2,1..7,4"]).unwrap();
        assert!(
            corners
                .text
                .starts_with("pond.tmx: the corners of cells 2,1 6x4"),
            "{}",
            corners.text
        );
        assert!(corners.text.contains(" 3  \"!~~~!\"\n"), "{}", corners.text);
    }

    #[test]
    fn json_names_the_views_shape_first_and_mistakes_say_the_way_out() {
        let scratch = Scratch::new("bt-view-mistakes").unwrap();
        let map = pond(scratch.path());
        let map = arg(&map);
        let json = bt(&["view", map, "--json"]).unwrap();
        assert!(
            json.text.starts_with("{\n  \"schema\": \"bt.view/1\","),
            "{}",
            json.text
        );
        let unknown = bt(&["view", map, "--layers", "ground,dec"]).unwrap_err();
        assert_eq!(
            format!("{unknown:#}"),
            "unknown layer “dec”: did you mean deco? (layers: deco, design, ground)"
        );
        let painted = bt(&["view", map, "--painted"]).unwrap_err();
        assert_eq!(
            format!("{painted:#}"),
            "pond.tmx has no AutoMap layers: what was painted is what it shows"
        );
        let region = bt(&["view", map, "--region", "1,2,3,4"]).unwrap_err();
        assert!(
            format!("{region:#}").contains("is not a region"),
            "{region:#}"
        );
        let step = bt(&["view", map, "--step", "0"]).unwrap_err();
        assert_eq!(format!("{step:#}"), "--step is a whole number above 0");
    }

    #[test]
    fn tileds_examples_read_as_they_look() {
        let Some(examples) = examples() else {
            return;
        };
        let view = |file: &str| bt(&["view", arg(&examples.join(file))]).unwrap().text;
        let desert = view("desert.tmx");
        assert!(
            desert.contains("  :  desert (terrain on Ground): 1240 cells\n"),
            "{desert}"
        );
        let hex = view("hexagonal-mini.tmx");
        assert!(
            hex.contains("; odd rows sit half a cell to the right, as on the map\n"),
            "{hex}"
        );
        let iso = view("isometric_grass_and_water.tmx");
        assert!(
            iso.contains("  ~  water (terrain on Tile Layer 1)"),
            "{iso}"
        );
        let knight = view("sticker-knight/map/sandbox.tmx");
        assert!(
            knight.contains("  S  sprites on shading ×17: shadow ×17\n"),
            "{knight}"
        );
        let corners = bt(&[
            "view",
            arg(&examples.join("hexagonal-mini.tmx")),
            "--corners",
        ])
        .unwrap_err();
        assert!(
            format!("{corners:#}").contains("this map is hexagonal"),
            "{corners:#}"
        );
    }

    #[test]
    fn painted_shows_what_was_painted_under_automap_layers() {
        let scratch = Scratch::new("bt-view-painted").unwrap();
        let Some(sewers) = sewers_with_automap_layers(&scratch) else {
            return;
        };
        let shown = bt(&["view", arg(&sewers), "--by", "layer"]).unwrap().text;
        let painted = bt(&["view", arg(&sewers), "--by", "layer", "--painted"])
            .unwrap()
            .text;
        assert!(shown.contains("layer Ground"), "{shown}");
        assert!(!painted.contains("layer Ground"), "{painted}");
        assert!(painted.contains("layer set"), "{painted}");
    }
}
