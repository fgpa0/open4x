"""Hulls, oars, masts and sails. Ships are built in metres facing -Y with
the waterline at z = 0; the rig's root is `hull` (it rolls on the swell),
each oar its own bone (`oar_l3`...), flags `flag*`."""

import math

from mathutils import Vector


def hull(k, p, L, B, deck, *, bow_rise=0.6, stern_rise=2.5, keel=-1.0, fullness=0.55, transom=False, band=(0.95, 0.3), name="hull", planks=None):
    """A lofted hull: tarred below the waterline, a band in the owner's
    colour under the gunwale, a planked deck. Returns (object, station),
    station(t) giving (half beam, keel, deck) at t (0 stern, 1 bow)."""

    def station(t):
        if transom:
            w = B / 2 * max(0.1, min(1.0, math.sin(math.pi * min(1.0, 0.18 + t * 0.95)) ** fullness * (1.0 if t > 0.05 else 0.85)))
        else:
            w = B / 2 * max(0.06, math.sin(math.pi * min(1.0, t * 1.06)) ** fullness)
        d = deck + stern_rise * max(0.0, 0.2 - t) / 0.2 + bow_rise * max(0.0, t - 0.82) / 0.18
        kz = keel + (abs(keel) + 0.4) * max(0.0, 0.1 - t) / 0.1 * (0.0 if transom else 1.0) + 0.5 * max(0.0, t - 0.88) / 0.12
        return w, kz, d

    rings = []
    for j in range(41):
        t = j / 40
        w, kz, d = station(t)
        y = L / 2 - t * L
        ring = []
        for i in range(17):
            th = -math.pi / 2 + math.pi * i / 16
            s = math.sin(th)
            ring.append((w * math.copysign(abs(s) ** 0.6, s), y, d - (d - kz) * math.cos(th) ** 1.4))
        rings.append(ring)
    planks = planks or k.mat("hull_planks", k.srgb(132, 96, 60), 0.75, pattern="grain", scale=1.6, amount=0.4)
    pitch = k.mat("pitch", k.srgb(36, 30, 26), 0.6, pattern="noise", scale=2, amount=0.3)
    deck_mat = k.mat("deck", k.srgb(170, 132, 88), 0.85, pattern="streaks", scale=3.0, amount=0.3)
    h = k.loft(name, rings, planks, closed=True, cap=True, smooth=0.9)

    def pick(c, nrm):
        _, _, d = station((L / 2 - c.y) / L)
        if nrm.z > 0.9 and c.z > d - 0.05:
            return 3
        if c.z < 0.2:
            return 1
        if d - band[0] < c.z < d - band[1]:
            return 2
        return 0

    k.paint(h, [planks, pitch, p["team_paint"], deck_mat], pick)
    return h, station


def oars(k, p, rig, station, L, ts, *, z_off=-0.25, reach=3.8, blade=1.2, name="oar", parent="hull", r=0.08):
    """A bank of oars down both sides at stations `ts`, each its own bone."""
    out = []
    for s, n in ((1, "l"), (-1, "r")):
        for i, t in enumerate(ts):
            w, _, d = station(t)
            y = L / 2 - t * L
            lock = Vector((s * w, y, d + z_off))
            bone = f"{name}_{n}{i}"
            rig.bone(bone, parent, lock)
            a = lock - Vector((s * reach * 0.24, -0.3 * reach / 3.8, -0.25 * reach / 3.8))
            b = Vector((s * (w + reach), y - 0.4, 0.3))
            o = [k.rod(a, b, r, p["wood"], n=6)]
            bl = k.box((r * 1.25, blade * 0.33, blade), p["wood"], loc=b + Vector((s * blade * 0.2, -0.05, -blade * 0.12)), rot=(0, s * 0.95, 0), bevel=r * 0.5)
            o.append(bl)
            k.bind(o, bone)
            out += o
    return out


