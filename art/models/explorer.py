"""Explorer: an expedition leader in a long coat and plumed hat, raising a
spyglass, a pack and a rolled map on the back."""

import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("melee")
    coat = k.mat("coat", k.srgb(88, 70, 52), 0.85, pattern="weave", scale=30, amount=0.25)
    parts, J = people.figure(k, p, rig, yaw=0.15, arms=("draw", "hang"), stride=0.2, outfit={"sleeves": "long", "skirt": 0.45, "tunic": coat, "beard": True})
    parts += people.helmet(k, p, J, "hat", color=k.mat("dark_felt", k.srgb(60, 48, 40), 0.9, pattern="noise", scale=10, amount=0.2))
    plume = k.blobs("plume", [("ellipsoid", J["head"] + J["up"] * 0.17 - J["fwd"] * 0.06 + J["side"] * 0.06, (0.03, 0.14, 0.05))], p["team"], res=0.012, faces=200)
    k.bind([plume], J["prefix"] + "head")
    parts.append(plume)
    # A brass spyglass from the fist towards the eye.
    wr = J["wrist_r"]
    eye = J["head"] + J["fwd"] * 0.08 + J["side"] * -0.04
    d = (wr - eye).normalized()
    glass = [k.rod(eye + d * 0.05, eye + d * 0.22, 0.022, p["gold_paint"], n=8), k.rod(eye + d * 0.22, eye + d * 0.42, 0.03, p["gold_paint"], n=8)]
    k.bind(glass, "arm_r_lo")
    parts += glass
    pack = k.box((0.3, 0.16, 0.36), p["leather"], loc=J["chest"] - J["fwd"] * 0.17 + Vector((0, 0, -0.04)), bevel=0.04)
    roll = k.cyl(0.05, 0.42, k.mat("map", k.srgb(222, 204, 160), 0.9), loc=J["chest"] - J["fwd"] * 0.2 + Vector((0, 0, 0.19)), rot=(0, 1.5708, 0), n=10)
    k.bind([pack, roll], J["prefix"] + "chest")
    parts += [pack, roll]
    parts += people.tool(k, p, J["wrist_l"] + Vector((0, 0.05, -0.85)), J["wrist_l"] + Vector((0, -0.05, 0.5)), "staff", bone="arm_l_lo")
    return parts, {"kind": "unit", "scale": 0.2, "rig": rig}
