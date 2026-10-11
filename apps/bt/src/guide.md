# Working with maps from the command line

`bt` reads and edits Tiled maps (`.tmx`) without the editor. The files it writes are the same
files the editor (better-tiled) and Tiled use. This guide (`bt guide`) is all you need: every
command and option is in it. `bt help COMMAND` prints one command's options.

## Conventions

- **Cells** are `X,Y`: whole numbers, counted from the top-left, x to the right and y down.
- **A region** is written `X,Y WxH`, e.g. `4,2 10x6`, or `X1,Y1..X2,Y2` with inclusive corners.
  Options take `X,Y WxH` too, or `X,Y,WxH`.
- **Points** (`at`) are in tile units:
  - `10,5` is the top-left corner of cell 10,5.
  - `10.5,5.5` is its centre.
  - `10.5,6` is the middle of its bottom edge.
- **Where things stand**: an object is in the cell its point is in.
  - A sprite's point is its foot, where its image touches the ground (a tree's trunk), not a
    corner of its image.
  - A point on the bottom edge of a cell is in that cell, so `at 10.5,6` stands in cell 10,5.
- **Names, not numbers.** Layers, terrains, sprites and object types go by name, and
  `bt catalog` lists them.
  - Names are lowercase words joined by `-`, e.g. `frozen-lake`.
  - Case and spaces don't matter, and an unknown name gets the closest matches.
  - Two terrains of one name are told apart by their set (`water/deep`), then their tileset.
- **Tiles** are `TILESET:ID`; several of one tileset `TILESET:ID,ID,…`, a range
  `TILESET:A-B`. `bt catalog MAP tiles` lists every tile with where it is used. Tiles that
  have a class in their tileset also go by its name, and a script can name tiles:
  `define water = tile beach-tileset:38,40-42`.
- **Layers** go by name, or `#ID` when two have one name.
- **Objects** are referred to as `#ID`, by name, or by type when only one object has that type
  (`move entry to …`). `selected` stands for the objects selected in the editor.
- **Read-only files** (locked by version control) are never written over. The commands that
  write fail with "the file is read-only" (or "its folder can't be written to") and exit 1.
  `bt` never writes a tileset file.

## Output

Every command prints lines of four kinds. Act on the first:

    problem: walls: a seam at 12,4 between water and grass
    warning: reading the map: …
    note: the rules ran on what was painted
    12 cells changed

- `problem:` lines are things to fix. Each says where (layer, cells, ids) and what; how to fix
  each kind is in this guide (`bt check` below).
- `warning:` lines may keep the output from being what it seems.
- `note:` lines say how a request was carried out.
- The rest are facts. Long lists end with "and N more".

`--json` on any command prints the whole outcome as JSON instead, with a `schema` field naming
its shape (`bt.check/1`, …). `--project FILE` on any command names the `.tiled-project` whose
custom types apply; without it, the nearest one above the map does.

Exit codes: 0 done, 1 failed (the reason follows `bt:` on standard error, and nothing was
written), 2 wrong usage, 3 refused, 4 out of date, 5 problems found (`bt check`).

## How to work

1. **Look**: `bt info MAP`, `bt catalog MAP`, `bt view MAP`, and `bt render MAP out.png`
   (then look at the picture). `bt areas` and `bt reach` say how cells connect.
