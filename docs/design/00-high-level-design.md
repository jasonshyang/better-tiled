# better-tiled — high-level design

*2026-10-09. This is the entry point to the design docs. It fixes the goals, the shape of the
workspace, the responsibilities of each crate and the rules that hold across them. Each area
gets its own design doc (§16), written before the area is built and updated to say what was
built; every decision and the alternatives it beat are logged in `docs/decisions.md`.*

## 1. What better-tiled is

better-tiled is a tilemap editor for game development, written in Rust. It opens the maps,
tilesets, terrain sets and automapping rules that [Tiled](https://www.mapeditor.org) writes, saves
them back as Tiled would, and adds three things Tiled lacks:

1. **Smooth authoring.** A GPU canvas that stays at the display's refresh rate, tools with
   consistent modifiers, selections that carry their tiles, brushes with sizes, undo for
   everything, autosave and crash recovery, and a keyboard that never gets stuck in a text field.
2. **Rules you can see.** Terrain painting and automapping say what they did and why they did
   less than asked: which rule changed a cell, which tile a terrain set lacks, which layer the
   rules read that the map doesn't have. Rules run on layers of your choice without destroying
   what you painted.
3. **A way out of the editor.** A command line, `bt`, through which a coding agent reads, edits
   and checks maps with the same code the editor runs, also while the map is open in the editor;
   and an export of the map as a Godot 4 scene that never overwrites work done in Godot.

Many purchased tile packs ship with Tiled terrain sets and `rules.txt` automapping. better-tiled
exists so those packs keep working while the editing itself gets much better.

### 1.1 Goals, in order

| # | Goal | What it means in practice |
|---|---|---|
| 1 | **Feel** | Input to pixels in one frame at 120 Hz on a large map; no stalls on save, load, reload or automap; every state of the editor is visible (the tool, the mode, the keys that apply). |
| 2 | **Tiled files stay Tiled files** | Every file Tiled 1.0 and later writes opens; a saved file is byte for byte what Tiled would write; nothing better-tiled adds goes into a Tiled file. Terrain and automap results match Tiled's. |
| 3 | **Nothing silent** | Every tool, rule run, save and export reports what it did and why it did less. Problems name the file, the layer or the cell, and the way out. Reports state facts, never opinions. |
| 4 | **Agents get what humans get** | `bt` offers the editor's operations as text commands with the same reports; a map open in the editor is edited in place as an undo step. The tooling is made of primitives an agent combines, not of answers. |
| 5 | **Never lose work** | Atomic saves, read-only files respected, crash recovery, a file watcher that asks before replacing unsaved edits, a Godot export that refuses to overwrite a changed scene. |
| 6 | **Maintainable by a person and an agent** | Small crates with one concept each, a thin application, design docs that say what and why, tests that fail loudly instead of skipping. |

### 1.2 Non-goals

- Tiled's JavaScript scripting API and its extensions.
- Exporters for engines other than Godot 4, and loaders for any engine.
- A Godot plugin, or reading Godot's edits back into the map.
- Editing in a browser or on a phone.
- Translations of the user interface.

## 2. Principles

### 2.1 Product principles

- **The map file is Tiled's.** `.tmx`, `.tsx`, `.tx`, their JSON forms, `.world` and
  `.tiled-project` are written exactly as Tiled writes them. What Tiled has no place for lives in
  `better-tiled.json` next to the map (§12).
- **Names, not numbers.** People and agents refer to layers, terrains, tiles, sprites and object
  types by name. Global tile ids, flip bits and `firstgid` arithmetic exist only inside the file
  format crate.
- **Deterministic.** Every random choice (terrain tiles, rule probabilities, random brushes) is a
  pure function of a seed and a position, so a preview is what gets painted, a region run equals a
  full run, and an export is reproducible.
- **Facts, not advice.** Diagnostics describe what is (this cell needs a tile with these lanes;
  this rule read layer `walls`, which is empty). They do not guess what the user meant.
- **One behaviour, many doors.** The editor, `bt` and the exports call the same library
  functions, so a map edited through any of them ends up the same.
- **Tiled's conventions win ties.** Where a choice is arbitrary (modifier keys, selection
  behaviour, property names with a meaning in Godot), better-tiled does what Tiled does, so that
  Tiled users and Tiled's documentation stay useful.

### 2.2 Engineering principles

These are the project's rules; the detailed docs and reviews hold code to them.

- **Builder patterns for configuration.** A type with more than two or three required fields
  gets a builder, never a constructor with a dozen positional arguments. Operations with several
  options take a request struct built the same way.
- **Prefer fewer types.** A new case goes into an existing type before a new type is added; two
  overlapping types are merged rather than bridged by a third.
- **No `unwrap()` or `expect()` outside tests and `main`.** Library code returns `Result`; the
  binaries unwrap at the outermost layer if they must. Indexing a collection by a position that
  might not exist counts as an `unwrap`.
- **No `unsafe`.** A use case that seems to need it goes to design review first. The one
  allowed so far is the macOS quit guard (`06-application.md`, §12).
- **No silent fallbacks.** A missing or invalid configuration value fails loudly at startup with
  a clear message. A value a file cannot express, an id that does not exist, a cast that cannot
  fit, a limit that was exceeded: each is an error or a warning, never a default.
- **Mutating functions say so** (`apply_fill`, `record_event`), and names say what they do; a
  constructor names its source (`from_tmx`, `from_outcome`).
- **No tuples for structured data.** Even two fields get a named struct. Flags are one typed set
  per concept, never loose integer constants.
- **No circular dependencies between modules.** A cycle means a missing abstraction.
- **Common helpers live in `bt-utils`.** A primitive two crates need belongs there (§3.5).
- **One concept per module; thin `lib.rs`.** `lib.rs` is `mod` declarations and explicit
  `pub use` re-exports (no glob re-exports); each type lives in its own module with its unit
  tests inline.
- **Minimal public surface.** `pub(crate)` by default; `pub` only at real entry points.
- **Errors are typed per crate.** Each library crate has one `Error` enum (`thiserror`) at its
  root; binaries use `anyhow` at the top. Recoverable problems in input files are collected as
  typed warnings (file, kind, message, target) next to the result, never dropped and never
  reduced to strings.
- **No process-global state in library crates.** Options are values passed in; caches are
  owned by the caller; hooks are trait objects given at construction. Only the application and
  the binaries hold globals, and only for logging.
- **Linear history.** Changes are rebased, never merged with a merge commit.

## 3. The shape of the workspace

