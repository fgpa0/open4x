"""Trebuchet: a counterweight engine, its long arm cocked with the sling
laid out; a crewman beside it."""

import math

import engines
import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("siege")
    rig.bone("frame", None, (0, 0, 0.5), (0, 1.5, 0.5), radius=0.5)
    pivot = Vector((0, 0.0, 3.1))
    rig.bone("throw", "frame", pivot, pivot + Vector((0, 2.0, -1.6)))
    parts = []
    frame = []
    for s in (1, -1):
        frame.append(engines.beam(k, p, (s * 0.65, -1.9, 0.12), (s * 0.65, 1.9, 0.12), 0.2))
        frame.append(engines.beam(k, p, (s * 0.55, -1.2, 0.15), (s * 0.45, 0.0, 3.15), 0.17))
        frame.append(engines.beam(k, p, (s * 0.55, 1.2, 0.15), (s * 0.45, 0.0, 3.15), 0.17))
        frame.append(engines.beam(k, p, (s * 0.5, -0.65, 1.4), (s * 0.5, 0.65, 1.4), 0.12))
    for y in (-1.8, 0.0, 1.8):
        frame.append(engines.beam(k, p, (-0.75, y, 0.12), (0.75, y, 0.12), 0.18))
    frame.append(k.cyl(0.08, 1.2, p["iron"], loc=pivot, rot=(0, math.pi / 2, 0), n=10))
    for s in (1, -1):
        frame.append(k.box((0.06, 1.3, 0.55), p["team_paint"], loc=(s * 0.74, 0.0, 0.5), bevel=0.01))
    k.bind(frame, "frame")
    parts += frame
    # The arm, cocked: the short end with the weight box up front, the long
    # end down behind with the sling.
    long_end = pivot + Vector((0, 2.6, -2.45))
    short_end = pivot + Vector((0, -0.85, 0.8))
    arm = [engines.beam(k, p, short_end, long_end, 0.16)]
    box = k.box((0.95, 0.85, 0.75), p["planks"], loc=short_end + Vector((0, -0.05, -0.55)), bevel=0.03)
    hanger = engines.beam(k, p, short_end, short_end + Vector((0, 0, -0.3)), 0.1)
    stones = k.ball(0.32, p["stone"], loc=short_end + Vector((0, -0.05, -0.2)), scale=(1.2, 1.1, 0.5))
    arm += [box, hanger, stones]
    sling = k.rod(long_end, long_end + Vector((0, -1.5, -0.25)), 0.02, p["rope"], n=4)
    pouch = k.ball(0.18, p["leather"], loc=long_end + Vector((0, -1.55, -0.2)), scale=(1, 1, 0.6))
    arm += [sling, pouch]
    k.bind(arm, "throw")
    parts += arm
    body, J = people.figure(k, p, rig, at=(-1.35, 1.0, 0), yaw=-0.8, arms=("hold", "hang"), stride=0.1, outfit={"sleeves": "long", "skirt": 0.7})
    parts += body
    parts += people.helmet(k, p, J, "kettle")
    return parts, {"kind": "unit", "scale": 0.12, "rig": rig}
