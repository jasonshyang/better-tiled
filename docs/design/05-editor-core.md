# Editor core — `bt-editor`

*2026-10-09; the M1 part as built at the end of M1 (§1.1), the M2 part at the end of M2
(§1.2) and the M3 part at the end of M3 (§1.3), all 2026-10-10, and the M4 part at the end of
M4 (§1.4), 2026-10-11. The design of the crate
that is the editor without a window: what is open, how it changes, how it relates to the files
on disk, what the user can do to it, and what the editor remembers between runs. The high-level
design (`00-high-level-design.md`, §8) states the shape: an asset store, documents as views on
assets, transactions that may span assets, one action table, intents in and effects out, no egui
type anywhere. This doc says what each part holds and the rules it follows.*

## 1. Scope

`bt-editor` owns the editor's state and the logic that changes it. The window
(`06-application.md`) draws that state and feeds it intents; `bt` reaches it through the live
link; tests drive it directly. It is **synchronous and single-threaded**: every call runs on
the thread that owns the `Editor`, which in the window is the UI thread. Work that blocks
(reading files, hashing them, decoding images, scanning folders) is the caller's to run on a
worker; the results come back as intents. The crate therefore has no threads, no timers and no
channels of its own, and every behaviour is a function of what it was told.

Dependencies: `bt-core`, `bt-tmx`, `bt-edit`, `bt-terrain`, `bt-automap`, `bt-project`,
`bt-image` (the terrain helpers' jobs decode a tileset's image on a worker), `bt-utils`, `serde`
and `toml` for what is remembered. Not `bt-render`, `bt-script` (the window
runs scripts and hands the result in) or `bt-godot` (the export is an effect the window carries
out).

### 1.1 As built at M1

M1 built the part of the editor that opens, shows, saves and remembers maps; the rest of this
doc is the design the later milestones build. What exists, module by module:

| Module | What it holds |
|---|---|
| `store/` | `AssetStore` of `Asset`s (`AssetId`, kind Map, Tileset or Project, canonical path, `Root`, version, `OnDisk { version, content_hash }`, load warnings); `unsaved()`, `by_path`, `tilesets_of`, `refresh_tilesets` (§3) |
| `document/` | `Documents` (tab order, current), `Document` (asset, `Selection { current_layer, layers }`, camera `ViewState`, `History`, `ViewToggles { grid, highlight_current_layer }`) |
| `history/` | `Transaction`, `Tx` (copy-on-write roots), `transact`, `transact_merging`, `break_merge`, gestures (`begin_gesture`, `gesture_apply`, `end_gesture`, `cancel_gesture`), `undo`, `redo`, `travel_to`; the later-changes-win rule (`EditError::Blocked`, `ChangedSince`); histories bounded by count (500 by default), not yet by bytes |
| `actions/` | the action table (16 commands: Open, Save, Save As, Save All, Close, Quit, Undo, Redo, Zoom In, Zoom Out, Zoom Normal, Fit in View, Show Grid, Highlight Current Layer, Next and Previous Document), `Chord`, `Key`, `Modifiers`, `Keymap` with overrides from the settings |
| `files/` | `LoadJob` (run on a worker: the map, its external tilesets and the nearest project above it, each hashed) and `Loaded`; open, save (atomic, hashed), Save As (rebasing references as a transaction), close and quit with their questions |
| `settings/` | `Settings` (§13 as built), migration by `version`, `data_folder()` |
| `recovery.rs`, `session.rs` | §10 as built |
| `editor.rs`, `intent.rs`, `effect.rs`, `notice.rs`, `error.rs` | `Editor` with its builder, queries and `apply`; `Intent`, `Effect`, `Decision`, `Notice`, `EditError` |

The intents built: `Run`, `Open`, `Loaded`, `FilePicked`, `Decided`, `SelectDocument`,
`CloseDocument`, `SelectLayer`, `SetLayerVisible`, `SetLayerLocked`, `RenameLayer`, `SetCamera`,
`Recoverable`, `Tick`. The effects: `Load(LoadJob)`, `PickFile(Open | SaveAs)`, `Ask(Decision)`,
`Notify(Notice)`, `View(ZoomIn | ZoomOut | ZoomNormal | Fit)`, `WriteSnapshot`,
`DeleteSnapshot`, `Quit`. Questions are `Unsaved { names, purpose: Close | Quit }` and
`Recover { names }`, answered `Save`, `DontSave`, `Cancel`, `Recover` or `Discard`. Layer
visibility, lock and name are transactions, so they undo.

`Editor::builder().settings(..).recovery_folder(..).pid(..).mac(..).home(..).build()` fails
only when the settings' key bindings name an unknown command or key. The crate's dependencies
at M1 are `bt-core`, `bt-tmx`, `bt-utils`, `glam`, `log`, `serde`, `serde_json`, `thiserror`
and `toml`.

### 1.2 As built at M2

M2 made the editor edit: everything a level needs, from the action table to the tileset
editor's model. What it added or changed, module by module:

