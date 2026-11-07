"""Longswordsman: a knight on foot in plate and a great helm, longsword up."""

import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("melee")
    arms = (((-0.26, -0.14, 1.12), (-0.05, -0.32, 1.24)), ((-0.25, -0.14, 1.1), (-0.03, -0.33, 1.14)))
    parts, J = people.figure(k, p, rig, arms=arms, stride=0.12, outfit={"armour": "plate", "sleeves": "long", "skirt": 0.62})
    parts += people.helmet(k, p, J, "great")
    surcoat = k.cyl(0.2, 0.5, p["team"], loc=J["hips"] + Vector((0, 0, 0.12)), n=18, r2=0.165)
    k.blend(surcoat, ["hips", "spine", "leg_r_up", "leg_l_up"])
    parts.append(surcoat)
    grip = (J["hand_r"] + J["hand_l"]) / 2
    parts += people.blade(k, p, grip, J["fwd"] * 0.3 + Vector((0, 0, 1)), length=1.0, width=0.06, bone="arm_r_lo")
    for n in ("r", "l"):
        g = k.cyl(0.07, 0.3, p["iron"], loc=(J["knee_" + n] + J["ankle_" + n]) / 2, n=10, r2=0.058)
        k.bind(g, "leg_" + n + "_lo")
        parts.append(g)
    return parts, {"kind": "unit", "scale": 0.2, "rig": rig}
