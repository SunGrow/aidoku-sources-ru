#!/usr/bin/env python3
"""Generate source icons.

Aidoku wants a square, opaque 128x128 PNG per source. These are plain
original marks -- a tinted rounded square with the source's initials -- so
that nothing here reproduces a site's own logo.

Usage: python tools/make_icons.py [source_id ...]
"""

from __future__ import annotations

import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

SIZE = 128
RADIUS = 28
SUPERSAMPLE = 4

# source id -> (initials, background, foreground)
ICONS: dict[str, tuple[str, str, str]] = {
    "ru.wamanga": ("W", "#2b1b4f", "#f2e9ff"),
    "ru.comx": ("CX", "#7f1220", "#ffe8ea"),
    "ru.remanga": ("R", "#0f3f4a", "#d8f6ff"),
    "ru.desume": ("D", "#8a3a10", "#ffeadb"),
    "ru.acomics": ("A", "#1d4620", "#e4ffe6"),
    "ru.mangalib": ("ML", "#152a6b", "#e3ebff"),
    "ru.readmanga": ("RM", "#3d2a0c", "#fff1d6"),
    "ru.mintmanga": ("MM", "#0d4034", "#ddfff4"),
    "ru.selfmanga": ("SM", "#43123d", "#ffe6fb"),
    "ru.usagi": ("U", "#4a1430", "#ffe3f0"),
}

FONT_CANDIDATES = [
    "C:/Windows/Fonts/segoeuib.ttf",
    "C:/Windows/Fonts/arialbd.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
]


def load_font(size: int) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    for path in FONT_CANDIDATES:
        if Path(path).exists():
            return ImageFont.truetype(path, size)
    return ImageFont.load_default()


def render(text: str, background: str, foreground: str) -> Image.Image:
    scale = SIZE * SUPERSAMPLE
    image = Image.new("RGB", (scale, scale), background)
    draw = ImageDraw.Draw(image)

    # A soft inner square keeps the mark from looking like a flat color chip.
    inset = 10 * SUPERSAMPLE
    draw.rounded_rectangle(
        [inset, inset, scale - inset, scale - inset],
        radius=RADIUS * SUPERSAMPLE,
        outline=foreground,
        width=3 * SUPERSAMPLE,
    )

    font_size = int(scale * (0.46 if len(text) > 1 else 0.58))
    font = load_font(font_size)
    left, top, right, bottom = draw.textbbox((0, 0), text, font=font)
    draw.text(
        ((scale - right - left) / 2, (scale - bottom - top) / 2),
        text,
        font=font,
        fill=foreground,
    )

    return image.resize((SIZE, SIZE), Image.LANCZOS)


def main(argv: list[str]) -> int:
    root = Path(__file__).resolve().parent.parent
    wanted = argv or sorted(ICONS)

    for source_id in wanted:
        if source_id not in ICONS:
            print(f"no icon spec for {source_id}", file=sys.stderr)
            return 1

        destination = root / "sources" / source_id / "res" / "icon.png"
        if not destination.parent.exists():
            # Sources are added over time; skip ones not scaffolded yet.
            continue

        render(*ICONS[source_id]).save(destination, "PNG", optimize=True)
        print(f"wrote {destination.relative_to(root)}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
