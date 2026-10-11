//! `bt export`: a map or tileset in another of Tiled's formats, or a map as the resolved JSON
//! for other programs, and that JSON's schema (`10-agents.md`, §10.8).

use std::path::Path;

use anyhow::{Context, bail};
use bt_script::{ExportFormat, ExportOutcome, RESOLVED_SCHEMA, ResolvedRequest};
use bt_tmx::{ExportOptions, Format, Loader, WriteRequest};

use crate::cli::ExportArgs;
use crate::exit::{Printed, Status};
use crate::load::load_map;
use crate::output::printed;
use crate::run::Run;

/// Runs `bt export`.
pub(crate) fn export(run: &Run, args: &ExportArgs) -> anyhow::Result<Printed> {
    if args.schema {
        return Ok(Printed::done(RESOLVED_SCHEMA));
    }
    let (Some(input), Some(out)) = (&args.input, &args.out) else {
        bail!("bt export needs the file to export and the file to write");
    };
    if args.resolved {
        return export_resolved(run, input, out);
    }
    let extension = extension_of(out);
    if extension == "png" {
        bail!("pictures are not built yet");
    }
    let Some(format) = Format::from_path(out) else {
        bail!(
            "{} names no format bt writes: .tmx, .tmj, .json or .js for a map, .tsx, .tsj or \
             .json for a tileset, .png for a picture",
            out.display()
        );
    };
    let options = ExportOptions {
        embed_tilesets: args.embed_tilesets,
        detach_templates: args.detach_templates,
        resolve_types_and_properties: args.resolve_types,
        minimize: args.minimize,
    };
    let mut warnings = Vec::new();
    if is_tileset(input)? {
        if format == Format::JavaScript {
            bail!("a tileset is written as .tsx, .tsj or .json, not .js");
        }
        let mut loader = Loader::builder().build();
        let loaded = loader
            .load_tileset(input)
            .with_context(|| format!("{} can't be read", input.display()))?;
        warnings.extend(loaded.warnings.iter().map(ToString::to_string));
        let written = WriteRequest::for_tileset(&loaded.value, out)
            .format(format)
            .loader(&loader)
            .export(options)
            .write()
            .with_context(|| format!("{} was not written", out.display()))?;
        warnings.extend(written.iter().map(ToString::to_string));
    } else {
        let loaded = load_map(input, run)?;
        warnings.extend(loaded.warnings.iter().map(ToString::to_string));
        let written = WriteRequest::for_map(&loaded.map, out)
            .format(format)
            .loader(&loaded.loader)
            .export(options)
            .write()
            .with_context(|| format!("{} was not written", out.display()))?;
        warnings.extend(written.iter().map(ToString::to_string));
    }
    let outcome = ExportOutcome {
        input: input.clone(),
        out: out.clone(),
        format: match format {
            Format::Xml => ExportFormat::Xml,
            Format::Json => ExportFormat::Json,
            Format::JavaScript => ExportFormat::JavaScript,
        },
        options: [
            (args.embed_tilesets, "--embed-tilesets"),
            (args.detach_templates, "--detach-templates"),
            (args.resolve_types, "--resolve-types"),
            (args.minimize, "--minimize"),
        ]
        .iter()
        .filter(|option| option.0)
        .map(|option| option.1.to_owned())
        .collect(),
        warnings,
    };
    printed(&outcome, run.cli.json, Status::Done)
}

/// Writes the map at `input` as the resolved JSON to `out`, which must be a `.json` file.
fn export_resolved(run: &Run, input: &Path, out: &Path) -> anyhow::Result<Printed> {
    if extension_of(out) != "json" {
        bail!(
            "--resolved writes JSON: {} needs the .json extension",
            out.display()
        );
    }
    if is_tileset(input)? {
        bail!(
            "--resolved writes a map, and {} is a tileset",
            input.display()
        );
    }
    let loaded = load_map(input, run)?;
    let mut warnings: Vec<String> = loaded.warnings.iter().map(ToString::to_string).collect();
    let written = ResolvedRequest::new(&loaded.map, &loaded.path)
        .custom_types(loaded.loader.custom_types())
        .write(out)
        .with_context(|| format!("{} was not written", out.display()))?;
    warnings.extend(written.iter().map(ToString::to_string));
    let outcome = ExportOutcome {
        input: input.to_path_buf(),
        out: out.to_path_buf(),
        format: ExportFormat::Resolved,
        options: Vec::new(),
        warnings,
    };
    printed(&outcome, run.cli.json, Status::Done)
}

