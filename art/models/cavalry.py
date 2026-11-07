"""Cavalry: a trooper in a short jacket and a plumed shako on a bay horse,
sabre drawn, a carbine slung at the saddle."""

import beasts
import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("melee")
    coat = k.mat("dark_bay", k.srgb(96, 56, 34), 0.75, pattern="noise", scale=5, amount=0.35, color2=k.srgb(64, 36, 22))
    p["mane"] = k.mat("black_mane", k.srgb(26, 22, 20), 0.7, pattern="hair", scale=10, amount=0.3)
    parts, A = beasts.quadruped(k, p, "horse", coat, rig=rig, points=p["mane"], prefix="horse_", extra={"lh": 1.0, "br": 0.32, "bw": 0.28})
    parts += beasts.tack(k, p, A, prefix="horse_")
    body, J = people.figure(k, p, rig, parent="horse_body", mounted=A["seat"] + Vector((0, 0, 0.06)), arms=("high", "hold"), outfit={"sleeves": "long", "skirt": 0.86})
    parts += body
    h, up, fwd = J["head"], J["up"], J["fwd"]
    shako = [k.cyl(0.1, 0.2, p["black"], loc=h + up * 0.15, n=16, r2=0.11)]
    shako.append(k.box((0.15, 0.07, 0.012), p["black"], loc=h + up * 0.06 + fwd * 0.1, rot=(-0.3, 0, 0)))
    shako.append(k.ball(0.03, p["gold_paint"], loc=h + up * 0.17 + fwd * 0.1))
    shako.append(k.blobs("plume", [("ellipsoid", h + up * 0.32, (0.03, 0.03, 0.08))], p["team"], res=0.01, faces=150))
    k.bind(shako, J["prefix"] + "head")
    parts += shako
    sash = k.box((0.05, 0.3, 0.5), p["white_paint"], loc=J["chest"] + Vector((0, 0, -0.08)), rot=(0, -0.6, 0))
    k.bind([sash], J["prefix"] + "chest")
    parts.append(sash)
    wr = J["wrist_r"]
    parts += people.blade(k, p, wr, (0.1, -0.5, 1.0), length=0.85, width=0.035, bone="arm_r_lo")
    carbine = people.musket(k, p, A["seat"] + Vector((0.24, 0.35, -0.3)), A["seat"] + Vector((0.24, -0.5, 0.05)))
    k.bind(carbine, "horse_body")
    parts += carbine
    return parts, {"kind": "unit", "scale": 0.19, "rig": rig}
