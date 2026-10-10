# Decisions

One entry per decision that shaped the design, newest last. Each names what was decided, what
it beat, and why. The design docs in `docs/design/` state the result; this file keeps the
reasoning so it is not relitigated.

## 2026-10-09 — Two programs over one set of crates

**Decided:** `apps/better-tiled` (the editor window) and `apps/bt` (the command line) are
separate binaries sharing every library crate.
**Instead of:** one binary that behaves as `bt` when started under that name.
**Why:** `bt` then builds without egui, wgpu's window plumbing or file dialogs, so agent tooling
compiles and tests in seconds; the editor's command line holds only what a window needs; and
every headless job has one home. `bt open` finds the editor next to its own executable, through
`$BETTER_TILED`, on the `PATH`, or in `/Applications`.

## 2026-10-09 — Hybrid licensing

**Decided:** `bt-edit`, `bt-terrain`, `bt-automap`, `bt-editor`, `bt-script` and both
applications are GPL-3.0-or-later; `bt-utils`, `bt-core`, `bt-image`, `bt-tmx`, `bt-render`,
`bt-project`, `bt-live` and `bt-godot` are MIT OR Apache-2.0.
**Instead of:** everything GPL, or everything permissive with every algorithm re-implemented from
behavioural descriptions.
**Why:** porting Tiled's automapper and Wang filler directly is the fastest route to identical
results, and Tiled's editor code is GPL; keeping the core crates permissive lets other tools read
and write Tiled files with them. GPL-derived code never enters a permissive crate.

## 2026-10-09 — `bt-godot` is permissive and written from Godot's format

**Decided:** the Godot export is written from Godot's documented text-scene and resource formats
and checked against Godot running headless; it ports nothing from Tiled's `tscn` plugin.
**Instead of:** porting Tiled's exporter and making the crate GPL.
**Why:** the export goes well beyond Tiled's (a complete shared TileSet, the regeneration
contract, stable identity), so a port would be rewritten anyway, and a permissive crate can be
reused by Godot tooling. The first commit of the crate is reviewed for this.

## 2026-10-09 — `clap` for `bt`, one command table, fixed exit codes

**Decided:** `bt`'s commands and options are `clap` derive types, declared once; a test checks
that every command and option `bt guide` names exists; exit codes are 0 done, 1 failed, 2 wrong
usage, 3 refused, 4 out of date, 5 problems found (`check`).
**Instead of:** a hand-written parser with its own option table, and `check` always exiting 0.
**Why:** a hand-kept option table and a hand-written guide drift apart; a distinct code for
"problems found" lets scripts and CI use `check` like a linter without confusing problems with a
command that could not run.

## 2026-10-09 — CI on three platforms from the first commit

**Decided:** GitHub Actions runs format, clippy and the tests on macOS, Linux and Windows,
fetching Tiled's examples and test maps at a pinned commit and running the renderer on each
platform's software adapter. The purchased packs never leave the development machine, so their
tests skip on CI and say so.
**Instead of:** macOS only, or macOS and Linux with Windows later.
**Why:** the repository is public and the editor claims to build everywhere; a platform that is
not tested is not supported. Software adapters make the GPU tests run without a GPU.

## 2026-10-09 — Tiled parity scope

**Decided:** every file Tiled 1.0 and later writes is read and written, oblique maps and
`.js`-wrapped maps included; exports are Tiled's own formats, pictures, the resolved JSON and
the Godot scene. Tiled's scripting API, its other exporters and its translations are out.
**Instead of:** orthogonal maps only for a first version, or dropping the odd dialects.
**Why:** reading every dialect is cheap once the raw tree exists, and a map that does not open
is the one thing a Tiled user will not forgive; the non-goals are features whose value lies
elsewhere (engine loaders, JavaScript extensions).

## 2026-10-09 — No Python oracles in the repository

**Decided:** golden fixtures for terrain and automapping come from Tiled's own test cases and
from property tests; the repository holds Rust only.
**Instead of:** keeping GPL-derived Python ports of Tiled's algorithms as fixture generators.
**Why:** Tiled's automapping tests ship expected results, Tiled's Wang picks are unseedable (so
an oracle could only confirm "a valid lowest-penalty choice", which a property test states
directly), and one language keeps the repository and its CI simple.