```
better-tiled/
├── Cargo.toml              workspace: apps/*, crates/*
├── apps/
│   ├── better-tiled/       the editor window (eframe/egui + wgpu)                 GPL-3.0-or-later
│   └── bt/                 the command line for agents and scripts                GPL-3.0-or-later
├── crates/
│   ├── bt-utils/           primitives shared by several crates                    MIT OR Apache-2.0
│   ├── bt-core/            the data model and map geometry; pure data             MIT OR Apache-2.0
│   ├── bt-image/           image files: decoding, caching, sizes, alpha bounds    MIT OR Apache-2.0
│   ├── bt-tmx/             Tiled's file formats, byte-identical writing            MIT OR Apache-2.0
│   ├── bt-render/          wgpu renderer for every orientation, headless PNGs     MIT OR Apache-2.0
│   ├── bt-project/         better-tiled.json, the project metadata file           MIT OR Apache-2.0
│   ├── bt-live/            the protocol between bt and a running editor           MIT OR Apache-2.0
│   ├── bt-godot/           the Godot 4 scene export and its regeneration contract MIT OR Apache-2.0
│   ├── bt-edit/            editing operations (ports of Tiled's commands)         GPL-3.0-or-later
│   ├── bt-terrain/         terrain (Wang set) brush and fill (port of WangFiller) GPL-3.0-or-later
│   ├── bt-automap/         rule-based automapping (port of AutoMapper), explain   GPL-3.0-or-later
│   ├── bt-editor/          the editor without a window: assets, history, actions  GPL-3.0-or-later
│   ├── bt-script/          the edit script language, queries, checks and reports  GPL-3.0-or-later
│   └── bt-test-data/       finds the git-ignored test corpora (development only)
├── docs/
│   ├── design/             living design docs, one per area (§16)
│   └── decisions.md        dated log of decisions and why
├── assets/                 purchased tile packs for tests (git-ignored, never committed)
└── reference/              Tiled's sources and examples for tests (git-ignored)
```

### 3.1 Why two binaries

The editor and `bt` are separate programs that share every library crate. `bt` builds without
egui, wgpu's window plumbing or the file dialogs, so changes to agent tooling compile and test
in seconds, and the editor's command line holds only what a window needs (files to open, a
screenshot mode, a benchmark); every headless job (exports, renders, checks) is a `bt` command.
When `bt open` needs the editor it looks next to its own executable, then at `$BETTER_TILED`,
then on the `PATH`, then (on macOS) in `/Applications`.

### 3.2 Why this many crates

Each crate is one concept with one reason to change, and the dependency graph is a strict DAG
(§3.3). The split pays for itself three ways: a crate's tests run without the others' setup
(no window, no GPU, no Godot); the permissive crates stay usable by other tools; and the
application stays thin, because logic that needs no widget has a home outside it. The rule for
new code: **if it needs no `egui` type, it does not live in `apps/`.**

### 3.3 Dependency graph

Each crate depends only on crates in the layers below it. The table lists the exact edges.

```
apps        better-tiled                                   bt
            ───────────────────────────────────────────────────────────
layer 4     bt-editor          bt-script        bt-godot      bt-live
layer 3     bt-edit   bt-terrain   bt-automap   bt-render
layer 2     bt-tmx    bt-project
layer 1     bt-core                                          bt-image
layer 0     bt-utils
```

| Crate | Depends on |
|---|---|
| `bt-utils` | nothing of ours |
| `bt-core` | `bt-utils` |
| `bt-image` | `bt-utils` |
| `bt-tmx` | `bt-utils`, `bt-core`, `bt-image` (image sizes for tileset checks) |
| `bt-render` | `bt-core`, `bt-image` |
| `bt-edit` | `bt-core` |
| `bt-terrain` | `bt-core`, `bt-image` (autotile layout detection reads pixels) |
| `bt-automap` | `bt-core`, `bt-tmx` (rule maps are Tiled maps; confined to one module, §10.2), `bt-project` (the AutoMap record its layers module writes and restores, §10.3) |
| `bt-project` | `bt-utils`, `bt-core` |
| `bt-live` | `bt-utils` |
| `bt-godot` | `bt-core`, `bt-image`, `bt-project` |
| `bt-editor` | `bt-core`, `bt-tmx`, `bt-image`, `bt-edit`, `bt-terrain`, `bt-automap`, `bt-project` |
| `bt-script` | `bt-core`, `bt-tmx`, `bt-image`, `bt-render`, `bt-edit`, `bt-terrain`, `bt-automap`, `bt-project` |
| `apps/bt` | `bt-script`, `bt-godot`, `bt-live`, `bt-tmx`, `bt-render` |
| `apps/better-tiled` | everything but `bt-test-data` |

`bt-test-data` is a dev-dependency of any crate whose tests read the git-ignored corpora.

### 3.4 Licensing

Tiled's editor (`src/tiled`) is GPL-2.0-or-later and its core library `libtiled` is
BSD-2-Clause. Crates that port editor code (`bt-edit`, `bt-terrain`, `bt-automap`) are
GPL-3.0-or-later and name their sources in their crate docs; crates that only follow `libtiled`'s
logic keep its notice in `THIRD_PARTY_NOTICES.md` and are MIT OR Apache-2.0. A crate that depends
on a GPL crate is distributed under the GPL whatever its own license says, which is why
`bt-editor` and `bt-script` are GPL and `bt-godot` must be written from Godot's file format, not
ported from Tiled's `tscn` plugin. GPL-derived code never enters a permissive crate.

### 3.5 What goes into `bt-utils`

The primitives several crates need, each in its own module and each defined exactly once:

| Module | What |
|---|---|
| `rect` | `IRect` (integer, position and size, half-open) and `DRect` (floating point); one definition each, one convention |
| `rng` | the seeded generator (SplitMix64), a per-cell derived generator, and Tiled's weighted `RandomPicker` |
| `raster` | points on a line and on an ellipse (Bresenham), used by brushes, shapes and strokes |
| `bands` | a set of cells as banded rectangles in Tiled's `QRegion` visit order, used by fills, terrain and automap |
| `hash` | a fast hasher for cell keys, and the content hashes files, Godot headers and the `bt` history use (one FNV, one place) |
| `atomic_write` | write to a temporary file next to the target, fsync, rename; refuse read-only files and folders with a typed reason |
| `paths` | search upward with stops (repository top, home folder), relative display paths, file labels |
| `text` | plurals, quoted lists, lowercase hyphenated names and their fuzzy matching |

Anything two crates would otherwise copy goes here; anything only one crate needs stays in it.

## 4. The data model (`bt-core`)

`bt-core` is pure data: no image pixels, no GPU handles, no file I/O, no widget types. Importers
build it, the renderer reads it, the editing crates mutate it, and undo is built on cloning it.

