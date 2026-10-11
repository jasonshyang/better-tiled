# Agent runs

The release gate for `bt` (`docs/design/13-testing.md`, §10.1): a fresh agent gets one brief from
this folder and the output of `bt guide`, nothing else, and works on a copy of the map the brief
names. The gate passes when, for every brief, `bt check` on the result is clean (or lists only
the problems the map had before) and a render shows what the brief asked for. The agent's own
report is never the evidence; what in `bt` got in its way is, and it is turned into typed
problems or guide text, never into a generator.

Each brief names its map relative to Tiled's examples (`reference/tiled/examples`, fetched by
`scripts/fetch-reference.sh`) or, for `editor-pack.md`, a map of a purchased pack in `assets/`.
The person running the gate copies the map's folder to a scratch folder first, puts `bt` (and
for `editor-pack.md` the running editor) on the agent's `PATH`, and gives the agent:

    {the brief}

    Here is bt's guide, the only documentation there is:

    {the output of bt guide}

| Brief | Map | What it exercises |
|---|---|---|
| `desert.md` | `desert.tmx` | terrains of one set, walls drawn by tiles, objects |
| `sewers.md` | `sewer_automap/sewers.tmx` | AutoMap rules beside the map, AutoMap layers |
| `isometric.md` | `isometric_grass_and_water.tmx` | a non-square grid, terrain on diamonds |
| `hexagonal.md` | `hexagonal-mini.tmx` | hexagons, plain tiles without terrains |
| `perspective.md` | `perspective_walls.tmx` | walls of tiles placed by hand |
| `sticker-knight.md` | `sticker-knight/map/sandbox.tmx` | a map of objects only, sprites by their feet |
| `editor-pack.md` | a Highlands pack map, open in the editor | the live link: edits as undo steps, marks |