def mast(k, p, base, height, r=0.22):
    base = Vector(base)
    return [k.rod(base, base + Vector((0, 0, height)), r, p["dark_wood"], n=10, r2=r * 0.6)]


def square_sail(k, p, centre, w, h, material, bulge=0.9, yard=True, yard_r=0.12):
    """A square sail hanging from a yard at the top of (centre, w, h)."""
    centre = Vector(centre)
    out = []
    s = k.sheet("sail", w, h, material, nx=12, ny=10, bulge=bulge, thickness=0.05)
    s.location = centre
    out.append(s)
    if yard:
        out.append(k.rod(centre + Vector((-w / 2 - 0.4 * yard_r / 0.12, 0.1, h / 2)), centre + Vector((w / 2 + 0.4 * yard_r / 0.12, 0.1, h / 2)), yard_r, p["dark_wood"], n=8))
    return out


def lateen(k, p, foot, height, length, material, lean=0.55, cross=None):
    """A triangular sail on a long slanted yard, along the ship (Y), with
    an upright cross painted on it in `cross` (a material) if given."""
    import bmesh
    import bpy

    foot = Vector(foot)
    top = foot + Vector((0, length * 0.45, height))
    low = foot + Vector((0, -length * 0.5, height * 0.12))
    clew = foot + Vector((0, length * 0.32, height * 0.05))
    out = [k.rod(low - (top - low) * 0.05, top + (top - low) * 0.05, 0.1, p["dark_wood"], n=8)]
    bm = bmesh.new()
    n = 22
    grid = []
    for j in range(n + 1):
        row = []
        for i in range(n + 1 - j):
            u, v = i / n, j / n
            pnt = low + (top - low) * u + (clew - low) * v
            pnt += Vector((0.6 * math.sin(math.pi * u) * math.sin(math.pi * min(1.0, v * 1.5 + 0.1)), 0, 0))
            row.append(bm.verts.new(pnt))
        grid.append(row)
    for j in range(n):
        for i in range(n - j):
            bm.faces.new((grid[j][i], grid[j][i + 1], grid[j + 1][i]))
            if i + 1 < n - j:
                bm.faces.new((grid[j][i + 1], grid[j + 1][i + 1], grid[j + 1][i]))
    me = bpy.data.meshes.new("lateen")
    bm.to_mesh(me)
    bm.free()
    o = bpy.data.objects.new("lateen", me)
    bpy.context.collection.objects.link(o)
    o.data.materials.append(material)
    o.data.shade_smooth()
    mod = o.modifiers.new("s", "SOLIDIFY")
    mod.thickness = 0.05
    if cross:
        # The cross as its own thin sheet following the sail's belly, on
        # both faces: map its (y, z) corners back to the sail's (u, v).
        e1, e2 = top - low, clew - low
        det = e1.y * e2.z - e1.z * e2.y

        def on_sail(y, z, side):
            dy, dz = y - low.y, z - low.z
            u = (dy * e2.z - dz * e2.y) / det
            v = (e1.y * dz - e1.z * dy) / det
            q = low + e1 * u + e2 * v
            bulge = 0.6 * math.sin(math.pi * u) * math.sin(math.pi * min(1.0, v * 1.5 + 0.1))
            return (q.x + bulge + (0.12 if side > 0 else -0.17), y, z)

        c = low * 0.3 + top * 0.38 + clew * 0.32
        arm, w_ = height * 0.22, height * 0.045
        for side in (-1, 1):
            for (y0, y1, z0, z1) in ((c.y - w_, c.y + w_, c.z - arm, c.z + arm), (c.y - arm * 0.7, c.y + arm * 0.7, c.z + arm * 0.25 - w_, c.z + arm * 0.25 + w_)):
                bm = bmesh.new()
                nx, nz = 8, 8
                vs = [[bm.verts.new(on_sail(y0 + (y1 - y0) * i / nx, z0 + (z1 - z0) * j / nz, side)) for i in range(nx + 1)] for j in range(nz + 1)]
                for j in range(nz):
                    for i in range(nx):
                        f = (vs[j][i], vs[j][i + 1], vs[j + 1][i + 1], vs[j + 1][i])
                        bm.faces.new(f if side > 0 else tuple(reversed(f)))
                me = bpy.data.meshes.new("cross")
                bm.to_mesh(me)
                bm.free()
                co = bpy.data.objects.new("cross", me)
                bpy.context.collection.objects.link(co)
                co.data.materials.append(cross)
                co.data.shade_smooth()
                out.append(co)
    out.append(o)
    return out


