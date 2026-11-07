"""Galleon: a tall, high-castled ocean warship with square sails in the
owner's colour on two masts and a lateen mizzen, a row of guns along the
waist."""

import math

import people
import ships
from mathutils import Vector

L, B, DECK = 32.0, 8.6, 3.0


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("none")
    rig.bone("hull", None, (0, 0, 0.0), (0, -L / 2, 0), radius=4.0)
    h, station = ships.hull(k, p, L, B, DECK, bow_rise=1.6, stern_rise=1.8, keel=-2.0, fullness=0.44, transom=True, band=(1.2, 0.5))
    parts = [h]
    # A tall sterncastle in two tiers, and a forecastle.
    w, _, d = station(0.08)
    parts.append(k.box((w * 2 * 0.96, 8.0, 2.4), p["planks"], loc=(0, L / 2 - 4.0, d + 1.1), bevel=0.08))
    parts.append(k.box((w * 2 * 0.86, 4.6, 1.8), p["planks"], loc=(0, L / 2 - 2.4, d + 3.2), bevel=0.08))
    parts.append(k.box((w * 2 * 0.88, 4.8, 0.3), p["team_paint"], loc=(0, L / 2 - 2.4, d + 4.15)))
    parts.append(k.box((w * 2, 8.2, 0.3), p["team_paint"], loc=(0, L / 2 - 4.0, d + 2.35)))
    for i in range(4):
        parts.append(k.box((0.7, 0.3, 0.9), p["black"], loc=(-w * 0.55 + i * w * 0.37, L / 2 + 0.05, d + 1.2)))
    w, _, d = station(0.88)
    parts.append(k.box((w * 2 * 0.9, 4.6, 1.8), p["planks"], loc=(0, L / 2 - 0.88 * L, d + 0.8), bevel=0.08))
    parts.append(k.rod((0, -L / 2 + 2.0, station(0.95)[2] + 0.6), (0, -L / 2 - 6.0, station(0.95)[2] + 3.6), 0.24, p["dark_wood"], r2=0.12))
    # Gun ports along the waist.
    for s in (-1, 1):
        for i in range(6):
            t = 0.32 + i * 0.07
            w, _, d = station(t)
            y = L / 2 - t * L
            parts.append(k.box((0.22, 0.8, 0.7), p["black"], loc=(s * w * 0.99, y, d - 0.9)))
            parts.append(k.cyl(0.18, 0.6, p["iron"], loc=(s * (w + 0.15), y, d - 0.9), rot=(0, math.pi / 2, 0), n=8))
    tops = []
    for y, hgt, wdt in ((-8.5, 21.0, 11.0), (1.0, 24.0, 12.5)):
        parts += ships.mast(k, p, (0, y, DECK), hgt, 0.34)
        z = DECK + 4.0
        for tier, frac in enumerate((0.46, 0.34)):
            sh = hgt * frac * 0.85
            sw = wdt * (1.0 - tier * 0.2)
            parts += ships.square_sail(k, p, (0, y - 0.5, z + sh / 2), sw, sh, p["team"], bulge=1.0 - tier * 0.2)
            z += sh + 0.5
        parts.append(k.cyl(1.0, 0.3, p["planks"], loc=(0, y, DECK + hgt * 0.5), n=10))
        tops.append(Vector((0, y, DECK + hgt)))
    # Lateen mizzen over the castle.
    parts += ships.mast(k, p, (0, 10.5, DECK + 2.4), 15.0, 0.22)
    parts += ships.lateen(k, p, (0.3, 10.5, DECK + 4.6), 11.0, 9.0, p["team"])
    tops.append(Vector((0, 10.5, DECK + 17.4)))
    for a, b in zip(tops, tops[1:]):
        parts.append(ships.stay(k, p, a, b + Vector((0, 0, -5)), 0.06))
    parts.append(ships.stay(k, p, tops[0], (0, -L / 2 - 5.5, station(0.95)[2] + 3.4), 0.06))
    for t in tops[:2]:
        for s in (-1, 1):
            w, _, d = station((L / 2 - t.y) / L)
            parts.append(ships.stay(k, p, t + Vector((0, 0, -3)), (s * w, t.y + 1.0, d), 0.05))
    parts += ships.bulwark(k, p, station, L, 1.0, t0=0.24, t1=0.84, thick=0.25)
    parts += ships.hatch(k, p, (0, -3.5, DECK), 2.2, 1.8)
    parts += ships.capstan(k, p, (0, 5.0, DECK))
    parts += ships.ship_boat(k, p, (0, -1.8, DECK + 0.3), 4.2)
    for s_ in (-1, 1):
        parts += ships.barrel(k, p, (s_ * 2.4, -9.5, DECK))
        parts += ships.coil(k, p, (s_ * 2.6, 2.6, DECK), 0.5)
    k.bind(parts, "hull")
    parts += ships.flag(k, p, rig, tops[1] + Vector((0, 0, 0.3)), "flag", size=2.4)
    w, _, d = station(0.08)
    parts += ships.flag(k, p, rig, (0, L / 2 + 0.6, d + 8.6), "flag_stern", size=2.2)
    pole = k.rod((0, L / 2 + 0.4, d + 4.2), (0, L / 2 + 0.6, d + 8.8), 0.08, p["dark_wood"])
    k.bind(pole, "hull")
    parts.append(pole)
    return parts, {"kind": "unit", "scale": 0.015, "rig": rig}
