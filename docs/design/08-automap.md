# AutoMap — `bt-automap`

*2026-10-09. The design of the crate that applies Tiled's automapping rules: reading rule maps
and `rules.txt`, running the rules on a map or a region of it with results equal to Tiled's,
explaining what the rules did at a cell, analysing rule maps for problems, and AutoMap layers,
which keep what the rules make apart from what the user painted. The high-level design
(`00-high-level-design.md`, §10.2 and §10.3) states the goals; this doc says how. Tiled's own
sources are cited as `reference/tiled/src/...`.*

## 1. Scope

`bt-automap` ports Tiled's `AutoMapper` (`reference/tiled/src/tiled/automapper.cpp`,
`automappingmanager.cpp`, `automappingutils.cpp`, GPL-2.0-or-later, used under
GPL-3.0-or-later) with three additions Tiled lacks: exact region runs, explanations, and
non-destructive layers. Compiling, applying and explaining work on maps in memory. One module,
`files`, is the only one that touches disk: it finds and loads rule maps through `bt-tmx` and
reloads them when they change.

Not here: the AutoMap panel and the Explain tool's display (the application), the project-file
records of AutoMap layers (`bt-project` defines them; this crate converts to and from them), undo
(the editor core), and the texts the status bar shows (the editor builds them from the typed
reports here).

Dependencies: `bt-core`, `bt-tmx` (in `files` only), `bt-utils` (`rng`, `bands`, `hash`).

## 2. Terms

| Term | Meaning |
|---|---|
| rule map | a Tiled map whose layers describe rules: `input*_name`, `inputnot*_name`, `output*_name`, optional `regions*` and `rule_options` layers, `//` note layers |
| `rules.txt` | the list of rule maps that apply to a map, with `[name]` filters and nested lists (`02-file-formats.md`, §13.4) |
| rule | one 8-connected region of non-empty cells over all input and output layers of a rule map; rules are ordered by the top-left of their region |
| input set | the `input<index>_` and `inputnot<index>_` layers of one index; a rule matches where any one of its input sets holds |
| condition | one input set's requirement on one target layer at one offset: a list of cells it must hold ("any of") or must not ("none of") |
| output set | the `output<index>_` layers of one index; the unnamed index is always written, named ones are random alternatives weighted by `Probability` |
| special tiles | the built-in tileset's Negate, Ignore, NonEmpty, Empty and Other, or any tile with a `MatchType` property (also from its class) |
| match | a rule holding at a position; applying it copies its output there |
| full run, region run | the rules on the whole map; the rules recomputed only where an edit can change the result |
| AutoMap layers | the setup under which the rules run on chosen layers and the painted map stays as painted while the result is what is shown and saved |
| binding | a map layer standing for a name the rules use |

## 3. Module map

| Module | Holds |
|---|---|
| `rules` | `RuleSet` (the compiled rule maps and everything derived from them), `Report` |
| `files` | finding `rules.txt` for a map, loading and reloading through a `bt-tmx` loader, the files to watch, `MatchType` from classes |
| `rulemap` | compiling one rule map: layer roles, options, special tiles, rule extraction, input sets, output sets, the tileset table (§4, §5) |
| `engine` | binding layers and tilesets, dense grids, the anchor index, the two matching modes, border modes, applying, chained rule maps, write-back (§6) |
| `region` | the locality analysis and the windows of region runs (§7) |
| `explain` | the traced run and its types (§8) |
| `analysis` | what is wrong or odd in a rule map, and what a run could not do (§9) |
| `layers` | AutoMap layers: setup, bindings, the derived result, stable object ids, conversion to and from the project file's records (§10) |
| `rng` | the pure random function of seed, rule map, rule, position and purpose |
| `bx` | inclusive boxes with Minkowski sums, the geometry of locality |

## 4. Rule maps as read

The grammar is Tiled's, exactly, because every pack's rules were written for it
(`reference/tiled/docs/manual/automapping.md`):

