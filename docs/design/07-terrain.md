# Terrain — `bt-terrain`

*2026-10-09. The design of the crate that paints and fills terrain with Tiled's terrain sets
(Wang sets): the prepared set, the solver, the brush geometry, the diagnostics that say why a
cell could not be filled, and the labelling helpers (autotile layouts, auto-labelling, the
patterns a set needs, RPG Maker autotiles). The high-level design (`00-high-level-design.md`,
§10.1) states the goal: Tiled's results, with every failure explained. Tiled's own sources are
cited as `reference/tiled/src/...`.*

## 1. Scope

`bt-terrain` ports Tiled's `WangFiller` and `WangBrush` (`reference/tiled/src/tiled/wangfiller.cpp`,
`wangbrush.cpp`, GPL-2.0-or-later, used under GPL-3.0-or-later) and the parts of libtiled's
`WangSet` the solver needs. It works on one tile layer of a map at a time and returns what it
would change; it holds no UI state, keeps no history and knows nothing about the editor. Around
the solver it holds what makes terrain sets usable: the reasons a cell stays red, the autotile
layouts that label a whole block at once, the patterns a set still lacks, and the expansion of
RPG Maker autotiles into a set Tiled can use.

Dependencies: `bt-core`, `bt-utils` (`rng`, `bands`, `raster`, `hash`), `bt-image` (layout
detection and auto-labelling read pixels; RPG Maker expansion writes an image).

## 2. Terms

| Term | Meaning |
|---|---|
| terrain set, Wang set | the colour labels of a tileset's tiles on their corners and edges, with their colours' names and probabilities |
| colour, terrain | one label; a 1-based index into the set's colours; 0 is "no terrain" |
| lane | one of the eight slots of a tile: Top, TopRight, Right, BottomRight, Bottom, BottomLeft, Left, TopLeft; odd lanes are corners |
| kind | corner, edge or mixed: which lanes the set uses; a mixed set's colour may be used on corners, edges or both |
| candidate | a tile the solver may place, possibly flipped or turned when the tileset allows it, with the Wang id of that variant |
| hard constraint | a lane a cell must have (painted, or shared with a placed tile); a tile that disagrees is out |
| soft preference | a lane a cell would like to keep (its current tile, its surroundings); disagreement costs a penalty |
| penalty, colour distance | how many tiles it takes to get from one colour to another; a transition no tile makes is unreachable |
| corrections | the brush's re-solving of neighbours outside the painted area whose tile no longer fits |
| lookahead | the fill's check that a candidate leaves every unplaced neighbour some tile |
| invalid cell | a cell no tile fits; left unchanged and reported with what it needed |

## 3. Module map

| Module | Holds |
|---|---|
| `terrain` | `Terrain`: a prepared set (§4) |
| `request` | `PaintRequest`, `FillRequest`, `BrushTarget`, `Report`, `Preview` (§5) |
| `filler` | the solver (§6) |
| `brush` | hit testing, `Stroke`, lines, rotational symmetry (§7) |
| `topology` | neighbours on square, staggered and hexagonal grids |
| `wangid` | lane arithmetic on packed `u64` Wang ids, masks per kind, transforms |
| `diagnose` | `Needed`, `Cause` and how they are found (§8) |
| `feedback` | the sentences: what a set lacks, what a stroke pushed back or painted over (§8.2) |
| `layouts` | the autotile layouts, detection in a tileset image, labelling a block (§9) |
| `autolabel` | one terrain labelled from a reference tile (§10) |
| `patterns` | the combinations a set needs and which it has (§11) |
| `rpg_maker` | RPG Maker A2 autotiles expanded into a blob set (§12) |

## 4. The prepared set

`Terrain::new(map, slot, set_index)` (or `from_tileset`) turns a Wang set into what the solver
needs, once per tileset version; the editor caches it by the tileset's `Arc` and the set index
and rebuilds it when the tileset changes. It holds no reference to the map.

- **Candidates.** Every labelled tile in tile-id order, then the flipped and turned variants
  the tileset's `Transformations` allow, enumerated as Tiled enumerates them (three quarter
  turns, then horizontal mirrors of those, then vertical mirrors), each with the transformed
  Wang id masked by the kind. With `prefer_untransformed`, a variant whose Wang id a plain tile
  already has is left out. The order is deterministic, so every seeded choice is.
