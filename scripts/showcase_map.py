"""Build the measured Tiny Swords banner layout from a user-supplied Free Pack.

Only layout coordinates live in the repository. Licensed pixels are extracted
into the recording's scratch directory. See docs/media/README.md for provenance.
"""

from io import BytesIO
import json
from pathlib import Path
from xml.etree import ElementTree as ET
from zipfile import ZipFile

from PIL import Image

LAYOUT = Path(__file__).with_name("showcase-layout.json")
TILE_SIZE = 64
FLIP_HORIZONTAL = 0x80000000


def create_map(archive, folder):
    layout = json.loads(LAYOUT.read_text())
    width, height = layout["width"], layout["height"]
    for name in ["ground", "terrain"]:
        if len(layout[name]) != height or any(len(row) != width for row in layout[name]):
            raise ValueError(f"{name} must contain {height} rows of {width} cells")
    folder.mkdir()
    images = folder / "images"
    images.mkdir()

    def node(parent, tag, **attrs):
        return ET.SubElement(parent, tag, {key: str(value) for key, value in attrs.items()})

    def write_xml(name, element):
        ET.indent(element)
        ET.ElementTree(element).write(folder / name, encoding="UTF-8", xml_declaration=True)

    def atlas(name, file, *, size, columns, rows):
        tileset = ET.Element("tileset", name=name, tilewidth=str(size), tileheight=str(size),
                             tilecount=str(columns * rows), columns=str(columns))
        node(tileset, "image", source=f"images/{file}", width=columns * size, height=rows * size)
        return tileset

    village = ET.Element("map", version="1.10", tiledversion="1.11.2", orientation="orthogonal",
                         renderorder="right-down", width=str(width), height=str(height),
                         tilewidth=str(TILE_SIZE), tileheight=str(TILE_SIZE), infinite="0",
                         nextlayerid="8", nextobjectid="3000")

    def add_tileset(first_gid, name, tileset):
        write_xml(f"{name}.tsx", tileset)
        node(village, "tileset", firstgid=first_gid, source=f"{name}.tsx")

    with ZipFile(archive) as pack:
        def read_asset(source):
            member = "Tiny Swords (Free Pack)/" + source
            return pack.read(member)  # A missing or incompatible pack must fail explicitly.

        for palette in range(1, 6):
            file = f"Tilemap_color{palette}.png"
            (images / file).write_bytes(read_asset(f"Terrain/Tileset/{file}"))
        for file in ["Water Background color.png", "Water Foam.png"]:
            (images / file).write_bytes(read_asset(f"Terrain/Tileset/{file}"))
        add_tileset(1, "water", atlas("Water", "Water Background color.png", size=64, columns=1, rows=1))
        for palette in range(1, 6):
            add_tileset(2 + (palette - 1) * 54, f"terrain{palette}", atlas(
                f"Terrain {palette}", f"Tilemap_color{palette}.png", size=64, columns=9, rows=6))
        surf = atlas("Surf", "Water Foam.png", size=192, columns=16, rows=1)
        animation = node(node(surf, "tile", id=0), "animation")
        for frame in range(16):
            node(animation, "frame", tileid=frame, duration=100)
        add_tileset(300, "surf", surf)

        # Keep the measured shadow pieces below transparent cliff and ramp pixels.
        # Baking their overlaps into an atlas avoids double-darkening adjacent pieces.
        shadow = Image.open(BytesIO(read_asset("Terrain/Tileset/Shadow.png"))).convert("RGBA")
        shadow_atlas = Image.new("RGBA", (width * TILE_SIZE, height * TILE_SIZE))
        for index in layout["shadow_sources"]:
            shadow_atlas.alpha_composite(shadow, ((index % width - 1) * TILE_SIZE,
                                                  (index // width - 1) * TILE_SIZE))
        shadow_atlas.save(images / "shadows.png")
        add_tileset(400, "shadows", atlas("Cliff shadows", "shadows.png", size=64,
                                           columns=width, rows=height))

        objects = ET.Element("tileset", name="Village", tilewidth="320", tileheight="320",
                             tilecount="0", columns="0", objectalignment="bottomleft")
        variants = {}
        for placement in layout["objects"]:
            key = f'{placement["sprite"]}:{placement["phase"]}'
            if key not in variants:
                variants[key] = len(variants)
        next_id = len(variants)
        frame_tiles = {}
        for name, sprite in layout["sprites"].items():
            source = Image.open(BytesIO(read_asset(sprite["source"]))).convert("RGBA")
            frame_width, frame_height = sprite["width"], sprite["height"]
            columns = source.width // frame_width
            if not columns or source.width % frame_width or source.height % frame_height:
                raise ValueError(f"Invalid frame dimensions for {name}: {source.size}")
            frame_tiles[name] = []
            for frame in range(sprite["frames"]):
                x, y = frame % columns * frame_width, frame // columns * frame_height
                if y + frame_height > source.height:
                    raise ValueError(f"Frame {frame} exceeds {sprite['source']}")
                picture = source.crop((x, y, x + frame_width, y + frame_height))
                file = f"{name}.png" if frame == 0 else f"{name}-{frame}.png"
                picture.save(images / file)
                tile = node(objects, "tile", id=next_id)
                node(tile, "image", source=f"images/{file}", width=frame_width, height=frame_height)
                frame_tiles[name].append(next_id)
                next_id += 1
        for key, tile_id in variants.items():
            name, phase_text = key.split(":")
            phase = int(phase_text)
            sprite = layout["sprites"][name]
            if not 0 <= phase < sprite["frames"]:
                raise ValueError(f"Invalid animation phase for {key}")
            tile = node(objects, "tile", id=tile_id, type=name)
            file = f"{name}.png" if phase == 0 else f"{name}-{phase}.png"
            node(tile, "image", source=f"images/{file}", width=sprite["width"], height=sprite["height"])
            if sprite["frames"] > 1:
                animation = node(tile, "animation")
                for step in range(sprite["frames"]):
                    node(animation, "frame", tileid=frame_tiles[name][(phase + step) % sprite["frames"]],
                         duration=100)
        objects.set("tilecount", str(next_id))
        add_tileset(1000, "village", objects)

    def tile_layer(layer_id, name, rows):
        entry = node(village, "layer", id=layer_id, name=name, width=width, height=height)
        node(entry, "data", encoding="csv").text = "\n" + ",\n".join(
            ",".join(str(gid) for gid in row) for row in rows
        ) + "\n"

    tile_layer(1, "Ocean", [[1] * width for _ in range(height)])
    shoreline = node(village, "objectgroup", id=2, name="Shoreline", draworder="index")
    for y, row in enumerate(layout["terrain"]):
        for x, gid in enumerate(row):
            coast = gid and any(
                not (0 <= neighbor_x < width and 0 <= neighbor_y < height)
                or not layout["terrain"][neighbor_y][neighbor_x]
                for neighbor_x, neighbor_y in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
            )
            if coast:
                node(shoreline, "object", id=1000 + y * width + x, gid=300,
                     x=(x - 1) * TILE_SIZE, y=(y + 2) * TILE_SIZE, width=192, height=192)
    tile_layer(3, "Ground beneath cliffs", layout["ground"])
    selected_shadows = set(layout["shadows"])
    tile_layer(4, "Cliff shadows", [
        [400 + y * width + x if y * width + x in selected_shadows else 0 for x in range(width)]
        for y in range(height)
    ])
    tile_layer(5, "Terrain", layout["terrain"])
    # Explicit order preserves overlapping trees and archers standing on the roofs.
    props = node(village, "objectgroup", id=7, name="Props", draworder="index")
    for placement in layout["objects"]:
        sprite = layout["sprites"][placement["sprite"]]
        gid = 1000 + variants[f'{placement["sprite"]}:{placement["phase"]}']
        if placement["flip_x"]:
            gid |= FLIP_HORIZONTAL
        node(props, "object", id=placement["id"], name=placement["sprite"], gid=gid,
             x=placement["x"], y=placement["y"] + sprite["height"],
             width=sprite["width"], height=sprite["height"])
    write_xml("showcase.tmx", village)
    return folder / "showcase.tmx"
