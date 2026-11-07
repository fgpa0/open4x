"""Archer: a bowman drawing a longbow, quiver on the back."""

import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("bow")
    parts, J = people.figure(k, p, rig, yaw=0.25, arms=("draw", "reach"), stride=0.16, outfit={"sleeves": "long", "skirt": 0.7, "hair": p["hair"]})
    parts += people.helmet(k, p, J, "hood", color=k.mat("green_wool", k.srgb(86, 104, 62), 0.9, pattern="weave", scale=40, amount=0.2))
    grip = J["hand_l"]
    parts += people.bow(k, p, grip, (0.0, 0.05, 1.0), J["fwd"], height=1.45, bone="arm_l_lo")
    arrow = k.rod(J["hand_r"], grip + J["fwd"] * 0.12, 0.007, p["wood"], n=4)
    k.bind(arrow, "arm_r_lo")
    parts.append(arrow)
    parts += people.quiver(k, p, J["chest"] + J["fwd"] * -0.16 + Vector((0.08, 0, 0.05)), "chest")
    return parts, {"kind": "unit", "scale": 0.2, "rig": rig}
