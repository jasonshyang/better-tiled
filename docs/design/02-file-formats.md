# File formats — `bt-tmx` and `bt-image`

*2026-10-09; as built at the end of M0, 2026-10-10. The design of the crate that reads and
writes Tiled's files and the crate that reads image files. The high-level design
(`00-high-level-design.md`, §5) states the goal: every file Tiled 1.0 and later writes opens, a
saved file is byte for byte what Tiled would write, and nothing is dropped silently. This doc
says how. Tiled's own sources are cited as `reference/tiled/src/...`; its format documentation
is `reference/tiled/docs/reference/`.*

## 1. Scope and files

| File | Kind | Syntax | Read | Written |
|---|---|---|---|---|
| `.tmx` (`.xml`) | map | XML | yes | yes |
| `.tsx` (`.xml`) | tileset | XML | yes | yes |
| `.tx` (`.xml`) | object template | XML | yes | yes |
| `.tmj`, `.json` | map | JSON | yes | yes |
| `.tsj`, `.json` | tileset | JSON | yes | yes |
| `.tj`, `.json` | template | JSON | yes | yes |
| `.js` | map wrapped in a JavaScript call | JSON inside | yes | yes (export) |
| `.world` | world | JSON (Qt layout) | yes | yes |
| `.tiled-project` | project with custom types | JSON (Qt layout) | yes | yes |
| `objecttypes.xml` / `.json` | legacy object types | XML or JSON | yes | no (imported into custom types) |
| `.stamp` | a saved brush with variations | JSON with embedded maps | yes | yes |
| `rules.txt` | automapping rule list | text | yes | no |
| map fragment | tiles or objects on the clipboard | TMX text | yes | yes |
| `:/automap-tiles.tsx` | Tiled's built-in rule tileset | built into the binary | yes | referenced as written |

`bt-image` reads PNG, JPEG, GIF, BMP, WebP, TGA, TIFF, PNM and ICO (the formats Tiled reads
through Qt), probes sizes from headers, and renders maps used as images through a hook.

## 2. The contract

**Reading.** Any map, tileset or template that any Tiled since 1.0 wrote, in either syntax,
with any encoding, and every legacy shape listed in §9, loads. The reader is lenient where Tiled
is lenient and in a few places where Tiled is not (§9.3). A file fails to load only when it
cannot be read, is not well-formed XML or JSON, or is not the kind of file asked for. Every
other problem becomes a typed warning next to the loaded value, and as much of the file as
possible is kept: a missing tileset is a placeholder that keeps its reference and its gid
range, an unknown gid is an empty cell, an invalid attribute keeps its default.

**Writing.** A file last saved by Tiled 1.0.1 or later comes back byte for byte when the data
did not change, apart from a short list of files that even Tiled would rewrite (§12.4). Older
files are written in today's format. Writing never reads from disk; everything it needs is in
the values it is given. Every write is atomic and refuses read-only targets with a typed reason.

**Determinism.** The same values written to the same path give the same bytes: properties and
JSON keys are sorted, chunks and Wang tiles are ordered, and nothing depends on the time or the
machine. The one exception, zstd layer data, is called out in §8.4.

## 3. Architecture

```
            XML text ──► xml::read ──┐                        ┌──► xml::print ──► XML text
                                     ▼                        │
                                 raw tree ──► finish ──► model ──► unfinish ──► raw tree
                                     ▲                        │
           JSON text ──► json::read ─┘                        └──► json::print ──► JSON text
```

- **Syntax layers** (`xml::read`, `json::read`) know only their syntax: elements and
  attributes through `roxmltree`, objects and keys through `serde_json::Value`. They fill the
  **raw tree**: `RawMap`, `RawTileset`, `RawLayer`, `RawObject`, `RawProperty` and so on, whose
  fields are `Option`s and strings exactly as the file states them. Each syntax parser also
  recognises the legacy shapes of its syntax (JSON 1.0 tile maps, say) and expresses them in
  the raw tree, so the raw tree is the union of everything either syntax can say.
- **`finish`** turns a raw tree into the model once: defaults, legacy conversions (§9),
  reference resolution through the `Loader`, gid mapping, validation and warnings. There is one
  place for every default and every conversion.
- **`unfinish`** turns the model into a raw tree with every value present, and each **printer**
  (`xml::print`, `json::print`, `json::qt`) decides what its syntax omits and how it is laid
  out, following Tiled's writers attribute by attribute.

The raw tree is `pub(crate)`. The crate's public surface is the `Loader`, the write and
to-string functions, the warning and error types, `Format`, and the readers and writers of the
other file kinds.

## 4. Reading

### 4.1 The loader

```rust
let mut loader = Loader::builder()
    .custom_types(types)        // Arc<CustomTypes>; the project's, for JSON class values
    .automap_tiles(tileset)     // what `:/automap-tiles.tsx` stands for; the built-in one by default
    .build();
let Loaded { value: map, warnings } = loader.load_map(path)?;
```

