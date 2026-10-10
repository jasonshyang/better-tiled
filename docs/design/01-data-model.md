# Data model — `bt-core` and `bt-utils`

*2026-10-09; as built at the end of M0, 2026-10-10, with M2's object geometry (§13.6). The design of the two bottom crates: the
primitives every crate shares and the pure data model of maps, tilesets, objects, properties,
terrain sets, worlds and projects, with the grid geometry of every Tiled orientation. The
high-level design (`00-high-level-design.md`, §3.5 and §4) says why these crates exist; this doc
says what they contain and the rules that hold inside them. Tiled's own sources are cited as
`reference/tiled/src/...`.*

## 1. Scope

`bt-core` is data and arithmetic on data. It contains no pixels, no file access, no undo, no
widget types and no process-global state. Importers build its values, the renderer and the
exports read them, the editing crates mutate them, and undo is built on cloning them.

`bt-utils` sits below it with the primitives that several crates would otherwise copy. It
depends on nothing of ours; `bt-core` depends on `bt-utils` and `glam`.

What belongs elsewhere, so that it is never asked of these crates:

| Concern | Crate |
|---|---|
| Reading and writing files, resolving references on disk, global tile ids | `bt-tmx` |
| Decoded image pixels, image sizes read from files | `bt-image` |
| Editing operations (fills, clipboard, resizing, object commands) | `bt-edit` |
| Terrain filling, automapping | `bt-terrain`, `bt-automap` |
| History, selection, dirty state | `bt-editor` |

## 2. Conventions

### 2.1 Coordinate spaces

Three spaces appear throughout the model and every crate above it. They are the spaces Tiled's
renderers use (`reference/tiled/src/libtiled/maprenderer.h`), so Tiled's files mean the same
thing here.

| Space | Unit | What lives in it | Type |
|---|---|---|---|
| **Tile** | cells | cell positions; fractional positions inside a cell | `IVec2`, `DVec2` |
| **Pixel** | map pixels | object positions, sizes and polygon points as the files store them | `DVec2` |
| **Screen** | map pixels as drawn | where everything is drawn; the camera, layer offsets, pictures | `DVec2` |

On orthogonal, staggered and hexagonal maps pixel and screen space coincide. On isometric maps
pixel space is the unprojected grid of `tile_height × tile_height` squares and screen space is
its projection; on oblique maps pixel space is unsheared. `MapGeometry` (§13) converts between
all three. The screen origin is the top-left of the map's bounding rectangle for finite maps.

Layer offsets, parallax and tile offsets apply in screen space. A chunk position is a tile
position divided by the chunk size with floor division, so negative tile positions of infinite
maps fall into negative chunks.

### 2.2 Numbers

- Tile positions and sizes are `i32` (`IVec2`); a map is never wider than `i32` allows.
- Everything Tiled stores as a real number is `f64` here: object geometry, opacity, parallax,
  probability, tint. An `f32` would lose digits that Tiled's JSON writer preserves, and a saved
  file must come back byte for byte.
- Ids and counts are `u32`. Id newtypes (§2.3) never convert to or from each other implicitly.
- Degrees for rotation, clockwise, as Tiled stores them.

### 2.3 Identity