- Layers are visited depth-first; group and image layers are skipped; a name starting with `//`
  is a note. `regions`, `regions_input` and `regions_output` (case-insensitive) are the legacy
  region layers, each at most once and a tile layer. `rule_options` is an object layer of
  unrotated rectangles whose properties are rule options for the rules entirely inside them.
  Any other name must contain `_`: the part after the first underscore is the target layer's
  name (case-sensitive), the part before is `output`, `inputnot` or `input` (checked in that
  order, case-insensitively) plus an index. A layer without an underscore disables the whole
  rule map, as in Tiled; the warning says so and suggests `//` for notes.
- Input layers carry `StrictEmpty`/`AutoEmpty` and `IgnoreHorizontalFlip`, `IgnoreVerticalFlip`,
  `IgnoreDiagonalFlip`, `IgnoreHexRotate120` (each clearing a bit of the flip mask); output
  layers carry `Probability` (the last one wins) and any other property, which is merged into
  the target layer when it is written.
- Map options (case-insensitive, Qt's conversions of string values): `DeleteTiles`,
  `MatchOutsideMap`, `OverflowBorder`, `WrapBorder`, `AutomappingRadius`, `MatchInOrder`,
  `NoOverlappingRules`, and the rule defaults `Probability`, `ModX`, `ModY`, `OffsetX`,
  `OffsetY`, `NoOverlappingOutput`, `Disabled`, `IgnoreLock`. A value that cannot be read is a
  warning naming the property, never a silent zero.
- **Special tiles** by `MatchType`, read from the tile's properties, which the loader has already
  resolved from the tile's class with the project's custom types (one source of truth: there is
  no table of ids). Their meaning per layer kind follows Tiled's table: on an input layer a
  regular tile is "any of", `Empty` requires an empty cell, `NonEmpty` forbids one, `Other`
  forbids every tile the same condition uses anywhere (and the empty cell unless `Empty` is used
  there), `Negate` swaps "any" and "none" at the position, `Ignore` is no condition but joins
  rule parts; `inputnot` inverts; on an output layer a regular tile is written with its flips,
  `Empty` erases, the others do nothing. A cell with no tile is a condition only with `AutoEmpty`.
- **Rules** are the 8-connected components of the union of input and output cells (special
  tiles count), ordered by the top-left of their bounding box; a component needs both an input
  and an output part; `rule_options` rectangles apply to components they contain entirely,
  later rectangles winning. Legacy region layers force `MatchInOrder` unless the map sets it.
- **Input sets** are compiled once when the rule map is loaded (Tiled recompiles them on every
  run): for each input set and each target, the positions with their "any of" and "none of"
  lists, deduplicated, "any" subsuming "none", contradictory positions making the set
  impossible, a set with no conditions and no `Ignore` never matching (Tiled 1.12). Cells are
  expressed in a tileset numbering shared by every rule map of the rule set and bound to the
  target map's tilesets only when applying.
- **Output sets**: the unnamed index is the fixed output, always applied; named indexes are
  random alternatives with their weights; an index with nothing inside a rule's region is left
  out of that rule.

## 5. Tileset identity

Rules match tiles by tileset identity, so the rule maps' tilesets must be unified with the
map's the way Tiled unifies them: the same `Arc`, the same external file, or, for a tileset
embedded in a rule map, one "similar" by Tiled's criteria (tile count, image file, tile size,
spacing, margin, offset, and the same images for collections). The `TilesetTable` holds each
tileset once with the rule maps that use it. A rule-map tileset that is a **copy** of one of the
map's (the same image through another file, or an alike collection) never matches and is
reported as such with both files named, because Tiled says nothing and the rules silently do
nothing. The built-in tileset `:/automap-tiles.tsx` is the one the loader was built with.

## 6. Running

### 6.1 Binding and grids

The engine binds each name the rules use to a map layer: by name as Tiled finds layers (the
bottom-most tile layer of that name), or through the name map a run gives it (§10); output
layers the map lacks are created on top, in rule-map order, when something is written to them.
The involved layers are copied into dense grids covering the area of interest (the map, or a
window around an edit; a sparse overflow map beyond a size limit), rules read and write the
grids, and the end of the run writes back only the cells that differ, so copy-on-write chunks
keep undo snapshots cheap.

### 6.2 Candidates and matching

