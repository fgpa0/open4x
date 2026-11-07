#!/usr/bin/env python3
"""Rasterises Earth's land, large lakes and landmasses into data/earth.ron.

Source: Natural Earth 1:50m land and lakes (public domain, naturalearthdata.com),
as GeoJSON from github.com/nvkelso/natural-earth-vector:

    curl -LO https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_50m_land.geojson
    curl -LO https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_50m_lakes.geojson
    python3 data/tools/earth_mask.py ne_50m_land.geojson ne_50m_lakes.geojson data/earth.ron

Replaces the `land:` and `masses:` lists in earth.ron (everything else in the
file is kept). The grid has ROWS rows of COLS cells (half a degree each),
north to south, west to east from 180 W, each row run-length encoded: a
symbol, then how many cells it repeats for when more than one.

- `land`: '.' sea, 'a'..'i' a ninth to all of the cell land, 'L' lake.
- `masses`: which landmass covers most of the cell: '.' none (sea, or one of
  the many small islands), otherwise one symbol per landmass, the largest
  first (MASS_SYMBOLS). The generator keeps different landmasses apart.

Coverage is exact east-west (scanline intervals) and sampled SUB times per
cell north-south. Pure Python, no dependencies; takes a minute or so.
"""
import json
import math
import re
import sys

COLS, ROWS, SUB = 720, 360, 6
CELL = 360 / COLS
# Any symbol but digits (run lengths), '.', '"' and '\\'.
MASS_SYMBOLS = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!#$%&'()*+,-/:;<=>?@[]^_`{|}~"
MIN_MASS_KM2 = 2500


def polygons(path):
    data = json.load(open(path))
    out = []
    for f in data["features"]:
        g = f["geometry"]
        out.extend(g["coordinates"] if g["type"] == "MultiPolygon" else [g["coordinates"]])
    return out


def area_km2(ring):
    """Roughly: the integral of x dy, with x shrunk by the cosine of latitude."""
    a = 0.0
    for (x0, y0), (x1, y1) in zip(ring, ring[1:] + ring[:1]):
        a += (x0 + x1) / 2 * math.cos(math.radians((y0 + y1) / 2)) * (y1 - y0)
    return abs(a) * 111.32 ** 2


def coverage(ring_list):
    """{(row, col): covered fraction}, by even-odd fill of every ring."""
    nsub = ROWS * SUB
    ymin = min(y for r in ring_list for _, y in r)
    ymax = max(y for r in ring_list for _, y in r)
    s0 = max(0, int((90 - ymax) / CELL * SUB) - 1)
    s1 = min(nsub - 1, int((90 - ymin) / CELL * SUB) + 1)
    buckets = {}
    for ring in ring_list:
        for (x0, y0), (x1, y1) in zip(ring, ring[1:] + ring[:1]):
            if y0 == y1:
                continue
            lo, hi = min(y0, y1), max(y0, y1)
            a = max(s0, int((90 - hi) / CELL * SUB - 0.5))
            b = min(s1, int((90 - lo) / CELL * SUB + 0.5))
            for s in range(a, b + 1):
                buckets.setdefault(s, []).append((x0, y0, x1, y1))
    cover = {}
    for s, edges in buckets.items():
        y = 90 - (s + 0.5) / SUB * CELL
        xs = sorted(x0 + (y - y0) / (y1 - y0) * (x1 - x0) for x0, y0, x1, y1 in edges if (y0 <= y < y1) or (y1 <= y < y0))
        row = s // SUB
        for i in range(0, len(xs) - 1, 2):
            a, b = (xs[i] + 180) / CELL, (xs[i + 1] + 180) / CELL
            c = int(a)
            while c < COLS and c < b:
                key = (row, c)
                cover[key] = cover.get(key, 0.0) + (min(b, c + 1) - max(a, c)) / SUB
                c += 1
    return cover


def rle(cells):
    out, i = [], 0
    while i < len(cells):
        j = i
        while j < len(cells) and cells[j] == cells[i]:
            j += 1
        out.append(cells[i] + (str(j - i) if j - i > 1 else ""))
        i = j
    return "".join(out)


def main():
    land_polys = polygons(sys.argv[1])
    land = coverage([r for p in land_polys for r in p])
    lakes = coverage([r for p in polygons(sys.argv[2]) for r in p])
    # Landmasses, largest first.
    big = sorted(((area_km2(p[0]), p) for p in land_polys), key=lambda t: -t[0])
    big = [p for a, p in big if a >= MIN_MASS_KM2][: len(MASS_SYMBOLS)]
    best = {}
    for i, p in enumerate(big):
        for key, f in coverage(p).items():
            if f > best.get(key, (0.0, None))[0]:
                best[key] = (f, MASS_SYMBOLS[i])
    land_rows, mass_rows = [], []
    for r in range(ROWS):
        cells, masses = [], []
        for c in range(COLS):
            lake = lakes.get((r, c), 0.0)
            f = max(0.0, min(1.0, land.get((r, c), 0.0) - lake))
            if lake >= 0.5:
                cells.append("L")
            else:
                n = round(f * 9)
                cells.append("." if n == 0 else "abcdefghi"[n - 1])
            masses.append(best[(r, c)][1] if (r, c) in best and best[(r, c)][0] >= 0.05 else ".")
        land_rows.append(rle(cells))
        mass_rows.append(rle(masses))
    print(f"{len(big)} landmasses", file=sys.stderr)
    fmt = lambda rows: "\n".join(f'        "{row}",' for row in rows)
    text = open(sys.argv[3]).read()
    text = re.sub(r"    land: \[\n.*?\n    \],\n", lambda m: f"    land: [\n{fmt(land_rows)}\n    ],\n", text, count=1, flags=re.S)
    if "    masses: [" in text:
        text = re.sub(r"    masses: \[\n.*?\n    \],\n", lambda m: f"    masses: [\n{fmt(mass_rows)}\n    ],\n", text, count=1, flags=re.S)
    else:
        text = text.replace("    ranges: [\n", f"    masses: [\n{fmt(mass_rows)}\n    ],\n    ranges: [\n", 1)
    open(sys.argv[3], "w").write(text)


if __name__ == "__main__":
    main()