| Module | What it holds |
|---|---|
| `actions/` | the action table, 92 commands, made by one macro with const builders (`.on(context)`, `.keys`, `.keys_by_platform`, `.toggle`, `.tool`, `.repeating`, `.text_edit`); `enabled` with the reason a command cannot run (its context, a lock, nothing to act on, not built yet), `checked`, `execute`; the palette (commands, recent files, layers, "x, y" to go to a cell) ranked by Tiled's search score with a word found in a file's name counting double, recency, and half for a disabled entry; equal ranks list the shorter label first |
| `document/` | `Selection { current_layer, layers, tiles, objects, nodes, carried }`; selecting layers, tiles and objects (on a tile's collision shapes in the tileset editor's Collision mode); `carried.rs`, carried selections (below); `TilesetEditing { home, tiles, current, mode: Tiles \| Collision \| Animation }`, the tileset editor a document shows; the magic wand's and Select Same Tile's areas |
| `store/` | `TilesetHome { Asset(id) \| Embedded { map, slot } }`, where a tileset lives; `tileset_at`, `home_of_slot`; `Tx::tileset_at_mut` changes either kind |
| `tools/` | `ToolKind` (18 tools) with their `ToolFamily`; `ToolState` (the tool, the one before, the last of each family, the brush, random mode, size, selection mode, fill shape, the seed previews and paint share); `tool_family(doc)`, which is the object tools' while collision shapes are edited; strokes (`Stroke::Begin`, `Paint`, `Erase`, `Move`, `End`, `Cancel`), previews that are what gets painted, Bucket Fill, Shape Fill, the picker and capture |
| `objects/` | `ObjectHost::Map \| Tile`, where the object tools edit; picking by the object-selection preference; `SnapToggle` (Command toggles grid snapping, with Shift the fine grid, as Tiled's `SnapHelper`); `ObjectStroke` drags (`Move`, `Rotate` from the drag's two points, `Resize` by a handle, `MoveNodes`, and the Offset Layers tool's move); `selection_handles`; Edit Polygons' nodes (`node_handles`, `node_at`, `segment_at`, select, insert, delete, join, split); the object commands, the Objects panel's rows (`object_rows`), visibility, centring and reordering |
| `clipboard.rs` | tiles (copy, cut, delete, paste as the brush, paste in place) and objects (paste at the pointer or in place) |
| `layer_commands.rs`, `map_commands.rs`, `offset_map.rs` | the Layer menu, several layers selected, layers moved by a drag, opacity as one merging step; Crop to Selection, Autocrop, Resize Map (refused for infinite maps) and Offset Map (`MapOffset`, `LayerChoice`; locked layers are left and named) |
| `tileset_commands.rs` | Edit Tileset and back, the tiles chosen, the editor's mode, a tile's animation as one step, `edited_tileset`, and `collision_view`: a one-cell map of the tile and its shapes for the window to draw |
| `files/` | New Map and New Tileset (embedded, with Tiled's defaults), Add External Tileset (read on a worker, added as one step), tileset files (`.tsx`, `.tsj`) opened in tabs of their own and saved to their files |
| `watch.rs` | the watcher's model (`Observation`, `Found`, `WatchModel`): a file changed by another program reloads as a step, or asks when the open copy has unsaved changes |
| `recovery.rs` | unsaved tileset files are snapshotted too, and come back in their tabs |
| `settings/` | the watcher switch, the fine grid (1 to 128, a value outside refused when loaded or changed), snapping, object selection, enclosed rubber bands, a tool key that goes back; `ChangeSettings` checks the key bindings |
| `properties/` | the property-editing model of §14, with `TypeEdit` for the Custom Types editor (enums and their values, classes and their members, which are edited with the same `Change`s as properties); what each map document's Properties panel inspects (`Inspected::Map`, `Layers`, `Objects`, or while the tileset editor is open `Tileset` and `Tiles`), following the last thing chosen as Tiled's current object does; `attribute_rows`, `property_rows` and `member_rows` |

**Carried selections** are explicit state, as §4 designs them, but without a `TileSelection`
type: the selection holds `carried: Option<Carried { base, tiles, layers }>` beside its cells. A
transaction that changes anything but the carried tiles lets them go where they are (the
transaction sets a flag when it keeps them), and so does selecting other cells or layers.
Dragging inside the selection with a tool that selects tiles is a `Stroke` of kind `Move`; X, Y
and Z mirror and turn the selected tiles while such a tool is current; Delete and Cut drop the
carried tiles and show what they covered; undo brings the carry back with the selection.

**The tileset editor** edits a tileset where it lives. A tileset file opens in a tab of its own
(its document's asset is the tileset); a map's embedded tileset is edited in place of the map,
in the map's document, so its changes are steps of the map's history (Tiled opens it in a tab).
While the tileset editor is open, the Map and TileLayer contexts do not hold and ObjectLayer
holds only in Collision mode, so the map's commands say why they wait. The edit commands (Cut,
Copy, Paste, Delete, Select All and None, Invert, the flips and turns) belong to any document,
and their own rules say when there is nothing for them to act on.

The intents added: `RunPalette`, `CreateMap`, `CreateTileset`, `ResizeMap`, `OffsetMap`,
`EditTileset`, `StopEditingTileset`, `SelectTilesetTiles`, `SetTilesetMode`,
`SetTileAnimation`, `SelectLayers`, `MoveLayers`, `SetLayerOpacity`, `SelectTiles`, `PickTiles`,
`Stroke`, `Fill`, `ShapeFill`, `PickFromMap`, `Capture`, `SetFillShape`, `SetSelectionMode`,
`SelectObjects`, `InsertObject`, `ObjectStroke`, `PasteObjects`, `MoveObjectsToLayer`,
`SetObjectVisible`, `CenterOnObject`, `ReorderObjects`, `SelectNodes`, `InsertNode`,
`ChangeProperties`, `SetAttribute`, `EditCustomTypes`, `Observed`, `ChangeSettings`. The three
property intents carry a run number: the edits of one run of typing fold into one step, so no
event has to say the field was left (the design's single `EditProperties` is these three). The effects added: `Show(Dialog)` (New Map, New Tileset,
Preferences, the palette, Move Objects to Layer, Resize Map, Offset Map, Custom Types),
`PickFile` of a tileset image, an external tileset or a property's file, `PasteAtPointer`, `ReloadImages`, `WriteSettings`, the view
requests `CenterOnCell` and `CenterOn`, and the question `Conflict` (a file changed on disk
while unsaved) answered `KeepMine` or `Reload`.

**Custom types** live in the project asset only. Without a project file an untitled project
holds them for the run, as Tiled's `ProjectManager` does, and it is never in the quit prompt; a
project found later replaces it, with a warning when it held types. Types edits are steps of the
current document's history; Save, Save All and the quit prompt write a changed project file.
The project file is not watched yet (§18).

The crate now depends on `bt-edit` too.

### 1.3 As built at M3

M3 brought the rules into the editor. What it added, module by module:

| Module | What it holds |
|---|---|
| `tools/terrain.rs` | `TerrainChoice { home, set, color }`, the terrain painted, naming a set by where its tileset lives; `terrain_in_use(doc)`, the choice made ready for a document (the prepared `bt_terrain::Terrain` in that map's slot, the current tile layer, the terrains' names) or why it cannot paint (`NoTerrain`: none chosen, the tileset not the map's, no layer, not a tile layer, locked); `terrain_preview` and `terrain_fill_preview`, exactly what a dab or a fill places; `TerrainStroke` (`Begin`, `Paint { targets, mirror }`, `End`, `Cancel`), a gesture whose dabs draw from one seed and which says at its end what no tile fits, which terrain it pushed back and which other set it painted over; the terrain under a target taken as the one painted |
| `tools/` | `ToolKind::Terrain` (T); `ToolState` keeps the terrain choice, the terrain-fill switch of Bucket Fill and Shape Fill (picking a brush turns it off, as §6 wants), the terrain seed and where the last terrain stroke ended |
| `terrain_sets.rs` | the tileset editor's Terrain mode: `LabelTerrain`, the set and terrain labelling paints; `LabelStroke` (`Begin`, `Paint { marks }`, `End`, `Cancel`), lanes labelled or cleared, each drag one step, a label left with no terrain removed; `TerrainSetEdit`, one step per change of the sets: new set of each kind (Tiled's "Unnamed Set" with one terrain), remove, duplicate, rename; terrains added (Tiled's colours in turn, named "Terrain N"), removed and moved with the labels following, renamed, recoloured, weighted; the labelling choice follows every edit |
| `document/` | `TilesetMode::Terrain`; `TilesetEditing` keeps the terrain labelling paints |
| `inspect/`, `properties/` | `Inspected::TerrainSet` and `Inspected::Terrain`: in the Terrain mode the Properties panel shows the terrain labelled with, or its set for no terrain; a set's name, class and type (`Attribute::TerrainType`), a terrain's name, class, colour and probability, and custom properties of either under their classes' members (`ClassUsage::WANG_SET`, `WANG_COLOR`) |
| `terrain_helpers/` | the Terrain mode's helpers over `bt-terrain`'s labelling helpers. `TilesetImageJob`: the edited tileset's image decoded and read on a worker for Layout… or Auto-label, its `TilesetImageRead` brought back (a collection of images is refused: it has no one image). Layout…: per document, a `LayoutSearch` (`Searching`, `Found(FoundLayouts)` with `detect`'s fits and the samples, `Failed`), shown only while the tileset and its image's cut are the ones searched; `layout_guess(doc, into, fit)`, a block's terrains by its looks (`KnownTerrains::guess`); `label_all_blocks(doc, into)`, `label_all` over the fits of the set's kind; `LayoutTarget::{Set, NewSet(kind)}`; Apply ("Apply Layout": one block) and Label All ("Label All Layouts") each one step that makes a new set ("Unnamed Set") when asked, adds the terrains past the set's that the blocks use, named and coloured as new terrains are, takes a block's labels exactly (a tile the layout leaves unlabelled loses its label), gives a terrain, and the set, without a tile one of the block's tiles that is all of it, and makes the set the one labelled with; a block of the wrong kind, past the tileset, or with one terrain on both sides is refused with the reason. Auto-label: the terrain labelled with, from one tile (`label_with(Method::Auto, …)` on the worker), applied with `Proposal::apply_to` as one step ("Auto-label Terrain"), and a notice of how many tiles it was found on and how it was told apart. Patterns: `needed_patterns(doc, set)` (`patterns::needed` on the prepared set); a tile labelled with a whole pattern of the set labelled with, one step a click ("Label Terrain Pattern"), lanes of other kinds left out |
| `files/` | New Tileset's third source, `NewTilesetSource::RpgMakerAutotiles { sheet, sheet_size }` (square tiles of an even size, a whole autotile at least, else `problem`); `NewTileset::warning`, the sheet warning; `NewTileset::tileset` is `None` for it. `AutotilesJob` puts the sheet together on a worker (`rpg_maker::expand`, which writes `<stem> blob.png` beside it, then `rpg_maker::tileset`) and its `ExpandedAutotiles` adds the tileset to the map embedded, as one step ("New Tileset"), with a notice of what was written; undoing the step leaves the image |
| `automap/` | AutoMap in the editor. `AutoMapRules`, per map: the rules file found (the AutoMap layers' own, else a `rules.txt` beside the map, else the project's), the rules read (kept through a failed read, with the error), the files they came from with their hashes, a run or a start waiting for them, and the `LastRun` (rules, area, seed, the map before and right after, the report, the idle rule maps). `RulesJob` reads the rules on a worker through a loader holding the store's tilesets (`RulesLoaded` back); a read counts only if it is the latest. Rules are looked for the first time a map is current, read again when a file they came from changes, a `rules.txt` appears beside the map, or the project's classes would give a rule tile another `MatchType`. `automap(doc)`, Map ▸ AutoMap: one step ("AutoMap") over the selection or the whole map, its notice saying what the rules did or why nothing (the layers they read that the map lacks or that are empty, else no rule matched), tilesets the run added becoming store assets. `LiveEdit` (`Start`, `Stop`, `Reroll`, `Bind`, `AddRun`, `RemoveRun`), each one step: Start binds the names the rules use to the layers of those names and makes the outputs the map lacks; Stop makes the result what was painted. After every intent the result is derived again where the painted map, the setup or the rules changed (`after_painting` around a change of cells, else in full), and a map opened whose rules now make something else than its file holds becomes unsaved. `LayersToSave`, `write_layers_record`, `read_layers`, `snapshot_layers`: AutoMap layers saved, opened, moved on Save As and recovered through `bt-project`. The Explain tool's cell and its `CellExplanation`s (`ExplainedRun::{Live(run), LastRun, RunNow}`), made again only when what they explain changed |
| `document/`, `history/` | `Document::automap`, the AutoMap layers' `Setup`, part of every transaction (`AutoMapChange` before and after, beside the selection); a transaction that changes it gives the map a new version, so it is unsaved; `ViewToggles::show_painted` |
| `files/`, `recovery.rs` | a map opened with a project-file AutoMap record is what was painted, rebuilt from the file (`LoadJob::home`, `LoadedFiles::layers`); Save writes the rules' result and the record, removes a record left from before when AutoMap layers are off, and Save As takes the map's entry along; `SnapshotMeta::automap` carries the setup through recovery; project files are locked in the folder `EditorBuilder::lock_folder` names |

The intents added: `ChooseTerrain`, `TerrainStroke`, `CaptureTerrain`, `SetTerrainFill`,
`ChooseLabelTerrain`, `LabelStroke`, `EditTerrainSets`. A document that becomes current by any
means (opened, loaded, switched to, next in line when another closes) brings the tools of what
it edits: one rule, in `Editor::apply`.

For the terrain helpers: the intents `FindLayouts`, `ApplyLayout`, `LabelAllLayouts`,
`AutoLabel`, `LabelPattern`, and the workers' `TilesetImageRead` and `AutotilesExpanded`; the
effects `ReadTilesetImage(TilesetImageJob)` and `ExpandAutotiles(AutotilesJob)`, which the
window runs on workers; the queries `layout_search`, `layout_guess`, `label_all_blocks` and
`needed_patterns`. Tests: `tests/terrain_helpers.rs` (each step on tilesets drawn lane by lane,
its labels what `bt-terrain` computes, undone at once; refusals; a search of another tileset)
and `tests/autotiles.rs` (a sheet drawn in a scratch folder through the job and its result, the
tileset `bt-terrain` makes, one step; a sheet that cannot be put together). The crate now
depends on `bt-image` too, which the jobs decode tilesets' images with.

For AutoMap: the actions `AutoMap` (⌘M), `LiveAutoMap`, `ShowPainted` and `ExplainTool` (A,
`ToolKind::Explain`); the intents `RulesLoaded`, `EditLive` and `ExplainCell`; the effect
`LoadRules(RulesJob)`; the queries `automap_rules`, `shown_map` (what the canvas, its tools and
Save take), `live_layers`, `made_by_rules`, `explained_cell` and `cell_explanations`. AutoMap is
off while AutoMap layers are on, since it would write the rules' output into what was painted.
§16's `Run { bindings }` is the engine's own `LayerNames`, its unbound names temporary layers
(`decisions.md`, 2026-10-10). Tests: `tests/automap.rs` (rules beside a map read once it is
current; AutoMap as one step, over a selection, waiting for the rules; the reasons a run did
nothing; files changed, a failed read, a `rules.txt` appearing, a class changing a match type;
AutoMap layers started, painted under, bound, re-rolled, stopped, saved and opened, a map whose
rules changed, recovered work; the three explanations and a failed condition).

The crate now depends on `bt-terrain` (`decisions.md`, 2026-10-10): §1's first version kept the
terrain to three ids and left the solver to the window, which could not make the transaction
the editor makes for every tool.

### 1.4 As built at M4

M4 added §17, the live link's side of the editor, in `external.rs`:

- `document_at(path)`, the document showing a map by its canonical path.
- `prepare_external_edit(doc) -> Option<ExternalBase>`: the document, the map's version and
  path, the map as painted and as it shows, the AutoMap layers' setup, the rules the editor
  read with their file (`BaseRules`), the custom types, the external tilesets it has open with
  their unsaved edits (`KnownTileset`, so a script reading another map gets the editor's copy),
  and the selected cells and objects. `None` for a document that is no map.
- `apply_external_edit(ExternalEdit { doc, base_version, painted, setup, label })`: one
  transaction labelled by the script, refused during a stroke or drag (`Busy`) and when the
  version moved on (`Conflict`); external tilesets the result uses that the editor does not
  have open are opened with it. It returns the effects of `after_changes`, the epilogue every
  intent runs (AutoMap layers derived again, notices).
- `external_undo(doc) -> ExternalUndone { undo, effects }`, `undo` being `Undone { label,
  earlier }`, `AlreadyUndone { label }` or `Nothing`; `NotLatest { edit, latest }` names the
  person's edit since. The editor keeps the map each external edit left, by identity; a closed
  document forgets its external edits and marks.
- `set_marks(doc, marks, add)` and `marks(doc)`: `Mark { label, cells, color }`, a label marked
  again moving its mark, colours taken in turn from `MARK_COLORS` (5), an empty list without
  `add` clearing them. `is_stroking()` says a gesture is under way. `Intent::SaveDocument(doc)`
  saves a document whether or not it is current, for `bt save`.

Tests: `tests/external.rs` (one step labelled by the script and taken back; a conflict and a
stroke refused; `bt undo` only while latest, and the one before after it; marks replace, add,
move and clear).

### 1.5 As built at M5 (2026-10-15)

The action table adds Export, Export As, Godot Setup and Verify in Godot. `Effect::Godot`
carries `GodotOperation` and `DocumentId`; no exporter dependency enters this crate.
`Intent::RememberExport` and `Editor::remember_export` record the target in one transaction.
A successful map save emits `AfterSave`, including Save As and Save All. The window decides
whether export-on-save is configured and runs the exporter in the background.


## 2. The editor

```
Editor
├── store: AssetStore                 every loaded map, tileset, template, world, project (§3)
├── documents: Documents              the tabs: selections, cameras, histories (§4)
├── tools: ToolState                  the tool, the brush, the terrain, the snap mode… (§6)
├── actions: Keymap                   the user's key bindings over the action table (§7)
├── palette: PaletteState             the command palette's recent entries (§7.4)
├── files: Files                      recent files, the open project, loaded worlds, the loader (§8)
├── watch: WatchModel                 what to watch and what to make of a change (§9)
├── recovery: RecoveryPlan            which assets to snapshot and when (§10)
├── session: Session                  what the next start restores (§10)
├── settings: Settings                the preferences (§13)
├── stamps: StampStore                the saved brushes (§15)
└── notices: Notices                  what the user should be told (§12)
```

The surface is three kinds of function: **queries** (`&self`) that panels and tools read each
frame, **intents** (`apply(&mut self, Intent) -> Vec<Effect>`) that change state, and
**effects** the window must carry out and cannot be done here: show a dialog, pick a file, run a
worker job, flash a notice, repaint. Nothing in the crate reaches into the window.

## 3. Assets

```
AssetId(u64)                           unique for the process, never reused
Asset
├── id, kind: Map | Tileset | Template | World | Project
├── path: Option<PathBuf>               canonical; None for a new, unsaved asset
├── root: Root                          Arc<Map> | Arc<Tileset> | Arc<ObjectTemplate> | Arc<World> | Arc<Project>
├── version: u64                        bumped by every transaction that changes the root
├── on_disk: Option<OnDisk { version, content_hash, modified, read_only }>
├── missing: bool                       the file was deleted or moved away: the asset is the only copy
└── warnings: Vec<Warning>              from the load, for the Issues panel
```

- **Identity** is the canonical path. Opening a file that is already an asset returns the
  asset. An asset stays in the store while a document shows it or another asset depends on it
  (a map on its tilesets and templates, a world on nothing, the project on nothing); it is
  dropped when nothing does, unless it is dirty, in which case the quit prompt names it.
- **Dirty** is `on_disk.is_none() || version != on_disk.version || missing`. Undo restores the
  version a transaction recorded, so undoing back to the saved state makes the asset clean
  again, and a transaction that changes nothing is never recorded (§5). A recovered asset starts
  with no `on_disk`, so it is dirty until saved.
- **The store is the authority on tilesets.** A map's `TilesetRef`s hold `Arc<Tileset>`s; the
  tileset asset's root is the current one. When a map root is restored from a snapshot
  (undo, redo, travel) or taken from a worker that loaded it, `refresh_tilesets(map)` replaces
  every external `TilesetRef`'s `Arc` with the store's root for that path. Refreshing does not
  bump the map's version: the map did not change, its tileset did. Embedded tilesets live in
  the map and need no refresh.
- **Unsaved work** is one query, `store.unsaved() -> Vec<AssetId>`, whatever the kind. The quit
  prompt, Save All and the close prompts use nothing else.

## 4. Documents

```
DocumentId(u64)
Document
├── asset: AssetId                      a map or a tileset
├── selection: Selection
│   ├── tiles: TileSelection { region: TileRegion, carried: Option<Carried> }
│   ├── objects: Vec<ObjectId>
│   ├── layers: Vec<LayerId>            the multi-selection in the Layers panel
│   └── current_layer: Option<LayerId>
├── camera: ViewState { center: DVec2, zoom: f64 }
├── history: History                    the transactions made in this document (§5)
├── automap: Option<AutoMapSetup>       which rules run on which layers (§16); part of every transaction
├── marks: Vec<Mark>                    outlines `bt mark` asked for
└── view: ViewToggles                   show painted, highlight, grid…: this tab's View switches
```

`Carried { base: Arc<Map>, tiles: FloatingTiles, layers: Vec<LayerId> }` is the state of a
selection that carries its tiles (`04-editing-operations.md`, §9): the map without them and the
tiles where they are now. It is part of the selection, so a transaction records it before and
after, and undo brings the carry back with the selection. Nothing is inferred from pointer
equality.

A **tileset document** is a document whose asset is a tileset: its selection is the tiles
selected in the tileset view and the current Wang set; its history holds transactions on the
tileset asset. It is not a map with one tileset.

`Documents` keeps the tab order and the current document. Documents are identified by
`DocumentId` everywhere; nothing holds a tab index.

## 5. Transactions

```
Transaction
├── label: String                       "Paint", "Resize Map", "bt: paint water"
├── merge_key: Option<MergeKey>         consecutive transactions with the same key collapse
├── document: DocumentId                where it was made (whose history holds it)
├── selection: Before/After<Selection>
├── current_layer: Before/After<Option<LayerId>>
├── automap: Before/After<Option<AutoMapSetup>>
└── changes: Vec<AssetChange { asset: AssetId, before: Root, after: Root, before_version: u64, after_version: u64 }>
```

### 5.1 Making one

```rust
editor.transact(doc, "Paint", |tx| {
    let map = tx.map_mut(asset)?;                 // copy-on-write from the current root
    bt_edit::paste(map, &clip, at, &targets, mask)
})?;
```

`transact` runs the closure against copies of the roots it asks for, compares each touched
root with its original, and records a transaction when any differs; a closure that changes
nothing leaves no trace and returns its report. `transact_merging(doc, label, key, f)` does the
same but, when the last transaction of the document has the same key and nothing was undone
since, folds the change into it (typing in a field, dragging a slider); `break_merge(doc)` ends
the run (a field losing focus).

A **gesture** (a brush stroke, a drag) applies edits live and records once:
`begin_gesture(doc, label) -> Gesture`, `gesture.apply(|tx| …)` any number of times (each
replaces the current roots so every panel shows the stroke as it happens), `end_gesture`
records one transaction from the roots at the start to the roots at the end, and
`cancel_gesture` puts the start back. An external edit that lands while a gesture is open waits
(§17).

### 5.2 Rules

- **Atomic across assets.** A transaction that touches a map and a tileset (a terrain label
  made from a map tab), a map and a world (a resize that moves the map in its world), or a
  template and every map that instances it, commits all of them or none.
- **Recorded once, in the document where it was made.** Other documents that show a touched
  asset see the new root (through the store) and gain no history entry.
- **Undo restores roots, versions, selection, current layer and AutoMap setup**, in the
  document whose history holds the transaction. Redo restores the "after" side. A new
  transaction clears the redo side.
- **Later changes win.** A transaction is undone only while every asset it touched is still at
  the version it left (`after_version`); otherwise `undo` returns `Blocked { asset, by: the
  document and label of the later transaction }` and the window says which step must be undone
  first. The same holds for redo.
- **Tilesets refresh, histories never rewrite.** Restoring a map snapshot refreshes its
  tileset references from the store (§3). Save As, which rebases relative references, and
  "adjust tile indexes" after an image grew are transactions; nothing edits a recorded
  transaction in place.
- **Bounded.** A history keeps at most a configured number of transactions and an estimated
  byte budget; the oldest go first.
- `History::entries()` (labels, oldest first, with the current position) and
  `travel_to(doc, position)` serve the History panel.

## 6. Tool state

Editor-wide, stored once, never copied between documents:

```
ToolState
├── tool: ToolKind, previous_tool, last_tile_tool, last_object_tool
├── brush: Option<Brush>                bt-edit's; `brush_origin`, `random_seed`
├── brush_mode: Stamp | Random | TerrainFill
├── eraser_size: u32
├── terrain: Option<TerrainChoice { slot, wang_set, color }>, full_tile: bool, terrain_seed: u64
├── template: Option<PathBuf>           what the Template tool places
├── snap: SnapMode, fine_divisions: u32
├── selection_mode: SelectionMode       the toolbar's default for the selection tools
├── fill_shape: FillShape
├── quick_stamps: [Option<Brush>; 9]
└── last_stroke: Option<LastStroke { layer, cell }>, last_terrain_stroke
```

Rules that keep tools coherent, each one function here rather than a flag in a panel: setting a
brush by any means (the Tiles panel, a capture, a pick, a paste, a stamp) ends terrain-fill
mode; picking a tile switches to the brush tool; switching to a tile layer brings back the last
tile tool and an object layer the last object tool; a tool's own key pressed again goes back to
the previous tool when the setting says so.

## 7. Actions, keys and the palette

### 7.1 The action table

Every command is declared once:

```
ActionSpec {
    id: ActionId,                       a const enum variant; the table is checked exhaustive by a test
    label: &str, keywords: &str,        for menus and the palette
    context: Context,                   Always | Map | TileLayer | ObjectLayer | TileSelection | ObjectSelection | TilesetTab
    keys: &[Chord], keys_mac: &[Chord], the defaults
    kind: Command | Toggle | Tool(ToolKind),
    kms: Option<&str>,                  Tiled's id for .kms import and export
    repeats: bool,                      fires again while the key is held
    text_edit: bool,                    a text field handles it itself (Cut, Copy, Paste, Undo, Redo, Select All)
}
```

`Chord { modifiers: Modifiers, key: Key }` is the crate's own key type (letters, digits,
function keys, arrows, navigation keys, punctuation as the US layout names it), with text forms
`cmd+shift+k` for storage, Qt's portable text for `.kms` files, and the window's job to convert
to and from its toolkit's events. `Command` means ⌘ on macOS and Ctrl elsewhere.

### 7.2 The keymap

`Keymap { overrides: BTreeMap<ActionId, Vec<Chord>> }` over the table's defaults: `chords(id)`,
`chord(id)` (the first, which menus show), `set`, `put`, `remove`, `reset`, `reset_all`,
`is_customized`, `conflicts(id, chord)` (actions bound to the same chord whose contexts can
both be active, so S on tile layers and S on object layers is no conflict), `to_kms`,
`import_kms`. `key_text(id)` and `with_key(text, id)` name a command's key as it is bound, so
every hint and message in the window says the right key; a command without a key is named
without one. The keymap is passed to whoever needs it; nothing publishes it through a global.

### 7.3 Enabled, checked, why not, execute

- `enabled(&self, id) -> Enabled | Disabled(Option<Reason>)`: the context must match the
  current document (no document: only the always-on commands; a tileset document: the commands
  that make sense for a tileset), no lock may block it (§11), and the per-command conditions
  hold (something selected, a clipboard of the right kind, a layer that can be raised…). A
  reason is given when a key that does nothing would otherwise be a mystery.
- `checked(&self, id) -> Option<bool>` for toggles and tools.
- `apply(Intent::Run(id))` dispatches to the command's function, which makes transactions,
  changes tool state, or returns effects. Nothing inside opens a dialog or touches a file: a
  command that needs a file name returns `Effect::PickFile`, and the window brings the answer
  back as `Intent::FilePicked`.

### 7.4 Intents and the palette

`Intent` is the one way in: `Run(ActionId)`, `SelectTool`, `SetBrush`, `SelectLayer`,
`SelectObjects`, `EditProperties(targets, Change)`, `Gesture(…)`, `Open(path)`, `FilePicked`,
`Decision(DecisionId, choice)`, `FileEvent` (§9), `Loaded(…)`, `ExternalEdit` (§17),
`Tick(now)`, and so on. Panels and tools return intents; the window applies them.

The palette ranks `Entry`s (actions, files of the project and recent files, layers of the map,
stamps, project commands, an `x, y` go-to) with one fuzzy matcher (`bt-utils::text`), a bonus
for the entries run most recently and a penalty for disabled ones; the window only draws the
list.

## 8. Files

### 8.1 Opening

`apply(Intent::Open(path))` decides the kind (project, world, template, tileset by extension or
by sniffing a `.json`, else map) and returns the effect that loads it on a worker; `Loaded`
brings the result back and the editor finishes the open:

1. **Project adoption.** A map adopts the nearest `.tiled-project` above it (the first by name
   in a folder with several), searching upward through `bt-utils::paths::search_up`, stopping
   at the top of a repository and below the home folder, and only when no project is open.
   Opening another project asks to close the maps of the current one (Tiled's session switch)
   and reopens the maps the new project had open.
2. **Dedupe.** A map that is already open becomes the current document.
3. **World adoption.** A `.world` beside the map or one folder up is loaded, so its neighbours
   show.
4. **AutoMap layers.** The map's entry in `better-tiled.json` (through `bt-project`) restores
   the setup and the painted state; the result is recomputed and compared with the file, and a
   difference is a note and a dirty map (§16).
5. **View state.** The session's remembered camera and current layer for that file.

Templates open in the Template editor; tilesets open as tileset documents; worlds load and show
the current map's neighbours or open a map of theirs.

### 8.2 Saving

`apply(Intent::Run(Save))` on a map: the external tilesets of the map that are dirty are
written first, each a `bt-tmx` write (a tileset that cannot be written keeps its changes and
the map is saved anyway, with a notice, because Tiled never needs a tileset written with a
map); then the map, which is the rules' result when AutoMap layers are on; then its entry in
`better-tiled.json`; then the asset is marked saved (`on_disk` from the bytes written, so the
watcher ignores our own write); then the recent list, the world's rectangle for the map, and
`Effect::RepeatExport` when the setting says so. The order means a crash between steps loses
nothing that a second save would not repair. A save needing a name, a format or an unknown
extension returns a decision.

Save As rebases every relative reference and file property to the new folder as a transaction,
retargets the asset, and clears the export target as Tiled does. Reload is a transaction too
(so it can be undone) and keeps the unsaved edits of the map's tilesets. Close and quit return
`Decision`s (`Unsaved { assets, options: Save | DontSave | Cancel }`) rather than dialogs.

### 8.3 What the editor knows about files

`Files` holds the `bt-tmx` `Loader` (built with the project's custom types), the open project
with its scanned file list for the palette, the loaded worlds, the recent files and projects,
and the external commands of the project. Opening a map clears the loader's cache so that a
tileset changed on disk is read again; the store then takes the loaded `Arc`s or keeps the ones
it already has when the content is equal.

## 9. The watcher's model

The window polls the file system every 800 ms on a worker; the model decides what the results
mean.

```
WatchModel::watched() -> Vec<PathBuf>              documents, their tilesets, images, templates, rules files, the project file
WatchModel::observe(observations) -> Vec<FileEvent>  from (path, mtime, content hash | missing) per file
```

| Observation | Event |
|---|---|
| a document's file changed and the asset is clean | `Reload(asset)`: reloaded in place as a transaction |
| a document's file changed and the asset is dirty | `Decision::Conflict { asset, options: ReloadFromDisk, KeepMine }` |
| a document's file is gone | `Missing(asset)`: the tab is the only copy and shows it; the asset is dirty |
| a missing file is back with the same content hash | `Returned(asset)`: clean again |
| back with another content | `Decision::Conflict` (returned) |
| an external tileset changed on disk and no document edited it | `ReloadTileset(path)`: the store's root replaces it everywhere |
| an external tileset changed on disk and a document edited it | `Decision::Conflict { tileset }` |
| a tileset image changed size so that its column count changed | `Decision::AdjustTiles { tileset, from, to, maps }`: either answer records the new size; Adjust is a transaction over every map that uses it (`bt-edit::adjust_tiles`) |
| a tileset image changed otherwise, an image layer's image, a template | `ReloadImages`, `ReloadTemplate(path)` (instances follow as a transaction) |
| a rules file or rule map, or a project file | `ReloadRules(asset)`, `ReloadProject` (keeping unsaved custom types) |

Our own writes are recognised by content hash, never by time. Each decision names what would
be lost. The window shows one conflict at a time.

## 10. Recovery and session

- **Recovery.** `RecoveryPlan::due(now) -> Vec<Snapshot>` lists dirty assets to snapshot: two
  seconds after their last change, or every thirty seconds while they keep changing. A
  snapshot is what Save would write (the result map, the tileset text, plus the map's
  `better-tiled.json` entry so the painted state of AutoMap layers survives) with a `.meta.json`
  naming the original path and the run, in `<data>/recovery/<pid>-<asset>.snapshot`. Each run
  touches `<pid>.alive` there every ten seconds and removes it when it ends; a run counts as
  alive for a minute after. At start the window hands the folder's contents in; `orphans` keeps
  the snapshots of other runs without a live heartbeat, and the editor offers to reopen them as
  dirty assets without `on_disk` (beside the file when the file is open too). Recovered work is
  the document shown, whichever of it and the session's files loads last. A clean or closed
  asset's snapshot is deleted; a quit nobody answered keeps them, and
  `snapshots_for_unanswered_exit` gives the snapshots its latest changes still need, for the
  window to write on its way out. At M1 maps are snapshotted; tilesets and AutoMap entries come
  with the milestones that edit them.
- **Session.** `Session` holds the open project, the open files with their camera and current
  layer and which is current, the view switches, the panel layout, the per-file view states
  of the last hundred files, the maps open in each project, the loaded worlds and the preferred
  save format. The window writes it (debounced, atomically) whenever it changes and restores it
  at start when the setting is on and no files were given on the command line. At M1 it holds
  the open files and the current one, the twenty recent files, the camera (centre, and zoom in
  screen points per world pixel, so it means the same on every display) and current layer of
  the last hundred files, and the panel layout (kept, not yet used).

## 11. Locks

A locked layer, or a layer inside a locked group, keeps its tiles and objects. The rules, one
function each, are the only place that decides:

| Command | With locked layers |
|---|---|
| painting, erasing, terrain, fills, paste, lifting a selection | skip the locked layers, report them (`skipped_locked`) |
| object commands (move, delete, align, properties) on a selection | act on the unlocked objects, say what was left out and which layer holds the lock |
| Select All, Delete, Cut on a tile layer | refuse, naming the lock ("“trees” is locked: unlock it to delete its tiles") |
| moving objects into a locked layer, pasting there | refuse |

`lock_blocker(id) -> Option<Reason>` feeds `enabled`; `locks::holders(map, layer)` and the
sentence builders give every message the same wording.

## 12. Issues and notices

```
Issue  { severity: Error | Warning | Info, source: Map | Files | AutoMap | Godot, text, target: Option<Target>, fix: Option<Fix> }
Target = Layer | Object { layer, id } | Tileset(slot) | Tile { slot, id } | GodotExport
Fix    = Locate { Tileset | TilesetImage | LayerImage | Template }
Notice { severity, text, target: Option<Target>, detail: Option<String> }
```

Issues are built from the typed warnings of the crates (missing files are errors with a Locate
fix, load warnings are warnings), the rule problems of the current map, and the last Godot
export's notes, deduplicated by their typed identity, never by substring. The collection is
cached on the asset version and re-run every few seconds for files that may appear or vanish.
A notice is what the status bar flashes or a dialog shows; a problem is never a bare string in
a field, and the error dialog shows notices with their details.

## 13. Settings

Typed, `serde`, defaults for every field, a `version` key with migration functions, stored in
`<data>/settings.toml` and written atomically:

- **general**: autosave and recovery, reload changed files, restore the session, safe writing,
  export on save, the live link, Tiled's export options, the stamps folder;
- **interface**: grid colour, major grid, fine grid divisions, fade colour, object line width,
  natural sorting, tool shortcut toggles;
- **behaviour**: object selection (any layer, prefer the current, current only), wheel zooms,
  middle-button autoscroll, smooth scrolling, duplicate adds "Copy", tileset grid, pixel-perfect
  tile objects, select enclosed;
- **theme**: base colour, selection colour, font size, custom font;
- **keymap**: the overrides.

At M1 the settings are `general` (restore the session, safe writing, recovery, history length),
`interface` (grid colour, major grid, fade colour, object line width), `behaviour` (wheel
zooms), `theme` (dark, selection colour, font size) and `keymap`, at version 1; every table
refuses a key it does not know, so a misspelt setting stops the start with the file and the
reason.

Colours are plain RGBA. The data folder is `~/Library/Application Support/better-tiled` on
macOS, `%APPDATA%\better-tiled` on Windows, `$XDG_DATA_HOME/better-tiled` else; it holds
`settings.toml`, `session.json`, `recovery/`, `stamps/` (unless the setting points at Tiled's
folder) and `history/` (which `bt-script` writes for `bt undo`).

## 14. The property-editing model

What the Properties panel, the Custom Types editor and every properties dialog show and change,
UI-free:

- `PropertySet::of(targets)` for one or several things of one kind (map, layers, objects,
  tileset, tiles, Wang set or colour, project, world): each target's own properties plus what it
  inherits (class defaults, a tile's properties for tile objects, a template's).
- `rows() -> Vec<Shown { name, value: One(Property) | Mixed, holders, inherited_from, resettable }>`:
  every property any target has, marked when the values differ or some lack it.
- `Change { path: Vec<Step { Member(name) | Item(index) }>, op: Set(Property) | Reset | List(ListEdit) }`
  with `ListEdit = Push | Remove | Move | Duplicate`, and the operations Add, Remove, Rename,
  Convert and Paste; `apply(targets, change)` becomes one transaction over every target.
- The keep-type rule: a plain value typed into a field keeps each target's own type when it
  converts there and back unchanged, so a `7` typed over an int and a string sets `7` and `"7"`.
- `convert(value, to)` and `conversions(property)` follow Tiled's Convert To; enum values get
  their names or flags, class values expand to members with the class defaults dimmed, lists edit
  in place, object references resolve against the map, file values resolve against the owning
  file's folder.

## 15. Stamps

`StampStore` is a folder of Tiled `.stamp` files (`02-file-formats.md`, §13.5): `load()`,
`save(stamp)`, `rename`, `delete`, `add_variation`, each writing its file at once (Tiled's
behaviour, and Tiled reads the folder too). A stamp becomes a `Brush` by resolving its tilesets
against the map's (by file, or for embedded tilesets by name and image). The nine quick stamps
are brushes in `ToolState` and in the session.

## 16. AutoMap layers

A document's `automap: Option<AutoMapSetup { rules: PathBuf, runs: Vec<Run { bindings: rule
layer → map layer }>, seed }>` is part of every transaction. After each transaction and each
rules reload the editor hands the painted map and the setup to `bt-automap`'s layers module and
keeps the derived result (`shown_map(doc)`), recomputed by region for a region change and in
full otherwise. The canvas, the minimap, Save, the exports, recovery and the live link read
`shown_map`; "Show what I painted" is a view toggle. Rules come from the setup's file, else a
`rules.txt` beside the map, else the project's rules file; they reload when their files change
and a failed reload keeps the previous rules with a notice. Start, Stop (which bakes the result
into the painted map), Re-roll (a new seed) and binding changes are transactions.

## 17. External edits

The live link runs a script on a copy of the current state and hands the result back:

```
prepare_external_edit(doc) -> Option<ExternalBase>
apply_external_edit(ExternalEdit) -> Result<Vec<Effect>, ExternalEditError>
external_undo(doc) -> Result<ExternalUndone, ExternalUndoError>
```

An edit applies as one transaction labelled by the script when the map's version is still the
one the script started from; otherwise the window reruns the script on the new base, up to a
few times. `external_undo` undoes the last external transaction only while it is the latest in
the document. `set_marks(doc, marks)` replaces or adds the outlines `bt mark` asked for.

## 18. Worlds, templates, projects

- A **world** is an asset; moving a map in it with the World tool or resizing a map that is in
  one is a transaction recorded in the map's document and touching the world (and the map).
  World edits therefore undo with the map's history, in order, and the World tool has no undo
  stack of its own.
- A **template** is an asset; editing it in the Template editor is a transaction on the
  template, and every open map's instances follow in the same transaction (fields they do not
  override). A template changed on disk reloads the same way.
- The **project** is an asset; custom types edited in the Custom Types editor are a transaction
  on it; the project file is watched and reloaded, keeping unsaved types.

## 19. Module layout

```
crates/bt-editor/src/
├── lib.rs           mod lines and pub use
├── editor.rs        Editor, apply(Intent) -> Vec<Effect>, the queries
├── intent.rs        Intent
├── effect.rs        Effect, Decision
├── store/           asset.rs, store.rs (ids, roots, versions, dirty, refresh), dependencies.rs
├── document/        document.rs, selection.rs, carried.rs, documents.rs
├── history/         transaction.rs, history.rs, gesture.rs
├── tools/           state.rs (ToolState and its rules)
├── actions/         table.rs (ActionSpec), chord.rs, keymap.rs, kms.rs, enabled.rs, execute.rs, palette.rs
├── files/           open.rs, save.rs, project.rs, worlds.rs, templates.rs, recent.rs
├── watch.rs         WatchModel and FileEvent
├── recovery.rs      RecoveryPlan, snapshots, orphans
├── session.rs       Session
├── locks.rs
├── issues.rs        Issue, Notice, collection
├── settings/        settings.rs, migrate.rs, storage.rs
├── properties/      set.rs (PropertySet, Shown), change.rs (Change, apply), convert.rs
├── stamps.rs        StampStore
├── automap.rs       setup, rules loading and reload, the derived result
└── external.rs      the live link's edits and marks
```

## 20. Tests

| Test | What it pins |
|---|---|
| Transactions | random sequences of edits, undo and redo on several documents and assets leave every root, version, dirty flag and selection as a plain model predicts; undo back to the saved version is clean; a transaction that changes nothing is not recorded; merge keys fold and `break_merge` splits |
| Cross-asset | a tileset edit from a map tab shows in every map using it and undoes atomically; a later edit of that tileset in another document blocks the undo with the right name; a template edit updates every instance in one transaction |
| Refresh | restoring a snapshot picks up the store's tileset; a refresh does not dirty a map |
| Actions | the table is exhaustive; every default chord is unique within a context; `.kms` round trips; `enabled` for every command in every context; `why_not` texts |
| Files | open, save, save as, reload and close on temporary folders with external tilesets, read-only files and folders, a tileset that cannot be written, AutoMap layers, projects above and below a repository top, worlds beside the map |
| Watcher | the table of §9, observation by observation, including our own writes and a file deleted and returned |
| Recovery and session | the schedule, orphans by dead process, what a snapshot holds, a session written and restored |
| Locks | each rule of §11 with a lock on the layer and on its group |
| Properties | multi-target rows, mixed values, inherited values, the keep-type rule, every conversion, list edits |
| Settings | defaults, every migration from an older version, atomic write |
| External edits | version conflicts, undo of the latest only, marks |

Everything runs without a window and without a GPU.
