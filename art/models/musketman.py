"""Musketman: a broad hat, white cross-belts, aiming a matchlock."""

import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("gun")
    right = ((-0.32, -0.08, 1.26), (-0.15, -0.26, 1.37))
    left = ((-0.14, -0.36, 1.24), (-0.04, -0.62, 1.35))
    parts, J = people.figure(k, p, rig, yaw=0.2, arms=(right, left), stride=0.14, outfit={"sleeves": "long", "skirt": 0.6, "beard": True})
    parts += people.helmet(k, p, J, "hat")
    butt = J["shoulder_r"] + J["fwd"] * 0.02 + Vector((0.04, 0, -0.04))
    muzzle = butt + (J["wrist_l"] - butt).normalized() * 1.2
    parts += people.musket(k, p, butt, muzzle, bone="arm_r_lo")
    belts = []
    for s in (1, -1):
        b = k.box((0.05, 0.25, 0.46), p["white_paint"], loc=J["chest"] + Vector((0, 0, -0.1)), rot=(0, s * 0.55, 0))
        belts.append(b)
    k.bind(belts, "chest")
    parts += belts
    for i in range(4):
        flask = k.cyl(0.02, 0.07, p["wood"], loc=J["chest"] + Vector((-0.12 + 0.07 * i, -0.13, -0.1 - 0.03 * abs(i - 1.5))), n=6)
        k.bind(flask, "chest")
        parts.append(flask)
    return parts, {"kind": "unit", "scale": 0.2, "rig": rig}
