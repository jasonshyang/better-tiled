# The application — `apps/better-tiled`

*2026-10-09; the M1 part as built at the end of M1 (§1.1), the M2 part at the end of M2
(§1.2) and the M3 part at the end of M3 (§1.3), all 2026-10-10, and the M4 part at the end of
M4 (§1.4), 2026-10-11. The design of the window:
tabs, menus, panels, dialogs, tools and the canvas, built on `bt-editor` (`05-editor-core.md`)
and `bt-render` (`03-rendering.md`) with eframe and egui. The high-level design
(`00-high-level-design.md`, §9) states the rule: the application holds no map logic; if it needs
no egui type, it does not live here. This doc says what the window is made of, how a frame runs,
how keys and pointer reach the tools, and how the window talks to the editor core.*

## 1. Scope

The application turns the editor's state into pixels and widgets and turns input into intents.
It owns the window, the frame loop, the toolkit's focus, the dialogs, the worker threads, the
timers, the socket server for `bt`, and the platform glue. It never calls `bt-edit`,
`bt-terrain` or `bt-automap` on a map directly: a tool sends an intent, the editor core makes
the transaction.

Dependencies: `bt-editor`, `bt-render`, `bt-image`, `bt-script` (to run the live link's
scripts), `bt-live`, `bt-godot`, `bt-tmx`, `bt-core`, `bt-utils`; `eframe`/`egui` with the
wgpu backend, `egui_wgpu`, `rfd` for file dialogs, `notify`-free polling for the watcher,
`log`/`env_logger`, and on macOS `objc2`/`objc2-app-kit` for the quit guard.

### 1.1 As built at M1

M1 built the shell that opens, shows, pans, zooms, saves and remembers maps; tools, the other
panels, the watcher and the live server are the later milestones'. What exists:

- **Startup** (`main.rs`, `cli.rs`, `app/startup.rs`): files to open, `--screenshot OUT.png`,
  `--bench`, `--help`, `--version`; the data folder; a settings file that does not parse or
  names an unknown setting stops the start with its path and the reason; a session that cannot
  be read is reported in the window and left out; eframe with the wgpu renderer, vsync and no
  toolkit persistence. A check (`--screenshot`, `--bench`) opens only its files and neither
  restores nor writes the session nor asks about recovery.
- **The frame** (`app/frame.rs`, `app/effects.rs`): `logic` delivers worker results and ticks
  the editor every frame (and every second while something is unsaved); `ui` handles a close
  request, opens dropped files, routes the keyboard when no modal is open, draws the menu bar,
  status bar, Layers (left) and Tiles (right) panels, the tab strip and canvas or the welcome
  screen, then the modals, applies the frame's intents in order, carries out the effects and
  ends the frame (session written a second after its last change, title, flash, closed tabs'
  canvases forgotten, the check's next step).
- **The canvas** (`canvas/`): the camera of §5 with Tiled's zoom steps for keys and wheel
  notches (animated about the pointer), pinch and ⌘-scroll zooming at once, trackpad scroll
  panning, middle-drag and Space-drag panning, fit at 90 %; the grid (with the major grid),
  the map border and the hovered cell as vector items; one view drawn through the paint
  callback, with the renderer in the callback resources and the image store shared with the
  Tiles panel. Squares of a far view still to be drawn and the next animation frame ask for
  repaints from the callback.
- **Keyboard** (`focus.rs`): only text fields keep the keyboard; Tab moves nothing outside one;
  keys go through the keymap with exact modifiers; plain keys are not shortcuts while typing,
  nor are the keys a text field handles itself; repeats only for the commands that repeat; ⌘=
  and ⌘- zoom the map, not the interface.
- **Panels** (`panels/`): Layers (the tree topmost first, visibility, lock, folding, selection,
  rename in place) and Tiles (tileset tabs, a zoom per tileset, an atlas in its own columns, a
  collection wrapped to the panel, only the rows in view drawn, a selected tile, a line saying
  why a tileset's image could not be read). Tile textures are uploaded once from the shared
  store and shrunk when larger than the GPU allows.
- **Modals** (`dialogs/`): the editor's questions and errors with details; Enter gives the
  default answer, Escape cancels when Cancel is one.
- **Files and workers** (`files/`, `workers.rs`): dropped files; `rfd`'s asynchronous dialogs
  waited for on a worker; a thread per load or dialog with one result channel and a waker; one
  writer thread for snapshots, their deletion and the session, in order, that touches the
  heartbeat every ten seconds and is joined on exit.
- **Quit** (`platform/quit.rs`): §12 as built.
- **Checks** (`dev.rs`): §13 as built.

The menus are the window's arrangement of `bt-editor`'s action table (File, Edit, View, with
Open Recent), each item asking the editor whether it is enabled and checked and the keymap for
its key. Not yet built: tools and gestures, the toolbar, the other panels, the panel layout in
the session, the watcher, the live server, the command palette, the `BT_SCREENSHOT_*` hooks.

### 1.2 As built at M2

M2 made the window edit. What it added:

- **Gestures** (`canvas/gesture.rs`): `Hover`, `Press`, `Drag`, `Release` (one the canvas never
  saw is made up), `DoubleClick`, `ModifiersChanged`, `Cancel` (Escape), `Confirm` (Enter) and
  `Leave`, each with the pointer's screen point, its world point, and its pixel and cell **on
  the current layer**: the layer's offset (its groups' included) is taken off, as Tiled's tools
  do, and the grid, the tile selection and the hovered cell are drawn at that offset. Escape
  and Enter reach the tool only while no text field has the keyboard and no dialog is open.
- **Tools** (`tools/`): the `Tool` trait (`handle`, `overlay`, `hint`, `reset`) over a
  `ToolContext`; the `ToolBox` makes the current one active and resets the one before. Built:
  Brush, Eraser, Bucket Fill, Shape Fill, Picker, Rectangular Select, Magic Wand, Select Same
  Tile (dragging inside the selection moves its tiles, `carry_drag.rs`), Offset Layers, Select
  Objects (with resize and rotate handles), Edit Polygons, Insert Rectangle, Ellipse, Capsule,
  Point, Tile, Text and Insert Polygon. §6 below as built: Insert Polygon is Tiled's one tool,
  the first point closing a polygon and a right-click or Enter finishing a polyline (no
  separate Insert Polyline, no Backspace); the selection modes at a press are Tiled's (Shift
  adds, Command subtracts, both intersect); Command toggles snapping for object gestures and
  Command with Shift the fine grid; Escape with nothing under way in an insert tool goes back
  to Select Objects, as in Tiled.