def bulwark(k, p, station, L, height, t0=0.03, t1=0.97, thick=0.2, material=None, cap=None, ends=False):
    """A raised wall along both sides of the deck with a cap rail, so the
    deck sits inside the hull rather than flush with its edge. With `ends`,
    walls across the stern and bow close it all round."""
    out = []
    if ends:
        for t in (t0, t1):
            w, _, d = station(t)
            y = L / 2 - t * L
            out.append(k.box((w * 2 + 0.02, thick, height + 0.15), material or p["planks"], loc=(0, y, d + (height - 0.15) / 2)))
            out.append(k.box((w * 2 + 0.14, thick + 0.12, 0.12), cap or p["dark_wood"], loc=(0, y, d + height + 0.06)))
    for s in (-1, 1):
        rings, rails = [], []
        for j in range(33):
            t = t0 + (t1 - t0) * j / 32
            w, _, d = station(t)
            y = L / 2 - t * L
            wi = max(0.02, w - thick)
            rings.append([(s * w, y, d - 0.15), (s * w, y, d + height), (s * wi, y, d + height), (s * wi, y, d - 0.15)])
            rails.append([(s * (w + 0.06), y, d + height), (s * (w + 0.06), y, d + height + 0.12), (s * (wi - 0.06), y, d + height + 0.12), (s * (wi - 0.06), y, d + height)])
        out.append(k.loft("bulwark", rings, material or p["planks"], closed=True, cap=True, smooth=0.6))
        out.append(k.loft("rail", rails, cap or p["dark_wood"], closed=True, cap=True, smooth=0.6))
    return out


def barrel(k, p, at, r=0.45, h=1.0, lying=False):
    at = Vector(at)
    rot = (math.pi / 2, 0, 0) if lying else (0, 0, 0)
    o = [k.cyl(r, h, p["wood"], loc=at + Vector((0, 0, r if lying else h / 2)), rot=rot, n=12, smooth=0.6)]
    for f in (-0.32, 0.32):
        off = Vector((0, f * h, r)) if lying else Vector((0, 0, h / 2 + f * h))
        o.append(k.cyl(r * 1.04, 0.07, p["iron"], loc=at + off, rot=rot, n=12))
    return o


def crate(k, p, at, size=0.9, yaw=0.0):
    at = Vector(at)
    return [k.box((size, size, size), p["planks"], loc=at + Vector((0, 0, size / 2)), rot=(0, 0, yaw), bevel=0.04)]


def coil(k, p, at, r=0.5):
    """A coil of rope lying on the deck."""
    at = Vector(at)
    out = []
    for i, rr in enumerate((r, r * 0.75, r * 0.5)):
        ring = [[at + Vector((math.cos(a) * (rr + 0.08 * math.cos(b)), math.sin(a) * (rr + 0.08 * math.cos(b)), 0.08 + 0.08 * math.sin(b) + i * 0.1)) for b in [j / 6 * math.tau for j in range(6)]] for a in [q / 16 * math.tau for q in range(17)]]
        out.append(k.loft("coil", ring, p["rope"], closed=True, cap=False))
    return out


