"""Galley: an ancient oared warship with a ram and one square sail."""

import math

import people
import ships
from mathutils import Vector

L, B, DECK = 28.0, 5.8, 1.9


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("melee")
    rig.bone("hull", None, (0, 0, 0.0), (0, -L / 2, 0), radius=3.0)
    h, station = ships.hull(k, p, L, B, DECK, bow_rise=0.7, stern_rise=2.6, keel=-0.9)
    parts = [h]
    parts.append(k.cyl(0.42, 2.4, p["bronze"], loc=(0, -L / 2 - 0.6, 0.15), rot=(math.radians(90), 0, 0), n=12, r2=0.05))
    for s in (-1, 1):
        w, _, d = station(0.9)
        parts.append(k.ball(0.32, p["white"], loc=(s * (w + 0.02), L / 2 - 0.9 * L, d - 0.6), scale=(0.25, 1, 0.7)))
        parts.append(k.ball(0.17, p["black"], loc=(s * (w + 0.1), L / 2 - 0.9 * L - 0.05, d - 0.6), scale=(0.3, 1, 1)))
    post = [("ball", (0, L / 2 + 0.6 + 1.3 * math.sin(i / 8 * math.radians(200)), 4.4 + 1.3 * (1 - math.cos(i / 8 * math.radians(200)))), 0.3 - 0.022 * i) for i in range(9)]
    post += [("capsule", a[1], b[1], min(a[2], b[2])) for a, b in zip(post, post[1:])]
    parts.append(k.blobs("stern_post", post, p["planks"], res=0.08, faces=1000))
    for s in (-1, 1):
        for i in range(9):
            t = 0.22 + i * 0.07
            w, _, d = station(t)
            parts += [k.cyl(0.58, 0.08, p["team_paint"], loc=(s * (w + 0.05), L / 2 - t * L, d + 0.35), rot=(0, math.radians(90), 0), n=16)]
    for s in (-1, 1):
        w, _, d = station(0.1)
        parts.append(k.rod((s * w, L / 2 - 0.1 * L, d + 0.8), (s * (w + 0.9), L / 2 + 1.2, -0.3), 0.12, p["dark_wood"]))
    parts.append(k.box((1.1, L * 0.62, 0.25), p["dark_wood"], loc=(0, 0.6, DECK + 0.12), bevel=0.05))
    my, top = -1.4, DECK + 10.5
    parts += ships.mast(k, p, (0, my, DECK), top - DECK)
    parts += ships.square_sail(k, p, (0, my - 0.5, top - 4.3), 10.0, 6.4, p["team"], bulge=1.1)
    parts.append(ships.stay(k, p, (0, my, top - 0.3), (0, -L / 2 + 1.5, DECK + 0.9)))
    parts.append(ships.stay(k, p, (0, my, top - 0.3), (0, L / 2 - 2.5, DECK + 0.9)))
    parts += ships.bulwark(k, p, station, L, 0.7, t0=0.05, t1=0.9)
    parts += awning(k, p, (0, L / 2 - 4.2, station(0.15)[2]), 2.6, 3.0, 2.0)
    parts.append(k.box((2.2, 0.6, 0.5), p["dark_wood"], loc=(0, L / 2 - 5.8, station(0.2)[2] + 0.25)))
    for i, (x, y) in enumerate([(0.9, 2.5), (-0.9, 2.8), (0.9, 3.4), (-0.8, -4.5), (0.8, -4.8)]):
        parts += amphora(k, p, (x, y, DECK + 0.25))
    parts += ships.coil(k, p, (0.9, -7.0, DECK + 0.25), 0.45)
    parts += ships.coil(k, p, (-0.9, 6.0, DECK + 0.25), 0.4)
    k.bind(parts, "hull")
    parts += ships.oars(k, p, rig, station, L, [0.24 + i * 0.068 for i in range(9)])
    parts += ships.flag(k, p, rig, (0, my, top + 0.2), "flag")
    return parts, {"kind": "unit", "scale": 0.0145, "rig": rig}


def amphora(k, p, at):
    clay = k.mat("clay", k.srgb(176, 104, 66), 0.8, pattern="noise", scale=6, amount=0.2)
    at = Vector(at)
    return [k.ball(0.3, clay, loc=at + Vector((0, 0, 0.45)), scale=(1, 1, 1.5)), k.cyl(0.1, 0.3, clay, loc=at + Vector((0, 0, 1.0)), n=8)]


def awning(k, p, centre, w, d, h):
    """A cloth awning in the owner's colour on four posts."""
    centre = Vector(centre)
    out = [k.rod(centre + Vector((x, y, 0)), centre + Vector((x, y, h)), 0.07, p["dark_wood"], n=5) for x in (-w / 2, w / 2) for y in (-d / 2, d / 2)]
    cloth = k.sheet("awning", w + 0.4, d + 0.4, p["team"], nx=6, ny=6, bulge=0.0, thickness=0.05, sag=0.0)
    cloth.rotation_euler = (math.pi / 2, 0, 0)
    cloth.location = centre + Vector((0, 0, h))
    out.append(cloth)
    return out
