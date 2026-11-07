"""Horseman: a light cavalryman with spear and round shield on a bay horse."""

import beasts
import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("thrust")
    coat = k.mat("bay", k.srgb(128, 76, 42), 0.78, pattern="noise", scale=5, amount=0.35, color2=k.srgb(84, 46, 26))
    dark = k.mat("points", k.srgb(46, 34, 26), 0.7, pattern="hair", scale=10, amount=0.3)
    p["mane"] = dark
    parts, A = beasts.quadruped(k, p, "horse", coat, rig=rig, points=dark, prefix="horse_")
    parts += beasts.tack(k, p, A, prefix="horse_")
    body, J = people.figure(k, p, rig, parent="horse_body", mounted=A["seat"] + Vector((0, 0, 0.06)), arms=("hold", "shield"), outfit={"sleeves": "short", "skirt": 0.6, "beard": True})
    parts += body
    parts += people.helmet(k, p, J, "cone")
    wr = J["wrist_r"]
    parts += people.spear(k, p, wr + Vector((0, 0.3, -0.85)), wr + Vector((0, -0.5, 1.55)), bone="arm_r_lo")
    parts += people.shield(k, p, J["wrist_l"] + Vector((0.1, 0.06, 0.05)), (1, -0.25, 0.1), "round", 0.85, bone="arm_l_lo")
    return parts, {"kind": "unit", "scale": 0.2, "rig": rig}
