# Testing

*2026-10-09; what M0 to M3 built is as built at the end of M3, 2026-10-10, and M4's at the end
of M4, 2026-10-11. The rules every
crate's tests follow and the infrastructure they share: the corpora and how they are found, the
fixtures, the oracle and golden-file policies, randomised tests, the editor harness, external
programs, performance checks, the gates no test can automate, and continuous integration. Each
design doc's own tests section lists what its crate pins; this doc says how those tests are
built and run. The high-level design (`00-high-level-design.md`, §14) states the levels and the
principle: Tiled has almost no tests, so better-tiled's are its correctness argument.*

## 1. Principles

- **Nothing skips quietly.** A test whose data, GPU or Godot is missing fails, naming what is
  missing. `BT_SKIP_MISSING_TEST_DATA=1` turns that into a skip that prints what it skipped, and
  CI checks the skips it allows (§11).
- **Values, not prose.** Reports, warnings and problems are typed; tests assert the values. A
  few snapshot tests pin wording so a sentence does not drift, and they are the only place a
  test compares prose.
- **Deterministic.** Every random choice in the code takes a seed; every random test takes a
  seed from the environment or a fixed default and prints it on failure; no test reads a clock
  except the timing checks, which assert only in optimised builds; no test uses the network.
- **Isolated.** Each test writes in a folder of its own under the system's temporary folder
  (`better-tiled-tests/`), named after the test and the process, and removes it
  (`bt-test-data::Scratch`); library and application code take their folders as values, so no
  test touches the user's data folder, runtime folder or settings.
