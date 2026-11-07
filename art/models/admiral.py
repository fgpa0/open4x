"""Admiral: a fast flagship that leads a fleet. A slender oared galley with
a gilded stern and ram, two lateen sails in the owner's colour, a raised
command deck under a canopy where the admiral stands, and pennants
streaming from both mastheads and a standard at the stern."""

import math

import people
import ships
from mathutils import Vector

L, B, DECK = 24.0, 5.0, 1.8


def build(k):
    p = people.extend_palette(k, k.palette())
    gold = k.mat("gilt", k.srgb(214, 168, 72), 0.35, metal=1.0, pattern="noise", scale=12, amount=0.12)
    rig = k.Rig("none")
    rig.bone("hull", None, (0, 0, 0.0), (0, -L / 2, 0), radius=3.0)
    h, station = ships.hull(k, p, L, B, DECK, bow_rise=0.8, stern_rise=2.2, keel=-0.9, fullness=0.5, band=(0.95, 0.3))
    parts = [h]
    # A gilded ram and eyes at the bow.
    parts.append(k.cyl(0.36, 2.6, gold, loc=(0, -L / 2 - 0.7, 0.2), rot=(math.radians(90), 0, 0), n=12, r2=0.04))
    for s in (-1, 1):
        w, _, d = station(0.9)
        parts.append(k.ball(0.3, p["white"], loc=(s * (w + 0.02), L / 2 - 0.9 * L, d - 0.55), scale=(0.25, 1, 0.7)))
        parts.append(k.ball(0.16, p["black"], loc=(s * (w + 0.1), L / 2 - 0.9 * L - 0.05, d - 0.55), scale=(0.3, 1, 1)))
    # The stern curls up into a gilded fan.
    post = [("ball", (0, L / 2 + 0.5 + 1.2 * math.sin(i / 8 * math.radians(210)), 3.9 + 1.4 * (1 - math.cos(i / 8 * math.radians(210)))), 0.28 - 0.02 * i) for i in range(9)]
    post += [("capsule", a[1], b[1], min(a[2], b[2])) for a, b in zip(post, post[1:])]
    parts.append(k.blobs("stern_post", post, gold, res=0.08, faces=1000))
    # Painted shields along both sides, every other one gilded.
    for s in (-1, 1):
        for i in range(8):
            t = 0.25 + i * 0.07
            w, _, d = station(t)
            parts.append(k.cyl(0.5, 0.08, p["team_paint"] if i % 2 else gold, loc=(s * (w + 0.05), L / 2 - t * L, d + 0.35), rot=(0, math.radians(90), 0), n=16))
    # Steering oars.
    for s in (-1, 1):
        w, _, d = station(0.1)
        parts.append(k.rod((s * w, L / 2 - 0.1 * L, d + 0.8), (s * (w + 0.9), L / 2 + 1.1, -0.3), 0.11, p["dark_wood"]))
    parts.append(k.box((1.0, L * 0.5, 0.22), p["dark_wood"], loc=(0, -0.8, DECK + 0.11), bevel=0.05))
    # The command deck: raised at the stern, railed, under a canopy.
    w, _, d = station(0.13)
    deck_y = L / 2 - 4.0
    parts.append(k.box((w * 1.8, 5.0, 0.9), p["planks"], loc=(0, deck_y, d + 0.15), bevel=0.06))
    parts.append(k.box((w * 1.85, 5.1, 0.18), p["team_paint"], loc=(0, deck_y, d + 0.6)))
    top = d + 0.7
    for s in (-1, 1):
        parts.append(k.box((0.1, 4.8, 0.45), gold, loc=(s * w * 0.9, deck_y, top + 0.25)))
    parts += canopy(k, p, gold, (0, deck_y, top), 2.8, 3.6, 2.6)
    body, J = people.figure(k, p, rig, parent="hull", at=(0, deck_y - 0.6, top), arms=("hang", "low_fwd"), outfit={"sleeves": "long", "skirt": 0.25, "beard": True, "tunic": p["team"]})
    parts += body
    parts += people.helmet(k, p, J, "crest", color=gold)
    # Masts and sails.
    tops = []
    for y, hgt, ln in ((-2.0, 11.5, 11.0), (-8.0, 7.5, 7.0)):
        parts += ships.mast(k, p, (0, y, DECK), hgt + 0.8, 0.2)
        parts += ships.lateen(k, p, (0.35, y, DECK + 1.4), hgt, ln, p["team"])
        tops.append(Vector((0, y, DECK + hgt + 0.8)))
        parts.append(k.ball(0.3, gold, loc=tops[-1] + Vector((0, 0, 0.1))))
    for t in tops:
        parts.append(ships.stay(k, p, t, (0, -L / 2 + 1.4, DECK + 0.9)))
        parts.append(ships.stay(k, p, t, (0, deck_y - 2.4, top)))
    parts += ships.bulwark(k, p, station, L, 0.6, t0=0.22, t1=0.92)
    parts += ships.coil(k, p, (0.8, -5.0, DECK + 0.22), 0.4)
    parts += ships.coil(k, p, (-0.8, 2.5, DECK + 0.22), 0.4)
    # A standard on a gilded staff at the stern.
    staff_foot = Vector((0, L / 2 - 1.2, top))
    parts.append(k.rod(staff_foot, staff_foot + Vector((0, 0, 4.2)), 0.08, gold, n=6))
    k.bind(parts, "hull")
    parts += ships.oars(k, p, rig, station, L, [0.3 + i * 0.075 for i in range(7)], reach=3.4)
    parts += ships.flag(k, p, rig, tops[0] + Vector((0, 0, 0.2)), "flag", size=2.0)
    parts += ships.flag(k, p, rig, tops[1] + Vector((0, 0, 0.2)), "flag_fore", size=1.4)
    parts += ships.flag(k, p, rig, staff_foot + Vector((0, 0, 4.2)), "flag_stern", size=1.3)
    return parts, {"kind": "unit", "scale": 0.0165, "rig": rig}


def canopy(k, p, gold, centre, w, d, h):
    """A cloth canopy in the owner's colour on four gilded posts, fringed."""
    centre = Vector(centre)
    out = [k.rod(centre + Vector((x, y, 0)), centre + Vector((x, y, h)), 0.07, gold, n=6) for x in (-w / 2, w / 2) for y in (-d / 2, d / 2)]
    cloth = k.sheet("canopy", w + 0.5, d + 0.5, p["team"], nx=6, ny=6, bulge=0.25, thickness=0.05, sag=0.0)
    cloth.rotation_euler = (math.pi / 2, 0, 0)
    cloth.location = centre + Vector((0, 0, h + 0.1))
    out.append(cloth)
    for s in (-1, 1):
        out.append(k.box((w + 0.55, 0.06, 0.3), gold, loc=centre + Vector((0, s * (d + 0.5) / 2, h - 0.05))))
        out.append(k.box((0.06, d + 0.55, 0.3), gold, loc=centre + Vector((s * (w + 0.5) / 2, 0, h - 0.05))))
    return out