- **Overlays** (`canvas/overlay.rs`): marching ants for the tile selection (amber while it
  carries its tiles) and for the selected objects' frames in every tool; each tool's own
  vectors and previewed tiles.
- **The toolbar** (`app/toolbar.rs`): the tools of the family the document takes (the object
  tools in the collision editor, without Insert Tile and Insert Text, as Tiled's collision
  editor), Offset Layers on a map, and the options of the current tool where it is shown.
- **Menus** (`menus.rs`, `app/menu.rs`): File, Edit, View (with snapping), Map, Layer, Objects
  and Tools from the action table; the panels' context menus draw from the same entries, and a
  disabled entry shows its reason on hover.
- **Panels** (`panels/`): Layers and Objects are tabs of the left group, Tiles is on the right.
  Layers: several layers (Shift picks a range, ⌘ adds or removes), dragged to a new place or
  into a group, a context menu with every layer command, a strip of buttons, an opacity slider
  for the selected layers (one step per drag). Objects: each object layer's objects in the order they are
  drawn, a filter by name, class, kind or id, an eye per object, a double-click centres the view,
  dragging reorders. Tiles: a click or a dragged rectangle makes the brush; Edit Tileset.
  Properties, below Tiles (alone on the right in a tileset's tab): the built-in attributes with
  Tiled's labels and the custom properties, mixed values shown as mixed, values inherited from
  a class, a template or a tile dimmed until set and reset with a button; a field per type
  (text, numbers, a checkbox, a colour with "none", a file typed or chosen, an object's id, an
  enum's combo or flags, a class's members, a list edited in place); Add with a name and a type,
  and a row's menu with Remove, Rename, Convert To, Copy, Paste and Reset. Escape in a field puts
  its value back. A text object's text is edited here, on one line, rather than on the canvas.
- **The tileset editor** (`tileset_editor/`): shown in place of a map, or in a tileset's tab,
  with Tiles, Collision and Animation. Collision draws a one-cell map of the tile and its shapes
  on a canvas of its own (`CanvasRole::Collision`: no cells to select, its own camera) and edits
  them with the object tools; Animation lists the frames with their durations, moves and
  removes them, adds a frame per tile clicked in a palette, and plays a preview. While a map's
  tileset is edited in its place, the left panels wait.
- **Dialogs** (`dialogs/`): the command palette (⇧⌘P), New Map, New Tileset (an image picked
  through the file dialog), Preferences (General, Interface, Keyboard with search and a recorded
  key, Theme), Resize Map (a preview whose old map drags to set the offset), Offset Map, Move
  Objects to Layer, and the Custom Types editor (View ▸ Custom Types Editor: enums with their
  values, storage and flags; classes with their members, "use as" and colour; it needs an open
  document, whose history holds its steps).
- **Files** (`files/`): the watcher (a thread polling the files the editor names every 800 ms,
  reporting only what changed), the file dialogs' filters (images, tilesets).

`TestWindow` gained the pointer (`click_cell`, `drag_cells`, `click_world`, `drag_world`,
`click_on_screen`, `drag_on_screen`), answers to file dialogs, polling the watcher, and places
to find on screen: tiles of the Tiles panel, of the tileset editor and of the animation
palette, and points of the collision view.

### 1.3 As built at M3

M3 brought the rules into the window. What it added:

- **The Terrain tool** (`tools/terrain.rs`, T): the target under the pointer from
  `bt_terrain::hit_test` (the map's screen space less the layer's offset, as Tiled's brush
  measures it), a preview of exactly what a dab places with the cells no tile fits outlined in
  red and the reason in the status bar; a drag paints the targets `bt_terrain::Stroke` gives as
  one step; ⌘ paints whole tiles, Shift a line from where the last stroke ended, ⌥ the half turn
  around the map's centre too; a right-click takes the terrain under the pointer.
- **The fill tools**: Bucket Fill and Shape Fill get a "Terrain fill" toggle and outline the
  cells no terrain tile fits.
- **Panels**: the right group's upper part is two tabs over one current tileset, Tiles and
  Terrains; the Terrains panel lists the tileset's terrain sets, each with an erase row and its
  terrains (colour, the tile that stands for it, name), and a click chooses one and the Terrain
  tool.
- **The tileset editor's Terrain mode** (`tileset_editor/terrain.rs`): the tiles with the chosen
  set's labels drawn over them and the lane under the pointer outlined, found as Tiled's tileset
  view finds it (a corner set's quadrant, an edge set's triangle, a mixed set's ninth, the
  middle nothing), on an isometric tileset on the diamond Tiled maps each tile to; a drag labels
  lanes, the right button clears them; beside the tiles, the sets with New (corner, edge or
  mixed), Rename, Duplicate and Remove, and the chosen set's terrains with Add, a colour button,
  a weight, Rename, Move Up, Move Down and Remove. The Tiles and Terrain modes share one tile
  grid (`draw_tiles`).
- **The Terrain mode's helpers**, buttons below the sets; one is open at a time and takes the
  tiles' clicks while it is. **Layout…** (`tileset_editor/layouts.rs`), a panel beside the sets:
  the blocks found in the tileset's image (looked for on a worker when it opens, and on Look
  again) listed and outlined on the tiles; the layouts, each with a picture of its tiles' lanes;
  a block chosen by a click on a block found, or on the tile where the chosen layout's block
  starts, outlined boldly; the set it goes into (the sets of the layout's kind, the one labelled
  with first, or a new one) and its two terrains, guessed from its looks or chosen (the set's, a
  new one, nothing under); Apply and Label All. **Auto-label…**: the next tile clicked is the one
  all of the terrain labelled with; the status bar says how many tiles it was found on.
  **Patterns** (`tileset_editor/patterns.rs`), a panel beside the sets: what the chosen set
  needs, each pattern drawn as a tile of its lanes and framed in red when no tile has it (Only
  the missing ones lists those alone), worked out again only when the tileset changes; a click
  chooses one and each tile clicked then takes it, one step a click; a set needing more than can
  be listed says how many.
