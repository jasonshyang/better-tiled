# Rendering — `bt-render`

*2026-10-09; as built at the end of M1, 2026-10-10. The design of the crate that draws a map:
for the editor's canvas, for the headless pictures (`bt render`, Export as Image, the minimap,
the tests) and for every orientation Tiled has. The high-level design
(`00-high-level-design.md`, §6) states the goal: one drawing path, Tiled's draw order and
placement, crisp pixel art at any zoom, one frame at 120 Hz on a large map at any zoom. Tiled's
own sources are cited as `reference/tiled/src/...`.*

## 1. Scope

`bt-render` turns a `Map` and a camera into pixels. It draws tile layers, tile objects, image
layers and the vector content the editor and the pictures need (grid, object outlines and text,
selection outlines, previews, marks), with Tiled's placement, order and layer effects, on a wgpu
device the caller provides, into a render pass the caller owns.

It does not decode images (`bt-image` does), decide what is shown (the caller fills a
`FrameInput`), know widgets (the window embeds it through a paint callback) or hold global
state (every cache is inside the renderer, and the renderer is created once).

Dependencies: `bt-core` (the model and `MapGeometry`), `bt-image` (decoded images and the
"map used as an image" hook), `bt-utils`, `wgpu` (the version `egui-wgpu` uses), `bytemuck`,
`fontdue` for text glyphs and `epaint_default_fonts` for the font (egui's own, so text on the
canvas matches the panels), `glam`, `pollster` (the headless device), `thiserror`, `log`.

## 2. What must match Tiled

These are the behaviours engines and Tiled users depend on
(`reference/tiled/src/libtiled/maprenderer.cpp`, the orientation renderers, and the editor's
`mapscene.cpp`); the tests of §9 pin each one.

| Rule | Detail |
|---|---|
| **Anchoring** | A cell's image is placed with its bottom-left at `MapGeometry::cell_anchor`; a larger image extends up and to the right. On isometric maps that anchor is the diamond's top plus `(-tw/2, th)` with Tiled's integer halves |
| **Tile offset** | Added in screen pixels, scaled with the image's scale, never rotated |
| **Render size and fill mode** | `Tile`: the image's own size; `Grid`: stretched to the map's cell; `PreserveAspectFit`: a uniform scale and centred in the cell |
| **Flips** | `H` and `V` mirror about the image centre; `D` is a transpose on square and staggered grids, drawn as a quarter turn with the sides swapped so the image stays bottom-left anchored; on hexagonal maps `D` is a 60° and `HEX120` a 120° clockwise turn about the image centre, and a 180° turn is drawn as a mirror on both axes so pixel art stays exact |
| **Draw order** | Tiled's per orientation (`01-data-model.md`, §13.3): render order only on orthogonal and oblique maps; isometric by diagonal; hexagonal and staggered by row or half-row. Overlapping oversized tiles stack in that order across chunk boundaries |
| **Integer quirks** | Everything comes from `MapGeometry`, which keeps Tiled's integer halves and floors |
| **Objects** | Tile and text objects are drawn upright at the projected position with their size in screen pixels; the default alignment is bottom-centre on isometric maps and bottom-left elsewhere unless the tileset says otherwise; rotation is clockwise in screen space around the alignment origin. Shapes are drawn from `bt-core`'s `object_outline`, and a rectangle, ellipse or capsule without a size as Tiled's 20-pixel marker (`object_marker`: a square, or a circle for ellipses and capsules) |
| **Animation** | A tile shows the frame for the elapsed time, frames stretched to the base tile's size on layers and to the object's size on objects; a frame of zero duration is skipped rather than shown forever |
| **Layer stacking** | Offsets add up the tree; opacity multiplies down the tree and applies **per primitive** (no layer or group is flattened, so overlapping tiles in a translucent layer show through each other, as in Tiled); visibility and locks are the ancestors'; blend modes are set per layer and ignored on groups (Tiled's "pass-through") |
| **Tint** | Tiled's exact formula for a straight pixel `c, a` and a tint `T, t`: `rgb = T ⊙ (c·a + (1 − a))`, `alpha = a·t`, so semi-transparent pixels pull toward the tint; tint applies to tiles, tile objects and image layers, not to shapes or text |
| **Parallax** | `(1 − f) · (viewCentre − parallaxOrigin)` with `f` the product of the factors up the tree, as Tiled's documentation says; off for headless pictures |
| **Image layers** | At the layer origin, repeated from the origin in both directions when `repeat_x`/`repeat_y`, tinted, up to a bounded number of copies per axis |
| **Top-down object layers** | Sorted by screen y, which is what Tiled's editor shows; hit tests in `bt-edit` use the same key, so what is on top is what is picked |
| **Missing tiles** | A tile that cannot be drawn (unknown id, a tileset that failed to load, an undecodable image) is Tiled's marker: a half-transparent dark cell with a red border and a red cross, one map pixel wide |

