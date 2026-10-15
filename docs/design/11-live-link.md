# The live link — `bt-live`

*2026-10-09. The design of the link between `bt` and a running editor: where the editor listens,
what travels over the socket, how a request becomes work on the editor's thread, how an edit
lands as one undo step without racing the person at the keyboard, and what keeps another
process from feeding an agent a false map. The high-level design (`00-high-level-design.md`,
§11.3) states the goals; the editor core's side is `05-editor-core.md`, §17, the window's
`06-application.md`, §11, and `bt`'s `10-agents.md`, §13.*

## 1. Scope

`bt-live` is the transport and the protocol: the folder editors listen in, the socket framing,
the request and response types with their version, the client that finds editors and asks
them, and the server that accepts connections and hands work to the window's thread with a
deadline. It depends on `bt-utils` only, so it knows no maps: the handler that answers a
request lives in the application, which runs scripts through `bt-script` and asks the editor
core for the map. `bt`'s commands over the link (`editor`, `mark`, `open`, `save`, the routing
of `edit` and `undo`) are `apps/bt`'s; the outlines on the canvas are the window's.

Dependencies: `bt-utils` (`paths`), `serde`, `serde_json`, `libc` (the user id and socket
ownership on Unix).

## 2. Terms

| Term | Meaning |
|---|---|
| runtime folder | the private per-user folder under `/tmp` where sockets and locks live |
| editor | one running `better-tiled` process, listening on its own socket |
| request | one JSON line `bt` sends on a fresh connection; the answer is one JSON line back |
| job | the part of a request that needs the editor's state, run on the window's thread between frames |
| deadline | the moment `bt` stops waiting; a job not taken by then is dropped, never done unseen |
| base | the state an edit script starts from: the document's version and maps |
| external edit | a script's result applied to the document as one transaction |
| mark | cells `bt` outlines on the canvas, with a label and a colour |

## 3. Module map

| Module | Holds |
|---|---|
| `dir` | where editors listen, the privacy check (§4) |
| `protocol` | `Request`, `Response` and their parts, the protocol version, run-length cells (§5) |
| `wire` | one line each way: `ask` on the client, `serve` on the server (§6) |
| `client` | `editors()`, `Editor::ask`, stale sockets, version mismatches (§7) |
| `server` | `Server::start`, the listener thread, `Handler` (§8) |
| `jobs` | `Jobs<Kind, Answer>`: a deadline channel from connection threads to the window (§8) |

## 4. Where editors listen

An editor listens on a Unix domain socket `<runtime>/<pid>.sock`, where the runtime folder is
`BT_LIVE_DIR` when set, else `bt-utils::paths::runtime_dir()`: `/tmp/better-tiled-<uid>`. Not
the data folder, for three reasons: a socket path is limited to about a hundred bytes and a
long home path breaks it; `/tmp` is the same whatever `$HOME` is, and agent sandboxes change
`$HOME`; and every program of the user finds it without configuration. The folder is created
with mode `0700`, and both sides check before use that it is a directory owned by the current
user and readable by no one else (fixing the mode when it is wider): anyone else able to write
there could listen in the editor's place. A folder that fails the check is not used, and `bt`
says why.

Windows has no live link: `bt` works with files there, `bt editor` says so, and the editor
starts no server. The preference "Let bt work with the open maps" (on by default) starts and
stops the server at runtime.

## 5. The protocol

The version is one integer, `PROTOCOL`, in every `Status`; it changes when a request or
answer changes incompatibly. A client that meets another version says "another version:
restart it", and never guesses.

Maps are named by their canonical path, so `/tmp/a.tmx` and `/private/tmp/a.tmx` are one map.
Cells travel as runs `[y, x0, x1)` row by row, so a selection of ten thousand cells is a short
list.

| Request | Answer | What it is |
|---|---|---|
| `Status` | `Status { protocol, pid, focused, documents }` | the open documents (§5.1) |
| `Snapshot { path }` | `MapText { dirty, map, tilesets }` | the map as the editor shows it, as TMX text, with its dirty external tilesets as TSX text |
| `Edit { path, script, prelude, seed, dry_run, view, by, runs, want_map, want_cells }` | `Edited { report, area, applied, map, cells }` | a script run on the map and applied as one undo step (§9) |
| `Undo { path }` | `Done` | the last external edit taken back (§9.3) |
| `Mark { path, marks, add, focus }` | `Done` | outlines on the canvas (§10) |
| `Open { path }` | `Done` | the map opened, or shown when open |
| `Save { path }` | `Done` | the map saved through the editor's own save |
| `Screenshot { out }` | `Done` | the window as the person sees it, written as a PNG to `out` |
| `RememberExport { path, target, format }` | `Done` | the map's export target set, as one undo step |