- **Weights.** A candidate's weight is the tile's probability times the product of the
  probabilities of the colours on its lanes; a weight of zero or less means "never placed", as
  in Tiled.
- **Colour distances.** Two colours are adjacent when a tile has both on corner lanes or both on
  edge lanes (corners and edges do not connect); the matrix is relaxed to shortest paths;
  `max_distance` is the longest finite one and the brush's correction radius. `penalty(from,
  to)` is the distance, or "unreachable".
- **Kind per colour.** `brush_kind(colour)` is Tiled's `effectiveTypeForColor`: on a mixed set a
  colour used only on corners paints as a corner set, only on edges as an edge set, else both.
- **Wang id of a cell.** `wang_of_cell(cell)` composes the tile's label with the cell's flips as
  Tiled does (the transpose is a quarter turn plus a horizontal mirror, then H, then V).
- **Completeness.** Tiled's `isComplete` decides whether fills run the lookahead; it is never
  true for corner and edge sets (its wildcard check looks at all eight lanes). The quirk is kept,
  because the lookahead's presence changes which cells become invalid, and the result must match.
- Labels that name a colour the set lacks, all-zero labels and labels of tiles the tileset no
  longer has are ignored, as Tiled refuses or never stores them.

## 5. Requests and results

```rust
let report = bt_terrain::paint(&mut map, PaintRequest::new(layer, &terrain, color)
    .targets(targets)            // what the brush touched
    .seed(seed)
    .mirror(map_size))?;         // optional 180° symmetry on finite maps

let preview = bt_terrain::preview_fill(&map, FillRequest::new(layer, &terrain, color)
    .cells(cells)
    .allow_empty(true)           // Tiled's bucket and shape fills; false for the Stamp tool in terrain mode
    .seed(seed))?;