- **Fast by default.** `cargo test --workspace` finishes in under two minutes on the development
  machine in the test profile. Suites that take longer (headless Godot, the real-map exports,
  the pack's full-map renders) run in the same command but are named so they can be run alone.
- **Named as sentences.** A test's name states the behaviour it pins, in snake case: a failure
  reads as a broken promise, not a number.

## 2. Levels

| Level | Where | Detailed in |
|---|---|---|
| unit | inline `#[cfg(test)] mod tests` in every module | each doc |
| integration | `tests/` of each crate, with a `common` module | each doc |
| round trip | `bt-tmx` against the corpora | `02-file-formats.md`, §16 |
| oracle | Tiled's expected results and sample sets; Godot headless; the packs' previews | §5 |
| property and randomised | `bt-automap`, `bt-terrain`, `bt-edit`, `bt-editor`, `bt-script` | §6 |
| golden images and snapshots | `bt-render`, `bt-script`, `bt-godot`, the application | §5.3 |
| editor flows | `apps/better-tiled` through `egui_kittest` | §7, `06-application.md`, §15 |
| external programs | Godot headless | §8 |
| performance | release-only assertions and the benchmark mode | §9 |
| gates | agent runs, windowed checks | §10 |

## 3. Corpora

Two folders hold data the repository does not: `reference/` and `assets/`. Both are
git-ignored.

- **`reference/tiled/`** is a sparse checkout of Tiled's repository at the commit pinned in
  `scripts/fetch-reference.sh`: the folders `docs/`, `examples/`, `src/` and `tests/` (a
  sparse checkout takes whole folders). The script clones with blobs filtered, turns git's line
  ending conversion off so that files compare byte for byte on Windows too, checks out the
  pinned commit and leaves a `reference/PINNED` file naming it; updating Tiled's version is one
  line. Nothing from it is ever committed here: copies drift and carry Tiled's licences along.
  The repository's `.gitattributes` keeps the line endings of the committed fixtures, goldens
  and vectors for the same reason.
- **`assets/`** holds the purchased packs. They never leave the development machine, so every
  test that reads them skips on CI and says so.

`bt-test-data` finds both: `test_data(relative) -> Option<PathBuf>` returns the path in this
checkout or, from a git worktree, in the main checkout (so worktrees share one copy), and
fails the test when it is missing unless the skip variable is set, in which case it prints
"skipping: … is missing" and returns `None` for the test to return early. `missing(what, hint)`
does the same for anything else a test needs (a GPU adapter, Godot). Every skip is also
appended to the file `BT_SKIP_LOG` names, which CI checks (§11). The crate depends on nothing
of ours without the `fixtures` feature; the shared fixtures (§4) are behind it, so only the
dev-dependencies that ask for them build `bt-core` into the test crate.

## 4. Fixtures

- **Built in code, not committed, wherever possible.** Tilesets are drawn pixel by pixel into
  temporary PNGs (a marker tile, two terrains in every corner combination, an overlay with
  edges, a tree with a trunk for feet tests); maps and rule maps are written from ASCII grids
  (`.` empty, numbers for tiles, letters for the rule tiles, a suffix for flips); projects, rule
  files and `better-tiled.json` are written into the test's folder. A fixture that must be a
  file is written at run time, so its content stays beside the assertion that uses it.
- **Committed only when small and ours:** the dialect fixtures of `02-file-formats.md`, §16,
  one per legacy row; the golden images and snapshot texts; the fuzz corpora. Nothing of
  Tiled's or of a pack's is committed.
- **Shared builders**: `Scratch` (a folder per test) is in `bt-test-data`;
  `Scratch::mirror(from, name)` mirrors a folder into it, copying Tiled's files and hard-linking
  the rest, so a test can save a pack's map without ever writing the pack. With the `fixtures`
  feature (M1), `bt-test-data::fixtures` adds `grid` (ASCII grids to tile layers: `.` empty, a
  tile id with an optional `@slot` and the flips `h`, `v`, `d`, `x`), `atlas` (tilesets drawn
  pixel by pixel, each tile one colour with a marker in its corner, with offsets, render sizes,
  fill modes and animations) and `map` (`MapBuilder`: a map of any orientation with those
  tilesets and tile, object and image layers). Rule maps, Wang sets and classes join them with
  the milestones that need them. Each crate's `tests/common` adds what only it needs. As built
  at M3: grids name AutoMap's rule tiles by letter (`X` Negate, `I` Ignore, `N` NonEmpty, `E`
  Empty, `O` Other, in slot 1 unless an `@slot` says otherwise) and `rule_tiles()` draws their
  tileset; `TerrainSetBuilder` builds a terrain set of any kind with named terrains, labelled
  tile by tile or with every combination (`corners`, `edges`, `every_combination`,
  `combination_tile`); the picture fixtures draw tilesets lane by lane for the terrain helpers
  (`blank_picture`, `paint_lanes`, `paint_a2_autotile`, `AtlasBuilder::picture`).

## 5. Oracles and goldens

### 5.1 Oracles

An oracle is an independent answer the port must reproduce:

- **Tiled's automapping test cases**, each with its expected result, run as Tiled's own harness
  runs them (`08-automap.md`, §14), and the directories without a result checked for their
  documented behaviour; a test asserts every directory is covered, so a new case in Tiled's
  repository fails the suite until it is handled.
- **Tiled's Wang sample sets**: Tiled's picks are unseedable, so the property "every tile chosen
  is a lowest-penalty, positive-weight choice that passes the lookahead, every invalid cell has
  no such tile, and the same cells are resolved" is asserted directly (`07-terrain.md`, §14).
- **A CPU compositor** in `bt-render`'s tests implements Tiled's cell renderer rules cell by
  cell; the GPU path must match it pixel for pixel (`03-rendering.md`, §12).
- **Godot headless** loads every export and reports cell centres, node positions, custom data
  and terrain mismatches, compared within 0.01 px with `bt-core`'s own geometry
  (`12-godot-export.md`, §16).
- **The packs' previews**, made by the packs' own tools, against our renders of their maps:
  exact where the preview adds nothing the map lacks, within a tolerance elsewhere, with the
  difference explained in the test.

