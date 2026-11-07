"""Ironclad: a low steam warship in grey iron plate, a tall funnel, a gun
turret fore and aft, a band in the owner's colour."""

import math

import people
import ships
from mathutils import Vector

L, B, DECK = 30.0, 7.4, 1.8


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("cannon")
    rig.bone("hull", None, (0, 0, 0.0), (0, -L / 2, 0), radius=4.0)
    iron = k.mat("iron_plate", k.srgb(84, 88, 96), 0.45, metal=0.7, pattern="streaks", scale=2.0, amount=0.25)
    dark = k.mat("dark_iron", k.srgb(50, 52, 58), 0.5, metal=0.6)
    h, station = ships.hull(k, p, L, B, DECK, bow_rise=0.3, stern_rise=0.3, keel=-1.8, fullness=0.6, transom=True, band=(0.6, 0.35), planks=iron)
    parts = [h]
    # A raised casemate amidships with gunports, a funnel and a mast.
    w, _, d = station(0.5)
    parts.append(k.box((w * 1.5, 12.0, 1.6), iron, loc=(0, 0.5, d + 0.8), bevel=0.15))
    for s in (-1, 1):
        for i in range(4):
            parts.append(k.box((0.12, 0.6, 0.5), p["black"], loc=(s * w * 0.75, -3.6 + i * 2.6, d + 0.9)))
    parts.append(k.cyl(1.0, 6.0, dark, loc=(0, 1.5, d + 4.6), n=16))
    parts.append(k.cyl(1.05, 0.5, p["team_paint"], loc=(0, 1.5, d + 6.6), n=16))
    parts.append(k.cyl(1.05, 0.3, p["black"], loc=(0, 1.5, d + 7.5), n=16))
    parts += ships.mast(k, p, (0, -3.2, d + 1.6), 9.0, 0.16)
    # Turrets fore and aft, each with a gun.
    for y, aim in ((-9.5, -1), (9.5, 1)):
        w, _, d = station((L / 2 - y) / L)
        parts.append(k.cyl(1.7, 1.2, iron, loc=(0, y, d + 0.6), n=20))
        parts.append(k.cyl(1.72, 0.12, p["black"], loc=(0, y, d + 1.22), n=20))
        gun = k.rod((0, y + aim * 1.2, d + 0.7), (0, y + aim * 4.0, d + 0.8), 0.22, dark, n=10, r2=0.17)
        parts.append(gun)
    parts += ships.bulwark(k, p, station, L, 0.4, t0=0.04, t1=0.96, thick=0.15, material=iron)
    parts.append(k.box((0.2, L * 0.9, 0.1), p["black"], loc=(0, 0, station(0.5)[2] + 0.02)))
    k.bind(parts, "hull")
    parts += ships.flag(k, p, rig, Vector((0, -3.2, station(0.5)[2] + 10.7)), "flag", size=1.8)
    return parts, {"kind": "unit", "scale": 0.016, "rig": rig}
