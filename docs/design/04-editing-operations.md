# Editing operations — `bt-edit`

*2026-10-09. The design of the crate that holds the logic behind every editing command as
functions over `Map`: selections, fills, the brush, the clipboard, carried selections, map,
layer, tileset and object commands. The high-level design (`00-high-level-design.md`, §7) sets
the conventions; this doc says what each part does, the rules it follows (Tiled's, and where
not), and how it is tested. Tiled's own sources are cited as `reference/tiled/src/...`.*

## 1. Scope

`bt-edit` changes maps. It takes a `&mut Map` and the inputs an operation needs (a region, a
brush, ids, a delta), edits in place, and returns a report of what changed. It holds no UI state,
no history and no knowledge of files or pixels. The editor records its results as transactions
(`05-editor-core.md`); `bt-script` runs them from scripts; the tests run them on synthetic and
real maps. Terrain painting and automapping are their own crates and are not here.

Dependencies: `bt-core` and `bt-utils`. Most of the crate ports the behaviour of Tiled's editor
(`reference/tiled/src/tiled`, GPL-2.0-or-later, used under GPL-3.0-or-later); each module names
its sources.

## 2. Conventions

- **In place, with a report.** An operation mutates the map it is given and returns what it
  changed: rectangles per layer, ids made or removed, tilesets added. When nothing would change
  it leaves the map untouched and returns an empty report. Callers snapshot the `Arc<Map>` before
  calling; there is no undo logic here.
- **Typed errors for impossible requests.** A layer id that does not exist, a layer of the wrong
  kind, an unknown object, a tileset slot out of range: `Err(EditError)`, never a silent no-op.
  "Nothing to do" is `Ok` with an empty report.
- **Modes, not modifier keys.** Callers pass `SelectionMode::Add`, `SnapMode::Grid`,
  `FillShape::Ellipse`, `square: true`; the window decides what the keys mean.
- **Every orientation.** Anything that depends on the grid goes through `MapGeometry`:
  neighbours, stamp fitting on staggered rows, hexagonal turns, snapping, tile-to-pixel offsets,
  the lattice of carried tiles. No function is "orthogonal only".
- **Randomness is the caller's.** Operations that choose take a `&mut Rng` (`bt-utils`), and
  per-cell choices derive from `(seed, position)` so a preview equals the paint and does not
  depend on visit order.
- **Locks.** Painting, erasing and lifting skip locked layers (ancestors included), as Tiled
  does, and say so in the report; every other lock decision is the editor's, because it has the
  reason to show.
- **Ids.** New layers and objects take `Map::next_*_id`; references between objects are rewired
  when objects are copied or duplicated (§11).

## 3. Module map

| Module | Holds |
|---|---|
| `error` | `EditError` |
| `report` | the report types: `LayerChange`, `PasteReport`, `EraseReport`, `ResizeReport`, `RemovedTileset`, `ObjectsReport` |
| `region` | `TileRegion`, `SelectionMode` |
| `select` | flood fill, magic wand, select same tile, invert, the fill area rule |
| `shapes` | lines, ellipses and the rectangle and ellipse shapes of Shape Fill and Rectangular Select |
| `brush` | `Brush`, `Variation`, `BrushLayer`, `BrushMode`, placement, fill, random cells, transforms, stagger fitting |
| `clip` | `TileClip`, copy, cut, erase, paste, target layers |
| `float` | `FloatingTiles`: tiles carried by a selection |
| `map_ops` | resize, crop, autocrop, offset, finite and infinite |
| `layer_ops` | new, duplicate, remove, move, raise and lower, group and ungroup, merge down, via copy and cut, visibility and locks, offsets |
| `tileset_ops` | find, add, unify, usage, remove, replace, swap tiles |
| `adjust_tiles` | re-indexing after a tileset image changed its column count |
| `objects/` | `find`, `edit` (add, duplicate, remove, move), `order`, `bounds`, `hit`, `transform` (flip, rotate), `align`, `snap`, `convert` (to polygon, reset tile size) |
| `object_clip` | `ObjectClip`, copy, cut, paste with reference rewiring |
| `ids` | id allocation and `IdRemap` |

