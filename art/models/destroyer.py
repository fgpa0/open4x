"""Destroyer: a long, narrow grey hull with a raked bow, a bridge with
windows, two raked funnels with bands in the owner's colour, single guns
fore and aft, torpedo tubes amidships and a lattice mast."""

import math

import people
import ships
from mathutils import Vector

L, B, DECK = 36.0, 4.4, 1.9


def gun(k, mats, at, aim, size=1.0):
    grey, dark = mats
    out = [k.box((1.3 * size, 1.5 * size, 0.9 * size), grey, loc=at + Vector((0, 0, 0.45 * size)), bevel=0.2 * size)]
    out.append(k.rod(at + Vector((0, aim * 0.6 * size, 0.55 * size)), at + Vector((0, aim * 3.0 * size, 0.6 * size)), 0.12 * size, dark, n=8, r2=0.1 * size))
    return out


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("cannon")
    rig.bone("hull", None, (0, 0, 0.0), (0, -L / 2, 0), radius=3.0)
    grey = k.mat("navy_grey", k.srgb(128, 134, 140), 0.55, metal=0.3, pattern="streaks", scale=2.0, amount=0.15)
    dark = k.mat("gun_grey", k.srgb(84, 88, 94), 0.5, metal=0.5)
    glass = k.mat("bridge_glass", k.srgb(40, 50, 60), 0.15, metal=0.4)
    h, station = ships.hull(k, p, L, B, DECK, bow_rise=1.0, stern_rise=0.0, keel=-1.6, fullness=0.45, transom=True, band=(0.9, 0.35), planks=grey)
    parts = [h]
    _, _, d = station(0.5)
    # A raised forecastle, the bridge and a mast behind it.
    parts.append(k.box((B * 0.8, 12.0, 0.9), grey, loc=(0, -6.5, d + 0.45), bevel=0.1))
    parts.append(k.box((2.8, 3.0, 2.2), grey, loc=(0, -4.0, d + 2.0), bevel=0.12))
    parts.append(k.box((2.85, 0.1, 0.5), glass, loc=(0, -5.5, d + 2.6)))
    parts.append(k.box((3.2, 2.0, 0.15), grey, loc=(0, -4.2, d + 3.15)))
    parts.append(k.rod((0, -3.0, d + 3.0), (0, -2.5, d + 11.0), 0.12, dark, n=6))
    parts.append(k.rod((-1.6, -2.75, d + 7.5), (1.6, -2.75, d + 7.5), 0.07, dark, n=5))
    for s in (-1, 1):
        parts.append(k.rod((0, -3.6, d + 3.1), (s * 0.8, -3.0, d + 7.0), 0.06, dark, n=5))
    # Two raked funnels.
    for y in (0.5, 4.5):
        f = k.cyl(0.9, 4.0, grey, loc=(0, y, d + 2.8), n=18, rot=(-0.12, 0, 0))
        f.scale = (1.0, 1.35, 1.0)
        parts.append(f)
        for r, hgt, z, m in ((0.95, 0.6, 3.9, p["team_paint"]), (0.96, 0.3, 4.65, p["black"])):
            band = k.cyl(r, hgt, m, loc=(0, y - 0.12 * (z - 2.8), d + z), n=18, rot=(-0.12, 0, 0))
            band.scale = (1.0, 1.35, 1.0)
            parts.append(band)
    # Torpedo tubes on the centreline, a deckhouse aft.
    for y in (7.5, 10.0):
        parts.append(k.cyl(0.5, 0.3, dark, loc=(0, y, d + 0.2), n=12))
        for s in (-0.25, 0.25):
            parts.append(k.rod((s, y - 1.6, d + 0.55), (s, y + 1.6, d + 0.55), 0.18, dark, n=8))
    parts.append(k.box((2.4, 4.0, 1.4), grey, loc=(0, 13.0, d + 0.7), bevel=0.1))
    # Guns fore and aft.
    parts += gun(k, (grey, dark), Vector((0, -11.0, d + 0.9)), -1)
    parts += gun(k, (grey, dark), Vector((0, -8.0, d + 0.9)), -1, 0.85)
    parts += gun(k, (grey, dark), Vector((0, 15.5, d)), 1)
    parts += ships.bulwark(k, p, station, L, 0.3, t0=0.02, t1=0.98, thick=0.1, material=grey)
    # The owner's number on the bow.
    for s in (-1, 1):
        w, _, dd = station(0.86)
        parts.append(k.box((0.06, 1.6, 0.9), p["team_paint"], loc=(s * w * 1.0, L / 2 - 0.86 * L, dd - 0.6)))
    k.bind(parts, "hull")
    parts += ships.flag(k, p, rig, Vector((0, -2.5, d + 11.0)), "flag", size=1.4)
    return parts, {"kind": "unit", "scale": 0.0145, "rig": rig}
