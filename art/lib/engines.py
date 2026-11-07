"""Wheels and timber for carts and siege engines."""

import math

from mathutils import Vector


def wheel(k, p, centre, r, bone, spokes=8, width=0.08, tyre=None):
    """A spoked wheel turning about X, bound to `bone`."""
    centre = Vector(centre)
    rot = (0, math.pi / 2, 0)
    out = [rim(k, centre, r, r * 0.84, width, tyre or p["dark_wood"])]
    for i in range(spokes):
        a = i / spokes * math.tau
        out.append(k.rod(centre, centre + Vector((0, math.cos(a), math.sin(a))) * r * 0.88, r * 0.045, p["wood"], n=5))
    out.append(k.cyl(r * 0.18, width * 2.4, p["dark_wood"], loc=centre, rot=rot, n=10))
    k.bind(out, bone)
    return out


def beam(k, p, a, b, size=0.12, mat=None):
    """A squared timber from a to b."""
    a, b = Vector(a), Vector(b)
    o = k.box((size, size, (b - a).length), mat or p["wood"], loc=(a + b) / 2, bevel=size * 0.12)
    o.rotation_mode = "QUATERNION"
    o.rotation_quaternion = (b - a).normalized().to_track_quat("Z", "Y")
    return o


def rim(k, centre, outer, inner, width, material, n=28):
    """A ring turning about X: a wheel's rim."""
    centre = Vector(centre)
    rings = []
    for x in (-width / 2, width / 2):
        for rad in ((outer, inner) if x < 0 else (inner, outer)):
            rings.append([centre + Vector((x, math.cos(a) * rad, math.sin(a) * rad)) for a in [i / n * math.tau for i in range(n)]])
    return k.loft("rim", rings + [rings[0]], material, closed=True, cap=False, smooth=0.6)