- **New Tileset** offers RPG Maker autotiles as a third source: the sheet, one tile size
  (guessed from the sheet's shape), how many autotiles it holds and the image they are written
  to, and the sheet warning when it applies; after OK the sheet is put together on a worker.
- The effects `ReadTilesetImage` and `ExpandAutotiles` run on workers. Tests
  (`app/terrain_helpers_tests.rs`): each helper's flow as a person clicks it on tilesets drawn
  lane by lane, Layout…'s Apply, a block chosen by hand and Label All, Auto-label…, Patterns
  (and a set too large to list), and New Tileset from an RPG Maker sheet, checked on the
  editor's state.
- **The AutoMap panel** (`panels/automap.rs`), a third tab of the left group: Run AutoMap with
  its key; the cell the Explain tool chose and its explanations; where the rules come from,
  whether they are being read and why a read failed; their problems (open at first only when
  one is more than a note), the rule maps with their rule counts, and the last run (its matches
  over the whole map or the selection, the cells it changed, each rule map's matches, the rule
  maps that matched nothing folded into one line with each one's reason, the outputs locks kept
  out, its problems).
- **Live AutoMap** (`panels/automap_live.rs`), the panel's section for AutoMap layers: on or
  off, the seed and Re-roll, Show what I painted, and per run each name the rules use with what
  they do with it (reads, writes, reads and writes, places objects on) and a chooser of the
  map's layers of the right kind or a temporary layer; a name bound to an empty layer is marked
  "empty"; Add a run, Remove this run; the runs' problems. The canvas draws, and its tools read,
  the editor's `shown_map`; the status bar says of the hovered tile of the current layer whether
  it was painted or made by the rules.
- **The Explain tool** (`tools/explain.rs`, A, beside Offset Layers): a click chooses the cell,
  outlined, and brings the AutoMap panel forward. Its view (`panels/automap_explain.rs`), per run
  explained (each Live run, the last run, or a run now): each layer the rules write before and
  after, the writes in order, the rules applied and those held back by Probability or overlap,
  those that came close (a condition naming the tiles here holds) with the conditions that
  failed, and how many others could write the cell, naming the layers they needed tiles on when
  every failed condition found the cell empty; the tiles no rule names. Open, beside each rule
  applied or that came close, opens its rule map in a tab of its own.
- The Terrains panel numbers the terrains a set leaves unnamed ("Terrain 1"), as every set of the
  packs does. `Startup::lock_folder`: the runtime folder project files are locked in, passed to
  the editor; tests use a scratch folder. Tests: `app/automap_tests.rs` (⌘M and the panel, Live
  bound from the panel, the Explain tool) and `app/m3_tests.rs`, the milestone's measure on the
  Highlands pack.

### 1.4 As built at M4

M4 added the live server of §11 (`11-live-link.md`, §14, has the module table):

- **The listener** (`live/`): started in `logic` while the preference "Let bt work with the
  open maps" (`General.live_link`, on by default, in Preferences ▸ General) is on, stopped when
  it is turned off, and started again within ten seconds when its socket vanished; a listener
  that cannot start says why once, as a warning. `Startup::live_folder` is where it listens:
  the user's live folder, none in checks, a folder of the test's own in tests.
- **Jobs** (`app/live.rs`, in `logic`, so they are answered while the window is hidden): the
  status of every open document in `bt`'s words (the tool and its brush, the selection as runs,
  the cells in view and under the pointer of the map shown, the last six undo labels, the
  marks, the World tool and where the keys go); a snapshot of a map as it shows, with its dirty
  tilesets; a base for an edit and its result applied, waiting out a stroke or drag; `bt undo`
  of the last external edit; marks, focusing the view only on the map shown; `bt open`,
  answered once the document is there; `bt save` through `Intent::SaveDocument`, an unwritten
  tileset reported by name; a screenshot, answered when its picture arrives, refused while the
  window is minimised.
