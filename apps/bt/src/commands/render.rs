//! `bt render`: a picture of the map with its cells numbered along the edges, the cells of a
//! shape outlined in red, and the marks the editor has on the map numbered in their colours, so
//! a plan can be checked while the editor's window is hidden (`10-agents.md`, §10.3, §13).

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, anyhow, bail};
use bt_core::{Color, CustomTypes, Map};
use bt_live::{MarkSpec, cells_from_runs};
use bt_render::HeadlessDevice;
use bt_script::{Context, GridLines, PictureMark, RenderOutcome, RenderRequest, parse_region};
use bt_utils::paths::display_name;
use glam::IVec2;

use crate::cli::RenderArgs;
use crate::commands::know::{DEFAULT_SEED, shape_cells};
use crate::commands::layers::named_layers;
use crate::commands::look::shown_or_painted;
use crate::exit::{Printed, Status};
use crate::load::load_map;
use crate::output::printed;
use crate::picture_file::write_picture;
use crate::run::Run;
use crate::script_input::shape_names;

/// The colours the editor gives a map's marks in turn, as its canvas draws them: the first mark
/// in the first, the sixth in the first again. The live link names a mark's cells and label but
/// not its colour, so a mark the editor coloured otherwise (one marked again after others were
/// taken away) is drawn in the colour of its place.
const MARK_COLORS: [Color; 5] = [
    Color::rgba(255, 64, 160, 255),
    Color::rgba(255, 150, 0, 255),
    Color::rgba(40, 200, 90, 255),
    Color::rgba(230, 200, 0, 255),
    Color::rgba(170, 80, 255, 255),
];

/// Runs `bt render`.
pub(crate) fn render(run: &Run, args: &RenderArgs) -> anyhow::Result<Printed> {
    if args.with.is_some() && args.highlight.is_none() {
        bail!("--with FILE names what --highlight SHAPE uses, and there is no --highlight");
    }
    let mut loaded = load_map(&args.map, run)?;
    let names = shape_names(&mut loaded, args.with.as_deref(), None)?;
    let map = shown_or_painted(&loaded, args.painted)?;
    let mut request = RenderRequest::new(map, args.out.display().to_string())
        .custom_types(Arc::clone(loaded.loader.custom_types()))
        .grid(grid_lines(args));
    if !args.region.is_empty() {
        let region = parse_region(&args.region.join(" ")).map_err(anyhow::Error::msg)?;
        request = request.region(region);
    }
    if let Some(scale) = args.scale {
        request = request.scale(scale);
    }
    if !args.layers.is_empty() {
        request = request.layers(named_layers(map, &names, &args.layers)?);
    }
    if let Some(written) = &args.highlight {
        let context = match run.link().open_map(&loaded.path) {
            Some(open) => Context::new(DEFAULT_SEED).with_selection(open.selection()),
            None => Context::new(DEFAULT_SEED),
        };
        let shape = shape_cells(map, &names, &context, written)?;
        if shape.cells.is_empty() {
            bail!(
                "“{written}” holds no cells of {}: nothing to outline",
                display_name(&args.map)
            );
        }
        request = request.highlight(shape.cells);
    }
    if args.marks {
        request = request.marks(editor_marks(run, &loaded.path)?);
    }
    let rendered = RenderOutcome::from_request(&request, device()?)?;
    write_picture(&args.out, &rendered.picture)?;
    printed(&rendered.outcome, run.cli.json, Status::Done)
}

/// The GPU pictures are drawn on, or why there is none.
pub(crate) fn device() -> anyhow::Result<&'static HeadlessDevice> {
    HeadlessDevice::acquire().map_err(|error| {
        anyhow!(
            "the picture can't be drawn: bt render draws on the GPU, and {error} (bt view MAP \
             shows the map as text without one)"
        )
    })
}

