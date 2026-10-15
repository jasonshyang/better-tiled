#!/usr/bin/env python3
"""Record the map being built in Better Tiled and assemble the README GIF.

Requires Rust, a working wgpu adapter, ffmpeg, and Pillow.
Uses the current Tiny Swords Free Pack ZIP; asset files stay in scratch.
"""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from xml.etree import ElementTree as ET

from PIL import Image, ImageDraw, ImageFont
from showcase_map import create_map

ROOT = Path(__file__).resolve().parent.parent
FPS = 10


def run(args, **kwargs):
    subprocess.run(args, cwd=ROOT, check=True, **kwargs)


def capture(work, archive):
    map_path = create_map(archive, work / "village")
    shutil.copyfile(map_path, map_path.with_name("reference.tmx"))
    template = ET.parse(map_path)
    for layer in template.findall("layer"):
        layer.find("data").text = ",".join(["0"] * (int(layer.get("width")) * int(layer.get("height"))))
    for layer in template.findall("objectgroup"):
        for item in layer.findall("object"):
            layer.remove(item)
    template.write(map_path, encoding="UTF-8", xml_declaration=True)
    env = os.environ | {
        "BT_SHOWCASE_DIR": str(work / "raw"),
        "BT_SHOWCASE_MAP": str(map_path),
    }
    run([
        "cargo", "test", "-p", "better-tiled", "--locked",
        "test_window::showcase::record_showcase", "--", "--ignored", "--exact",
    ], env=env)


def assemble(work, output, font):
    raw = work / "raw"
    frames = work / "captioned"
    frames.mkdir()
    title_font = ImageFont.truetype(str(font), 20)
    small_font = ImageFont.truetype(str(font), 15)
    count = 0

    for entry in json.loads((raw / "frames.json").read_text()):
        with Image.open(raw / entry["file"]) as source:
            screenshot = source.convert("RGB").resize((1280, 800), Image.Resampling.LANCZOS)
        frame = Image.new("RGB", (1280, 840), "#181b1f")
        frame.paste(screenshot, (0, 0))
        draw = ImageDraw.Draw(frame)
        draw.line((0, 800, 1280, 800), fill="#344350")
        draw.text((20, 810), entry["caption"], font=title_font, fill="#f1f6fb")
        draw.text((1260, 813), "Tiny Swords by Pixel Frog", font=small_font, fill="#bdcad6", anchor="ra")
        for _ in range(entry["ticks"]):
            frame.save(frames / f"{count:04}.png")
            count += 1

    output.parent.mkdir(parents=True, exist_ok=True)
    run([
        "ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
        "-framerate", str(FPS), "-i", str(frames / "%04d.png"),
        "-filter_complex",
        "split[a][b];[a]palettegen=stats_mode=full[p];"
        "[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle",
        "-loop", "0", str(output),
    ])
    with Image.open(output) as gif:
        duration = 0
        for index in range(gif.n_frames):
            gif.seek(index)
            duration += gif.info["duration"]
        if gif.info.get("loop") != 0 or gif.size != (1280, 840):
            raise RuntimeError("GIF must loop forever at 1280 x 840")
    print(f"{output}: {duration / 1000:.1f}s, {output.stat().st_size / 1024**2:.2f} MiB")


def main():
    mac = sys.platform == "darwin"
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--asset-zip", type=Path, required=True, help="Current Tiny Swords (Free Pack).zip")
    parser.add_argument("--output", type=Path, default=ROOT / "docs/media/tiny-swords-showcase.gif")
    parser.add_argument("--font", type=Path, default=Path(
        "/System/Library/Fonts/Supplemental/Arial.ttf" if mac else
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"))
    parser.add_argument("--work-dir", type=Path, help="Keep frames and verification logs in a new folder")
    args = parser.parse_args()
    if not args.font.is_file():
        parser.error(f"Font missing: {args.font}; pass --font")
    if not shutil.which("ffmpeg"):
        parser.error("ffmpeg is required")
    if args.work_dir:
        work = args.work_dir.resolve()
        work.mkdir(parents=True, exist_ok=False)
        capture(work, args.asset_zip)
        assemble(work, args.output.resolve(), args.font)
    else:
        with tempfile.TemporaryDirectory(prefix="bt-showcase-") as folder:
            work = Path(folder).resolve()
            capture(work, args.asset_zip)
            assemble(work, args.output.resolve(), args.font)


if __name__ == "__main__":
    main()