```

- `BrushTarget` is `Corner(vertex)`, `Edge { cell, side }` or `Tile(cell)` (Tiled's "fill full
  tile"); `cells()` lists the cells a target constrains, for outlines.
- `paint` and `fill` edit the layer and return `Report { changed: Option<IRect>, invalid:
  Vec<IVec2>, needed: Vec<Needed> }`; `preview_paint` and `preview_fill` return `Preview {
  cells: Vec<PlacedCell { at, cell }>, invalid, needed }` without touching the map, and a
  preview with the same seed is exactly what the paint will do. `Cell::EMPTY` in a result
  erases the cell.
- Colour 0 erases terrain: the empty candidate and tiles whose lanes are "nothing" fit it.
- A fill with colour 0 is Tiled's fill: random terrain of the whole set that connects to the
  surroundings. A fill with a colour makes the filled cells prefer it softly, so the area is
  that terrain except where its border needs transitions; this is an addition to Tiled, whose
  fill ignores the chosen colour.
- A layer that is not a tile layer, or a set that does not exist, is a typed error.

## 6. The solver

The solver is Tiled's: greedy, one pass over the cells in region order, with hard constraints
propagated to neighbours and a correction queue for the brush. The steps, in order:

1. **Bounds.** On a finite layer, cells of the region outside the layer are marked resolved
   (empty) and dropped: they get no constraints and take no part in the lookahead. The region is
   split into Tiled's row bands (`bt-utils::bands`), which is the visit order.
2. **Soft border preference (fills only).** Each cell on a band's border takes, on its outward
   lanes that are not hard, the colours the tiles around the region show it
   (`wang_id_from_surroundings`: edges from the neighbour's opposite lane; corners from the
   diagonal neighbour, falling back to the side neighbours).
3. **Bounds of correction.** The region's bounding box grown by `max_distance` (one more on
   hexagonal grids), clipped to a finite layer.
4. **Resolve** each region cell: find the best candidate; if none, record the cell as invalid
   with what it needed (§8) and leave it; else place it, mark it resolved and give each of its
   eight unresolved neighbours the lanes they share (the opposite edge lane and, for a side
   neighbour, the two corners next to it). With corrections, a side neighbour outside the region
   but inside the bounds whose current tile now disagrees is queued, with its free lanes
   preferring its current tile so the change stays minimal.
5. **Corrections**: the queue is drained in batches, each cell resolved at most once, out to the
   bounds. Diagonal neighbours receive constraints but are only queued through a side neighbour,
   as in Tiled.
6. **Result**: the resolved cells and the invalid cells, row-major, each invalid cell with its
   first need.

**The best candidate** at a cell: among the candidates (plus the empty cell when erasing is
allowed) whose lanes agree with every hard lane, the penalty is the sum over soft lanes of the
colour distance from the wanted colour to the candidate's; an unreachable transition rejects the
candidate when corrections are on and costs `max_distance + 1` otherwise. The lowest-penalty
tier goes into a weighted picker. The picker draws without replacement with a generator derived
from the seed and the cell (`bt-utils::rng::cell_rng`), so the choice does not depend on the
visit order and a preview equals the paint. For fills, each drawn candidate must pass the
one-step **lookahead**: every unresolved neighbour must still have some tile that fits its
constraints once this candidate is placed (`wang_id_is_used`, cached per constraint); a
candidate that fails is skipped, and when every candidate of the tier fails the cell is invalid
even if a worse tier would have worked, as in Tiled. There is no backtracking.

## 7. The brush

- **Hit testing** (`hit_test(map, position, full_tile, kind)`): the pointer's tile and position
  inside it from `MapGeometry::screen_to_tile` (on hexagonal maps whole tiles, on staggered maps
  the position inside the diamond). Corner sets snap to the nearest vertex; edge sets pick the
  triangle of the tile split along both diagonals; mixed sets use a 3×3 grid where the corners
  snap to the vertex they touch, the sides are edges and the centre does nothing. Full-tile mode
  targets the tile.
- **`Stroke`** reproduces Tiled's drag: on corners the vertices along the line from the previous
  vertex, 4-connected; on edges the painted edge follows the drag direction so a drag draws a
  path through tile centres, staying inside a tile keeps the current edge; on mixed sets only the
  hovered target, no interpolation; in full-tile mode the tiles along the line, 8-connected.
  Each `drag_to` returns the targets to paint since the previous event; the editor paints them
  in one gesture.
- **Lines** (`line_targets(from, to)`) for Shift strokes from the last painted point, with
  mixed sets forced to tile mode as in Tiled; **symmetry** (`PaintRequest::mirror`) adds each
  target's 180° rotation around the map's centre on finite maps.
- **Topology.** On staggered and hexagonal maps a Wang tile is a diamond whose top faces the
  top-right neighbour; neighbours across a lane come from `MapGeometry`'s `top_left`, `top_right`,
  `bottom_left`, `bottom_right` and the stagger-axis-dependent diagonal offsets, as Tiled's
  `WangFiller` finds them, so painting works on every orientation Tiled supports.

## 8. Diagnostics

### 8.1 What a cell needed

A cell no tile fits comes back with `Needed { cell, lanes: [Option<u8>; 8], cause }`, the lanes
of the set's kind the cell required (`None` where anything would do):

| `Cause` | How it is found |
|---|---|
| `NoTile` | no candidate agrees with the hard lanes, or (brush) every candidate that does holds a colour with no way to the wanted one: the hard lanes plus those stuck lanes |
| `Neighbour(lane)` | the lookahead rejected every candidate of the best tier: the most frequent neighbour it rejected for, with the lanes that neighbour would have needed |
| `ZeroProbability` | a candidate fits best but its weight is zero: its lanes |

Tests check that every `Needed` names a pattern no tile of the set has, so the sentence built
from it is never wrong.

### 8.2 Sentences

`feedback` turns results into what the status bar and `bt edit` say, with a `name(colour)`
callback so the editor uses its panels' names and `bt` its own:

- `missing_text(set, name, needed)`: "no tile fits 3 cells: “Ground” has none with Sand at the
  top-left corner and Grass at the other three corners (and 1 other pattern)", or the plain
  reason when there is one: no plain tile of a terrain, no tile that borders nothing, no tile
  joining two terrains, a best tile with probability zero. Lacks are grouped and the commonest
  comes first.
- `pushed_back(map, terrain, colour, before, after, area, painted)`: which other terrain the
  stroke pushed back where it painted, with the tile that would have let it stay, on every
  orientation; `replaced_sets(...)`: cells of another terrain set the stroke painted over (a set
  treats other sets' tiles as empty), for the "paint each set on a layer of its own" hint;
  `fill_note` for a fill whose terrain has no plain tile.

## 9. Autotile layouts

Tile generators and engines lay out a terrain's tiles in a few well-known sheets; labelling a
block that follows one is one operation instead of 47 corner clicks.

- A `Layout` is text art: each tile is 3×3 characters after a `|` margin (`#` the terrain, `.`
  what it is drawn over, a space a lane the kind does not have; a tile of spaces is an empty
  slot), tiles one column apart; `size()`, `slots()` (`Slot { terrain: lanes bit set, middle }`),
  `tile_count()` and `labels(origin, columns, terrain, under)`, the Wang ids of a block whose
  top-left tile sits at a column and row of an atlas. A tile that is all one thing but for its
  middle (a lone blob, a lone hole) gets no label, since labels cannot tell it from a full tile.