- **Map**: orientation (orthogonal, isometric, staggered, hexagonal, oblique), tile size, render
  order, stagger axis and index, hex side length, background colour, infinite or not (every tile
  layer's bounds follow it), an ordered list of tileset references, a layer tree, properties, and one
  named `FileFormat` value holding what a byte-identical write needs (format version, layer data
  encoding and compression, line endings, chunk size, export settings, the tileset references as
  written). Tilesets carry the same kind of value.
- **Layers** form a tree: tile, object, image and group layers share the common attributes (id,
  name, class, visibility, lock, opacity, tint, offset, parallax, blend mode, properties) and carry
  a kind-specific payload.
- **Tile layers** store cells in 16×16 chunks behind `Arc`, in a map keyed by chunk position. A
  chunk is the unit of copy-on-write (an undo step shares every chunk a stroke did not touch), of
  GPU buffer caching and of dirty tracking. Finite and infinite maps use the same representation.
- **Cells** are 8 bytes and `Copy`: a tileset slot (the map's index into its tileset list, with
  zero meaning empty), flip flags (horizontal, vertical, anti-diagonal, hexagonal 120°) and a
  local tile id. Global ids are computed only when writing files.
- **Tilesets** are shared, immutable values (`Arc<Tileset>`): image or collection, tiles with
  their properties, animation frames, collision shapes, Wang sets (the eight lanes of Tiled's
  Wang id, in Tiled's order), transformation flags. A map refers to an external tileset by
  path and to an embedded one by holding it; two maps open on the same `.tsx` share one `Arc`.
- **Objects** live in object layers in Tiled's pixel space. The shape is one enum (rectangle,
  ellipse, point, polygon, polyline, text, tile), so a tile object cannot also be a polygon; each
  object has rotation, visibility, class, properties and, for template instances, the template
  reference with the set of fields the instance overrides.
- **Properties** are a typed enum (string, int, float, bool, colour, file, object reference,
  class with members, enum, list). Values are kept in the form the file stores (an enum as its
  name or number) and resolved on demand against the project's custom types, which are an
  explicit value passed in, never global state.
- **Ids, not pointers.** Layers and objects carry map-unique id newtypes; tiles are (tileset
  slot, local id); templates and tilesets are reached by path. Parent links are derived. Looking
  up an id that does not exist returns `None`; it is never treated as "visible" or "locked".
- **Geometry** for every orientation lives here too: tile to pixel and back, hit testing, draw
  order, neighbours, snapping, and where an object is drawn on screen. Every crate that places
  something uses these functions, so the canvas, the headless renderer, the Godot export and
  `bt view` agree, and no operation is "orthogonal only".

Worlds (`.world`), projects (`.tiled-project`) and templates (`.tx`) are plain data here as well.

## 5. Files (`bt-tmx`, `bt-image`)

### 5.1 Reading

One `Loader`, built with the project's custom types and the location of the built-in
automapping tileset, reads maps, tilesets and templates in XML or JSON, detecting the format
from the contents so mixed references work. External tilesets and templates are cached by
canonical path and modification time, so every map read through one loader shares their `Arc`s
and a tileset changed on disk is read again on the next load. Every encoding Tiled has used is
accepted (XML tiles, CSV, base64 plain, gzip, zlib, zstd) and every legacy dialect back to
Tiled 1.0 (the file-formats doc lists them). Problems that would make Tiled refuse a file
(unknown gids, a missing tileset, an invalid Wang id) become typed warnings carried next to the
loaded map, so as much of a file as possible opens. Only an unreadable file, malformed XML or
JSON, or a file of the wrong kind is an error. Oblique maps and `.js`-wrapped maps are read and
written like any other; only the Godot export declines oblique maps, because Godot has no tile
shape for them.

XML and JSON are two syntaxes for one raw tree: each parser fills the raw tree with what its
file says, and a single step turns the raw tree into the model, applying defaults and legacy
conversions once. Writing goes the other way, model to raw tree, and each syntax printer omits
what its format omits. Embedded images are kept as bytes in the model and decoded through
`bt-image`; nothing is written to a temporary folder while reading.

The loader also reads `.world` files (including pattern-matched worlds), `.tiled-project` files
with their custom types, legacy `objecttypes.xml`, Tiled `.stamp` files, `rules.txt`, and map
fragments as TMX text (the clipboard format shared with Tiled).

### 5.2 Writing

A saved file is what Tiled would write for the same data, byte for byte: attribute order, number
formatting, compression output (a deterministic deflate port), line endings and indentation.
Global tile ids are assigned on write as Tiled assigns them. Round-trip tests enforce this over
the corpora (§14). The one deliberate difference, zstd layer data always compressed at the
fastest level, is documented in the crate. The writer reads nothing from disk: template
instances are diffed against the template value the loader holds.

Every write goes through `bt-utils`' atomic write: the bytes go to a temporary file next to the
target and are renamed into place, so a crash never leaves a half-written map. A read-only file
(locked by version control, say) is never written over; the writer refuses with a typed reason
that names it, and callers show that reason rather than silently skipping the save. Safe
writing is an option of the write request, not a global switch.

### 5.3 Images

`bt-image` is the one place that decodes image files (PNG, JPEG, GIF, BMP, WebP, TGA, TIFF, PNM,
ICO: the raster formats Tiled's images use; SVG and Qt's rarer formats are not read), caches
decoded images by path and modification time or by content hash for embedded bytes, reads an
image's size from its header without decoding it, applies a colour-key transparency, and
measures alpha bounds (used to find where a sprite's feet are). Decoding runs on the store's
worker threads with a completion hook, so a tileset's images never stall a frame. There is one
image store per process: the renderer, the panels and the headless pictures all read it, and
widget textures are derived from it rather than decoded again. A map file can serve as the image
of a tileset, a tile or an image layer (a picture of that map): the image store takes a
`MapImageSource` trait object at construction, which the application implements with the
headless renderer, so there is no global hook and no nested GPU device.

## 6. Rendering (`bt-render`)

The renderer draws a `Map` for every orientation with the geometry from `bt-core`, in Tiled's
draw order, with layer opacity, tint, blend modes (on groups too), offsets and parallax, animated
tiles, tile objects, image layers, and the vector content the editor and the pictures need: the
grid, object outlines and text, selection outlines, previews and marks. Text objects are
rasterised with a font bundled in the binary, so headless pictures show them too. One drawing
path serves the canvas and the headless pictures, so `bt render`, Export as Image and the
window agree.

- Tile images are packed into 2D-array texture pages; tiles are drawn as instances that name a
  sprite by index; animation advances through a small remap table, so no instance buffer is
  rebuilt for an animated tile.
- Tile layers are cached per 16×16 chunk, keyed by the chunk's `Arc`, which the cache retains:
  a chunk's instances are rebuilt only when the chunk changes, and a frame uploads only the
  visible ones. Far zoomed out, the tile layers are drawn from pictures of world-aligned squares,
  each holding a run of layers, so the work stays bounded by the screen at any zoom.
- Pages, sprites and chunk caches are evicted under a byte budget; the renderer is created once
  and never recreated to forget a tileset.
- The fragment shader samples with clamped texel loads ("sharp bilinear" magnified, box
  filtered minified): crisp pixel art at any zoom, no bleeding between tiles.
- An image larger than a page is split across pages; nothing is left out without a warning.
- The same renderer runs headless through one reusable device to produce PNGs for `bt render`,
  Export as Image, the minimap and the tests' golden images.
- The budget is one frame at 120 Hz (8.3 ms) for a 1000×1000-cell, ten-layer map at any zoom,
  with the editor's own work (input, panels, overlays) inside it.

The editor embeds the renderer through egui's wgpu paint callback; the renderer never knows about
widgets, and its errors are typed.

## 7. Editing operations (`bt-edit`)

`bt-edit` is the logic behind Tiled's editor commands as functions over `Map`: regions (sets of
cells with union, subtraction, intersection, outlines and runs), flood and magic-wand selection,
line, ellipse and shape fills, the multi-layer tile clipboard, stamps with weighted variations,
random brushes, carried selections (tiles lifted off a selection and moved, mirrored and turned
on any grid), map resize, crop, offset and infinite conversion, layer commands (new, duplicate,
move, group, merge, via copy or cut), tileset commands (add, replace, remove, swap tiles, adjust
indexes after an image grew), object commands (find, add, duplicate, move, order, flip, rotate,
align, distribute, snap, hit testing) and the object clipboard with reference rewiring.

Conventions that hold across the crate:

- An operation takes `&mut Map` and edits in place; the caller snapshots the `Arc<Map>` first,
  which is what undo is built on, so there is no undo logic here.
- An operation returns what changed (rectangles per layer, new ids, removed objects) so callers
  can report it, and leaves the map untouched when it changes nothing. A target that does not
  exist (a layer id, an object id, a tileset slot) is a typed error, not a no-op.
- No UI state: selections, the current layer and clipboards are passed in and returned, and
  callers pass modes (`SelectionMode::Add`), never modifier keys.
- Randomness comes from a `bt-utils` generator the caller seeds, so a preview is what gets
  painted.
- Every operation that depends on the grid goes through `bt-core`'s geometry and works on every
  orientation; stamps, snapping and offsets on staggered and hexagonal maps included.
- Locked layers are skipped by painting and erasing, as in Tiled; other lock decisions belong to
  the editor (§8), which has the reason to show.
- The brush is one type, a stamp with weighted variations and per-layer parts, used by the
  tools, the quick stamps and the saved `.stamp` files alike; the clipboard has one tile form
  and one object form. Segment, polygon and snapping geometry exists once, in `bt-core`.

## 8. The editor core (`bt-editor`)

`bt-editor` is the editor without a window: what is open, how it changes, how it relates to the
files on disk, and what the user can do to it. It has no egui types and is tested without a
window, so the application's own tests cover only what crosses widgets.

### 8.1 Assets and documents

- An **asset store** owns every loaded map, tileset, template, world and the project with its
  custom types. Each asset has an id, a path, the version on disk, and a dirty state. There is
  one list of unsaved work; the quit prompt and Save All cover all of it, whatever kind it is.
- A **document** is a tab onto an asset: its selection (cells, objects, layers), its current
  layer, its camera, and its history. A tileset open in a tab is a tileset document, not a map
  with one tileset.
- **Editor-wide state** (the tool, the brush, the terrain, the stamp) lives once, outside the
  documents, and is never copied between tabs.
- A tile selection's **carried** state (which tiles were lifted, from which layers, and what
  they covered) is explicit in the selection, so it travels through undo and redo and is never
  inferred from pointer equality.
