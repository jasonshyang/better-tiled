# README showcase

`tiny-swords-showcase.gif` is an automated construction timelapse in the actual Better Tiled UI.
It starts with empty tile and object layers, fills the ocean, paints the islands, adds cliffs
and ramps, then places the buildings, trees, characters and details. The camera stays fixed,
and the finished scene animates for three seconds before the loop restarts.

Painting uses real canvas clicks and the application's stamp brush. Object placement uses
the same undoable insertion intents as the UI. The recording checks that the saved map matches
every reference tile, sprite position, flip and object draw order. A short stage caption and
artwork credit sit below the captured UI.

Artwork: **[Tiny Swords by Pixel Frog](https://pixelfrog-assets.itch.io/tiny-swords)**,
the current **Tiny Swords Free Pack** supplied by the user. Tile selection, sprite positions,
mirroring and overlapping object order were measured from the creator's
[promotional banner](https://raw.githubusercontent.com/Adriano-97/pixelImages/refs/heads/main/BigBanner_03.gif).
The layout is stored in `scripts/showcase-layout.json`; the generator preserves separate ground,
shadow and cliff layers so transparent ramps and corners join correctly. The banner's logo
obscures the northeast: ground behind it is inferred, and that hidden arrangement is unverified.
The creator permits personal and commercial project use and modification, but prohibits
redistribution, resale or repackaging of the asset files. This repository includes the rendered
showcase and recording tools; the ZIP and extracted sprites remain outside the repository.
The current pack is not CC0. The application keeps its own licenses.

## Regenerate

Requires macOS or Linux with a working wgpu adapter, Rust 1.95, ffmpeg, and Python 3.9+
with Pillow. The GPU capture runs offscreen, so it does not need screen recording permissions
or capture anything from the desktop. It works
in a temporary generated village with isolated editor settings. Supply your own current
Free Pack ZIP downloaded from the creator; it is not stored in this repository.

```bash
python3 scripts/record-showcase.py --asset-zip "/path/to/Tiny Swords (Free Pack).zip"
```

Use `--work-dir /tmp/bt-showcase-frames` with a **new folder** to retain the raw captures,
completed map, reference map, frame timing manifest and construction verification log.
Use `--output` for another GIF path. Font defaults are Arial on macOS and DejaVu Sans
on Linux; pass `--font` if it is installed elsewhere.

The GIF loops at 10 fps and 1280 × 840. Keep the construction sequence focused so it
remains readable in the README and small enough to load quickly. The ordinary test suite
skips the GPU recording.
