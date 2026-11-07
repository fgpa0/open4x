"""Caravel: a small ocean-going ship with a high sterncastle and lateen
sails in the owner's colour."""

import math

import people
import ships
from mathutils import Vector

L, B, DECK = 22.0, 6.6, 2.4


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("none")
    rig.bone("hull", None, (0, 0, 0.0), (0, -L / 2, 0), radius=3.0)
    h, station = ships.hull(k, p, L, B, DECK, bow_rise=1.2, stern_rise=1.2, keel=-1.6, fullness=0.42, transom=True, band=(0.7, 0.25))
    parts = [h]
    # Sterncastle and a small forecastle.
    w, _, d = station(0.08)
    parts.append(k.box((w * 2 * 0.95, 5.0, 2.2), p["planks"], loc=(0, L / 2 - 2.6, d + 1.0), bevel=0.08))
    parts.append(k.box((w * 2, 5.2, 0.35), p["team_paint"], loc=(0, L / 2 - 2.6, d + 2.2)))
    for s in (-1, 1):
        for i in range(4):
            parts.append(k.box((0.12, 0.55, 0.7), p["black"], loc=(s * w * 0.96, L / 2 - 1.0 - i * 1.2, d + 1.0)))
    # A forecastle: a raised deck following the bow's shape, railed.
    fore = []
    for j in range(9):
        t = 0.8 + 0.17 * j / 8
        w, _, d = station(t)
        y = L / 2 - t * L
        wi = w * 0.95
        fore.append([(-wi, y, d - 0.2), (-wi, y, d + 1.0), (wi, y, d + 1.0), (wi, y, d - 0.2)])
    parts.append(k.loft("forecastle", fore, p["planks"], closed=True, cap=True, smooth=0.5))
    rail = []
    for s_ in (-1, 1):
        rail = [[(s_ * station(0.8 + 0.17 * j / 8)[0] * 0.97, L / 2 - (0.8 + 0.17 * j / 8) * L, station(0.8 + 0.17 * j / 8)[2] + z) for z in (1.0, 1.55)] for j in range(9)]
        rings = [[(x, y, z0), (x, y, z1), (x - s_ * 0.12, y, z1), (x - s_ * 0.12, y, z0)] for (x, y, z0), (_, _, z1) in rail]
        parts.append(k.loft("fore_rail", rings, p["dark_wood"], closed=True, cap=True, smooth=0.5))
    parts.append(k.box((B * 0.75, 0.15, 0.6), p["dark_wood"], loc=(0, L / 2 - 0.8 * L + 0.05, station(0.8)[2] + 1.3)))
    parts.append(k.rod((0, -L / 2 + 1.5, station(0.95)[2] + 0.3), (0, -L / 2 - 3.5, station(0.95)[2] + 2.2), 0.16, p["dark_wood"]))
    tops = []
    for y, hgt, ln in ((-2.6, 14.0, 13.0), (4.4, 11.0, 10.0)):
        parts += ships.mast(k, p, (0, y, DECK), hgt + 1.0, 0.22)
        parts += ships.lateen(k, p, (0.4, y, DECK + 1.6), hgt, ln, p["team"])
        tops.append(Vector((0, y, DECK + hgt + 1.0)))
    parts += ships.bulwark(k, p, station, L, 0.8, t0=0.22, t1=0.95)
    parts += ships.hatch(k, p, (0, 1.0, DECK), 1.8, 1.6)
    parts += ships.capstan(k, p, (0, -6.5, DECK))
    for x, y in [(1.6, -1.2), (1.6, -0.2), (-1.7, 6.0)]:
        parts += ships.barrel(k, p, (x, y, DECK))
    parts += ships.crate(k, p, (-1.6, -1.0, DECK), 0.9, 0.3)
    parts += ships.coil(k, p, (-1.6, 3.2, DECK), 0.45)
    parts += ships.ship_boat(k, p, (1.1, 4.6, DECK + 0.1), 3.2)
    # Rails around the sterncastle roof.
    w, _, d = station(0.08)
    for s_ in (-1, 1):
        parts.append(k.box((0.15, 5.2, 0.6), p["dark_wood"], loc=(s_ * w * 0.95, L / 2 - 2.6, d + 2.65)))
    parts.append(k.box((w * 1.9, 0.15, 0.6), p["dark_wood"], loc=(0, L / 2 - 0.1, d + 2.65)))
    for t in tops:
        parts.append(ships.stay(k, p, t, (0, -L / 2 + 0.5, station(0.95)[2])))
        parts.append(ships.stay(k, p, t, (0, L / 2 - 0.5, station(0.05)[2] + 2.2)))
    k.bind(parts, "hull")
    parts += ships.flag(k, p, rig, tops[0] + Vector((0, 0, 0.2)), "flag", size=1.6)
    return parts, {"kind": "unit", "scale": 0.021, "rig": rig}