/// A picture of `map` with `changed` outlined in red, written to `out`: the cells of `region`
/// (as written on the command line; the whole map when empty) at `scale`, for `bt edit
/// --render`.
pub(crate) fn draw_change(
    map: &Map,
    custom_types: Arc<CustomTypes>,
    out: &Path,
    region: &[String],
    scale: Option<f64>,
    changed: &[IVec2],
) -> anyhow::Result<RenderOutcome> {
    let mut request = RenderRequest::new(map, out.display().to_string())
        .custom_types(custom_types)
        .highlight(changed.to_vec());
    if !region.is_empty() {
        request = request.region(parse_region(&region.join(" ")).map_err(anyhow::Error::msg)?);
    }
    if let Some(scale) = scale {
        request = request.scale(scale);
    }
    let rendered = RenderOutcome::from_request(&request, device()?)?;
    write_picture(out, &rendered.picture)?;
    Ok(rendered.outcome)
}

/// The grid lines `--grid` and `--clean` ask for: a line every five cells unless they say
/// otherwise.
fn grid_lines(args: &RenderArgs) -> GridLines {
    if args.clean {
        GridLines::Clean
    } else if args.grid {
        GridLines::Cells
    } else {
        GridLines::Fives
    }
}

/// The marks the editor has on the map at `path`; an error when no editor has the map open, or
/// its editor has no marks on it.
fn editor_marks(run: &Run, path: &Path) -> anyhow::Result<Vec<PictureMark>> {
    let open = run.link().open_map(path).with_context(|| {
        format!(
            "{} is not open in the editor: --marks draws the marks bt mark made there (bt open \
             MAP opens it)",
            display_name(path)
        )
    })?;
    if open.document.marks.is_empty() {
        bail!(
            "the editor has no marks on {}: bt mark MAP SHAPE makes one",
            display_name(path)
        );
    }
    Ok(marks_drawn(&open.document.marks))
}

