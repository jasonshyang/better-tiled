# The application — `apps/better-tiled`

*2026-10-09; the M1 part as built at the end of M1, 2026-10-10 (§1.1). The design of the window:
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
`log`/`env_logger`, and on macOS `objc2`/`objc2-app-kit` to give the quit key to the window.

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
└── platform: Platform                            the quit key, menu, reveal, open-with
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

- **macOS quit**: the window's close button and ⌘Q ask about unsaved work: at start the app
  menu's Quit item gives up its ⌘Q key equivalent (safe AppKit calls through `objc2-app-kit`),
  so the key reaches the window and runs the Quit command. Choosing Quit from the app menu, the
  Dock or a logout still ends the run without asking: asking there needs the application
  delegate's `applicationShouldTerminate:`, which can only be added with `unsafe` and waits for
  a design review. Such an exit writes the snapshots the latest changes need and keeps them, so
  the next start offers the work back.
- **Open with the system** and **reveal in the file manager** through the platform's commands.
- **HiDPI**: the camera and overlays work in device pixels; text and strokes are cosmetic.
- **Theme**: the egui style is derived from the theme settings (base colour decides light or
  dark, selection colour, font size, an optional custom font); icons are vector-drawn.
- **Fonts**: the interface font and the macOS symbol glyphs for key names; the renderer's
  text font is `bt-render`'s.

## 13. Development modes

`--screenshot` and `--bench` exist for checks without a person. A `dev` module reads
`BT_SCREENSHOT_*` variables to run actions, select a cell for Explain or show rule matches
before the screenshot, so a scripted run can picture any state; it is compiled in but inert
without the variables.

As built: `--screenshot OUT.png` waits until no load runs, no image is being decoded (on the
canvas or in the Tiles panel) and no far square is pending, for three frames in a row, then
saves the window's own picture and quits; it fails after 30 s, saying whether the window was
not ready or its picture never came (a locked screen or a hidden window presents nothing).
`--bench MAP` waits the same way, then pans across the map at every power-of-two zoom from
1/32 to 64, 15 frames to settle and 240 measured at each, a view width a second, and prints
per zoom the interval between frames (median, 95th percentile, worst), the window's own work,
the renderer's prepare time and its counts, then the display's rate and how many frames took
over half again the median; frames faster than any display are reported as not presented. The
`BT_SCREENSHOT_*` hooks come with the tools.

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