### 5.2 Which Tiled behaviours are asserted

Where Tiled has a bug the design fixes on purpose (`08-automap.md`, §12), the oracle is the
documented correct behaviour and the test says which Tiled behaviour it departs from, so a
future reader does not "fix" the test back to Tiled. Where Tiled's behaviour is a quirk the
design keeps (`07-terrain.md`, §4), the test pins the quirk and says why.

### 5.3 Goldens and snapshots

A golden is a committed file a test compares its output with: PNGs for renders (synthetic maps
compared exactly; real maps within a stated tolerance and pixel budget), `.snap.txt` files for
report wording, `.tscn` texts for the Godot writer's output on the features map. Rules:

- goldens are small, ours, and produced by the code under test; a change to one is reviewed as
  a change to behaviour;
- `BT_UPDATE_GOLDENS=1` rewrites them; a failing comparison writes the actual output and, for
  images, a difference picture (differences in red over the dimmed golden) into
  `BT_GOLDEN_DIFF_DIR` when set, and CI uploads that folder on failure;
- a test never updates a golden on its own, and never compares against a file it wrote in the
  same run.

## 6. Randomised tests

Randomised tests use a seeded generator from `bt-utils::rng` (never the standard library's
unseeded one). `BT_FUZZ_SEED` sets the seed and `BT_FUZZ_CASES` the count; the failure message
prints both and the generated case as text (an ASCII grid, a script, a transaction list), so a
failure reproduces with one command. The property tests: region runs equal full runs
(`bt-automap`), explanations agree with runs, every terrain choice is valid (`bt-terrain`),
flood fills equal a plain search (`bt-edit`), random transaction sequences undo to the original
and keep dirty states right (`bt-editor`), script round trips through the parser
(`bt-script`), and a project file with unknown keys survives any edit (`bt-project`).

The readers also have libFuzzer targets under `fuzz/`, a workspace of its own because it
needs a nightly toolchain, with a small committed corpus each, run by hand and by a weekly
workflow. `bt-tmx`'s are `xml_map`, `json_map`, `tileset`, `layer_data` (CSV, base64 and
decompression) and `project_world`; `bt-godot`'s scene reader and `bt-live`'s wire get theirs
with their crates. The normal suite runs a seeded mutation pass over the same readers in a few
seconds (`bt-tmx`'s `tests/mutation.rs`). The deflate test is the one generator not taken from
`bt-utils::rng`: its inputs must equal those of the script that asked zlib for its vectors.

## 7. The editor harness

The application is tested through `egui_kittest` with a `TestWindow` handle
(`06-application.md`, §15) that opens files, runs actions, presses keys, clicks and drags at a
cell or an object, types into a named field, answers a modal and reads back the editor's public
queries; tests never reach into the window's fields. Time is stepped, never slept: a test that
waits for a worker or for `bt` on another thread steps the harness until the condition holds,
with a bound that fails the test. The live-link tests start a server in the test's own folder
through `BT_LIVE_DIR` and run `bt`'s command table in-process on a thread. A picture test
compares `--screenshot` of a known map with a golden within a tolerance; the editor's feel is
checked in a window (§10.2).

As built at M1, `TestWindow` (`apps/better-tiled/src/test_window.rs`) runs the app without a
GPU in the harness, with a data folder of its own; it drops files on the window, presses keys,
clicks labelled widgets, clicks the close button and ends the run as eframe does, and it runs
frames until the workers are done and the layout has stopped moving (a new dialog measures
itself a frame before it is placed, so a click right after it opens would miss). The
`--screenshot` picture test waits for a window that can present; the development machine runs
it by hand for now.

As built at M2, the window's tests drive the tools with the pointer (`click_cell`,
`drag_cells`, `click_world`, `drag_world`, `click_on_screen`, `drag_on_screen`), answer file
dialogs, poll the watcher, and find tiles of the Tiles panel, the tileset editor and the
animation palette, and points of the collision view, on screen. A double-click needs the
earlier clicks to be over (`pause`), as egui counts close clicks anywhere as one burst. The
milestone's measure is a window test of its own, `a_level_is_built_from_scratch_in_the_window`:
a new map, a tileset from an image, painting and filling, an object layer with a rectangle, a
collision shape drawn in the tileset editor, Save As, and the file read back. `bt-edit`'s
randomised tests (brushes, carried tiles, layer and map commands, objects, nodes) use the
seeded runs of §6 through `bt-test-data`'s `Fuzz`.

