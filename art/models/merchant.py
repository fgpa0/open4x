"""Merchant: a pack mule laden with bales, led by a merchant in a wide hat."""

import beasts
import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("none")
    coat = k.mat("mule", k.srgb(104, 86, 70), 0.85, pattern="noise", scale=5, amount=0.3, color2=k.srgb(70, 58, 48))
    parts, A = beasts.quadruped(k, p, "mule", coat, rig=rig, prefix="mule_", at=(0.35, 0.25, 0), points=k.mat("mule_dark", k.srgb(52, 44, 38), 0.8))
    w = A["w"]
    bw, br, cz, bl = A["girth"]
    load = [beasts.drape(k, "packcloth", A, -0.5, 0.5, 80, p["team"], 0.02, grow=1.08)]
    for s in (1, -1):
        bale = k.box((0.26, 0.6, 0.45), p["linen"], loc=w((s * (bw + 0.16), 0, cz + 0.12)), bevel=0.05)
        stripe = k.box((0.27, 0.1, 0.46), p["team_paint"], loc=w((s * (bw + 0.165), 0, cz + 0.12)))
        rope = k.box((0.275, 0.62, 0.04), p["rope"], loc=w((s * (bw + 0.165), 0, cz + 0.2)))
        load += [bale, stripe, rope]
    bundle = k.cyl(0.16, 0.7, k.mat("rug", k.srgb(170, 60, 44), 0.9, pattern="weave", scale=20, amount=0.35), loc=w((0, 0.05, cz + br + 0.22)), rot=(0, 1.5708, 0), n=12)
    jar = k.cyl(0.11, 0.3, k.mat("clay", k.srgb(176, 104, 66), 0.8, pattern="noise", scale=6, amount=0.2), loc=w((0, -0.25, cz + br + 0.2)), n=12, r2=0.06)
    load += [bundle, jar]
    k.bind(load, "mule_body")
    parts += load
    body, J = people.figure(k, p, rig, at=(-0.45, -0.75, 0), arms=("hold", "reach"), stride=0.16, outfit={"tunic": p["team"], "sleeves": "long", "skirt": 0.3, "beard": True})
    parts += body
    parts += people.helmet(k, p, J, "hat", color=k.mat("hat_red", k.srgb(120, 46, 38), 0.85, pattern="noise", scale=10, amount=0.2))
    wr = J["wrist_r"]
    parts += people.tool(k, p, wr + Vector((0, 0.08, -0.95)), wr + Vector((0, -0.12, 0.7)), "staff", bone="arm_r_lo")
    lead = k.rod(J["hand_l"], A["muzzle"] + Vector((0, 0.05, 0.0)), 0.012, p["rope"], n=4)
    k.bind(lead, "arm_l_lo")
    parts.append(lead)
    return parts, {"kind": "unit", "scale": 0.19, "rig": rig}