| Loader method | What |
|---|---|
| `load_map(path)`, `parse_map(text, path)` | a map with its external tilesets and templates; `path` is where the map is considered to be, so relative references resolve against its directory |
| `load_tileset(path)`, `parse_tileset(text, path)` | a tileset, cached (§4.2) when loaded |
| `load_template(path)`, `parse_template(text, path)` | a template, cached when loaded |
| `load_world(path)`, `parse_world(text, path)` | a world, its properties decoded with the loader's custom types |
| `load_stamp(path)`, `parse_stamp(text, path)` | a saved brush with the tilesets its variations use |
| `parse_fragment(text, path)` | tiles or objects from clipboard text found anywhere (§13.5) |
| `insert_tileset(path, tileset)` | use this value for the file at `path` until the file changes (an unsaved version) |
| `invalidate(path)`, `clear_cache()` | forget a cached file, or all |

| Free function | What |
|---|---|
| `load_project(path)`, `parse_project(text, path)` | a project with its custom types (it brings its own, so no loader); a legacy `objectTypesFile` is imported and cleared |
| `load_object_types(path)`, `parse_object_types(text, path)` | legacy object types as classes usable for objects and tiles |
| `parse_rules_file(path)` | the rule maps a `rules.txt` lists (§13.4) |
| `resolve_world_maps(world, world_path)` | a world's maps, listed and found by its patterns (§13.1) |
| `fragment_text(map, loader)` | a map as clipboard text (§13.5) |
| `rebase_file_references(map, from_dir, to_dir)` | Save As (§12.3) |
| `builtin_automap_tiles()` | the built-in rule tileset (§10) |

Every loaded value comes back as `Loaded<T> { value: T, warnings: Vec<Warning> }`. The loader
holds no process-global state; two loaders with different custom types coexist.

### 4.2 Caching and sharing

External tilesets and templates are cached by canonical path with the file's modification
time. A map loaded through a loader shares the cached `Arc<Tileset>` with every other map
loaded through it, which is what lets the editor treat a tileset as one thing across tabs. A
lookup compares the modification time and reads the file again when it changed. A tileset file
whose root refers to another file (`<tileset source="…">`) stands for that file, as in Tiled;
a chain that reaches itself is an error. `insert_tileset` registers an unsaved value so that
`bt` reading a map through the live link sees the editor's tileset edits.

### 4.3 Format detection

The format is read from the contents first: a first non-blank character of `<` is XML, `{` is
JSON, and text with a later line starting with `{` is a JavaScript-wrapped map. Only when the
contents decide nothing does the extension decide (`.tmx/.tsx/.tx/.xml` XML;
`.tmj/.tsj/.tj/.json` JSON; `.js` JavaScript), and a file with neither is read as XML. So a
`.tmx` map may reference a `.tsj` tileset and a `.json` file is opened without parsing it twice.
A JSON file's `type` key, when present, must name the kind asked for; files from before Tiled
1.0 have no `type`.

### 4.4 Text

Files are UTF-8 with or without a byte order mark, or UTF-16 with one. The line ending of the
first line is remembered in the value's `FileFormat` and used when it is written back. A
`DOCTYPE` (written by Tiled before 1.0) is accepted. Values are parsed the way a forgiving
reader should: integers accept surrounding whitespace, a leading `+`, and float notation
(`32.0`, `1e+06`); reals accept exponent notation (Tiled writes six significant digits, so
`1.23457e+06` occurs); booleans accept `0`/`1` and `true`/`false`; gids accept the signed form
some tools write; colours accept `#rrggbb`, `#aarrggbb` and the forms without `#`. An
attribute that is present but unparsable keeps its default and produces a warning naming the
element and value.

### 4.5 References and paths

A reference written in a file resolves against the directory of the file it is written in:
backslashes become `/`, `file://` is stripped, an absolute path or a Windows drive path is
taken as it is, and a verbatim reference (`:/…`, `qrc:`, `ext:`, `http:` and other schemes of
two or more letters) is kept unresolved. The resolved path is made absolute and lexically
cleaned (`.` and `..` folded); symbolic links are not resolved, so the path a user sees is the
one they typed. Every reference keeps its original text next to its resolved path (`FileRef`,
`ImageRef`, `TilesetRef`, `TemplateRef`), and the writer puts the original back when it still
points at the same file (§12.3).

### 4.6 Placeholders

A missing external tileset becomes a placeholder tileset named after the file's stem, with the
reference kept, registered in the gid map so that the cells using it keep their tile ids and
the file round-trips; a warning names the file. A missing template leaves the instance with
its own fields and the reference kept. A missing image keeps its reference and declared size;
the renderer draws a marker.

## 5. Warnings and errors

```
Error = Io { path, source } | Write { path, source } | Xml { path, source } | Json { path, source }
      | Format { path, message }           not the kind of file asked for, or unreadable text
Warning { file: PathBuf, kind: WarningKind, message: String, target: Option<Target> }
```

