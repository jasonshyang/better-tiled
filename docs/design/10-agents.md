# Agents — `bt-script` and `apps/bt`

*2026-10-09; as built at the end of M4 (§17), 2026-10-11. The design of the command line for
coding agents: the `bt` program, the script language behind `bt edit`, the names, shapes and
feet that let an agent speak of a map without tile ids, the typed reports, the checks, and the
guide that is the only documentation an agent gets. The high-level design
(`00-high-level-design.md`, §11) states the goals and the design rules; this doc says how they
are kept.*

## 1. Scope

`bt-script` is the library: the script parser and runner, the shape language, name resolution,
feet, the text views, the catalogue, the checks, the diff, the undo history and the typed
reports. It works on maps in memory through `bt-core`, `bt-edit`, `bt-terrain`, `bt-automap`,
`bt-render` and `bt-project`, reads files through `bt-tmx` only where a script names another map,
and has no knowledge of the editor or of sockets. `apps/bt` is the binary: the `clap` command
table, loading a map from its file or from the running editor, the live-link client
(`11-live-link.md`), the Godot commands (`12-godot-export.md`), and `bt guide`. The editor runs
scripts through the same library, so an edit made through the live link is the same edit.

Dependencies: `bt-script` on `bt-core`, `bt-tmx`, `bt-image`, `bt-render`, `bt-edit`,
`bt-terrain`, `bt-automap`, `bt-project`, `bt-utils`; `apps/bt` on `bt-script`, `bt-godot`,
`bt-live`, `bt-tmx`, `bt-render`, `clap`, `anyhow` (binaries only).

## 2. Terms

| Term | Meaning |
|---|---|
| cell | `X,Y`, whole numbers from the top-left; the unit of every shape |
| point | `X,Y` in tile units; `10.5,5.5` is the centre of cell 10,5 and `10.5,6` the middle of its bottom edge |
| region | `X,Y WxH` or `X1,Y1..X2,Y2` with inclusive corners; in options `X,Y,WxH` |
| name, slug | lowercase words joined by `-`; case and separators never matter on input |
| terrain | one colour of one terrain set, by name |
| sprite | a tile of an image-collection tileset, placed as a tile object by its foot |
| foot | where a sprite touches the ground: the middle of the lowest opaque pixels of its image |
| standing cell | the cell a point stands in; a point on a cell's bottom edge stands in that cell |
| tile group | tiles under one name: a tileset class, or `define NAME = tile …` |
| material | terrains painted together on several layers (`define NAME = TERRAIN layer L + …`) |
| group | sprites placed together at offsets (a tree and its shadow) |
| piece | tiles and objects copied from a map once and placed by a foot cell |
| shape, area | a set of cells written in the shape language (§6) |
| operation | one line of a script |
| problem line | a line of output an agent should act on; everything else is a fact |
| report | a typed value every command returns, with a text and a JSON rendering |

## 3. Module map

`bt-script`:

| Module | Holds |
|---|---|
| `names` | `Names`: tilesets, terrains, sprites, tile groups, defined names; lookups with suggestions (§5) |
| `feet` | feet measured from images, standing cells, positions for a foot |
| `grid` | sides and touching cells per orientation, floods, areas, shortest ways |
| `shapes` | the shape language's cells: rasterisation and narrowing (§6) |
| `script` | the parser: `Op` and its parts, one `Located` op per line (§7) |
| `run` | `Runner`: a script on a copy of a map, all or nothing (§8) |
| `report` | `Report`, `Line`, the text renderer and the JSON rendering (§9) |
| `edit_report` | what a script changed: the typed outcome of a run (§9) |
| `view` | the text view, `--ids`, `--corners` (§10.2) |
| `info`, `objects`, `catalog`, `regions`, `explain_text`, `diff`, `create` | the other Look and Know commands' outcomes (§10) |
| `check` | problems and what things stand on (§11) |
| `picture` | numbered renders through `bt-render` (§10.3) |
| `history` | the per-map versions `bt undo` restores (§12) |

`apps/bt`:

| Module | Holds |
|---|---|
| `cli` | the `clap` derive types: every command and option, once |
| `main` | dispatch, exit codes, output |
| `load` | a map from its file or from the editor, `--project`, `--painted` (§4.3) |
| `commands/*` | one module per command group, thin: arguments in, a report out |
| `live` | the client side of the live link (`11-live-link.md`) |
| `godot` | `bt godot …` (`12-godot-export.md`) |
| `guide.md` | the guide, included as a string (§14) |

## 4. The program

### 4.1 Commands

Commands and their options are `clap` derive types declared once. The table is the one in
`00-high-level-design.md`, §11.1; the options:

| Command | Arguments and options |
|---|---|
| `guide` | |
| `info` | `MAP` |
| `view` | `MAP [--region R] [--layers L,…] [--by terrain\|tile\|layer] [--runs] [--no-objects] [--step N] [--painted]`; `MAP --ids\|--corners [--region R] [--layers L,…]` |
| `at` | `MAP X,Y [--painted]` |
| `objects` | `MAP [--type T] [--layer L] [--in R] [--name N]` |
| `catalog` | `MAP [terrains\|sprites\|tiles [TILESET\|TILESET:ID,…]\|types] [--find TEXT] [--sheet OUT.png [--scale N]]` |
| `areas` | `MAP SHAPE [--all] [--with FILE] [--seed N]` |
| `reach` | `MAP FROM TO [--through SHAPE] [--with FILE] [--seed N]` |
| `explain` | `MAP X,Y [RULES] [RULE-LAYER=MAP-LAYER …] [--seed N] [--run N] [--all]` |
| `render` | `MAP OUT.png [--region R] [--scale S] [--grid] [--clean] [--highlight SHAPE] [--layers L,…] [--marks] [--with FILE] [--painted]` |
| `new` | `MAP --like OTHER.tmx [--size WxH]`; `MAP --size WxH --tile WxH [--tilesets A,B] [--layers name:tiles,…]`; `[--force]` |
| `edit` | `MAP [SCRIPT\|-] [--do "OPS"] [--with FILE] [--seed N] [--dry-run] [--view [--by B] [--runs]] [--render OUT.png [--region R] [--scale S]] [--out OTHER.tmx] [--in-editor] [--mark LABEL] [--force]`; with `--do`, several maps |
| `undo` | `MAP [--force]` |
| `check` | `MAP` |
| `diff` | `MAP [OTHER]` |
| `editor` | `[MAP] [--screenshot OUT.png]` |
| `mark` | `MAP SHAPE [--label TEXT] [--add] [--no-focus] [--with FILE]`; `MAP --clear` |
| `open`, `save` | `MAP` |
| `godot` | `setup \| export \| status \| verify …` |
| `export` | `MAP\|TILESET OUT [--embed-tilesets] [--detach-templates] [--resolve-types] [--minimize]`; `MAP OUT.json --resolved`; `--schema` |

Every command takes `--project FILE` (the `.tiled-project` whose custom types apply; else the
nearest one above the map) and `--json`. Unknown options are usage errors that list the known
ones; a region option takes `X,Y WxH` unquoted as well as `X,Y,WxH`; a bare `X,Y,W,H` is refused
rather than guessed. `bt help COMMAND` prints the command's line from the same table.

### 4.2 Exit codes and output

