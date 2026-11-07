"""Catapult: a torsion engine on wheels, its arm drawn back with a stone in
the cup; a crewman beside it."""

import math

import engines
import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("siege")
    rig.bone("frame", None, (0, 0, 0.6), (0, 1.0, 0.6), radius=0.4)
    for y in (-0.75, 0.85):
        for s, n in ((1, "l"), (-1, "r")):
            rig.bone(f"wheel_{n}{int((y + 1) * 10)}", "frame", (s * 0.72, y, 0.38))
    pivot = Vector((0, 0.35, 0.95))
    rig.bone("throw", "frame", pivot, pivot + Vector((0, -1.4, 0.4)))
    parts = []
    frame = []
    for s in (1, -1):
        frame.append(engines.beam(k, p, (s * 0.55, -1.0, 0.45), (s * 0.55, 1.1, 0.45), 0.16))
        frame.append(engines.beam(k, p, (s * 0.42, 0.2, 0.45), (s * 0.42, 0.45, 1.25), 0.13))
        frame.append(engines.beam(k, p, (s * 0.42, -0.35, 0.45), (s * 0.42, 0.25, 1.25), 0.11))
    for y in (-0.95, 0.0, 1.05):
        frame.append(engines.beam(k, p, (-0.62, y, 0.45), (0.62, y, 0.45), 0.13))
    frame.append(engines.beam(k, p, (-0.5, 0.0, 1.25), (0.5, 0.0, 1.25), 0.14, p["team_paint"]))
    # The torsion skein: twisted rope between the side frames.
    frame.append(k.cyl(0.13, 0.84, p["rope"], loc=pivot, rot=(0, math.pi / 2, 0), n=12))
    for s in (1, -1):
        frame.append(k.cyl(0.17, 0.08, p["iron"], loc=pivot + Vector((s * 0.44, 0, 0)), rot=(0, math.pi / 2, 0), n=12))
        frame.append(k.box((0.05, 0.9, 0.35), p["team_paint"], loc=(s * 0.64, 0.05, 0.62), bevel=0.01))
    frame.append(engines.beam(k, p, (0, 0.95, 0.45), (0, 0.6, 0.85), 0.1))
    k.bind(frame, "frame")
    parts += frame
    # The arm, cocked back, with a cup and stone.
    tip = pivot + Vector((0, 1.15, -0.35))
    arm = [engines.beam(k, p, pivot - (tip - pivot).normalized() * 0.12, tip, 0.1)]
    arm.append(k.ball(0.17, p["dark_wood"], loc=tip + Vector((0, 0.05, 0.08)), scale=(1, 1, 0.55)))
    arm.append(k.ball(0.14, p["stone"], loc=tip + Vector((0, 0.05, 0.18))))
    k.bind(arm, "throw")
    parts += arm
    for y in (-0.75, 0.85):
        for s, n in ((1, "l"), (-1, "r")):
            parts += engines.wheel(k, p, (s * 0.72, y, 0.38), 0.38, f"wheel_{n}{int((y + 1) * 10)}", spokes=6)
    body, J = people.figure(k, p, rig, at=(-1.15, -0.4, 0), yaw=-0.5, arms=("hang", "hang"), stride=0.1, outfit={"sleeves": "long", "skirt": 0.7, "beard": True})
    parts += body
    parts += people.helmet(k, p, J, "cap")
    return parts, {"kind": "unit", "scale": 0.17, "rig": rig}