- **The canvas** draws the marks (each tinted and outlined in its colour, labelled above its
  first row's left end) and, for three seconds after an edit of `bt`'s lands, the changed area's
  outline fading; the status bar flashes `bt: LABEL`.

Tests: `app/live_tests.rs`, eight end-to-end tests through a socket of the test's own (listed in
`11-live-link.md`, §14); `TestWindow::with_live_link` and `ask_bt`, which steps the window while
`bt`'s client waits on a thread.

### 1.5 As built at M5 (2026-10-15)

`app/godot.rs` owns setup, background export/verify jobs, a caller-owned cached exporter,
refusal choices and per-document export Issues. The worker channel wakes the window and
serializes writes through the exporter mutex; a save during an export queues the latest map.
File's menu and the command palette expose Export to Godot, Export As, Godot Setup and Verify
in Godot. Export As uses the shared asynchronous file dialog. Export Issues remain visible
after the status flash, with links to source layers, objects and tiles. Automatic failures do
not open a dialog or reverse a successful save. The setup sheet provides a background preview
before settings are stored with the project-file lock.


## 2. Startup

`main` parses the command line (files to open; `--screenshot OUT.png` opens, draws, saves the
window and quits; `--bench` pans and zooms through the open map and prints frame times and
renderer stats), sets up logging (the console panel tees the log), finds the data folder,
reads `settings.toml` and `session.json` through `bt-editor`, and launches eframe with the wgpu
backend, vsync on, no toolkit persistence (the session is ours), and the window title and
icon. `App::new` builds the `Editor`, creates the renderer once in the paint callback's
resources, restores the session when no files were given, hands the recovery folder to the
editor and shows the recovery prompt when it has orphans, starts the live server when the
setting is on, and opens the files given.

## 3. The window

```
App
├── editor: bt_editor::Editor
├── window: Window { tab_order, panels: PanelLayout, left_tab, right_tab, clear_view, fullscreen, title }
├── canvases: HashMap<DocumentId, Canvas>        per document: camera, gesture, tool instance, previews, hover
├── tools: ToolBox                                one instance per ToolKind
├── panels: Panels                                per-panel UI state (scroll, filters, expanded rows, search)
├── dialogs: Dialogs                              every dialog's UI state
├── modals: Vec<Modal>                            the modal stack (§9)
├── workers: Workers                              loading, hashing, scanning, exporting; results as intents
├── live: Option<LiveServer>
├── focus: Focus                                  §4
├── perf: FrameStats
└── platform: Platform                            quit guard, menu, reveal, open-with
```

### 3.1 A frame

eframe calls `logic` before and `ui` for every frame; the order inside is fixed:

1. **Logic** (runs even when the window is hidden): deliver worker results and live-link jobs
   as intents (§11), tick the editor (`Intent::Tick`), which syncs AutoMap layers, decides
   recovery snapshots (handed to a worker) and expires notices.
2. **Close requests** from the window or the platform quit guard: `Intent::Run(Quit)`; a
   decision about unsaved work becomes a modal.
3. **Input**: dropped files become `Open` intents; the keyboard is routed (§4) and shortcuts
   dispatched; a modal open means no shortcuts but the palette's own.
4. **Chrome**: the menu bar (the window's arrangement of `bt-editor`'s action table, each item
   asking `enabled` and `checked`), the toolbar (tools of the current layer family, their
   options, Live, Grid, Highlight, World), the status bar (§10).
5. **Panels**: the left group (Layers, Objects, Project), the right group (Tiles, Terrains,
   Stamps, History, AutoMap) and the Properties panel, each drawing from queries and returning
   intents; a tileset document shows the tileset editor in the centre instead.
6. **Centre**: the tab strip and the current canvas (§5), or the welcome screen.
7. **Modals and dialogs** (§9), then the command palette.
8. **End of frame**: apply the intents collected, carry out the effects returned, remember
   whether a text field has the keyboard, write the session when it changed (debounced), update
   the title and the quit guard's "unsaved" flag, record frame stats.

Intents collected during a frame are applied at its end, in order, so a panel never sees a
state another panel changed halfway through the frame.

### 3.2 Repainting

egui repaints on input. The window asks for more only when something will change without
input: the renderer's next animation frame (`request_repaint_after`), a worker result or live
job (the waker requests a repaint), a flash message expiring, a camera animation in flight, the
watcher's 800 ms tick and the recovery schedule. An idle map draws no frames, and a map with
water animates at the water's rate, not at 120 Hz.

## 4. Keyboard

### 4.1 Focus

The toolkit gives the keyboard to whichever widget has focus; left alone, a Tab from the map
would focus a button, plain keys would stop reaching the map, the arrows would walk the panels
and Space would press the button. `Focus` enforces the rules of the high-level design, once per
frame, before anything is drawn:

- Only text fields and the tileset editor's canvases (collision shapes, animation frames) keep
  the keyboard. Any other focused widget surrenders it.
- In a text field, Tab and Shift-Tab go to the next or previous text field, passing at most a
  few non-text widgets; from the map, Tab moves focus nowhere.
- Pressing, scrolling, zooming or Space-dragging the map, and picking tiles, end typing in a
  field. A space typed with the pointer over the map is held back until it is clear the Space
  did not start a pan, then typed.
- The Escape that leaves a field does nothing else: it is recorded for the frame, and the
  canvas and panels check it before acting on Escape.