- The **property-editing model** (what several selected things share, where they differ, what
  they inherit from a class or template, and the operations change, reset, remove, add, rename,
  convert and paste) lives here, so the Properties panel only draws it.
- The **stamp store** (a folder of Tiled `.stamp` files) and the quick stamps live here too.
- Documents are identified by id everywhere; nothing holds a tab index.

### 8.2 Transactions and undo

- Every change is a **transaction**: a label, an optional merge key (consecutive edits with the
  same key, such as typing in a field or dragging a slider, collapse into one step), the
  selection before and after, and for each asset it touched the root before and the root after.
  Undo restores the roots. Snapshots are cheap because chunks and object lists are shared
  copy-on-write.
- A transaction may span assets: a tileset edited from a map's tab, a resize that also moves the
  map in its world, a template change that updates its instances. It is recorded in the history
  of the document where it was made and applied atomically to every asset. A transaction that
  changed an asset is undone only while no later transaction changed that asset; otherwise the
  editor names the document whose step comes first.
- **Tilesets are not part of a map's history.** A map snapshot carries the tileset values it was
  made with, but when a snapshot is restored its tileset references are refreshed from the
  store. Undo in one map never changes a tileset, and a tileset change (an edit, a reload, an
  image whose size changed) never rewrites a history. Save As, which rebases relative paths, is a
  transaction too. No history is ever edited in place.
- **Dirty** is "the current root is not the root on disk": undoing back to the saved state
  makes an asset clean again. A content hash of the file tells the watcher whether a change on
  disk is our own save.
- Histories are bounded by steps and by bytes.

### 8.3 Actions and keys

- Every command is declared once, in one table: id, label, search keywords, context (always, a
  map, a tile layer, an object layer, a tile selection, a tileset tab, a text field…), default
  keys per platform, kind (command, toggle, tool), Tiled's `.kms` id, and what it needs enabled.
  Adding an action touches that one place.
- `enabled` and `checked` are pure functions of a snapshot of the editor; `execute` takes the
  editor and an action and returns **effects** (a dialog to show, a file to pick, a message to
  flash) for the window to carry out. No dialog opens inside dispatch.
- The **keymap** binds keys to actions with several bindings per action, detects conflicts
  across compatible contexts, imports and exports Tiled's `.kms` files, and is passed where it is
  needed so that menus, the palette, hints and messages name keys from it.
- The **command palette** ranks actions, files, layers, stamps, project commands and "x, y"
  go-to entries with one fuzzy matcher and a recency bonus.
- Panels and tools send **intents** up and never reach into the editor's fields; the
  application applies intents through `execute`.

