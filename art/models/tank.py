"""Tank: a riveted hull in olive paint on two tracks with road wheels, a
round turret with a long gun, a band and the commander's flag in the owner's
colour, the commander standing in the open hatch, tools and spare track
links on the hull."""

import math

import people
import ships
from mathutils import Vector

LEN, WID = 6.0, 3.0
TRACK_W, TRACK_H = 0.6, 1.0


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("cannon")
    olive = k.mat("tank_olive", k.srgb(92, 98, 64), 0.6, metal=0.2, pattern="noise", scale=3, amount=0.25, color2=k.srgb(78, 82, 54))
    dark = k.mat("track_steel", k.srgb(52, 50, 48), 0.55, metal=0.6, pattern="noise", scale=20, amount=0.3)
    rubber = k.mat("rubber", k.srgb(34, 32, 30), 0.8)
    helmet_m = k.mat("tanker_cap", k.srgb(70, 54, 40), 0.7, pattern="noise", scale=10, amount=0.2)

    rig.bone("frame", None, Vector((0, 0, 0.3)), Vector((0, -LEN / 2, 0.3)), radius=1.5)
    parts = []
    # Tracks: a loop around the wheels, road wheels and a sprocket, each
    # wheel on its own bone so it turns with the distance covered.
    hull = []
    r = 0.36
    xs = [-2.1 + i * 1.05 for i in range(5)]
    for s, n in ((1, "l"), (-1, "r")):
        x = s * (WID / 2 - TRACK_W / 2)
        # The track: runs above and below, wrapped round the idler and sprocket.
        hull.append(k.box((TRACK_W, LEN - 0.9, 0.12), dark, loc=(x, 0, 0.06), bevel=0.03))
        hull.append(k.box((TRACK_W, LEN - 0.9, 0.12), dark, loc=(x, 0, TRACK_H - 0.06), bevel=0.03))
        for y, rad in ((-(LEN / 2 - 0.45), 0.5), (LEN / 2 - 0.45, 0.47)):
            hull.append(k.cyl(rad, TRACK_W, dark, loc=(x, y, rad), rot=(0, math.pi / 2, 0), n=18))
        # Track shoes: ridges across the run.
        for j in range(22):
            y = -(LEN - 0.9) / 2 + (j + 0.5) * (LEN - 0.9) / 22
            hull.append(k.box((TRACK_W + 0.02, 0.06, 0.03), dark, loc=(x, y, TRACK_H + 0.005)))
        for j, y in enumerate(xs):
            bone = f"wheel_{n}{j}"
            rig.bone(bone, "frame", Vector((x, y, r)))
            w = [k.cyl(r, TRACK_W * 0.7, rubber, loc=(x, y, r), rot=(0, math.pi / 2, 0), n=16)]
            w.append(k.cyl(r * 0.8, TRACK_W * 0.74, olive, loc=(x, y, r), rot=(0, math.pi / 2, 0), n=16))
            w.append(k.cyl(r * 0.25, TRACK_W * 0.8, dark, loc=(x, y, r), rot=(0, math.pi / 2, 0), n=8))
            for q in range(4):
                a = q / 4 * math.tau
                w.append(k.box((TRACK_W * 0.76, 0.06, 0.06), dark, loc=(x, y + math.cos(a) * r * 0.5, r + math.sin(a) * r * 0.5)))
            k.bind(w, bone)
            parts += w
        # A mudguard over the track.
        hull.append(k.box((TRACK_W + 0.1, LEN - 0.3, 0.05), olive, loc=(x + s * 0.03, 0, TRACK_H + 0.08), bevel=0.01))

    # The hull: a sloped glacis at the front, a flat deck, an engine deck at
    # the back with grilles and exhausts.
    body = k.loft(
        "tank_hull",
        [
            [(sx * hw, y, z) for sx, hw, z in [(-1, WID / 2 - TRACK_W, 0.3), (-1, WID / 2 - TRACK_W, zt), (1, WID / 2 - TRACK_W, zt), (1, WID / 2 - TRACK_W, 0.3)]]
            for y, zt in ((-LEN / 2 + 0.05, 0.75), (-LEN / 2 + 1.2, 1.55), (LEN / 2 - 0.15, 1.55), (LEN / 2, 1.0))
        ],
        olive,
        closed=True,
        cap=True,
        smooth=0.3,
    )
    hull.append(body)
    hull.append(k.box((WID - 0.1, LEN - 1.4, 0.36), olive, loc=(0, 0.45, 1.37), bevel=0.05))
    hull.append(k.box((WID - 0.1, 0.06, 0.12), p["team_paint"], loc=(0, -LEN / 2 + 1.17, 1.52)))
    for i in range(5):
        hull.append(k.box((1.6, 0.05, 0.03), p["black"], loc=(0, 1.6 + i * 0.18, 1.56)))
    for s in (-1, 1):
        hull.append(k.rod((s * 0.7, LEN / 2 - 0.05, 1.2), (s * 0.7, LEN / 2 + 0.25, 1.15), 0.08, dark, n=8))
        hull.append(k.box((0.25, 0.25, 0.25), olive, loc=(s * 0.95, -LEN / 2 + 0.6, 1.22), bevel=0.04))
    # Rivets along the glacis, spare track links on it, a tow cable, tools.
    for i in range(8):
        hull.append(k.ball(0.035, olive, loc=(-1.0 + i * 0.29, -LEN / 2 + 0.66, 1.18), subdiv=1))
    for i in range(4):
        hull.append(k.box((0.55, 0.14, 0.04), dark, loc=(-0.85 + i * 0.57, -LEN / 2 + 0.9, 1.33), rot=(-0.6, 0, 0)))
    hull.append(k.rod((1.15, -1.5, 1.6), (1.15, 1.8, 1.6), 0.04, p["rope"], n=5))
    hull.append(k.rod((-1.15, -1.0, 1.58), (-1.15, 0.6, 1.58), 0.03, p["wood"], n=5))
    hull.append(k.box((0.12, 0.3, 0.05), p["steel"], loc=(-1.15, -1.15, 1.58)))
    k.bind(hull, "frame")
    parts += hull

    # The turret and its gun (the barrel recoils).
    tz = 1.55
    rig.bone("turret", "frame", Vector((0, -0.2, tz)), Vector((0, -1.5, tz)))
    turret = [k.cyl(1.05, 0.75, olive, loc=(0, -0.1, tz + 0.37), n=28, r2=0.85)]
    turret.append(k.box((1.4, 0.8, 0.6), olive, loc=(0, 0.75, tz + 0.33), bevel=0.12))
    turret.append(k.box((1.0, 0.5, 0.55), olive, loc=(0, -1.0, tz + 0.35), bevel=0.1))
    turret.append(k.cyl(1.07, 0.16, p["team_paint"], loc=(0, -0.1, tz + 0.45), n=28))
    turret.append(k.cyl(0.32, 0.12, olive, loc=(0.35, 0.25, tz + 0.78), n=16))
    # A radio aerial with a pennant in the owner's colour.
    turret.append(k.rod((-0.6, 0.7, tz + 0.6), (-0.6, 0.7, tz + 2.6), 0.015, dark, n=4))
    k.bind(turret, "turret")
    parts += turret
    parts += ships.flag(k, p, rig, Vector((-0.6, 0.7, tz + 2.6)), "flag", size=0.28, parent="turret")
    muzzle_base = Vector((0, -1.25, tz + 0.38))
    rig.bone("barrel", "turret", muzzle_base, muzzle_base + Vector((0, -3.0, 0)))
    gun = [k.rod(muzzle_base, muzzle_base + Vector((0, -3.0, 0.05)), 0.09, dark, n=12, r2=0.08)]
    gun.append(k.cyl(0.14, 0.35, dark, loc=muzzle_base + Vector((0, -3.0, 0.05)), rot=(math.pi / 2, 0, 0), n=12))
    gun.append(k.cyl(0.16, 0.5, olive, loc=muzzle_base + Vector((0, -0.15, 0)), rot=(math.pi / 2, 0, 0), n=14))
    k.bind(gun, "barrel")
    parts += gun

    # The commander, standing in the hatch from the waist up.
    hatch = Vector((0.35, 0.25, tz + 0.78))
    body, J = people.figure(k, p, rig, parent="turret", prefix="c_", at=hatch - Vector((0, 0, 1.15)), arms=("hold", "reach"), outfit={"sleeves": "long", "skirt": None, "legs": "trousers"})
    h, up, fwd = J["head"], J["up"], J["fwd"]
    cap = [k.ball(0.115, helmet_m, loc=h + up * 0.04, scale=(1.0, 1.05, 0.85))]
    for s in (-1, 1):
        cap.append(k.cyl(0.04, 0.03, p["black"], loc=h + J["side"] * s * 0.1, rot=(0, math.pi / 2, 0), n=10))
    cap.append(k.box((0.17, 0.05, 0.05), k.mat("goggles", k.srgb(60, 70, 80), 0.2, metal=0.5), loc=h + up * 0.1 + fwd * 0.06))
    k.bind(cap, J["prefix"] + "head")
    parts += body + cap
    return parts, {"kind": "unit", "scale": 0.095, "rig": rig}
