"""Battleship: a broad grey hull with three turrets of triple big guns, a
tall tower bridge with fire-control tops, one great funnel with a band in
the owner's colour, secondary guns along the sides and a tripod mast."""

import math

import people
import ships
from mathutils import Vector

L, B, DECK = 48.0, 8.6, 2.6


def turret(k, mats, at, aim, guns=3):
    grey, dark = mats
    out = [k.cyl(1.9, 0.5, grey, loc=at + Vector((0, 0, 0.25)), n=24)]
    body = k.box((3.4, 4.0, 1.4), grey, loc=at + Vector((0, -aim * 0.3, 1.0)), bevel=0.35)
    out.append(body)
    for i in range(guns):
        x = (i - (guns - 1) / 2) * 0.85
        out.append(k.rod(at + Vector((x, aim * 1.6, 1.1)), at + Vector((x, aim * 6.2, 1.2)), 0.2, dark, n=10, r2=0.16))
    return out


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("cannon")
    rig.bone("hull", None, (0, 0, 0.0), (0, -L / 2, 0), radius=5.0)
    grey = k.mat("navy_grey", k.srgb(120, 126, 134), 0.55, metal=0.3, pattern="streaks", scale=2.0, amount=0.15)
    dark = k.mat("gun_grey", k.srgb(80, 84, 90), 0.5, metal=0.5)
    deck = k.mat("teak", k.srgb(176, 150, 112), 0.85, pattern="streaks", scale=3.0, amount=0.3)
    glass = k.mat("bridge_glass", k.srgb(40, 50, 60), 0.15, metal=0.4)
    h, station = ships.hull(k, p, L, B, DECK, bow_rise=0.9, stern_rise=0.0, keel=-2.6, fullness=0.5, transom=True, band=(1.5, 0.6), planks=grey)
    parts = [h]
    _, _, d = station(0.5)
    mats = (grey, dark)
    # Turrets: two forward (the second raised to fire over the first), one aft.
    parts += turret(k, mats, Vector((0, -15.0, d)), -1)
    parts.append(k.cyl(2.0, 1.2, grey, loc=(0, -9.6, d + 0.6), n=24))
    parts += turret(k, mats, Vector((0, -9.6, d + 1.2)), -1)
    parts += turret(k, mats, Vector((0, 14.0, d)), 1)
    # The tower bridge: stacked decks narrowing upwards, windows, tops.
    for i, (w, l, z) in enumerate(((5.0, 7.0, 0.0), (4.0, 5.0, 1.8), (3.2, 3.6, 3.6), (2.4, 2.6, 5.4), (1.8, 1.8, 7.2))):
        parts.append(k.box((w, l, 1.8), grey, loc=(0, -4.0 + i * 0.3, d + z + 0.9), bevel=0.1))
        parts.append(k.box((w + 0.05, 0.1, 0.4), glass, loc=(0, -4.0 + i * 0.3 - l / 2, d + z + 1.3)))
    parts.append(k.cyl(1.2, 1.0, grey, loc=(0, -3.0, d + 9.5), n=16))
    parts.append(k.box((2.8, 0.6, 0.5), dark, loc=(0, -3.0, d + 10.2)))
    parts.append(k.rod((0, -3.0, d + 10.0), (0, -3.0, d + 15.0), 0.12, dark, n=6))
    parts.append(k.rod((-2.0, -3.0, d + 13.5), (2.0, -3.0, d + 13.5), 0.08, dark, n=5))
    # One big funnel, banded in the owner's colour.
    f = k.cyl(1.4, 5.0, grey, loc=(0, 3.4, d + 3.5), n=20, rot=(-0.08, 0, 0))
    f.scale = (1.0, 1.4, 1.0)
    parts.append(f)
    for r, hgt, z, m in ((1.45, 0.9, 5.0, p["team_paint"]), (1.46, 0.4, 5.85, p["black"])):
        band = k.cyl(r, hgt, m, loc=(0, 3.4 - 0.08 * (z - 3.5), d + z), n=20, rot=(-0.08, 0, 0))
        band.scale = (1.0, 1.4, 1.0)
        parts.append(band)
    # A tripod mainmast aft of the funnel, a deckhouse, boats.
    for s in (-1, 1):
        parts.append(k.rod((s * 1.2, 8.6, d + 2.0), (0, 7.6, d + 12.0), 0.12, dark, n=6))
    parts.append(k.rod((0, 9.4, d + 2.0), (0, 7.6, d + 12.0), 0.12, dark, n=6))
    parts.append(k.box((5.0, 6.0, 1.8), grey, loc=(0, 8.0, d + 0.9), bevel=0.1))
    for s in (-1, 1):
        parts += ships.ship_boat(k, p, (s * 1.8, 6.0, d + 1.9), length=3.6)
    # Secondary guns in casemates along the sides.
    for s in (-1, 1):
        for i in range(5):
            t = 0.32 + i * 0.08
            w, _, dd = station(t)
            y = L / 2 - t * L
            parts.append(k.box((0.9, 1.2, 0.8), grey, loc=(s * (w - 0.5), y, dd + 0.4), bevel=0.1))
            parts.append(k.rod((s * (w - 0.3), y, dd + 0.5), (s * (w + 1.2), y, dd + 0.55), 0.09, dark, n=6))
    # A teak deck strip along the centre.
    parts.append(k.box((B * 0.6, L * 0.85, 0.04), deck, loc=(0, 0, d + 0.01)))
    parts += ships.bulwark(k, p, station, L, 0.35, t0=0.02, t1=0.98, thick=0.12, material=grey)
    k.bind(parts, "hull")
    parts += ships.flag(k, p, rig, Vector((0, -3.0, d + 15.0)), "flag", size=1.8)
    return parts, {"kind": "unit", "scale": 0.0115, "rig": rig}