### 8.4 Files on disk

- **Locks** are decided here: which layers a command may touch, with the reason it left others
  alone.
- **Recovery snapshots** of unsaved assets are written on a schedule (shortly after the last
  edit, and at least every half minute) and hold what Save would write, the painted state of
  AutoMap layers included; **session state** (open files, cameras, panels, per-project open
  maps) is written continuously, so a crash loses nothing but the last seconds.
- **The watcher's conflict model** lives here: a changed file reloads when the asset is clean
  (as a transaction) and asks when it isn't; a file that disappears keeps the tab as the only
  copy; a file that returns unchanged is clean again; a tileset image whose size changed offers
  to adjust the tiles' references in every map that uses it. Hashing, scanning and loading run
  on worker threads; only the commit of a transaction runs on the UI thread.
- **Issues** (missing files, load warnings, rule problems, export notes) are built from the
  typed warnings of the crates, with a severity, a source and a target to jump to; the
  application only draws them. Messages to the user are one `Notice` type with a severity, so a
  problem is never a bare string in a status field.
- **Settings** are typed (`serde` with defaults and versioned migration) in three scopes: global
  preferences, per-project session, per-file view state. Colours are plain RGBA here; the window
  converts them.

Data flows one way: a widget, a key or a `bt` request produces an intent; the intent runs a
library operation on a copy of the asset; the document records the transaction; panels and the
canvas re-read the store each frame, with caches keyed on asset versions and chunk identities.
There are no change signals and no re-entrancy guards.

## 9. The application (`apps/better-tiled`)

The application is the window: tabs, menus, panels, dialogs, tools and the canvas. It holds no
map logic of its own. The `App` is a small struct of a few sub-structs, one per concern.

```
apps/better-tiled/src/
├── main.rs        arguments, logging, eframe launch; --screenshot and --bench for checks
├── app/           the window: tabs, menus, panel layout, the modal stack, status bar
├── canvas/        camera, input to map coordinates, renderer callback, overlays
├── focus/         keyboard routing (§9.1)
├── tools/         one module per tool (§9.2)
├── panels/        tiles, terrains, stamps, layers, objects, properties, project, issues,
│                  console, history, automap, minimap (§9.3)
├── tileset_editor/ terrain labelling, animation, collision
├── dialogs/       new map and tileset, preferences, custom types, project and world
│                  properties, Godot setup, export; each driven by an effect from bt-editor
├── files/         the window side of open, save, reload and close; dialogs; the watcher's timer
├── live/          the socket server for bt and its job queue
└── platform/      macOS quit guard, reveal in Finder, open with the system editor
```

Modals are one stack of one enum, so "is a modal open" is one question. Dialogs run from
effects and report back as intents. Blocking work (file dialogs, Godot verification, exports of
many maps) never runs inside a frame.

### 9.1 Keyboard focus

Single-key shortcuts work everywhere except while a text field or a tileset canvas has the
keyboard. Scrolling, zooming, Space-dragging the map or picking tiles ends typing in a field. Tab
and Shift-Tab move between fields and never leave the keyboard with a widget that cannot use it.
The Escape that leaves a field does nothing else. Keys reach actions through one router with
exact modifiers; a disabled action does not swallow its key unless it has a reason to show.
These rules are a single module with tests, because they are the most common way an editor
feels broken.

### 9.2 Tools

A tool is a type implementing one trait: it receives pointer and key events already converted to
map coordinates, draws its preview through the canvas's overlay API, and commits edits by sending
an intent that runs `bt-edit`, `bt-terrain` or `bt-automap` on a copy of the map and records a
transaction. Tools never write to the map directly. The tools: brush (with stamps, multi-layer
brushes, random mode, size, flips and turns on every grid), terrain, eraser, fill and shape fill
(plain or with a terrain), picker, rectangular select, magic wand, same tile, offset layers, the
object tools (select, rectangle, ellipse, capsule, point, polygon, polyline, tile, text,
template, edit polygons), the World tool and the Explain tool. A tile selection carries its
tiles: moving, flipping and turning the selection moves them, each as one step, and what they
covered shows again when they move on.