As built at M3, the measure is `a_packs_rules_and_terrain_sets_give_tileds_results_with_every_failure_explained`
(`app/m3_tests.rs`), on the Highlands pack opened where it is and never saved, skipped without
the pack: the rules do nothing on the pack's sample scene and AutoMap's notice and the Explain
tool say why; bound to the layer the scene paints its platforms on, Live AutoMap rebuilds its
2763 platform cells as the pack draws them; a terrain set of the pack paints with every tile
meeting its neighbours. The exact results it rests on are the crates' own: Tiled's automapping
cases and the pack suites of `bt-automap`, the sample sets and pack sets of `bt-terrain`. The
randomised tests M3 added: region runs equal full runs and explanations agree with runs
(`bt-automap`), anchors never change a run, AutoMap layers updated around a change equal
deriving them again. `bt-test-data`'s `workspace_root` is a path without `..`, so a path built
from it compares equal to the cleaned paths the crates report.

As built at M4, the live-link tests (`app/live_tests.rs`) give the window a folder of its own
(`LiveFolder`: short, private, under `/tmp`) through `Startup::live_folder` rather than
`BT_LIVE_DIR`, and ask it with `bt-live`'s client on a thread (`TestWindow::ask_bt`), stepping
the window until the answer comes. `bt`'s command table is a binary of its own, so `apps/bt`
tests its routing to the editor against a stand-in server (`bt_live::Server` with the test's
handler), and the window's edit flow is tested against a scripted window as well
(`live/edit.rs`: conflicts rerun, three runs at most). The guide's example runs as a test on a
map built from the Highlands pack (`apps/bt/src/guide_tests.rs`), the automated part of §10.1.

## 8. External programs

Godot is found through `GODOT`, then the macOS application, then the path, and the version
found is printed; tests that need it skip through `bt-test-data::missing` when it is absent
and the skip variable is set. Godot runs headless only (`--headless`, an import then a load
with a describing script), with a time limit, in a scratch project the test builds. No test
runs Tiled.

## 9. Performance

| What | Budget | Where |
|---|---|---|
| a frame on the budget map at any zoom | 8.3 ms, measured by `better-tiled --bench` | `03-rendering.md`, §11 |
| a terrain dab preview on a pack's set | under 1 ms | `07-terrain.md`, §13 |
| a full and a region AutoMap run on the pack's sample scene | bounds asserted per run | `08-automap.md`, §13 |
| an export on save of the pack's sample scene, checks included | 100 ms | `12-godot-export.md`, §15 |
| the workspace test suite | 2 minutes | §1 |

`scripts/budget-map.py` writes the budget map (1000×1000 cells in ten layers from Tiled's desert
tileset) to `target/budget/budget.tmx` for `better-tiled --bench`.