0 done, 1 failed, 2 wrong usage, 3 refused, 4 out of date, 5 problems found. A report goes to
standard output; a failure's message goes to standard error with `bt:` in front. A refusal and
"out of date" are answers, printed like any report with their own code (`bt godot`, and `bt
check`'s 5), so a script can tell "problems" from "could not run". Nothing is written on a
failure: a command either finishes or leaves every file as it was.

### 4.3 Loading a map

`load(path, options)` gives every command the same map:

- the file, through a `bt-tmx` loader built with the project's custom types;
- the editor's copy instead, with its unsaved changes and its unsaved tileset edits, when the map
  is open in a running editor with unsaved changes (`11-live-link.md`); `bt info` says which;
- with `--painted`, on a map with AutoMap layers, what was painted rather than what the rules
  make of it (`08-automap.md`, §10), read from the file and the project file; refused while the
  map has unsaved changes in the editor, with `bt save` as the way;
- with the map's reading warnings, which `check` lists as problems and `info` as warnings.

A map with AutoMap layers is edited as what was painted and reported as what the rules make
(§8.7). Read-only files (locked by version control) are never written over: the commands that
write say "the file is read-only" or "its folder can't be written to" and exit 1.

## 5. Names

Agents name things; `bt` never asks them for a tile id except for tiles that have no name.

- **Slugs.** `"Pine Tree_big"` is `pine-tree-big`: lowercase letters and digits, every other run
  one `-`. Every lookup slugs its input, so case and separators never matter.
- **Terrains.** A colour's name; the set's name when the set has one colour; else `set-n`. Names
  that collide are qualified in two steps, `set/name` then `tileset/set/name`, and what still
  collides gets `-2`, `-3`. An unqualified name is accepted when it is the last part of exactly
  one qualified name, and refused as ambiguous with the options when of several.
- **Sprites.** The tiles of image-collection tilesets, named after their image file's stem;
  frames that only belong to another tile's animation are not sprites of their own; a sprite
  also goes as `tileset:id`.
- **Tiles.** `TILESET:ID`, `TILESET:A-B`, and after one of those more ids of the same tileset;
  tile groups by class name; `define NAME = tile …` for the script; a sprite's name for its
  tile. A tile id the tileset lacks says the tileset's range.
- **Grounds.** In `where`, `on`, `inside`, `avoid` and `near` a name is a terrain, a tile group,
  tiles written as ids (several ids written together are one ground: any of them) or a defined
  area, in that order of lookup; a tile name named where a terrain is required says "is tiles,
  not a terrain: place tiles with `tile …`".
- **Layers** by name or `#id`; several layers of one name must be told apart by id.
- **Objects** by `#id`, by name when unique, by type when exactly one object has it (`move entry
  to …`), `selected` for the editor's selection and `last` for what the previous operation made.
- **Unknown names** answer with the closest three (names containing the query first, then by
  edit distance up to a third of the query's length, at least two) and where the full list is:
  `bt catalog MAP terrains`, `bt info MAP`, `bt objects MAP`.

`Names::of(map)` builds every table once per map; a runner rebuilds it when a script adds
layers or tilesets, keeping what the script defined.

## 6. Shapes and connection

### 6.1 Cells

A shape is rasterised on the map's grid: a cell belongs to it when the cell's centre does, inside
the map for finite maps (an infinite map's `all` is the area painted so far, or its saved size
while it is empty).

| Shape | Cells |
|---|---|
| `X,Y`, `cells X,Y …`, `rect`, `all` | as written |
| `ellipse CX,CY RXxRY [rough R]`, `circle` | centre in tile units, radii in cells; `rough` moves the radius by up to `R` of itself with smooth noise around the circle (five sine waves, seeded), for natural shores |
| `poly X,Y …` | centres inside, even-odd rule |
| `path X,Y … [width W]` | centres within `W/2` of the line, plus a 4-connected line of cells along it, so a thin path never hangs together by corners only; round ends and bends |
| `where NAME,…` | the cells any of the grounds covers wholly |
| `under LAYER,…` | the cells the visible objects of those layers cover |
| `@NAME`, `@#ID`, `@selected` | the cells inside an object: its shape for rectangles, ellipses, polygons and sprites' images, the standing cell for a point |
| `selection` | the cells selected in the editor |

Narrowing, applied in this order: `minus SHAPE` (any number), `on` (cells a ground covers
wholly), `inside` (cells whose touching neighbours a ground covers wholly too, so edges painted
around the shape stay on it), `avoid [margin N]` (drop cells within `N` of a ground's touch;
`N = 0` drops the cells it touches), `near [margin N]` (keep cells within `N`, default 1),
`containing X,Y …` (keep the connected parts holding those cells, refused when a cell is not in
the shape). `rough` and `width` belong to the shape just before them, a `minus` shape included;
`margin` to the `avoid` or `near` before it.

A ground **covers** a cell when its terrain is on every lane of the set's kind on some tile
layer, or the cell holds one of its tiles on the layers it names (all tile layers by default), or
the cell is in the defined area; it **touches** a cell when its terrain is on any lane.

### 6.2 Connection

Cells that share a side are connected: four sides on orthogonal and isometric maps, six on
hexagonal ones, the four diagonal neighbours on staggered ones. A margin counts cells that
touch, at a side or a corner: eight on orthogonal, isometric and staggered maps, six on
hexagonal ones. `grid` gives `sides`, `touching`, `within`, `flood`, `areas` (connected
components, largest first), `way` (breadth-first shortest path through allowed cells) and
`turns` (a path's corners, for a short description). Which cells count is the agent's: a shape
says.

## 7. The script language

One operation per line; `#` starts a comment except in `#12` (an object id); words with spaces
go in quotes; `seed N` on its own line sets every following operation's randomness, and `seed N`
at the end of an operation gives that one its own. A comment on the first line names the edit
in the editor's history. Parsing checks syntax only; names resolve when the script runs, so a
parse error says the line and what was expected, and an unknown operation gets "did you mean".

| Family | Operations |
|---|---|
| names | `define NAME = TERRAIN [layer L] + …` (material); `= sprite S [layer L] [+ sprite S2 at DX,DY [layer L]]` (group); `= tile TILE,… [layer L]`; `= area SHAPE` (the cells as they are on that line); `= copy MAP REGION [minus SHAPE] [layers L,…] [objects] [foot X,Y]` (piece) |
| terrain | `paint TERRAIN SHAPE [layer L]`, `erase TERRAIN SHAPE [layer L]` |
| tiles | `tile TILE,… SHAPE [layer L]`, `clear SHAPE layer L,…\|all`, `copy MAP REGION [layers L,…] to X,Y [objects]` |
| sprites, pieces | `place NAME,… at X,Y … \| in SHAPE [jitter J] \| rect X,Y WxH` with `[flip\|flip random] [layer L] [name N] [like OBJECT] [key=value…]`; `scatter NAME,… in SHAPE [density D\|count N] [spacing S] [jitter J] [flip random] [layer L] [like OBJECT]` |
| objects | `object TYPE at X,Y \| rect \| ellipse \| poly \| path … [name N] [layer L] [like OBJECT] [key=value…]`; `object TYPE in SHAPE [count N] [spacing S] …`; `set OBJECT … key=value…`; `move OBJECT to X,Y`; `move OBJECT … by DX,DY`; `delete OBJECT …`; `delete objects in SHAPE [type T] [layer L]`; `delete objects on TERRAIN,… [type T] [layer L]` |
| rules | `automap [RULES] [RULE-LAYER=MAP-LAYER …]`; `automap layers [RULES] [RULE-LAYER=MAP-LAYER …]`, `automap layers add …`, `automap layers seed N`, `automap layers off` |
| map | `property key=value …`; `layer add NAME tiles\|objects [above L\|below L]`, `rename`, `delete`, `hide`, `show`, `lock`, `unlock`, `move L above\|below M`; `resize WxH [shift DX,DY]` |
| drawing | `draw X,Y` … legend lines `C = …` … rows `\|…` … `end` |

The parser produces `Located { line, text, op: Op, seed }`; `Op` is one enum with a variant per
operation and typed parts (`Area`, `ShapeSpec`, `Placement`, `Geometry`, `Target`, `Legend`,
`LayerOp`, `LayersOp`). `--do "a; b"` splits on semicolons outside quotes. A `--with FILE` holds
`define` lines only (and `seed`), run before the script and also given to `areas`, `reach`,
`mark` and `render --highlight`, so names last across commands without touching a tileset.

## 8. Running a script

`Runner::new(map, path, loader, seed)` takes a copy; `run(op)` applies one operation; the caller
takes the map back only when every operation succeeded. A failure names its line and leaves the
caller's map untouched: all or nothing.

### 8.1 Randomness

Each operation draws from its own generator: the script seed mixed with the operation's
ordinal, or the operation's own `seed N`. A script with the same seed on the same map gives the
same map, in the file and in the editor.

### 8.2 Which layer

When an operation does not name a layer: a terrain goes where its set's tiles already are, else
the only tile layer, else the script must say; tiles go on the layer their tile name declares,
else the layer that holds most of them, else the only tile layer; a sprite goes where that
sprite is, else where its tileset's sprites are, else any layer with sprites, else the only
object layer, else a layer whose name contains one of `prop`, `sprite`, `decor`, `object`,
`entit`, `thing`, `tree`; a typed object goes where its type is, else where typed shapes are,
else the only object layer, else a name containing `design`, `marker`, `logic`, `spawn`,
`trigger`, `zone`, `meta`, `game`, `object`. Otherwise the error lists the layers and asks for
`layer L`. A locked layer refuses with `layer unlock L` as the way.

### 8.3 Terrain

`paint` and `erase` paint every cell of the shape as a full-tile target through `bt-terrain`
(`07-terrain.md`, §5), with the operation's seed. What came back goes into the report: cells no
tile fits (left as they were) with what the set lacks (`missing_text`), terrains the fill
pushed back outside the shape with the tile that would have let them stay, and cells of other
terrain sets the fill replaced (a set treats other sets' tiles as empty). A material paints its
parts in order.

### 8.4 Sprites, groups and pieces

`place … at` puts feet at the points; `in SHAPE` one per cell at the cell's bottom middle, moved
by up to `jitter` sideways and half of it up so it stays in the cell; `rect` stretches one sprite
to a box, on orthogonal maps. `scatter` shuffles the shape's cells with the operation's
generator, takes `count` or `density × cells` of them, keeps feet at least `spacing` apart from
each other and from the sprites already on the target layer, and skips cells where a group's
other sprites would stand outside the shape; it reports how many it placed when short of room.
A group places its first sprite at the foot and the others at their offsets, mirrored when
flipped; the name and properties go to the first. `like OBJECT` copies that object's type and
properties onto what was placed, except what the line wrote out; `size=WxH` stretches a sprite
around its foot. A piece is placed by its foot cell (by default the middle of its bottom row):
its tiles go on the layers of the same names (empty cells leave the map's), its objects on
theirs (made when missing), and `scatter` places a piece only where all of it is on the map.

### 8.5 Objects

Geometry is in tile units and converted through the map's geometry: a point, a rectangle, an
ellipse, a polygon or a path (points relative to the first). `object TYPE in SHAPE` picks cells
at random `spacing` apart and puts points at their centres. `set` takes the object's own fields
(`name`, `type`, `visible`, `rotation`, `size=WxH`, which keeps a shape's top-left and a
sprite's bottom-left; points, polygons and paths have no size) and properties; a value keeps the
type the property has, else the type the project's class gives, else it reads as an object
reference (`#12`), a whole number, a number (with a `.`), `true`/`false`, or text; an empty
value removes the property. `move … to` moves one object by its foot (a sprite), its point or
its top-left; `by` moves several. `delete objects on TERRAIN` deletes what stands on it as
`check` sees it (§11).

### 8.6 Rules

`automap` loads the rules file given, else the `rules.txt` beside the map, through the map's
loader so tilesets are shared, binds `RULE-LAYER=MAP-LAYER` pairs as a run (`08-automap.md`,
§10) and applies the rules once as Tiled would. Before running it says what would keep the rules
from doing anything: rule-map warnings, rule maps whose tilesets are copies of the map's, input
layers the map lacks (with a near-miss name, since names must match exactly) or has empty, and
after it the number of matches. On a map with AutoMap layers the step is a note: the rules run
anyway. `automap layers` starts (or starts over), adds a run, re-seeds or stops the map's AutoMap
layers; `off` bakes the result into painted tiles. New output layers are created and named in
the report.

### 8.7 Maps with AutoMap layers

The runner edits what was painted and the result is what the rules make of it. The report says
the map's changes as the rules make them and, under them, what the script painted; an object
the rules placed cannot be edited, and the error says so with the two ways out (change what the
rules read, or `automap layers off`). The file gets the result; the project file gets what was
painted (`09-project-file.md`).

### 8.8 Map operations

`resize` resizes as `bt-edit` does and reports what it leaves to do: the new strips, the tile
layers that had a tile in every cell and are still empty there when the script ends, and the
objects that reach the old edges next to new cells (walls, a stretched ground); every other
change is reported against the old cells moved by the shift. `property` and `layer` are the
`bt-edit` operations by name. `draw` paints its legend in order: terrains and materials, then
`clear`, then tiles as drawn (nothing fitted), then sprites at the bottom middle of their cells
and objects at the centre; rows reaching outside a finite map skip those cells with a note.

## 9. Reports

Every command returns a typed outcome (`EditOutcome`, `CheckOutcome`, `InfoOutcome`, …) and the
binary renders it: as text through one `Report` model (`Line { kind: Problem | Warning | Note |
Fact, text }` in sections, problems first, two-space indents, counts capped with "and N more"),
or as JSON with `--json` (the outcome serialised, with a `schema` field naming its shape). Tests
assert the outcomes as values; a few snapshot tests pin the wording. The editor, `bt` and `bt
--json` therefore describe one run in one way.

An `EditOutcome` holds: the map and the result; the number of operations; the size change and
the shift; per layer, in draw order from the top, what changed (new or removed layer, renamed,
shown or hidden, locked, cells changed with each terrain's cells before and after, tiles placed
or cleared, objects added as id ranges with their kinds and positions, removed with kind and
position, moved); what was painted apart from what the rules made; cells no tile fits; the
notes per line; what the edit did that `check` would point out (new seams, new hidden edges,
objects that stood on one ground and stand on another now), or the line that says there are
none; the changed cells and their bounding rectangle; the edit's label (the first-line comment,
else the first changing operation and "+N more"); the changed area as text when asked; and the
save line (the file, the project file, the `bt undo` hint).

## 10. Looking and knowing

### 10.1 `info`, `at`, `objects`

`info`: orientation, size and tile size; how a non-orthogonal grid sits on screen (the isometric
diamond's corners and edges, the shifted rows or columns of hexagonal and staggered maps);
whether the editor has it open and dirty; class and properties; tilesets with their tile counts,
columns, oversized tiles, terrains and what they are drawn over; layers top first with hidden,
locked, offset and parallax; the AutoMap line: the map's AutoMap layers (rules, runs, seed, what
the rules read and make, layers whose painted state was lost) or the `rules.txt` beside the map
(what it reads and writes, what the map lacks or has empty); on a rule map, its warnings and the
tiles that make no rule; reading warnings. `at X,Y`: the cell on every layer, top first, with
the tile, its terrain lanes, its sprite or class, and the objects standing there. `objects`:
one line per object with id, kind, name, layer, where it stands and its box, type and
properties, filtered by type, layer, region or name.

### 10.2 `view`

One character per cell with column and row rulers and a legend. By terrain (the default when a
cell shows one) a cell shows the terrain covering more than half of it, so a painted area reads
as exactly the cells painted; by tile each kind of tile has a symbol; by layer each layer has
one. Symbols are stable across views of one map: terrains whose names say what they are get a
hint (`~` water, ice; `"` grass; `:` sand; `,` dirt, path; `*` snow; `#` wall, cliff, platform;
`%` lava; `&` tree; `=` wood, bridge), the rest take the next free ground symbol in the order of
the tilesets' terrains, objects take capitals, and what is beyond the alphabet shows as `?` with
`--ids` as the way. Views wider or taller than 160 cells show one character per square of
cells (`--step N` chooses); `--runs` adds each row's runs (`~14-29`); hexagonal and staggered
maps print two characters per cell with the shifted rows one further right, so the cells that
touch are the ones next to each other in the text. `--ids` prints each cell's tile id layer by
layer; `--corners` prints the terrain at each corner of the cells, which shows how wide a gap or
a strip really is.

### 10.3 `render`

A PNG through `bt-render`'s headless path, with cell numbers along the top and left, a grid line
every five cells (`--grid`: every cell; `--clean`: neither), at most 2048 pixels a side unless
`--scale` says otherwise (up to 8), cells outlined in red for `--highlight SHAPE` (a region or
any shape of §6) and for what an edit changed, the editor's marks numbered in their colours for
`--marks`, and non-orthogonal maps in their own projection with `x,y` written inside every fifth
cell. Shapes take their class colours from the project.

### 10.4 `catalog`

What the map can be built with, by name. Terrains with their set's kind, the terrains each has
tiles with ("borders", and `nothing` when it has edge tiles), whether its edge tiles are opaque
(measured on a few edge tiles: the outer quarter under an empty lane is at least nine tenths
opaque), and where it is used by layer. Sprites with their image size, animation, the sprite
they are usually paired with ("usually with `shadow` at 0,1.22 on `shadows`, 6 of 8": the
median offset to the nearest sprite on another layer within two cells, when at least sixty
percent of the copies have one) and what all copies share (type and properties, as `place …
like #ID` would give). Tiles by id with where each is used and the unused ones as ranges, blank
tiles noted; `tiles TILESET:ID,…` (up to twelve) says where each is and which tiles the map has
next to it on each side, for shores and walls placed by hand. Types with counts and the
properties their objects carry. `--find` filters; `--sheet` draws the listed items numbered, or
every tile with its id laid out as in the tileset, the used ones outlined, `--scale` bigger.

### 10.5 `areas`, `reach`

`areas MAP SHAPE`: the connected areas of the shape's cells, largest first, each with its size,
bounding rectangle, which map edges it touches and a cell in it, and the `containing` line to
name one in a script. `reach MAP FROM TO [--through SHAPE]`: the shortest way through the
allowed cells (every cell between must be allowed; the ends may not be), as steps and turns, its
cells when few, and the cells every way passes through (closing one closes them all); with no
way, how far one gets from `FROM`, the nearest reached cell to `TO` with its distance counting
every cell, and what the border of the reached area is made of.

### 10.6 `explain`

The explanation of `08-automap.md`, §8 as text: the rules named by rule map and number with
their cells in the rule map; the changes at the cell in order with where the rule matched; rules
that matched and changed nothing with why; the rules that came close, each with what it would
write and the conditions that fail (what the layer must hold and what it has, noting what the
run changed earlier); the rest counted, listed with `--all`; a tile at the cell that no rule
names said first; a layer the map lacks read as empty with a near-miss name. On a map with
AutoMap layers each run is explained on what it reads, in the map's layer names (`--run N` for
one); otherwise the rules are the file named, else the `rules.txt` beside the map, with
bindings as the edit step takes them.

### 10.7 `diff`, `new`

`diff A B` lists what differs layer by layer in the words of an edit report; `diff MAP` lists
the editor's unsaved changes to it. `new` writes an empty map like another (its tilesets and
layers, emptied, with a hint for each layer that was filled with one tile, properties cleared,
file properties rebased) or from a size, a tile size, tilesets and `name:kind` layers; an
existing file needs `--force`.

### 10.8 `export`

`export MAP OUT` writes the map in the format `OUT`'s extension names, through `bt-tmx` with
Tiled's export options: `.tmx`, `.tmj` or `.json` (Tiled's JSON), `.js`; a tileset to `.tsx`,
`.tsj` or `.json`; `.png` a plain picture of the map without rulers; `.tscn` the Godot export of
`12-godot-export.md`. `OUT.json --resolved` writes the resolved scene JSON for other programs:
no global ids, flips as booleans, templates detached and inherited properties written out,
tiles with their terrain names, every cell's centre and every object's screen position and
outline on every orientation, image paths relative to the file; its JSON Schema ships with the
program (`bt export --schema` prints it) and every export is validated against it in tests.
Oblique maps are refused. Read-only targets are refused like any other write.

## 11. Check

`check` lists problems, then facts. Problems, in order: the map's reading warnings; terrain
seams per layer (a tile whose lanes disagree with the tile sharing its right or bottom side, on
staggered and hexagonal maps the diagonal neighbours; or a terrain ending abruptly next to an
empty cell when the set has edge tiles); opaque edge tiles lying over anything but whole ground
(the top-most tile below is a terrain's edge, plain tiles, or nothing), per layer and per what
is under them; objects standing outside the map; sprites on the same spot (feet within 0.4
cells); objects standing on nothing when most of their layer stands on something (a layer of
clouds is fine); objects whose type the project does not define, or with properties that are not
members of their class; orphaned project-file entries (`09-project-file.md`, §8). Then what
objects stand on and what zones cover, layer by layer: sprites and typed points grouped by
ground with counts, the few on other ground than the rest listed by id (at most a fifth of the
group and six), and each zone's cover by ground.

"What is under" a cell for an object on a layer: on the top-most shown tile layer below that has
a tile there, the terrain covering more than half of the tile; a terrain's edge tile looks
further down; plain tiles are "tiles of LAYER"; the bottom layer filling nine tenths of the map
with one tile is a backdrop and counts as nothing.

`check` states what is there and never what to do about it; the ways out live in the guide,
where they are read once, not repeated in every output. `made_by(before, after, cells)` is the
same checks on the two maps for the edit report: problems the map did not have before, and
objects standing in changed cells whose ground changed. `check` exits 5 when it lists a problem.

## 12. Undo

Before `bt edit` writes a file it keeps the previous bytes in `<data>/history/<stem>-<hash of
the absolute path>/` as `N.before`, with `N.after` holding a fingerprint of what was written and
the edit's label, and `N.entry` holding the map's project-file record as it was (or `none`).
Fifty versions are kept per map. `bt undo` refuses when the file's fingerprint is not the one the
last edit wrote (it was saved since), unless `--force`; it writes the kept bytes back atomically,
never over a read-only file, puts the record back, and says how many earlier edits remain. For a
map open in the editor, `bt undo` is the editor's undo of that edit (`11-live-link.md`); an
earlier edit of the file cannot be undone under unsaved changes, and the message says so.

## 13. With the editor

The routing is the binary's; the protocol is `11-live-link.md`. `load` prefers the editor's copy
when the map is open there with unsaved changes. `edit` runs in the editor whenever the map is
open there, unless `--out` names another file; it lands as one undo step and does not save, and
the report says so with `bt save` and `bt undo` as the next moves; `--in-editor` and `--mark`
fail instead of writing the file when the map is not open; `--dry-run --mark LABEL` outlines the
cells an edit would change without changing anything. An edit of a file the editor has open with
unsaved changes that `bt` cannot reach is refused unless `--force`, which leaves the editor
asking whether to reload. `editor`, `mark`, `open` and `save` are the live link's commands;
`render --marks` draws the editor's marks on a file render, so a plan can be checked while the
editor's window is hidden.

## 14. The guide

`bt guide` prints one document, `guide.md`, the only text a fresh agent gets. It holds the
conventions, how to work (look, change, check, undo), working beside a person in the editor,
the Look and Know commands, maps that are not square, the script language with every operation
and the ways out of what `check` reports, pieces, sprites, objects, drawings, new maps, one
complete example and the Godot commands. It is written for a reader who acts on it: short
sections, every command shown in the form it is typed, every behaviour stated once. Two tests
keep it honest: every command and option the guide names exists in the `clap` table, and every
command and option in the table appears in the guide; and the guide's example script runs on a
fixture map and `check` is clean afterwards.

## 15. Design rules, as kept

- **Tooling, not answers.** Primitives an agent combines; no generator for a particular thing,
  no "where should X go", no guessed edge rules for hand-placed tiles. Judgments in output are
  removed when found: `check` lists the odd ones by id and leaves the verdict to the reader.
- **Problems before summaries.** Anything an agent should fix is a `Problem` line; a `Problem`
  names where (layer, cells, ids) and what; what to do about it is in the guide.
- **Names, feet and shapes.** No tile ids where a name exists; objects stand where their feet
  are; areas are shapes narrowed by grounds.
- **All or nothing.** A script applies whole or changes nothing; a failure names its line.
- **`bt` never writes a tileset file.** Names that should last go in a `--with` file.
- **The same report for every door.** Outcomes are typed; text and JSON are renderings.

Deliberate limits: `place … rect` is for orthogonal maps (sprites elsewhere go by their feet);
`move … to` moves one object; `reach` knows nothing of gravity or doors (the agent says which
cells count); a definition lasts one script unless it is in a `--with` file.

## 16. Tests

| Test | What it pins |
|---|---|
| Parser | every operation reads as written, including quotes, comments, `#12` ids, `seed` per line, `rough`, `width` and `margin` binding, `draw` blocks; mistakes say the line and what was expected, with "did you mean" |
| Shapes | cells whose centres lie inside; thin paths stay 4-connected; `inside`, `avoid margin`, `near`, `minus`, `containing` on a synthetic map; connection per orientation, including the diagonal sides of staggered maps |
| Names | slugs, terrain qualification and uniqueness, sprites from collections without animation frames, tile ranges, suggestions |
| Feet | the foot of a drawn tree is the middle of its trunk; the shadow below does not count; standing cells at edges |
| Runner | on synthetic tilesets with terrains: painted terrain covers exactly the cells and fits around; what the set lacks is said; pushed-back and replaced terrains are named; sprites stand where put and `scatter` keeps its distance and fits groups; objects, `set`, `like`, `last`, `move`, `delete … on`; pieces placed and scattered by their foot; `resize` reports; `draw` order; mistakes change nothing |
| Reports | outcomes asserted as values; `--json` round-trips through its schema; a few snapshot texts |
| Check | seams, crowded sprites, opaque edges over a path, floating objects, project types, orphans; `made_by` after an edit |
| Files | read-only files and folders refused with the right message; unsaved changes in the editor never written over; undo puts back the bytes and the record, refuses a changed file, honours `--force` |
| Tiled's examples | sewers (rules beside the map, AutoMap layers started by a script, chained layers), isometric, hexagonal and staggered maps, the desert, perspective walls, sticker-knight's object-only sandbox: the scripts of the guide's kind run and `check` is clean |
| The packs | a chamber built by script on the Highlands pack reads as it looks in `view`; the AutoMap rules reach their output through bindings; AutoMap layers keep what was painted and the file gets what the rules make |
| Guide | every command and option named in the guide exists and vice versa; the example script runs |
| Agents | a manual gate before a release: fresh Opus agents, one brief and `bt guide`, on each kind of map above; results checked with `check` and renders, never by the agent's own report |

## 17. As built at M4

M4 built `bt-script`, `apps/bt` and the editor's side of §13 (`11-live-link.md`, §14). Every
command of §4.1 runs but `godot`, which comes with the Godot export (M5). The decisions taken
while building are in `decisions.md` (2026-10-11: the script language and its reports; names,
shapes and feet; running scripts; looking and knowing; `bt edit` and edits through the live
link; hidden edges).

`bt-script`, module by module:

| Module | What it holds |
|---|---|
| `report` | `Report`, `Section`, `Line` (`problem:`, `warning:`, `note:`), `Outcome` (a `SCHEMA` like `bt.check/1`, and `report()`), `to_json`, `to_json_pretty`, `from_json` |
| `words`, `tally`, `property_text`, `layer_tree`, `rule_words` | the words reports share: cells, regions, points, counts capped with "and N more", values as scripts write them, layers top first, rule maps' layer names and near misses |
| `script/` | the parser: `parse_script`, `parse_definitions`, `split_do`, `Op` and its parts, `Located`; `parse_area`, `parse_cell`, `parse_region`… for command-line shapes |
| `names`, `context`, `error` | `Names` (§5), `Context` with `Selection` and `last`, the crate's `Error` with `NameKind` |
| `grid`, `shapes`, `feet`, `ground` | sides and floods per orientation; `area_cells`; feet and standing cells; `GroundUnder`: what an object stands on |
| `run/` | §8: `RunInput::builder(map, path)` (seed, selection, AutoMap layers, definitions, rules read, view) and `run_script` giving `EditResult { painted, shown, setup, layers, setup_changed, outcome }` or `RunError`; `defined_names` for `--with` |
| `changes/`, `edit_report`, `diff` | `MapChanges::between`; `EditOutcome` (`bt.edit/1`) with `Saved`; `DiffOutcome` |
| `check`, `hidden_edges`, `opacity` | `CheckOutcome` and `Problem`; `made_by`; opaque edges over other ground; `EdgeMeter` |
| `info`, `at`, `objects`, `view` (+`symbols`, `view_ids`, `view_corners`), `catalog` (+`catalog_sprites`, `catalog_tiles`, `sheet`, `tile_pictures`), `regions`, `explain_text`, `create`, `picture` | the Look and Know outcomes of §10 |
| `export`, `resolved/` | `ExportOutcome`; the resolved JSON (`ResolvedRequest`, `RESOLVED_FORMAT` "bt resolved map 1", `RESOLVED_SCHEMA`) |
| `history` | the versions `bt undo` restores (§12) |

`apps/bt`: `cli` (the table), `run` (a command's arguments, folders and the editors, found once
when first needed), `exit` (`Status`, `Printed` with a failure for a command that did part of what
was asked), `output`, `folders`, `load` (`LoadedMap`, `MapCopy`), `link` (`Link`, `OpenMap`),
`script_input` (a script's input from a loaded map; the names of `--with`), `record` (a map's
AutoMap record: written by `edit`, kept for and put back by `undo`, taken out by `new --force`),
`picture_file` (PNGs written atomically over the one there), `commands/*` and `guide.md`.

Where it differs from the text above, or says more:

- **Dependencies.** `bt-script` also uses `glam`, `serde`, `serde_json` and `thiserror`.
  `apps/bt` depends on `bt-script`, `bt-live`, `bt-tmx`, `bt-render`, `bt-image` (pictures),
  `bt-automap` (a map's rules for `info`, `explain` and AutoMap layers), `bt-project` (AutoMap
  records), `bt-core` and `bt-utils`; `bt-godot` comes with M5.
- **§4.2.** Usage errors include `bt edit`'s: several maps without `--do`; `--out`, `--render`
  or `--mark` with several maps; a script on a terminal. `bt new` over a file without `--force`
  is a refusal (exit 3, `bt.refused/1`). A command that did part of what was asked (`bt edit`
  of several maps) prints what it did, then the failure, and exits 1.
- **§4.3.** A map with unsaved changes in the editor comes with what was painted under its
  AutoMap layers, rebuilt from the record the editor sends with the map (§13). The project file
  a map's AutoMap record is written to is the one found above the map, else the one next to the
  `.tiled-project` that applies.
- **§8.** As `decisions.md` says ("Running scripts as built"): a builder and one call instead of
  `Runner::new` and `run(op)`; the runner fills the problems the edit made and the view, so both
  doors report the same.
- **§9.** `EditOutcome` also holds the editor's sentence for `--mark` and the picture of
  `--render`; with `--json`, several maps print an array.
- **§10.** Pictures of grids other than orthogonal are framed on the region's own cells and keep
  the shifted rows shifted when the region starts on an odd row or column. `bt render --marks`
  draws each mark in the colour the editor drew it in. `bt explain` without rules named takes
  the `rules.txt` beside the map, else the project's rules file; rules named are found from the
  map's folder. `bt export MAP OUT.png` draws the whole map at its own size without rulers.
  `--resolved` refuses an oblique map with exit 1 and leaves AutoMap's rule tiles out, with a
  warning; it applies Tiled's export options itself (`bt-tmx` keeps its own pass to itself).
- **§11.** Seams on staggered maps pair corners as Tiled's own filler does, so the staggered
  isometric example lists 110 seams, which the picture shows as grass tips at diamond corners.
- **§13.** `bt edit --render` asks the editor for the edited map as text; `--mark` adds its mark
  to the map's marks. In the window the script runs on the connection's thread
  (`11-live-link.md`, §14).
- **§14.** The guide's example test builds the kind of map the example was written for from the
  Highlands pack (mirrored into a scratch folder; `bt new` with the pack's terrain, platform and
  sprite tilesets), runs the example's commands as written, and requires the edit to make
  nothing `check` points out and `check` to be clean.

Tests: `bt-script` 156 unit tests and 40 integration tests (`tests/runner_*.rs` on synthetic
tilesets, Tiled's examples and the Highlands pack; `tests/reports.rs`); `apps/bt` 51, every
command on fixtures and Tiled's examples, `bt edit` on files and against a stand-in editor, the
guide's tests; the end-to-end tests of the live link are in `apps/better-tiled`.
