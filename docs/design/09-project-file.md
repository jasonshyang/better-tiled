# The project file — `bt-project`

*2026-10-09. The design of `better-tiled.json`: what it holds, where it is found and created,
its schema, and how the editor and `bt` read and write it without stepping on each other. The
high-level design (`00-high-level-design.md`, §12) states the goal: Tiled files stay exactly
what Tiled writes, and everything better-tiled needs beyond them lives here.*

## 1. Scope

`bt-project` is the only code that reads or writes `better-tiled.json`. It defines the file's
types, finds the file for a map, and performs every read-modify-write. It knows nothing of the
algorithms that use its records: the AutoMap layers record is produced and consumed by
`bt-automap` (`08-automap.md`, §10) and the Godot setup by `bt-godot` (`12-godot-export.md`);
this crate stores them, keeps what it does not understand, and keeps the file stable.

Not here: Tiled's own `.tiled-project` (read and written by `bt-tmx`, `02-file-formats.md`),
the editor's settings and session (`05-editor-core.md`, §13), and `bt`'s undo history.

Dependencies: `bt-core` (cells, flips, layers and objects for the checksums and rows),
`bt-utils` (`paths`, `hash`, `atomic_write`), `serde`, `serde_json`.

## 2. Terms

| Term | Meaning |
|---|---|
| project file | a `better-tiled.json`; one covers the maps in its folder and below |
| key | a map's name in the file: its path relative to the file's folder, with `/` |
| entry | what the file says about one map, under its key |
| record | one typed section of an entry (`automap`) or of the file (`godot`) |
| painted entry | one layer's painted cells (or generated objects) inside the AutoMap record |
| repository | the nearest folder above with `.git`, `.hg` or `.svn`: where the search stops |

## 3. Module map

| Module | Holds |
|---|---|
| `file` | `ProjectFile`: find, location, load, the locked read-modify-write (§5, §7) |
| `paths` | keys, relative and resolved paths, lexical normalisation (§4.3) |
| `schema` | the serde types of the file: `Document`, `MapEntry` and the records (§6) |
| `automap` | `AutoMapRecord`, `RunRecord`, `BindingRecord`, `PaintedRecord` (§6.2) |
| `rows` | the painted rows' text encoding (§6.3) |
| `checksum` | layer checksums (§6.4) |
| `godot` | `GodotSetup` and its layer setups (§6.5) |

## 4. Where the file is

### 4.1 Finding

`ProjectFile::find(map) -> Option<PathBuf>` looks in the map's own folder, then in each folder
above, nearest first, and returns the first `better-tiled.json`. The search stops at the top of
the map's repository and below the user's home folder (`bt-utils::paths::search_up`, the same
walk that finds a `.tiled-project`): a project file higher up belongs to something else, and a
clone of the repository would not have it. The map's own folder is searched even when it is the
home folder, so a map there keeps its file beside it.

### 4.2 Creating

`ProjectFile::location(map, open_project) -> PathBuf` is where the map's entry goes when no
file is found: next to the open `.tiled-project` when the map is in its folder or below and the
search from the map reaches that folder; else next to the map. Never where the map's own search
would not find it again.

| Layout | Found or created at |
|---|---|
| `game/levels/caves/deep.tmx`, nothing else | `game/levels/caves/better-tiled.json` |
| the same, with `game/game.tiled-project` open | `game/better-tiled.json` |
| the same, with `other/x.tiled-project` open | `game/levels/caves/better-tiled.json` |
| `game/levels/better-tiled.json` exists | that file, whatever is open |
| `~/better-tiled.json` and `~/games/new/.git`, map in `~/games/new/maps/` | not found; created in `~/games/new/maps/` |

### 4.3 Paths

Keys and the paths inside the file are relative to the file's folder, written with `/`, and
resolved lexically (`.` removed, `..` folded) without following links. The file is found in the
form the map's path has, through the same links, and the paths it gives back are in that form
too: the rules file of an AutoMap record and the map then read their tilesets and images by the
same paths, which is what makes an embedded tileset in a rule map match the map's. A canonical
path (through `/private/var` on macOS, say) would be another path to the same file and would
not match. A map's key is computed from the same normalised form, so one file always gets one
key.

## 5. Reading

`ProjectFile::load(path) -> Result<Document, Error>`: a missing file reads as an empty document;
a file that cannot be read or parsed is an error naming the file and the reason. Every caller
stops on that error rather than starting afresh: the editor opens the map with its AutoMap
layers off and says why; `bt edit` saves nothing; a Godot export writes nothing. An entry with
an unreadable record (a row that does not decode, a tileset key the map lacks) is a note from the
record's consumer, and only that record is affected (§6.3).

