"""Crossbowman: a padded jack, kettle hat and a crossbow levelled forward."""

import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("bow")
    parts, J = people.figure(k, p, rig, arms=("high_fwd", "high_fwd"), stride=0.12, outfit={"sleeves": "long", "skirt": 0.62})
    parts += people.helmet(k, p, J, "kettle")
    grip = (J["wrist_r"] + J["wrist_l"]) / 2 + J["fwd"] * 0.05
    parts += people.crossbow(k, p, grip, J["fwd"], bone="arm_r_lo")
    pavise = k.box((0.5, 0.06, 0.8), p["team_paint"], loc=J["chest"] - J["fwd"] * 0.2 + Vector((0, 0, -0.05)), rot=(0.12, 0, 0), bevel=0.02)
    cross = k.box((0.08, 0.07, 0.55), p["gold_paint"], loc=J["chest"] - J["fwd"] * 0.235 + Vector((0, 0, -0.05)), rot=(0.12, 0, 0))
    k.bind([pavise, cross], "chest")
    parts += [pavise, cross]
    parts += people.quiver(k, p, J["hips"] + Vector((0.2, 0.05, -0.1)), "hips")
    return parts, {"kind": "unit", "scale": 0.2, "rig": rig}
