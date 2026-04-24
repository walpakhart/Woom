#!/usr/bin/env python3
"""Apply a macOS-style squircle mask to the source icon so .icns/.ico/.png
renders sit inside the standard rounded-rect shape you see in the Dock."""

import os
import sys
from PIL import Image, ImageDraw, ImageFilter

SRC = os.path.expanduser(
    "~/Repos/pers/forge/apps/desktop/src-tauri/icons/source.png"
)
OUT = os.path.expanduser(
    "~/Repos/pers/forge/apps/desktop/src-tauri/icons/source-masked.png"
)

# macOS app icons: ~22.37% corner radius on the full canvas. Apple uses a
# continuous "squircle" (superellipse) but a standard rounded rectangle is
# close enough at typical display sizes and trivially generated in PIL.
CORNER_FRACTION = 0.2237

src = Image.open(SRC).convert("RGBA")
w, h = src.size
radius = int(min(w, h) * CORNER_FRACTION)

# Render mask at 4x for smoother corners, then downsample.
scale = 4
mask_hi = Image.new("L", (w * scale, h * scale), 0)
ImageDraw.Draw(mask_hi).rounded_rectangle(
    [(0, 0), (w * scale - 1, h * scale - 1)],
    radius=radius * scale,
    fill=255,
)
mask = mask_hi.resize((w, h), Image.LANCZOS)

out = Image.new("RGBA", (w, h), (0, 0, 0, 0))
out.paste(src, (0, 0), mask)
out.save(OUT)
print(f"wrote {OUT} ({w}x{h})")