- While a modal is open the toolkit's default focus applies: Tab walks the dialog, Space
  presses its buttons.
- A tileset editor canvas gives the keyboard back as soon as the pointer is over the map.

### 4.2 Shortcuts

One router: the toolkit's Cut, Copy and Paste events are turned back into key presses so they
go through the keymap like everything else (a paste of TMX text is adopted as the clipboard);
"typing" is a focused text field, a tileset canvas with the keyboard, a Tab in transit or the
pointer over the Custom Types or Template editor, and plain keys (no ⌘/Ctrl) are not shortcuts
while typing; the text-edit keys are a text field's own while one is focused; the tileset
editor takes its own commands while its canvas has the keyboard; then every action's chords
are matched with **exact modifiers** (⌘Z never fires for ⇧⌘Z, a shifted symbol counts as its
key); a disabled action does not consume its key unless it has a reason, which is flashed;
repeats are dropped except for the actions that repeat; a match becomes `Intent::Run`. The
quick stamps are actions like any other, so they can be rebound.

## 5. The canvas

Each document has a `Canvas`:

- **Camera**: centre and zoom, with an animated zoom anchored on the pointer (a short time
  constant), a range of 1/32 to 64, snapping of the origin to device pixels so what the GPU
  draws is what the overlays measure; pan by trackpad scroll, Space-drag and middle-drag;
  wheel zoom or scroll per the setting; fit, zoom to 100 %, go to a cell, reveal cells.
- **Input normalisation**: raw toolkit events become `Gesture`s in map terms, each carrying the
  position in screen, pixel and tile space, the modifiers at the gesture's start and now, and
  the button: `Hover`, `Press`, `Drag { start, current }`, `Release`, `Click(count)`, `Key`,
  `ModifiersChanged`, `Cancel`. Every pointer move is handled, so fast strokes skip no cell; a
  missed release is synthesised.
- **Tool dispatch**: the current tool's `handle(gesture, cx)`; the World tool and navigation
  are tools too. Right-click meanings are the tool's.
- **Overlays**: the tool's `overlay` fills `VectorItem`s and `PreviewTile`s for the renderer
  (`03-rendering.md`, §6): grid and map border, the hovered cell, object outlines and names
  by class colour, selection ants (amber while carrying), the brush footprint (faint where the
  selection masks it, red outside a finite map), fill and terrain previews with red cells
  where no tile fits, capture and erase rectangles, handles and rubber bands, `bt` marks and the
  outline of the last `bt` edit while it fades, the Explain overlay, world frames and resize
  handles, the pointer tag with a dragged size.
- **Rendering**: the renderer is called through `egui_wgpu::CallbackTrait` with one
  `FrameInput` per visible view: the current map and, when a world is loaded, its neighbours as
  further views with their own cameras (clicking one switches to it). The minimap is a small
  view at impostor zoom, rebuilt by the renderer, shown when the map does not fit.
- **Hover**: the cell, pixel, tile with flips, painted-versus-made tile and object under the
  pointer go to the status bar.

## 6. Tools

```rust
trait Tool {
    fn kind(&self) -> ToolKind;
    fn availability(&self, cx: &ToolContext) -> Availability;        // enabled, or a reason
    fn options(&self) -> ToolOptions;                                 // the toolbar's toggles and choices for this tool
    fn handle(&mut self, gesture: &Gesture, cx: &mut ToolContext) -> Handled;
    fn overlay(&self, cx: &ToolContext, out: &mut Overlay);
    fn status(&self, cx: &ToolContext) -> StatusHint;                 // the hint and the modifier meanings
}
```

`ToolContext` gives read access to the editor's queries and the canvas's camera, and
`intents` to push to. A tool commits edits only through `Intent::Gesture { begin | apply |
end | cancel }` and `Intent::Run`; it never holds a map. A tool is therefore a pure state
machine, and the tests drive it with gesture sequences and check the transactions.

| Tool | Key | Layer | Gesture | Modifiers |
|---|---|---|---|---|
| Brush | B | tile | click or drag paints the brush; a stroke is one gesture; right-drag captures the selected layers into a brush (Shift at release cuts) | Shift line from the last point, ⇧⌘ ellipse, D random, X/Y flip, Z/⇧Z turn, [ ] size |
| Terrain | T | tile | paints corners, edges or whole tiles; right-click picks the terrain under the pointer | Shift line, ⌘ whole tile, ⌥ 180° symmetry on finite maps |
| Eraser | E | tile | erases the selected layers along a stroke; right-drag erases a rectangle | Shift every shown unlocked layer, [ ] size |
| Fill | F | tile | hover previews the area; dragging over other tiles adds their kinds; release fills | Shift fills the selection; right-click or Esc cancels |
| Shape Fill | P | tile | drag a rectangle or ellipse | Shift square, ⌥ from the centre, ⌘ paints the shape with terrain edges |
| Picker | I | tile | the topmost cell becomes the brush, and the Brush tool comes back | |
| Rect Select, Magic Wand, Same Tile | R, W, S | tile | selection; dragging inside a selection moves its carried tiles | Shift add, ⌥/⌃ subtract, both intersect, decided at the gesture's start; during a rectangle Shift squares and ⌘ grows from the start |
| Offset Layers | M | any | drag moves the selected unlocked layers' offsets | ⌘ toggles snapping |
| Explain | A | any | click a cell: the AutoMap tab explains it; hover previews after a moment | |
| World | N | any | drag a map, click another to open it, drag a handle to resize; right-click for the menu | arrows move, Shift ×5, ⌘ disables snapping |
| Select Objects | S | object | click selects, click again switches resize and rotate handles, drag moves, drag empty space rubber-bands, double-click a polygon edits it | Shift/⌃ toggle membership, ⌥ cycles stacked objects and moves from anywhere, ⌘ keeps proportions, Shift resizes from the origin, ⌃ snaps rotation to 15°, ⌥-right-click lists the objects under the pointer |
| Insert Rectangle, Ellipse, Capsule | R, C, ⇧C | object | drag to size, a click makes a point-sized one | Shift square, ⌘ snap |
| Insert Point, Tile, Text, Template | I, T, E, V | object | press, drag to position, release places; text edits inline on the canvas | ⌘ snap; Tile: X/Y/Z transform the ghost |
| Insert Polygon, Polyline | P, L | object | click adds points, the first point closes a polygon, right-click or Enter finishes, Backspace removes the last; an endpoint of a selected polyline extends or connects | ⌘ snap |
| Edit Polygons | O | object | nodes and segments; drag nodes; double-click a segment inserts a node; right-click for join, split, delete | Shift/⌃ toggle, ⌥ forces a move |

