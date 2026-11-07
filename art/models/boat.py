"""Boat: the skiff a land unit travels in while embarked: a rower, a short
mast with a sail in the owner's colour."""

import math

import people
import ships
from mathutils import Vector

L, B, DECK = 4.6, 1.7, 0.32


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("none")
    rig.bone("hull", None, (0, 0, 0.0), (0, -L / 2, 0), radius=0.8)
    h, station = ships.hull(k, p, L, B, DECK, bow_rise=0.2, stern_rise=0.12, keel=-0.4, fullness=0.5, band=(0.0, 0.0))
    parts = [h]
    # An open boat: sides rising above the floorboards, a gunwale, ribs,
    # thwarts to sit on, oarlocks, and a little cargo.
    # The sides above the floor, closed all round, with a rail.
    parts += ships.bulwark(k, p, station, L, 0.34, t0=0.025, t1=0.975, thick=0.06, ends=True)
    for t in (0.2, 0.35, 0.5, 0.65, 0.8):
        w, _, d = station(t)
        y = L / 2 - t * L
        for s_ in (-1, 1):
            parts.append(k.box((0.05, 0.06, 0.4), p["dark_wood"], loc=(s_ * (w - 0.09), y, d + 0.1)))
    for t, wd in ((0.42, 0.18), (0.62, 0.16)):
        w, _, d = station(t)
        parts.append(k.box((w * 2 - 0.1, wd, 0.06), p["planks"], loc=(0, L / 2 - t * L, d + 0.08)))
    w, _, d = station(0.12)
    parts.append(k.box((w * 2 - 0.1, 0.5, 0.06), p["planks"], loc=(0, L / 2 - 0.12 * L, d + 0.1)))
    w, _, d = station(0.88)
    parts.append(k.box((w * 2 - 0.1, 0.35, 0.06), p["planks"], loc=(0, L / 2 - 0.88 * L, d + 0.1)))
    for s_ in (-1, 1):
        w, _, d = station(0.5)
        parts.append(k.cyl(0.03, 0.18, p["iron"], loc=(s_ * w, 0.0, d + 0.4), n=6))
    parts += ships.barrel(k, p, (0.25, L / 2 - 0.55, DECK - 0.05), r=0.18, h=0.36)
    parts.append(k.ball(0.22, p["linen"], loc=(-0.25, L / 2 - 0.6, DECK + 0.12), scale=(1.0, 0.8, 0.75)))
    parts += ships.coil(k, p, (0.0, -L / 2 + 0.75, DECK + 0.05), 0.16)
    parts += ships.mast(k, p, (0, -0.9, DECK - 0.3), 3.2, 0.06)
    parts += ships.square_sail(k, p, (0, -1.0, DECK + 1.6), 2.0, 1.6, p["team"], bulge=0.3, yard_r=0.035)
    k.bind(parts, "hull")
    body, J = people.figure(k, p, rig, parent="hull", mounted=(0, 0.3, DECK + 0.2), arms=("low_fwd", "low_fwd"), outfit={"sleeves": "long", "skirt": None, "beard": True})
    parts += body
    parts += ships.oars(k, p, rig, station, L, [0.5], z_off=0.0, reach=1.5, blade=0.5, r=0.03)
    return parts, {"kind": "unit", "scale": 0.13, "rig": rig}