## 4. Regions and selection tools

`TileRegion` is a set of cells stored as sorted, disjoint horizontal runs per row: `insert`,
`remove`, `contains`, `union`, `subtract`, `intersect`, `xor`, `clipped(rect)`, `translate`,
`bounds`, `len`, `iter()` in row-major order, `runs()`, `rects()` (Tiled's `QRegion` banding,
through `bt-utils::bands`), `outline()` as closed loops of grid vertices, and `intersects`,
`is_subset`. Equality is set equality. The outline in screen space comes from `MapGeometry::region_outline`.

`SelectionMode { Replace, Add, Subtract, Intersect }` with `apply(current, new)`. The window
maps Shift, Ctrl and both to the modes; the crate only knows modes.

The selection tools, as functions over a layer and `MapGeometry` (which supplies 4- or
6-connected neighbours):

| Function | Rule |
|---|---|
| `flood_fill(layer, start, bounds, geometry, matches)` | scanline flood over cells for which `matches(cell)` holds, bounded by `bounds` |
| `magic_wand(layer, start, extra, geometry)` | the flood of cells equal to the start cell (tile and flips) or to any of `extra` (the kinds dragged over) |
| `select_same_tile(layers, cells)` | every cell of the layers holding one of `cells`; matching the empty cell selects empty space within the layer's bounds on infinite maps and within the map on finite ones |
| `fill_area(layer, start, extra, geometry, selection)` | Bucket Fill's area: on finite maps the flood over the whole layer then clipped to the selection; on infinite maps bounded by the selection, else by the layer's allocated chunks |
| `invert(region, bounds)` | the complement within the map's bounds |

## 5. Shapes

`points_on_line(a, b)` and `points_on_ellipse(centre, rx, ry)` come from `bt-utils::raster`
(Bresenham, as Tiled's `pointsOnLine` and Zingl's ellipse). `ellipse_region(rect)` is the filled
ellipse; `shape_fill_region(FillShape, start, end, square, from_center)` and
`rect_select_rect(start, end, square, from_center)` give the Shape Fill and Rectangular Select
areas: `square` uses the smaller side for fills and the larger for selections, as Tiled does;
`from_center` grows from the start point.

## 6. The brush

One type is the brush everywhere: the Stamp tool's, Bucket and Shape Fill's, the quick stamps'
and the saved stamps'.

```
Brush
├── variations: Vec<Variation { probability: f64, layers: Vec<BrushLayer>, size: IVec2 }>
│       BrushLayer { name: String, cells: TileLayer (unbounded), opacity: f64, tint: Option<Color> }
├── tilesets: Vec<TilesetRef>           what the cells' slots refer to, so a brush works in any map
├── origin: IVec2                       the cell its first variation was captured at (stagger fitting)
├── mode: BrushMode                     Stamp | Random
└── size: u32                           for one-tile and random brushes: a size × size square (1 = off)
```

| Function | What |
|---|---|
| `Brush::from_cells(map, cells)`, `Brush::capture(map, layers, rect)` | a brush from picked tiles (holes stay empty) or from a rectangle of the selected layers (several layers make a multi-layer brush) |
| `placement(points, geometry, rng) -> Option<TileClip>` | what a click, a line or an ellipse stroke paints: a variation centred on each point, skipping points where it would overlap tiles placed earlier in the same stroke, a new variation picked after each placement; fitted to the stagger parity of the target rows on staggered and hexagonal maps |
| `fill(region, rng) -> Option<TileClip>` | what Bucket and Shape Fill paint: the region's bounding box tiled with variations from its top-left corner, each row as tall as its tallest variation, masked by the region |
| `random_cells()` | Tiled's random mode: every tile of every variation weighted by its probability, one per painted cell, chosen by `cell_rng(seed, position)` |
| `sized(points)` | a one-tile or random brush expanded to its `size × size` square around each point |
| `flipped(direction, geometry)`, `rotated(direction, geometry)` | mirrors and quarter turns of every variation; sixth turns and hexagonal mirrors on hexagonal maps through cube coordinates, with the cells' flip flags transformed by `bt-core`'s tables |
| `footprint(point) -> IRect` | the cells a placement covers, for previews and selection masking |