The modifiers are Tiled's wherever Tiled has them, so Tiled users keep their habits; new
chords follow one vocabulary (Shift constrains, ⌥ grows from the centre, ⌘ toggles snapping);
and the status bar names what each held modifier does right now, which is what makes the
mixture discoverable. A key pressed on the wrong layer family says so instead of switching
silently; picking a tile switches to the Brush with a flash.

The tool options bar is declarative: a tool returns `ToolOptions` (toggles, choices, a seed
to re-roll), the toolbar draws them, and a change is an intent.

## 7. Panels

Every panel is a function of the editor's queries plus its own UI state (scroll, filter,
expanded rows), returning intents. None mutates the editor and none holds a mailbox another
part polls.

| Panel | Shows | Intents |
|---|---|---|
| Tiles | the map's tilesets as tabs (filtered by name when there are many), the tiles with per-tileset zoom, wrap and grid, a search over image collections ("N of M"), rearranging; edit mode hosts the tileset editor (§8) | pick tiles (a brush), tileset commands (edit, embed, export, replace, remove, open the file), tile commands (swap, replace image, remove) |
| Terrains | the Wang sets of the current tileset with their colours, thumbnails and names, an erase row | choose a terrain (and the Terrain tool), move a colour, edit the set |
| Stamps | the stamp store as a tree with thumbnails and probabilities, filter | use a stamp or a variation, add a variation, rename, delete, set the folder |
| Layers | the tree, visibility and lock toggles, inline rename, drag to reorder or into groups, multi-select | select, toggle, move, every layer command |
| Objects | the object layers as a tree with a filter (name, class, kind, id) | select and centre, reorder |
| Properties | the property-editing model's rows for the selection (map, layers, objects, tileset, tiles, Wang sets, project, world), enum combos, flag checkboxes, class members dimmed until set, lists edited in place, file fields that open or reveal, object references that go to, choose from a list or pick on the map, long-text editor, arithmetic in number fields, Align and Distribute buttons for several objects | `EditProperties` changes, picks, go-to |
| Project | the project's folders as a tree with natural sorting | open a file, add or remove a folder, refresh |
| History | the current document's transactions with the current position | travel |
| AutoMap | the rules' state and problems, the Live bindings (start, bind, add a run, remove, re-roll, stop, show painted), the Explain tool's result: attempts grouped by outcome with rule pictures and condition lines, Open the rule, Show where it matches | AutoMap intents, open a rule map |
| Issues | problems with severity, source, jump target and Locate File | go to, locate |
| Console | the log, All or Problems, clear, copy | |
| Minimap | §5 | recentre |

Panel layout: two tab groups, show and hide per panel (View ▸ Views and Toolbars), a layout
generation that resets sizes, Clear View (Tab hides every panel and says how to bring them
back), full screen. The layout is part of the session.

## 8. The tileset editor

Edit mode of the Tiles panel for the current tileset, or the centre of a tileset document, with
three modes that are tool modes over the tileset's tiles:

- **Terrain**: labels painted onto lanes with a corner, edge or tile hit test (isometric
  tilesets on their diamond), right-drag erases, each stroke a gesture; set commands (new of
  each kind, rename, duplicate, delete, move colours); Auto-label from one tile; **Layout…**
  with the presets and detection from `bt-terrain`, Apply and Label All; **Patterns** showing
  what the set needs with the missing ones highlighted, click one then tiles to label them.
- **Animation**: frames dragged, double-clicked or ⇧-clicked in from the tileset (⇧-drag adds a
  run), reordered by drag, durations applied to several, cut, copy, paste, delete, a playing
  preview.
- **Collision**: the object tools over the tile's collision shapes as an object container
  (`04-editing-operations.md`, §11), with the shape list (show, hide, reorder, duplicate),
  ⇧-click to copy shapes to other tiles, and the Properties panel on a shape.

Every change is a transaction on the tileset asset, made from whichever document shows it.

## 9. Dialogs and modals

`modals: Vec<Modal>` is the one place that knows whether something modal is open. A `Modal` is
a `Decision` from the editor core waiting for an answer (unsaved work on close or quit, a file
changed on disk, an image whose size changed, a Godot export that wrote nothing, a save to an
unknown extension, a tileset removal that loses tiles), an error with its details, the recovery
prompt, or a dialog with its own state. Dialogs: New Map, New Tileset (from an image, an image
collection, or RPG Maker autotiles), Resize Map (with the minimap preview), Offset Map,
Preferences (General, Interface, Keyboard with search, conflicts and record-a-key, `.kms`
import and export, Theme), Custom Types, Project Properties, World Properties, Export As, the
Godot setup sheet, the Template editor, the Edit Commands window, Go to Tile, the object
reference list, the long-text editor, About.