`Done { message, nothing }` carries a sentence for the person and whether there was nothing to
do (so `bt undo` can fall through to the file's own history). `Edited.report` is the typed edit
outcome of `10-agents.md`, §9, serialised; `map` and `cells` are present only when asked for.

### 5.1 A document's status

`DocStatus { path, title, active, dirty, tileset, layer, tool, brush, selection, objects, view,
mouse, history, undo_steps, marks }`: the canonical path (none for a map never saved), whether
it is the one shown, whether it has unsaved changes, whether it is a tileset opened on its own,
the current layer's name, the tool's name and what it paints with in `bt`'s own names (a terrain,
a tile, an `NxM` stamp, an eraser size), the selected cells as runs and the selected objects'
ids, the cells in view and the cell under the pointer (the shown map only), the last six undo
labels and the count, and the marks. The world tool and the editor's focus state are reported
too, so an agent can tell what the person is doing before interrupting.

## 6. The wire

One connection carries one request and one answer, each a line of JSON: `{"op": "snapshot",
…}` in, `{"ok": …}` or `{"error": "…"}` out. The server reads at most 16 MiB and gives a
connection ten seconds to send its line; the client sets its own read timeout per request. A
line that is not a request is answered with an error and the server goes on. There is no
session, no streaming and no second message, so neither side keeps state between calls, and a
crashed peer costs the other at most one timeout.

## 7. The client

