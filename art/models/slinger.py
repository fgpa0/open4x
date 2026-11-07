"""Slinger: a lightly clad skirmisher whirling a sling overhead, a pouch of
stones at the hip."""

import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("melee")
    parts, J = people.figure(k, p, rig, yaw=0.2, arms=("high", "reach"), stride=0.22, outfit={"sleeves": "short", "skirt": 0.72, "legs": "bare", "boots": False, "hair": p["hair"]})
    parts += people.helmet(k, p, J, "headband")
    # The sling: two cords from the fist to a pouch with its stone, swung out behind.
    wr = J["wrist_r"]
    pouch_at = wr + Vector((0.12, 0.3, 0.28))
    cords = [k.rod(wr, pouch_at + Vector((s * 0.03, 0, 0)), 0.006, p["rope"], n=4) for s in (-1, 1)]
    pouch = k.ball(0.04, p["leather"], loc=pouch_at, scale=(1.0, 1.4, 0.6))
    stone = k.ball(0.03, p["stone"], loc=pouch_at + Vector((0, 0, 0.02)), subdiv=1)
    k.bind(cords + [pouch, stone], "arm_r_lo")
    parts += cords + [pouch, stone]
    bag = k.ball(0.08, p["leather"], loc=J["hips"] + Vector((-0.17, 0.02, -0.06)), scale=(0.7, 0.9, 1.0))
    k.bind([bag], "hips")
    parts.append(bag)
    return parts, {"kind": "unit", "scale": 0.2, "rig": rig}
