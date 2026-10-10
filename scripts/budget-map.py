#!/usr/bin/env python3
"""Writes the budget map of docs/design/03-rendering.md, §11: 1000×1000 cells in ten layers, the
map one frame at 120 Hz is measured on (better-tiled --bench). Every cell of the first layer is
painted and half the cells of each other layer, from Tiled's desert tileset in reference/ (run
scripts/fetch-reference.sh first). The tiles are chosen by a fixed generator, so every run writes
the same file.

Run:  python3 scripts/budget-map.py [out.tmx]     (target/budget/budget.tmx by default)
"""

import base64
import os
import pathlib
import struct
import sys
import zlib

SIZE = 1000
LAYERS = 10
TILES = 48
MASK = (1 << 64) - 1


def splitmix(state):
    """The next state and value of splitmix64."""
    state = (state + 0x9E3779B97F4A7C15) & MASK
    z = state
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
    return state, z ^ (z >> 31)


def layer_data(index):
    """A layer's global tile ids, base64 of zlib, as Tiled writes them."""
    state = 0xB7D6E7 + index
    cells = bytearray()
    for _ in range(SIZE * SIZE):
        state, value = splitmix(state)
        painted = index == 0 or value & 1
        gid = 1 + (value >> 8) % TILES if painted else 0
        cells += struct.pack("<I", gid)
    return base64.b64encode(zlib.compress(bytes(cells), 9)).decode("ascii")


def main():
    root = pathlib.Path(__file__).resolve().parent.parent
    out = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else root / "target/budget/budget.tmx"
    tileset = root / "reference/tiled/examples/desert.tsx"
    if not tileset.exists():
        sys.exit(f"{tileset} is missing: run scripts/fetch-reference.sh")
    out.parent.mkdir(parents=True, exist_ok=True)
    source = os.path.relpath(tileset, out.resolve().parent)
    lines = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        f'<map version="1.10" tiledversion="1.11.2" orientation="orthogonal" renderorder="right-down" '
        f'width="{SIZE}" height="{SIZE}" tilewidth="32" tileheight="32" infinite="0" '
        f'nextlayerid="{LAYERS + 1}" nextobjectid="1">',
        f' <tileset firstgid="1" source="{source}"/>',
    ]
    for index in range(LAYERS):
        lines.append(
            f' <layer id="{index + 1}" name="Layer {index + 1}" width="{SIZE}" height="{SIZE}">'
        )
        lines.append('  <data encoding="base64" compression="zlib">')
        lines.append("   " + layer_data(index))
        lines.append("  </data>")
        lines.append(" </layer>")
    lines.append("</map>")
    out.write_text("\n".join(lines) + "\n")
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