`editors(dir) -> (Vec<Editor>, Vec<Problem>)` lists `*.sock` in the folder and asks each for
its `Status` (eight seconds): editors that answered, the focused one first and then the newest
process; a socket whose process is gone is removed as stale; one that answers another protocol
version, or does not answer, is a problem with the process id ("didn't answer in time: is a
dialog open in it?"). `Editor::document(path)` finds a map among its documents; `Editor::ask`
sends one request with a wait that fits it: fifteen seconds for most, sixty for a save, ten
minutes for an edit (a script may run AutoMap rules over a large map).

`bt` reads a map from the editor only when the editor has it open with unsaved changes; a map
open and clean is read from its file, which is the same bytes. When an editor that does not
answer may hold unsaved changes to the map, `bt` refuses to read or write the file rather than
show an outdated map or lose those changes: it knows from the recovery folder (`05-editor-core.md`,
§10), whose metadata names the original path, and from the process being alive.

## 8. The server

`Server::start(dir, handler)` binds `<pid>.sock` (removing a leftover of the same name), spawns
the listener thread, and spawns one thread per connection that reads the request and calls
`Handler::handle(&Request) -> Result<Response, String>`. Dropping the server stops the listener
and removes the socket. Every ten seconds the window asks `Server::lost()` whether the socket
file is still there; macOS cleans old files out of `/tmp`, and a server whose socket vanished
is dropped and started again, so `bt` can always find a running editor.

`Jobs<Kind, Answer>` is how a connection thread reaches the window: `ask(kind, wait)` sends a
job with a deadline (`now + wait`), wakes the window (`request_repaint`) and waits for the
answer with a little slack; the window takes the jobs each frame in `logic`, which runs while
the window is hidden, and skips any whose deadline passed: `bt` has stopped waiting and said so,
and a job done unseen (an edit applied after `bt` reported a timeout) would be worse than one
dropped. Waits: five seconds for a job the window can answer at once, twenty for applying an
edit or saving, since an edit waits for the end of a brush stroke or a drag.

The application's handler (`06-application.md`, §11) turns requests into jobs and runs scripts
on the connection thread, so the window never waits for a script; the editor core's
`prepare_external_edit`, `apply_external_edit`, `external_undo` and `set_marks`
(`05-editor-core.md`, §17) are what the jobs call.

## 9. Edits

### 9.1 Why scripts run in the editor

A script could run in `bt` on a snapshot and send the result back as TMX. It does not, because
the editor's documents hold tilesets with unsaved edits and the map's identity (asset versions,
layer ids as the editor knows them): a TMX round trip would replace the person's tilesets and
lose what they had not saved. The script runs inside the editor process, on the base the editor
hands it, through the same `bt-script` the file path uses, so an edit made through the link is
the same edit, with the same report.

### 9.2 The flow

1. `Prepare` (a job): the base: the document's id and version, the painted map, the shown map
   (the rules' result when AutoMap layers are on), the AutoMap setup with its rules, the
   project's custom types, and the selection (cells and objects) for `selection` and
   `selected`.
2. The script runs on the connection thread on a copy of the base, with the prelude of a
   `--with` file, the seed and the view options; a panic inside the script is caught and
   reported as "the script crashed inside the editor", never taking the window down.
3. A dry run, or a script that changed nothing, answers with the report and applies nothing.
4. `Apply { doc, version, map, setup_change, label, area }` (a job): the editor core applies
   the result as one transaction labelled by the script, with the AutoMap setup change when the
   script started, changed or stopped the layers, only while the document's version is still
   the base's. The canvas outlines the changed area for a few seconds and the status bar
   flashes the label. An `Apply` that arrives during a gesture waits (retried every thirty
   milliseconds) until the stroke or drag ends.
5. On a version conflict (the person edited meanwhile) the script runs again on the new base,
   up to three times; then "the map kept changing in the editor: run it again". The person's
   edits are never overwritten and never interleaved with the script's.

The edit does not save: the person decides, or `bt save` does. `RememberExport` follows the
same prepare-apply-retry path with no script.

### 9.3 Undo

`external_undo` takes back the last external transaction only while it is the latest in the
document: the editor keeps the map each external edit left (by identity, not by content), and
if the person has edited since, `bt undo` refuses with the label of their edit and points at the
editor's own undo. A second `bt undo` takes back the external edit before it when that one is
now the latest. A closed document forgets its external edits.

## 10. Marks

`Mark { path, marks: Vec<MarkSpec { runs, label }>, add, focus }`: without `add` the map's marks
are replaced; marking a label again moves that mark; an empty list with `add` false clears
them. With `focus` the view moves to show the marks' bounds ("the view moved to show it", or "in
view already"); marks on a map the editor is not showing wait there, since `bt` never switches
the map the person is looking at. Each mark gets the next of five bright colours unlike the
selection's blue; the canvas tints the cells (up to twenty thousand) and outlines them, with the
label on the first row's left end. `Status` lists the marks, so `bt render --marks` can draw
them on a file render when the window is hidden.

## 11. Open, save, screenshot

`Open` opens the file, or shows it when open, and asks the window system for focus. When no
editor runs, `bt open` starts one itself (the `better-tiled` program, detached in its own
process group) and waits up to twenty seconds for it to answer; if it does not, it says the
preference may be off. `Save` runs the editor's save: a map saved whose dirty tileset could not
be written is reported to `bt` as an error naming the tileset, with the map saved. `Screenshot`
is a viewport command whose image arrives the next frame, scaled to the window's size in points,
written to the absolute path `bt` chose; a hidden window answers an error with `bt render` as
the way, and the rest of `bt editor`'s output stands.

## 12. Safety

- Only the user's own folder is used, checked on both sides; nothing in `/tmp` outside it is
  read or written.
- Requests are data: a script is parsed by `bt-script` like one from a file, with the same
  limits; nothing in a request names code to run.
- The editor writes a file on request only for `Save`, `Open` (the session) and `Screenshot`
  (the path `bt` gave, under `bt`'s own permissions), never for an edit.
- A deadline bounds every job; a timeout on either side leaves the editor consistent, since an
  edit applies as one transaction or not at all.
- The protocol version and the request size are checked before anything is parsed further.

## 13. Tests

| Test | What it pins |
|---|---|
| Wire | a round trip of every request and answer; an error line for a non-request; the size limit; the server's read timeout; runs and cells convert both ways |
| Client | stale sockets of dead processes are removed; another protocol version is a problem naming the process; the focused editor comes first |
| Server | requests reach the handler and answers come back; a window that takes no jobs times out within the wait; a dropped server removes its socket; a lost socket is detected |
| Jobs | a job past its deadline is skipped; an answer sent just before the deadline still arrives (the slack) |
| Privacy | a folder owned by another user or open to others is refused on both sides |
| End to end (`apps/better-tiled`) | `bt` on a thread against the real editor stepped by `egui_kittest`: an edit lands as one undo step and `bt undo` takes it back; undo refuses after the person's edit; `selection` and `selected`; marks replace, add, move and clear, and never switch the shown map; a dry run marks the cells it would change; save writes what the editor has and reports an unwritable tileset; a failed or dry run leaves the editor alone; a conflicting apply is refused and the script reruns; a map with AutoMap layers is edited as what was painted; `--remember` sets the export target as an undo step |

## 14. As built at M4

M4 built the crate, the editor's server and handler, the editor core's external edits and
`bt`'s commands over the link. The crate:

| Module | What it holds |
|---|---|
| `dir` | `live_dir()`, `LIVE_DIR_VAR`; sockets named `<pid>.sock`; whether a process is alive (`kill` with no signal) |
| `protocol` | `PROTOCOL` (1); `Request` with `EditRequest` and `MarkSpec`; `Response` (`Status` with `DocStatus` and `KeyboardFocus`, `MapText` with `TilesetText`, `Edited`, `Done`) and its `into_status`, `into_map_text`, `into_edited`, `into_done`; `Run` with `runs_from_cells` and `cells_from_runs`; `canonical_path` |
| `wire` | `ask`, `serve` and the server's `Limits` (Unix only) |
| `client` | `editors(folder) -> Result<Editors, Error>`, `Editors { answered, problems }` with `with_map_open(path)`; `Editor::{status, pid, socket, document, ask}`; `Problem::{OtherVersion, NoAnswer, Failed}` |
| `server` | `Server::{start, lost}`, `Handler` (a closure is one), `SOCKET_CHECK_INTERVAL` |
| `jobs` | `Jobs<Kind, Answer>::{new, ask, take}`, `Job { kind, reply }`, `Reply::{deadline, is_overdue, send}`, `JOB_WAIT`, `LONG_JOB_WAIT` |
| `error` | `Error`; an error converts into its sentence, so a handler's `?` answers with it |

Where it differs from the text above, or says more:

- **Dependencies** are `rustix` instead of `libc` (whether a process is alive, and `poll`), with
  `glam` for points and `thiserror`. `bt-utils` gained a `serde` feature, which writes `IRect` as
  `{"x", "y", "w", "h"}`, and `paths::make_private_dir`, the runtime folder's check made public.
- **The folder.** Both programs call `live_dir()`. A `BT_LIVE_DIR` must be an absolute path
  (each program would read a relative one against its own working folder), short enough for
  `<folder>/<pid>.sock` to fit a socket path, and passes the runtime folder's check.
  `Server::start` and `editors` check the folder they are given too, so one cleaned away is made
  again. A folder open to others is narrowed to `0700`, as §4 says, and refused only when it
  stays open: that is what §13's "refused" comes to for it.
- **Requests.** `Edit` holds an `EditRequest` (`EditRequest::new(path, script)` and struct update
  for the options), so the handler passes one value on; on the wire it is the same flat object.
  `seed` is optional and `by` is a string in `bt`'s words, read by `bt-script`. `RememberExport`
  is left out until the Godot export. Every type refuses fields it does not know, except the
  `status` request, which every version reads whatever else it carries; `editors` reads an
  answer's `protocol` before the rest of it.
- **Answers** are tagged by `"answer"` (`status`, `map_text`, `edited`, `done`) inside
  `{"ok": …}`, so an answer reads on its own. `Edited.report` is the outcome as JSON
  (`serde_json::Value`, what `bt-script` writes); `area` and `DocStatus.view` are `IRect`s,
  `mouse` an `IVec2` written `[x, y]`. Cells are `Run { y, x0, x1 }`, written `[y, x0, x1]`; a
  run with no cells is refused. `DocStatus` adds `world_tool` and `keyboard` (`map`,
  `text_field`, `tileset_canvas`, `dialog`) for the document shown. `Status::of_this_process`
  fills in the version and the process id.
- **Canonical paths** have one definition, `canonical_path`. A path whose file does not exist
  resolves its deepest folder that does, so a deleted map open in the editor is still found
  through `/tmp`.
- **The wire.** The ten seconds are for the whole request line, however many reads it takes. A
  client never sends a line over 16 MiB (`Error::TooLarge`) and reads at most 256 MiB of answer,
  since a snapshot holds a whole map. macOS refuses to set a socket's timeout once the peer has
  closed; that is taken as no timeout, since reading no longer waits then, or an answer already
  sent would be lost. A client whose write finds the connection closed still reads the answer.
- **The client** asks the sockets one at a time; the newest process is the highest process id.
  A socket nobody listens on whose process runs (its id reused) is kept and named. An editor
  that answers its status with an error (its window took no job in time) is a
  `Problem::Failed` with its sentence. `Editor::document` skips tilesets opened on their own.
- **The server** serves sixteen connections at once and answers one more, unread, that the editor
  is busy. Its thread waits in `poll` on the socket and on a stop socket, so a dropped server
  always stops, also once macOS has removed the file and nothing could connect to wake it.
  `lost()` also says the thread stopped. A dropped server removes its socket only while the file
  is the one it made (device, inode, change time), so a server started before the old one is
  dropped keeps its socket. A handler's panic is answered "the editor failed while answering".
- **Jobs.** An asker stops waiting at the deadline while the job is queued, so five seconds are
  five; once the window has taken it, five seconds of slack; and at once when the window drops
  it unanswered (`Error::NotFinished`). `take()` drops overdue jobs; a job kept for a later frame
  (an edit waiting for a stroke to end) is dropped once `reply.is_overdue()`. `Reply::send`
  consumes the reply and says whether the asker still waited.
- **Windows**: `live_dir()` names the folder without checking it, `Server::start` is
  `Error::Unsupported` and `editors` finds none.

Tests: 57 unit tests on Unix, covering §13's rows but the end to end one; 29 run without Unix
sockets, two of them only there. Sockets live in short folders of their own
(`/tmp/btl-<pid>-<n>`, `0700`), tests run in parallel, and waits are shortened through
parameters (`Limits`, `editors_within`, `Jobs::with_slack`). No test starts a process: a child
holds every open socket until it runs its program, which kept another test's closed socket
listening; an exited process is an id no system gives. Another user's folder is `/`, when it is
root's and the tests are not run as root.

**The window's side** (`apps/better-tiled`, `live/` and `app/live.rs`):

| Module | What it holds |
|---|---|
| `live` | `LiveLink`: the listener, started and stopped by the preference "Let bt work with the open maps" (`General.live_link`) and started again when its socket vanished; the jobs the connections asked for |
| `live/handler` | `LiveHandler`: each request becomes a job for the window (`Status`, `Snapshot`, `Undo`, `Mark`, `Open`, `Save`, `Screenshot`), or the edit flow |
| `live/edit` | the edit flow of §9.2 on the connection's thread, against a `Window` (prepare a base, apply a result), so its reruns are tested with a scripted window |
| `live/jobs` | `LiveJob` and `Answered`: what the window is asked and what it answers, a base and an apply among them |
| `app/live` | the jobs done in `logic` each frame; held jobs for an edit or an undo during a stroke, a map being opened, a screenshot arriving |

- **The edit flow.** The handler parses the script and the prelude (whose errors say `--with
  file, line N`) and the view's `by` before asking the window anything. The base comes with the
  editor's tilesets (unsaved edits included, put into the script's loader), the selection, the
  rules the editor read (passed as rules already read; read again through the loader when the
  editor has none for the AutoMap layers' file yet) and the map as it shows. A run that panics
  answers "the script crashed inside the editor". The report's `saved` says `in_editor`, a dry
  run or nothing changed; `bt` replaces the map's path in it with the path as typed. `Apply`
  carries the changed area: once it lands the canvas outlines it for three seconds, dark under
  light and fading, and the status bar flashes `bt: LABEL`.
- **`bt`'s side** (`apps/bt`): `Link` finds the editors once per command, when one first needs
  them; `load` reads the editor's copy of a map with unsaved changes (`MapCopy`), and a file an
  editor that did not answer may hold unsaved (its recovery snapshot names the map) is left
  alone. `edit`, `undo`, `mark`, `open`, `save`, `editor` and `diff MAP` go through the link;
  `bt edit --mark` adds its mark to the map's marks.
- **End to end** (`apps/better-tiled/src/app/live_tests.rs`, the real window stepped by
  `egui_kittest`, `bt`'s client on a thread): the status of the open maps and their unsaved
  changes; marks replace, add, move and clear and never switch the map shown; open, save and
  nothing to undo; the preference stops and starts the listener; an edit lands as one undo step
  that `bt undo` takes back, with its label flashed; a dry run sends the cells it would change
  and the map, and a failing script leaves the editor alone; `selection` is what the person
  selected, and their edit after `bt`'s keeps `bt undo` off; a map with AutoMap layers is
  edited as what was painted. `live/edit` tests a conflict's rerun and the three runs' limit;
  `apps/bt` tests `bt edit` routed to an editor (a stand-in server) with `--mark`.

Not here yet: `RememberExport`, which comes with the Godot export (M5).

### 15 As built at M5 (2026-10-15)

The protocol adds `remember_export` with canonical map `path` and absolute scene `target`,
answered by `Done`. The window waits for a stroke to finish within the existing request
deadline, then records one ordinary undoable transaction. It changes the open map without
saving it. Wire round trips cover the request and a window test proves ordinary Undo restores
the previous export settings.