2. **Change**: write a script and run `bt edit MAP <<'EOF' … EOF` (or `bt edit MAP SCRIPT`, or
   `-` for standard input), or a line or two with `--do`, operations split by `;`:

       bt edit MAP --do "paint water ellipse 12,8 5x3 rough 0.2 on grass layer ground; scatter tree in where grass density 0.05"

   With `--do`, several maps can be given (`bt edit A.tmx B.tmx --do "layer add sky tiles"`): the
   operations run on each in turn, and the first that fails stops the rest.

   A script is all or nothing: if a line fails, it says which and nothing is written.
   - It reports what changed on each layer, what was painted apart from what rules made, and
     the cells no tile fits.
   - It names the problems the edit made that `bt check` would list (seams, hidden edges,
     objects outside the map, on one spot or over nothing, types and properties the project
     doesn't define) and the objects it put on other ground, or says there are none; `bt check`
     lists everything.
   - `--view` adds the changed area as text (`--by tile`, `terrain` or `layer`, and `--runs`,
     as in `bt view`).
   - `--render out.png` adds a picture with the change outlined (`--region`, `--scale` as for
     `bt render`).
   - `--dry-run` reports the result without writing it.
   - `--out OTHER.tmx` writes the result there instead (a copy to look at or try things on).
   - `--seed N` sets the randomness of the whole script (scripts with the same seed give the
     same map).
   - `--force` writes a map an editor holds unsaved changes to that bt cannot reach; the editor
     then asks the person whether to reload.
3. **Check**: `bt check MAP` lists problems:
   - reading warnings and missing files;
   - terrain seams, and opaque edges lying over anything but whole ground;
   - objects outside the map, on top of each other, or over nothing (when the rest of their
     layer stands on something);
   - objects whose type the project doesn't define, or with properties their class lacks.

   Then what objects stand on and what zones cover, layer by layer: the few that stand on
   other ground than the rest of their layer are listed by id (trees in a lake, say).
   `bt check` exits 5 when it lists a problem.
4. **Undo**: `bt undo MAP` takes back the last `bt edit`, one edit at a time. It refuses when
   the file was saved since (`--force` takes it back anyway).

**What to do about problems.**
- *A seam*: two terrains of different sets meet in one layer, or the set has no tile for that
  combination. Paint one of them on its own layer, or leave a terrain both border between them:
  two cells of it between their whole cells (`bt catalog MAP terrains` says what each borders).
- *An opaque edge over something*: the terrain's edge tiles draw their own ground and hide what
  is under them. Paint it `inside` ground that matches, or keep it away from what it hides with
  `avoid THING margin 1`.
- *An object over nothing, or outside the map*: move it (`move #ID to X,Y`) or delete it, or
  paint ground under it.
- *Objects on the same spot*: move one, or delete it.
- *An undefined type or property*: use a type `bt catalog MAP types` lists, or set the property
  the class has. Objects the AutoMap rules place (`bt check` says which) take their types and
  properties from the rule maps: they change with what the rules read, or in the rule maps.

## When the map is open in the editor

People work on the same maps in the editor, and you may be helping one of them. When a map is
open there, bt works with the editor's copy instead of the file:

- What bt reads (`info`, `view`, `render`, `check`, …) includes the changes not saved yet.
  `bt diff MAP` lists them.
- `bt edit` makes its change in the editor, as one step the person can undo, and doesn't save.
  `bt save MAP` writes the file: when they ask, or before the file is used elsewhere. `bt undo
  MAP` takes the edit back, unless they changed the map since. Add `--in-editor` when the edit is
  meant for the editor: bt then fails instead of writing the file if the map isn't open there.
- `bt edit MAP --dry-run --mark LABEL` outlines in the editor the cells the edit would change,
  without changing anything: a plan for the person to look at before you make it. `--mark LABEL`
  without `--dry-run` outlines what the edit changed.
- `bt editor` shows what the person is doing: the maps open, the one shown, its selected cells
  and objects, the tool and what it paints with, the cells in view and under the mouse, the last
  steps of its history, and the marks.
- In scripts, `selection` is the cells selected in the editor and `selected` the selected
  objects: "fill this with pines" is `scatter pine-tree in selection density 0.1`.
- `bt mark` outlines cells in the editor, with a label, to show the person something ("the
  bridge would go here"). The view moves to the marks unless `--no-focus`. A new mark replaces
  the map's earlier ones unless `--add` (marking a label again moves that mark); `--clear` takes
  them all away. Marks belong to their map, and bt never switches the map the person is looking
  at. `bt editor` lists the marks, and `bt render MAP OUT.png --marks` draws them, numbered: you
  can check them even when the editor's window is hidden.

      bt editor [MAP]                 what is open, selected and in view
      bt editor --screenshot OUT.png  also a picture of the editor's window, as the person sees it
      bt mark MAP SHAPE [--label TEXT] [--add] [--no-focus] [--with FILE]   (or bt mark MAP --clear)
      bt open MAP                     opens the map in the editor (starting it if needed)
      bt save MAP                     saves the map open in the editor

## Look

    bt info MAP                     size, layers (top first), tilesets, terrains, objects, properties
    bt view MAP [--region R] [--layers a,b] [--by terrain|tile|layer] [--runs] [--no-objects] [--step N] [--painted]
    bt view MAP --ids [--region R] [--layers a,b]   each cell's tile id, layer by layer
    bt view MAP --corners [--region R] [--layers a,b]   the terrain at each corner of the cells
    bt render MAP OUT.png [--region R] [--scale S] [--grid] [--clean] [--highlight SHAPE] [--layers a,b] [--marks] [--with FILE] [--painted]
    bt at MAP X,Y [--painted]       everything in one cell, layer by layer
    bt objects MAP [--type T] [--layer L] [--in R] [--name N]
    bt catalog MAP [terrains|sprites|types] [--find TEXT] [--sheet OUT.png [--scale N]]
    bt catalog MAP tiles [TILESET] [--sheet OUT.png [--scale N]]   every tile: where used, which aren't
    bt catalog MAP tiles TILESET:ID[,ID…]   those tiles: where, and which tiles are next to them on each side
    bt areas MAP SHAPE [--all] [--with FILE] [--seed N]   the connected areas of a shape's cells, e.g. each lake of where water
    bt reach MAP FROM TO [--through SHAPE] [--with FILE] [--seed N]   is there a way from FROM to TO through SHAPE's cells?
    bt explain MAP X,Y [RULES] [RULE-LAYER=MAP-LAYER …] [--seed N] [--run N] [--all]   what AutoMap does at a cell, and why
    bt diff MAP OTHER               what differs between two maps, layer by layer
    bt diff MAP                     the changes not saved yet, in the editor

`bt view` prints one character per cell, with column and row numbers. Lowercase letters and
signs are the ground; capital letters are objects, marked where they stand. A legend follows.

- By terrain (the default when the map has terrains), a cell shows the terrain covering more
  than half of it. A painted area therefore reads as the cells that were painted, and a cell in
  an inner corner of it too (three of its corners are painted).
- By tile (the default without terrains), each kind of tile has its symbol; by layer, each
  layer has one.
- A symbol keeps its meaning in every view of the map, before and after edits. When there are
  more kinds than symbols, the rest show as `?`, and `--ids` shows each cell's tile id instead.
- `--runs` adds each row's runs, e.g. `~14-29`, so you don't have to count columns.
- Maps wider or taller than 160 cells are shown one character per square of cells (`--step N`
  chooses the square). Use `--region` to see detail.
- `--corners` shows what terrains are painted on: the corners of the cells (corner X,Y is the
  top-left corner of cell X,Y). It shows how wide a gap or a strip really is: two cells that
  look like a gap in a wall may have wall on all but one corner between them.

`bt render` numbers the cells along the top and left and draws a grid line every 5 cells
(`--grid`: every cell; `--clean`: neither). It is at most 2048 pixels a side unless `--scale`
says otherwise (up to 8); a small map is drawn up to 4 times larger, so its cells can be told
apart. `--highlight` outlines cells: a region, or any shape as in scripts
(`--highlight "where water containing 12,10"`; with `--with FILE`, the names it defines too); a
shape with no cells is an error. `--layers` draws only those layers. `--marks` draws the marks
the editor has on the map, so the map must be open there.
`bt catalog --sheet` draws the listed terrains and sprites with their list numbers; `bt catalog
MAP tiles --sheet OUT.png` draws every tile with its id under it, laid out as in the tileset, and
outlines the ones the map uses (`tiles TILESET:A-B`: those alone; `--scale N`: bigger). For tiles
placed by hand (shores, walls without terrains), `bt catalog MAP tiles TILESET:ID` says which
tiles the map has next to that one on each side.

**How cells connect.** Cells that share a side are connected: four sides on orthogonal and
isometric maps, six on hexagonal ones, four (the diagonal neighbours) on staggered ones. Shapes
are written as in scripts, and which cells count is yours to say.

- `bt areas MAP where water` lists each connected area, largest first: its size, where it lies,
  which edges of the map it touches and a cell in it (`--all`: every area). In a script,
  `where water containing X,Y` is the area with that cell.
- `bt reach MAP 3,4 30,20 --through "all minus where wall"` says whether a way leads from one
  to the other, moving only through the cells of `--through` (the ends may be outside it), and
  gives the shortest, and the cells every way passes through (a single pass, a door). Without
  one, it says how far you get and what closes the way. It exits 0 either way, saying yes or
  no. FROM and TO can be shapes too (`"rect 0,0 20x2"`: any of those cells).

**Why AutoMap puts (or doesn't put) a tile somewhere.** `bt explain MAP X,Y` runs the AutoMap
rules on the map as it is (changing nothing) and says what the run does at that cell. It takes
the rules and the layer names like the edit step `automap` (`bt explain MAP 12,7 rules.txt
walls=ground`): rules named are found from the map's folder; without them, the `rules.txt`
beside the map, else the rules file the project names. On a map with AutoMap layers it explains
their runs, and takes no rules or names.

- *Changes, in order*: every tile written to the cell, by which rule. A rule is named by its
  rule map and number (the order AutoMap goes through them) with its cells in its rule map
  (`bt view RULEMAP --region …` shows them). "Matched at 10,7" is where the top-left of the
  rule's input lands on the map.
- *Matched, changed nothing here*: rules whose conditions hold but that wrote nothing, with why
  (their Probability left them out, a locked layer…).
- *Came close*: rules that write this cell from some position and have a condition about
  particular tiles that holds there. Each says what it would write here, and its conditions that
  don't hold: `walls 10,9 must be platformv2-3t:2, has empty`. The rest are counted; `--all`
  lists them.
- A tile at the cell that no rule names (painted with a tile the rules don't use) is said first.
- A condition sees the cell as it is when the rule is tried: earlier rule maps (and, with
  MatchInOrder, earlier rules) may have changed it, which the line says. Rules match on layers
  by exact name: a layer the map lacks reads as empty, and a near miss is pointed out (the map
  has a layer `Set`, not `set`).
- `bt info` on the map names what keeps rules from running (rule maps that can't be read, with
  why; inputs the map lacks or has empty); `bt info` on a rule map lists its warnings and the
  tiles that make no rule. The edit step `automap` names them too.
- It explains a run on the map as it is now. Rules that rewrite the layer they read find
  something else on a second run: explain before the edit step that runs them. Random choices
  follow `--seed`.
- On a map with AutoMap layers (below), it explains each of their runs on what it reads, in the
  map's layer names (`--run N`: just that one).

**Maps with AutoMap layers.** A map can have AutoMap layers: its rules run after every edit, on
the layers chosen for them (the platform rules on `plat1`, then on `plat2`…), and what was
painted is kept apart from what they make of it. They are started in the editor or with the edit
step `automap layers`; `bt info` says which layers the rules run on.

- `bt edit` edits what was painted, as the editor's tools do, and reports what changed on the
  map as the rules make it, with what the script painted under it. The map file gets what the
  rules make; `better-tiled.json` (next to the map or its `.tiled-project`, inside the map's
  repository) keeps what was painted. Tiled reads the map file as a finished map.
- An object the rules placed can't be edited: change what the rules read, or stop the layers
  (`automap layers off`).
- `bt view`, `bt render` and `bt at` show the map as the rules make it; `--painted` shows what
  was painted.
- The edit step `automap` has nothing to do on such a map (the rules run anyway). `bt undo` puts
  back what was painted along with the map.

## Maps that aren't square

On isometric, hexagonal and staggered maps, cells are still `X,Y` in a grid. What differs is how
the grid is drawn:

- `bt info` says how it sits on screen: on an isometric map, 0,0 is the diamond's top corner, x
  runs down-right and y down-left, and the edge at the lower right is the last column. Shapes
  are in cells, so a path that turns between x and y runs straight down the screen; to wind on
  screen it turns between `X+N,Y` and `X,Y+N` (right and left), not between rows.
- A path takes the cells whose centres lie within half its width of its line: `width 2` along
  cell centres (`X.5`) takes three cells, along cell edges two; turns take more.
- `bt view` prints the grid unturned. On hexagonal and staggered maps whose rows are shifted it
  shows the shift: two characters a cell, the shifted rows one character further right, so the
  cells that touch are the ones next to each other in the text.
- `bt render` outlines the cells as the map draws them and writes `x,y` inside every 5th cell.
  Highlights and marks follow the cells too.
- Shapes are in grid units: `rect`, `ellipse` and `path` take the cells whose grid position falls
  inside them, so on an isometric map a `rect` is a diamond on screen, and on a hexagonal map a
  `path` is a band of hexagons.
- Cells connect through their sides (six on a hexagonal map) and margins count the cells
  touching (see How cells connect).

## Edit scripts

One operation per line. `#` starts a comment. Words with spaces go in "quotes". Any operation
can end with `seed N` for its own random choices; a line `seed N` sets them for every line after
it. A comment on the first line names the edit (`# grow the lake`): the editor's history shows
that name.

**Where**: operations that change cells take a shape:

    X,Y                       one cell
    rect X,Y WxH              (or X1,Y1..X2,Y2, or just X,Y WxH)
    ellipse CX,CY RXxRY       centre in tile units, radii in cells; `rough R` makes the edge wander
                              by up to R of the radius (0.1 subtle, 0.3 ragged), for a natural shore
    circle CX,CY R
    poly X,Y X,Y X,Y …        the cells whose centres are inside
    path X,Y X,Y … [width W]  a line through the points, W cells wide (a river, a road)
    cells X,Y X,Y …
    all                       the whole map (an infinite one: the area painted so far, or its
                              saved size while it has nothing)
    where NAME[,NAME]         the cells all of that terrain now (its edge cells don't count), or
                              with one of those tiles (on any layer), or of a defined area
    under LAYER[,LAYER]       the cells the objects of those layers cover (a level of sprites)
    @NAME or @#ID             the cells inside an object (a zone); @selected: the selected objects'
    selection                 the cells selected in the editor

Narrow a shape with these. Their names are terrains, tiles (`hex-mini:10,14`) or tile and
area names:

- `on NAME,…`: keep the cells fully on one of them.
- `inside NAME,…`: like `on`, and the cells touching them too. Then the edges painted around
  the area also stay on that terrain: a lake that doesn't spill over the island's rim.
- `avoid NAME,…`: drop the cells they touch: their own, and the cells their painted edges
  reach. `margin N` drops N more rings of cells around those. `margin 1` keeps the edges painted
  around the new cells clear of them too: a path that stops short of a river instead of running
  under its banks, or a terrain painted beside one it has no tiles with (`paint cobblestone …
  avoid brick margin 1` leaves the two cells of sand they need between them).
- `near NAME,…`: keep the cells next to them; `margin N`: within N cells. `where plains near
  mountain` is the plains along the mountains.
- `minus SHAPE`.
- `containing X,Y`: keep only the connected part that includes that cell (after the rest):
  `where water containing 12,10` is one lake of several.

A margin counts cells that touch at a side or a corner (on hexagonal maps, the six around).
`rough` and `width` belong to the shape just before them (a `minus` shape too), `margin` to the
`avoid` or `near` before it.

Names for cells and tiles, for the rest of the script:

    define NAME = area SHAPE                 the shape's cells as they are on this line
    define NAME = tile TILE[,TILE…] [layer L]

    define lake = area where water containing 12,10
    define other-water = area where water minus where lake
    paint water path 12,10 25,9 width 1.5 avoid other-water margin 2   # joins the lake, keeps off the rest

A defined area or tile name works wherever a terrain name does in shapes (`where lake`, `avoid
lake margin 2`), and a tile name in `tile` and in drawings.

Definitions you use again go in a file: `--with FILE` runs its `define` lines (and `seed`)
before `bt edit`'s script, and gives `bt areas`, `bt reach`, `bt mark` and `bt render
--highlight` the same names (`bt reach MAP 3,4 30,20 --through "all minus where wall" --with
names.bt`). Names last one script otherwise; bt never writes them into a tileset.

A path's points are in tile units, like `at` (`10.5,5.5` is the centre of cell 10,5). It takes the
cells whose centres are within W/2 of the line, and a line of cells joined by their sides so it
never hangs together by corners alone; on a slant that can be a cell more than W. Its ends and
bends are round. Painted, it gets a fringe of edge tiles on each side, like any area.

**Terrain** (the tiles around fit themselves, transitions included):

    paint TERRAIN SHAPE [layer L]    the cells become that terrain (where it is already, its
                                     tiles are picked again: `minus where TERRAIN` keeps them)
    erase TERRAIN SHAPE [layer L]    the terrain is taken away, the edges around repaired
    define NAME = TERRAIN [layer L] + TERRAIN [layer L] …     a material: paint both at once

- A terrain goes on the layer where its set's tiles already are, else the only tile layer.
  Name the layer the first time (`layer L`).
- Paint the ground first, then what lies on it (lakes, paths) on their own layers, and use
  `on GROUND` so they stay on it.
- The tiles around a painted area become its edges: a painted 4x3 rect is 4x3 cells of
  terrain, with a fringe of edge tiles around it.
- Terrain corners are shared by neighbouring cells, so a single cell left out between painted
  cells fills in. To leave a gap, make it at least two cells wide.
- `bt catalog` says which terrains a terrain has tiles with ("borders"), and which it has none
  with. Painting one next to a terrain it has no tiles with pushes that terrain back: the fill
  changes the cells around to fit, and the report names them, with the tile that would have let
  it stay. A blob's own shape isn't a push. To keep both, leave a terrain they both border
  between them.
- When no tile fits a cell, it is left as it was, and the report says what the set lacks: "no
  tile fits 12 cells: water has no plain tile", "no tile of “desert” borders an empty cell"
  (paint a ground terrain first), or the pattern no tile has.
- A set treats the tiles of other sets as empty, so painting next to them puts its edges over
  them, and the report says where (the cells of another terrain set the fill replaced).
- Some terrains have opaque edges: their edge tiles draw their own ground where the terrain
  ends, like a frozen lake whose edges show snow. `bt catalog` says which. Paint such a terrain
  `inside` ground that matches, and keep its edges off what they would hide (a path on another
  layer) with `avoid PATH margin 1`.

**Tiles**

    tile TILE[,TILE…] SHAPE [layer L]               those tiles (picked at random when several;
                                                     a tile written twice comes up twice as often):
                                                     TILESET:ID, TILESET:A-B, a tile name; with
                                                     `layer L`, that layer, always; without, the
                                                     layer a tile name names, else the layer that
                                                     holds most of them, else the only tile layer
                                                     (bt asks when that's a guess)
    clear SHAPE layer L[,L…]                         empty the cells (or `layers all`)
    copy MAP REGION [layers L,…] to X,Y [objects]   copy cells (and the objects standing there,
                                                     on those layers) from another map (or `.`:
                                                     this one); a relative MAP is from this
                                                     map's folder

To swap tiles for others by what is there (each ground tile for its mountain), write a line per
pair: `tile hex-mini:10 rect 6,8 14x3 on hex-mini:1`.

**Rules**

    automap [RULES] [RULE-LAYER=MAP-LAYER …]         run AutoMap rules once (rules.txt next to the
                                                     map, or the file given), e.g. for cliffs;
                                                     walls=ground runs rules made for a layer
                                                     called walls on the layer ground; bt
                                                     explain says why a cell came out as it did
    automap layers [RULES] [RULE-LAYER=MAP-LAYER …]  AutoMap layers: from now on the rules run
                                                     after every edit, on what was painted
                                                     (walls=plat1 as above; outputs with no
                                                     layer get one, named in the report)
    automap layers add RULE-LAYER=MAP-LAYER …        the same rules once more, on other layers
                                                     (walls=plat2)
    automap layers seed N | off                      other random choices | stop them (what
                                                     the rules made stays, as painted tiles)

Before it runs, `automap` says what would keep the rules from doing anything: warnings of the
rule maps, input layers the map lacks (with a near-miss name, since names must match exactly) or
has empty; after it, how many matches it applied.

**Pieces**: tiles (and objects) that belong together on several layers, like a palm drawn as
tiles on two layers, a tower top, a bridge: take them from a map once, then place and scatter
them like sprites.

    define palm = copy . 36,19 3x4 [minus SHAPE] layers Over,Fringe [objects] [foot 37,22]
    place palm at 24,40                    its foot cell on 24,40
    scatter palm in where sand count 2 spacing 4

The foot is the cell that goes where you place it: by default the middle of the piece's bottom
row. Empty cells of the piece leave the map's as they are; `minus SHAPE` leaves cells out (a shell
beside the palm). Its tiles go on the layers of the same names, its objects on theirs (made when
missing). `scatter` puts it only where all of it is on the map.

**Sprites** (pictures placed by their feet; `bt catalog MAP sprites` lists them):

    place SPRITE[,SPRITE…] at X,Y [X,Y …] [flip | flip random] [layer L] [name N] [like OBJECT] [key=value…]
    place SPRITE[,SPRITE…] in SHAPE [jitter J]     one per cell, standing at its bottom middle
    place SPRITE rect X,Y WxH                       stretched to that box (orthogonal maps)
    scatter SPRITE[,SPRITE…] in SHAPE [density D | count N] [spacing S] [jitter J] [flip random] [layer L] [like OBJECT]

`like OBJECT` gives what is placed that object's type and properties (gems like the gems
already there); `bt catalog MAP sprites` says what all copies of a sprite have in common. The
key=values are those `set` takes: `type=coin`, and `size=WxH` stretches a sprite around its foot.

`scatter` picks cells at random (15 of 100 by default) and keeps feet at least S cells apart
(1 by default), existing sprites included. It reports when there was no room for all. `jitter`
moves feet off the middle of their cells' bottom edges by up to J cells (0.35 for `scatter`, 0
for `place`). A sprite goes on the layer where that sprite (or its tileset's sprites) already
is, or say `layer L`.

Sprites that belong together, like a tree and its shadow, can be placed as one:

    define big-pine = sprite pine-tree-big-snow layer props + sprite pine-tree-shadow-big at 0,1.22 layer shadows
    scatter big-pine, small-pine in where snow-ground density 0.05 spacing 3

`at DX,DY` is from the first sprite's foot (mirrored when flipped). `scatter` places a group
only where all its sprites stand in the area, so a shadow doesn't land off the edge.
`bt catalog MAP sprites` says which sprites a map usually pairs, and at what offset ("usually
with …").

**Objects** (markers, zones: spawns, exits, hazards):

    object TYPE at X,Y [name N] [layer L] [like OBJECT] [key=value…]   a point
    object TYPE rect X,Y WxH | ellipse CX,CY RXxRY | poly X,Y … | path X,Y …   a shape (tile units)
    object TYPE in SHAPE [count N] [spacing S] [key=value…]            points at random cells
                                                  (e.g. in circle 22,26 5 on snow avoid ice)
    set OBJECT … key=value…        (name=, type=, visible=, rotation=, size=WxH set the object
                                    itself; an empty value removes a property; size keeps a
                                    shape's top-left corner and a sprite's bottom-left)
    move OBJECT to X,Y             (a sprite's foot, a point, a shape's top-left corner)
    move OBJECT OBJECT … by DX,DY
    delete OBJECT …  |  delete objects in SHAPE [type T] [layer L]
    delete objects on TERRAIN[,…] [type T] [layer L]   those standing on it, as bt check sees it

OBJECT can be `selected`: the objects selected in the editor (`move selected by 1,0`), or
`last`: the objects the last operation that made objects made (`place gem at 3.5,4 7.5,4` then
`set last points=5`). `object TYPE … like OBJECT` takes another object's properties too.

Values keep the type the property has (or the project's type gives); otherwise `3` is a whole
number, `2.5` a number, `true` a yes/no, `#12` a reference to object #12, and anything else text.

**Layers and the map**

    layer add NAME tiles|objects [above L | below L]
    layer rename L NEW | delete L | hide L | show L | lock L | unlock L | move L above|below M
    resize WxH [shift DX,DY]       (the report says which layers the new cells are still empty
                                    on when the script ends, and which objects reach the old
                                    edges: walls, stretched ground; what else changed is said
                                    against the old cells where the shift moved them)
    property key=value …           map properties (an empty value removes one)

A locked layer refuses changes until `layer unlock L`.

**Drawing with characters**: each character stands for a terrain (or material), tiles, a sprite
or an object. `.` and spaces leave cells as they are. Rows start with `|`.

    draw 4,2
      # = snow-ground layer ground
      ~ = frozen-lake layer ice
      T = sprite pine-tree-big-snow
      S = object spawn wave=1
      |..####....
      |.##~~##T..
      |.##~~##.S.
    end

Tiles are `C = tile TILESET:ID [layer L]` (or `C = TILESET:ID`, or a tile name); `C = clear
layer L` empties the cells (a doorway in a wall). Terrains are painted first, then cells are
emptied and the tiles go in as drawn, then sprites stand at the bottom middle of their cells and
objects at the centre of theirs. A drawing has no `on`, `inside` or `avoid`: it paints what it
shows. Rows reaching outside a finite map leave those cells out, with a note.

## New maps and other formats

    bt new MAP --like OTHER.tmx [--size WxH] [--force]        same tilesets and layers, empty
    bt new MAP --size WxH --tile WxH [--tilesets A.tsx,B.tsx] [--layers ground:tiles,props:objects] [--force]
    bt export MAP OUT [--embed-tilesets] [--detach-templates] [--resolve-types] [--minimize]
    bt export MAP OUT.json --resolved
    bt export --schema

- `bt new --like` keeps the other map's tilesets and layers, emptied, and says which layers
  were filled with one tile (a backdrop to fill again). An existing file needs `--force`:
  without it bt refuses (exit 3) and writes nothing.
- `bt export` writes the map (or a tileset) in the format OUT's extension names: `.tmx`, `.tmj`
  or `.json` (Tiled's JSON), `.js`, a tileset as `.tsx`, `.tsj` or `.json`, and `.png` a picture
  of the whole map at its own size. The options are Tiled's export options.
- `--resolved` writes the map for other programs as plain JSON (OUT ends in `.json`): no global
  ids, flips as yes/no, templates and inherited properties written out, tiles with their
  terrain and sprite names, every cell's centre and every object's screen position and outline,
  paths relative to the JSON file. AutoMap's rule tiles are left out, and oblique maps are
  refused. `bt export --schema` prints its JSON Schema.

## Example

    bt new c9.tmx --like c2.tmx
    bt edit c9.tmx --render c9.png <<'EOF'
    # a frozen lake on a platform
    seed 9
    tile terrain-highlands:64 all layer sky
    paint platformv2-3tiles-tall ellipse 22,15.5 18x11.5 rough 0.1 layer ground
    automap "../TiledMap Editor/rules.txt" walls=ground   # the pack's rules are made for a layer called walls
    paint frozen-lake ellipse 22.5,12.5 8.5x4.5 rough 0.2 inside platformv2-3tiles-tall layer ice
    paint terrain1 path 22.5,30 22.5,22 14,20 10,13 10.5,5 width 2 on platformv2-3tiles-tall layer dirt
    scatter pine-tree-big-snow, pine-tree-small-snow in where platformv2-3tiles-tall avoid frozen-lake, terrain1 density 0.07 spacing 2.5
    object entry at 22.5,30
    object spawn at 15,19.5 enemy=golem wave=1
    EOF
    bt check c9.tmx

The editor (`better-tiled MAP`) opens the same file.
