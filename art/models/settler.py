"""Settler: a covered wagon drawn by an ox, a settler walking beside it."""

import math

import beasts
import engines
import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("none")
    ox = k.mat("ox", k.srgb(150, 110, 76), 0.85, pattern="noise", scale=4, amount=0.35, color2=k.srgb(96, 70, 48))
    p["horn_light"] = k.mat("horn_light", k.srgb(226, 214, 186), 0.5)
    parts, A = beasts.quadruped(k, p, "ox", ox, rig=rig, prefix="ox_", at=(0, -1.55, 0), points=k.mat("ox_dark", k.srgb(70, 52, 40), 0.8))
    yoke = k.rod(A["chest"] + Vector((-0.4, 0.25, 0.42)), A["chest"] + Vector((0.4, 0.25, 0.42)), 0.05, p["dark_wood"])
    k.bind(yoke, "ox_neck")
    parts.append(yoke)
    # Wagon bed, canvas hoops in the owner's colour, barrels and sacks.
    bed = Vector((0, 0.9, 0.95))
    rig.bone("cart_body", None, bed, bed + Vector((0, 0.8, 0)), radius=0.4)
    for y in (0.35, 1.55):
        rig.bone(f"wheel_l{int(y * 10)}", "cart_body", (0.62, y, 0.48))
        rig.bone(f"wheel_r{int(y * 10)}", "cart_body", (-0.62, y, 0.48))
    box = k.box((1.05, 1.9, 0.38), p["planks"], loc=bed, bevel=0.02)
    rings = []
    for j in range(8):
        y = bed.y - 0.92 + j * 1.84 / 7
        rings.append([(math.cos(a) * 0.56, y, bed.z + 0.15 + math.sin(a) * 0.72) for a in [i / 14 * math.pi for i in range(15)]])
    cover = k.loft("cover", rings, p["team"], closed=False, cap=False)
    mod = cover.modifiers.new("s", "SOLIDIFY")
    mod.thickness = 0.03
    pole = k.rod((0, bed.y - 0.95, bed.z - 0.15), A["chest"] + Vector((0, 0.25, 0.35)), 0.05, p["dark_wood"])
    barrel = k.cyl(0.18, 0.4, p["wood"], loc=(0.35, bed.y + 1.05, bed.z + 0.1), n=12)
    sack = k.ball(0.2, p["linen"], loc=(-0.3, bed.y + 1.0, bed.z + 0.25), scale=(1, 0.9, 1.1))
    k.bind([box, cover, pole, barrel, sack], "cart_body")
    parts += [box, cover, pole, barrel, sack]
    for y in (0.35, 1.55):
        parts += engines.wheel(k, p, (0.62, y, 0.48), 0.48, f"wheel_l{int(y * 10)}")
        parts += engines.wheel(k, p, (-0.62, y, 0.48), 0.48, f"wheel_r{int(y * 10)}")
    # The settler walks at the ox's side with a goad.
    body, J = people.figure(k, p, rig, at=(-0.95, -1.2, 0), arms=("hold", "hang"), stride=0.18, outfit={"sleeves": "long", "skirt": 0.75, "beard": True})
    parts += body
    parts += people.helmet(k, p, J, "hat", color=k.mat("brown_felt", k.srgb(110, 84, 56), 0.9, pattern="noise", scale=10, amount=0.2))
    wr = J["wrist_r"]
    parts += people.tool(k, p, wr + Vector((0, 0.1, -0.9)), wr + Vector((0.1, -0.35, 0.9)), "staff", bone="arm_r_lo")
    return parts, {"kind": "unit", "scale": 0.15, "rig": rig}
