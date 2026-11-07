"""Scout: a lightly dressed explorer with a walking staff and a satchel."""

import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("melee")
    parts, J = people.figure(k, p, rig, arms=("hold", "hang"), stride=0.2, outfit={"sleeves": "long", "skirt": 0.75, "beard": True})
    parts += people.helmet(k, p, J, "hat", color=k.mat("brown_felt", k.srgb(110, 84, 56), 0.9, pattern="noise", scale=10, amount=0.2))
    wr = J["wrist_r"]
    parts += people.tool(k, p, wr + Vector((0, 0.06, -1.0)), wr + Vector((0, -0.1, 0.55)), "staff", bone="arm_r_lo")
    bag = k.box((0.08, 0.2, 0.18), p["leather"], loc=J["hips"] + Vector((0.2, 0.0, 0.0)), bevel=0.03)
    strap = k.rod(J["shoulder_r"] + Vector((0, 0, 0.03)), J["hips"] + Vector((0.2, 0, 0.08)), 0.015, p["leather"])
    roll = k.cyl(0.07, 0.36, k.mat("blanket", k.srgb(150, 60, 48), 0.9, pattern="weave", scale=30, amount=0.3), loc=J["chest"] + Vector((0, 0.15, 0.12)), rot=(0, 1.5708, 0), n=10)
    k.bind([bag], "hips")
    k.bind([strap, roll], "chest")
    parts += [bag, strap, roll]
    return parts, {"kind": "unit", "scale": 0.2, "rig": rig}
