"""Swordsman: a legionary in mail with a curved scutum and a short sword."""

import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("melee")
    parts, J = people.figure(k, p, rig, arms=("raise", "shield"), stride=0.12, outfit={"armour": "mail", "legs": "bare", "boots": True, "skirt": 0.66})
    parts += people.helmet(k, p, J, "crest")
    parts += people.blade(k, p, J["hand_r"], J["fwd"] * 0.6 + Vector((0, 0, 1)), length=0.55, width=0.06, bone="arm_r_lo")
    parts += people.shield(k, p, J["wrist_l"] + Vector((0.08, -0.08, 0.0)), (0.35, -1, 0), "scutum", 1.05, bone="arm_l_lo")
    return parts, {"kind": "unit", "scale": 0.2, "rig": rig}