- `LAYOUTS`, blob sets first: Blob 7×7 (cr31's 47 tiles, Tiled's manual and test set), Godot 3's
  3×3 minimal, Tilesetter's blob, Godot's 2×2, Tilesetter's Wang, the 3×3 island, the 3×3 hole,
  the 2×2 hole, Godot's 3×3 sides. The Godot and Tilesetter arrangements follow TileBitTools'
  templates (MIT, credited in the module).
- **Detection** (`Samples::new(image, tile size, columns, …)`, `detect(samples) -> Vec<Fit>`,
  `places(samples, index)`): mean colours sampled near each lane of each tile; a block fits a
  layout at an origin when, with the layout's labels, the terrain lanes and the under lanes
  separate in colour with enough contrast and at most one lane in thirty disagrees; a block
  whose terrain side is see-through is rejected (props over nothing look like islands); fits
  are ranked by tile count, then reading order. On the packs this was tuned against, no false
  fit appears and every real block is found.
- **Label All** (`recognize(side, known, least, among)`): once blocks are found, blocks that
  look alike get the same terrain and the same ground: colour histograms (4 bits per channel)
  compared by overlap, at least 0.8 for the same terrain and 0.5 for a ground seen before.

## 10. Auto-labelling

`autolabel::label_with(method, image, kind, tile size, reference, colour, tiles)` labels one
terrain from one full tile of it: each lane region of each tile is compared with the reference
tile's; `Transparency` takes the other side to be nothing, `Learned` learns a threshold from all
lanes' distances with Otsu's method on a log scale, and `Auto` picks between them from the
tiles. Tiles with no part of the terrain are left alone, and other terrains' labels are kept.

## 11. Patterns