The object tools edit any **object container**: an object layer of the map, a tile's collision
shapes in the tileset editor (in that tile's space, isometric projection included) or a
template. There is one set of handles, snapping modes and gestures, not a copy per container.
Every tool and every overlay reads the same shown map (the rules' result when AutoMap layers
are on), so what is outlined is what is drawn.

### 9.3 Panels

Panels are immediate-mode views of the store and the editor state that return intents. The
Properties panel edits every property kind, several selected things at once (differing values
marked), class and template inheritance with resets, object references with pick-on-map, and
arithmetic in number fields. The Tiles panel has per-tileset scroll state and a search over image
collections. The Layers panel is a tree with drag-reordering, groups, multi-selection, locks and
visibility. The AutoMap panel holds the Live layer bindings and the Explain output; the Issues
panel lists problems with jump targets; the History panel walks the transactions.

## 10. Rules (`bt-terrain`, `bt-automap`)

Both crates are ports of Tiled's algorithms, validated against Tiled's own test cases, with
deliberate differences documented in their crate docs.

### 10.1 Terrain

`bt-terrain` prepares a Wang set (candidate tiles with the flips the tileset allows,
probabilities, colour distances) and offers the brush (corners, edges or whole tiles as hard
constraints, neighbours re-solved out to the set's maximum colour distance), the fill (cells get
terrain that connects to the unchanged tiles around them) and previews that are exactly what a
paint would do; each takes one request value built with a builder. Terrain colours are a typed
value. Its report says which cells no tile fits and **why**: the lanes the cell needs, whether a
neighbour forced them, or whether the only fitting tile has probability zero; which tiles of
another terrain the stroke pushed back, naming the tile that would have let them stay; and which
cells of another terrain set it painted over, on every orientation. The same crate holds the
autotile layouts (the 47-tile blob, 16-tile and 3×3 island and hole sheets, RPG Maker A2
autotiles), detection of such blocks in a tileset image, one-click labelling of every block
found, and the "patterns" a set needs with the ones it lacks.

### 10.2 AutoMap

`bt-automap` compiles rule maps as Tiled 1.12 reads them (input, inputnot and output layers,
legacy regions, the special tiles Empty, Ignore, NonEmpty, Other and Negate read from the
built-in tileset's tile properties, so there is one source of truth; map and rule options;
MatchType from custom types) and applies them with results equal to Tiled's full run. Compiling,
applying and explaining work on maps in memory; finding `rules.txt` for a map, loading the rule
maps and reloading them when they change is one module, the only one that touches files, so the
engine's tests need no XML. An option that cannot be parsed is a warning, not a zero. A **region
run** after an edit is exact: it recomputes only the cells an edit can influence and falls back
to a full run when a rule map makes that impossible, saying why. Randomness is a pure function
of the seed, the rule and the position. **Explain** traces a run for one cell: the rules that
changed it, in order, those that matched without changing it, those that came close with each
condition's verdict, and a picture of each rule from its rule map; tracing is an observer the
engine is generic over, so an untraced run pays nothing for it. **Analysis** reports warnings
per rule map (tiles a regions layer leaves out, input and output that don't touch, tilesets that
are copies of the map's and so never match) and notes (tiles that make no rule). A rule map that
cannot be read keeps its previous rules until it is fixed.

### 10.3 AutoMap layers: rules without destruction

Tiled's automapping writes into the layers it names and cannot tell what the rules made from what
the user painted. better-tiled keeps the two apart, in `bt-automap`'s `layers` module:

- The document's map is **what was painted**. A **setup** (which rules run, which rule layer is
  bound to which map layer, the seed) is part of every transaction.
- **What the rules make** is derived: the setup runs over the painted map (region runs after
  each stroke, full runs when a rule map needs them) and the canvas, the minimap, Save, the
  exports, recovery and `bt` show the result. "Show what I painted" shows the other.
- The painted cells that differ from the result, the setup, the generated objects' ids and
  per-layer checksums are stored in `better-tiled.json` (§12), so the `.tmx` stays exactly what a
  Tiled user expects: the finished map.
- Bindings are a name map the engine takes with the run: no layer is renamed, hidden or
  unlocked in a working copy to make the rules find it, and explanations speak the map's names.
- Chained rule maps (one writes a layer the next reads) need no map layer for the intermediate
  result; binding a layer twice is refused; an empty input layer says so in the panel.

## 11. Agents (`bt-script`, `bt-live`, `apps/bt`)

### 11.1 What `bt` is

`bt` is the editor's operations as commands for a coding agent working with a person. Its
documentation is one text, `bt guide`, which is the only thing a fresh agent gets. The commands
form five groups:

| Group | Commands | What they answer |
|---|---|---|
| Look | `info`, `view`, `render`, `at`, `objects` | What the map is: text with one character per cell and a legend, a numbered picture, a cell layer by layer |
| Know | `catalog`, `areas`, `reach`, `explain`, `diff` | What it can be built with (terrains, sprites, tiles by id with usage), how cells connect, what a rule run does at a cell, what differs |
| Change | `edit`, `new`, `undo` | A script of operations, all or nothing, with a report per layer; a new map like another; the version before the last edit |
| Check | `check` | Terrain seams, opaque edges over the ground, objects over nothing or on top of each other, missing files: problems first, then facts |
| Editor | `editor`, `mark`, `open`, `save`, `diff` | What the person is doing; outlines on their canvas; opening and saving in the editor |
| Export | `export`, `godot …` | A map or tileset in another Tiled format, as a picture, resolved as plain JSON for other programs (no global ids, flips as booleans, every cell's centre and every object's screen position, with a schema), or as a Godot scene (§13.4) |

Commands and their options are declared once, as `clap` derive types, and a test checks that
every command and option `bt guide` names exists. Exit codes are fixed: 0 done, 1 failed, 2
wrong usage, 3 refused, 4 out of date, 5 problems found (`check`), so scripts can tell
"problems" from "could not run". `bt undo` restores the version
before the last `bt edit` from a per-map history in the application's data folder. A map open
in the editor with unsaved changes is never written by `bt`; the edit goes to the editor
instead (§11.3).

### 11.2 Design rules for `bt`

- **Tooling, not answers.** Primitives an agent combines: read, query, shape, transform, verify.
  No generators for particular things (bridges, rooms) and no "where should X go".
- **Problems before summaries.** Agents act on lines marked as problems and skip prose, so
  anything an agent should fix is a problem line that names a concrete fix, and everything else
  is a fact.
- **Names, feet and shapes.** Layers, terrains, sprites and types go by lowercase hyphenated
  names with fuzzy matching; tiles by `TILESET:ID`; a sprite stands where its feet are, measured
  from its image's alpha; areas are shapes (`rect`, `where TERRAIN`, `avoid`, `containing X,Y`,
  `near NAME`, `under LAYER`, `selection`) combined with `minus`.
- **All or nothing.** An edit script either applies whole or changes nothing, and reports what
  changed per layer, what it painted versus what rules made, the seams and hidden edges it caused,
  and the objects whose ground changed.
- **`bt` never writes a tileset file.** Names that should last across commands go in a
  definitions file passed with `--with`, not into the pack's `.tsx`.
- **The same report for every door.** The editor, `bt` and `bt --json` describe the same run in
  the same words; reports are typed values with a text and a JSON rendering.

`bt-script` is the library behind all of this: the script parser and executor, the shape
language, name resolution, the text views, the checks and the report types. Both `bt` and the
editor's live link run scripts through it.

### 11.3 The live link

When a map is open in the editor, `bt` works with the editor's copy instead of the file. The
editor listens on a Unix socket in a private per-user folder under `/tmp` (socket paths are
short and independent of `$HOME`, which agent sandboxes change); `bt-live` defines where it is,
the framing (one request per connection, one JSON line each way, a size limit, a bounded number
of connections served at once) and the request and response types, versioned. Reads see
unsaved changes; `bt edit` runs its script inside the editor process on a copy of the document's
map and lands as one transaction, waiting while a stroke or drag is in progress; it carries the
version it started from and retries on a copy of the new state when the user edited meanwhile;
`bt undo` takes it back only while the map is still in the state the edit left. Requests carry a
deadline, so a job `bt` stopped waiting for is dropped rather than applied unseen. Both sides
check that the socket folder belongs to the user, so no other process can feed an agent a false
map.

## 12. The project file (`bt-project`)

`better-tiled.json` holds what Tiled has no place for, so Tiled files stay pure:

- the AutoMap layer setup and painted state of each map (§10.3);
- the Godot setup of the project (§13.3);
- anything a later feature needs that Tiled would show as a stray property.

It is found upward from the map like git finds `.git`, stopping at the repository's top and
below the home folder; created next to the open `.tiled-project`, else next to the map. Both the
editor and `bt` read and write it through `bt-project`, which re-reads the file under a file
lock before each write, replaces only its own entries and keeps keys it doesn't know, so the two
write the same bytes and a newer version's keys survive an older one's save. Entries are keyed
by the map's path relative to the file: Save As and a move made in the editor take the entry
along, and a map whose entry is orphaned is reported, not silently started afresh. A file that
cannot be read is an error that stops the operation, never a silent fresh start.