`WarningKind` names what went wrong so that the Issues panel and `bt` can group and act:
`MissingFile`, `InvalidValue`, `UnknownGid`, `CorruptLayerData`, `DuplicateId`, `UnknownKey`,
`LegacyShape`, `InvalidWangId`, `InvalidPattern`, `UnknownPropertyType`, `CircularClass`,
`Renumbered`. `Target` points at what is concerned when there is one: `Layer(LayerId)`,
`Object(ObjectId)`, `Tileset(name)` or `Tile { tileset: name, tile }`. Warnings are deduplicated
(a cached tileset used twice reports once) and carry the file's name, so a map's warnings tell
which of its files each one came from. The write error that refuses a read-only file carries
`Refused` from `bt-utils` and is asked for with `Error::refused()`.

## 6. Global tile ids

Files refer to tiles by gid: 28 bits of index into a map-wide space where each tileset owns
the range starting at its `firstgid`, and four flag bits:

| Bit | Mask | Meaning |
|---|---|---|
| 31 | `0x8000_0000` | flipped horizontally |
| 30 | `0x4000_0000` | flipped vertically |
| 29 | `0x2000_0000` | flipped anti-diagonally; 60° turn on hexagonal maps |
| 28 | `0x1000_0000` | 120° turn on hexagonal maps; masked on every orientation |

`gid::split` and `gid::join` convert between a raw gid and a `SplitGid { gid, flip }`. Reading
uses a `GidMapper` of `firstgid → TilesetSlot`: the tileset with the largest `firstgid ≤ gid`
owns the gid, the local id is the difference, gid 0 is the empty cell (flags on 0 are ignored),
and a gid past the end of a tileset is attributed to it anyway, as Tiled does, so references to
tiles of a tileset whose image is missing survive. A gid below the first `firstgid`, or any gid
when the file declares no tilesets, is an empty cell with one aggregated `UnknownGid` warning
per layer. Gids exist nowhere outside this crate.

## 7. Properties

### 7.1 Value forms

| Type | XML `value` | JSON `value` |
|---|---|---|
| string | attribute, or element text when the string has a line break | string |
| int | decimal | number |
| float | shortest round-trip form (Qt's `QVariant::toString`) | number |
| bool | `true`/`false` | bool |
| color | `#aarrggbb` or `""` | the same string |
| file | path relative to the containing file | the same |
| object | the id (0 = none) | number |
| class | nested `<properties>` of the set members only, each with its own type | object of raw member values |
| enum | the name, comma-joined names for flags, or the number (per storage type) | the same |
| list | `<item>` children, each typed like a property without a name | array of `{type, propertytype?, value}` |

A property's `type` attribute is absent for strings in XML and present in JSON; a custom type
is named by `propertytype` (lower-case in maps, tilesets, worlds and project properties,
camel-case `propertyType` inside the project's type definitions, both accepted). JSON class
values carry no member types, so decoding them needs the project's custom types: the
`PropertyDecoder` gives each member the type of the class's member of that name (a file member
becomes a path, an object member an id). A member the class lacks, or every member when the
loader has no such class, is typed after its JSON form: text a string, a number an int or a
float.

### 7.2 What is kept

The model stores values in their file form (`01-data-model.md`, §9), so the loader converts
nothing it does not have to. After reading, `CustomTypes::normalize_all` brings enum values and
class members into their types' storage form, as Tiled does when its project is loaded. A
`propertytype` naming a type the project lacks is **kept**; Tiled drops the link on the next
save, which is the one place its format loses data, and there is no reason to copy that.

## 8. Tile layer data

### 8.1 Encodings

| `encoding` | `compression` | `LayerDataFormat` | Payload |
|---|---|---|---|
| absent | — | `Xml` | `<tile gid="…"/>` per cell, row-major; read only |
| `csv` | — | `Csv` | decimal gids, comma-separated |
| `base64` | absent | `Base64` | little-endian `u32` per cell |
| `base64` | `zlib` | `Base64Zlib` | a zlib stream |
| `base64` | `gzip` | `Base64Gzip` | a gzip member |
| `base64` | `zstd` | `Base64Zstd` | a zstd frame |

A map has one layer data format; the reader takes the last layer's, as Tiled does, and the
writer uses it for every layer. The XML `<tile>` form is read and never written; a map that
used it is written as CSV. Infinite maps carry `<chunk x y width height>` elements (JSON:
`chunks` with `startx`/`starty`) whose contents use the same encoding; a finite map that
contains chunks is read too, and cells outside its bounds are dropped with a warning.

### 8.2 Decoding

Decoding is lenient and bounded: CSV skips whitespace anywhere, treats an empty field as 0 and
accepts a trailing comma (Tiled refuses it); base64 ignores whitespace; zlib and gzip are
detected from the header whatever the attribute says, as Tiled's `inflateInit2(15 + 32)` does;
a decompressed payload must be exactly `width × height × 4` bytes and decompression is capped at
that size, so a corrupt stream cannot allocate without limit. Whatever could be read is kept
and the problem is a `CorruptLayerData` warning with the cell where it happened.

### 8.3 Encoding

CSV is written as Tiled writes it: a line break, then each row's values followed by commas
except the very last value, each row ending in a line break, and `</data>` at column zero.
Base64 is written with Tiled's fixed whitespace (`"\n   "` before, `"\n  "` after, regardless
of nesting). Chunks are aligned to the map's output chunk size with floor division for negative
positions, only chunks with a tile are written, sorted by row then column. "Minimize output"
drops the whitespace.