Two things are deliberately different from Tiled and documented as such: a layer with a blend
mode is composited as a whole against what is below it (Tiled blends each primitive, so
overlapping tiles inside one blended layer differ), and the parallax origin keeps the
documented sign where Tiled 1.11 flipped it.

## 3. Inputs

```rust
FrameInput::new(map: Arc<Map>, camera: Camera, viewport_px: UVec2)   // the map (the rules' result when AutoMap layers are on)
    .time_ms(u64)                       // for animations
    .layers(LayerVisibility)            // the editor's view switches: highlight(layers, fade) (fade below, dim above), hide(layers)
    .previews(layer, Arc<[PreviewTile]>)  // tiles drawn above `layer` with an alpha
    .vectors(Arc<[VectorItem]>)         // §6: outlines, selection, grid, marks, text
    .objects(ObjectStyle)               // outline width in device pixels, class colours, whether shapes are drawn
    .parallax(bool)
    .background(Option<Color>)          // under the layers (blend modes blend with it); none leaves the target as it is

Camera { origin: DVec2, zoom: f64 }     // world pixel at the top-left; device pixels per world pixel
    .snapped()                          // the origin on whole device pixels, so texel edges land where overlays measure them
```

A `View` is one `FrameInput` drawn into one rectangle; a frame may hold several views (a map
and its world neighbours), drawn by the same renderer with their own cameras.

## 4. The frame

```
register ─► pack ─► animate ─► draw list ─► upload ─► encode (offscreen compositing) ─► paint (caller's pass)
```

1. **Register.** Each tileset of the map is known by the identity of its `Arc` and a
   generation counter: a tileset gets a contiguous range of sprite indexes (atlas: `base + tile
   id`; collection: a map from tile id to sprite), its images are requested from `bt-image`,
   and a tileset whose `Arc` changed (an edit, a reload) gets a new range while the old one is
   evicted. Image layers get standalone sprites.
2. **Pack.** Decoded images are copied into 4096×4096 RGBA8 pages of a 2D-array texture by a
   shelf packer with a two-texel gutter; an image larger than a page is split across pages by
   tile rows, so nothing is left out. Pages carry one mip level below the full one for
   minification (§5). Pages, sprites and chunk caches are evicted least-recently-used under a
   byte budget (512 MiB by default: half for pages, a quarter each for impostors and chunk
   caches); the renderer is never recreated to forget something.
3. **Animate.** A remap table maps a sprite index to the sprite of its current frame; the CPU
   updates the entries of animated tiles and reports when the next frame is due, so the window
   repaints only then. Instances never change for an animation.
4. **Draw list.** A depth-first walk of the layer tree builds `Draw`s, each an instance range
   with a `LayerParams` (offset with parallax, opacity, tint) and a blend mode: tile layers from
   chunk caches (§5), tile objects sorted by screen y where the layer is top-down, image layers
   as repeated quads, previews above their layer, the highlight fade quad under the highlighted
   layer and dimming above it, and the vector items that belong above each layer.
5. **Upload.** The frame's visible instances (one buffer), the layer parameters (a dynamic
   uniform at a 256-byte stride) and the sprite and remap tables when they changed. The upload
   is proportional to what is visible, never to the map.
6. **Encode.** When a visible layer has a blend mode, the view is composited offscreen (§7).
7. **Paint.** One instanced draw per `Draw` into the caller's render pass, or a blit of the
   composited image. The vector pass follows (§6).

## 5. Tile layers

### 5.1 Instances

A tile is one instance: a destination rectangle in layer-local world pixels, a rotation pivot,
a rotation and an image spin (hexagonal turns), a sprite index, flags (`H`, `V`, transpose,
solid fill), a colour. The vertex shader expands six vertices per instance; the fragment shader
reads `sprites[remap[index]]`.