A dialog opens because the editor returned `Effect::ShowDialog(kind, seed)`; it collects its
values and returns an intent. File dialogs run through `rfd`'s asynchronous API, waited for on a
worker so a frame never blocks (on macOS `rfd` shows the panel on the main thread), and the
chosen path comes back as `Intent::FilePicked`, one for each file chosen. Escape in a
dialog's field leaves the field, not the dialog.

As built in M6, File ▸ Export as Image opens a scale (1–800%) and background-colour
dialog, then a PNG save panel. A worker renders a snapshot of `shown_map` with the shared
headless renderer and the project's object class colours, and atomically replaces the PNG.
It exports visible layers, includes unsaved edits and does not save or mark the map clean.
Read-only targets and write failures produce an error notice and preserve the previous file.
The Properties panel's editable Image rows offer Choose Image and Clear Image. A worker
probes the selected file before applying one undoable change; references are relative to the
map or tileset that owns the field, and image-layer repetition and transparent colour remain
the layer's attributes.

## 10. Status bar and toolbar

The status bar lays out its right side first (zoom menu, map size, the ⛔ and ⚠ counts that
open Issues) and gives the left what is left, cutting long texts to a budget with the whole on
hover: the flash notice (a Godot refusal is a red item that asks again, notes end with a ▸ that
opens Issues), the hovered cell and pixel, the tile with its flips, painted versus made, the
terrain and Explain notes, a layer switcher, the selected objects with how many are hidden or
locked, and the tool's hint with the held modifiers' meanings. The toolbar draws its right-hand
buttons first and clips the tools to the room left; labels drop before buttons do.

## 11. Workers, the watcher, recovery, the live server

- **Workers** run loads, content hashes, project scans, Godot verification and exports; each
  result is an intent. Image decoding is `bt-image`'s own pool with a waker that requests a
  repaint.
- **The watcher** polls the editor's `watched()` list every 800 ms on a worker (modification
  time and content hash), hands the observations to the editor and shows the decisions it
  returns, one at a time.
- **Recovery** writes the snapshots the editor's plan asks for on a worker, through atomic
  writes; on an unanswered quit the last snapshot is written synchronously after in-flight
  writes are joined.
- **The live server** (`bt-live`): a listener thread accepts one request per connection, a
  bounded number at a time; requests that need the editor become jobs with a deadline and are
  delivered in `logic`, so they are answered while the window is hidden; a script runs on the
  connection thread against `prepare_external_edit`'s base and lands through
  `apply_external_edit`, waiting while the canvas is in a gesture and rerunning on a version
  conflict; a screenshot is a viewport command whose image arrives next frame. The server
  rebinds when its socket vanishes.

## 12. Platform