Empty cells in a brush never erase (Tiled's rule). A brush captured from an empty area has
layers and no tiles and paints nothing. Painting a tile adds its tileset to the map when the map
lacks it (§8).

## 7. The clipboard

`TileClip { origin, size, layers: Vec<ClipLayer>, tilesets }` is what Copy puts aside and what
every painting operation produces before it is pasted: named layers of one size, bottom to top,
plus the tilesets their cells use, so a clip pastes into another map.

| Function | Rule |
|---|---|
| `copy(map, layers, region)`, `cut(map, layers, region)` | the selected layers' cells in the region; cut erases them from the unlocked ones |
| `erase(map, layers, region) -> EraseReport` | empties the region on the given layers, skipping locked ones |
| `find_target_layers(map, clip, current, selected)` | Tiled's three rules: a one-layer clip goes to the current layer; a clip with as many layers as there are selected tile layers goes to them in order; otherwise each clip layer goes to the next tile layer of the same name, and a missing one is created beside the last target |
| `paste(map, clip, top_left, targets, mask) -> Result<PasteReport>` | paints non-empty cells into each target, clipped to finite layers and to `mask` (the tile selection; an empty mask does not restrict), skipping locked targets, unifying tilesets (§8), creating the layers `targets` asks for with the clip layer's name, opacity and tint |
| `TileClip::{trimmed, centered_at, content_region, flipped, rotated}` | trimming to content, placement, transforms (hexagonal ones included) |

Paste as the editor does it: `Brush::from_clip(clip.trimmed())` becomes the brush; Paste in
Place pastes at `clip.origin` into `find_target_layers`.

## 8. Tilesets

| Function | Rule |
|---|---|
| `find_tileset(map, tileset)` | a map tileset that can stand in for another: the same `Arc`, the same file, or an embedded tileset "similar" by Tiled's rules (same tile count, image, tile size, spacing, margin and offset; same tile images for collections) |
| `unify_tileset(map, tileset) -> Unified { slot, added }` | the slot to use, adding the tileset when none matches |
| `add_tileset`, `replace_tileset(slot, tileset)` | append; swap in another tileset, keeping the cells' ids |
| `tileset_usage(map, slot) -> TilesetUsage { cells, objects, templates }` | what would be lost by removing it |
| `remove_tileset(map, slot) -> RemovedTileset` | removes it and empties its cells and tile objects; later slots are renumbered in every layer and object |
| `swap_tiles(map, a, b)` | exchanges two tiles everywhere |

## 9. Carried selections

A tile selection can carry its tiles: `FloatingTiles::lift(map, layers, region)` takes the
tiles of the unlocked selected layers off the map and remembers them with their lattice
positions; `moved`, `flipped(direction, pivot)` and `rotated(direction, pivot)` return the
tiles elsewhere; `place(map)` paints them where they are now; `region()` is the selection where
it is now; `at_rest()` says whether they are back exactly where and as they were lifted;
`clip(map)` is a clip of them for Copy. The map always holds the tiles where they currently
are (the editor keeps the carried state in the selection and re-lifts from the base it kept,
`05-editor-core.md`), so every other crate sees a normal map.

Positions live on the grid's own lattice, so shapes survive on every map: cells on orthogonal
and isometric maps, the diamond grid behind the shifted rows of staggered maps, axial
coordinates on hexagonal maps where a turn is 60°. The transform is one exact affine map of
doubled lattice coordinates, so four quarter turns (six sixth turns) or two mirrors give back
exactly what was lifted. Mirrors are mirrors on screen (on an isometric map a horizontal mirror
swaps the grid's axes), and tile flags change as they do for the brush, so a turned tile is
drawn turned. Tiles that leave a finite layer are not painted but stay carried and come back
when moved onto the map again.

## 10. Map and layer commands

**Map** (`map_ops`): `resize_map(size, offset, remove_objects_outside) -> ResizeReport`
(objects and image layers move by the tile offset converted through `tile_to_pixel`, so
isometric, staggered and hexagonal maps move them right; objects outside are listed or
removed), `crop_to(rect)`, `autocrop()`, `autocrop_to_layer(id)`, `offset_map(layers, area,
offset, wrap_x, wrap_y)` (tiles shift within the area and wrap when asked; objects whose
unrotated bounds centre lies in the area move and wrap; image and group layers change their
offset), `make_infinite()`, `make_finite()` (cropped to the used bounds, reported).

**Layers** (`layer_ops`): `locate(id) -> LayerLocation { parent, index }`, `children`,
`layer_order` (draw order), `new_layer(kind, above)` with Tiled's naming ("Tile Layer 1"),
`duplicate_layers` ("Copy" suffix, new ids throughout), `remove_layers`, `move_layers(parent,
index)` (drag and drop, into groups), `raise`, `lower`, `group_layers`, `ungroup_layers`,
`merge_layers_down` (tile over tile overwrites; objects append with their ids; the lower layer's
attributes win), `layer_via_copy`, `layer_via_cut` (tiles or objects), `toggle_visible`,
`toggle_locked`, `other_layers`, and `offset_layers(ids, delta, snap)` for the Offset Layers
tool (locked layers stay; a layer inside a moved group does not move twice).

**Erasing** (`clip::erase` and `erase_targets(map, selected, all)`): the eraser works on the
selected unlocked tile layers that are shown, or with `all` on every shown unlocked tile layer.
Tiled's Shift-erase also erases hidden layers; this crate does not, because an edit to a layer
the user cannot see is a surprise.

## 11. Objects

All positions are pixel space relative to the object's layer; the caller removes layer offsets
and parallax. Isometric pixel space is the unprojected grid, so a screen-space rubber band is a
quad there, which `objects_in_quad` takes as four corners.

| Area | Functions |
|---|---|
| Find | `find_object(id) -> Found { layer, object }`, `object_index`, `object_mut` (copy-on-write) |
| Edit | `add_object(layer, object, index)` (a new id when 0), `duplicate_objects(ids, offset, copy_suffix)`, `remove_objects`, `move_objects(ids, delta)`, `move_objects_to_layer` |
| Order | `raise`, `lower`, `to_top`, `to_bottom`, `up`, `down` on index-ordered layers; `render_order(layer, geometry)`: list order, or by **screen y** on top-down layers (the same key the renderer uses) |
| Bounds | `object_bounds(map, object)`: the shape, or the tile image placed by its alignment and tile offset, rotated around the position; `objects_bounds(ids)`; `object_outline` from `bt-core` for drawing |
| Hit tests | `object_contains(map, object, point, tolerance)`: inside filled shapes or within `tolerance` of them, of a polyline or of a point; size-less rectangles, ellipses and capsules through Tiled's marker; `objects_at(layer, point, tolerance)` topmost first; `objects_in_quad(layer, quad, RectMode::Touching | Enclosed)` |
| Transform | `flip_objects(ids, direction, origin)` (Tiled's `MapObject::flip`: positions mirrored, polygon points mirrored, rotation negated, tile flags flipped, the alignment point kept consistent, overrides marked on template instances), `rotate_objects(ids, degrees, origin)` (around a point with rotations normalised to `[0, 360)`, or each around its own position within `[-180, 180]`; points only move), `flip_origin(ids)` |
| Align | `align_objects(ids, Align, to)` and `distribute_objects(ids, Axis)`, with `align_deltas` and `distribute_deltas` as pure functions over bounds in any space, so the editor aligns what the screen shows on isometric maps too |
| Snap | `SnapMode { None, Grid, FineGrid(n), Pixels }`; `snap(map, point, mode)` through `MapGeometry::snap_to_grid` on every orientation; `snap_delta(anchor, delta, mode)` snaps the moved selection's anchor and moves everything by the same amount; `move_anchor(ids)` |
| Convert | `rect_to_polygon`, `reset_tile_object_size` |

**Object clipboard** (`object_clip`): `copy_objects(ids)` and `cut_objects(ids)` give an
`ObjectClip { objects, tilesets }`; `paste_objects(layer, clip, offset)` gives the pasted
objects new ids, rewires object-reference properties among them (`IdRemap`), unifies their
tilesets and reports the new ids; `paste_offset(clip, cursor, snap)` centres the clip under
the cursor, snapped. `duplicate_objects` uses the same remap.

## 12. Adjusting tile indexes

When an atlas image changes its column count, tiles keep showing the same picture only if their
ids move with their row and column: `columns_changed(old, new) -> bool` (both grids need at
least two rows), `adjust_tile_indexes(map, slot, old_columns, new_columns)` for cells and tile
objects (template instances keep their template's tile unless they override it; cells whose
column no longer exists are erased and counted), and `adjust_tile_metadata(tileset, …)` for the
tileset's own tile data, animations and terrain labels. The editor asks before applying them.

## 13. Errors and reports

```
EditError = NoSuchLayer(LayerId) | NotATileLayer(LayerId) | NotAnObjectLayer(LayerId)
          | NoSuchObject(ObjectId) | NoSuchTileset(TilesetSlot) | NoSuchTile { slot, tile }
          | OutOfIds
LayerChange { layer: LayerId, rect: IRect }
PasteReport { changed: Vec<LayerChange>, created_layers: Vec<LayerId>, added_tilesets: Vec<TilesetSlot>, skipped_locked: Vec<LayerId> }
EraseReport { changed: Vec<LayerChange>, skipped_locked: Vec<LayerId> }
ResizeReport { removed_objects: Vec<ObjectId>, pixel_offset: DVec2 }
RemovedTileset { tileset: TilesetRef, cells: usize, objects: Vec<ObjectId> }
ObjectsReport { changed: Vec<ObjectId> }
```

Reports are values with named fields; nothing is a tuple. A report says what the editor needs to
tell the user and what the live link needs to send back.

## 14. Behaviours kept from Tiled, and the differences

Kept: the selection masks every painting operation; empty brush cells never erase; a brush
placed along a line skips overlapping copies; fills anchor their pattern at the fill area's
corner; painting adds the tileset; the three target-layer rules; flood fills are 4-connected on
square grids and 6-connected on hexagonal and staggered grids and match tile and flips exactly;
merge down keeps the lower layer's attributes; flipping negates rotation and keeps the alignment
point; top-down order ties keep list order.

Different, each logged in `docs/decisions.md`: the eraser leaves hidden layers alone; random
choices are seeded and per cell; a selection can carry its tiles; an impossible request is an
error, not a no-op; stamps, snapping and offsets work on staggered and hexagonal maps.

## 15. Tests

| Test | What it pins |
|---|---|
| Inline unit tests | every operation on small synthetic maps built with a test builder (`testutil`): layers of each kind, groups, locks, tilesets, template instances, every orientation |
| Properties | region algebra laws (union, subtraction, intersection against a set oracle); flood fill against a plain breadth-first search; four quarter turns and two mirrors of a brush and of carried tiles are the identity, six sixth turns on hexagonal maps; lift then place leaves the map unchanged; paste then erase of the same region leaves it unchanged; resize by an offset and back keeps the content; align and distribute leave bounds inside the target; `Map::validate` holds after every operation |
| Tiled parity | the target-layer rules, the fill anchoring, the line placement, the flip and rotation of objects, the merge rules, offset wrapping and the adjust-index mapping, each with the cases Tiled's own code implies |
| Real maps | capture, transform and paste round trips on the purchased packs' maps and on Tiled's examples, orientation by orientation; a brush captured on a staggered map keeps its shape when placed on either row parity |
| Determinism | the same seed and positions give the same cells whatever the visit order |
