# Godot export — `bt-godot`

*2026-10-09. The design of the export of a map to a Godot 4 scene: what a scene holds and how
every orientation lands on Godot's grids, the regeneration contract that keeps work done in
Godot safe, the identity that lets a scene built on the export survive re-exports, the property
conventions, the shared TileSet, the report with its codes, the front door in the editor and in
`bt`, and the check Godot itself runs. The high-level design (`00-high-level-design.md`, §13)
states the goals. Everything here about Godot's behaviour was checked against Godot 4.7
headless; the minimum Godot is 4.3, the first with `TileMapLayer` and `Parallax2D`.*

## 1. Scope

`bt-godot` turns a map in memory into the text files of a Godot project: a scene (`.tscn`),
optionally a shared TileSet (`.tres`), copies of images from outside the project and a manifest
of those copies. It reads the project only to decide what it may write (headers, uids,
references) and to say what Godot will do with scenes built on the export. It never runs Godot;
`verify` does, on request, and no export depends on it. It is MIT OR Apache-2.0 and written
from Godot's own format, not from Tiled's exporter.

Not here: the editor's dialogs and flashes (`06-application.md`, §9 and §10), the `bt godot`
commands' parsing (`10-agents.md`), the `godot` section's schema (`09-project-file.md`, §6.5).

Dependencies: `bt-core`, `bt-image` (image bytes for copies and sizes), `bt-project` (the
setup), `bt-utils` (`hash`, `atomic_write`, `paths`).

## 2. Terms

| Term | Meaning |
|---|---|
| generated file | a scene, TileSet or image copy the export wrote: build output |
| header | three `;` comment lines at the top of a generated scene or TileSet, the last with a hash of the rest |
| marker | `metadata/bt_source` on the root, naming the map; it survives a Godot re-save |
| uid | Godot's resource id (`uid://…`); the export derives one from the file's `res://` path |
| level scene | a scene of the user's that inherits or instances the export; the starter is one the export writes once |
| shared TileSet | one `.tres` per grid that every scene of that grid uses, with append-only source ids |
| grid key | `ortho-16x16`, `iso-64x32`, `stag-y-odd-64x32`, `hex-y-odd-14x12-s6`: what maps sharing a TileSet must agree on |
| cell box | the rectangle a tile's image is centred in on Godot's grid |
| shift | the constant by which Godot's cell centres differ from Tiled's on some grids |
| alternative tile | a Godot tile variant: flips, and corrected origins for flipped tall tiles |
| custom data | per-tile values in a TileSet, one layer per Tiled tile property |
| consumed | a property the export gave a meaning to; it is not written as metadata |
| refusal | a file the export will not replace, with what would be lost and the way out |

## 3. Module map

| Module | Holds |
|---|---|
| `export` | the API: `export`, `export_map`, `status`, `export_all`, `generated_scenes`, `scene_of`, `images_copied` (§4) |
| `plan` | every output built in memory before anything is written |
| `contract` | `Found`, `Refusal`, `Status`, `Summary`, the header and hash, backups, the write (§8) |
| `scan` | one pass over the Godot project: uids, headers, references; cached by path, size and time |
| `read` | a reader for `.tscn` and `.tres` text: comments, header, sections, the values the export looks at; FNV-1a; uid text |
| `scene`, `text` | the scene model and Godot's text syntax as Godot 4.7 writes it |
| `grid` | shapes, layouts, cell boxes, the shift, flipped alternatives (§6) |
| `tileset`, `animation`, `tiledata`, `collision`, `terrain` | the TileSet's contents (§7) |
| `layers`, `objects`, `names`, `values`, `conventions` | nodes, identity, metadata, the property names (§5, §9) |
| `images` | copies and `images/.sources.json` (§8.6) |
| `shared` | the shared TileSet's registry (§8.7) |
| `inheritors` | the level-scene check (§10.3) |
| `starter` | the starter level scene (§10.4) |
| `setup`, `version` | the `godot` section's defaults and paths; Godot's version from `project.godot` |
| `report` | `Report`, `Note`, `Element`, the codes (§11) |
| `verify` | Godot headless on a scene (§13) |

## 4. The API

