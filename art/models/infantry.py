"""Infantry: a soldier in a field tunic, olive trousers and puttees and a
steel helmet, aiming a bolt-action rifle, with webbing, ammunition pouches,
a pack, an entrenching tool and grenades."""

import math

import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("gun")
    right = ((-0.32, -0.08, 1.26), (-0.15, -0.26, 1.37))
    left = ((-0.14, -0.36, 1.24), (-0.04, -0.62, 1.35))
    olive = k.mat("olive_drab", k.srgb(96, 92, 62), 0.85, pattern="weave", scale=40, amount=0.2)
    webbing = k.mat("webbing", k.srgb(150, 140, 102), 0.9, pattern="weave", scale=50, amount=0.25)
    helmet_m = k.mat("helmet_steel", k.srgb(88, 92, 70), 0.6, metal=0.3, pattern="noise", scale=10, amount=0.25)
    p["trousers"] = olive
    parts, J = people.figure(k, p, rig, yaw=0.2, arms=(right, left), stride=0.18, outfit={"sleeves": "long", "skirt": 0.86, "legs": "trousers"})
    h, up, fwd, side = J["head"], J["up"], J["fwd"], J["side"]
    # A steel helmet: a shallow dome with a wide brim, a band in the owner's
    # colour and a chin strap.
    helm = [k.ball(0.122, helmet_m, loc=h + up * 0.05, scale=(1.0, 1.05, 0.72))]
    helm.append(k.cyl(0.18, 0.014, helmet_m, loc=h + up * 0.035, n=22, r2=0.16))
    helm.append(k.cyl(0.124, 0.022, p["team"], loc=h + up * 0.06, n=20))
    helm.append(k.rod(h + side * 0.1 + up * 0.02, h + fwd * 0.06 - up * 0.1, 0.006, p["leather"], n=4))
    helm.append(k.rod(h - side * 0.1 + up * 0.02, h + fwd * 0.06 - up * 0.1, 0.006, p["leather"], n=4))
    k.bind(helm, J["prefix"] + "head")
    parts += helm
    # Puttees wound from the boot to below the knee.
    puttee = k.mat("puttee", k.srgb(110, 104, 72), 0.9, pattern="weave", scale=60, amount=0.35)
    for n in ("r", "l"):
        a, b = J[f"ankle_{n}"] + Vector((0, 0, 0.04)), J[f"knee_{n}"] - (J[f"knee_{n}"] - J[f"ankle_{n}"]) * 0.2
        wrap = k.rod(a, b, 0.05, puttee, n=10, r2=0.06)
        k.bind([wrap], J["prefix"] + f"leg_{n}_lo")
        parts.append(wrap)
    # The rifle: a plain stock, a bolt, a short bayonet.
    butt = J["shoulder_r"] + J["fwd"] * 0.02 + Vector((0.04, 0, -0.04))
    muzzle = butt + (J["wrist_l"] - butt).normalized() * 1.1
    parts += people.musket(k, p, butt, muzzle, bone="arm_r_lo")
    d = (muzzle - butt).normalized()
    bolt = k.rod(butt + d * 0.32 + Vector((0.04, 0, 0.03)), butt + d * 0.32 + Vector((0.09, 0, 0.0)), 0.01, p["steel"], n=4)
    bayonet = k.rod(muzzle, muzzle + d * 0.22, 0.008, p["steel"], n=4, r2=0.002)
    k.bind([bolt, bayonet], "arm_r_lo")
    parts += [bolt, bayonet]
    # Webbing: braces over the shoulders, a belt with pouches, a pack with a
    # rolled blanket, a shovel and two grenades.
    chest, hips = J["chest"], J["hips"]
    gear = []
    for s in (-1, 1):
        gear.append(k.box((0.045, 0.26, 0.5), webbing, loc=chest + side * s * 0.09 + Vector((0, 0, -0.08)), rot=(0, s * 0.1, 0)))
    belt = k.cyl(0.2, 0.06, webbing, loc=hips + Vector((0, 0, 0.1)), n=16)
    gear.append(belt)
    for i, a in enumerate((-0.9, -0.45, 0.45, 0.9)):
        gear.append(k.box((0.09, 0.06, 0.1), webbing, loc=hips + Vector((math.sin(a) * 0.2, -math.cos(a) * 0.2, 0.06)), rot=(0, 0, a), bevel=0.015))
    pack_m = k.mat("pack_canvas", k.srgb(124, 116, 84), 0.9, pattern="weave", scale=30, amount=0.3)
    gear.append(k.box((0.3, 0.14, 0.34), pack_m, loc=chest - fwd * 0.17 + Vector((0, 0, -0.04)), bevel=0.04))
    gear.append(k.cyl(0.055, 0.38, k.mat("blanket", k.srgb(110, 88, 66), 0.95, pattern="weave", scale=30, amount=0.3), loc=chest - fwd * 0.17 + Vector((0, 0, 0.17)), rot=(0, math.pi / 2, 0), n=10))
    gear.append(k.box((0.12, 0.02, 0.16), p["steel"], loc=chest - fwd * 0.25 + Vector((0.0, 0, -0.32)), bevel=0.01))
    gear.append(k.rod(chest - fwd * 0.25 + Vector((0, 0, -0.24)), chest - fwd * 0.25 + Vector((0, 0, 0.0)), 0.014, p["wood"], n=5))
    k.bind(gear, J["prefix"] + "chest")
    parts += gear
    nades = []
    for s in (-1, 1):
        g = hips + side * s * 0.17 + fwd * 0.05 + Vector((0, 0, 0.0))
        nades.append(k.ball(0.04, helmet_m, loc=g, scale=(1, 1, 1.3), subdiv=1))
        nades.append(k.cyl(0.012, 0.03, p["steel"], loc=g + Vector((0, 0, 0.05)), n=6))
    k.bind(nades, J["prefix"] + "hips")
    parts += nades
    return parts, {"kind": "unit", "scale": 0.2, "rig": rig}