For each rule the compiler picks an **anchor**: its most selective "any of" condition. The
engine indexes the positions of anchor tiles in the grids (with a per-tileset bitset to reject
most cells at once), so a rule is tried only where its anchor tile is, and only rules with no
usable anchor scan every lattice position. Matching is then Tiled's: a rule's input bounding
box must overlap the region by at least one tile, must fit inside a finite map unless
`MatchOutsideMap`, reads outside the map are empty, clamped (`OverflowBorder`) or wrapped
(`WrapBorder`), positions are on the `ModX`/`ModY` lattice at `OffsetX`/`OffsetY`, and a rule's
`Probability` is rolled per position before matching.

- **Simultaneous** (the default): every rule's candidates are collected, filtered by the roll
  and by matching against the state before the rule map ran, sorted by rule then row then
  column, and applied in that order; later writes win cell by cell.
- **`MatchInOrder`**: rules go one by one and each position is matched and applied in turn, so a
  rule sees earlier rules' writes and its own; the anchor positions are kept up to date as
  rules write.

### 6.3 Applying

A match picks its random output set (weighted, one entry consumes no randomness), checks
`NoOverlappingOutput` against the rule's own footprint so far in this pass, copies the fixed
and chosen outputs over the rule's whole output region (regular tiles and `Empty` written,
other cells untouched; wrapped or clipped at a finite map's edge), merges the output layers'
other properties into the target, and places output objects shifted by the difference of
`tile_to_pixel` at the match and at the rule, with new ids and object references rewired. Locks
are honoured as the layer tree gives them, group locks included, unless `IgnoreLock`; a locked
output is reported. `DeleteTiles` erases the input cells of the apply region from every output
tile layer before matching, honouring the same locks. Rule maps of one rule set run in order
over the same grids, so each sees the writes of the previous ones; on infinite maps the apply
region grows by what earlier rule maps wrote.

### 6.4 Randomness

`rng::unit(seed, rule_map_index, rule_id, position, purpose)` is a pure function (a mixing hash),
with `purpose` telling a skip roll from an output choice. The same seed gives the same map in a
full run and in region runs; the editor keeps the seed in the AutoMap setup and "re-roll"
changes it.

### 6.5 The report

`Report { changed: Option<IRect>, matches, rule_map_matches: Vec<usize>, problems: Vec<Problem>,
incremental: bool, created_layers: Vec<LayerId>, tilesets_added, objects_added, objects_removed:
Vec<ObjectId>, locked_outputs: Vec<LockedOutput> }`. Problems are typed (`Problem { kind, text
}`), so the editor can group them and `bt` can print them as problem lines.

## 7. Region runs

An edit changes the result only near it, so running the rules on the whole map after every
stroke is wasted work and, with `MatchInOrder`, a different result. A region run recomputes,
inside a window around the edit, exactly what a full run would produce there, and nothing
outside.

- The **locality analysis** gives each rule map a dependency box: the offsets `d` such that its
  result at a cell depends only on the state at `cell + d` before it ran. In simultaneous mode
  that is the union of the rules' read offsets. With `MatchInOrder` the analysis follows
  read-after-write chains between rules, counting only writes that can change a later
  condition's outcome (replacing A by B cannot flip "is C"); a rule whose matches can feed its
  own later matches makes the rule map **non-local**, and so does `NoOverlappingOutput` on a rule
  whose matches can overlap.
- **Windows** are computed backward through the chain: the last rule map's window is the edit
  grown by its box, and each earlier one is the next window grown by its own box, so every read
  the later stages make is inside what the earlier ones recomputed. Writes are limited to the
  exact boxes.
- A run **falls back** to a full run, with the reason in the report, when a rule map is
  non-local, when `WrapBorder` is on, or on an infinite map when a later rule map has rules that
  match on empty cells (where they are tried depends on everything earlier ones wrote).
  `supports_incremental()`, `margin()` and `affected_area(map, region)` let callers know in
  advance what a region run may change, for redrawing and snapshots.
- `apply(map, Some(rect), seed)` runs around one rectangle; `apply_to(map, rects, seed)` runs
  around the cells of a selection, not everything inside their bounds, which is what Tiled does
  with a selection.

