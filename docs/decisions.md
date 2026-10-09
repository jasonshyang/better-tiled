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
