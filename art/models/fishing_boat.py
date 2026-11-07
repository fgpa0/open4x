"""Fishing boat: a small open boat with two fishermen, a net heaped in the
stern and a furled lugsail. Sails out from coastal towns by day (ambient
traffic, not a unit)."""

import people
import ships
from mathutils import Vector

L, B, DECK = 4.6, 1.7, 0.32


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("none")
    rig.bone("hull", None, (0, 0, 0.0), (0, -L / 2, 0), radius=0.8)
    paint = k.mat("boat_paint", k.srgb(56, 92, 120), 0.7, pattern="noise", scale=4, amount=0.3)
    h, station = ships.hull(k, p, L, B, DECK, bow_rise=0.2, stern_rise=0.12, keel=-0.4, fullness=0.5, band=(0.0, 0.0))
    parts = [h]
    parts += ships.bulwark(k, p, station, L, 0.32, t0=0.025, t1=0.975, thick=0.06, ends=True, material=paint)
    for t, wd in ((0.35, 0.18), (0.6, 0.16)):
        w, _, d = station(t)
        parts.append(k.box((w * 2 - 0.1, wd, 0.06), p["planks"], loc=(0, L / 2 - t * L, d + 0.08)))
    # A heap of net with cork floats, a basket of fish.
    net = k.mat("net", k.srgb(150, 130, 96), 0.95, pattern="weave", scale=40, amount=0.5)
    parts.append(k.ball(0.38, net, loc=(0.1, L / 2 - 0.7, DECK + 0.05), scale=(1.1, 1.0, 0.45)))
    cork = k.mat("cork", k.srgb(200, 150, 80), 0.8)
    for i in range(5):
        parts.append(k.ball(0.06, cork, loc=(-0.2 + i * 0.12, L / 2 - 0.5 - (i % 2) * 0.2, DECK + 0.2), subdiv=1))
    basket = k.mat("basket", k.srgb(170, 130, 80), 0.9, pattern="weave", scale=20, amount=0.4)
    parts.append(k.cyl(0.2, 0.25, basket, loc=(-0.3, -0.6, DECK + 0.05), n=10))
    fish = k.mat("catch", k.srgb(170, 186, 196), 0.3, metal=0.4)
    parts.append(k.ball(0.17, fish, loc=(-0.3, -0.6, DECK + 0.2), scale=(1.0, 1.0, 0.35)))
    parts += ships.mast(k, p, (0, -0.6, DECK - 0.3), 2.6, 0.05)
    sail = k.mat("lug", k.srgb(214, 196, 160), 0.9, pattern="weave", scale=12, amount=0.2)
    parts.append(k.cyl(0.12, 1.6, sail, loc=(0, -0.62, DECK + 2.0), rot=(0, 1.5708, 0), n=8))
    k.bind(parts, "hull")
    for at, arms in (((0, 0.25, DECK + 0.2), ("low_fwd", "low_fwd")), ((0, L / 2 - 0.45, DECK + 0.2), ("hang", "low_fwd"))):
        body, J = people.figure(k, p, rig, parent="hull", prefix="b" if at[1] > 1 else "", mounted=at, arms=arms, outfit={"sleeves": "long", "skirt": None, "beard": at[1] > 1, "tunic": k.mat("smock", k.srgb(150, 60, 50), 0.9, pattern="weave", scale=20, amount=0.3)})
        parts += body
    parts += ships.oars(k, p, rig, station, L, [0.45], z_off=0.0, reach=1.4, blade=0.45, r=0.03)
    return parts, {"kind": "unit", "scale": 0.13, "rig": rig}