### 8.4 Compression

zlib and gzip streams must be the bytes zlib produces for Tiled's call
(`deflateInit2(level, Z_DEFLATED, 15 (+16), 8, Z_DEFAULT_STRATEGY)` followed by one
`deflate(Z_FINISH)`), or every save of an unchanged map would rewrite all its layer data. The
`deflate` module is a port of the parts of zlib's `deflate.c` and `trees.c` that call uses:
levels 1–9, the default strategy, a 32 K window, `memLevel` 8, all input at hand. As in Tiled,
level `-1` is zlib's default (6) and every other level is clamped to 1–9. It is the one place
in the workspace that reproduces another program's bytes rather than its behaviour, and it
carries zlib's notice. It is also the one module allowed to index slices: it keeps zlib's array
arithmetic, whose indices zlib's invariants bound. 800 vectors generated with zlib 1.2.12
(`scripts/deflate-vectors.py`) pin it: every level, both wrappers, inputs up to 200,000 bytes.

A gzip member names no file and no time and gives Unix (3) as its system, as Tiled's Linux
builds write it. Tiled's macOS and Windows builds write their own system codes in that byte, so
a gzip map they saved comes back with that one byte changed.

zstd is decoded with `ruzstd` and encoded at its fastest level whatever the map's compression
level says, because no pure-Rust encoder offers zlib-style levels. A zstd map therefore saves
with different (valid) bytes than Tiled would write, and its data is rewritten on every save.
This is the contract's one deliberate exception, documented in the crate and tested as such.

## 9. Dialects

Tiled never reads the `version` it writes; every reader is a feature sniffer, and so is this
one. A file declaring `version="1.10"` may contain 1.12 features. Every shape below is detected
structurally in `finish`, converted once, and reported as a `LegacyShape` warning when the
conversion changes what a save will write.

### 9.1 Read and converted

| Shape | Tiled versions | Read as |
|---|---|---|
| `type` attribute for a class on tiles and objects | before 1.9, and 1.10+ again | `class`; `class` wins when both exist |
| `<tile gid>` XML layer data | ≤ 0.15 | cells; written as CSV |
| gzip layer data | all (deprecated for new maps) | read and written back as gzip |
| `DOCTYPE` | < 1.0 | accepted and dropped |
| layer `x`/`y` | never written by Tiled | ignored with a warning |
| image layer `x`/`y` | < 0.15 | the layer's offset |
| tile objects without `width`/`height` | < 0.12 | the tile's size |
| `<terraintypes>` and tile `terrain="tl,tr,bl,br"` | ≤ 1.4 | a corner Wang set named `Terrains`, one colour per terrain |
| hexadecimal `wangid` (`0x…`, four bits a lane) | ≤ 1.4 | eight lanes |
| `<wangcornercolor>` / `<wangedgecolor>` (JSON `cornercolors` / `edgecolors`) | 1.1–1.4 | one colour list; the set's kind inferred when only one kind existed |
| properties on a tile's collision `<objectgroup>` | < 1.1 | moved onto the tile |
| `animation-frameN` / `animation-delayN` tile properties (The Mana World) | old | frames with `duration = delay × 10`; the properties are kept, as Tiled keeps them |
| JSON `properties` as an object plus `propertytypes` | JSON 1.0/1.1 | typed properties |
| JSON `tiles` as an object keyed by id, `tileproperties`, `tilepropertytypes` | JSON 1.0/1.1 | the tile list; triggers Tiled's "reset tile order" |
| JSON numeric `version`, missing `type` | ≤ 1.5 | accepted |
| `.js` wrapper `(function(name,data){…})("name",\n{…});` | all | the JSON from the first line starting with `{` |
| `type="std::nullptr_t"` on a list property | 1.12.0–1.12.2 | a list; the value is kept |
| negative gids, integers written as floats | other tools | parsed |
| export target `"."` | TSX files | no target |
| `objecttypes.xml` / `.json`, project `objectTypesFile` | < 1.8 | classes for objects and tiles, merged into the project's types once |
| commands with capitalised keys | < 1.4 projects | commands |

### 9.2 Read but not written

The XML `<tile>` layer form, hexadecimal Wang ids, separate corner and edge colours, terrain
types, JSON 1.0/1.1 property and tile maps, the `type` spelling for classes when the
compatibility version is 1.9, and `objecttypes` files are read only. Everything else that is
read is also written, in today's form.

