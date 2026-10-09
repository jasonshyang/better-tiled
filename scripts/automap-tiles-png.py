#!/usr/bin/env python3
"""Writes crates/bt-tmx/src/automap-tiles.png: the image of the built-in rule tileset.

Five 32x32 tiles in a row, drawn here (not taken from Tiled): Negate (a red circle with a
slash), Ignore (a grey dotted outline), NonEmpty (a solid green square), Empty (a hollow blue
square) and Other (an orange square with stripes). Uses only the standard library.
"""

import pathlib
import struct
import zlib

SIZE = 32
COUNT = 5


def png(width, height, pixels):
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    rows = b"".join(b"\x00" + bytes(pixels[y * width * 4:(y + 1) * width * 4]) for y in range(height))
    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
            + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b""))


def draw():
    width = SIZE * COUNT
    pixels = [0] * (width * SIZE * 4)

    def put(tile, x, y, color):
        if 0 <= x < SIZE and 0 <= y < SIZE:
            i = (y * width + tile * SIZE + x) * 4
            pixels[i:i + 4] = color

    red, grey, green, blue, orange = (220, 40, 40, 255), (150, 150, 150, 255), (60, 170, 70, 255), (50, 90, 220, 255), (240, 140, 30, 255)
    dark = (30, 30, 30, 255)
    for y in range(SIZE):
        for x in range(SIZE):
            cx, cy = x - 15.5, y - 15.5
            distance = (cx * cx + cy * cy) ** 0.5
            # Negate: a ring and a slash.
            if 10.5 <= distance <= 13.5 or (distance <= 12 and abs(cx + cy) <= 1.6):
                put(0, x, y, red)
            inside = 3 <= x <= 28 and 3 <= y <= 28
            edge = inside and (x in (3, 4, 27, 28) or y in (3, 4, 27, 28))
            # Ignore: a dotted outline.
            if edge and ((x + y) // 3) % 2 == 0:
                put(1, x, y, grey)
            # NonEmpty: a filled square with a dark rim.
            if inside:
                put(2, x, y, dark if edge else green)
            # Empty: a hollow square.
            if edge:
                put(3, x, y, blue)
            # Other: a square with diagonal stripes.
            if inside:
                put(4, x, y, dark if edge else (orange if ((x - y) // 4) % 2 == 0 else (255, 215, 160, 255)))
    return png(width, SIZE, pixels)


target = pathlib.Path(__file__).resolve().parent.parent / "crates/bt-tmx/src/automap-tiles.png"
target.write_bytes(draw())
print(f"wrote {target}")