| Newtype | What it names | Scope of uniqueness |
|---|---|---|
| `LayerId(u32)` | a layer in a map | the map; ids are never reused (`Map::next_layer_id`) |
| `ObjectId(u32)` | an object in a map, or a collision shape in a tile | the map, or the tile |
| `TileId(u32)` | a tile in a tileset (Tiled's "local id") | the tileset |
| `TilesetSlot` | a tileset's index in the map's tileset list (`TilesetSlot::new(index) -> Option`, below 65535) | the map |
| `CustomTypeId(u32)` | a class or enum in a project | the project |

Tilesets and templates are reached by path (external) or held by value (embedded); maps refer to
them through `TilesetRef` and a template's `FileRef`, never by pointer identity alone. Parent
links (layer to group, object to layer) are not stored; `Map` answers them.

### 2.4 Strings, names, colours

- Strings are UTF-8 `String`s; a class is a `String` where empty means "none", as in Tiled.
- `Color` is straight-alpha RGBA8 with Tiled's `#rrggbb` / `#aarrggbb` text form (alpha first)
  as its only text form, parsed leniently (with or without `#`).
- Flag sets (`Flip`, `ObjectOverrides`, `ClassUsage`) are typed with the `bitflags` crate. No
  loose integer constants.

### 2.5 Visibility

Every type is `pub` with `pub` fields where the type is a plain record (`Map`, `Layer`,
`Tileset`…): the model is data, and the crates above it build and mutate it directly. Types
with invariants (`Cell`, `Chunk`, `TileLayer`, `WangId`) keep their fields private and expose
constructors and accessors that keep the invariant. Helpers that only this crate needs are
`pub(crate)`.

## 3. Module map

One concept per module; `lib.rs` is `mod` lines and explicit `pub use` lines.

| Module | Holds |
|---|---|
| `ids` | the id newtypes of §2.3 |
| `color` | `Color` |
| `cell` | `Cell` |
| `flip` | `Flip` and its transforms |
| `chunk` | `Chunk`, `ChunkPos`, `CHUNK_SIZE` |
| `tile_layer` | `TileLayer`, `TileBounds` |
| `object_layer` | `ObjectLayer`, `DrawOrder` |
| `image_layer` | `ImageLayer` |
| `layer` | `Layer`, `LayerKind`, `BlendMode`, `EffectiveLayer` |
| `map` | `Map`, `Orientation`, `RenderOrder`, `StaggerAxis`, `StaggerIndex`, `TilesetRef` |
| `file_format` | `FileFormat`, `LayerDataFormat`, `LineEnding`, `ExportSettings` |
| `image_ref` | `ImageRef`, `ImageSource` |
| `tileset` | `Tileset`, `TilesetImage`, `Alignment`, `TileRenderSize`, `FillMode`, `TilesetGrid`, `Transformations` |
| `tile` | `TileData`, `Frame` |
| `object` | `MapObject`, `Shape`, `ObjectOverrides` |
| `text` | `TextData`, `HAlign`, `VAlign` |
| `template` | `ObjectTemplate` |
| `property` | `Properties`, `Property`, `Value` |
| `custom_types` | `CustomTypes`, `CustomType`, `ClassType`, `EnumType`, `ClassUsage`, `EnumStorage` |
| `wang` | `WangId`, `WangSet`, `WangColor`, `WangSetType` |
| `world` | `World`, `WorldMap`, `WorldPattern` |
| `project` | `Project`, `Command`, `CompatibilityVersion`, `FileRef` |
| `geometry` | `MapGeometry`, `HexParams`, `GridLine`, `DrawOrderKey` (with `geometry/hex` for cube coordinates) |
| `validate` | `Map::validate` and `Violation` |

## 4. Map

```
Map
├── orientation: Orientation          orthogonal | isometric | staggered | hexagonal | oblique
├── render_order: RenderOrder         right-down (default) | right-up | left-down | left-up
├── width, height: u32                in tiles; the bounds of finite maps
├── tile_width, tile_height: u32
├── infinite: bool                    every tile layer's TileBounds follows it (§6.1)
├── hex_side_length: i32              hexagonal maps
├── stagger_axis, stagger_index       staggered and hexagonal maps (y / odd by default)
├── skew: IVec2                       oblique maps (Tiled 1.12)
├── parallax_origin: DVec2
├── background_color: Option<Color>
├── class: String
├── properties: Properties
├── tilesets: Vec<TilesetRef>         ordered; the order decides gids on write
├── layers: Vec<Layer>                bottom first, a tree through groups
├── next_layer_id, next_object_id: u32
└── file: FileFormat                  what a byte-identical write needs (§4.2)
```

### 4.1 Tileset references

```
TilesetRef
├── file: Option<FileRef>     the tileset's file, as written and resolved (None: embedded)
├── first_gid: Option<u32>    the firstgid the file gave this tileset, a hint for the writer
└── tileset: Arc<Tileset>
```

A map holds its tilesets as shared, immutable values. Two maps open on the same `.tsx` share
one `Arc`; editing a tileset means replacing the `Arc` in every map that uses it (the editor
core's job). `Map::slot_of(&Arc<Tileset>)` finds a tileset by identity, `Map::tileset(slot)` by
slot. The `first_gid` hint is the only trace of global ids in the model: the writer keeps it
while it is still valid, so a map whose tilesets did not change writes back the gids it had,
even when a tileset's image is missing and its tile count unknown.

### 4.2 File format details

Everything a byte-identical write needs that is not map content lives in one value, so it is
obvious what is "how the file was written" rather than "what the map is":

```
FileFormat
├── version: String             the format version as read ("1.10"); empty for new maps
├── tiled_version: String       informational, as read
├── line_ending: LineEnding     Lf | CrLf, from the file's first line
├── layer_data: LayerDataFormat Xml | Csv | Base64 | Base64Gzip | Base64Zlib | Base64Zstd
├── compression_level: Option<i32>  None = the format's default
├── output_chunk_size: UVec2    16×16 unless the file says otherwise
└── export: ExportSettings      the last export target and format (File ▸ Export repeats it)
```

Tilesets and templates carry the same value for their own files.

### 4.3 Ids

`next_layer_id` and `next_object_id` are what the files store; a new layer or object takes the
next id and bumps it, and ids are never reused. Unlike Tiled, which trusts the stored values,
loading raises each one to one past the highest id in use when it is lower, with a warning:
otherwise the next new object would get an id another object already has, and object references
would point at two things.

### 4.4 Lookups

`Map` answers the tree questions every crate asks, once, here:

| Function | Answer |
|---|---|
| `layer(id)`, `layer_mut(id)` | the layer anywhere in the tree, or `None` |
| `layer_path(id)` | its ancestors from the root down, or `None` |
| `for_each_layer(f)` | depth-first visit in draw order, parents before children |
| `effective_layer(id)` | `EffectiveLayer`: visible (all ancestors visible), locked (any ancestor locked), opacity (product), tint (product per channel), offset (sum), parallax (product) — or `None` for an unknown id |
| `object(id)`, `object_layer_of(id)` | an object by id and the layer holding it |
| `tile_data(cell)` | the `TileData` of a cell's tile, if its tileset has one |
| `tile_bounding_rect()` | the cells a finite map covers, or the union of the tile layers' used cells for an infinite one |
| `bounding_rect()` | the same in screen space (what an exported picture covers) |

An unknown id is `None`. It is never "visible", "locked" or any other default: callers decide
what a missing layer means, and `bt-edit` makes it a typed error.

## 5. Layers

```
Layer
├── id: LayerId
├── name, class: String
├── visible, locked: bool
├── opacity: f64                 0..1
├── tint: Option<Color>
├── offset: DVec2                screen pixels
├── parallax: DVec2              factor, 1 = none
├── blend_mode: BlendMode        Normal | Add | Multiply | Screen | Overlay | Darken | Lighten |
│                                ColorDodge | ColorBurn | HardLight | SoftLight | Difference | Exclusion
├── properties: Properties
└── kind: LayerKind              Tile(TileLayer) | Object(ObjectLayer) | Image(ImageLayer) | Group(Vec<Layer>)
```

`Map::layers` is the bottom layer first, as the files list them, and a group's children are
nested inside it in the same order. A group draws nothing itself; its attributes apply to its
children through `effective_layer`. Draw order is the depth-first order of `for_each_layer`.

`ObjectLayer { color: Option<Color>, draw_order: DrawOrder (TopDown | Index), objects:
Arc<Vec<MapObject>> }`: the object list is shared copy-on-write for the same reason chunks are,
so an undo snapshot of a tile edit does not copy the objects. `ImageLayer { image:
Option<ImageRef>, repeat_x, repeat_y }`.

## 6. Tile layers, chunks and cells

### 6.1 Tile layers

```
TileLayer
├── size: UVec2                  a finite layer's bounds; what the file declares for an infinite one
├── bounds: TileBounds           Finite | Infinite
└── chunks: BTreeMap<ChunkPos, Arc<Chunk>>
```

Finite and infinite layers share one representation: a sparse map of 16×16 chunks. A finite
layer refuses positions outside its bounds (`set_cell` returns `Err`, never stores silently);
an infinite layer accepts any position. The bounds kind is the map's: `Map::infinite` says it
and every tile layer agrees, which `Map::validate` checks and the resize and make-infinite
operations in `bt-edit` maintain.

Chunks are keyed in a `BTreeMap` ordered by row then column, so every walk over a layer (the
writer, a hash, a diff, a text view) is deterministic without sorting first. A lookup costs a
logarithm of a few thousand at most, which no caller notices.

| Function | Behaviour |
|---|---|
| `cell(p)` | the cell, `Cell::EMPTY` outside allocated chunks |
| `set_cell(p, cell)` | copy-on-write: a chunk shared with a snapshot is cloned on first write; returns whether anything changed; `Err` outside finite bounds |
| `chunks()` | chunks in order with their positions |
| `cells()` | non-empty cells with positions, in chunk order then row-major |
| `used_bounds()` | the smallest rectangle holding every non-empty cell |
| `chunk_bounds()` | the union of allocated chunks (Tiled's `bounds`) |
| `prune_empty_chunks()` | drops chunks that hold no tile; operations call it when they erase |

### 6.2 Chunks

`CHUNK_SIZE = 16`; `Chunk` is `[Cell; 256]` row-major behind an `Arc`. A chunk carries no
revision number: a cache that wants to know whether a chunk changed keys on the `Arc` and
retains it, so the pointer cannot be reused while the cache holds it. `ChunkPos` is `IVec2` in
chunk units; `ChunkPos::of(tile)` floors, so `(-1, -1)` is the chunk left of and above the
origin.

### 6.3 Cells

A cell is eight bytes and `Copy`:

```
Cell { slot_plus_one: u16, flip: Flip (u8), tile: TileId (u32) }
```

`slot_plus_one == 0` is the empty cell, and an empty cell has `flip == NONE` and `tile == 0`
(the constructors guarantee it, so `==` is content equality and `Cell::EMPTY` is the one empty
value). `Cell::new(slot, tile)`, `with_flip`, `slot() -> Option<TilesetSlot>`, `is_empty`,
`same_tile(other)` (ignores flips).

### 6.4 Flips

`Flip` has four bits with Tiled's in-memory meaning (`reference/tiled/src/libtiled/tilelayer.h`):

| Bit | Name | Orthogonal and isometric maps | Hexagonal maps |
|---|---|---|---|
| 1 | `H` | mirrored left-right | the same |
| 2 | `V` | mirrored top-bottom | the same |
| 4 | `D` | mirrored along the anti-diagonal (transpose) | turned 60° clockwise |
| 8 | `HEX120` | unused | turned 120° clockwise |

Drawing applies `D`, then `H`, then `V` on square grids; on hexagonal grids it mirrors by `H`
and `V`, then turns by 60° × `D` + 120° × `HEX120`. The transforms are the ones Tiled's tables
give (`Cell::rotate`, `TileLayer::rotateHexagonal`, `flipHexagonal`): `flipped_horizontally`,
`flipped_vertically`, `rotated_cw`, `rotated_ccw` (quarter turns), `rotated_hex(clockwise)`
(sixth turns) and `flipped_hex(horizontally)`. Tests check every table against the matrix
meaning of the flags, and that four quarter turns and six sixth turns are the identity.

## 7. Tilesets

```
Tileset
├── name, class: String
├── tile_width, tile_height: u32         the grid cell for atlases; the largest tile for collections
├── spacing, margin: u32
├── image: TilesetImage                  Atlas { image: ImageRef, columns: u32, tile_count: u32 } | Collection
├── tile_offset: IVec2
├── object_alignment: Alignment          Unspecified (default) or one of nine positions
├── tile_render_size: TileRenderSize     Tile | Grid
├── fill_mode: FillMode                  Stretch | PreserveAspectFit
├── grid: Option<TilesetGrid>            orientation and cell size for overlays and collision editing
├── background_color: Option<Color>
├── transformations: Transformations     bit flags HFLIP, VFLIP, ROTATE, PREFER_UNTRANSFORMED (for terrain filling)
├── tiles: BTreeMap<TileId, TileData>    atlas: only tiles with data; collection: every tile
├── tile_order: Vec<TileId>              a collection's display order when not by id (Tiled 1.7+); empty = by id
├── wang_sets: Vec<WangSet>
├── properties: Properties
├── next_tile_id: u32                    the id the next added collection tile gets
└── file: FileFormat                     for a tileset in a file of its own
```

The kind is an enum, so an atlas without an image or a collection with a column count cannot
exist. For an atlas, `columns` and `tile_count` are what the image actually has (the loader
counts them in the image it reads, as Tiled does); the `ImageRef` keeps the size the file
declared, which is what "the image changed size since the file was written" is measured
against. Tile images: `tile_image(id) -> Option<TileImage { image: &ImageRef, rect: IRect }>`
computes an atlas cell from the columns, spacing and margin, or returns a collection tile's
image and sub-rectangle. `has_tile`, `tile`, `tile_pixel_size`, `animated_tile(id, time_ms)`
(the frame shown at a time), `tile_ids()` (display order), `set_tile_order`, `relocate_tiles`
(Tiled's rearranging, ids unchanged) complete the surface.

```
TileData
├── class: String
├── probability: f64                     relative weight for random painting and terrain filling (1.0)
├── image: Option<ImageRef>              collection tiles
├── rect: Option<IRect>                  a sub-rectangle of that image (Tiled 1.9+)
├── animation: Vec<Frame { tile: TileId, duration_ms: u32 }>
├── collision: Option<CollisionGroup>    { id: LayerId, layer: ObjectLayer }: the shapes in the tile's own
│                                        pixel space with ids unique in the tile, and the group's colour and draw order
└── properties: Properties
```

`ImageRef { source: ImageSource, declared_size: UVec2, transparent: Option<Color> }` with
`ImageSource::File(FileRef)` or `ImageSource::Embedded(EmbeddedImage { format, bytes:
Arc<[u8]> })`: an embedded image is bytes in the model, decoded through `bt-image` by content
hash and written back as the same bytes, never a temporary file. `ImageRef::path()` is the
resolved file of a file image. An atlas tile inside the image the file declares holds an entry
only when it has data; the loader drops empty entries there.

## 8. Objects

```
MapObject
├── id: ObjectId
├── name, class: String
├── pos: DVec2                           pixel space
├── size: DVec2
├── rotation: f64                        degrees, clockwise, around the alignment origin
├── visible: bool
├── opacity: f64                         Tiled 1.12
├── shape: Shape                         Rectangle | Ellipse | Point | Capsule | Polygon(Vec<DVec2>) | Polyline(Vec<DVec2>) | Text(TextData)
├── tile: Option<Cell>                   a tile object draws this tile at `size`
├── template: Option<FileRef>            the template of a template instance
├── overrides: ObjectOverrides           which fields a template instance sets itself
└── properties: Properties
```

Where an object is: for rectangles, ellipses, capsules and text, `pos` is the top-left of the
box; for points it is the point; polygon and polyline points are relative to `pos`; for tile
objects `pos` is the alignment origin, bottom-left by default and bottom-centre on isometric
maps (§13.6), and `size` is the drawn size. A tile object keeps `tile` beside its shape, as in
Tiled's model, and drawing uses the tile. `TextData` holds the text, font family, pixel size,
wrap, colour, bold, italic, underline, strikeout, kerning and alignment with Tiled's defaults
(sans-serif, 16, no wrap, black, kerning on, left, top).

**Templates.** A template instance carries the template's values for every field it does not
override, so the object is complete without the template file at hand; `overrides` (name, class,
size, rotation, opacity, visible, tile, shape, text) says which fields are its own and are the
only ones written. Custom properties merge: the instance's own on top of the template's.
`ObjectTemplate { object: Option<MapObject>, tilesets: Vec<TilesetRef>, file: FileFormat }` is a
`.tx` file: an object without id and position, and the tileset its tile refers to;
`ObjectTemplate::from_map_object(map, object)` is "Save As Template".

## 9. Properties

```
Properties = BTreeMap<String, Property>        sorted by name, as Tiled writes them
Property { value: Value, custom_type: Option<String> }
Value = String(String) | Int(i64) | Float(f64) | Bool(bool) | Color(Option<Color>)
      | File(String) | Object(ObjectId) | Class(Properties) | List(Vec<Property>)
```

Values are kept **in the form the file stores them**: an enum value is its name or its number
as written, a class value holds only the members that are set, a file is the path string as
written (resolved against the containing file's directory when needed, by `bt-tmx` or the
caller that knows the directory), and `custom_type` is the `propertytype` name. Nothing is
lost when a map is opened without the project that defines its types, which is where Tiled
loses the type link. Typing is resolved on demand through `CustomTypes` (§10). A `Color(None)`
is Tiled's empty colour; an `Object(ObjectId(0))` is "none".

## 10. Custom types

```
CustomTypes { types: Vec<CustomType> }                  file order
CustomType { id: CustomTypeId, name: String, kind: Class(ClassType) | Enum(EnumType) }
ClassType  { members: Properties, color: Color, draw_fill: bool, use_as: ClassUsage }
EnumType   { storage: EnumStorage (String | Int), values: Vec<String>, values_as_flags: bool }
ClassUsage = PROPERTY | MAP | LAYER | OBJECT | TILE | TILESET | WANG_SET | WANG_COLOR | PROJECT
```

A type is identified by name **and** usage: an enum and an object-only class may share a name,
as in Tiled. The operations, each a port of Tiled's rule in `reference/tiled/src/libtiled/propertytype.cpp`
and `object.cpp`:

| Operation | Rule |
|---|---|
| `find(name, usage)`, `class_for(name, usage)`, `property_type(name)` | by name, filtered by overlapping usage |
| `merge(other)` | a type replaces the existing one with its name and an overlapping usage, keeping that id; others are added with fresh ids |
| `normalize(property)` | brings a value into its type's storage form (enum names or numbers, members typed like the class member) |
| `resolve_value(property)` | fills a class value with the members it leaves unset, recursively, stopping at a class that contains itself |
| `resolved_properties(class, usage, own)` | class member defaults overridden by the data's own properties |
| `object_properties(object, tile, template)` | class members, then the tile's properties, then the template's, then the object's own; the class is the object's own, else the template's, else the tile's |
| `circular_members()` | the `(class, member)` pairs that make a class contain itself; the loader warns and drops them as Tiled does |
| `EnumType::{to_int, to_name, to_storage, default_value}` | names ↔ numbers, comma-joined names for flags, unknown names kept unchanged |

## 11. Terrain sets

```
WangId([u8; 8])                              lanes in Tiled's order: Top, TopRight, Right, BottomRight,
                                             Bottom, BottomLeft, Left, TopLeft; odd indexes are corners;
                                             a lane is a 1-based colour index, 0 = unset
WangSet   { name, class, kind: Corner | Edge | Mixed, tile: Option<TileId>, colors: Vec<WangColor>,
            tiles: BTreeMap<TileId, WangId>, properties }
WangColor { name, class, color: Color, tile: Option<TileId>, probability: f64, properties }
```

The eight lanes are the information Tiled packs into its 64-bit Wang id; they are kept as
eight bytes here because every reader of the model wants a lane, and `bt-terrain` packs them
into a `u64` where it needs masks. A set has at most 254 colours. `WangId::parse` reads both the
comma-separated form and the hexadecimal form of files from before Tiled 1.5, which is the one
piece of file syntax the model knows, because it is a value, not a document.

## 12. Worlds, projects, templates

```
World        { maps: Vec<WorldMap { file: FileRef, rect: IRect }>, patterns: Vec<WorldPattern>,
               only_show_adjacent_maps: bool, grid_size: IVec2, properties }
WorldPattern { regexp: String, multiplier: IVec2, offset: IVec2, map_size: IVec2 }  with map_rect(column, row)
Project      { folders: Vec<FileRef>, extensions_path: FileRef, automapping_rules_file: Option<FileRef>,
               object_types_file: Option<FileRef>, commands: Vec<Command>,
               compatibility_version: CompatibilityVersion, custom_types: CustomTypes, properties,
               unknown_keys: BTreeMap<String, String> }   keys a newer Tiled wrote, as JSON text, written back
Command      { name, command, arguments, working_directory, shortcut, enabled, save_before_execute, show_output }
CompatibilityVersion = Tiled1_8 | Tiled1_9 | Tiled1_10 | Latest | Other(i64)   with number() and version_string()
FileRef      { source: String (as written), path: PathBuf (resolved) }
```

`CompatibilityVersion` decides the `version` a writer puts in new files and whether tile and
object classes are spelled `class` (1.9 only) or `type`; nothing else, as in Tiled.

## 13. Geometry

`MapGeometry` is a `Copy` value built from a map (`Map::geometry()`): orientation, render
order, tile size, map size, infinite, hex side length, stagger axis and index, skew. Every
formula, including its integer rounding, is a port of libtiled's renderers
(`orthogonalrenderer.cpp`, `isometricrenderer.cpp`, `staggeredrenderer.cpp`,
`hexagonalrenderer.cpp`, `obliquerenderer.cpp`, `hex.cpp`), so a cell is where Tiled draws it.

### 13.1 Conversions

| Function | Meaning | Per orientation |
|---|---|---|
| `tile_to_screen(DVec2)` | Tiled's `tileToScreenCoords` | orthogonal: the cell's top-left; isometric: the top corner of its diamond; hexagonal and staggered: the top-left of its bounding box (integer); oblique: the sheared top-left |
| `screen_to_tile(DVec2)` | the inverse, fractional | hexagonal: the cell with the nearest centre; staggered: the cell plus the position inside its diamond |
| `tile_at(DVec2)` | the cell under a screen position (`screen_to_tile` floored) | |
| `pixel_to_screen`, `screen_to_pixel` | object positions to drawn positions and back | identity except isometric (projection) and oblique (shear; a singular shear maps to itself) |
| `pixel_to_tile`, `tile_to_pixel` | object positions to cells and back | isometric divides by the tile height on both axes |
| `cell_anchor(IVec2)` | the bottom-left of a cell's image before the tile offset; Tiled draws tile images from there, so larger images extend up and right | |
| `cell_image_rect(IVec2, size)` | the screen rectangle an image of `size` covers at a cell | |

Round trips hold within floating-point noise on square and isometric grids; on hexagonal and
staggered grids `tile_to_screen` is integer and `screen_to_tile` returns to the same cell.

### 13.2 Hexagonal and staggered grids

`hex_params()` gives Tiled's `RenderParams` with halves rounded down, so the effective tile can
be a pixel smaller than the map's:

```
side_length_x = hex_side_length if hexagonal and stagger axis X, else 0
side_length_y = hex_side_length if hexagonal and stagger axis Y, else 0
side_offset_x = (tile_width  - side_length_x) / 2        column_width = side_offset_x + side_length_x
side_offset_y = (tile_height - side_length_y) / 2        row_height   = side_offset_y + side_length_y
tile_width'   = column_width + side_offset_x              tile_height' = row_height + side_offset_y
```

A column is shifted down (axis X) or a row shifted right (axis Y) when its parity matches the
stagger index. A staggered map is the hexagonal case with a side length of zero. `geometry/hex`
holds cube coordinates (`Hex::from_offset`, `to_offset`, `rotated_cw`, `rotated_ccw`) for
turning stamps by 60° and for terrain neighbours; turning a whole stamp is `bt-edit`'s.

### 13.3 Draw order

`for_each_tile(exposed: DRect, f)` visits the cells a screen rectangle exposes in Tiled's draw
order, and `draw_order_key(tile) -> DrawOrderKey { major, minor }` is the sort key that
reproduces that order without visiting: by row in the render order on orthogonal and oblique
maps (the render order is honoured only there, as in Tiled), by diagonal `x + y` on isometric
maps, by row on hexagonal and staggered maps with the stagger-axis-X variant sorting by
half-row. The renderer merges chunks by this key on every orientation but orthogonal right-down.

### 13.4 Neighbours, outlines, grid

- `neighbors(tile)`: the cells Tiled's fill tools connect: four on square grids, six on
  hexagonal and staggered grids, clockwise; `top_left`, `top_right`, `bottom_left`,
  `bottom_right` are the diagonal neighbours of staggered grids.
- `tile_polygon(tile)`: the cell's screen outline clockwise (rectangle, diamond, parallelogram
  or hexagon). `rect_outline(rect)` and `region_outline(cells)`: closed loops around a set of
  cells, with shared edges cancelled, for selections and brush footprints.
- `grid_lines(visible, major)`: the segments of the grid inside a screen rectangle, with every
  `major`-th line marked, and `grid_fade_size()` the zoom below which the grid is pointless.
- `bounding_rect(tiles)`, `map_bounding_rect()`, `tiles_in_rect(screen)`: the screen rectangle
  of a block of cells and back.

### 13.5 Snapping

`snap_to_grid(pixel, subdivisions)`: a pixel-space position to the grid or to a finer grid of
`subdivisions` cells per tile; hexagonal and staggered grids snap to the nearest corner of the
cell, or its centre when subdivided. This is the only snapping code; the object tools, the
collision editor and `bt-edit` call it.

### 13.6 Objects

`object_alignment(tileset)`: Tiled's effective alignment: the tileset's for tile objects when
set, else bottom-centre on isometric maps and bottom-left elsewhere, and top-left for every
other object. `upright_object_rect(pos, size, alignment)`: the screen rectangle of an object
drawn upright (tile and text objects): the projected position minus the alignment fraction of
the size. `object_outline(object)`: the drawn outline of any object in screen space, rotation
included, which the canvas, the headless pictures, the Godot export and `bt view` all use; it is
built with the renderer (M1), which is its first user.

As built at M2, the one object geometry lives here, following libtiled: `object_outline(geometry,
object, tileset)` and `object_pixel_outline` (the same in pixel space) give an `Outline`
(`Area`, `Path` or `Point`, with `bounds`, `segments`, `hits`, `touches`, `lies_within`);
`object_marker` is Tiled's 20-pixel square or circle for a shape without a size;
`tile_object_box` places a tile object's image by its alignment and scaled tile offset;
`ObjectFrame` maps an object's own coordinates to the screen and the map's pixels and back
(upright for tile and text objects); `Segment` with the nearest point and distance, and the
polygon and rotation helpers they need. `bt-edit` measures, hits and resizes objects with them,
and the renderer draws them. `MapObject::kind_name` names an object's kind for the Objects
panel.

## 14. Validation

`Map::validate() -> Vec<Violation>` states the invariants the crate cannot enforce in a
constructor, so loaders can report them as warnings and tests can assert them:

- layer ids unique across the tree, object ids unique across the map, `next_*_id` above every id;
- every cell's slot names a tileset of the map; a tile id past a tileset's tiles is reported,
  not fixed (Tiled keeps such references, and so does the writer);
- every tile layer's bounds kind matches `Map::infinite`, and finite layers hold no cell
  outside their bounds;
- Wang lanes name existing colours, sets have at most 254 colours;
- template instances with `overrides` have a `template`.

`bt-tmx` runs it after loading and turns each violation it does not already report into a
warning; `bt-edit`'s property tests run it after every operation.

## 15. `bt-utils`

Each module is one primitive, defined once for every crate above:

| Module | API |
|---|---|
| `rect` | `IRect { x, y, w, h }`: integer, half-open, `from_min_size`, `from_corners` (inclusive), `bounding(positions) -> Option`, `min`, `max` (exclusive), `size`, `is_empty`, `contains`, `intersection`, `union`, `translated`, `points()` row-major. `DRect { min, max }`: floating point, half-open, `from_min_size`, `from_points`, `size`, `is_empty`, `contains`, `intersects`, `intersection`, `union`, `expanded`, `translated`, `to_aligned()` (Qt's `toAlignedRect`). One convention: a rect is empty when a side is not positive |
| `rng` | `Rng::new(seed)` (SplitMix64, `next_u64`, `next_f64`, `below(n)`), `cell_rng(seed, position)` (a generator derived from a seed and a cell, so a result depends on where, not on visit order), `RandomPicker<T>` (Tiled's weighted picker: weights, `pick`) |
| `raster` | `points_on_line(a, b, Connectivity::{Eight, Four})` (Bresenham, every cell once), `points_on_ellipse(center, radii)`, `ellipse_region(rect) -> Vec<IRect>` (one span a row) (Zingl's rasterisers, MIT, as Tiled uses them) |
| `bands` | `banded_rects(cells) -> Vec<IRect>`: a set of cells as the row bands `QRegion` would give, which is the order Tiled's filler and automapper visit (a set's bounding rectangle is `IRect::bounding`) |
| `hash` | `CellHasher`, `CellHashMap`, `CellHashSet` (a fast non-cryptographic hasher for position keys), `Fnv64`, `fnv1a_64(bytes)` (also the content hash of embedded images), `fnv1a_32(bytes)` |
| `atomic_write` | `write_file(path, bytes, WriteMode::{Atomic, InPlace})`: atomically, a temporary file next to the target (`.<name>.<pid>-<n>.tmp`, the first free `n` taken with `create_new`), permissions copied from an existing target, `fsync`, rename, directory `fsync` on Unix; in place for folders where the rename misbehaves; follows symbolic links; refuses with `Refused::ReadOnlyFile` or `Refused::ReadOnlyFolder` (typed, carried in the error) before writing anything; a failure leaves the original and removes the temporary file |
| `paths` | `search_up(start, home, found)` (upward search stopping after a repository top and before the home folder), `relative_path(base_dir, target) -> Option` with `/` separators, `clean_path` (lexical, like `QDir::cleanPath`), `absolute_path -> Result`, `file_stem` (Qt's `completeBaseName`), `display_name` |
| `text` | `plural(n, word)`, `quoted_list`, `and_list`, `slug` (lowercase hyphenated names), `fuzzy_match(query, candidates)` with the one scoring used by every name lookup, `natural_cmp` |

No module reads configuration, holds state between calls or depends on another crate of ours.

## 16. Memory and cost

A cell is 8 bytes, a chunk 2 KiB plus the `Arc`; a 1000×1000 layer fully painted is about
8 MB, and an undo step after a brush stroke costs the stroke's chunks only. `Map::clone` is a
deep clone of the tree with shared chunks, object lists and tilesets: cheap enough that `bt-edit`
works on clones and `bt-editor` keeps hundreds of snapshots. Finding a layer walks the tree;
maps have tens of layers, not thousands, so no index is kept.

## 17. Tests

- Every module has inline tests of its own invariants (cell size, empty-cell canonical form,
  copy-on-write sharing, id allocation, tile order after relocation).
- `flip`: the transform tables against Tiled's and against the matrix meaning of the flags.
- `geometry`: round trips of every conversion on every orientation with odd tile sizes and both
  stagger settings, generated positions included; the staggered and hexagonal cases of Tiled's
  own unit tests (`reference/tiled/tests/staggeredrenderer`); draw-order keys agree with
  `for_each_tile`; outlines close and cover the right pixels.
- `custom_types`: Tiled's example project (`examples.tiled-project`: `Body` → `Fixture` →
  `Filter`) resolved, normalised and completed as Tiled does; circular classes detected.
- `validate`: each violation produced by a minimal map.
- `bt-utils`: `atomic_write` against read-only files and folders, symbolic links, failures
  mid-way; `rng` determinism and `cell_rng` independence from visit order; `raster` against
  known pixel sets; `paths` on Windows-style and POSIX inputs.