### 5.2 Chunk caches

Instances are built per `(layer, chunk)` the first time a chunk is seen and again only when the
chunk's `Arc` changes (the cache holds the `Arc`, so the pointer cannot be reused while cached),
when the tileset generation changes, or when the geometry changes. On orthogonal right-down maps
the visible chunks are appended row by row, which already is the draw order. On every other
grid each chunk's instances are stored sorted by `draw_order_key` with their keys, and the
visible chunks are merged by key (a k-way merge copying runs at once), so a tall tile at a chunk
boundary stacks exactly as Tiled stacks it.

Culling takes the chunk range the view covers plus a margin of the largest tile extent and
offset, and walks either that range or the layer's chunk list, whichever is smaller.

### 5.3 Far zoom: impostors

Zoomed far out, a 1000×1000-cell map exposes a million cells per layer, and no instance count
stays inside the budget. Below 8 device pixels a cell, and below 16 when a view shows more than
100,000 cells summed over its tile layers, tile layers are drawn from **impostors**:

- A **run** is a stretch of tile layers with nothing drawn between them that composite plainly
  (normal blending, one parallax factor): anything else drawn (an object or image layer, a
  preview, a vector item on a layer, the highlight fade, a blend group) ends the run first, so
  the order of everything stays Tiled's.
- An impostor is a picture of a **world-aligned square** of a run, holding every layer of the
  run with its offset, opacity and tint, drawn from the chunks' own instances in Tiled's order
  and clipped to the square. Squares meet exactly, and an oversized tile stacks across a
  square's edge as Tiled stacks it: the square above holds the part that reaches into it, under
  that square's upper layers. A square is at most 128 texels and 64 cells a side, and holds at
  most about 41,000 cells times layers (64 by 64 cells of ten layers; fewer cells for more
  layers, never fewer than 8 a side), so drawing one takes a few milliseconds at any zoom, tile
  size and depth.
- The resolution is the power of two (device pixels per world pixel) that meets the screen's, so
  an impostor is never blurrier than a direct draw would be, unless the impostor pages cannot
  hold the squares in view, when it steps down. The squares a view needs are bounded by the
  screen, not the map.
- A square is kept while its run's chunks, attributes and tilesets are the ones it was drawn
  from; the run's chunks are held, so an address is never reused while it is compared. An edit
  in a run redraws its squares in view.
- Squares are drawn, nearest the view's centre first, for 3 ms a frame and at least one. A
  square not drawn yet shows its last picture, or nothing, and the frame reports it pending
  (`RenderStats::pending_impostors`), so the window asks for another frame and a headless render
  prepares again until none is left.
- Impostors are drawn at the time they were drawn: animations stand still far out.

Squares live in fixed 128-texel slots of 2048×2048 impostor pages (an array texture the sprite
shader samples through impostor sprites); a slot is cleared by a sprite pipeline that replaces
rather than blends. The minimap is a view at impostor zoom.

### 5.4 Sampling

The fragment shader samples with `textureLoad` clamped to the sprite's own rectangle, so a
neighbouring tile in the page can never bleed in at any zoom, and uses no mipmap chain across
sprites. Magnified, it applies "sharp bilinear": texel edges anti-aliased over one device
pixel, so fractional zooms give no crawling pixels. Minified, it box-filters up to 4×4 texels of
the mip level whose footprint fits (full or half), so a zoomed-out map does not shimmer. Colours
are premultiplied in the shader; pages hold straight alpha as decoded.

## 6. Vector content