def hatch(k, p, at, w=2.0, d=1.6):
    at = Vector(at)
    out = [k.box((w + 0.3, d + 0.3, 0.4), p["dark_wood"], loc=at + Vector((0, 0, 0.2)), bevel=0.04)]
    out.append(k.box((w, d, 0.1), p["black"], loc=at + Vector((0, 0, 0.4))))
    for i in range(5):
        out.append(k.box((w, 0.08, 0.06), p["wood"], loc=at + Vector((0, -d / 2 + (i + 0.5) * d / 5, 0.44))))
        out.append(k.box((0.08, d, 0.06), p["wood"], loc=at + Vector((-w / 2 + (i + 0.5) * w / 5, 0, 0.44))))
    return out


def capstan(k, p, at):
    at = Vector(at)
    out = [k.cyl(0.45, 1.1, p["dark_wood"], loc=at + Vector((0, 0, 0.55)), n=10, r2=0.35)]
    out.append(k.cyl(0.55, 0.15, p["dark_wood"], loc=at + Vector((0, 0, 1.0)), n=10))
    for i in range(4):
        a = i / 4 * math.pi
        d = Vector((math.cos(a), math.sin(a), 0))
        out.append(k.rod(at + Vector((0, 0, 1.0)) - d * 1.3, at + Vector((0, 0, 1.0)) + d * 1.3, 0.06, p["wood"], n=5))
    return out


def ship_boat(k, p, at, length=4.0, yaw=0.0):
    """A small boat stowed upside down on deck."""
    from mathutils import Euler

    at = Vector(at)
    rings = []
    for j in range(9):
        t = j / 8
        w = 0.8 * math.sin(math.pi * (0.06 + 0.88 * t)) ** 0.6
        rings.append([(math.cos(a) * w, (t - 0.5) * length, 0.55 - math.sin(a) * 0.5) for a in [i / 10 * math.pi for i in range(11)]])
    o = k.loft("boat", rings, p["planks"], closed=False, cap=False, smooth=0.6)
    o.modifiers.new("s", "SOLIDIFY").thickness = 0.06
    out = [o]
    k.pivot(out, (0, 0, 0), Euler((0, 0, yaw)))
    for x in out:
        x.location += at
    return out


def deck_gun(k, p, at, side):
    """A cannon on a carriage at the bulwark, pointing out to `side`."""
    at = Vector(at)
    out = [k.box((1.2, 0.9, 0.5), p["dark_wood"], loc=at + Vector((0, 0, 0.35)))]
    out.append(k.rod(at + Vector((-side * 0.5, 0, 0.7)), at + Vector((side * 1.5, 0, 0.75)), 0.18, p["iron"], n=10, r2=0.13))
    return out


def flag(k, p, rig, top, name, size=1.6, parent="hull"):
    """A long tapering pennant in the owner's colour streaming back (+Y)
    from a masthead, waving, on its own bone."""
    import bmesh
    import bpy

    top = Vector(top)
    rig.bone(name, parent, top)
    bm = bmesh.new()
    n = 10
    length, h = size * 2.4, size * 0.75
    rows = []
    for i in range(n + 1):
        t = i / n
        y = top.y + t * length
        x = top.x + math.sin(t * 5.0) * 0.12 * size * t
        half = h * (1.0 - 0.75 * t) / 2
        z = top.z - h / 2 - t * size * 0.35
        rows.append((bm.verts.new((x, y, z + half)), bm.verts.new((x, y, z - half))))
    for (a, b), (c, d) in zip(rows, rows[1:]):
        bm.faces.new((a, b, d, c))
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    f = bpy.data.objects.new(name, me)
    bpy.context.collection.objects.link(f)
    f.data.materials.append(p["team"])
    f.data.shade_smooth()
    f.modifiers.new("s", "SOLIDIFY").thickness = 0.04
    k.bind(f, name)
    return [f]


def stay(k, p, a, b, r=0.05):
    return k.rod(a, b, r, p["rope"], n=5)
