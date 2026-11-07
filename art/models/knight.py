"""Knight: an armoured rider with lance and kite shield on a barded horse."""

import beasts
import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("thrust")
    coat = k.mat("grey", k.srgb(150, 146, 140), 0.75, pattern="noise", scale=5, amount=0.35, color2=k.srgb(96, 92, 88))
    p["mane"] = k.mat("dark_mane", k.srgb(40, 36, 34), 0.7, pattern="hair", scale=10, amount=0.3)
    parts, A = beasts.quadruped(k, p, "horse", coat, rig=rig, prefix="horse_", points=p["mane"], extra={"lh": 0.9, "br": 0.36, "bw": 0.3})
    parts += beasts.tack(k, p, A, prefix="horse_")
    # A caparison in the owner's colour hanging to the knees, with a gold hem.
    trapper = beasts.drape(k, "trapper", A, -1.05, 1.15, 100, p["team"], 0.02, lift=-0.04, grow=1.12)
    k.blend([trapper], ["horse_body", "horse_fl_up", "horse_fr_up", "horse_hl_up", "horse_hr_up"])
    parts += [trapper]
    chanfron = k.ball(0.11, p["iron"], loc=A["muzzle"] + Vector((0, 0.16, 0.16)), scale=(0.8, 1.6, 0.7), rot=(0.9, 0, 0))
    k.bind(chanfron, "horse_head")
    parts.append(chanfron)
    body, J = people.figure(k, p, rig, parent="horse_body", mounted=A["seat"] + Vector((0, 0, 0.1)), arms=("hold", "shield"), outfit={"armour": "plate", "sleeves": "long", "skirt": 0.6})
    parts += body
    parts += people.helmet(k, p, J, "great")
    plume = k.blobs("plume", [("ellipsoid", J["head"] + Vector((0, 0.05, 0.3)), (0.04, 0.1, 0.12))], p["team"], res=0.012, faces=200)
    k.bind(plume, "head")
    parts.append(plume)
    wr = J["wrist_r"]
    lance = people.spear(k, p, wr + Vector((0, 0.7, -0.45)), wr + Vector((0, -1.5, 1.35)), r=0.03, bone="arm_r_lo")
    vamplate = k.cyl(0.09, 0.14, p["iron"], loc=wr + Vector((0, -0.12, 0.08)), rot=(0.9, 0, 0), n=12, r2=0.025)
    k.bind(vamplate, "arm_r_lo")
    parts += lance + [vamplate]
    parts += people.shield(k, p, J["wrist_l"] + Vector((0.1, 0.05, 0.05)), (1, -0.3, 0.05), "kite", 0.85, bone="arm_l_lo")
    return parts, {"kind": "unit", "scale": 0.19, "rig": rig}
