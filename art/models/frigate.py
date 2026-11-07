"""Frigate: a three-masted, square-rigged warship with a gun deck behind
ports on a band in the owner's colour."""

import math

import people
import ships
from mathutils import Vector

L, B, DECK = 42.0, 10.5, 3.6


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("cannon")
    rig.bone("hull", None, (0, 0, 0.0), (0, -L / 2, 0), radius=5.0)
    planks = k.mat("dark_planks", k.srgb(96, 72, 50), 0.75, pattern="grain", scale=1.4, amount=0.35)
    h, station = ships.hull(k, p, L, B, DECK, bow_rise=1.4, stern_rise=1.6, keel=-2.4, fullness=0.4, transom=True, band=(2.0, 0.9), planks=planks)
    parts = [h]
    # Gun ports along the band, the guns' muzzles showing.
    for s in (-1, 1):
        for i in range(11):
            t = 0.15 + i * 0.068
            w, _, d = station(t)
            y = L / 2 - t * L
            parts.append(k.box((0.25, 1.0, 0.9), p["black"], loc=(s * w * 0.99, y, d - 1.45)))
            parts.append(k.cyl(0.22, 0.8, p["iron"], loc=(s * (w + 0.2), y, d - 1.45), rot=(0, math.pi / 2, 0), n=8))
    # Quarterdeck and stern gallery, forecastle, bowsprit.
    w, _, d = station(0.08)
    parts.append(k.box((w * 2 * 0.96, 9.0, 1.6), planks, loc=(0, L / 2 - 4.6, d + 0.6), bevel=0.08))
    parts.append(k.box((w * 1.6, 0.6, 2.2), p["gold_paint"], loc=(0, L / 2 + 0.2, d + 0.2)))
    for i in range(5):
        parts.append(k.box((0.7, 0.3, 0.9), p["black"], loc=(-w * 0.6 + i * w * 0.3, L / 2 + 0.45, d + 0.3)))
    parts.append(k.rod((0, -L / 2 + 2.5, station(0.95)[2] + 0.4), (0, -L / 2 - 8.0, station(0.95)[2] + 4.2), 0.3, p["dark_wood"], r2=0.15))
    tops = []
    for y, hgt, wdt in ((-11.0, 26.0, 13.0), (0.5, 30.0, 15.0), (11.5, 23.0, 11.0)):
        parts += ships.mast(k, p, (0, y, DECK), hgt, 0.4)
        z = DECK + 4.5
        for tier, frac in enumerate((0.42, 0.32, 0.22)):
            sh = hgt * frac * 0.85
            sw = wdt * (1.0 - tier * 0.18)
            parts += ships.square_sail(k, p, (0, y - 0.6, z + sh / 2), sw, sh, p["team"], bulge=1.0 - tier * 0.2)
            z += sh + 0.5
        parts.append(k.cyl(1.2, 0.3, planks, loc=(0, y, DECK + hgt * 0.45), n=10))
        tops.append(Vector((0, y, DECK + hgt)))
    for a, b in zip(tops, tops[1:]):
        parts.append(ships.stay(k, p, a, b + Vector((0, 0, -6)), 0.07))
    parts.append(ships.stay(k, p, tops[0], (0, -L / 2 - 7.5, station(0.95)[2] + 4.0), 0.07))
    for t in tops:
        for s in (-1, 1):
            w, _, d = station((L / 2 - t.y) / L)
            parts.append(ships.stay(k, p, t + Vector((0, 0, -3)), (s * w, t.y + 1.0, d), 0.05))
    parts += ships.bulwark(k, p, station, L, 1.2, t0=0.2, t1=0.95, thick=0.3, material=planks)
    for y in (-6.0, 5.5):
        parts += ships.hatch(k, p, (0, y, DECK), 2.4, 2.0)
    parts += ships.capstan(k, p, (0, 9.0, DECK))
    parts += ships.ship_boat(k, p, (0, -3.0, DECK + 0.4), 5.0)
    for s_ in (-1, 1):
        for i in range(4):
            t = 0.32 + i * 0.1
            w, _, d = station(t)
            parts += ships.deck_gun(k, p, (s_ * (w - 1.2), L / 2 - t * L, d), s_)
        parts += ships.barrel(k, p, (s_ * 2.5, -12.0, DECK))
        parts += ships.coil(k, p, (s_ * 2.8, 2.0, DECK), 0.55)
    # The quarterdeck: rails and the wheel.
    w, _, d = station(0.08)
    for s_ in (-1, 1):
        parts.append(k.box((0.2, 9.0, 0.9), planks, loc=(s_ * w * 0.95, L / 2 - 4.6, d + 1.85)))
    wheel_at = Vector((0, L / 2 - 7.5, d + 1.4))
    parts.append(k.cyl(0.9, 0.12, p["dark_wood"], loc=wheel_at + Vector((0, 0, 1.0)), rot=(math.pi / 2, 0, 0), n=16))
    parts.append(k.box((0.3, 0.3, 1.0), p["dark_wood"], loc=wheel_at + Vector((0, 0.2, 0.5))))
    k.bind(parts, "hull")
    parts += ships.flag(k, p, rig, tops[1] + Vector((0, 0, 0.3)), "flag", size=2.6)
    parts += ships.flag(k, p, rig, (0, L / 2 + 0.8, d + 6.0), "flag_stern", size=2.4)
    pole = k.rod((0, L / 2 + 0.6, d + 1.0), (0, L / 2 + 0.8, d + 6.2), 0.08, p["dark_wood"])
    k.bind(pole, "hull")
    parts.append(pole)
    return parts, {"kind": "unit", "scale": 0.0125, "rig": rig}
