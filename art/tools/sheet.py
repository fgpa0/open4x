"""Puts the first view of several previews (target/art/<name>.png) on one
contact sheet: python3 art/tools/sheet.py out.png name..."""

import sys

from PIL import Image, ImageDraw

out, names = sys.argv[1], sys.argv[2:]
tiles = []
for n in names:
    im = Image.open(f"target/art/{n}.png")
    h = im.height
    views = min(4, max(1, im.width // h))
    if views == 4:
        tile = Image.new("RGB", (h * 2, h))
        tile.paste(im.crop((0, 0, h, h)), (0, 0))
        tile.paste(im.crop((h * 3, 0, h * 4, h)), (h, 0))
    else:
        tile = im.convert("RGB")
    tile = tile.resize((tile.width * 300 // tile.height, 300))
    ImageDraw.Draw(tile).text((6, 4), n, fill=(255, 255, 255))
    tiles.append(tile)
cols = 3
w = max(t.width for t in tiles)
sheet = Image.new("RGB", (w * cols, 300 * ((len(tiles) + cols - 1) // cols)), (40, 40, 40))
for i, t in enumerate(tiles):
    sheet.paste(t, ((i % cols) * w, (i // cols) * 300))
sheet.save(out)