Timing tests measure in every build and assert only when `debug_assertions` is off, so a debug
run reports and a release run (`cargo test --release`, or CI's test profile) enforces. The test
profile optimises dependencies and our crates one level, which keeps the suite fast without
hiding debug checks. The benchmark mode runs on the development machine, not in CI.

## 10. Gates

### 10.1 Agent runs

Before a release of `bt`, fresh agents are given one brief each and `bt guide`, nothing else,
on several kinds of map: Tiled's desert, sewers (rules beside the map), isometric, hexagonal,
perspective walls and sticker-knight's object-only sandbox, and one pack map in the open
editor. The briefs live in `apps/bt/tests/briefs/`. The gate passes when each result's
`bt check` is clean and a render shows what the brief asked for; the agent's own report is
never the evidence. Friction the runs show is turned into typed problems or guide text, never
into a generator.

As built at M4, the gate ran on 2026-10-11 with Opus agents, two at a time. Each got its brief,
`bt` at a fixed path with its data in a scratch home, the map's folder (a copy of Tiled's
examples; for the editor, the guide example's `c9.tmx` on a copy of the Highlands pack, open in
an editor with a live folder of its own) and the instruction to read `bt guide` first. Judged by
`bt check` and renders: the desert market, the isometric river, the hexagonal range with one
pass (checked with `bt reach`: every way across crosses the pass), the room of hand-placed walls,
sticker-knight's new section and, in the editor, the bigger lake with two crossings marked as
one undo step all pass. The sewers' room is drawn by the rules as asked; its `check` lists only
objects the map's own rules place with types and properties the examples' project lacks, four
of them as soon as the rules run on the untouched layout, and `check` now says they are the
rules'. The first desert run placed `merchant` points the project doesn't define, which only
`check` caught; once the edit report named every new problem, the rerun chose the project's
`NPC` and passed. What the runs changed: the edit report names every new problem `check` would
list; `check` names the objects the rules place; the save line and `bt info` say where the
project file keeping what was painted is; small maps are drawn larger; a narrow view's column
ruler has its tens; property values read one way in every report; `bt areas` and `bt mark` take
a shape's words unquoted; usage errors name the program `bt`; and the guide says how terrains
that don't border each other are spaced, how `avoid` and its margin count, how paths are as wide
as they are and wind on isometric maps, which layer `tile` uses, what `under` covers, that
sprites' types aren't checked, and that `reach` exits 0 either way. Left as they are: `explain`
says nothing of objects the rules place; `bt` cannot measure a crossing or a jump; a pack tile
whose picture doesn't match its terrain label (the Highlands' `terrain-highlands:1497`) is found
only in a picture; the guide (35 KB) is more than some agents' tools show at once, and they read
it from the saved output.

### 10.2 Windowed checks

Some things only a window shows: the feel of pan and zoom at 120 Hz on the largest pack map,
the latency of a stroke, hints and flashes, Godot's own drawing of exports (tall tiles, hex
fallbacks, parallax from a camera) beside ours. These are `#[ignore]`d tests with the reason in
their attribute, run by hand (`cargo test -- --ignored`), each leaving its pictures side by
side for comparison. `scripts/ui` drives the real window on macOS (screenshots at one pixel per
point, real clicks, drags and keys through the accessibility API), so an agent can run a
windowed check and read the result.

## 11. Continuous integration

GitHub Actions (`.github/workflows/ci.yml`), on every push and pull request, one job per
platform (macOS, Linux, Windows) with the toolchain pinned by `rust-toolchain.toml` and the
build cached:

1. `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`;
2. `scripts/fetch-reference.sh`, which fails the job when the pinned commit cannot be fetched;
3. Godot installed on Linux (the official headless binary at a pinned version), so the headless
   suites run there; macOS and Windows allow the Godot skip. This step comes with M5, the
   milestone of the Godot export;
4. `cargo test --workspace` in the test profile with `BT_SKIP_MISSING_TEST_DATA=1` and
   `BT_SKIP_LOG` set. From M1, the renderer runs on the software adapter where the platform has
   one (lavapipe on Linux, installed as `mesa-vulkan-drivers`; WARP on Windows; both chosen by
   `BT_SOFTWARE_GPU=1`) and on the runner's Metal device on macOS;
5. `check-skips` (a program of `bt-test-data`) reads the skip log and fails the job if a skip
   names something other than `assets/` (or Godot, off Linux): a skip of `reference/` means the
   fetch or a path is broken, not that the data is private;