## 2026-10-09 — Nothing of Tiled's is committed

**Decided:** `reference/` is fetched by a script at a pinned commit of Tiled's repository and is
git-ignored; CI fetches it the same way.
**Instead of:** committing copies of Tiled's small test maps.
**Why:** copies drift from their source and carry Tiled's licenses and credits into this
repository; a pinned fetch gives the same bytes everywhere and makes updating Tiled's version one
line.

## 2026-10-09 — Godot 4.3 is the minimum

**Decided:** the export targets Godot 4.3 and later; unique node ids are written for projects
whose features say 4.6 or later.
**Instead of:** supporting the deprecated `TileMap` node for older projects.
**Why:** `TileMapLayer` and `Parallax2D` arrived in 4.3 and the export is built on them; 4.6's
unique ids are what let a level scene keep its overrides through renames.

## 2026-10-09 — Headless pictures rasterise text

**Decided:** `bt render` and Export as Image draw text objects with a font bundled in the
binary, through the same drawing path as the canvas.
**Instead of:** drawing only the outline of a text object off-screen.
**Why:** an agent reading a picture should see what the designer sees, and one drawing path
cannot drift from itself.

## 2026-10-09 — Assets, documents and transactions

**Decided:** an asset store owns every loaded map, tileset, template, world and project; a
document is a tab onto an asset; every change is a transaction recording, per asset it touched,
the root before and after, plus the selection. Tilesets are not part of a map's history: a
restored snapshot refreshes its tileset references from the store.
**Instead of:** one history per map that owns its tilesets, with tileset reloads and edits
written into every snapshot of every history after the fact.
**Why:** rewriting histories is where undo stops being trustworthy; one store gives one list of
unsaved work, cross-asset atomicity, and a quit prompt that cannot miss a dirty tileset.

## 2026-10-09 — Snapshot undo on copy-on-write chunks

**Decided:** undo keeps whole `Arc<Map>` snapshots; tile layers are 16×16 chunks behind `Arc`
and object lists are shared the same way.
**Instead of:** a command pattern with a hand-written inverse per command.
**Why:** a stroke copies only the chunks it touched, so snapshots cost about what a diff would,
and there is no inverse to get wrong; the undo tests become property tests.

## 2026-10-09 — One raw tree behind XML and JSON

**Decided:** `bt-tmx` parses both syntaxes into one raw tree and converts raw to model in one
step; writing goes model to raw to syntax.
**Instead of:** two readers and two writers that each know the defaults and legacy conversions.
**Why:** Tiled's own two codecs disagree in places because they are two; one conversion means
one place for every default and every dialect.

## 2026-10-09 — No process-global state in library crates

**Decided:** options are values passed in, caches are owned by the caller, hooks are trait
objects given at construction; only the binaries hold globals, for logging.
**Instead of:** static switches and function pointers set at startup.
**Why:** globals make tests order-dependent, hide who depends on what, and are how one crate ends
up reaching into another; every one of them has a value-shaped alternative.

## 2026-10-09 — The editor core is a crate

**Decided:** `bt-editor` holds everything the editor is apart from its widgets: the asset store,
transactions, the action table, the keymap, the palette ranking, settings, recovery, session and
the watcher's conflict model. The rule for new code: if it needs no egui type, it does not live
in `apps/`.
**Instead of:** an application crate that owns the logic and end-to-end UI tests that reach into
its fields.
**Why:** logic tested without a window is fast and stable; the window stays small enough to read;
and the live link can run the same editor logic for `bt` without a widget in sight.

## 2026-10-09 — Representation choices in the data model

**Decided:** every real number from a file is `f64`; tile layers key their chunks in a
`BTreeMap` ordered by row then column; flag sets use the `bitflags` crate; a Wang id is eight
lanes (`[u8; 8]`), packed into a `u64` only inside the terrain filler; a tileset's kind (atlas
or collection) is an enum; a cell is eight bytes with the empty cell canonical.
**Instead of:** `f32` where Tiled's own UI uses single precision; a `HashMap` of chunks sorted
by each writer; hand-written flag newtypes; a packed `u64` Wang id everywhere; an optional
image field that leaves "collection with columns" representable.
**Why:** JSON files store fifteen digits and must come back byte for byte; every walk over a
layer is deterministic without sorting; one way to write a flag set; readers of the model want
a lane, not a shift; invalid states that cannot be built need no checks.