### 9.3 Deliberate differences from Tiled's reader

- A JSON layer without `visible` or `opacity` is visible and opaque, as the documentation
  says; Tiled loads it invisible (`varianttomapconverter.cpp:607`), which is a bug.
- `next_layer_id` and `next_object_id` not above an id in use are raised, with a warning when
  the file stated them.
- A trailing comma in CSV data is accepted.
- A `propertytype` naming an unknown type is kept (§7.2).
- A world pattern that does not compile is kept with a warning, so saving the world does not
  drop it; Tiled drops it.
- Unknown elements, attributes and keys produce an `UnknownKey` warning instead of a debug
  message, so a file written by a newer Tiled says what it carries that will be lost.

## 10. Images and tilesets

An atlas tileset's tiles are the grid cut from its image: `for y = margin; y + tile_height <=
image_height; y += tile_height + spacing` and the same for x. The loader reads the image's
size from its header through `bt-image` (never decoding it) and counts `columns` and
`tile_count` from the real image, as Tiled does; the declared `<image width height>` stays in
the `ImageRef` as the size the file was written for, which is what "the image changed size"
compares against. A missing image keeps the declared size, so the tile count is still known.
Collection tiles ignore a per-tile `trans` and declared size, as Tiled does. An image the file
gives no size gets the size its header states.

The writer writes the declared size and counts `columns` and `tilecount` from it, so a tileset
whose image changed size is written as it was read until the editor records the new size
(`05-editor-core.md`). An atlas tile inside the declared grid holds an entry only when it has
data: the loader drops empty entries there (a tile that had only terrain corners, say).

Embedded images (`<image format="png"><data encoding="base64">…`) are kept as bytes in the
model and decoded by content hash. Nothing is written to a temporary folder while loading.

The built-in rule tileset `:/automap-tiles.tsx` (five tiles: Negate, Ignore, NonEmpty, Empty,
Other, each with a `MatchType` property) is a value the loader is built with
(`builtin_automap_tiles()` by default): its definition and its image are compiled into the
binary, and a map that references it writes the reference back verbatim. The image is drawn
for this project (`scripts/automap-tiles-png.py` writes `automap-tiles.png`), not copied from
Tiled's editor, whose sources are GPL.

## 11. Templates

A template instance in a map is written with only what it overrides (`01-data-model.md`,
§8). The loader gives an instance the template's values for every other field and records the
overrides it read: `name` and `size` when present and non-empty, `rotation`, `opacity` and
`visible` when the key exists, `tile` when `gid` is non-zero, the shape when a shape element
exists, the text flags when `<text>` exists. The writer states a field when its override flag
is set or it differs from the template held by the loader passed with `WriteRequest::loader`;
it reads no file, and without a loader the flags alone decide. An instance's own properties
are those that differ from the template's. A template's own tileset uses `firstgid="1"` in a
gid space of its own.

## 12. Writing

### 12.1 Requests

```rust
WriteRequest::for_map(&map, path)         // or for_tileset, for_template, for_world, for_project, for_stamp
    .format(Format::Json)                 // default: from the extension, else XML
    .loader(&loader)                      // the templates instances differ from, the custom types
    .export(ExportOptions { embed_tilesets, detach_templates, resolve_types_and_properties, minimize })
    .safe_writing(false)                  // default: true
    .compatibility_version(version)       // the `version` a new file gets; latest by default
    .write()?;                            // -> Vec<Warning>, e.g. a renumbered firstgid
```

The same request renders to text without writing (`render() -> Rendered { text, warnings }`)
for the live link and tests; `fragment_text(map, loader)` renders a map as clipboard text
(§13.5). Export options apply to a clone, as Tiled's `ExportHelper` does: external
tilesets cloned in and their image paths re-relativised; template instances detached with
their template's class and properties baked in and the template's tileset added; classes and
tiles resolved into every object's and element's properties; minimized output. A plain save
applies none of them.

### 12.2 Byte identity

The printers reproduce Tiled's writers (`reference/tiled/src/libtiled/mapwriter.cpp`,
`maptovariantconverter.cpp`, `plugins/json/qjsonparser/json.cpp`):

- **XML layout**: the declaration `<?xml version="1.0" encoding="UTF-8"?>`, one-space
  indentation, every element on its own line, text content inline with its end tag, empty
  elements as `<x/>`, `QXmlStreamWriter`'s escaping (attributes escape tab, line feed and
  carriage return numerically; characters XML forbids are dropped).
- **Attribute and element order** as Tiled's writer emits them, element by element; the
  object group's `color` and `draworder` before its `id` and `name`, an image layer's
  `<image>` before its `<properties>`, a Wang set's `<properties>` last, and so on.
- **Omission rules**: an attribute is omitted when it has its default (opacity 1, visible,
  unlocked, rotation 0, `spacing`/`margin` 0 in XML but always written in JSON, and the rest of
  Tiled's list). A collection lists every tile in display order. An atlas lists its tiles
  with data by id, unless the ids beyond its grid leave a gap (Tiled's `anyTileOutOfOrder`);
  then it lists every tile of the grid and those beyond. Wang tiles are sorted by id and
  properties by name.
- **Numbers**: integers as decimal; reals in XML as `QString::number(double)` gives them,
  `%g` with six significant digits (`146.97`, `693.727`, `1.23457e+06`, exponents with a sign
  and at least two digits); `float` custom properties as the shortest form that reads back
  exactly; JSON reals as `QString::number(d, 'g', 15)` lays them out, integral values without a
  decimal point. Colours as `#rrggbb` when opaque, else `#aarrggbb`; a tileset image's `trans`
  without `#`, JSON's `transparentcolor` with it.