```rust
export(map_path, loader, target, &Options) -> Result<Report, Error>        // the saved file
export_map(&map, map_path, custom_types, target, &Options)                 // the editor's map
status(map_path, loader, target) -> Result<Status, Error>                  // writes nothing
export_all(project, loader_for, &Options) -> Result<AllExported, Error>    // every generated scene again
generated_scenes(project) -> Scenes { generated, copies }
scene_of(map_path, &map) -> Result<Option<PathBuf>, String>                // the remembered target, else the setup's
verify::verify(godot, scene) -> Result<Verdict, String>
```

`Options { replace, check, new_uid }`. `Error::Failed(message)` is exit 1; `Error::Refused(Vec<Refusal>)`
is exit 3. The outcome maps to the editor's question and `bt`'s exit code through one table:

| Outcome | Editor | `bt` |
|---|---|---|
| written | the flash with counts and notes, Issues for the notes | 0 |
| up to date | "up to date", nothing written | 0 |
| `--check` would write | — | 4 |
| failed | the error; on save, an Issue | 1 |
| refused: a generated file changed | Replace (keep a backup), Show the file, Cancel; on save, an Issue and a red status item | 3 |
| refused: the scene moved in Godot | Export there | 3 |
| refused: the shared TileSet missing | Export every map again, unless a user scene paints with it | 3 |
| refused: a copy carries the scene's uid | Show the copy | 3 |
| refused: the uid is another file's | Give it a new uid | 3 |
| refused: a level scene or another map's export is in the way | Show the file; Replace as a second choice for another map's export | 3 |
| wrong arguments | — | 2 |