- **macOS quit**: the window's close button and the Quit command ask about unsaved work in the
  frame loop. The app menu's Quit (⌘Q), the Dock's and a logout call AppKit's `terminate:`,
  which asks the application's delegate `applicationShouldTerminate:`; winit's delegate has no
  answer and must stay winit's, so `platform/quit.rs` adds the method to its class at start
  (the workspace's one `unsafe` code, `decisions.md`). The method asks a `QuitGuard` the frame
  loop keeps up to date: with nothing unsaved the application quits; otherwise the system's
  quit is cancelled, the window is brought forward and the Quit command asks (a logout is
  interrupted until it is answered). If the guard cannot be set up, the window says so; an exit
  nobody answered writes the snapshots the latest changes need and keeps them.
- **Open with the system** and **reveal in the file manager** through the platform's commands.
- **HiDPI**: the camera and overlays work in device pixels; text and strokes are cosmetic.
- **Theme**: the egui style is derived from the theme settings (base colour decides light or
  dark, selection colour, font size, an optional custom font); icons are vector-drawn.
- **Fonts**: the interface font and the macOS symbol glyphs for key names; the renderer's
  text font is `bt-render`'s. As built (`style/fonts.rs`): egui's fonts, and on macOS the
  system's Apple Symbols after them, for the ⇧ ⌥ ⌃ and key symbols egui's fonts lack (they
  have ⌘); a symbols font that cannot be read is logged and those symbols show as boxes.

## 13. Development modes

`--screenshot` and `--bench` exist for checks without a person. A `dev` module reads
`BT_SCREENSHOT_*` variables to run actions, select a cell for Explain or show rule matches
before the screenshot, so a scripted run can picture any state; it is compiled in but inert
without the variables.

As built: `--screenshot OUT.png` waits until no load runs, no image is being decoded (on the
canvas or in the Tiles panel) and no far square is pending, for three frames in a row and
0.3 s (longer than egui's 0.2 s animations, so a window a command opened has faded in), then
saves the window's own picture and quits; it fails after 30 s, saying whether the window was
not ready or its picture never came (a locked screen or a hidden window presents nothing).
`--bench MAP` waits the same way, then pans across the map at every power-of-two zoom from
1/32 to 64, 15 frames to settle and 240 measured at each, a view width a second, and prints
per zoom the interval between frames (median, 95th percentile, worst), the window's own work,
the renderer's prepare time and its counts, then the display's rate and how many frames took
over half again the median; frames faster than any display are reported as not presented, and
with debug logging each slow frame is logged with the window's own work and the renderer's. As
built at M2, `BT_SCREENSHOT_ACTIONS` names commands by their keymap names, separated by commas
(`rect_select_tool,select_all`): `--screenshot` runs them once the window has settled, waits for
it to settle again, then takes the picture; a name that is no command stops the start with the
reason. As built at M3, `BT_SCREENSHOT_TABS` brings panel tabs forward by their titles
(`automap,terrains`), and `BT_SCREENSHOT_EXPLAIN=x,y` has the Explain tool choose that cell of
the current map before the commands run; either stops the start when it names nothing. Showing
where a rule matches did not come with M3.

## 14. Module layout

```
apps/better-tiled/src/
├── main.rs            arguments, logging, data folder, eframe launch
├── cli.rs             the command line
├── app/               App, the frame (logic and ui), effects, startup, tabs, menu bar, toolbar, status bar, title, quit
├── focus.rs           keyboard routing and the shortcut router
├── canvas/            camera.rs, gesture.rs, view.rs (the canvas and its input), paint.rs (the paint callback), overlay.rs, minimap.rs
├── tools/             tool.rs (the trait), one file per tool, options.rs
├── panels/            tiles.rs, tile_images.rs, terrains.rs, stamps.rs, layers.rs, objects.rs, properties.rs,
│                      project.rs, history.rs, automap.rs, issues.rs, console.rs, layout.rs
├── tileset_editor/    terrain.rs, animation.rs, collision.rs, layouts.rs, patterns.rs
├── dialogs/           one file per dialog, modal.rs (the stack)
├── files/             drop.rs, pick.rs (async file dialogs), writer.rs (snapshots, session, heartbeat), watcher.rs
├── workers.rs         the worker threads and their result channel
├── live/              server.rs, jobs.rs, screenshot.rs
├── platform/          quit.rs (macOS), open.rs, reveal.rs
├── style/             theme.rs, icons.rs, fonts.rs
├── dev.rs             --screenshot, --bench, the BT_SCREENSHOT_* hooks
└── test_window.rs     the window under test (tests only)
```

At M1 the files that exist are `main.rs`, `cli.rs`, `app/` (`frame.rs`, `effects.rs`,
`startup.rs`, `menu.rs`, `tabs.rs`, `status_bar.rs`, `tests.rs`), `focus.rs`, `canvas/`
(`camera.rs`, `view.rs`, `paint.rs`, `overlay.rs`), `panels/` (`layers.rs`, `tiles.rs`,
`tile_images.rs`), `dialogs/modal.rs`, `files/` (`drop.rs`, `pick.rs`, `writer.rs`),
`workers.rs`, `platform/quit.rs`, `style/theme.rs`, `dev.rs` and `test_window.rs`.

## 15. Tests

The logic lives in `bt-editor` and is tested there; the application's tests cover what crosses
widgets, through `egui_kittest` and a `TestWindow` handle that opens files, runs actions,
presses keys, clicks and drags the canvas at a cell or an object, types into a named field,
answers a modal, and reads back public queries of the editor. Tests never reach into the
window's fields.

| Flow | What it checks |
|---|---|
| Keyboard | a plain key after Tab from the map, after Shift-Tab, after typing then scrolling, after Escape from a field, with the tileset canvas focused and the pointer over the map; a held space over the map; ⌘Z after leaving a field |
| Tools | each tool's gesture sequence on a small map produces the expected transaction, preview and status hint; a shared key on the wrong layer family flashes instead of switching |
| Panels | each panel returns the intent its control stands for; the Tiles panel keeps per-tileset scroll and search; a conditional button does not shift a field's id |
| Modals | unsaved prompts on close, quit and project switch; a conflict from the watcher; the image-size question; the Godot refusal; Escape in a dialog's field |
| Live link | `bt` on a thread edits, marks, undoes, saves and opens while the window steps; an edit waits for a stroke to end; a conflict reruns |
| Pictures | `--screenshot` of a known map matches a committed picture within a tolerance |

As built at M1, `TestWindow` is the app without a GPU in an `egui_kittest` harness with a data
folder of its own: it drops files on the window, presses keys, clicks labelled widgets, clicks
the close button, ends the run as eframe does, and reads the editor back; it runs frames until
the workers are done and the layout has stopped moving. The M1 flows: a dropped map (Tiled's
desert, and a pack map mirrored into a scratch folder) opens and saves byte for byte; a layer's
visibility toggle is an edit that undoes; shortcuts run commands; closing with unsaved work
asks, Cancel keeps the window and Don't Save closes it; the session brings the files back with
their cameras; an exit nobody answered leaves the work to recover at the next start.

What a person checks in the real window, each release: the feel of pan and zoom at 120 Hz on
the largest pack map, the latency of a stroke, the readability of hints and flashes, and the
dialogs' layout at the smallest supported window size.


### M6 world rendering as built

Visible world neighbours are additional `FrameInput`s through the shared GPU renderer. Their
cameras translate each map's bounding rectangle into the active map's coordinates. Each map
can limit its background to its own rectangle, preserving the surrounding canvas and other
maps; the active map draws last with its tool overlays. Frames and filenames show map extents.
Neighbour loads run on workers and reuse asset identities when opened as documents. The World
tool previews movement and all eight resize handles, commits on release, and cancels with
Escape. Its window test drives movement, cancellation, resize, arrows and neighbour opening
with the real GPU renderer.
