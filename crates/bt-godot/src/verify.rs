//! Checking a scene in Godot itself (`bt godot verify`, ⌘K ▸ Check in Godot; docs/design/12-godot-export.md).
//! Godot, headless, imports the project as its editor would, then loads the scene. Whatever it
//! prints as an error or a warning, and every cell whose tile the layer's TileSet doesn't have
//! (Godot draws nothing there), is a problem. It is never part of an export: exporting doesn't
//! need Godot.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

/// Where Godot's macOS app keeps its executable.
pub const MAC_APP: &str = "/Applications/Godot.app/Contents/MacOS/Godot";

/// Why nothing can be checked in Godot.
pub const NO_GODOT: &str = "checking a scene needs Godot 4, and there is none where better-tiled looks: set GODOT to its executable (it looks there, then in /Applications/Godot.app, then for godot on the PATH)";

/// How long Godot may take to import the project (the first import of a big one is slow), and
/// to load the scene.
const IMPORT_TIME: Duration = Duration::from_secs(600);
const LOAD_TIME: Duration = Duration::from_secs(120);

/// The most cells without tile data the script lists; it counts them all.
const LISTED: usize = 1000;

/// Godot's executable: `GODOT`, then the macOS app, then `godot` on the PATH.
pub fn executable() -> Option<PathBuf> {
    find(
        std::env::var_os("GODOT"),
        Path::new(MAC_APP),
        std::env::var_os("PATH"),
    )
}

/// [`executable`], from what it looks at.
pub(super) fn find(godot: Option<OsString>, app: &Path, path: Option<OsString>) -> Option<PathBuf> {
    let name = if cfg!(windows) { "godot.exe" } else { "godot" };
    let on_path: Vec<PathBuf> = path
        .iter()
        .flat_map(std::env::split_paths)
        .map(|dir| dir.join(name))
        .collect();
    godot
        .map(PathBuf::from)
        .into_iter()
        .chain([app.to_path_buf()])
        .chain(on_path)
        .find(|p| p.is_file())
}

/// When Godot said something.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum Step {
    /// Importing the project (`--import`).
    Import,
    /// Loading the scene.
    Load,
}

/// What Godot printed as an error or a warning (on stderr), once however often it printed it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Message {
    /// Whether the message came from importing or loading.
    pub step: Step,
    /// Whether Godot classified the message as an error.
    pub error: bool,
    /// The message printed by Godot.
    pub text: String,
    /// Where Godot says it happened (`at: …`), when it says.
    pub at: Option<String>,
    /// How many times Godot printed the same message.
    pub times: usize,
}

/// A painted cell Godot finds no tile for.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Cell {
    /// The layer's path in the scene.
    pub layer: String,
    /// The tile-layer cell coordinate.
    pub cell: [i64; 2],
    /// The atlas source id the cell refers to.
    pub source: i64,
    /// The atlas coordinates the cell refers to.
    pub atlas: [i64; 2],
    /// With the flip flags Godot keeps there.
    pub alternative: i64,
    /// What the TileSet lacks: "tile_set" (the layer has none), "source", "tile" or
    /// "alternative".
    pub lacks: String,
}

/// What Godot made of a scene.
#[derive(Debug, serde::Serialize)]
pub struct Verdict {
    /// The scene path verified by Godot.
    pub scene: PathBuf,
    /// The scene path inside its Godot project.
    pub res: String,
    /// The Godot that looked, and its version ("4.7.2") when it said.
    pub godot: PathBuf,
    /// The Godot version reported by the engine.
    pub version: Option<String>,
    /// Godot errors and warnings collected while importing and loading.
    pub messages: Vec<Message>,
    /// Whether Godot loaded and instantiated it as a scene.
    pub loaded: bool,
    /// The number of tile, object, image and group layers.
    pub layers: usize,
    /// Cells.
    pub cells: usize,
    /// The cells without tile data, the first [`LISTED`] of them; `missing` counts them all.
    pub listed: Vec<Cell>,
    /// Missing.
    pub missing: usize,
}

impl Verdict {
    /// The number of error messages Godot emitted.
    pub fn errors(&self) -> usize {
        self.messages.iter().filter(|m| m.error).count() + usize::from(!self.loaded)
    }

    /// The number of warning messages Godot emitted.
    pub fn warnings(&self) -> usize {
        self.messages.iter().filter(|m| !m.error).count()
    }

    /// Whether Godot found nothing wrong.
    pub fn clean(&self) -> bool {
        self.errors() == 0 && self.warnings() == 0 && self.missing == 0
    }