/// A file's extension in lowercase; empty for none.
fn extension_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Whether `path` is a tileset: `.tsx` and `.tsj` are, a `.json` file when Tiled's `type` says
/// so, anything else is a map.
fn is_tileset(path: &Path) -> anyhow::Result<bool> {
    match extension_of(path).as_str() {
        "tsx" | "tsj" => Ok(true),
        "json" => {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("{} can't be read", path.display()))?;
            let value: serde_json::Value = serde_json::from_str(&text)
                .with_context(|| format!("{} is not JSON", path.display()))?;
            Ok(value.get("type").and_then(serde_json::Value::as_str) == Some("tileset"))
        }
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bt_core::{FileRef, TilesetRef};
    use bt_test_data::Scratch;
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::UVec2;

    use super::*;

    /// A map with an external tileset, written in `folder`.
    fn write_map(folder: &Path) -> PathBuf {
        let atlas = Atlas::builder().name("tiles").build_in(folder).unwrap();
        let tileset_path = folder.join("tiles.tsx");
        WriteRequest::for_tileset(&atlas.tileset, &tileset_path)
            .write()
            .unwrap();
        let mut map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(3, 2))
            .tileset(atlas.tileset.clone())
            .tile_layer("ground", "0 1 2\n3 4 5")
            .build()
            .unwrap();
        map.tilesets[0] = TilesetRef {
            file: Some(FileRef::new("tiles.tsx", &tileset_path)),
            first_gid: Some(1),
            tileset: atlas.tileset.clone(),
        };
        let path = folder.join("a.tmx");
        WriteRequest::for_map(&map, &path).write().unwrap();
        path
    }

    fn run(args: &[&str]) -> anyhow::Result<Printed> {
        let run = Run::of_args(args, crate::folders::Folders::default());
        match &run.cli.command {
            crate::cli::Command::Export(export_args) => export(&run, export_args),
            _ => panic!("an export"),
        }
    }

    #[test]
    fn a_map_is_written_in_the_format_its_extension_names() {
        let scratch = Scratch::new("bt-export-json").unwrap();
        let map = write_map(scratch.path());
        let out = scratch.join("a.tmj");
        let printed = run(&[
            "export",
            map.to_str().unwrap(),
            out.to_str().unwrap(),
            "--embed-tilesets",
        ])
        .unwrap();
        assert!(
            printed.text.contains("as Tiled's JSON (--embed-tilesets)"),
            "{}",
            printed.text
        );
        let mut loader = Loader::builder().build();
        let read = loader.load_map(&out).unwrap().value;
        assert!(read.tilesets[0].file.is_none(), "the tileset is embedded");
        let original = loader.load_map(&map).unwrap().value;
        assert_eq!(read.layers.len(), original.layers.len());

        let tileset = scratch.join("tiles.tsj");
        let tsx = scratch.join("tiles.tsx");
        run(&["export", tsx.to_str().unwrap(), tileset.to_str().unwrap()]).unwrap();
        assert!(loader.load_tileset(&tileset).is_ok());
    }

    #[test]
    fn unknown_formats_and_read_only_targets_are_refused() {
        let scratch = Scratch::new("bt-export-refused").unwrap();
        let map = write_map(scratch.path());
        let error = run(&["export", map.to_str().unwrap(), "a.txt"])
            .err()
            .unwrap();
        assert!(error.to_string().contains("names no format"), "{error}");
        let out = scratch.join("locked.tmx");
        std::fs::write(&out, "x").unwrap();
        let mut permissions = std::fs::metadata(&out).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&out, permissions).unwrap();
        let error = run(&["export", map.to_str().unwrap(), out.to_str().unwrap()])
            .err()
            .unwrap();
        assert!(format!("{error:#}").contains("read-only"), "{error:#}");
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "x");
    }

    #[test]
    fn the_schema_is_printed_as_it_ships() {
        let printed = run(&["export", "--schema"]).unwrap();
        assert_eq!(printed.status, Status::Done);
        assert_eq!(printed.text, RESOLVED_SCHEMA);
        let schema: serde_json::Value = serde_json::from_str(&printed.text).unwrap();
        assert_eq!(
            schema["properties"]["format"]["const"],
            bt_script::RESOLVED_FORMAT
        );
    }

    #[test]
    fn a_map_is_written_as_the_resolved_json_relative_to_its_folder() {
        let scratch = Scratch::new("bt-export-resolved").unwrap();
        let map = write_map(scratch.path());
        std::fs::create_dir_all(scratch.join("out")).unwrap();
        let out = scratch.join("out/a.json");
        let args = [
            "export",
            map.to_str().unwrap(),
            out.to_str().unwrap(),
            "--resolved",
        ];
        let printed = run(&args).unwrap();
        assert_eq!(
            printed.text,
            format!(
                "wrote {} as the resolved JSON (bt resolved map 1)\n",
                out.display()
            )
        );
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(written["format"], bt_script::RESOLVED_FORMAT);
        assert_eq!(written["map"]["source"], "../a.tmx");
        assert_eq!(written["tilesets"][0]["image"], "../tiles.png");
        assert_eq!(
            written["layers"][0]["rows"],
            serde_json::json!([[1, 2, 3], [4, 5, 6]])
        );
        assert_eq!(written["tiles"]["5"]["tile_id"], 4);
        let printed = run(&[&args[..], &["--json"]].concat()).unwrap();
        assert!(
            printed.text.contains("\"format\": \"resolved\""),
            "{}",
            printed.text
        );
    }

    #[test]
    fn the_resolved_json_is_a_json_file_of_a_map_and_never_written_over_a_read_only_file() {
        let scratch = Scratch::new("bt-export-resolved-refused").unwrap();
        let map = write_map(scratch.path());
        let resolved = |input: &Path, out: &Path| {
            run(&[
                "export",
                input.to_str().unwrap(),
                out.to_str().unwrap(),
                "--resolved",
            ])
        };
        let error = resolved(&map, &scratch.join("a.tmj")).err().unwrap();
        assert_eq!(
            error.to_string(),
            format!(
                "--resolved writes JSON: {} needs the .json extension",
                scratch.join("a.tmj").display()
            )
        );
        let tileset = scratch.join("tiles.tsx");
        let error = resolved(&tileset, &scratch.join("t.json")).err().unwrap();
        assert!(error.to_string().contains("is a tileset"), "{error}");
        let out = scratch.join("locked.json");
        std::fs::write(&out, "{}").unwrap();
        let mut permissions = std::fs::metadata(&out).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&out, permissions).unwrap();
        let error = resolved(&map, &out).err().unwrap();
        assert!(
            format!("{error:#}").ends_with("locked.json: the file is read-only"),
            "{error:#}"
        );
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "{}");
    }
}
