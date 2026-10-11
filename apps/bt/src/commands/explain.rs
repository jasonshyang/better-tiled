//! `bt explain`: what an AutoMap run does at a cell, and why (`10-agents.md`, §10.6). On a map
//! with AutoMap layers it explains their runs, each on what it reads (`--run N` for one);
//! otherwise a run of the rules named (from the map's folder, as the edit step `automap` names
//! them), else of the rules Tiled would find for the map, with `RULE-LAYER=MAP-LAYER` bindings
//! checked as the edit step checks them.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use bt_automap::{LayerNames, RuleSet};
use bt_core::{LayerId, LayerKind, Map};
use bt_script::{Binding, ExplainOutcome, ExplainRequest, Names, parse_cell};
use bt_utils::paths::{clean_path, display_name};

use crate::cli::ExplainArgs;
use crate::commands::layers::layer_ref;
use crate::commands::look::read_rules;
use crate::exit::{Printed, Status};
use crate::load::{LoadedMap, load_map};
use crate::output::printed;
use crate::run::Run;

/// What is written after the cell: the rules file, and how the rules' layer names bind to the
/// map's layers.
struct WrittenRules {
    /// The rules file named, if any.
    rules: Option<String>,
    /// The `RULE-LAYER=MAP-LAYER` pairs, in order.
    bindings: Vec<Binding>,
}

/// Rules read for the map, and what the explanation calls them.
struct ReadRules {
    /// The rules.
    set: RuleSet,
    /// The rules file as named, when it was; else the explanation names it by its file.
    name: Option<String>,
}

/// Runs `bt explain`.
pub(crate) fn explain(run: &Run, args: &ExplainArgs) -> anyhow::Result<Printed> {
    let cell = parse_cell(&args.cell).map_err(anyhow::Error::msg)?;
    let written = written_rules(&args.rules_and_bindings)?;
    let mut loaded = load_map(&args.map, run)?;
    if loaded.layers.is_some() && (written.rules.is_some() || !written.bindings.is_empty()) {
        bail!(
            "{} has AutoMap layers, whose rules run on the layers chosen for them: bt explain \
             MAP X,Y explains those runs (--run N for one), without rules or bindings",
            display_name(&args.map)
        );
    }
    let rules = rules_of(&mut loaded, written.rules.as_deref())?;
    let mut request = ExplainRequest::new(&loaded.map, &rules.set, cell).all(args.all);
    if let Some(name) = rules.name {
        request = request.rules_file(name);
    }
    match &loaded.layers {
        Some(layers) => request = request.automap_layers(&layers.painted, &layers.setup),
        None => {
            let names = Names::of(&loaded.map);
            let bindings = layer_names(&loaded.map, &names, &rules.set, &written.bindings)?;
            request = request.bindings(bindings);
        }
    }
    if let Some(seed) = args.seed {
        request = request.seed(seed);
    }
    if let Some(number) = args.run {
        request = request.run(number);
    }
    let outcome = ExplainOutcome::from_request(&request)?;
    printed(&outcome, run.cli.json, Status::Done)
}

/// The rules file and the bindings, as the edit step `automap` reads them: a word with `=` is a
/// binding, and one word without is the rules file.
fn written_rules(words: &[String]) -> anyhow::Result<WrittenRules> {
    let mut written = WrittenRules {
        rules: None,
        bindings: Vec::new(),
    };
    for word in words {
        match word.split_once('=') {
            Some((rule_layer, map_layer)) if !rule_layer.is_empty() && !map_layer.is_empty() => {
                written.bindings.push(Binding {
                    rule_layer: rule_layer.to_owned(),
                    map_layer: layer_ref(map_layer),
                });
            }
            None if written.rules.is_none() => written.rules = Some(word.clone()),
            _ => bail!(
                "unexpected “{word}”: after the cell come the rules file, then \
                 RULE-LAYER=MAP-LAYER pairs (e.g. walls=ground)"
            ),
        }
    }
    Ok(written)
}

/// The rules explained: those `named` (from the map's folder unless absolute), else the map's
/// AutoMap layers', else those Tiled would find for it; read through the map's loader, so the
/// rule maps share its tilesets.
fn rules_of(loaded: &mut LoadedMap, named: Option<&str>) -> anyhow::Result<ReadRules> {
    let Some(written) = named else {
        let found = read_rules(loaded).with_context(|| {
            format!(
                "{} has no rules.txt beside it, and no project names a rules file for it: name \
                 the rules, e.g. bt explain MAP X,Y ../rules/rules.txt",
                display_name(&loaded.path)
            )
        })?;
        return Ok(ReadRules {
            set: found.rules.map_err(anyhow::Error::msg)?,
            name: None,
        });
    };
    let path = rules_path(&loaded.path, written);
    if !path.exists() {
        bail!(
            "there are no rules at {} (the rules are named from the map's folder, as the edit \
             step automap names them)",
            path.display()
        );
    }
    let set = bt_automap::files::load(&mut loaded.loader, &path, Some(&loaded.path))?;
    Ok(ReadRules {
        set,
        name: Some(written.to_owned()),
    })
}