## 6. The schema

```json
{
  "betterTiled": 1,
  "godot": { … },
  "maps": {
    "levels/caves/deep.tmx": {
      "automap": { … }
    }
  }
}
```

`betterTiled` is the format version this file was last written with. Every object in the file
keeps the keys this version does not know, at every level (`serde(flatten)` into an ordered map),
and writes them back unchanged, so a file written by a newer version survives an older one's
save. A version number higher than the program's is read as far as understood and kept. Keys are
camelCase. Entries, keys and painted rows are sorted (`BTreeMap`s), so the same content always
gives the same bytes and a diff in version control shows only what changed.

### 6.1 Map entries

`maps` is keyed by map path (§4.3). An entry holds the records that apply to that map; an entry
with no record left is removed from the file, and a file with no entries and no `godot` section
is written as the empty document, not deleted.

### 6.2 The AutoMap record

The record is what `bt-automap`'s layers module needs to rebuild what was painted from the map
as saved (`08-automap.md`, §10). Meanings are its; the shape is fixed here.

| Key | Type | Holds |
|---|---|---|
| `rules` | string | the rules file or rule map, relative to the project file's folder |
| `seed` | integer | the seed of the rules' random choices |
| `runs` | array of runs | in order; each `{ "layers": [binding…] }` |
| binding `rules` | string | the name the rules use (`walls` for `input_walls`, `output_walls`) |
| binding `objects` | bool, default false | the rules' object output of that name rather than the tile layer |
| binding `layer` | integer | the map layer's id |
| binding `name` | string | the map layer's name when saved, for people reading the file |
| `painted` | array of painted entries | one per written layer whose painted cells differ from the map as saved |
| `nextObjectId` | integer, optional | the painted map's next object id when the generated objects took ids after it |

A painted entry:

| Key | Type | Holds |
|---|---|---|
| `layer` | integer | the layer's id |
| `name` | string | its name when saved |
| `checksum` | string | the layer as saved in the map (§6.4); when the map's layer no longer matches, the entry does not apply |
| `objects` | bool, default false | an object layer: `generated` lists the objects the rules placed |
| `tilesets` | array of strings | tile layers: the tilesets `rows` refer to by index, as `tileset key`s (§6.3) |
| `x` | integer, default 0 | tile layers: the column `rows` start at |
| `rows` | object, row number to string | tile layers: the painted cells of each row that has any (§6.3); no rows means nothing painted |
| `generated` | array of integers | object layers: the ids of the objects the rules placed |

### 6.3 Painted rows

A row is text, space separated, so that a diff of the project file is readable and a hand edit
is possible:

```
value   := "." | tileset ":" tile [ "/" flags ] ; tileset is an index into `tilesets`, tile an id
flags   := one or more of h v d x               ; horizontal, vertical, diagonal, hexagonal 120°
token   := value [ "*" count ]                  ; count repeats the value
row     := token ( " " token )*                 ; trailing empty cells are left off
```

A tileset key is the tileset's path as the map writes it (relative to the map), or `#name` for a
tileset embedded in the map. `rows::encode(cells) -> String` and `rows::decode(text) ->
Result<Vec<RowCell>, RowError>` are inverse; a count above a million, an unknown flag or a
malformed token is an error that names the token.

### 6.4 Checksums

`checksum::tile_layer(map, layer)` hashes the layer's size and its non-empty cells in row order,
each as position, tileset key, tile id and flips, so the tilesets' order in the map does not
matter. `checksum::objects(map, objects)` hashes each object's id, name, class, position, size,
rotation, visibility, tile (by tileset key), shape and properties: what editing in Tiled can
change. Both are FNV-1a over a canonical byte stream (`bt-utils::hash`), written as sixteen hex
digits. A checksum is a change detector, not a security measure.

### 6.5 The Godot setup

The `godot` section holds how the maps covered by this file go into a Godot project, as the
editor's setup sheet and `bt godot setup` write it. Paths outside the Godot project are relative
to the project file's folder; the others are `res://` paths. Meanings are `bt-godot`'s
(`12-godot-export.md`); the keys:

| Key | Type | Holds |
|---|---|---|
| `project` | string | the Godot project's folder, holding `project.godot` |
| `levels` | string | where scenes go (`res://levels/`) |
| `tileSets` | string | `shared` (one `.tres` per grid) or `embedded`; a string so a mode a newer version adds does not make the file unreadable |
| `tileSetFolder` | string | where shared TileSets go (`res://tilesets/`) |
| `images` | string | where copies of images from outside the project go (`res://images/`) |
| `levelScene` | bool | whether a first export also writes a starter scene that inherits the generated one |
| `exportOnSave` | bool | whether saving a map that has been exported exports it again |
| `physicsLayers` | array | per `godotPhysicsLayer` number: `collisionLayer`, `collisionMask` (default 1 each) |
| `navigationLayers` | array | per `godotNavigationLayer` number: `layers` (default 1) |
| `occlusionLayers` | array | per `godotOcclusionLayer` number: `lightMask` (default 1) |

Every key is optional; what is not given is Godot's default or the exporter's. An empty section
is kept as `{}`; a document without one writes none.

## 7. Writing

Both programs write the file, sometimes at the same moment (the editor saving one map while
`bt edit` saves another under the same file). `ProjectFile::edit(path, |document| …)` is the
only write path:

1. take an exclusive advisory lock for this file, held in the per-user runtime folder that
   `bt-utils::paths` provides (the same private folder the live link's socket uses), never
   beside the file, so nothing stray lands in the repository;
2. read the file again under the lock (§5);
3. apply the change: replace the caller's own record of the caller's own entry (`set_automap`,
   `set_godot`), drop an entry left empty, touch nothing else;
4. serialise (pretty, two-space indent, a final newline) with `betterTiled` set to the greater of
   the file's version and the program's;
5. write through a temporary file and rename (`bt-utils::atomic_write`), so a crash never
   leaves half a file;
6. release the lock.

A read-only file or folder is an error the caller reports; the map itself may still be saved
(`05-editor-core.md`, §8.2), with a notice that the painted state was not. A write that would
change nothing (the record equals what the file has) does not touch the file, so its
modification time stays meaningful.

## 8. Keys that move

- **Save As** and a move made in the editor take the entry along: `move_entry(from, to)` inside
  one file, or a removal from one file and an insertion into the file that covers the new place
  when the two differ. `bt edit --out` writes the copy's record under the copy's key and leaves
  the original's alone.
- **Orphans.** `Document::orphans(file)` lists the entries whose key names a file that no
  longer exists. The editor shows them in the Issues panel with the key and what the entry holds
  and offers to remove one; `bt check` lists them as problems (a map moved outside better-tiled
  keeps its painted state under the old key until it is moved back or the key is edited). An
  orphan is never dropped silently.

## 9. Versions

`betterTiled` changes only when a key's meaning changes; adding a key never bumps it, since
unknown keys are kept and defaults are explicit. A reader meeting a version it does not know
reads what it understands and says so once, in a note. The current version is 1.

## 10. Tests

| Test | What it pins |
|---|---|
| Finding | nearest file wins; the search stops at a repository top and below home; the map's own folder is searched even when it is home; a new file goes next to the open project only when that folder is reached from the map, else next to the map |
| Paths | keys and resolved paths in the map's form through a symbolic link (macOS temp folders); `..` keys; keys with `/` on Windows |
| Round trip | a document with unknown keys at every level is loaded, changed and saved with those keys intact; sorted output is byte-stable; an emptied entry disappears; a missing file reads as empty; an unreadable one is an error naming it |
| Rows | encode and decode are inverse on random rows with every flag, runs and gaps; every malformed token is refused with its text |
| Checksums | the tileset order does not change a tile layer's checksum; each object field that Tiled can edit does change the objects' checksum |
| Godot section | reads into its fields with defaults, writes back as read, keeps unknown keys in nested entries |
| Concurrent writes | two processes (and two threads with separate handles) editing different entries of one file lose nothing |
| Moves | Save As moves an entry within a file and across files; `--out` leaves the original; orphans are listed |

## 11. As built at M3

M3 built the whole crate, and `runtime_dir()` in `bt-utils::paths` for its locks. What exists:

| Module | What it holds |
|---|---|
| `file` | `ProjectFile::{find, location, load, edit, move_entry}`, `EntryPlace { file, key }` with `for_map`, `FILE_NAME`; the lock |
| `paths` | `key(file, path)`, `resolve(file, key)`, `tileset_key(tileset)` |
| `schema` | `Document` (`from_json`, `to_json`, `version`, `version_note`, `entry`, `keys`, `automap`, `set_automap`, `remove_automap`, `godot`, `set_godot`, `remove_godot`, `move_entry`, `remove_entry`, `orphans`), `MapEntry`, `Note` |
| `automap` | `AutoMapRecord`, `RunRecord`, `BindingRecord`, `PaintedRecord`: public fields, `unknown` in each |
| `rows` | `encode`, `decode`, `RowCell::{Empty, Tile { tileset, tile, flip }}`, `RowError::{CountTooLarge, UnknownFlag, Malformed}` |
| `checksum` | `tile_layer(map, layer)`, `objects(map, objects)` |
| `godot` | `GodotSetup` and its `PhysicsLayerSetup`, `NavigationLayerSetup`, `OcclusionLayerSetup` |
| `error` | `Error`, with `refused()` for a read-only file or folder |

Where it differs from the text above, or says more:

- **The user's folders are values.** `find(map, home)` and `location(map, open_project, home)`
  take the home folder, as `search_up` and the editor's builder do, and `edit(path,
  lock_folder, change)` and `move_entry` take the lock folder; the programs pass
  `std::env::home_dir()` and `runtime_dir()`, tests folders of their own (`13-testing.md`,
  §1). `find` and `location` return a `Result`: a relative map path is made absolute against
  the working folder. `location` returns the file `find` finds when there is one, so a writer
  calls it alone; whether the map's search reaches the open project's folder is decided with
  links followed, and the file's path stays in the map's form.
- **The lock** is `project-<16 hex digits>.lock` in the lock folder, from FNV-1a of the project
  file's canonical path (its folder's, for a file not made yet), so programs reaching one file
  through different links take one lock; it is `std::fs::File::lock`, which the system releases
  when a program dies. Lock files stay.
- **Writes.** An edit whose document equals what was read writes nothing, so a missing file
  whose change leaves it empty is not made. The empty document is `{"betterTiled": 1, "maps":
  {}}`. A move across files inserts into the new file first and removes from the old one after,
  and only while the entry is still what was copied, so a failure leaves it in both files.
- **Unknown keys of a replaced record.** `set_automap` and `set_godot` keep the replaced
  record's unknown top-level keys that the new one does not set, so an older version's save
  keeps what a newer one added there. Unknown keys of nested records (runs, bindings, painted
  entries, layer setups) are the caller's to carry: only `bt-automap` knows which new run is
  which old one.
- **Sorted keys** means every object's keys, known and unknown alike, by their text (painted
  rows by number), so two versions write a file in the same order.
- **Tileset keys** read `\` as `/` and fold `.` and `..`, so Tiled rewriting a reference as
  `../tiles/a.tsx` does not change a checksum; a reference not written yet is keyed by its
  resolved path.
- **Rows.** `decode` reads any spelling of the same cells (flags in any order, a count of one,
  trailing empty cells, several spaces); a repeated or empty flag list, flags on `.` and a count
  of zero are malformed. `encode` splits runs over a million.
- **Checksums** hash reals as their decimal form with six significant digits (`-0` as `0`): the
  precision Tiled's XML writes, so a layer checksummed before a save matches it read back. The
  objects' checksum also covers opacity, the template and the fields an instance sets, which
  Tiled 1.12 edits too. Pinned values guard the stream.
- **The newer-version note** is `Document::version_note()`; `load` returns the document alone.
- **The Godot setup** keeps every key, the layer masks included, as an `Option`, so a file is
  written back as read; `collision_layer_or_default()` and the others give Godot's 1.
- **Dependencies** add `thiserror` (`00-high-level-design.md`, §2.2).
- **`runtime_dir()`** is `11-live-link.md`, §4: `/tmp/better-tiled-<uid>`, the uid from `rustix`
  (no `unsafe`), made `0700` and checked on every call; a link, a non-folder or another user's
  folder is `bt_utils::Error::NotPrivate`, and a wider mode is narrowed. On Windows it is
  `better-tiled` in `%TEMP%`, the user's own temporary folder.

Tests: unit tests in each module, and `tests/` (`finding`, `round_trip`, `moves`,
`concurrent_writes`) for the table of §10 through files. The concurrent writes are three writers
in two processes (the test binary run again as a child) and two threads, one through a link,
and an edit shown waiting for another's lock; each fails with the lock taken out. Randomised:
rows, and random edits on documents with unknown keys everywhere checked against a plain JSON
model (`BT_FUZZ_SEED`, `BT_FUZZ_CASES`).

Not here yet: the callers. The editor's and `bt`'s reads and writes, the Issues panel's orphans,
`bt check` and the records' meanings come with `bt-automap`, `bt-godot` and their milestones.