## 13. Godot export (`bt-godot`)

### 13.1 What it writes

A map becomes a text scene (`.tscn`) in a Godot 4.3 or later project (the first Godot with
`TileMapLayer` and `Parallax2D`) with no plugin: one `TileMapLayer`
per tile layer sharing a complete `TileSet` (flips, oversized tiles placed as Tiled places them,
animations in their own atlas sources, tile properties as custom data, collision shapes as
physics, navigation and occlusion polygons, Wang sets as terrain sets); sprites, markers, areas
or bodies, paths, labels, scene instances for objects; groups and parallax as nodes; offsets,
opacity, tint and blend modes; classes and custom properties as groups and metadata. Every
orientation but oblique is placed on Godot's matching tile shape with Tiled's cell (x, y) equal
to Godot's. Property names with a meaning in Tiled's own exporter keep that meaning; the names
better-tiled adds start with `godot`. Images outside the project are copied in and tracked.

### 13.2 The regeneration contract

Generated files are build output. Each starts with comment lines naming the map it came from and
carrying a hash of the rest; Godot drops them on re-save. An export **replaces only a file it
wrote that nobody changed since**. Anything else (a scene Godot re-saved, a scene with a script
added, another map's export, a user's level scene, a file with no header) is refused, nothing at
all is written, and the refusal names the file, what would be lost and the way out. "Replace"
keeps a backup. The scene keeps its uid across exports (the first is derived from its path), node
names come from Tiled names and ids, and on Godot 4.6+ nodes get stable unique ids, so a level
scene that inherits the export keeps its own nodes and overrides when the map changes. An
unchanged map writes nothing, so Godot has nothing to re-import.

### 13.3 The front door

One exporter behind File ▸ Export to Godot, the repeat-export key, the command palette, export
on save, and `bt godot export`. A project is set up once (the Godot project folder, the levels
folder, a `TileSet` shared per grid or embedded per scene, where copied images go, export on
save, a starter level scene) and the setup is stored in `better-tiled.json`. Exports report what
they wrote, what they left out and why, and each renamed node; refusals ask their own question
in the editor and exit with their own code in `bt`. Every warning and note carries a code from
one enum, so the table of codes in the Godot doc, the JSON output and the tests are checked
against the same list; the mapping from an export's outcome to the editor's question and to
`bt`'s exit code is one table.

### 13.4 Commands

`bt godot setup | export | status | verify`: write the setup; export one map, or every generated
scene again, all or nothing; say for each map whether it is up to date, changed in Godot, moved or
refused, and what Godot will do with the level scenes; and have Godot itself, headless, import
the project and load the scene to list its errors.

## 14. Testing

Tiled has almost no tests, so better-tiled's are its correctness argument.

| Level | What | How |
|---|---|---|
| Unit | every module | inline `#[cfg(test)] mod tests` |
| Round trip | `bt-tmx` | every corpus file reads and writes back byte for byte (listed exceptions), in every format and encoding; committed fixtures for each legacy dialect |
| Oracle | `bt-terrain`, `bt-automap` | Tiled's own automapping test cases with their expected results, and Tiled's Wang test sets; Tiled's random picks are unseedable, so Wang tests assert that every choice is a valid lowest-penalty one rather than an exact tile |
| Fuzz and property | `bt-automap`, `bt-editor`, `bt-edit` | region runs equal full runs on random rule sets; random transaction sequences undo back to the original and keep dirty states right; explanations agree with real runs; flood fills agree with a plain search |
| Golden images | `bt-render` | headless renders of every orientation, blend mode and render order against committed PNGs, and against a plain CPU compositor |
| Editor flows | `apps/better-tiled` | `egui_kittest` end-to-end tests for what crosses widgets, driving the app through actions and a test handle, never through its fields: keyboard focus, dialogs, unsaved-work prompts, the live link against a running `bt` |
| Godot | `bt-godot` | exports of every example map loaded by Godot headless; picture comparison of Godot's render against ours for the real maps |
| Reports | `bt-script`, `bt-godot` | typed reports are asserted as values; the exact wording is covered by a few snapshot tests, so prose is not the test API |
| Agents | `bt` | fresh Opus agents given one brief and `bt guide`, on several kinds of maps (orthogonal, isometric, hexagonal, object-only, automapped); results checked with `bt check` and renders, never by the agent's own report |
| Performance | `bt-render`, `bt-terrain`, the editor | a frame-time benchmark on the largest map at several zooms (the budget in §6); a terrain dab under a millisecond |

Corpora: Tiled's examples and test maps, fetched into `reference/` by a script at a pinned
commit of Tiled's repository (nothing of Tiled's is committed here), and the purchased packs
(`assets/`); both folders are git-ignored and found by `bt-test-data`. A test whose data, GPU or
Godot is missing **fails**, naming what is missing; `BT_SKIP_MISSING_TEST_DATA=1` turns that
into a skip that says what it skipped. Nothing skips quietly. Continuous integration runs
format, clippy and the tests on macOS, Linux and Windows from the first commit: it fetches
`reference/` the same way, runs the renderer on each platform's software adapter, and skips the
purchased packs, which never leave this machine.

## 15. Platform and dependencies

- Rust 2024 edition, stable toolchain pinned in `rust-toolchain.toml`; macOS (Apple Silicon,
  120 Hz) is the first-class platform, Linux and Windows build and are tested in CI. The live
  link needs Unix sockets; on Windows it is off with a message.
