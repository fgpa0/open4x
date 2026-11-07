"""Rifleman: a soldier in a uniform coat and kepi, aiming a long rifle with
a bayonet, a cartridge box and a pack."""

import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("gun")
    right = ((-0.32, -0.08, 1.26), (-0.15, -0.26, 1.37))
    left = ((-0.14, -0.36, 1.24), (-0.04, -0.62, 1.35))
    trousers = k.mat("trousers", k.srgb(70, 72, 84), 0.85, pattern="weave", scale=40, amount=0.2)
    parts, J = people.figure(k, p, rig, yaw=0.2, arms=(right, left), stride=0.16, outfit={"sleeves": "long", "skirt": 0.82, "legs": "trousers"})
    # A kepi: a short cylinder leaning forward, a peak, the owner's colour band.
    h, up, fwd = J["head"], J["up"], J["fwd"]
    cap = [k.cyl(0.1, 0.14, trousers, loc=h + up * 0.12 + fwd * 0.01, n=16, r2=0.09)]
    cap.append(k.cyl(0.103, 0.03, p["team"], loc=h + up * 0.07, n=16))
    cap.append(k.box((0.16, 0.08, 0.012), p["black"], loc=h + up * 0.06 + fwd * 0.1, rot=(-0.25, 0, 0)))
    k.bind(cap, J["prefix"] + "head")
    parts += cap
    butt = J["shoulder_r"] + J["fwd"] * 0.02 + Vector((0.04, 0, -0.04))
    muzzle = butt + (J["wrist_l"] - butt).normalized() * 1.25
    parts += people.musket(k, p, butt, muzzle, bone="arm_r_lo")
    d = (muzzle - butt).normalized()
    bayonet = k.rod(muzzle, muzzle + d * 0.3, 0.008, p["steel"], n=4, r2=0.002)
    k.bind([bayonet], "arm_r_lo")
    parts.append(bayonet)
    belt = k.box((0.05, 0.25, 0.46), p["white_paint"], loc=J["chest"] + Vector((0, 0, -0.1)), rot=(0, 0.55, 0))
    box = k.box((0.16, 0.06, 0.1), p["leather"], loc=J["hips"] + Vector((0.12, 0.12, 0.06)), bevel=0.02)
    pack = k.box((0.3, 0.14, 0.32), k.mat("canvas", k.srgb(120, 110, 86), 0.9, pattern="weave", scale=30, amount=0.3), loc=J["chest"] - J["fwd"] * 0.16 + Vector((0, 0, -0.02)), bevel=0.03)
    roll = k.cyl(0.05, 0.36, k.mat("greatcoat", k.srgb(90, 92, 100), 0.9, pattern="weave", scale=30, amount=0.3), loc=J["chest"] - J["fwd"] * 0.16 + Vector((0, 0, 0.18)), rot=(0, 1.5708, 0), n=10)
    k.bind([belt, pack, roll], J["prefix"] + "chest")
    k.bind([box], J["prefix"] + "hips")
    parts += [belt, box, pack, roll]
    return parts, {"kind": "unit", "scale": 0.2, "rig": rig}