The invariant is fuzzed: on random rule sets and maps, a region run equals a full run inside the
affected area and changes nothing outside it.

## 8. Explain

A full run can record what it does without changing the map; the engine is generic over an
observer, so an untraced run pays nothing.

- `explain_cell(map, cell, seed) -> Explanation`: for every layer the rules write, the cell
  before and after; the **writes** to the cell in order (a rule at a position, or
  `DeleteTiles`); every **attempt**: each rule whose output covers the cell at each position from
  which it does, in the order the run tries them, with its `Outcome` (applied with which
  alternative and which locked outputs it skipped; skipped by probability; overlapping; no
  match; outside the map; off the lattice; disabled; can never match) and, per input set, each
  **check**: the layer, whether the map has it, the cell read, "any of" or "none of" which
  tiles, what was found there, what was there before the run, and whether it holds. Checks see
  the state Tiled would check: before the rule map in simultaneous mode, at scan time with
  `MatchInOrder`. Also how often each rule applied in the whole run, and the **unnamed** tiles at
  the cell: tiles on read-only layers that no condition names, which rules see only as "a tile".
- `explain_rule(map, rule, seed, max_failures) -> RuleMatches`: every position where the rule
  matched (applied or not), the near misses whose closest input set fails at most that many
  checks, and applied matches later overwritten. `Attempt::holds_a_tile` is the "came close"
  signal the panel uses: a condition naming particular tiles holds, so the rule is about what is
  here; ranking by failures alone drowns in noise on a pack with two thousand rules.