## 2026-10-09 — Byte identity policies

**Decided:** TMX reals are written with Qt's six significant digits and JSON reals with Qt's
fifteen; a map keeps the `firstgid` the file gave each tileset as a hint and the writer uses
the hints while they still form a valid sequence; `version` and `tiledversion` are written back
as read; zstd layer data is written at `ruzstd`'s fastest level and is the one documented
exception to byte identity; zlib and gzip go through a port of zlib's deflate.
**Instead of:** shortest round-trip numbers everywhere (more precise, every file rewritten);
recomputing every gid on every save (Tiled's way, which shifts gids when an image is missing);
a native zstd library (not pure Rust) or no zstd at all.
**Why:** a Tiled user who opens a map here and saves it must see no diff; precision beyond
Tiled's cannot be stored in Tiled's files anyway.

## 2026-10-09 — Deliberate departures from Tiled's reader

**Decided:** JSON layers without `visible` or `opacity` load visible and opaque; `next_*_id`
values lower than an id in use are raised with a warning; a trailing comma in CSV data is
accepted; a `propertytype` naming a type the project lacks is kept and written back; a world
pattern that does not compile is kept with a warning; unknown elements, attributes and keys
produce a warning.
**Instead of:** copying Tiled's behaviour in each case (invisible layers, duplicate ids, a hard
error, a dropped type link, a dropped pattern, a debug message).
**Why:** each of Tiled's behaviours is a bug or a data loss that its own documentation does not
describe; being more lenient than Tiled is safe, being stricter is not, and nothing is dropped
silently.

## 2026-10-09 — Embedded images are bytes in the model

**Decided:** an image embedded in a Tiled file is kept as bytes in its `ImageRef` and decoded
through `bt-image` by content hash.
**Instead of:** writing it to a temporary folder at load time and referring to that file.
**Why:** loading a file must not write anything; a temporary folder the system cleans up is a
reference that can break; the bytes are what the writer needs anyway.

## 2026-10-09 — Rendering choices

**Decided:** a layer with a blend mode is composited as a whole against what is below it; the
parallax origin keeps the sign Tiled's documentation gives; top-down object layers sort by
screen y everywhere, hit tests included; tint uses Tiled's exact formula; far zoom draws each
chunk as one impostor picture; the grid, outlines, selection, marks and text are drawn by the
renderer itself; a frame assembles the visible instances from per-chunk caches on the CPU.
**Instead of:** blending every primitive as Tiled does (impossible in one GPU pass without
backdrop reads); copying the 1.11 sign regression; sorting by pixel y as Tiled's export does;
leaving vector drawing to the window's widget painter; keeping chunk instances resident on the
GPU with one draw per chunk.
**Why:** per-layer compositing differs from Tiled only where tiles overlap inside one blended
layer; the documented parallax is what users read; one order for drawing and picking; one
drawing path cannot drift from itself; impostors are what keeps a million-cell view inside the
budget; per-chunk GPU slots cannot interleave chunks in Tiled's draw order on projected grids,
and the CPU assembly is linear in what is visible.

## 2026-10-09 — Editing choices

**Decided:** one `Brush` type for the Stamp tool, the fills, the quick stamps and the saved
stamps; an impossible request (a missing layer, object or tileset) is a typed error; the eraser
leaves hidden layers alone; every operation that depends on the grid goes through the map
geometry, staggered and hexagonal maps included; a selection carries its tiles on the grid's
own lattice.
**Instead of:** three brush representations; `None` or `false` for a bad id; Tiled's
Shift-erase of hidden layers; stamps and snapping that work on square grids only; Tiled's
cut-and-paste as the only way to move tiles.
**Why:** one type means one set of transforms and one file format; a no-op hides bugs; an edit
to what the user cannot see is a surprise; Tiled's maps come in every orientation; moving a
selection is Tiled's most requested missing feature.

## 2026-10-09 — Editor core choices