An export on save never asks and never replaces: the save succeeds, the refusal becomes an
Issue. The editor exports the shown map (the rules' result with AutoMap layers), which is what
Save writes, so the editor's export of a saved map and `bt`'s export of the file are the same
bytes. An unchanged map writes nothing, so Godot has nothing to re-import.

## 5. What a scene holds

| Tiled | Godot |
|---|---|
| the map | a `Node2D` named after the file, nearest-neighbour filtering, properties as metadata, the marker |
| tile layer | a `TileMapLayer` using the TileSet (embedded or shared); offset, opacity, tint, visibility, blend mode |
| object layer | a `Node2D` (y-sorted when drawn top down) holding the objects: sprites as `Sprite2D` or `AnimatedSprite2D`, points as `Marker2D`, rectangles, ellipses, capsules and polygons as `Area2D` with a collision shape (polygons on isometric maps), polylines as `Path2D`, text as `Label`, `resPath` objects as instances, `godotNode` objects as bodies |
| image layer | a `Sprite2D`, repeating over the map's bounds in whole image steps when the layer repeats |
| group | a `Node2D` at the group's offset |
| parallax | a `Parallax2D` holding a layer whose factor times its groups' is not 1, with that product as `scroll_scale`; never nested |
| class | the node's group and `metadata/class` |
| properties | metadata under keys Godot takes (§9.3); object references as `NodePath`s, files inside the project as `res://` paths |
| tile properties, classes | custom data layers |
| Wang sets | terrain sets |
| tile collision | physics, navigation and occlusion polygons |
| what a scene cannot hold | a `godot/left-out` note: the background colour (with where Godot sets it), the parallax origin when it is not the window's centre, text styling a `Label` lacks |

AutoMap's rule tiles are left out with a note. Oblique maps are refused.

## 6. Grids

Tiled's cell (x, y) is Godot's cell (x, y) on every grid. W and Hh below are Tiled's rounded-down
tile size (`bt-core`'s hex parameters).

| Tiled | `tile_shape`, `tile_layout`, `tile_offset_axis` | `tile_size` | cell box |
|---|---|---|---|
| orthogonal | square | the tile size | the tile size |
| isometric | isometric, diamond down | the tile size | (2⌊tw/2⌋, th) |
| staggered | isometric, stacked (odd index) or stacked offset (even), horizontal (axis y) or vertical (x) | (W, Hh) | (W, Hh) |
| hexagonal, side = 2⌊(th − s)/2⌋ (tw for axis x) | hexagon, as staggered | (W, Hh) | (W, Hh) |
| hexagonal, other sides | half-offset square, as staggered, with a warning | one row (axis y) or column (x) | from the real hex size |

- **The shift.** On orthogonal, staggered and exact-hex grids Godot's cell centres equal
  Tiled's. On isometric maps and the hex fallback they differ by a constant: `bt-core`'s centre
  of cell (0, 0) minus a port of Godot's `map_to_local` for the shape written. Every
  `TileMapLayer` keeps its Tiled offset, and objects, image layers and parallax contents are
  moved by the shift, so game code can call `layer.local_to_map(position)` as for a map painted
  in Godot. The report states the shift.
- **Tall tiles.** `texture_origin = ((Bw − w) div 2, (h − Bh) div 2) − tile offset`, which
  reproduces Tiled's bottom-left anchoring under Godot's draw rule; an odd difference cannot be
  expressed and gets a half-pixel warning. Godot mirrors `texture_origin` for flipped cells
  while Tiled flips the image in place, so every flip combination the map uses on a tile with a
  non-zero origin gets an explicit alternative tile with a corrected origin.
- **Hex turns.** Tiled's half turn becomes a mirror both ways; 60° and 120° turns are drawn
  unturned with a warning. Picking near cell edges differs on hex grids (Godot picks by
  hexagon, Tiled by nearest centre); that is Godot's and is documented, not corrected.

## 7. The TileSet

- **Which tiles.** Every tile of every atlas tileset of the map, transparent tiles included
  (Godot users may paint any tile, and no painted cell may lack tile data); an image collection
  only when a tile layer paints with it, then every tile. Rule tilesets stay out.
- **Sources.** Numbered in tileset order, each atlas's base source then its animation sources,
  a collection's tiles one source each; every source carries a `resource_name` such as
  `tilesets/desert.tsx`, `tilesets/props.tsx#12` or `tilesets/beach.tsx#anim1`, which is the
  shared TileSet's registry key. Probability is written when it is not 1.
- **Animations.** Godot steps an atlas tile's animation through cells laid out from the tile's
  own cell, so each animated tile goes in an atlas source of its own on the same texture, at its
  first frame's cell, its frames staying plain tiles of the base source. A solver fits row runs,
  fixed strides (`animation_separation`), columns, column strides and whole-row wraps
  (`animation_columns`); animations whose cells overlap are packed into further sources;
  anything else shows its first frame with a warning. `animation_columns` and
  `animation_separation` are written before the durations, as Godot does and requires; a zero
  duration is written as one millisecond. Cells painted with an animated tile point at its
  animation source. Collection animations whose frames are evenly spaced parts of one image
  animate from the tile's own source.
- **Custom data.** One layer per tile property name, sorted by name as Tiled sorts them,
  written before the sources (else every value loads as its default): bool, int, float and
  string as themselves; file as a `res://` path inside the project, else the string with a
  warning; color as `Color`; object as the id; enum by its storage type; class as `Dictionary`;
  list as `Array`. A layer has the type of its first value, ints and floats together making a
  float layer; a value of another type is left out with a warning, since Godot converts or
  nulls silently. The tile's class becomes a `class` String layer unless a property of that
  name exists, which wins with a note. Type 0 is never written.
- **Collision.** Each collision object of a tile becomes a polygon on the physics layer its
  `godotPhysicsLayer` names (0 by default), one-way with `godotOneWay` and its margin, kept out
  of physics with `godotCollision = false`; rectangles, ellipses and capsules become polygons;
  points, polylines, text and tiles have no area and are left out with a warning; a tileset with
  an isometric grid has its shapes projected. `godotNavigationLayer` adds a `NavigationPolygon`
  of convex parts (concave shapes split, with a note; crossing edges or no area, left out with
  a warning); `godotOcclusionLayer` adds an `OccluderPolygon2D`. The layers' bits come from the
  setup; a layer above 0 used without an entry gets Godot's default with a warning.
- **Terrains.** Corner, edge and mixed Wang sets become terrain sets of mode 1, 2 and 0; colour
  c is terrain c − 1 with its name and colour; Wang index i maps to the peering bit through
  `[12,15,0,3,4,7,8,11]` on orthogonal and `[14,1,2,5,6,9,10,13]` on isometric and staggered
  grids; the tile's own terrain is its most common colour, ties to the lower index. On hex maps
  edge and mixed sets become six-sided (the two non-adjacent indices dropped) and corner sets
  are left out, each with a warning; a tile in several sets keeps the first with a warning; an
  animated tile's terrain sits on its animation source's tile; terrain is on alternative 0
  only, and a flip that changes a tile's colours warns. Godot's own neighbour checks on the
  export count exactly Tiled's mismatches. Godot's terrain brush paints edge and mixed sets
  seamlessly and can leave seams in corner sets, because Godot gives a tile one centre terrain;
  that is accepted: the TileSet stays a faithful image of the Wang set rather than carrying an
  alternative per centre colour.

## 8. The regeneration contract

Generated files are build output; user files are only read. The one exception is the starter
level scene, written once on request.

### 8.1 The header and the marker

```
; Generated by better-tiled from ../maps/desert.tmx. Edit the map, or a scene that
; inherits this one: changes made here are replaced by the next export.
; better-tiled-export 1 fnv1a64=9c1e04d2a7b3f051
[gd_scene format=3 uid="uid://b6x62a7vjuo44"]
```

The hash is FNV-1a 64 over every byte from the `[gd_scene` (or `[gd_resource`) line on; it
detects change, nothing more. Godot loads `;` lines before the header in both file kinds and
drops them when it saves the file again, so a file with the marker but no header was re-saved
by Godot. Nothing in a file varies between runs: relative paths, no timestamp, no version
beyond the header format's. The marker is the root's first `metadata/bt_source` line; a map
property of that name goes under `bt_source_`.

### 8.2 The uid

A file keeps the uid it has. A new file gets one derived from its `res://` path (FNV-1a 64
masked to 63 bits, never 0, written in Godot's base-34 text), so deleting and exporting again
gives the same uid and references by uid keep resolving after a path goes stale. Before writing,
the scan looks for another file carrying that uid (headers, `*.uid` files, `uid=` in `*.import`
files, skipping `.godot/`, hidden folders and `.gdignore` folders): if it carries this map's
marker, the export moved in Godot; otherwise the uid is in use and `--new-uid` salts the
derivation. A file that has its own uid and finds another file carrying it was copied outside
Godot; the copy is what needs a uid, and neither `--replace` nor `--new-uid` changes that.

### 8.3 What an export finds

| Found | Meaning | Action |
|---|---|---|
| nothing | new | write, with a derived unused uid |
| the same bytes | up to date | nothing |
| header, hash matches, same map | ours | replace, keeping the uid |
| header, hash matches, another map | another map's export | refuse; Replace as a way out |
| header, hash differs | edited by hand or a merge | refuse, listing what it has beyond the export |
| marker, no header | re-saved by Godot | refuse, listing what Godot added (scripts, connections, groups, extra nodes, foreign unique ids) |
| no header, root instances a generated scene, or a starter | the user's level scene | refuse, even with Replace: export to another scene |
| no header, Tiled's shapes (`TileMap` with `format = 2` on `TileSet_0`, or atlas sources all `TileSetAtlasSource_N`) | Tiled's exporter | refuse once, with the explanation; Replace turns `$TileMap` into one layer per Tiled layer |
| no header otherwise | an earlier version, a person or Godot | refuse once, listing what it holds |
| an image copy changed in Godot, image unchanged | copy changed | keep it, with a warning |
| an image copy changed in Godot, image changed too | copy conflict | refuse |
| this file's own uid elsewhere | a duplicate | refuse, naming the copy, beyond Replace and `--new-uid` |
| the scene's uid on a file with this map's marker | moved in Godot | refuse: export there, or `--new-uid` for a new scene here |
| the derived uid on another file | uid in use | refuse: `--new-uid` |
| a shared TileSet missing while scenes use it | missing | refuse, even with Replace (§8.7) |
| a read-only file or folder | — | fail |

`Found` is one enum; `refused()`, `beyond_replace()` and `way_out(path, found)` are its methods,
so the editor's question, `bt`'s sentence and the JSON word for each state come from one place.

### 8.4 Replace

The old file is kept beside the new one as `name.tscn.bak`, then `name.tscn.2.bak` and so on,
never over an earlier backup, with its uid removed from the header so that renaming it back
cannot create a duplicate uid; Godot does not import `.bak` files. The new file keeps the old
uid.

### 8.5 The write

1. Build every output in memory: the scene, the shared TileSet if any, the image copies, the
   manifest.
2. Scan the project once (in the editor the scan is cached by path, size and modification time
   and re-reads only what changed).
3. Classify every output and check its uid. If anything is refused, nothing is written, images
   included.
4. Write each changed file atomically: images, then the TileSet, then the scene, then the
   manifest. Files already as the export would write them are not touched.

### 8.6 Image copies

Images inside the project are used in place. Others are copied into the setup's images folder
(`res://images/`, shared by every scene), else `images/` beside the scene or the shared TileSet;
a copy mirrors the image's path from the map's folder without the leading `..`, so maps of one
pack share copies, and a different image already in that place gets a numbered name.
`images/.sources.json` records for each copy its source, the hash of what was written, the
generated files using it and, when two maps bring the same bytes from different images, those
other sources. A copy changed in Godot is kept and noted, and refused once its image changed
too; a copy nothing uses any more is removed only while its bytes still match the hash and no
file in the project mentions its path. An image that cannot be read fails the export, naming
each one: the unused tiles of a painted collection and the tilesets other maps registered in a
shared TileSet count, so one lost image stops every map of that grid until it is restored.

### 8.7 The shared TileSet

With a setup (or a map's `tilesetResPath`) the maps of one grid share one `.tres` at the map's
`tilesetResPath`, else `<tileSetFolder>/<grid key>.tres`. The file is the registry: each
source's `resource_name` names its tileset, tile or animation; a re-export keeps every id and a
new source takes the highest plus one; the file holds the union of every tileset ever
registered, each made again from its own file, whichever map is exported, so exporting one map
drops nothing another relies on and gives the same bytes in any order. Animated tiles stay in
the animation source the file has them in while they fit. A registered tileset whose file is
missing keeps its sections verbatim with a warning; sources a tileset no longer makes are kept
so their ids stay taken. A map of another grid fails. A missing file that scenes still use is
refused even with Replace, since a new one would number its sources afresh under their cells:
when only generated scenes use it, `bt godot export --all` makes it afresh from every map; when
a scene of the user's paints with it, only restoring it keeps that scene's tiles, and the editor
offers no "Export every map again". The file goes through the contract like a scene, its
header and marker naming the grid's tilesets.

## 9. Conventions

### 9.1 Tiled's names

Case-sensitive, and meaning what Tiled's exporter means, with two deliberate fixes:

| Name | On | Godot |
|---|---|---|
| `tilesetResPath` | map | where the shared TileSet goes |
| `noExport` | any layer, a group with its children, an object | left out; read as Tiled reads a flag (a number other than 0, a string other than empty, `0` or `false`) |
| `tilesetOnly` | tile layer | the layer left out, its tilesets registered |
| `ySortEnabled` | map, group, any layer | `y_sort_enabled` = the value; Tiled turns a tile layer's on for any value, `false` included, and the export warns where that differs |
| `zIndex` | group, any layer | `z_index`, clamped to ±4096 with a warning; on a tile it stays custom data |
| `resPath` | object, then its template, its tile, its class | an instance of that scene (§9.4) |
| `exportAlternates` | tileset | ignored |
| any tile property | tile | custom data (§7); files become `res://` and lists `Array`s, the second fix |

### 9.2 better-tiled's names

All start with `godot`, so they cannot collide with a game's own properties.

| Name | On | Godot |
|---|---|---|
| `godotYSortOrigin` | tile layer; tile | `y_sort_origin` |
| `godotZIndex` | tile; object | `z_index` |
| `godotPhysicsLayer`, `godotOneWay`, `godotOneWayMargin`, `godotCollision` | a tile's collision object | its physics polygon's layer, one-way flag and margin; `false` keeps it out of physics |
| `godotNavigationLayer`, `godotOcclusionLayer` | a tile's collision object | a navigation polygon, an occluder, of that layer |
| `godotNode` | object, or its class, template or tile | `StaticBody2D`, `AnimatableBody2D`, `CharacterBody2D`, `RigidBody2D`, `Area2D` or `Node2D` for a shape (which keeps its collision shape) or a tile object (which gets a `Sprite` child and its tile's physics shapes, flipped and scaled with it); an unknown value keeps the usual node with a warning |
| `godotCollisionLayer`, `godotCollisionMask` | object | `collision_layer` and `collision_mask` of a body or area; 1 (Godot's default) is left out |

Project-wide settings (physics, navigation and occlusion layer bits, folders, the TileSet mode)
are the `godot` section of `better-tiled.json`.

### 9.3 Metadata keys

A consumed property is written neither as metadata nor as custom data, and the report lists
the names consumed. Every other property is metadata under a key Godot takes, an ASCII
identifier: a name that already is one keeps it; otherwise accents are dropped (`état` →
`etat`), other characters become `_`, a leading digit gets a `_` before it, and a key already
taken gets `_`, then `_2`, `_3`, with one note listing each rename. The export's own keys stay
its own: `id` and `layer_id` on every layer and object node, `class` where a class is written,
`bt_source` on the root; a property of one of those names goes under `id_` and so on, with a
warning. Node names, groups and custom data layer names take Unicode unchanged.

### 9.4 Instances

An object with a `resPath` (`res://….tscn` or `.scn`, found in the project) becomes an instance
of that scene, keeping its name and unique id, placed at the centre of its rotated box minus
the shift with its rotation and visibility (Tiled's exporter uses the unrotated centre and no
rotation; unrotated objects land where Tiled puts them, rotated ones get a note). Properties go
into metadata, and a name the scene's root script declares with `@export` (read from the `.gd`
text) is also set directly, quoted when beyond ASCII; properties the script does not declare
are reported, since Godot drops unknown instance properties silently; C# scripts and exports in
a base script fall back to metadata. A missing or malformed `resPath` keeps the typed node with
a warning, so the scene still loads. Collision bits on an instance stay metadata.

## 10. Identity

### 10.1 Names

A layer is named after its Tiled name, else `layer_<id>`; an object after its name, else
`<class or kind>_<id>`; among duplicate siblings the lowest id keeps the plain name and the
others get `_<id>`; characters Godot refuses are replaced. Adding an object never renames
another, since a new object has a higher id. The report lists each node whose path changed
since the scene on disk, by Tiled id.

### 10.2 Unique ids

On projects whose `project.godot` names Godot 4.6 or later (`config/features`), every node gets
a `unique_id`: the root 1, object o `2^29 + o`, layer l `2^30 + l` (ids below `2^28`), and helper
nodes (a `Parallax2D` holder, a collision child, a sprite child) and larger ids a hash in
`3·2^29 …` re-salted on a clash. Layers also carry `metadata/layer_id`, objects `metadata/id`,
so exports without ids are understood too. A map file whose layers have no ids gets a warning
to save it once. Godot 4.6 is where `unique_id` and `parent_id_path` appeared; older projects
get names alone, which still carry identity.

### 10.3 The level-scene check

Godot work belongs in a scene that inherits or instances the export. The check reads every such
scene the scan lists (by path or uid), replays Godot's loading of it on the new export, and
reports; it never writes a user scene. How Godot 4.7 finds things: an override by name under
its resolved parent first, then by `unique_id` anywhere in the instance, else "vanished"; a
parent by `parent_id_path`, then by path, else the root, naming the node `A_B#C`; connections
by their two paths, dropped silently.

| Found in a level scene | What Godot will do | Code |
|---|---|---|
| an override of a node whose path changes, matched by id | follow it | `level-scene-follows` (info) |
| an override matched by id whose parent is gone | keep it at the root, renamed `A_B#C` | `level-scene-renamed` |
| the same, in a scene without ids | lose or detach it: save the scene once in Godot | `level-scene-lost` |
| an override whose old path another node now has | apply it to that node | `level-scene-retargeted` |
| an override of a deleted object | drop it | `level-scene-dropped` |
| a child added under a deleted or unmatched node | move it to the root as `A#B` | `level-scene-moved-to-root` |
| a `tile_map_data` override on a generated layer | freeze that layer | `level-scene-tiles` |
| an override of a node whose type the export changes | apply what the new type has, drop the rest silently | `level-scene-type`, with the old and new types |
| a connection to or from a path that no longer exists | drop it silently | `level-scene-connection` |
| an override of an object AutoMap placed | lose it when the rules change | `level-scene-automap` (info) |
| a scene that cannot be read | — | `level-scene-unreadable` |

### 10.4 The starter

With `levelScene` in the setup, a map's first export also writes `<map>_level.tscn` beside the
scene: a root that instances the generated scene, with a comment saying to put the player, the
camera and scripts there and never to paint on the generated layers. It is written with
create-new only, never over a file, and never again; another file in its place gets a warning
naming it. Writing it ourselves means the user's first save of it in Godot is a load and save,
which writes the `parent_id_path`s that keep overrides on nodes the map later renames.

## 11. Reports

```
Report { files: Vec<File { path, state }>, counts, notes: Vec<Note>, renamed, consumed, checked, level_scenes }
Note   { code: &'static str, severity: Warning | Info, message, elements: Vec<Element> }
Element = Layer(id) | Object(id) | Tileset(slot) | Tile(slot, id)
```

Notes of one kind are grouped and counted, never listed one per tile; each names the elements
it concerns, so the Issues panel jumps to them. The codes are one list, from which the table
below, the JSON output and the tests are checked:

| Code | Kind | When |
|---|---|---|
| `godot/hex-fallback`, `godot/hex-rotation` | warning | a hex side Godot's hexagon cannot draw; tiles turned by 60° or 120° |
| `godot/grid-shift` | info | isometric maps and the hex fallback: objects and image layers moved by the shift |
| `godot/half-pixel-origin`, `godot/tile-render-size` | warning | a tile that cannot be centred to the pixel; a tileset drawn at the grid's size |
| `godot/animation-layout` | warning | animations no atlas layout fits: the first frame shows |
| `godot/tile-image-missing`, `godot/tileset-missing` | warning | tiles with no image, per tileset; a tile object whose tileset is missing |
| `godot/rule-tiles`, `godot/blend-mode` | warning | rule tiles left out; a blend mode Godot lacks |
| `godot/left-out` | info | the background colour, the parallax origin, text styling |
| `godot/physics-quadrant-size` | info | tile layers set to collide cell by cell (§12) |
| `godot/reserved-name`, `godot/metadata-key` | warning, info | properties renamed because of the export's own keys; keys Godot does not take |
| `godot/layer-ids-unsaved`, `godot/map-loading` | warning | layers without ids in the file; what loading the map warned |
| `godot/ysort-false`, `godot/zindex-clamped`, `godot/differs-from-tiled`, `godot/ysort-top-down`, `godot/tall-tiles-no-ysort` | warning, warning, info, info, info | the layer conventions' edge cases |
| `godot/property-type`, `godot/setting-type` | warning | a `godot*` value of the wrong type or range, left out |
| `godot/respath-missing`, `godot/respath-rotated`, `godot/export-undeclared` | warning, info, warning | instances (§9.4) |
| `godot/node-type`, `godot/body-no-shape` | warning | `godotNode` edge cases |
| `godot/custom-data-type`, `godot/custom-data-file`, `godot/custom-data-class` | warning | custom data (§7) |
| `godot/tile-collision-kind`, `godot/navigation-split`, `godot/navigation-shape` | warning, info, warning | collision and navigation (§7) |
| `godot/physics-layer-unset`, `godot/navigation-layer-unset`, `godot/occlusion-layer-unset` | warning | a layer above 0 without a setup entry |
| `godot/wang-several-sets`, `godot/wang-hex-sides`, `godot/wang-hex-corners`, `godot/wang-flipped` | warning | terrains (§7) |
| `godot/shared-tileset-missing`, `godot/shared-source-kept` | warning, info | the shared TileSet's kept parts (§8.7) |
| `godot/copy-changed`, `godot/copy-kept`, `godot/image-not-copied`, `godot/image-sources` | warning, info, warning, warning | image copies (§8.6) |
| `godot/uid-duplicated` | warning | an up-to-date file whose copy elsewhere carries its uid |
| `godot/level-scene-*` | see §10.3 | the level-scene check |

Where it shows: the editor's status flash and the Issues panel's Godot source; `bt`'s summary
on standard output and `warning:` and `note:` lines; `--json` the whole report, with each
refusal's `found` word and `way_out`.

## 12. Draw order and physics

Y-sort is never implied: it is on for object layers drawn top down and where `ySortEnabled`
says so; a non-orthogonal map with tiles taller than the cell and no `ySortEnabled` gets an info
note. Godot 4.5 and later merge the collision shapes of a physics quadrant into one body and cut
a mirrored tile's shapes away where they overlap another cell's; a tile layer with mirrored
colliding tiles whose shapes can reach into another cell therefore gets `physics_quadrant_size
= 1`, with a note, on projects naming 4.5 or later or no version. A `Parallax2D` is never inside
another and has no `position`; a group with an offset is a `Node2D`; a repeating image layer
covers the map's bounds in whole image steps from the layer's origin.

## 13. The front door and `verify`

- **Setup, once per project** (the editor's sheet or `bt godot setup`): the Godot project (the
  nearest `project.godot` at or above the map, else chosen), the levels folder, shared or
  embedded TileSets, the images folder, export on save, the starter level scene; the sheet shows
  Godot's version and how many images this map would copy. The setup is the `godot` section of
  `better-tiled.json`; what is not given keeps its value.
- **Every export after**: File ▸ Export to Godot, the repeat-export key, the palette, export on
  save, `bt godot export`; the target is remembered in the map as Tiled remembers it
  (`format="tscn"`), so a map Tiled exported runs ours from then on. `bt` exports the saved file
  and says when the open map has unsaved changes; `--remember` sets the target through the live
  link as one undo step.
- **`bt godot status`** says for each map: up to date, out of date, changed in Godot, moved,
  refused for another reason (each file with its way out) or never exported, with the level-scene
  findings; without maps, every generated scene of the project, and the copies of generated
  scenes, which are left alone.
- **`bt godot export --all`** exports every generated scene of a project again from the map its
  marker names, all or nothing: every scene as `--check` first, none written if one would be
  refused or fail, the exit code the worst.
- **`verify`**: Godot headless (`GODOT`, else the macOS app, else the path) imports the project
  as its editor would, loads the scene, keeps it in the tree for two physics frames so scripts'
  `_ready` runs, and reports Godot's errors and warnings and every cell whose tile the TileSet
  lacks; exit 1 on any. It runs only on request (`bt godot verify`, ⌘K ▸ Check in Godot) and
  never inside an export.

## 14. Deliberate choices and limits

- Node names and unique ids come from Tiled ids, not visit order; a first export over a scene
  with counter names renames once and reports each.
- Rotated instances are placed at the rotated centre with their rotation, unlike Tiled's
  exporter.
- The TileSet holds every tile; `format=3` text with decimal arrays, because Godot 4.3 must load
  the output.
- Godot's terrain brush may leave seams in corner sets; the mapping stays faithful (§7).
- `godotNode = Node2D` keeps the collision shapes for their geometry.
- The parallax origin is noted when it is not the window's centre, not corrected: a camera zoom
  would break any correction.
- Polylines are not occluders; `@export` is found by reading GDScript text only.
- Two project files sharing one Godot project may disagree on layer bits; the last export wins,
  with a warning.
- Oblique maps are refused; nothing is read back from Godot; no plugin.

## 15. Cost

An export on save must stay under a hundred milliseconds on the largest pack scene, checks
included: outputs are built in memory, the project scan is cached by modification time, the
level-scene check reads only the scenes the scan lists, and an unchanged map writes nothing.
The real-map suite reports the timings in release builds and asserts a bound.

## 16. Tests

| Test | What it pins |
|---|---|
| Text | byte-identical output on two runs; the reader on our output, a Godot re-save (format 4, base64, unique ids, `parent_id_path`), Tiled's output and a hand-written inheritor; uid text against Godot's; FNV vectors |
| Contract | every `Found` state; backups and uid stripping; nothing written on a refusal, images included; an identical export leaves the modification time; derived uids are deterministic; `--check` and `status`; copies changed in Godot kept and then refused; stale manifests read; pruning rules; the scan reads uids and references where Godot looks |
| Grids | the TileSet lines per orientation, stagger axis and index; exact and fallback hex sizes; cell boxes; the shift on finite and infinite isometric maps and odd sizes |
| Oracle (headless) | Godot's `to_global(map_to_local(c))` equals `bt-core`'s cell centres within 0.01 px on every grid, negative cells included; every object's global position and shape points equal the editor's outline minus the shift |
| TileSet (headless) | custom data values and types read back exactly; animation frames land on the right cells and the island example loads with no errors; one-way and physics layers read back; navigation and occluder polygons; terrain mismatch counts equal Tiled's; complete edge and mixed sets paint seamlessly |
| Identity (headless) | a property test that adds, deletes and reorders never rename a survivor and ids never repeat; the version gate; the rename report; Godot does what the level-scene check predicts after a rename, a move and a delete; a scene whose helpers have no ids loads clean |
| Shared TileSet | ids stay as maps are added; exporting one map keeps what the others use; the same bytes in any order; a grid mismatch fails; a missing file in use is refused, and refused for `--all` when a user scene paints with it; Godot loads every scene after each export |
| Front door | the setup names the scene and the copies; the starter is written once and loads as an inheritor; the editor's export of a saved map equals `bt`'s byte for byte; a refused export on save leaves the save done with an Issue; Tiled's scene asks once |
| Real maps | Tiled's orthogonal, isometric, staggered and hexagonal examples and the packs' maps: import clean, no cell without tile data, identical twice and from both doors, timed |
| Windowed (ignored, run by hand) | Godot's picture beside better-tiled's for the packs' chambers, every orientation, tall tiles with and without y-sort, sprites, two maps through one TileSet, parallax from a camera at the window's centre |
| Verify | Godot is found in the right order; its output becomes messages; a scene broken by hand is reported with its cell; without Godot the message says where to set it and the tests skip through `bt-test-data` |