Everything the editor draws that is not an image goes through the renderer too, so the canvas
and the headless pictures are the same picture: the grid (fading out as the zoom drops, as
Tiled's does), object outlines and fills by class colour, object names, text objects, point
markers, selection outlines (marching ants with a dash phase), brush and tool previews, `bt`
marks, and the map border. A `VectorItem` (`polyline`, `polygon`, `rect`, `ellipse`, `text`) has
a colour, a fill, a stroke width that is **cosmetic** (`stroke`, device pixels, independent of
zoom) or in world pixels (`world_stroke`), an optional dash pattern, an offset in device pixels,
and where it goes: above everything (the default), `above_layer(id)`, or `on_layer(id)`, drawn
with that layer's offset and opacity right after its content. Strokes are expanded to quads on
the CPU; polygons are triangulated; text (`TextItem`, which also lays out Tiled's text objects)
is laid out and rasterised with `fontdue` from egui's own font, bundled in the binary, into the
pages and drawn as sprites. The vector pass draws after the sprite draws of the layer it belongs
to, so an object's outline sits on its layer and the grid sits on top. `grid_vectors` makes the
grid of a `MapGeometry` as vector items with Tiled's fading.

## 7. Blend modes

Only when a visible layer has a blend mode other than normal: the view is drawn into an offscreen
`accum` target; runs of normal layers go straight in; a blended layer is drawn alone into a
`layer` target, `accum` is copied to `backdrop`, and a full-screen composite pass combines the
two with the W3C separable formula of the mode (`Add` is a clamped sum, as Qt's `Plus`), writing
back into `accum`; `paint` then blits the result. Colours are premultiplied throughout. A
viewport larger than the device's texture limit cannot be composited and is reported, not
silently drawn without blending.

## 8. Headless

```rust
let device = HeadlessDevice::acquire()?;                 // one per process, shared by every caller
let image = Render::of(map).scale(1.0).time_ms(0).background(color).vectors(items).run(&device)?;
let image = Render::of(map).view(origin, size).run_with(device, queue, &mut renderer, &mut images)?;
```

A headless render covers the map's bounding rectangle (`Map::bounding_rect`, as Tiled's Export
as Image), on every orientation and for infinite maps, with the image's top-left at that
rectangle's top-left and `render_origin` saying where a pixel is in the world. An image larger
than the device's texture limit is rendered in bands and stitched, never refused. The result is
straight-alpha RGBA8. Before reading back, a band waits for every image it asked for and
prepares again until no impostor square is pending. Tests, `bt render`, Export as Image, the
minimap thumbnails of world neighbours and the "map used as an image" hook all go through this
one function; the hook is a `MapImageSource` the application builds from the headless device
and gives to `bt-image`, and a map drawn as an image draws its own images through a
`NestedSupply` (decoded on the spot) instead of the store's workers.

## 9. Interfaces

| Function | What |
|---|---|
| `MapRenderer::builder(device, queue, format).budget(bytes).page_side(texels).build()` | once per window or process |
| `prepare(images: &mut dyn ImageSupply, views: &[FrameInput])` | §4 steps 1–5, the impostor squares drawn; returns `RenderStats` |
| `encode(encoder)` | the offscreen compositing of the prepared frame |
| `paint(pass, view_index)` | the sprite and vector draws of one view into the caller's pass |
| `stats()` | instances, vector vertices, draws, pages, impostors kept and squares pending, pending images, whether anything animates and when its next frame is due, bytes uploaded, prepare time |
| `trim()` | lets go of every chunk and tileset the last frame did not draw (a closed tab); eviction under the budget is automatic |
| `max_texture_side()` | the largest target the device allows |
| `image_request(&ImageRef)` | the `bt-image` request of an image of the model, for anyone else showing it (the Tiles panel) |

`ImageSupply` is where decoded images come from: the `bt-image` store (its workers) or a
`NestedSupply`. The editor embeds the renderer through `egui_wgpu::CallbackTrait`: `prepare` in
the callback's prepare, `encode` with the callback's encoder, `paint` in its paint, with one
`MapRenderer` in the callback resources. Errors are typed: `RenderError = NoAdapter | DeviceLost
| TooLarge { size, limit } | NoSuchLayer(LayerId)`; an image that cannot be decoded is drawn as
Tiled's marker, not an error. A `FrameInput` that names a layer that does not exist is a
`RenderError::NoSuchLayer`, not an ignored field.

## 10. Module layout

```
crates/bt-render/src/
├── lib.rs           mod lines and pub use
├── renderer.rs      MapRenderer: the frame's stages, GPU resources, bind groups, the impostor pass
├── input.rs         FrameInput, Camera, PreviewTile, LayerVisibility, ObjectStyle
├── pages.rs         the page texture array, the shelf packer, gutters, the half mip, eviction
├── sprites.rs       the sprite table, tileset ranges and generations, the animation remap
├── chunks.rs        per-chunk instance caches, culling, the draw-order merge
├── impostors.rs     far zoom: runs' squares, their slots, validity and drawing budget
├── draw_list.rs     the layer walk: offsets, parallax, opacity, tint, highlight, previews, objects, image layers, runs
├── instance.rs      the instance format and the cell and object transforms (Tiled's CellRenderer)
├── objects.rs       shape objects as vector items, class colours
├── grid.rs          the tile grid as vector items
├── vector.rs        VectorItem, stroke expansion, triangulation, dashes
├── text.rs          TextItem, layout, glyph rasterisation
├── compositor.rs    blend-mode targets and passes
├── headless.rs      HeadlessDevice, Render, bands, readback
├── supply.rs        ImageSupply, NestedSupply, image_request
├── stats.rs         RenderStats
├── error.rs         RenderError
└── shaders/         sprite.wgsl, vector.wgsl, blend.wgsl
```

## 11. Budget

The budget is one frame at 120 Hz (8.3 ms) for a 1000×1000-cell, ten-layer map at any zoom
with the editor's own work inside it. The cost model: register and animate are linear in the
number of tilesets and animated tiles; the draw list is linear in visible chunks (plus the merge
on projected grids); the upload is linear in visible instances; the GPU work is linear in
visible instances times their pixel area. Impostors bound the instance count at far zoom; the
chunk caches bound the per-frame CPU work at near zoom; eviction bounds memory. The editor's
benchmark mode (`better-tiled --bench`) pans and zooms through the budget map and prints frame
times and the renderer's stats; it runs on the development machine, not in CI.
`scripts/budget-map.py` writes the budget map (`target/budget/budget.tmx`): 1000×1000 cells,
the first of ten layers full and the others half full, from Tiled's desert tileset.

As built, measured on an M4 Pro whose display refreshes at 120 Hz, panning a view width a
second:

- **In the window** (`--bench`, a 904×817-point canvas at 2×), the median time between frames is
  8.3 ms at every zoom from 1/32 to 64, on the Ancient Ruins pack's example map and on the
  budget map; the 95th percentile is under 9 ms at most zooms and 9.6 ms at worst. About 0.4 %
  of frames take two or three refreshes; on nearly all of them the window's own work was under
  2.5 ms and the renderer's preparation under 2 ms, so the time went outside the app's per-frame
  work. The window's own work is 0.2 to 2 ms a frame.
- **The renderer alone** at 2880×1800 device pixels (prepare, encode, paint and the GPU's work)
  takes at the 95th percentile 8.0 ms at 1/32, 8.7 ms at 1/16, 6.6 ms at 1/8, 6.5 ms at 1/4 and
  under 2.6 ms from 1/2 to 64; far out the cost is drawing the squares that come into view,
  spread over the frames at 3 ms each.

## 12. Tests

| Test | What it pins |
|---|---|
| Synthetic orientations (`placement.rs`) | a marker tileset on orthogonal, isometric, staggered (both axes and indexes), hexagonal and oblique maps with odd sizes: every marker lands on the pixel `MapGeometry` predicts; overlapping oversized tiles stack in Tiled's order across chunk edges on every grid |
| CPU reference (`goldens.rs`, `tests/common`) | an independent CPU compositor implements Tiled's `CellRenderer` rules cell by cell in Tiled's order; six of Tiled's example maps rendered by the GPU match it (premultiplied, within 2) |
| Golden images (`goldens.rs`) | 17 committed PNGs of small maps covering flips (hexagonal turns included), render sizes and fill modes, tile offsets, animations at fixed times, tints, every blend mode, every render order with overlapping oversized tiles, repeated image layers, parallax, highlight, previews, markers, vector items and text |
| Pack (unit tests) | packing never overlaps; images larger than a page are split; eviction keeps what is in use |
| Headless (`headless.rs`) | banded renders equal one-piece renders; the origin and size are the map's bounding rectangle; a zoom of 2 is pixel-exact |
| Impostors (`headless.rs`) | a far view equals the direct render downscaled within a tolerance, one square covering the map; a tall tile of a lower layer reaching across a square's edge stays under the upper layer |
| Stats (`headless.rs`) | a static map reports no next frame; an animated one reports the right due time |
| Real maps (`packs.rs`) | the Highlands biome map's chambers against the pack's previews (six identical, two under 0.1 %) |

Tests need a GPU adapter; on CI they run on the platform's software adapter, and a missing
adapter fails the test unless `BT_SKIP_MISSING_TEST_DATA=1` turns it into a skip that says so.