- `rule_summaries()` (area, input and output areas, reads, writes, input sets, random outputs,
  options, why inactive) and `rule_picture(rule)` (the rule's tiles per layer, with roles) feed
  the panel's pictures. `with_layer_names(rename)` makes explanations speak the map's layer
  names when the run used bindings.

Whether an explanation is of "the run that just happened" or "a run now" is the editor's choice
(it keeps the map from before an explicit run for that); the crate explains the map it is given.

## 9. Analysis

What is wrong or odd, typed, per rule map (`RuleMapInfo { path, rules, error, inputs, outputs,
tile_outputs, object_outputs, runs, problems, notes }`):

| Kind | Examples |
|---|---|
| error (the rule map is not used) | a layer name without an underscore; no input or no output layers; a `regions` layer twice or not a tile layer |
| warning | an unknown property or layer; an option value that cannot be read; tiles outside the `regions` layers; a rule map listed in `rules.txt` that cannot be read (with the reason; its previous rules kept on a reload); an output object too large; a locked output skipped at run time |
| note | input tiles that touch no output tiles, or output tiles that touch no input, so they make no rule (orphans) |

Across the rule set: `tileset_copies(map)` (§5), `missing_layers(map)` (layers the rules read
that the map has no tile layer for, by exact name, leaving out those an earlier rule map
writes), and after a run the rule maps that matched nothing with the reasons the inputs give
(an empty or missing input layer), which the editor turns into "why the rules did less than they
could". Rule maps are never silently skipped: every reason has a typed problem.

## 10. AutoMap layers

Tiled's automapping writes into the layers it names and cannot tell what it made from what was
painted, and it finds layers only by name, so a pack whose rules read `walls` does nothing on a
map that paints platforms on `plat1`. The `layers` module keeps the two apart:

```
Setup { rules: PathBuf, runs: Vec<Run { bindings: Vec<Bound { name, objects: bool, layer: LayerId }> }>, seed }
RuleLayers { inputs, tile_outputs, object_outputs, made }      what the rules read and write, and which inputs an earlier rule map makes
AutoLayers { painted, setup, rules, stages: Vec<Arc<Map>>, problems, generation }
```

- The **painted** map is the document's; the **result** is derived: stage 0 is the painted map,
  stage `i + 1` is stage `i` after run `i`, the last stage is what the canvas shows, what Save,
  the exports, recovery and `bt` use.
- A **run** binds the names the rules use to map layers. Bindings are a name map the engine
  takes (`with_layer_names` plus "write unbound outputs into temporary layers that are dropped
  afterwards"): no layer is renamed, hidden or unlocked in a working copy. Inputs an earlier
  rule map makes need no map layer (Tiled's sewers example reads what the first rule map wrote);
  a layer bound twice is refused; a run whose bound layer is gone is skipped with a problem.
- **Updating** after a change: nothing changed, a tile area changed (a region run on a clone of
  the stage before, then the unaffected cells copied from the previous stage, exact by §7), or
  everything (the rules, the setup, the layer structure or an object output changed: a full
  run). Objects the rules place keep the ids they had in the previous result when they are the
  same objects, so a diff of the map does not call them changed.
- **Explaining** a run explains its input stage with its bound names (`explain_input(run)`).
- **Persistence**: `painted_record(setup, layers, painted, result)` produces the record
  `bt-project` stores for the map (the setup; per written tile layer the painted cells where they
  differ from the result, as run-length rows; per object layer the generated object ids; a
  checksum of each written layer as saved; the painted next object id) and `restore(record,
  loaded)` rebuilds the painted map from the saved result; a layer whose checksum no longer
  matches (edited elsewhere, in Tiled say) counts as all painted, with a note. The record types
  live in `bt-project` so the file format depends on no algorithm crate.

Start (binding chosen in the panel; right after an explicit full run the painted map is the map
before it), Stop (the result becomes the painted map), Re-roll and binding changes are the
editor's transactions (`05-editor-core.md`, §16).

## 11. Files

`files::find_rules(map_path, setup, project) -> Option<PathBuf>`: the setup's rules file, else a
`rules.txt` beside the map, else the project's rules file. `files::load(loader, rules_path,
map_path) -> Result<RuleSet>` reads the list (or a single rule map), filters it by the map's
file name, loads each rule map through the loader so tilesets are shared with the map, and
compiles. A missing rules file or rule map is an error, as in Tiled; a rule map that cannot be
parsed is a warning and, on `reload(previous)`, keeps the rules it had until it is fixed.
`tileset_files()` lists the external tilesets to watch, and `classes_changed(types)` says when
the project's custom types would give rule tiles another `MatchType`, so the editor reloads.
Nested `rules.txt` includes are read with a cycle guard, and a failed load never duplicates
entries.

## 12. Deliberate differences from Tiled

Each is documented in the crate and logged in `docs/decisions.md`:

- randomness is a pure function of the seed, the rule and the position (Tiled's is unseedable);
- `ModX`/`ModY` select positions on a lattice in map coordinates (Tiled's phase depends on where
  the scanned rectangle starts, which shifts patterns with selections and while drawing);
- locks inherited from groups are honoured by tile outputs and by `DeleteTiles` (Tiled ignores
  them there);
- objects are not duplicated when an output index has several object layers (a Tiled 1.11
  regression);
- new output layers are created in rule-map order (Tiled's depends on hash order);
- an option that cannot be read is a warning, not zero;
- region runs are exact and chained rule maps need no intermediate layers;
- include cycles and failed loads do not recurse or duplicate.

Where Tiled's behaviour is a documented limitation rather than a bug (object positions on maps
with other tile sizes than the rule map's; hexagonal maps where rules ignore the stagger), the
limitation is kept and named in a note.

## 13. Cost

Anchors make matching proportional to the anchor hits, not to rules times area; grids make reads
constant-time; region runs make a stroke cost its window, not the map. The Highlands pack (two
dozen rule maps, two thousand rules) is the benchmark: a full run of its sample scene and a
region run after a stroke are timed by a test that asserts an upper bound, so a regression shows
as a failure, not as a slower editor.

## 14. Tests

| Test | What it pins |
|---|---|
| Tiled's cases | every directory of `reference/tiled/tests/automapping` run as Tiled's own harness runs it (the rule map applied to the whole map), tile layers compared with the expected result; the cases without an expected result checked for their documented behaviour; a test that every case is covered |
| Options | each option, locks (own and inherited), legacy regions mode, invalid and unreadable rule maps, notes, tileset copies, object outputs on orthogonal, staggered and hexagonal maps, infinite-map growth, the margin |
| Fuzz | random rule sets and maps: a region run equals a full run inside the affected area and changes nothing outside; explanations agree with the runs they describe |
| Explain | writes, attempts, checks at the right moment in both modes, near misses, overwritten matches, unnamed tiles |
| The pack | the Highlands rules reproduce the pack's sample scene; incremental runs equal full runs; timings under their bounds |
| Layers | chained rule maps (sewers) with no intermediate layer; bindings on other layer names; a layer bound twice refused; stable object ids across runs; a record written and restored round-trips the painted map; a changed checksum counts as painted |
| Files | discovery order, filters and includes, reload keeping an unreadable rule map's rules, class-given `MatchType`s, paths through symbolic links |

## 15. As built at M3 (core)

The core of the crate: compiling and running rule maps with Tiled's results, the observer and
the run parameters later phases build on, the files, and the part of the analysis that needs no
run. `region`, `explain`, the post-run analysis and `layers` come later.

| Module | As built |
|---|---|
| `rulemap/` | `RuleMap` (path, source map, options, rules, inputs and outputs by name, problems, `is_used`) compiled once: layer roles (`layers`), options with Qt's conversions (`options`), `MatchType` from a tile's property or its class (`match_type`), 8-connected components (`extract`), input sets as Tiled's `compileInputSet` (`inputs`), output sets (`outputs`), `Rule`, and the `TilesetTable` |
| `rules/` | `RuleSet` (the rule maps in run order, the table, `rules_file`, `tileset(index)`, set-level problems) built by `RuleSet::builder()` from maps in memory; `Report` and `LockedOutput` |
| `engine/` | `RunRequest` (seed, `Area::{Whole, Selection, Edit}`, `LayerNames`), tileset and layer binding, grids, anchors, matching, applying, `DeleteTiles`, objects, write-back; `RuleSet::{apply, apply_to, run, run_observed}` |
| `observer` | `Observer` (with `ACTIVE`, `tries_every_position`, `attempt`, `check`, `write`), `NoObserver`, `Attempt`, `Outcome`, `Check`, `Condition`, `Write`, `Writer` |
| `files` | `find_rules`, `load`, `reload` and their `Error::{RulesFile, MissingRuleMap}` (the crate's one error, so `bt-tmx` stays in this module); `RuleSet::{tileset_files, classes_changed}` |
| `analysis` | `RuleSet::{tileset_copies, missing_layers}` with `TilesetCopy` and `MissingLayer` |
| `problem` | `Problem { kind, text }` with `ProblemKind` and `Severity` |
| `rng`, `bx`, `cell_set`, `object_cells`, `rule_ref` | the seeded draws; inclusive boxes; sets of cells as row runs visited in `QRegion`'s band order; Tiled's `objectTileRect` and `QRect`'s handling of zero and negative sizes; `RuleRef` |

**Grids.** A run binds the names the rules use (output layers of every rule map first, in rule
map order, as Tiled prepares them; inputs when their rule map runs) and copies each bound layer
into a grid: a dense copy over the area of interest (a finite map whole; on an infinite map the
region's box grown by the farthest any rule reaches), the layer itself behind it for reads
outside, and a sparse overflow for writes outside. Above 16 million cells a grid keeps no dense
copy. Each grid marks what was written; the write-back compares only those cells.

**Anchors.** Each input set's candidate anchors are its "any of" conditions on a layer the map
has that do not accept the empty cell. When a rule map starts, the engine indexes every
candidate's tiles in one pass per grid (a bitset per grid and tileset over tile ids rejects the
other cells) and gives each set the candidate whose tiles are in the fewest cells. A rule is
tried where one of its sets' anchors is, in Tiled's scan order (the region's band rectangles,
each row by row); with `MatchInOrder` the index and the rule's pending positions follow its
writes. A rule with a set that has no anchor scans its lattice, as do rules an observer asks to
see everywhere. With `OverflowBorder` or `WrapBorder`, positions whose box leaves the map are
tried too, since cells there read others. On the Highlands pack (24 rule maps, 1850 rules)
loading takes 5 ms, a full run of the sample scene's platforms 1.7 ms and of the same repeated
on a 480×480 map 16 ms (release, development machine).

Where the build differs from the sections above:

- `apply(map, Some(rect), seed)` runs on the whole map with a `FullRunFallback` note and
  `incremental: false` until region runs exist; `apply_to(map, rects, seed)` is Tiled's
  selection run (`incremental: true`). A run's `Report` has `tilesets_added: Vec<TilesetSlot>`
  and `objects_added: Vec<ObjectId>`; a `LockedOutput` counts the times a lock kept a rule map
  out of a layer (a `DeleteTiles` pass counts once).
- There is no separate `RuleMapInfo`: `RuleMap` answers what §9 lists, its errors, warnings and
  notes being its problems by severity.
- Bindings are `LayerNames` (tile and object names bound to layer ids, the rest looked up by
  name or written to temporary layers dropped after the run); a binding to a missing layer, a
  layer of the wrong kind or a layer bound twice refuses the run with a `BindingRefused`
  problem. `find_rules` takes the setup's rules file as a path until the setup type exists, and
  gives `None` when no file is found.
- Similar tilesets are unified when a run binds the table, in Tiled's order (the map's tilesets
  first, then those the run adds); the table merges only the same value or file. The tiles of a
  tileset the map lacks get slots after the map's and the tilesets used are added in table
  order at the end.
- `MatchType` is read when a rule map is compiled, from its own tileset; Tiled reads it at run
  time after unification, so a rule map embedding a copy of a map tileset whose tiles have other
  properties would read the map's. A `MatchType` that is a custom enum's value counts by its
  name (Tiled reads such a value as empty text, an ordinary tile), and one that is none of
  Tiled's names is a warning.
- Option values: text read as a yes or no follows Qt (anything but empty, `0` and `false` is
  true) with an `OddOptionValue` warning when it is not `true`/`false`/`1`/`0`; a modulus of 0
  is read as 1 with that warning; an output layer's `Probability` that cannot be read is a
  warning and is not merged into the target (Tiled merges it as a property).
- Fixed on purpose besides §12: `NoOverlappingOutput` keys an object output's footprint by its
  object layer (Tiled keys it under the tile layer of that name, usually none, so all object
  outputs share one footprint); references in list values are rewired with the rest; `DeleteTiles`
  honours locks, a group's included, unless the rule map sets `IgnoreLock` (Tiled ignores them
  there entirely).
- Kept from Tiled: every rule map a rules file lists must exist, filtered or not; a rule map
  that cannot be read is left out with a problem (kept from before on `reload`); points and
  polygons cover cells as Tiled's `objectTileRect` gives them; the region grows by a match's
  whole output region even where locks kept it out.
- Rules whose boxes share a top-left (one wrapped around the other's corner) are ordered by
  their first cell; Tiled's sort leaves them in any order.
- Write-back prunes the chunks it empties, as `bt-edit` does, so an infinite layer's chunk
  bounds can shrink where Tiled's keep their size.
- The engine reports `Applied`, `SkippedByProbability`, `Overlapping` and `NoMatch`; the other
  outcomes are for `explain` to give positions the engine never tries. Cells in checks and
  writes are in the run's numbering (the map's slots, then the tilesets it would add); rule
  tiles are `MatchCell`s in the table's (`RuleSet::tileset`).
- Not yet: nested rules files are not listed for watching (`bt-tmx` does not return them), and
  include cycles are skipped without a problem; region runs, the fuzz tests and the timed region
  run of §13 wait for `region`.
- Shared helpers: Tiled's similar-tileset rule is `bt-core`'s `Tileset::is_similar_to` (with
  `TilesetRef::is_same_tileset_as`), which `bt-edit` uses too; `bt-test-data`'s grids name the
  rule tiles by letter (`X` Negate, `I` Ignore, `N` NonEmpty, `E` Empty, `O` Other) and
  `rule_tiles()` draws the rule tileset.

Tests: all 19 directories of Tiled's automapping tests are covered, the 14 with an expected
result equal to it and the 5 without checked for what they show; the options, files and pack
suites of §14 (fuzz, explain and layers wait for their phases); the Highlands rules give back
the sample scene's platforms, each tile the scene's or another variation of the same tile, and
exactly its base tiles without the variation rule maps.
