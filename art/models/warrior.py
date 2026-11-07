"""Warrior: an ancient fighter with a stone-headed club and a hide shield."""

import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("melee")
    parts, J = people.figure(k, p, rig, arms=("raise", "shield"), stride=0.12, outfit={"legs": "bare", "boots": False, "skirt": 0.6, "beard": True, "belt": True})
    parts += people.helmet(k, p, J, "headband")
    parts += people.club(k, p, J["hand_r"], Vector((0.05, -0.2, 1)), length=0.75, bone="arm_r_lo")
    hide = k.mat("hide", k.srgb(150, 112, 74), 0.85, pattern="noise", scale=8, amount=0.4)
    shield = k.cyl(0.26, 0.03, hide, loc=J["wrist_l"] + Vector((0.08, -0.04, 0.0)), rot=(0, 1.35, 0.25), n=16)
    stripe = k.box((0.06, 0.04, 0.4), p["team_paint"], loc=J["wrist_l"] + Vector((0.1, -0.04, 0.0)), rot=(0, 0, 0.25))
    k.bind([shield, stripe], "arm_l_lo")
    parts += [shield, stripe]
    pelt = k.blobs("pelt", [("ellipsoid", J["chest"] + Vector((0.05, 0.02, 0.12)), (0.2, 0.13, 0.08), 1.0, (0.0, 0.3, 0.0))], p["fur"], res=0.012, faces=300)
    k.bind(pelt, "chest")
    parts.append(pelt)
    return parts, {"kind": "unit", "scale": 0.2, "rig": rig}
