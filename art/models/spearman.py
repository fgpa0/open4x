"""Spearman: a hoplite with a crested bronze helmet, round shield and spear."""

import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("thrust")
    linen = k.mat("linothorax", k.srgb(226, 214, 186), 0.85, pattern="weave", scale=40, amount=0.2)
    parts, J = people.figure(k, p, rig, arms=("hold", "shield"), stride=0.1, outfit={"armour": "leather", "legs": "bare", "boots": False, "skirt": 0.68})
    torso = k.cyl(0.19, 0.42, linen, loc=J["chest"] - Vector((0, 0, 0.12)), n=18, r2=0.17)
    k.bind(torso, "spine")
    parts.append(torso)
    for side, n in ((1, "l"), (-1, "r")):
        greave = k.cyl(0.062, 0.32, p["bronze"], loc=(J["knee_" + n] + J["ankle_" + n]) / 2 + Vector((0, -0.01, 0.02)), n=10, r2=0.05)
        k.bind(greave, "leg_" + n + "_lo")
        parts.append(greave)
    parts += people.helmet(k, p, J, "crest")
    wr = J["wrist_r"]
    parts += people.spear(k, p, wr + Vector((0, 0.08, -0.95)), wr + Vector((0, -0.12, 1.45)), bone="arm_r_lo")
    parts += people.shield(k, p, J["wrist_l"] + Vector((0.07, -0.05, 0.02)), (0.5, -1, 0.05), "round", 1.05, bone="arm_l_lo")
    return parts, {"kind": "unit", "scale": 0.2, "rig": rig}