    /// What Godot found, in words: a headline, then each message and the cells without tile
    /// data by layer.
    pub fn text(&self) -> String {
        let godot = self
            .version
            .as_ref()
            .map_or_else(|| "Godot".to_owned(), |v| format!("Godot {v}"));
        if self.clean() {
            let layers = if self.layers == 0 {
                "no tile layers".to_owned()
            } else {
                format!(
                    "{}, {}, each with its tile",
                    plural(self.layers, "tile layer", "tile layers"),
                    plural(self.cells, "cell", "cells")
                )
            };
            return format!("{}: {godot} loads it cleanly ({layers})\n", self.res);
        }
        let mut found = Vec::new();
        if self.errors() > 0 {
            found.push(plural(self.errors(), "error", "errors"));
        }
        if self.warnings() > 0 {
            found.push(plural(self.warnings(), "warning", "warnings"));
        }
        if self.missing > 0 {
            found.push(plural(
                self.missing,
                "cell without tile data",
                "cells without tile data",
            ));
        }
        let mut out = format!("{}: {godot} finds {}\n", self.res, and(&found));
        for m in &self.messages {
            let step = match m.step {
                Step::Import => "importing the project",
                Step::Load => "loading the scene",
            };
            let at =
                m.at.as_deref()
                    .filter(|a| a.contains("res://"))
                    .map(|a| format!(" (at {a})"))
                    .unwrap_or_default();
            let times = if m.times > 1 {
                format!(" ({} times)", m.times)
            } else {
                String::new()
            };
            out.push_str(&format!(
                "  {} {step}: {}{at}{times}\n",
                if m.error { "error" } else { "warning" },
                m.text
            ));
        }
        if !self.loaded {
            out.push_str(&format!("  error: {godot} couldn't load it as a scene\n"));
        }
        let mut layers: Vec<&str> = self.listed.iter().map(|c| c.layer.as_str()).collect();
        layers.dedup();
        for layer in layers {
            const SHOWN: usize = 5;
            let cells: Vec<&Cell> = self.listed.iter().filter(|c| c.layer == layer).collect();
            let mut items: Vec<String> = cells.iter().take(SHOWN).map(|c| lacking(c)).collect();
            if cells.len() > SHOWN {
                items.push(format!("and {} more", cells.len() - SHOWN));
            }
            out.push_str(&format!(
                "  {layer}: {}, so Godot draws nothing there: {}\n",
                plural(
                    cells.len(),
                    "cell without tile data",
                    "cells without tile data"
                ),
                items.join("; ")
            ));
        }
        if self.missing > self.listed.len() {
            out.push_str(&format!(
                "  and {} more cells without tile data\n",
                self.missing - self.listed.len()
            ));
        }
        out
    }

    /// Everything Godot found, for `--json`.
    pub fn json(&self) -> Value {
        let messages = |error: bool| -> Vec<Value> {
            let step = |s: Step| if s == Step::Import { "import" } else { "load" };
            self.messages.iter().filter(|m| m.error == error).map(|m| json!({ "step": step(m.step), "message": m.text, "at": m.at, "times": m.times })).collect()
        };
        let cells: Vec<Value> = self
            .listed
            .iter()
            .map(|c| json!({ "layer": c.layer, "cell": c.cell, "source": c.source, "atlas": c.atlas, "alternative": c.alternative, "lacks": c.lacks }))
            .collect();
        json!({
            "scene": self.scene, "res": self.res, "godot": self.godot, "version": self.version,
            "result": if self.clean() { "clean" } else { "problems" },
            "loaded": self.loaded, "layers": self.layers, "cells": self.cells,
            "errors": messages(true), "warnings": messages(false),
            "cells_without_tile_data": self.missing, "listed_cells": cells,
        })
    }
}

/// A cell without tile data, and what its TileSet lacks.
fn lacking(c: &Cell) -> String {
    let [x, y] = c.cell;
    let [ax, ay] = c.atlas;
    let (source, tile) = (c.source, format!("({ax}, {ay})"));
    match c.lacks.as_str() {
        "tile_set" => format!("({x}, {y}): the layer has no TileSet"),
        "source" => format!("({x}, {y}) uses source {source}, which the TileSet doesn't have"),
        "alternative" => format!(
            "({x}, {y}) uses alternative {} of tile {tile} in source {source}, which it doesn't have",
            c.alternative & !FLIPS
        ),
        _ => format!("({x}, {y}) uses tile {tile} of source {source}, which it doesn't have"),
    }
}

/// The flip flags Godot keeps in a cell's alternative.
const FLIPS: i64 = 4096 | 8192 | 16384;