/// The rules file `written` names for the map at `map`: from the map's folder unless absolute.
fn rules_path(map: &Path, written: &str) -> PathBuf {
    let path = Path::new(written);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    let folder = map.parent().unwrap_or(Path::new("."));
    clean_path(&folder.join(path))
}

/// The bindings as the rules take them, each checked as the edit step checks it: the map has
/// the layer, the rules use the name, and the layer is of the kind the rules use the name for.
fn layer_names(
    map: &Map,
    names: &Names,
    rules: &RuleSet,
    bindings: &[Binding],
) -> anyhow::Result<LayerNames> {
    let rule_layers = rules.rule_layers();
    let mut built = LayerNames::builder();
    for binding in bindings {
        let layer = names.layer(map, &binding.map_layer)?;
        let objects = match map.layer(layer).map(|l| &l.kind) {
            Some(LayerKind::Tile(_)) => false,
            Some(LayerKind::Object(_)) => true,
            _ => bail!(
                "{}: the rules read and write tile and object layers",
                layer_name(map, layer)
            ),
        };
        let name = binding.rule_layer.as_str();
        let as_tiles = rule_layers
            .inputs
            .iter()
            .chain(&rule_layers.tile_outputs)
            .any(|n| n == name);
        let as_objects = rule_layers.object_outputs.iter().any(|n| n == name);
        if !as_tiles && !as_objects {
            let mut known: Vec<&str> = rule_layers
                .inputs
                .iter()
                .chain(&rule_layers.tile_outputs)
                .chain(&rule_layers.object_outputs)
                .map(String::as_str)
                .collect();
            known.sort_unstable();
            known.dedup();
            if known.is_empty() {
                bail!("the rules don't name a layer “{name}” (they name no layers)");
            }
            bail!(
                "the rules don't name a layer “{name}” (they name {})",
                known.join(", ")
            );
        }
        if objects && !as_objects || !objects && !as_tiles {
            let kind = |objects: bool| {
                if objects {
                    "an object layer"
                } else {
                    "a tile layer"
                }
            };
            bail!(
                "{} is {}, and the rules' “{name}” is {}",
                layer_name(map, layer),
                kind(objects),
                kind(as_objects)
            );
        }
        built = if objects {
            built.objects(name, layer)
        } else {
            built.tiles(name, layer)
        };
    }
    Ok(built.build())
}