- `eframe`/`egui` with `wgpu` for the window and canvas, `rfd` for file dialogs, `glam` for
  vectors, `roxmltree` for XML, `serde_json` for JSON, `flate2`/`ruzstd` for layer data (with a
  port of zlib's deflate for writing it), `image` for decoding, `rayon` for packing,
  `thiserror`/`anyhow` for errors, `log`.
- Dependencies are pinned at the workspace level; a crate takes only the features it uses.
  Dev builds optimise dependencies so the editor feels right while developing.

## 16. Design documents

Each area has a doc in `docs/design/`, written before the area is built (what it will do and
why, the decisions, the tests) and updated when it ships (what it does, what was left out). The
planned set:

| Doc | Area |
|---|---|
| `00-high-level-design.md` | this document |
| `01-data-model.md` | `bt-core`, `bt-utils`: types, ids, chunks, properties, Wang ids, geometry per orientation |
| `02-file-formats.md` | `bt-tmx`, `bt-image`: the raw tree, byte-identical writing, every dialect, caching, atomic writes |
| `03-rendering.md` | `bt-render`: pages, instancing, chunk caches, vector content, orientations, headless, budget |
| `04-editing-operations.md` | `bt-edit`: regions, fills, clipboard, stamps, carried selections, object and layer ops |
| `05-editor-core.md` | `bt-editor`: assets, transactions, actions, keymap, locks, recovery, session, watcher, settings |
| `06-application.md` | `apps/better-tiled`: focus, tools, canvas, panels, dialogs, modals, platform |
| `07-terrain.md` | `bt-terrain`: filler, brush, feedback, patterns, layouts, RPG Maker |
| `08-automap.md` | `bt-automap`: rule maps, engine, region runs, explain, analysis, AutoMap layers |
| `09-project-file.md` | `bt-project`: `better-tiled.json` schema and search rules |
| `10-agents.md` | `bt-script`, `apps/bt`: commands, the script language, reports, `bt guide` |
| `11-live-link.md` | `bt-live`: socket, protocol, jobs, safety |
| `12-godot-export.md` | `bt-godot`: scene mapping, contract, identity, conventions, verify |
| `13-testing.md` | corpora, oracles, golden files, agent runs, CI |

`docs/decisions.md` is a dated log: one entry per decision, with the alternatives and why.

## 17. Build order

Each milestone is usable on its own and ends with its design docs updated to "as built".

| # | Milestone | Delivers | Done when |
|---|---|---|---|
| M0 | Foundations | workspace, CI, `bt-utils`, `bt-core`, `bt-image`, `bt-tmx`, `bt-test-data` | every corpus file round-trips under the rule of `02-file-formats.md`, §12.4 (byte for byte for every file Tiled saved, apart from those Tiled itself would rewrite); `bt-tmx` reads every Tiled example and test map |
| M1 | See it | `bt-render`, `bt-editor` (assets, transactions, settings, session, recovery), the editor shell (tabs, canvas, layers and tiles panels, pan and zoom), save | a real pack's map opens, scrolls at 120 Hz at every zoom, and saves unchanged |
| M2 | Paint it | `bt-edit`; actions, keymap, focus, palette, preferences; the tile and object tools; properties and custom types; the tileset editor; clipboard; the watcher | a level is built from scratch in the window without touching Tiled |
| M3 | Rules | `bt-terrain`, `bt-automap`, `bt-project`; Terrains and AutoMap panels, Explain tool, AutoMap layers, Live, autotile layouts | a pack's `rules.txt` and terrain sets give Tiled's results, with every failure explained |
| M4 | Agents | `bt-script`, `apps/bt`, `bt-live`, the editor's socket server, `bt guide` | an agent builds a scene on each kind of example map and in the open editor; `bt check` is clean |
| M5 | Godot | `bt-godot`, the setup sheet, export on save, `bt godot` | every example map exports and loads in Godot headless; a re-export over a changed scene is refused |
| M6 | Breadth | worlds and the World tool, templates, stamps, image layers, export as image, project commands, `.kms` import, the Issues panel's last sources | every Tiled action has an equivalent, except the non-goals |

Within a milestone the order is bottom-up by the dependency graph, tests first for ported
algorithms (the oracle before the port).

M0 was built on 2026-10-10; `01-data-model.md`, `02-file-formats.md` and the M0 parts of this
doc and `13-testing.md` are as built.

M1 was built on 2026-10-10; `03-rendering.md` and the M1 parts of `05-editor-core.md`,
`06-application.md` (their §1.1) and `13-testing.md` are as built. Building it changed two
designs, recorded in `decisions.md`: far zoom draws squares of layer runs instead of a picture
per chunk, and the macOS quit guard, reviewed, is the workspace's one `unsafe` code. The window
keeps 120 Hz at every zoom on a pack's map and on the budget map (`03-rendering.md`, §11).

M2 was built on 2026-10-10; `04-editing-operations.md` and the M2 parts of `05-editor-core.md`
and `06-application.md` (their §1.2), `01-data-model.md` (the object geometry, §13.6) and
`13-testing.md` are as built. Its measure is a window test, `a_level_is_built_from_scratch_in_the_window`:
a new map, a tileset made from an image, painting and filling, an object layer with a rectangle,
a collision shape drawn in the tileset editor, and Save As, the file read back as a Tiled map
holding all of it. Building it changed designs recorded in `decisions.md`, among them a map's
tilesets edited in place of the map, Tiled's modifiers with ⌘ for its Ctrl, one Insert Polygon
tool, and carried tiles held in the selection.

M3 was built on 2026-10-10; `07-terrain.md`, `08-automap.md` and `09-project-file.md` and the M3
parts of `05-editor-core.md` and `06-application.md` (their §1.3) and `13-testing.md` are as
built. Its measure is a window test on the Highlands pack,
`a_packs_rules_and_terrain_sets_give_tileds_results_with_every_failure_explained`: the pack's
rules do nothing on its own sample scene and the window says why (they read a layer the scene
leaves empty), Live AutoMap bound to the layer the scene paints on rebuilds its platforms as
the pack draws them, and a terrain set of the pack paints with every tile meeting its
neighbours; Tiled's own automapping cases give Tiled's results exactly. Building it changed
designs recorded in `decisions.md`, among them the editor core running the terrain solver,
AutoMap layers' runs as the engine's layer-name bindings, `bt-project` below `bt-automap` in the
crate layers, and two autotile layouts the packs draw every terrain in.

## 18. Glossary

- **Asset**: a loaded map, tileset, template, world or project, with a path and a dirty state.
- **Document**: a tab onto an asset, with its selection, camera and history.
- **Transaction**: one undoable change, possibly to several assets at once.
- **Intent / effect**: what a widget or command asks the editor to do; what the editor asks the
  window to do in return.
- **Cell**: one position of a tile layer; holds a tileset slot, a tile id and flips, or nothing.
- **Chunk**: a 16×16 block of cells, the unit of sharing, caching and dirty tracking.
- **Terrain, Wang set**: Tiled's names for the colour labels on tile corners and edges that let
  a brush pick transitions automatically; a *lane* is one of the eight corner or edge slots.
- **Rule map, `rules.txt`**: Tiled's automapping, written as maps whose layers describe what to
  match and what to write.
- **AutoMap layer**: a map layer the rules write into, kept apart from what was painted.
- **Painted / made**: what the user drew, and what the rules derived from it.
- **Carried selection**: a tile selection that moves, flips and turns its tiles with it.
- **Sprite**: a tile object; its *feet* are where its image touches the ground.
- **Live link**: the socket through which `bt` works with a map open in the editor.
- **Regeneration contract**: the rule that a generated Godot file is replaced only when
  unchanged since it was written.
- **Level scene**: a scene of the user's that inherits or instances the exported scene.
