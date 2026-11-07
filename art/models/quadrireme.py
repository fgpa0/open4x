"""Quadrireme: a big oared warship, two banks of oars, a fighting tower and
a bolt thrower on the foredeck."""

import math

import engines
import people
import ships
from mathutils import Vector

L, B, DECK = 38.0, 7.6, 2.6


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("siege")
    rig.bone("hull", None, (0, 0, 0.0), (0, -L / 2, 0), radius=4.0)
    h, station = ships.hull(k, p, L, B, DECK, bow_rise=1.0, stern_rise=3.2, keel=-1.3, band=(1.6, 0.4))
    parts = [h]
    parts.append(k.cyl(0.6, 3.0, p["bronze"], loc=(0, -L / 2 - 0.8, 0.2), rot=(math.radians(90), 0, 0), n=12, r2=0.08))
    post = [("ball", (0, L / 2 + 0.6 + 1.6 * math.sin(i / 8 * math.radians(190)), 5.6 + 1.6 * (1 - math.cos(i / 8 * math.radians(190)))), 0.4 - 0.03 * i) for i in range(9)]
    post += [("capsule", a[1], b[1], min(a[2], b[2])) for a, b in zip(post, post[1:])]
    parts.append(k.blobs("stern_post", post, p["planks"], res=0.1, faces=1000))
    # Fighting tower amidships, painted, with crenellations.
    tower = Vector((0, 3.0, DECK))
    parts.append(k.box((3.2, 3.2, 3.6), p["planks"], loc=tower + Vector((0, 0, 1.8)), bevel=0.06))
    parts.append(k.box((3.4, 3.4, 0.6), p["team_paint"], loc=tower + Vector((0, 0, 3.3))))
    for x in (-1.3, 0, 1.3):
        for y in (-1.3, 1.3):
            parts.append(k.box((0.5, 0.5, 0.5), p["planks"], loc=tower + Vector((x, y, 3.85))))
    # A bolt thrower on the foredeck.
    fore = Vector((0, -L / 2 + 8, DECK + 0.1))
    parts.append(engines.beam(k, p, fore, fore + Vector((0, -2.8, 0.9)), 0.3))
    parts.append(k.rod(fore + Vector((-1.4, -1.4, 0.6)), fore + Vector((1.4, -1.4, 0.6)), 0.12, p["dark_wood"]))
    parts.append(k.box((0.9, 0.9, 1.1), p["rope"], loc=fore + Vector((0, -1.0, 0.45))))
    for s in (-1, 1):
        for i in range(11):
            t = 0.2 + i * 0.058
            w, _, d = station(t)
            parts.append(k.cyl(0.66, 0.08, p["team_paint"], loc=(s * (w + 0.05), L / 2 - t * L, d + 0.4), rot=(0, math.radians(90), 0), n=16))
    my, top = -3.5, DECK + 11.5
    parts += ships.mast(k, p, (0, my, DECK), top - DECK, 0.28)
    parts += ships.square_sail(k, p, (0, my - 0.6, top - 4.6), 11.0, 7.0, p["team"], bulge=1.2)
    parts.append(ships.stay(k, p, (0, my, top - 0.3), (0, -L / 2 + 2, DECK + 1.2)))
    parts.append(ships.stay(k, p, (0, my, top - 0.3), (0, L / 2 - 3, DECK + 1.2)))
    parts += ships.bulwark(k, p, station, L, 0.9, t0=0.05, t1=0.9)
    parts += awning(k, p, (0, L / 2 - 5.5, station(0.15)[2]), 3.2, 3.6, 2.2)
    for x, y in [(1.2, -2.0), (-1.2, -2.3), (1.2, -1.2), (-1.0, 8.0), (1.0, 8.4), (0.0, 9.0)]:
        parts += amphora(k, p, (x, y, DECK + 0.25))
    parts += ships.coil(k, p, (1.4, -10.0, DECK + 0.25), 0.5)
    parts += ships.coil(k, p, (-1.4, 12.0, DECK + 0.25), 0.45)
    parts += ships.hatch(k, p, (0, -6.0, DECK + 0.1), 1.6, 1.4)
    k.bind(parts, "hull")
    ts = [0.22 + i * 0.075 for i in range(8)]
    parts += ships.oars(k, p, rig, station, L, ts, z_off=-0.4, reach=4.6, name="oar")
    parts += ships.oars(k, p, rig, station, L, [t + 0.035 for t in ts], z_off=-1.1, reach=3.6, name="oarb")
    parts += ships.flag(k, p, rig, (0, my, top + 0.2), "flag", size=2.0)
    return parts, {"kind": "unit", "scale": 0.0128, "rig": rig}


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