6. on failure, the skip log is uploaded as an artifact, and from M1 the golden difference
   folder too (`BT_GOLDEN_DIFF_DIR` is `target/golden-diffs`).

A weekly workflow (`.github/workflows/fuzz.yml`) runs each fuzz target for ten minutes and
keeps what failed; the long suites in release mode join it when they exist. The purchased
packs are never uploaded anywhere.

## 12. Running

| Variable | Effect |
|---|---|
| `BT_SKIP_MISSING_TEST_DATA=1` | missing data, GPU or Godot skips the test, loudly |
| `BT_SKIP_LOG=file` | every skip is appended to this file, for `check-skips` |
| `BT_UPDATE_GOLDENS=1` | rewrites goldens and snapshots from the current output |
| `BT_GOLDEN_DIFF_DIR=dir` | failing comparisons write actual output and difference pictures there |
| `BT_FUZZ_SEED`, `BT_FUZZ_CASES` | the randomised tests' seed and count |
| `BT_SOFTWARE_GPU=1` | the headless renderer prefers the software adapter |
| `GODOT=path` | the Godot executable for the headless suites |
| `BT_LIVE_DIR=dir` | where live-link tests put their sockets |

`cargo test -p <crate>` runs one crate; `cargo test --release` turns the timing checks on;
`cargo test -- --ignored` runs the windowed checks. Several checkouts or worktrees built at
once each need their own target directory (`CARGO_TARGET_DIR`), never a shared one.

## 13. Tests of the test infrastructure

| Test | What it pins |
|---|---|
| `bt-test-data` | files in the checkout are found; a worktree finds the main checkout; missing data fails with the variable named, or skips with a message |
| fixtures | each builder round-trips through `bt-tmx` and reads back equal; grids parse every token and refuse unknown ones |
| goldens | the comparison reports the first differing pixel or line; the update variable rewrites exactly what differs |
| CI | the skip filter fails on a `reference/` skip and passes an `assets/` skip (a unit test of `disallowed_skips`, which `check-skips` runs) |

## 14. As built at M5 (2026-10-15)

The baseline on Rust 1.95.0 passed 1,478 tests before M5 changes. `bt-godot` adds format and
regeneration tests, seeded reader mutations, and headless Godot checks for grids, physics,
navigation, terrain brush behavior, animations, custom data, instances and inherited scenes.
Its real-map tests load all 14 examples and both Highlands maps in Godot, compare disk and
in-memory exports byte for byte, then assert repeat exports write nothing. CLI and window
tests cover the public export flows, saved-file semantics, remember/undo and automatic
refusal persistence. CI downloads pinned Godot 4.7 on Linux; only the other platforms may
skip Godot through the existing explicit skip log. The weekly fuzzer includes
`godot_resource` for the text scene/resource reader.


## 15. As built at M6 (2026-10-15)

The Rust 1.95.0 baseline passed 1,633 tests with the manual showcase recorder ignored.
The completed M6 workspace run passed 1,703 tests, with zero failures and the same one ignored
recorder. Workspace format and Clippy checks passed.
M6 adds core integration suites for shortcut schemes, image export, stamp persistence,
project commands, world assets, template assets, project workspaces, Issues and collision
templates. They cover undo and redo, owner-relative references, session/recovery, stale
picker/worker results, project switching, dependency-save ordering and failures. Command
process tests cover streamed UTF-8 output, output limits, cancellation and shutdown.

Window tests exercise the actual dialogs, menus, shortcut routing, panels and file requests.
GPU tests draw world neighbours and frames, saved stamp thumbnails, template ghosts in map
and collision canvases, project browsing and actionable Issues. The rendered screenshots were
inspected during implementation. Shared renderer tests cover map files used as images,
including recursion failures. The full workspace suite includes the existing corpus,
headless Godot, CLI and live-link regressions. Format and Clippy checks run across all targets.