**Decided:** the editor core is synchronous and owned by the UI thread, with every blocking job
run by the window on a worker and fed back as an intent; dirty state is a version number per
asset that transactions bump and refreshes do not; a transaction is undone only while every
asset it touched is still at the version it left, else the editor names the later step; the
selection, the carried tiles, the current layer and the AutoMap setup are recorded in every
transaction; the keymap has its own chord type; settings are a `settings.toml` with versioned
migration and the session a `session.json`.
**Instead of:** threads and timers inside the core; dirty by pointer equality with the saved
root; undoing a cross-asset transaction unconditionally; selections outside the history;
the toolkit's shortcut type; the toolkit's own persistence.
**Why:** a core that is a function of what it is told is testable without a window and without
time; a version survives tileset refreshes that a pointer does not; silently undoing someone
else's later change is data loss; a selection that undo does not restore leaves the user lost;
the core must not name a toolkit; files a person can read and diff.

## 2026-10-09 — Application choices

**Decided:** tools implement one trait over gestures that carry the modifiers held at their
start and now; the window applies intents at the end of the frame and carries out effects; a
modal stack is the only knowledge of modality; file dialogs are asynchronous; the window
repaints only for input and for things that will change; Tiled's modifiers are kept where Tiled
has them, with the status bar naming what each held modifier does.
**Instead of:** a dispatch inside the canvas with a gesture enum per tool family; panels that
mutate the editor or leave mailboxes; fifteen booleans that mean "a modal is open"; blocking
dialogs inside dispatch; repainting every frame; a new modifier vocabulary across the board.
**Why:** tools as state machines get headless tests; one intent path means one order of
effects; one stack means one answer; a blocked frame is a stall; an idle editor should cost
nothing; Tiled users keep their habits and the hint makes the mixture discoverable.

## 2026-10-09 — Terrain choices

**Decided:** the solver keeps Tiled's quirks that change results (the `isComplete` check that
never holds for corner and edge sets, the lookahead that can leave a cell invalid when a worse
tile would have worked, no backtracking); every choice is a function of the seed and the cell;
a fill may be given a colour to prefer; diagnostics are typed (`Needed` with a `Cause`) and the
sentences are built from them; the autotile layouts, detection, auto-labelling, patterns and
RPG Maker expansion live in the crate, not in the window.
**Instead of:** a better solver (backtracking, higher tiers) that would fill what Tiled leaves
red; Tiled's unseedable picks; Tiled's fill that ignores the chosen colour; strings for what a
cell lacks; labelling logic inside the tileset editor's widgets.
**Why:** a pack's terrain set must paint here as it paints in Tiled, red cells included, or its
author's fixes will not match; a preview that equals the paint needs a seed; filling an area
with one terrain is what people reach for; a typed need can be checked against the set; `bt`
and the editor label tilesets alike.

## 2026-10-09 — AutoMap choices

**Decided:** bindings are a name map the engine takes, never a renamed, hidden or unlocked copy
of the map; `MatchType` comes from tile properties only (the loader resolves classes); problems
and rule-map information are typed; region runs are exact and fall back with a reason; the
differences from Tiled listed in `08-automap.md`, §12 (seeded randomness, lattice phase,
inherited locks, no duplicated objects, ordered new layers, warnings for unreadable options, no
recursion or duplication on reload); the project-file records of AutoMap layers are defined in
`bt-project` and converted here.
**Instead of:** Tiled's behaviour in each case; a hardcoded table of the built-in tiles' ids;
strings for warnings; full runs after every stroke; `bt-project` depending on this crate.
**Why:** a working copy with renamed layers is where a run and its explanation drift apart;
one source of truth for what a tile means; the editor and `bt` group and act on problems; a
stroke must cost its window; a file format crate must not depend on an algorithm crate.

## 2026-10-09 — Project file choices

