"""Composite Bowman: a lamellar-armoured archer with a recurve bow."""

import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("bow")
    parts, J = people.figure(k, p, rig, yaw=0.25, arms=("draw", "reach"), stride=0.14, outfit={"armour": "lamellar", "sleeves": "long", "skirt": 0.55, "beard": True})
    parts += people.helmet(k, p, J, "cone", color=p["bronze"])
    parts += people.bow(k, p, J["hand_l"], (0.0, 0.05, 1.0), J["fwd"], height=1.1, recurve=True, bone="arm_l_lo")
    arrow = k.rod(J["hand_r"], J["hand_l"] + J["fwd"] * 0.12, 0.007, p["wood"], n=4)
    k.bind(arrow, "arm_r_lo")
    parts.append(arrow)
    case = k.box((0.12, 0.08, 0.45), p["leather"], loc=J["hips"] + Vector((0.2, 0.05, -0.12)), rot=(0.2, 0.3, 0), bevel=0.02)
    k.bind(case, "hips")
    parts.append(case)
    parts += people.quiver(k, p, J["hips"] + Vector((-0.2, 0.05, -0.05)), "hips")
    return parts, {"kind": "unit", "scale": 0.2, "rig": rig}