- **Line endings**: LF inside the crate; the `FileFormat`'s ending applied to the whole text
  at the end.
- **`version`**: the one the file had; for a new file, the compatibility version's (`1.11`
  for the default, latest). `tiledversion`: the one the file had; for a new file, the Tiled
  version whose format this crate targets (`TILED_VERSION` in `names`, 1.12.2). The class
  spelling follows the version written (`class` for 1.9, else `type`).
- **Global ids**: each tileset keeps its `first_gid` hint when the hint is not below the end
  of the tiles the previous tileset uses; otherwise it starts after the previous tileset's
  whole range, `max(next_tile_id, tile_count, used span)` (Tiled's running sum), and a
  `Renumbered` warning names it. A tileset that only template instances take tiles from,
  without naming it, is not written.
- **JSON layout**: Tiled's `JsonWriter` with auto-formatting: nested objects open on a new line
  at four spaces per depth with keys one space further in, `"key":value` without a space,
  array items separated by `", "` and wrapped after every `width` items (the chunk width for
  infinite maps) so tile data is row-shaped, `/` escaped as `\/`, everything beyond ASCII as
  `\uXXXX` so files are ASCII, keys sorted. Minimized output has no whitespace. Reals are
  written with fifteen significant digits (`%.15g`). Projects and worlds use
  `QJsonDocument::toJson`'s layout: four-space indentation, `"key": value`, one item per line,
  raw UTF-8, a trailing newline. Stamps use its compact form.
- **CRLF, BOM, DOCTYPE** are not reproduced beyond the line ending: a file that had a byte
  order mark or a `DOCTYPE` was written before Tiled 1.0.1 and is outside the contract.

### 12.3 References

A reference is written as its original text when that text still resolves to the file's path;
otherwise as the path relative to the written file's directory with `/` separators and `..`
allowed; otherwise absolute. Verbatim references (`:/automap-tiles.tsx`, `ext:`, URLs) are
written as they are. References keep their resolved paths, so a map saved to another folder
keeps pointing at its tilesets, images and templates with nothing to do. `file` properties are
stored as the file wrote them, relative to it, so Save As rewrites them for the new folder
(`rebase_file_references(map, from_dir, to_dir)`, which covers the map's layers, objects and
embedded tilesets).

### 12.4 The round-trip rule

Every file in the corpora that records a `tiledversion` (XML `tiledversion=` or JSON
`"tiledversion":`, so every file Tiled 1.0.1 or later saved) must come back identical, except
files that Tiled itself would rewrite. The test lists each with its reason:

| File | Why Tiled would rewrite it too |
|---|---|
| the Highlands Biome Map folder (`assets/`) | written by the asset pack's own tool: properties and animations on one line |
| `examples/forest/forest.tsx`, `examples/sticker-knight/map/objs.tsx` | an `<image>` with its attributes in an order Tiled does not write |
| `examples/sticker-knight/ui/title.json` | saved by Tiled 1.8; the JSON writer of Tiled 1.12 adds every object's opacity |
| `src/tiled/resources/automap-tiles.tsx` | edited by hand: its tiles are listed out of order |

A file outside the rule must still read back to the same model, apart from what the writer
normalises (a missing `version`, renumbered gids).

### 12.5 Atomic writes

Every file goes through `bt-utils`' `write_atomically` (`01-data-model.md`, §15): a temporary
file beside the target, `fsync`, rename, permissions kept, symbolic links followed, a read-only
file or folder refused before anything is written, the temporary file removed on failure. Safe
writing can be turned off per request for folders where the rename misbehaves (synced and
network folders), and then the target is written in place. Worlds and projects are written the
same way; Tiled writes worlds in place.

## 13. The other file kinds

### 13.1 Worlds

`maps` (file name relative to the world, rectangle in pixels), `patterns` (`regexp`,
`multiplierX/Y` default 1 and never 0, `offsetX/Y` default 0, `mapWidth/Height` default
`|multiplier|`), `properties`, `onlyShowAdjacentMaps`, `gridWidth/Height`, `type`. A pattern's
expression must have exactly two capture groups (column, row); it is compiled with the `regex`
crate (Perl syntax without look-around and backreferences, which no known world uses) and
matched unanchored against the bare names of the files in the world's directory, as Tiled
matches them; `resolve_world_maps(world, world_path)` lists the maps it finds. A pattern that
does not compile, lacks the two groups or has a zero multiplier is kept with an
`InvalidPattern` warning and finds no maps. Written keys omit defaults and `maps`, `patterns`
and `properties` when empty.

### 13.2 Projects

`folders` (`.` is the project's directory), `extensionsPath` (default `extensions`),
`automappingRulesFile`, `commands`, `propertyTypes`, `properties`, `compatibilityVersion`
(1080, 1090, 1100; absent is latest; written only when not latest) and the legacy
`objectTypesFile`. Custom types are read in two passes: every type is created with placeholder
member values, then classes' member values are decoded against the full set in dependency
order. A member that would make a class contain itself is dropped with a `CircularClass`
warning; of a cycle, the member Tiled finds last goes. A type is identified by name and
overlapping usage; `useAs` missing means property-only (files from before 1.9). Unknown keys
are kept as JSON text (`Project::unknown_keys`) and written back. `properties` and every
class's `drawFill` are always written, as Tiled 1.12 writes them.

### 13.3 Object types

`<objecttypes><objecttype name color><property name type propertytype default/>…` or the JSON
array form: each type becomes a class usable for objects and tiles with its colour and member
defaults, merged into the project's types. Read only.

### 13.4 Rules files

`rules.txt`, one entry per line after trimming: empty lines and lines starting with `#` or
`//` are ignored; `[pattern]` restricts the entries that follow to maps whose file name
matches the case-insensitive wildcard (`*`, `?`, `[…]` sets with ranges and negation),
anchored at both ends, and `[*]` lifts it; a path relative to the rules file names a rule map,
or, with a `.txt` extension, another rules file read in place with the current filter, whose
own filter changes do not leak out. Include cycles are skipped. `parse_rules_file(path)`
returns a list of `RuleMapEntry { path, filter }` with `applies_to(map_file_name)`; rule maps
are not checked for existence here. A missing or unreadable rules file is an error.

### 13.5 Stamps and fragments

A `.stamp` file is Tiled's: a name, an optional `quickStampIndex` (the number key the stamp
is on) and `variations`, each a probability and a small TMJ map with one or more tile layers,
tilesets referenced by file (an embedded tileset is written whole). `Stamp { name,
quick_stamp_index, variations: Vec<StampVariation { probability, map } }` is read by
`Loader::load_stamp` and written in `QJsonDocument`'s compact layout, as Tiled writes stamps;
the editor's brush model is built from it in `bt-edit`.

A fragment is the clipboard's text form, shared with Tiled: a map with the source map's grid
and nothing else, its tilesets, one tile layer per copied layer in CSV (so an agent can read
and write it), or one object layer, with references as absolute paths so the text can be
pasted anywhere. `fragment_text(map, loader)` writes it; `Loader::parse_fragment` reads it from
text found anywhere, Tiled or a chat window included, dropping what surrounds the map.

## 14. `bt-image`

```rust
let mut images = ImageStore::builder()
    .threads(4)                           // default: 2
    .map_images(Arc::new(map_renderer))   // impl MapImageSource; optional
    .build()?;
let key = images.request(ImageRequest { input: ImageInput::File(path), transparent });
match images.poll(&key) { Ready(image) | Pending | Failed(error) | NotRequested }
```

| Function | What |
|---|---|
| `probe_size(path)`, `probe_size_of_bytes(bytes, format)` | the size from the image's header, without decoding |
| `decode_file(path, transparent)`, `decode_bytes(bytes, format, transparent)` | decoding on the calling thread |
| `request(ImageRequest) -> ImageKey`, `poll(&key) -> ImagePoll` | decoding on the store's worker threads (`std` threads sharing one job queue); `set_waker` sets what a worker calls when an image lands, so a window can repaint |
| `decode_now(ImageRequest)` | the image now, decoded on the calling thread when the store lacks it: for headless pictures and tests |
| `ImageInput::File(path)`, `ImageInput::Bytes { bytes, format }` | a file keyed by path and modification time, or embedded bytes keyed by content hash; `format` is the format the file names, used only when the bytes do not say (TGA); the colour key is part of the key because it changes the pixels |
| `DecodedImage { width, height, rgba: Vec<u8> }` | straight alpha; premultiplication is the renderer's business |
| `alpha_bounds(image, threshold) -> Option<IRect>` | the smallest rectangle of pixels with alpha above a threshold |
| `bottom_center(image, threshold) -> Option<DVec2>` | the middle of the lowest row with alpha above the threshold: where a sprite's feet are |
| `MapImageSource` | `fn render_map(&self, path, nested: &NestedImages) -> Result<DecodedImage, Error>`: a map file used as the image of a tileset, a tile or an image layer; the map's own images are decoded through `nested`, which refuses a map that would show itself |
| `trim(keep_bytes)`, `byte_size()` | eviction under a byte budget, least recently used first; the memory the decoded pixels take |

Errors are typed: `Error = Io | Decode | Unsupported | Recursive | NoMapSource | Stopped`, each
naming its file or "an embedded image". The `image` crate is used with only the decoders listed
in §1 enabled. `bt-tmx` uses `bt-image` only to probe sizes while loading; it never decodes
pixels.

## 15. Module layout

```
crates/bt-tmx/src/
├── lib.rs           mod lines and pub use
├── loader.rs        Loader, LoaderBuilder, Loaded<T>, the cache
├── write.rs         WriteRequest, Rendered
├── error.rs         Error
├── warning.rs       Warning, WarningKind, Target
├── format.rs        Format, detection, the JavaScript wrapper
├── text.rs          decoding, byte order marks, line endings
├── paths.rs         reference resolution and the written form of references
├── gid.rs           SplitGid, split and join, GidMapper
├── names.rs         every enumerated value's spelling, both directions; TILED_VERSION
├── raw/             the raw tree: map, tileset, tile, wang, layer, object, template, image, property, scalar
├── xml/             read.rs (roxmltree → raw), print.rs (raw → text), writer.rs (the QXmlStreamWriter emulation)
├── json/            read.rs (serde_json → raw), variant.rs (raw → JSON values, as maptovariantconverter),
│                    print.rs (Tiled's JsonWriter), qt.rs (QJsonDocument's layouts)
├── finish/          context, map (ids, gids and Map::validate's violations as warnings), tileset, wang,
│                    layer, object, template, property
├── unfinish/        map, tileset, layer, object, property, gids (first gids for writing)
├── data.rs          CSV and base64 payloads
├── deflate.rs       the zlib port (levels 1–9, zlib and gzip wrappers), with deflate_vectors.in
├── numbers.rs       Qt's number formatting (six digits, shortest, fifteen digits) and lenient parsing
├── export.rs        ExportOptions applied to a clone
├── rebase.rs        file properties rewritten for another folder
├── world.rs, project.rs, object_types.rs, stamp.rs, rules.rs, fragment.rs
└── automap_tiles.rs the built-in rule tileset, with automap-tiles.png
```

Legacy shapes are converted where their element is finished (terrains in `finish/wang.rs`,
image layer positions in `finish/layer.rs`), not in a module of their own. `bt-image` is
`store.rs` (the store and its workers), `decode.rs`, `probe.rs`, `measure.rs` (bounds and
feet), `key.rs`, `error.rs`.

## 16. Tests

| Test | What it pins |
|---|---|
| Inline unit tests | detection, text decoding, lenient parsing, path resolution, gid mapping, CSV and base64 decoding, number formatting (vectors from Qt: `0.39` → `0.39`, `999999.5` → `1e+06`, `2/3` → `0.666667`), the XML writer against `QXmlStreamWriter` samples, the JSON printers against Tiled's and `QJsonDocument`'s layouts, worlds, projects (Tiled's example project written back), stamps, fragments, rules files |
| Dialect fixtures (`tests/dialects/`) | 21 small files written by hand for the rows of §9.1, each with the warnings it must produce and the text it must write back (`.written.*` goldens; `BT_UPDATE_GOLDENS=1` rewrites them) |
| Corpus reads (`tests/reads.rs`) | every map, tileset, template, world and project under `reference/` and `assets/` (through `bt-test-data`) reads without an error |
| Corpus round trip (`tests/round_trip.rs`) | the same files written back under the rule of §12.4, with the first differing line in the failure message |
| Cross-syntax (`tests/cross_syntax.rs`) | every corpus map written in the other syntax and read back equals the original, after JSON maps are put into XML's precision |
| Writer → reader (`tests/writer_reader.rs`) | a map built in memory with every feature survives 100 combinations: five orientations, five layer data formats, finite and infinite, XML and JSON |
| Mutation (`tests/mutation.rs`) | corpus files with bytes and values changed at random never panic, and at least a fifth of them still read; `BT_FUZZ_SEED` and `BT_FUZZ_CASES` choose the run |
| Deflate | the 800 committed zlib vectors (§8.4), Tiled's level clamping, and round trips of random inputs at every level |
| Fuzzing (`fuzz/`) | libFuzzer targets for XML maps, JSON maps, tilesets, layer data, and projects and worlds, run for ten minutes each a week by `.github/workflows/fuzz.yml`: no panic, bounded memory |
| `bt-image` | every supported format decodes; sizes probed equal decoded sizes (Tiled's example images included); colour keys; alpha bounds and feet; the store's workers, waker, failures, eviction and map images, recursion included |

CI runs all of them with `reference/` fetched at the pinned Tiled commit and the dialect
fixtures, and skips, loudly, the corpus files under `assets/`.