**Decided:** `better-tiled.json` is found upward from the map, stopping at the repository's top
and below the home folder, and created next to the open `.tiled-project` only when the map's
search reaches that folder; keys and paths are relative to the file and kept in the map's own
form, never canonicalised; unknown keys are kept at every level and output is sorted; painted
cells are text rows with run lengths; checksums are FNV-1a of a canonical byte stream; every
write is a locked re-read, a change to the caller's own record and an atomic rename, with the
lock in the per-user runtime folder; orphaned entries are reported, never dropped.
**Instead of:** entries in `.tiled-project` (Tiled rewrites it with only the keys it knows) or
as map properties (Tiled shows them); a file per map; canonical paths (a path through a link or
a macOS temp folder is another path, and tilesets stop matching); binary or base64 rows; a
lock file beside the project file; silent fresh starts.
**Why:** a clone of the repository carries the file; a diff of it is readable; two programs
write it without losing each other's entries; the rules and the map read their tilesets by the
same paths; nothing in the user's repository is created that they did not ask for.

## 2026-10-09 — Agent tooling choices

**Decided:** one library, `bt-script`, behind `bt` and the editor's live link; `clap` derive
types as the single command table with `--json` on every command; outcomes are typed and text is
one renderer with `Problem`, `Warning`, `Note` and `Fact` lines, problems first; names are slugs
with qualification and suggestions; a sprite stands where its measured foot is; shapes are
rasterised by cell centre and narrowed by grounds; a script is all or nothing with per-operation
seeds; `check` states facts and the guide holds the ways out; `bt` never writes a tileset file;
the guide is checked against the command table by tests; agent runs are a manual release gate.
**Instead of:** a hand-written argument parser; string reports; advice in problem lines;
domain generators (bridges, crossings) or answers ("where should X go"); persisting names into
`.tsx` files; validating with the agents' own reports.
**Why:** one table cannot drift from the guide; typed outcomes are what tests and the editor
read; agents act on problem lines that name a concrete place and skip prose; primitives compose
across every kind of map while generators fit one pack; a tileset is the user's file.

## 2026-10-09 — Live link choices