/// Has Godot (`godot`, headless) import the project the scene at `scene` is in, as its editor
/// would (that writes its `.godot/` folder and `.import` files), then load the scene. Nothing of
/// better-tiled's goes into the project. Fails when Godot can't be run at all; what Godot
/// finds is in the verdict.
pub fn verify(godot: &Path, scene: &Path) -> Result<Verdict, String> {
    let scene = super::scene_target(scene)?;
    if !scene.is_file() {
        return Err(format!("there is no scene {}", scene.display()));
    }
    let project = super::project_root(&scene)?;
    let res = super::res_path(&project, &scene).ok_or_else(|| {
        format!(
            "{} isn't inside the Godot project {}",
            scene.display(),
            project.display()
        )
    })?;

    let import = run(godot, &project, &["--import".into()], IMPORT_TIME)?;
    let version = import.stdout.lines().find_map(version);
    let mut messages = Vec::new();
    said(&mut messages, &import.stderr, Step::Import);
    if !import.success {
        messages.push(Message {
            step: Step::Import,
            error: true,
            text: format!("Godot stopped {}", import.how),
            at: None,
            times: 1,
        });
    }

    // The script stays outside the project, which it would otherwise become part of.
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("bt-verify-{}-{}", std::process::id(), nonce));
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("couldn't write the script Godot runs: {e}"))?;
    let script = dir.join("verify.gd");
    let load = std::fs::write(&script, VERIFY.replace("LISTED", &LISTED.to_string()))
        .map_err(|e| format!("couldn't write the script Godot runs: {e}"))
        .and_then(|()| {
            run(
                godot,
                &project,
                &[
                    "--script".into(),
                    script.into_os_string(),
                    "--".into(),
                    res.clone().into(),
                ],
                LOAD_TIME,
            )
        });
    let _ = std::fs::remove_dir_all(&dir);
    let load = load?;
    said(&mut messages, &load.stderr, Step::Load);
    let result: Value = load
        .stdout
        .split("BT-JSON-BEGIN")
        .nth(1)
        .and_then(|s| s.split("BT-JSON-END").next())
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(Value::Null);
    if result.is_null() {
        messages.push(Message {
            step: Step::Load,
            error: true,
            text: format!(
                "Godot stopped before saying what it loaded{}",
                if load.success {
                    String::new()
                } else {
                    format!(" ({})", load.how)
                }
            ),
            at: None,
            times: 1,
        });
    }
    let count = |key: &str| result.get(key).and_then(Value::as_u64).unwrap_or(0) as usize;
    let int = |v: &Value| v.as_i64().unwrap_or(0);
    let pair = |v: &Value| [int(&v[0]), int(&v[1])];
    let listed = result
        .get("missing")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|c| Cell {
            layer: c["layer"].as_str().unwrap_or_default().to_owned(),
            cell: pair(&c["cell"]),
            source: int(&c["source"]),
            atlas: pair(&c["atlas"]),
            alternative: int(&c["alternative"]),
            lacks: c["lacks"].as_str().unwrap_or_default().to_owned(),
        })
        .collect();
    Ok(Verdict {
        scene,
        res,
        godot: godot.to_path_buf(),
        version,
        messages,
        loaded: result
            .get("loaded")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        layers: count("layers"),
        cells: count("cells"),
        listed,
        missing: count("missing_count"),
    })
}

/// "4.7.2" from Godot's first line, "Godot Engine v4.7.2.stable.official.ed1daf0bf - …".
pub(super) fn version(line: &str) -> Option<String> {
    let rest = line.strip_prefix("Godot Engine v")?;
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(rest.len());
    Some(rest[..end].trim_end_matches('.').to_owned()).filter(|v| !v.is_empty())
}

/// Adds what Godot printed on stderr at `step` to `messages`: each message once, with how often.
/// Lines Godot indents (`at: …`, backtraces) belong to the message above them; anything else on
/// stderr counts as an error.
pub(super) fn said(messages: &mut Vec<Message>, stderr: &str, step: Step) {
    let mut last: Option<usize> = None;
    for line in stderr.lines().map(plain) {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            if let Some(at) = line.trim_start().strip_prefix("at: ")
                && let Some(m) = last.and_then(|i| messages.get_mut(i))
                && m.at.is_none()
            {
                m.at = Some(at.to_owned());
            }
            continue;
        }
        let kinds = [
            ("SCRIPT ERROR:", true),
            ("USER ERROR:", true),
            ("SHADER ERROR:", true),
            ("ERROR:", true),
            ("USER WARNING:", false),
            ("SCRIPT WARNING:", false),
            ("WARNING:", false),
        ];
        let (error, text) = kinds
            .iter()
            .find_map(|(prefix, error)| line.strip_prefix(prefix).map(|rest| (*error, rest.trim())))
            .unwrap_or((true, line.trim()));
        match messages
            .iter()
            .position(|m| m.step == step && m.error == error && m.text == text)
        {
            Some(i) => {
                if let Some(message) = messages.get_mut(i) {
                    message.times += 1;
                }
                // Its `at:` line is the one already kept.
                last = None;
            }
            None => {
                messages.push(Message {
                    step,
                    error,
                    text: text.to_owned(),
                    at: None,
                    times: 1,
                });
                last = Some(messages.len() - 1);
            }
        }
    }
}