`patterns::needed(set) -> Patterns`: the combinations of the set's colours on the lanes
of its kind that the set needs, with how many tiles have each. Not Tiled's "every combination
of every colour": "nothing" is included when a terrain is drawn over nothing, terrains that never
meet are kept apart (each autotile on its own, a desert's islands each with its sand), and a
blob counts 47, not 256, even with a stray label; groups with too many combinations are counted,
not listed. The tileset editor's Patterns view draws them with the missing ones highlighted and
lets a click label tiles with a whole pattern.

## 12. RPG Maker autotiles

An RPG Maker A2 floor is 2×3 tiles drawn in quarters the engine composes per neighbourhood;
Tiled draws tiles as they are. `rpg_maker::expand(sheet)` composes each autotile once into the
47 tiles of the Blob 7×7 layout (inner corners from the top-right tile, the frame from the 2×2
block below), writes `<stem> blob.png` beside the sheet, never over an existing file, and
`tileset()` makes a tileset with one mixed set and one terrain per non-empty autotile, drawn
over nothing. `autotiles(w, h, tile)` and `guess_tile(w, h, current)` read the sheet's shape;
`sheet_warning` recognises A1, A3, A4, A5 and B–E sheets by name or size and says they are not
A2 floors. Painting with the result gives the same cells and masks as Tiled's own blob set.

## 13. Determinism and cost

Every choice is a function of the seed and the cell; the editor keeps one seed per terrain
stroke and `bt` one per script, and "re-roll" changes it. A single brush dab previews in under a
millisecond on the packs' sets (a release-build test asserts it); a `Terrain` is built once per
tileset version.

## 14. Tests

| Test | What it pins |
|---|---|
| Synthetic sets | corner, edge, mixed and blob sets built in tests: transformations and `prefer_untransformed`, determinism across seeds, distance rings, fill semantics with and without empty cells, border preference, infinite layers, zero probability, every hit test and stroke on square and staggered grids, and the invariant that every `Needed` names a pattern the set lacks |
| Tiled's sample sets | `reference/tiled/tests/wangtiles` through the same scenarios with several seeds: every tile chosen is a lowest-penalty, positive-weight choice that passes the lookahead, every invalid cell has no such tile, and the same cells are resolved |
| Real sets | the purchased packs' terrain sets stay consistent on empty and on existing layers |
| Feedback | each `Cause` and each sentence on a set built to lack exactly one thing |
| Layouts | every layout's slot count and labels; the Blob 7×7 labels equal Tiled's `wangblob.tsx`; detection on the packs finds every block and nothing else; Label All groups alike blocks |
| Auto-label and patterns | labels from one tile on a known sheet; needed patterns for a blob, for a desert with islands, for terrains that never meet |
| RPG Maker | an A2 sheet expands to the tiles and labels of a known blob set; the other sheet kinds warn |
| Performance | a dab under a millisecond, in release builds |

## 15. As built at M3 (the solver, the brush, diagnostics and feedback)

Built on 2026-10-10 as §3–§8 design it, in the modules `color` (`TerrainColor`, the typed
terrain), `wangid`, `topology`, `terrain` (`Terrain::new`, `from_tileset`), `request`,
`filler`, `paint` (`paint`, `fill`, `preview_paint`, `preview_fill`, and `write_preview`,
which places a preview already computed so a caller previews once), `diagnose`, `brush`
(`hit_test`, `Stroke`, `line_targets`), `feedback` and `error`. What differs:

- `PaintRequest::mirror` takes a flag; symmetry asked for on an infinite map is a typed error,
  since such a map has no centre. `line_targets(from, to, full_tile, kind)` takes the two
  targets.
- A cell left unfilled and filled later in the same run (a correction queued twice) is not
  reported; Tiled's invalid region keeps it red.
- `missing_text` groups lacks by the sentence said of them, so cells with one plain reason
  count once. `pushed_back(terrain, before, preview, colour)` reports a terrain where a lane of
  it went to a third terrain, anywhere in the preview (the painted tiles first of all); the
  tile that would have kept it has the painted terrain where the stroke put it and the old
  terrain elsewhere. `pushed_back_text`, `replaced_sets` and `replaced_text` say the rest.
- The editor prepares a set per query instead of keeping it per tileset version: about 20 µs
  on the largest pack set, a small share of a frame.
- Tests: `tests/solver.rs` (the brush's corners, edges and whole tiles, previews that are the
  paint, seeds, fills with and without a terrain, finite edges and infinite layers, symmetry,
  each `Cause`, the feedback sentences); `tests/packs.rs` (every terrain set of the packs fills
  an empty layer with each terrain and is painted over with the others, the tiles placed
  together agreeing and every need naming a missing pattern; a dab previews in about 8 µs,
  asserted under 1 ms in optimised builds); and, as unit tests (`oracle_tests.rs`, with a
  decision trace the solver keeps in tests only), Tiled's seven sample sets checked with code
  of their own for every decision of fills and brush targets of every kind and four seeds.

## 16. As built at M3 (labelling helpers)

M3 built `layouts`, `autolabel`, `patterns` and `rpg_maker` as public modules of `bt-terrain`
(their functions are named for their module, `rpg_maker::tileset`), on crate-private
`lanes` and `pixels`, and `bt-image::write_png`. What exists:

| Module | What it holds |
|---|---|
| `layouts` | `Layout { name, about, kind }` with `size`, `slots`, `tile_count`, `labels`; `Slot`; `LAYOUTS`; `Samples::{new, from_tileset, size, side_colors, terrain_colors}`; `Fit { layout, origin, over_nothing }` with `area`; `detect`, `places`; `Histogram`, `recognize`, `SAME_TERRAIN`, `SAME_GROUND`; `KnownTerrains::{from_set, guess, label_all, terrain_count}`, `Block` with `labels` |
| `autolabel` | `Method::{Auto, Transparency, Learned}`, `label_with`, `Proposal { color, kind, method, tiles }` with `apply_to` |
| `patterns` | `needed(&Terrain) -> Patterns { listed, total }` with `present`, `missing`; `NeededPattern { pattern, tiles }`; `MAX_LISTED` |
| `rpg_maker` | `autotiles`, `guess_tile`, `sheet_warning -> Option<SheetWarning>`, `expand -> Result<Expansion, RpgMakerError>`, `tileset(&Expansion, name)` |

Where it differs from §9–§12, or says more:

- **Two more layouts.** The Epic RPG World packs draw every terrain as a 5×5 island with a bump
  on each side: "5×5 cross island, diagonals" (all 15 corner tiles, the two diagonal ones in
  its top corners) and "5×5 cross island" (13, without them). The packs' own labels follow
  them. `LAYOUTS` is blob sets, then corner sets, then edge sets, each the larger first.
- **Shapes.** Sizes and origins are `UVec2`; `labels` returns `BTreeMap<TileId,
  Option<WangId>>` (`None`: the tile gets no label) and leaves out slots past the atlas's
  columns; terrains are `TerrainColor`.
- **Detection** allows one lane in fifteen to disagree, not one in thirty (scattered paving
  stones spill over a few lanes); a block is no fit when two or more lanes of one side of a tile
  all disagree (a tile the layout needs is not there, such as an empty corner where a layout
  has a diagonal tile); a side is see-through below a mean alpha of one half (ragged edges reach
  into it); and on layouts of 64 lanes or more what the terrain stands on may have two looks,
  nothing and something else (a raised island's own wall, the water around it).
- **The packs** (`tests/layouts.rs` lists every terrain tileset of Ancient Ruins and Desert):
  the flat blocks are found but one, the raised ones of the walls, cliffs and platforms mostly
  too, and nothing else (three 2×2 holes the Desert draws as decorations count as found). Not
  found: rocky ground over rocky ground, whose two sides share their colours; islands whose top
  is see-through (the "-transp" walls and animated sheets, the Desert's transparent cliffs,
  Terrain2's frames: only a rim shows); raised islands whose top is two colours (walls 8 and 9,
  some platforms); and animated previews whose middle is a checkerboard of see-through tiles.
- **Label All** is `KnownTerrains::label_all` here, not in the editor: blocks over nothing get
  no ground; a block's ground is never its own terrain; terrains past the set's are new, for
  the caller to add. `guess` gives one block's terrains without learning them.
- **Auto-labelling** returns a `Proposal` of the tiles with some part of the terrain;
  `apply_to(labels)` applies it: the terrain's lanes follow it, other terrains' labels stay,
  tiles it leaves out keep their labels (a stale lane of the terrain there stays too), and a
  label left empty goes. `tiles` is `BTreeMap<TileId, IRect>`. On the packs' single-terrain
  sets it gives nine tiles in ten their hand-made labels.
- **Patterns** take the prepared `Terrain`, so a pattern's tiles count the flipped and turned
  variants the tileset allows, as Tiled marks them used (`Terrain` gained a crate-private
  `labels()`). The list stops at Tiled's 65,535; a group of more than 2²⁰ combinations is
  counted and listed as far as the list goes.
- **RPG Maker.** `expand(sheet, tile)` takes the tile size and refuses an odd one, a sheet with
  no whole autotile, one with nothing drawn and one of more than 254 autotiles, with a typed
  `RpgMakerError`. The image is written as a new file (`write_png` creates it only when nothing
  is there, `bt_image::Error::Exists` otherwise): `<stem> blob.png`, then `<stem> blob 2.png` and
  on. `Expansion`'s fields are read through methods, so only `expand` makes one. Terrains are
  "Autotile N", each in the mean colour of its full tile, which is its tile; the set's tile is
  the first one's. `sheet_warning` returns a `SheetWarning` whose `Display` is the sentence.