**Decided:** `bt-live` is transport and protocol only, generic over the work it hands the
window (a `Handler` on the connection thread, a deadline channel to the window's thread); one
request per connection, one JSON line each way, a versioned `Status`; sockets in the private
per-user runtime folder under `/tmp`, checked for ownership and mode on both sides; scripts run
inside the editor process on a base the editor hands out, land as one transaction only while
the document's version is the base's, and rerun on a conflict up to three times; a job past its
deadline is dropped; marks never switch the shown map; no live link on Windows.
**Instead of:** a crate that knows maps (it would depend on `bt-core` and `bt-script`); a
long-lived session with streaming; a socket in the data folder (long `$HOME` paths break the
104-byte limit, and sandboxes change `$HOME`); running scripts in `bt` on a TMX snapshot (a
round trip replaces the person's tilesets and loses unsaved tileset edits); applying an edit
whenever it arrives; named pipes on Windows.
**Why:** the thinnest crate that two programs share; a request that cannot hang either side;
the person's keystrokes are never interleaved with or overwritten by an agent's edit; an edit
`bt` reported as timed out is never applied unseen.

## 2026-10-09 — Godot export choices

**Decided:** generated files are build output under a contract (a `;` header with an FNV-1a
hash, a marker on the root, a uid derived from the path, replace only what is ours and
unchanged, otherwise refuse with the way out and write nothing, backups on Replace); names and
unique ids from Tiled ids, with ids on Godot 4.6+ projects; every orientation but oblique on
Godot's grids with the TileMapLayers at Tiled's offset and objects shifted; a hex side Godot
cannot draw falls back to half-offset squares with a warning; a complete TileSet shared per
grid by default with append-only sources; animations in atlas sources of their own; custom
data in Tiled's types; Tiled's property names keep Tiled's meaning but for two reported fixes,
and better-tiled's start with `godot`; rotated instances at the rotated centre with rotation;
corner-set seams under Godot's brush accepted; the parallax origin noted, not corrected;
`verify` runs Godot only on request; one code list for the report.
**Instead of:** merging into or rewriting changed files; counter names; moving the layers and
keeping objects at Tiled's coordinates; refusing hex sides Godot lacks; a TileSet of the tiles
used, inside each scene; Tiled's exporter's placement of rotated instances; an alternative tile
per centre colour for corner sets; a `scroll_offset` correction a camera zoom would break;
running Godot inside an export; free-text warnings.
**Why:** nothing done in Godot is lost silently and every refusal has a way out; a level scene
survives renames; game code can convert positions with the layer's own grid; a user can paint
any tile and no cell lacks tile data; Godot 4.3 must load the output; the report is the same in
the editor, in `bt` and as JSON.

## 2026-10-09 — Testing choices

**Decided:** missing data, GPU or Godot fails a test unless one variable turns it into a loud
skip, and CI fails on any skip that is not the private packs (or Godot off Linux); fixtures are
built in code at run time and committed only when small and ours; oracles are Tiled's own
expected results, property statements where Tiled is unseedable, a CPU compositor, Godot
headless and the packs' previews; goldens are rewritten only by an explicit variable and
reviewed as behaviour changes; randomised tests use seeded generators with the seed and case
printed, and the readers get libFuzzer targets run weekly; timing checks assert only in
optimised builds; agent runs and windowed checks are release gates run by hand, not tests;
shared fixture builders live in `bt-test-data`, which `bt-core` and `bt-image` do not use.
**Instead of:** silent skips; committed copies of Tiled's or a pack's files; `proptest` or
`insta` dependencies; timing assertions in debug builds; agent transcripts kept in the
repository; a separate fixtures crate.
**Why:** a suite that passes on a machine without the data proves nothing unless it says what
it did not run; fixtures beside their assertions stay readable; one language and few test
dependencies keep CI simple; a gate that needs a person's eyes is not pretended to be a test.

## 2026-10-10 — The zlib port keeps zlib's indexing

**Decided:** `bt-tmx`'s `deflate` module, a port of zlib's `deflate.c` and `trees.c`, indexes
slices directly, the one module where `clippy::indexing_slicing` is allowed, with its reason
stated; 800 vectors from zlib 1.2.12 pin its output, and it handles levels as Tiled does (`-1`
is 6, every other level is clamped to 1–9).
**Instead of:** checked access throughout, or `flate2`'s encoder (`miniz_oxide`).
**Why:** another encoder writes other, equally valid bytes, so every save of an unchanged map
would rewrite its layer data; checked access would obscure a line-by-line port whose indices
zlib's invariants bound, and the vectors catch any departure.

## 2026-10-10 — gzip layer data carries Unix as its system

**Decided:** a gzip member written for layer data has no name, no time and the system byte 3
(Unix), whatever machine writes it.
**Instead of:** the writing machine's code, as zlib writes it, or the byte each file had.
**Why:** the bytes must not depend on the machine; Tiled's Linux builds write 3, and the gzip
maps in Tiled's own test data have it. A gzip map saved by Tiled on macOS or Windows comes back
with that one byte changed, which is rare because gzip is deprecated for new maps.

## 2026-10-10 — Write requests per kind of value, with the loader

**Decided:** a write starts from `WriteRequest::for_map`, `for_tileset`, `for_template`,
`for_world`, `for_project` or `for_stamp`; it takes the `Loader` for the templates instances are
compared with and for the custom types; `render()` returns `Rendered { text, warnings }`
without writing, and `fragment_text` renders clipboard text.
**Instead of:** one constructor over every kind with the custom types passed on their own, and
a `write_tileset`-style function per kind.
**Why:** one builder gives every kind the same options; the loader already holds the cached
templates and the project's types, and writing reads no files; rendering without writing gives
the live link and the tests the exact text a save would write.

## 2026-10-10 — The built-in rule tileset has an image of its own

**Decided:** the tileset behind `:/automap-tiles.tsx` is compiled into `bt-tmx` with a PNG
drawn by `scripts/automap-tiles-png.py`.
**Instead of:** Tiled's `automap-tiles.svg`.
**Why:** that image is part of Tiled's editor sources, which are GPL, and `bt-tmx` is
permissive; rules match the tiles' `MatchType` properties, not their pixels, so only the look
differs.

## 2026-10-10 — Tilesets are written for the image size they declare

**Decided:** an atlas's `columns` and `tilecount` are written for the image size its file
declares, and the loader drops atlas entries without data inside that grid.
**Instead of:** counts from the image as it is on disk, as Tiled writes them.
**Why:** a tileset whose image changed size since it was saved (a sheet in one of the packs
grew taller) would otherwise be rewritten by a save that edited nothing; the editor records the
new size when the user answers the image-changed question, and the next save writes its counts.

## 2026-10-10 — Tiled's JSON files are under the byte rule too

**Decided:** the round-trip rule covers every corpus file that records a `tiledversion`, XML or
JSON, with five files listed that Tiled itself would rewrite.
**Instead of:** holding only XML files to byte identity.
**Why:** Tiled's JSON writer is as deterministic as its XML writer, and a JSON map saved
unchanged must not show up as a change in version control.

## 2026-10-10 — Far zoom draws squares of layer runs

**Decided:** far out, a run of tile layers (normal blending, one parallax factor, nothing drawn
between them) is drawn from pictures of world-aligned squares, 256 texels a side, each holding
every layer of the run and clipped to its square; a square is kept while its run's chunks,
attributes and tilesets are the ones it was drawn from; squares are drawn until 100,000
instances a frame have gone into them; a view of more than 100,000 cells goes far below 16
device pixels a cell (else below 8).
**Instead of:** a picture per chunk of each layer (`03-rendering.md` as first designed).
**Why:** on the budget map at a Retina view, pictures per chunk and layer need a screen of
texture per layer (about 210 MB at 1/16 for ten layers, against 128 MB) and a walk over every
chunk picture each frame (39,000 at 1/32): 11 to 12 ms a frame. Squares are bounded by the
screen, not the map: the median far out is under 1 ms and the 95th percentile 6.9 ms at worst.
Clipping each square to itself, with the parts of oversized tiles that reach into it, keeps
Tiled's stacking across square edges, which pictures of whole chunk stacks would not.

## 2026-10-10 — Quitting asks without unsafe code

**Decided:** on macOS the app menu's Quit item gives its ⌘Q key equivalent to the window, so ⌘Q
runs the Quit command and asks about unsaved work as the close button does; a quit from the app
menu, the Dock or a logout ends the run without asking, and the window writes the snapshots the
latest changes need and keeps them for the next start.
**Instead of:** adding `applicationShouldTerminate:` to the application delegate at runtime
through `objc2` (`06-application.md`, §12 as first designed).
**Why:** that needs `unsafe`, which the workspace forbids and which goes to design review first;
the key and the close button are the usual ways to quit, and recovery keeps the rest. The
delegate method is the one question left for that review.

## 2026-10-10 — The saved zoom is in screen points

**Decided:** a document's camera keeps its zoom in screen points per world pixel (1 is 100 %).
**Instead of:** device pixels per world pixel.
**Why:** a session restored on another display, or a window moved to one, shows the map at the
same size it had; device pixels are the renderer's unit, worked out each frame.

## 2026-10-10 — Recovered work is the document shown

**Decided:** while a restored session's files load, a document opened from recovered work stays
the current one, whichever loads last.
**Instead of:** the session's current file becoming current again when it finishes loading.
**Why:** the user just chose to recover that work; the session's copy of the same file opens
beside it, unchanged.

## 2026-10-10 — Menus are the window's arrangement of the action table

**Decided:** which command goes in which menu, the separators and Open Recent are declared in
the window; `bt-editor` declares the commands, their contexts and keys, and answers whether each
is enabled and checked.
**Instead of:** a menu table in `bt-editor`.
**Why:** where a command sits is presentation, and the editor core holds none; `bt` has no menus
and needs no menu table.

## 2026-10-10 — Checks leave the user's state alone

**Decided:** `--screenshot` and `--bench` open only the files given, neither restore nor write
the session, and do not offer recovery; `--screenshot` waits for loads, images and far squares
to settle and fails with the reason after 30 s.
**Instead of:** starting as a normal run does.
**Why:** a scripted run must not reopen yesterday's files over its own, stop at a question, or
change what the next real start restores.

## 2026-10-10 — Pack maps are saved in mirrors

**Decided:** a test that writes a pack's map works on `Scratch::mirror` of the pack: Tiled's
files copied, images hard-linked.
**Instead of:** saving the pack's map where it is, or copying the whole pack.
**Why:** the purchased files must never be written; hard links make a mirror of a large pack
instant, and the editor writes only Tiled's files.
