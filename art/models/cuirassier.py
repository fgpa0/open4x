"""Cuirassier: heavy cavalry on a black horse, a steel breastplate and
lobster-tail helmet, a sabre raised, a sash in the owner's colour."""

import beasts
import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("melee")
    coat = k.mat("black_horse", k.srgb(40, 36, 36), 0.6, pattern="noise", scale=5, amount=0.3, color2=k.srgb(24, 22, 22))
    p["mane"] = k.mat("black_mane", k.srgb(20, 18, 18), 0.7, pattern="hair", scale=10, amount=0.3)
    parts, A = beasts.quadruped(k, p, "horse", coat, rig=rig, prefix="horse_", points=p["mane"], extra={"lh": 0.95, "br": 0.36, "bw": 0.3})
    parts += beasts.tack(k, p, A, prefix="horse_")
    body, J = people.figure(k, p, rig, parent="horse_body", mounted=A["seat"] + Vector((0, 0, 0.08)), arms=("high", "hold"), outfit={"armour": "plate", "sleeves": "long", "skirt": 0.62})
    parts += body
    parts += people.helmet(k, p, J, "kettle", color=p["steel"])
    # The lobster tail guarding the neck.
    tail = k.ball(0.1, p["steel"], loc=J["head"] - J["fwd"] * 0.1 - J["up"] * 0.05, scale=(1.1, 0.6, 0.7))
    k.bind([tail], J["prefix"] + "head")
    parts.append(tail)
    sash = k.box((0.05, 0.3, 0.5), p["team"], loc=J["chest"] + Vector((0, 0, -0.08)), rot=(0, 0.6, 0))
    k.bind([sash], J["prefix"] + "chest")
    parts.append(sash)
    wr = J["wrist_r"]
    parts += people.blade(k, p, wr, (0.05, -0.35, 1.0), length=0.85, width=0.04, bone="arm_r_lo")
    # Holsters at the saddle bow.
    for s in (-1, 1):
        h = k.cyl(0.035, 0.28, p["leather"], loc=A["seat"] + Vector((s * 0.2, -0.32, -0.12)), rot=(0.5, 0, 0), n=8)
        k.bind([h], "horse_body")
        parts.append(h)
    return parts, {"kind": "unit", "scale": 0.19, "rig": rig}
