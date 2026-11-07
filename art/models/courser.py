"""Courser: light cavalry on a lean chestnut horse, a javelin raised, a
quilted coat and a kettle hat."""

import beasts
import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("thrust")
    coat = k.mat("chestnut", k.srgb(150, 84, 44), 0.75, pattern="noise", scale=5, amount=0.35, color2=k.srgb(110, 58, 30))
    p["mane"] = k.mat("flaxen", k.srgb(196, 160, 110), 0.7, pattern="hair", scale=10, amount=0.3)
    parts, A = beasts.quadruped(k, p, "horse", coat, rig=rig, points=k.mat("chestnut_points", k.srgb(70, 40, 24), 0.7), prefix="horse_", extra={"lh": 1.02, "br": 0.3, "bw": 0.26})
    parts += beasts.tack(k, p, A, prefix="horse_")
    body, J = people.figure(k, p, rig, parent="horse_body", mounted=A["seat"] + Vector((0, 0, 0.06)), arms=("raise", "hold"), outfit={"armour": "leather", "sleeves": "long", "skirt": 0.6})
    parts += body
    parts += people.helmet(k, p, J, "kettle")
    wr = J["wrist_r"]
    parts += people.spear(k, p, wr + Vector((0, 0.55, -0.2)), wr + Vector((0, -0.75, 0.4)), r=0.013, bone="arm_r_lo")
    return parts, {"kind": "unit", "scale": 0.2, "rig": rig}