/// The editor's marks as the picture draws them: in their order, in the colours the editor gives
/// them in turn.
fn marks_drawn(marks: &[MarkSpec]) -> Vec<PictureMark> {
    marks
        .iter()
        .zip(MARK_COLORS.iter().cycle())
        .map(|(mark, &color)| PictureMark {
            cells: cells_from_runs(&mark.runs),
            color,
            label: mark.label.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use bt_image::decode_file;
    use bt_live::Run as CellRun;
    use bt_test_data::Scratch;
    use glam::IVec2;

    use super::*;
    use crate::commands::testing::{arg, bt, examples, lock, pond, sewers_with_automap_layers};

    /// Whether there is a GPU to draw on; without one the test fails, or says it skips when
    /// missing data is skipped.
    fn gpu() -> bool {
        match HeadlessDevice::acquire() {
            Ok(_) => true,
            Err(error) => {
                bt_test_data::missing("a GPU adapter", &error.to_string());
                false
            }
        }
    }

    #[test]
    fn a_picture_replaces_the_one_there_and_never_a_read_only_one() {
        if !gpu() {
            return;
        }
        let scratch = Scratch::new("bt-render").unwrap();
        let map = pond(scratch.path());
        let map = arg(&map);
        let out = scratch.join("pond.png");
        let first = bt(&[
            "render",
            map,
            arg(&out),
            "--region",
            "2,1",
            "6x4",
            "--scale",
            "2",
            "--highlight",
            "where water",
        ])
        .unwrap();
        let picture = decode_file(&out, None).unwrap();
        assert_eq!(
            first.text,
            format!(
                "wrote {} ({}x{} px): cells 2,1 6x4, 1 cell = 32 px, numbered along the top and left\n\
                 8 cells outlined in red\n",
                out.display(),
                picture.width,
                picture.height
            )
        );
        let clean = bt(&["render", map, arg(&out), "--clean", "--layers", "ground"]).unwrap();
        let replaced = decode_file(&out, None).unwrap();
        assert_eq!(
            (replaced.width, replaced.height),
            (160, 96),
            "{}",
            clean.text
        );
        let json = bt(&["render", map, arg(&out), "--grid", "--json"]).unwrap();
        assert!(
            json.text.starts_with("{\n  \"schema\": \"bt.render/1\","),
            "{}",
            json.text
        );
        let before = std::fs::read(&out).unwrap();
        lock(&out);
        let refused = bt(&["render", map, arg(&out)]).unwrap_err();
        assert!(
            format!("{refused:#}").ends_with("pond.png: the file is read-only"),
            "{refused:#}"
        );
        assert_eq!(std::fs::read(&out).unwrap(), before, "left as it was");
    }

    #[test]
    fn names_a_with_file_defines_reach_the_highlight() {
        if !gpu() {
            return;
        }
        let scratch = Scratch::new("bt-render-with").unwrap();
        let map = pond(scratch.path());
        let with = scratch.join("names.bt");
        std::fs::write(&with, "define yard = area rect 1,1 2x2\n").unwrap();
        let out = scratch.join("yard.png");
        let rendered = bt(&[
            "render",
            arg(&map),
            arg(&out),
            "--highlight",
            "where yard",
            "--with",
            arg(&with),
        ])
        .unwrap();
        assert!(
            rendered.text.ends_with("\n4 cells outlined in red\n"),
            "{}",
            rendered.text
        );
    }

    #[test]
    fn what_cant_be_drawn_says_so_and_nothing_is_written() {
        let scratch = Scratch::new("bt-render-mistakes").unwrap();
        let map = pond(scratch.path());
        let map = arg(&map);
        let out = scratch.join("never.png");
        let error = |more: &[&str]| {
            let mut all = vec!["render", map, arg(&out)];
            all.extend_from_slice(more);
            format!("{:#}", bt(&all).unwrap_err())
        };
        assert_eq!(
            error(&["--painted"]),
            "pond.tmx has no AutoMap layers: what was painted is what it shows"
        );
        assert_eq!(
            error(&["--marks"]),
            "pond.tmx is not open in the editor: --marks draws the marks bt mark made there (bt \
             open MAP opens it)"
        );
        assert_eq!(
            error(&["--with", "names.bt"]),
            "--with FILE names what --highlight SHAPE uses, and there is no --highlight"
        );
        assert!(
            error(&["--layers", "sky"]).starts_with("unknown layer “sky”"),
            "{}",
            error(&["--layers", "sky"])
        );
        assert_eq!(
            error(&["--highlight", "rect 20,20 2x2"]),
            "“rect 20,20 2x2” holds no cells of pond.tmx: nothing to outline"
        );
        assert!(
            error(&["--highlight", "lake"]).starts_with("“lake” is not a shape"),
            "{}",
            error(&["--highlight", "lake"])
        );
        assert!(!out.exists(), "nothing was written");
    }

    #[test]
    fn marks_are_drawn_in_the_colours_the_editor_gives_them_in_turn() {
        let marks: Vec<MarkSpec> = (0..6)
            .map(|n| MarkSpec {
                runs: vec![CellRun { y: n, x0: 0, x1: 2 }],
                label: format!("mark {n}"),
            })
            .collect();
        let drawn = marks_drawn(&marks);
        assert_eq!(drawn.len(), 6);
        assert_eq!(drawn[1].cells, [IVec2::new(0, 1), IVec2::new(1, 1)]);
        assert_eq!(drawn[1].label, "mark 1");
        assert_eq!(drawn[1].color, MARK_COLORS[1]);
        assert_eq!(
            drawn[5].color, MARK_COLORS[0],
            "the sixth takes the first again"
        );
    }

    #[test]
    fn tileds_grids_are_drawn_in_their_projection_and_painted_layers_too() {
        if !gpu() {
            return;
        }
        let Some(examples) = examples() else {
            return;
        };
        let scratch = Scratch::new("bt-render-examples").unwrap();
        let out = scratch.join("out.png");
        let iso = bt(&[
            "render",
            arg(&examples.join("isometric_grass_and_water.tmx")),
            arg(&out),
            "--region",
            "5,5,6x6",
            "--highlight",
            "6,6",
        ])
        .unwrap();
        assert!(
            iso.text
                .contains("every 5th diamond outlined and numbered x,y inside it"),
            "{}",
            iso.text
        );
        let hex = bt(&[
            "render",
            arg(&examples.join("hexagonal-mini.tmx")),
            arg(&out),
            "--json",
        ])
        .unwrap();
        assert!(
            hex.text.contains("\"projection\": \"hexagon\""),
            "{}",
            hex.text
        );
        let Some(sewers) = sewers_with_automap_layers(&scratch) else {
            return;
        };
        let painted = bt(&["render", arg(&sewers), arg(&out), "--painted", "--clean"]).unwrap();
        assert!(painted.text.contains("cells 0,0 31x31"), "{}", painted.text);
    }
}