/// `line` without terminal colour codes.
fn plain(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // ESC [ … letter
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// How a run of Godot went.
struct Ran {
    stdout: String,
    stderr: String,
    success: bool,
    /// How it stopped, when it failed ("with exit code 1").
    how: String,
}

/// Runs Godot headless in `project` with `args`, stopping it after `limit`.
fn run(godot: &Path, project: &Path, args: &[OsString], limit: Duration) -> Result<Ran, String> {
    let mut child = Command::new(godot)
        .arg("--headless")
        .arg("--path")
        .arg(project)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("couldn't run Godot ({}): {e}", godot.display()))?;
    // Read as it comes, so a full pipe never stops Godot.
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            String::from_utf8_lossy(&bytes).into_owned()
        })
    };
    let stdout = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let stderr = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("couldn't wait for Godot: {e}"))?
        {
            break status;
        }
        if start.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "Godot didn't finish within {} s ({} {})",
                limit.as_secs(),
                godot.display(),
                args.iter()
                    .map(|a| a.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let how = match status.code() {
        Some(code) => format!("with exit code {code}"),
        None => "on a signal".to_owned(),
    };
    Ok(Ran {
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
        success: status.success(),
        how,
    })
}

fn plural(n: usize, one: &str, many: &str) -> String {
    let n_text = n.to_string();
    // 1,200 rather than 1200.
    let mut grouped = String::new();
    for (i, c) in n_text.chars().enumerate() {
        if i > 0 && (n_text.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{grouped} {}", if n == 1 { one } else { many })
}

/// "a, b and c".
fn and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Loads the scene the command line names (a res:// path) and says what Godot made of it:
/// whether it loads, its tile layers and cells, and the cells whose tile the layer's TileSet
/// doesn't have. It asks with has_tile, since get_cell_tile_data prints an error of its own for
/// each such cell. Then the scene is in the tree for a few physics frames, as in a game, so that
/// what Godot says only then (collision shapes it can't build, say) is heard too. Every step is
/// guarded: a script error would leave Godot running.
const VERIFY: &str = r#"extends SceneTree

func _initialize() -> void:
	var result := {"loaded": false, "layers": 0, "cells": 0, "missing": [], "missing_count": 0}
	var args := OS.get_cmdline_user_args()
	var packed = ResourceLoader.load(args[0]) if args.size() > 0 else null
	var scene: Node = null
	if packed is PackedScene:
		scene = (packed as PackedScene).instantiate()
		if scene:
			result["loaded"] = true
			var flips := TileSetAtlasSource.TRANSFORM_FLIP_H | TileSetAtlasSource.TRANSFORM_FLIP_V | TileSetAtlasSource.TRANSFORM_TRANSPOSE
			for node: Node in [scene] + scene.find_children("*", "TileMapLayer", true, false):
				if not (node is TileMapLayer):
					continue
				var layer := node as TileMapLayer
				result["layers"] += 1
				var tile_set := layer.tile_set
				for c: Vector2i in layer.get_used_cells():
					result["cells"] += 1
					var id := layer.get_cell_source_id(c)
					var atlas := layer.get_cell_atlas_coords(c)
					var alternative := layer.get_cell_alternative_tile(c)
					var lacks := ""
					if tile_set == null:
						lacks = "tile_set"
					elif not tile_set.has_source(id):
						lacks = "source"
					elif not tile_set.get_source(id).has_tile(atlas):
						lacks = "tile"
					elif not tile_set.get_source(id).has_alternative_tile(atlas, alternative & ~flips):
						lacks = "alternative"
					if lacks != "":
						result["missing_count"] += 1
						if result["missing"].size() < LISTED:
							result["missing"].append({"layer": String(scene.get_path_to(layer)), "cell": [c.x, c.y], "source": id, "atlas": [atlas.x, atlas.y], "alternative": alternative, "lacks": lacks})
	# Said first, in case the scene's own scripts end the run. The tree frees the scene on quit.
	print("BT-JSON-BEGIN", JSON.stringify(result), "BT-JSON-END")
	if scene:
		root.add_child(scene)
		for i in 2:
			await physics_frame
	quit()
"#;
