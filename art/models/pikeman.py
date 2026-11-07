"""Pikeman: a morion and breastplate, and a very long pike held upright."""

import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("thrust")
    parts, J = people.figure(k, p, rig, arms=("hold", "hang"), stride=0.1, outfit={"armour": "plate", "sleeves": "long", "skirt": 0.72, "beard": True})
    parts += people.helmet(k, p, J, "morion")
    wr = J["wrist_r"]
    parts += people.spear(k, p, wr + Vector((0, 0.05, -1.0)), wr + Vector((0, -0.1, 2.6)), r=0.018, head="pike", bone="arm_r_lo")
    sash = k.box((0.07, 0.3, 0.55), p["team_paint"], loc=J["chest"] + Vector((0, -0.02, -0.05)), rot=(0, 0.6, 0))
    k.bind(sash, "chest")
    parts.append(sash)
    parts += people.blade(k, p, J["hips"] + Vector((0.19, 0.05, -0.08)), Vector((0.1, 0.6, -1)), length=0.6, bone="hips")
    return parts, {"kind": "unit", "scale": 0.2, "rig": rig}