/// A layer's name.
fn layer_name(map: &Map, layer: LayerId) -> String {
    map.layer(layer)
        .map_or_else(|| format!("#{}", layer.0), |l| l.name.clone())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use bt_core::{FileRef, Map, Property, TilesetRef, Value};
    use bt_test_data::Scratch;
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use bt_tmx::WriteRequest;
    use glam::UVec2;

    use crate::commands::testing::{arg, bt, examples, sewers_with_automap_layers};

    /// A level and its rules, in `folder`: the tileset `ground` in `ground.tsx`, the rule map
    /// `rules.tmx` listed by `rules.txt` (rule 1 turns tile 1 on `set` into 2, rule 2 turns 2
    /// into 3, rule 3 needs 4 with 5 to its right to write 6, matched in order), and
    /// `level.tmx`, whose layer is `Set`, not `set`, holding 1 at 0,0 and 4 at 1,0.
    fn level_with_rules(folder: &Path) -> PathBuf {
        let atlas = Atlas::builder().name("ground").build_in(folder).unwrap();
        let tsx = folder.join("ground.tsx");
        WriteRequest::for_tileset(&atlas.tileset, &tsx)
            .write()
            .unwrap();
        let external = |mut map: Map| {
            map.tilesets[0] = TilesetRef {
                file: Some(FileRef::new("ground.tsx", &tsx)),
                first_gid: Some(1),
                tileset: Arc::clone(&atlas.tileset),
            };
            map
        };
        let mut rule_map = external(
            MapBuilder::new(UVec2::splat(16), UVec2::new(8, 1))
                .tileset(Arc::clone(&atlas.tileset))
                .tile_layer("input_set", "1 . 2 . 4 5 . .")
                .tile_layer("output_set", "2 . 3 . 6 . . .")
                .build()
                .unwrap(),
        );
        rule_map
            .properties
            .insert("MatchInOrder".into(), Property::new(Value::Bool(true)));
        WriteRequest::for_map(&rule_map, &folder.join("rules.tmx"))
            .write()
            .unwrap();
        std::fs::write(folder.join("rules.txt"), "rules.tmx\n").unwrap();
        let level = external(
            MapBuilder::new(UVec2::splat(16), UVec2::new(3, 2))
                .tileset(Arc::clone(&atlas.tileset))
                .tile_layer("Set", "1 4 .\n. . .")
                .build()
                .unwrap(),
        );
        let path = folder.join("level.tmx");
        WriteRequest::for_map(&level, &path).write().unwrap();
        path
    }

    #[test]
    fn the_rules_beside_the_map_explain_a_cell_with_the_bindings_given() {
        let scratch = Scratch::new("bt-explain").unwrap();
        let level = level_with_rules(scratch.path());
        let level = arg(&level);
        let unbound = bt(&["explain", level, "0,0"]).unwrap().text;
        assert!(
            unbound.contains("the rules read the layer set, which the map doesn't have"),
            "{unbound}"
        );
        assert!(
            unbound.contains("the map has a layer Set, not set (names must match exactly)"),
            "{unbound}"
        );
        let bound = bt(&["explain", level, "0,0", "rules.txt", "set=Set"])
            .unwrap()
            .text;
        assert_eq!(
            bound,
            "cell 0,0: what a run of the rules does here on the map as it is now (rules.txt: 1 rule map, 3 rules; seed 1)\n\
             with set=Set:\n  \
             Set: ground:1 → ground:3\n  \
             changes, in order:\n    \
             1. Set ← ground:2 by rule 1 of rules.tmx (its cells at 0,0 1x1), matched at 0,0\n    \
             2. Set ← ground:3 by rule 2 of rules.tmx (its cells at 2,0 1x1), matched at 0,0\n  \
             1 other rule writes this cell from some position; nothing here is what they look for (--all lists them)\n"
        );
        let beside = bt(&["explain", level, "0,0", "set=Set", "--seed", "4"])
            .unwrap()
            .text;
        assert!(
            beside.starts_with("cell 0,0: what a run of the rules does here on the map as it is now (rules.txt: 1 rule map, 3 rules; seed 4)\n"),
            "{beside}"
        );
        let close = bt(&["explain", level, "1,0", "set=#1", "--all"])
            .unwrap()
            .text;
        assert!(
            close.contains("Set 2,0 must be ground:5, has empty"),
            "{close}"
        );
        let json = bt(&["explain", level, "0,0", "--json"]).unwrap().text;
        assert!(
            json.starts_with("{\n  \"schema\": \"bt.explain/1\","),
            "{json}"
        );
    }

    #[test]
    fn rules_and_bindings_that_dont_fit_say_what_does() {
        let scratch = Scratch::new("bt-explain-mistakes").unwrap();
        let level = level_with_rules(scratch.path());
        let level = arg(&level);
        let error = |args: &[&str]| {
            let mut all = vec!["explain", level];
            all.extend_from_slice(args);
            format!("{:#}", bt(&all).unwrap_err())
        };
        assert_eq!(
            error(&["0,0", "rules.txt", "walls=Set"]),
            "the rules don't name a layer “walls” (they name set)"
        );
        assert!(
            error(&["0,0", "set=Sett"]).starts_with("unknown layer “Sett”: did you mean set?"),
            "{}",
            error(&["0,0", "set=Sett"])
        );
        assert!(
            error(&["0,0", "rules.txt", "more.txt"]).starts_with("unexpected “more.txt”"),
            "{}",
            error(&["0,0", "rules.txt", "more.txt"])
        );
        assert!(
            error(&["0,0", "../elsewhere/rules.txt"]).starts_with("there are no rules at "),
            "{}",
            error(&["0,0", "../elsewhere/rules.txt"])
        );
        assert_eq!(
            error(&["0,0", "--run", "1"]),
            "--run 1 goes with maps that have AutoMap layers, and this one has none (bt info MAP says)"
        );
        assert_eq!(error(&["0"]), "“0” is not a cell: write X,Y");
        std::fs::remove_file(scratch.join("rules.txt")).unwrap();
        assert!(
            error(&["0,0"]).starts_with("level.tmx has no rules.txt beside it"),
            "{}",
            error(&["0,0"])
        );
        assert!(
            bt(&["explain", level, "0,0", "rules.tmx", "set=Set"]).is_ok(),
            "a rule map alone is rules too"
        );
    }

    #[test]
    fn tileds_sewers_explain_their_rules_and_their_automap_layers() {
        let Some(examples) = examples() else {
            return;
        };
        let sewers = examples.join("sewer_automap/sewers.tmx");
        let once = bt(&["explain", arg(&sewers), "10,10"]).unwrap().text;
        assert!(
            once.starts_with(
                "cell 10,10: what a run of the rules does here on the map as it is now (rules.txt: 9 rule maps"
            ),
            "{once}"
        );
        let scratch = Scratch::new("bt-explain-layers").unwrap();
        let Some(layered) = sewers_with_automap_layers(&scratch) else {
            return;
        };
        let layered = arg(&layered);
        let runs = bt(&["explain", layered, "10,10"]).unwrap().text;
        assert!(
            runs.starts_with(
                "cell 10,10: what the map's AutoMap layers do here (rules.txt: 9 rule maps"
            ),
            "{runs}"
        );
        assert!(runs.contains("\nrun 1 of 1"), "{runs}");
        assert!(bt(&["explain", layered, "10,10", "--run", "1"]).is_ok());
        let past = bt(&["explain", layered, "10,10", "--run", "2"]).unwrap_err();
        assert_eq!(
            format!("{past:#}"),
            "--run 2: the map's AutoMap layers have 1 runs, 1 to 1"
        );
        let named = bt(&["explain", layered, "10,10", "rules.txt"]).unwrap_err();
        assert!(
            format!("{named:#}").starts_with("sewers.tmx has AutoMap layers"),
            "{named:#}"
        );
    }
}
