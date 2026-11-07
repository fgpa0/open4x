"""Ship of the Line: a towering three-masted warship with two gun decks
behind bands in the owner's colour and a carved stern."""

import math

import people
import ships
from mathutils import Vector

L, B, DECK = 46.0, 12.0, 4.6


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("cannon")
    rig.bone("hull", None, (0, 0, 0.0), (0, -L / 2, 0), radius=6.0)
    planks = k.mat("black_planks", k.srgb(60, 48, 40), 0.75, pattern="grain", scale=1.4, amount=0.35)
    h, station = ships.hull(k, p, L, B, DECK, bow_rise=1.4, stern_rise=1.8, keel=-2.8, fullness=0.45, transom=True, band=(3.4, 0.8), planks=planks)
    parts = [h]
    # Two decks of gunports with a second band between them.
    for deck, drop in ((0, 1.2), (1, 2.9)):
        for s in (-1, 1):
            for i in range(12):
                t = 0.13 + i * 0.065
                w, _, d = station(t)
                y = L / 2 - t * L
                parts.append(k.box((0.25, 1.0, 0.85), p["black"], loc=(s * w * 0.99, y, d - drop)))
                parts.append(k.cyl(0.22, 0.8, p["iron"], loc=(s * (w + 0.2), y, d - drop), rot=(0, math.pi / 2, 0), n=8))
    w, _, d = station(0.08)
    parts.append(k.box((w * 2 * 0.96, 11.0, 2.0), planks, loc=(0, L / 2 - 5.5, d + 0.8), bevel=0.08))
    parts.append(k.box((w * 1.7, 0.7, 3.4), p["gold_paint"], loc=(0, L / 2 + 0.2, d + 0.4)))
    for row in range(2):
        for i in range(5):
            parts.append(k.box((0.8, 0.3, 0.9), p["black"], loc=(-w * 0.6 + i * w * 0.3, L / 2 + 0.6, d - 0.4 + row * 1.5)))
    parts.append(k.rod((0, -L / 2 + 2.5, station(0.95)[2] + 0.4), (0, -L / 2 - 9.0, station(0.95)[2] + 5.0), 0.34, p["dark_wood"], r2=0.16))
    tops = []
    for y, hgt, wdt in ((-13.0, 30.0, 15.0), (0.5, 34.0, 17.0), (13.0, 26.0, 12.0)):
        parts += ships.mast(k, p, (0, y, DECK), hgt, 0.45)
        z = DECK + 5.0
        for tier, frac in enumerate((0.42, 0.32, 0.22)):
            sh = hgt * frac * 0.85
            sw = wdt * (1.0 - tier * 0.18)
            parts += ships.square_sail(k, p, (0, y - 0.6, z + sh / 2), sw, sh, p["team"], bulge=1.0 - tier * 0.2)
            z += sh + 0.5
        parts.append(k.cyl(1.4, 0.3, planks, loc=(0, y, DECK + hgt * 0.45), n=10))
        tops.append(Vector((0, y, DECK + hgt)))
    for a, b in zip(tops, tops[1:]):
        parts.append(ships.stay(k, p, a, b + Vector((0, 0, -6)), 0.07))
    parts.append(ships.stay(k, p, tops[0], (0, -L / 2 - 8.5, station(0.95)[2] + 4.8), 0.07))
    for t in tops:
        for s in (-1, 1):
            w, _, d = station((L / 2 - t.y) / L)
            parts.append(ships.stay(k, p, t + Vector((0, 0, -3)), (s * w, t.y + 1.0, d), 0.05))
    parts += ships.bulwark(k, p, station, L, 1.2, t0=0.2, t1=0.95, thick=0.3, material=planks)
    for y in (-7.0, 6.0):
        parts += ships.hatch(k, p, (0, y, DECK), 2.6, 2.2)
    parts += ships.capstan(k, p, (0, 10.0, DECK))
    parts += ships.ship_boat(k, p, (0, -3.5, DECK + 0.4), 5.5)
    k.bind(parts, "hull")
    parts += ships.flag(k, p, rig, tops[1] + Vector((0, 0, 0.3)), "flag", size=2.8)
    parts += ships.flag(k, p, rig, (0, L / 2 + 0.9, d + 6.4), "flag_stern", size=2.6)
    pole = k.rod((0, L / 2 + 0.7, d + 1.2), (0, L / 2 + 0.9, d + 6.6), 0.08, p["dark_wood"])
    k.bind(pole, "hull")
    parts.append(pole)
    return parts, {"kind": "unit", "scale": 0.0115, "rig": rig}
