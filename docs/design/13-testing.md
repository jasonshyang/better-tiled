# Testing

*2026-10-09. The rules every crate's tests follow and the infrastructure they share: the
corpora and how they are found, the fixtures, the oracle and golden-file policies, randomised
tests, the editor harness, external programs, performance checks, the gates no test can
automate, and continuous integration. Each design doc's own tests section lists what its crate
pins; this doc says how those tests are built and run. The high-level design
(`00-high-level-design.md`, §14) states the levels and the principle: Tiled has almost no tests,
so better-tiled's are its correctness argument.*

## 1. Principles

- **Nothing skips quietly.** A test whose data, GPU or Godot is missing fails, naming what is
  missing. `BT_SKIP_MISSING_TEST_DATA=1` turns that into a skip that prints what it skipped, and
  CI checks the skips it allows (§11).
- **Values, not prose.** Reports, warnings and problems are typed; tests assert the values. A few
  snapshot tests pin wording so a sentence does not drift, and they are the only place a test
  compares prose.
- **Deterministic.** Every random choice in the code takes a seed; every random test takes a
  seed from the environment or a fixed default and prints it on failure; no test reads a clock
  except the timing checks, which assert only in optimised builds; no test uses the network.
- **Isolated.** Each test writes in a folder of its own under Cargo's temporary directory, named
  after the test and the process, and removes it; library and application code take their
  folders as values, so no test touches the user's data folder, runtime folder or settings.
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
  `scripts/fetch-reference.sh`: `examples/`, `tests/automapping/`, `tests/wangtiles/`,
  `src/libtiled/`, `src/tiled/` and `docs/manual/`. The script clones with blobs filtered,
  checks out the pinned commit and leaves a `reference/PINNED` file naming it; updating Tiled's
  version is one line. Nothing from it is ever committed here: copies drift and carry Tiled's
  licences along.
- **`assets/`** holds the purchased packs. They never leave the development machine, so every
  test that reads them skips on CI and says so.

`bt-test-data` finds both: `test_data(relative) -> Option<PathBuf>` returns the path in this
checkout or, from a git worktree, in the main checkout (so worktrees share one copy), and
fails the test when it is missing unless the skip variable is set, in which case it prints
"skipping: … is missing" and returns `None` for the test to return early. `missing(what)` does
the same for anything else a test needs (a GPU adapter, Godot). The crate also holds the
shared fixtures (§4) and depends on `bt-core` and `bt-image` for them; `bt-core` and `bt-image`
build their own inputs inline.

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
- **Shared builders** live in `bt-test-data::fixtures`: `Scratch` (a folder per test),
  `grid` (ASCII grids to tile layers and rule maps), `atlas` (drawn tilesets with Wang sets,
  classes and sprites), `map` (a map with those tilesets and named layers). Each crate's
  `tests/common` adds what only it needs.

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

The readers (`bt-tmx`'s XML, JSON, CSV, base64 and decompression; `bt-godot`'s scene reader;
`bt-live`'s wire) also have libFuzzer targets under `fuzz/`, with a small committed corpus
each, run by hand and by a weekly workflow; the normal suite runs a seeded mutation pass over
the same readers in a few seconds.

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

### 10.2 Windowed checks

Some things only a window shows: the feel of pan and zoom at 120 Hz on the largest pack map,
the latency of a stroke, hints and flashes, Godot's own drawing of exports (tall tiles, hex
fallbacks, parallax from a camera) beside ours. These are `#[ignore]`d tests with the reason in
their attribute, run by hand (`cargo test -- --ignored`), each leaving its pictures side by
side for comparison. `scripts/ui` drives the real window on macOS (screenshots at one pixel per
point, real clicks, drags and keys through the accessibility API), so an agent can run a
windowed check and read the result.

## 11. Continuous integration

GitHub Actions, on every push and pull request, one job per platform (macOS, Linux, Windows)
with the toolchain pinned by `rust-toolchain.toml` and the build cached:

1. `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings`;
2. `scripts/fetch-reference.sh`, which fails the job when the pinned commit cannot be fetched;
3. Godot installed on Linux (the official headless binary at a pinned version), so the headless
   suites run there; macOS and Windows allow the Godot skip;
4. `cargo test --workspace` in the test profile with `BT_SKIP_MISSING_TEST_DATA=1` and the
   renderer on the software adapter where the platform has one (lavapipe on Linux, WARP on
   Windows) and the runner's Metal device on macOS;
5. the test output's "skipping:" lines are collected, and the job fails if any names something
   other than `assets/` (or Godot, off Linux): a skip of `reference/` means the fetch or a path
   is broken, not that the data is private;
6. on failure, the golden difference folder and the test log are uploaded as artifacts.

A weekly workflow runs the fuzz targets for a fixed time and the `--ignored`-free long suites
in release mode. The purchased packs are never uploaded anywhere.

## 12. Running

| Variable | Effect |
|---|---|
| `BT_SKIP_MISSING_TEST_DATA=1` | missing data, GPU or Godot skips the test, loudly |
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
| CI | the skip filter fails on a `reference/` skip and passes an `assets/` skip (a unit test of the script) |
