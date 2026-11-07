"""Trees, shrubs and boulders for the forests: a near and a far (`_far`)
model of each species, foliage flagged so the plants' shader gives it
snow and autumn colours. Metres; exported at 0.03 (a spruce is ~0.55 game
units tall). Budgets are tight: there are tens of thousands of these."""

import math
import random

import bmesh
import bpy
from mathutils import Vector


def leaf(k, name, rgb, rough=0.85, amount=0.35, scale=3.0):
    return k.flag(k.mat(name, k.srgb(*rgb), rough, pattern="noise", scale=scale, amount=amount, color2=k.srgb(*[int(c * 0.55) for c in rgb])), "foliage")


def bark(k, name, rgb):
    return k.mat(name, k.srgb(*rgb), 0.9, pattern="grain", scale=4, amount=0.3)


def mesh_obj(name, bm, material, smooth=True):
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    o = bpy.data.objects.new(name, me)
    bpy.context.collection.objects.link(o)
    o.data.materials.append(material)
    if smooth:
        o.data.shade_smooth()
    return o


def trunk(k, material, base, top, r0, r1, n=6):
    return k.rod(base, top, r0, material, n=n, r2=r1)


def conifer_layer(bm, rng, z, r, h, n=9, droop=0.35):
    """A ragged, drooping cone of branches: a fan from the apex to a mid
    ring, out to a jagged rim, and an underside back to the trunk."""
    apex = bm.verts.new((0, 0, z + h))
    mid, rim = [], []
    for i in range(n):
        a = i / n * math.tau + rng.uniform(-0.15, 0.15)
        rr = r * rng.uniform(0.82, 1.15)
        mid.append(bm.verts.new((math.cos(a) * rr * 0.5, math.sin(a) * rr * 0.5, z + h * 0.45)))
        rim.append(bm.verts.new((math.cos(a) * rr, math.sin(a) * rr, z - droop * h * rng.uniform(0.2, 0.5))))
    under = bm.verts.new((0, 0, z + h * 0.08))
    for i in range(n):
        j = (i + 1) % n
        bm.faces.new((apex, mid[i], mid[j]))
        bm.faces.new((mid[i], rim[i], rim[j], mid[j]))
        bm.faces.new((under, rim[j], rim[i]))


def spruce(k, rng, needles, wood, far=False):
    if far:
        return [k.cyl(2.2, 17.0, needles, loc=(0, 0, 9.0), n=6, r2=0.2), trunk(k, wood, (0, 0, 0), (0, 0, 1.5), 0.35, 0.3, n=4)]
    bm = bmesh.new()
    layers = [(1.4, 3.3, 3.4), (3.3, 3.0, 3.2), (5.1, 2.7, 3.0), (6.9, 2.35, 2.8), (8.6, 2.0, 2.6), (10.2, 1.65, 2.4), (11.7, 1.3, 2.3), (13.1, 0.95, 2.2), (14.5, 0.6, 2.0), (15.8, 0.3, 1.6)]
    for z, r, h in layers:
        conifer_layer(bm, rng, z, r * 1.05, h, n=12 if r > 1 else 8)
    return [mesh_obj("spruce", bm, needles), trunk(k, wood, (0, 0, -0.2), (0, 0, 6.0), 0.42, 0.2, n=8)]


def crown(k, name, rng, centre, spread, count, size, material, flat=1.0, faces=None, res=None, subdiv=2, shades=None):
    """A broadleaf crown: leafy clumps (rounded, lumpy balls in a few
    shades) packed around `centre`, so light and shade break up like real
    foliage."""
    import bmesh as _bm

    centre = Vector(centre)
    mats = shades or [material]
    pts = [Vector((0, 0, spread * 0.25 * flat))]
    for _ in range(count - 1):
        d = Vector((rng.uniform(-1, 1), rng.uniform(-1, 1), rng.uniform(-0.5, 0.9) * flat))
        pts.append(d.normalized() * spread * rng.uniform(0.45, 0.95))
    out = []
    for n, q in enumerate(pts):
        bm = _bm.new()
        r = size * rng.uniform(0.75, 1.2)
        geom = _bm.ops.create_icosphere(bm, subdivisions=subdiv, radius=r)
        for v in geom["verts"]:
            v.co = v.co * rng.uniform(0.86, 1.1)
            v.co.z *= 0.82 * flat
            v.co += centre + q
        out.append(mesh_obj(f"{name}{n}", bm, mats[rng.randrange(len(mats))]))
    return out


def shades(k, name, rgb):
    """Three foliage materials around `rgb`, for clumps to vary."""
    return [leaf(k, f"{name}{i}", tuple(max(0, min(255, int(c * f))) for c in rgb)) for i, f in enumerate((0.88, 1.0, 1.12))]


def build(k):
    rng = random.Random(7)
    p = k.palette()
    k.flag(p["stone"], "bare")
    out = {}

    needles = leaf(k, "needles", (62, 104, 68))
    spruce_bark = bark(k, "spruce_bark", (84, 62, 44))
    out["spruce"] = spruce(k, rng, needles, spruce_bark)
    out["spruce_far"] = spruce(k, rng, needles, spruce_bark, far=True)

    # Scots pine: a tall bare reddish trunk, a few flat clumps up top.
    pine_bark = bark(k, "pine_bark", (132, 84, 54))
    pine_leaf = leaf(k, "pine_leaf", (46, 78, 46))
    t = [trunk(k, pine_bark, (0, 0, -0.2), (0.3, 0.2, 9.5), 0.38, 0.22), trunk(k, pine_bark, (0.3, 0.2, 9.5), (-0.2, 0.0, 13.0), 0.22, 0.12, n=5)]
    t.append(trunk(k, pine_bark, (0.25, 0.15, 10.0), (1.8, 0.4, 11.5), 0.12, 0.06, n=4))
    clumps = [((0.0, 0.0, 13.6), 2.4), ((1.9, 0.5, 11.9), 1.6), ((-1.4, -0.6, 12.3), 1.5)]
    for c, r in clumps:
        t += crown(k, "pine_crown", rng, c, r * 0.8, 5, r * 0.55, pine_leaf, flat=0.45, shades=shades(k, "pine_sh", (46, 78, 46)))
    out["pine"] = t
    out["pine_far"] = [trunk(k, pine_bark, (0, 0, 0), (0, 0, 10.5), 0.35, 0.2, n=4), k.ball(2.8, pine_leaf, loc=(0, 0, 12.6), scale=(1.1, 1.1, 0.5), subdiv=1)]

    # Birch: a slim white trunk, an airy tall crown.
    birch_bark = k.mat("birch_bark", k.srgb(226, 222, 210), 0.8, pattern="noise", scale=3, amount=0.5, color2=k.srgb(60, 56, 50))
    birch_leaf = leaf(k, "birch_leaf", (124, 160, 68))
    out["birch"] = [
        trunk(k, birch_bark, (0, 0, -0.2), (0.2, 0.1, 10.0), 0.26, 0.12),
        trunk(k, birch_bark, (0.1, 0.05, 6.0), (1.2, 0.4, 8.6), 0.1, 0.05, n=5),
        trunk(k, birch_bark, (0.15, 0.08, 7.0), (-1.0, -0.3, 9.4), 0.09, 0.04, n=5),
        *crown(k, "birch_crown", rng, (0.1, 0.0, 9.0), 2.2, 11, 1.2, birch_leaf, flat=1.6, shades=shades(k, "birch_sh", (124, 160, 68))),
    ]
    out["birch_far"] = [trunk(k, birch_bark, (0, 0, 0), (0, 0, 6.0), 0.24, 0.15, n=4), k.ball(2.4, birch_leaf, loc=(0, 0, 9.0), scale=(0.9, 0.9, 1.5), subdiv=1)]

    # Oak: a stout trunk forking into a broad, rounded crown.
    oak_bark = bark(k, "oak_bark", (80, 62, 46))
    oak_leaf = leaf(k, "oak_leaf", (70, 112, 42))
    out["oak"] = [
        trunk(k, oak_bark, (0, 0, -0.3), (0, 0, 4.5), 0.75, 0.5, n=7),
        trunk(k, oak_bark, (0, 0, 4.0), (1.6, 0.6, 7.5), 0.4, 0.2, n=5),
        trunk(k, oak_bark, (0, 0, 4.0), (-1.4, -0.5, 7.2), 0.38, 0.2, n=5),
        trunk(k, oak_bark, (0, 0, 5.0), (0.2, 1.8, 8.6), 0.3, 0.15, n=5),
        *crown(k, "oak_crown", rng, (0, 0, 8.4), 3.4, 12, 1.75, oak_leaf, flat=0.8, shades=shades(k, "oak_sh", (70, 112, 42))),
    ]
    out["oak_far"] = [trunk(k, oak_bark, (0, 0, 0), (0, 0, 5.0), 0.7, 0.5, n=4), k.ball(4.3, oak_leaf, loc=(0, 0, 8.2), scale=(1.0, 1.0, 0.8), subdiv=1)]

    # Tropical broadleaf: a taller trunk, a wide, lumpy, deep green crown.
    trop_bark = bark(k, "trop_bark", (110, 92, 70))
    trop_leaf = leaf(k, "trop_leaf", (56, 124, 36))
    out["tropical"] = [
        trunk(k, trop_bark, (0, 0, -0.3), (0.3, 0, 7.5), 0.6, 0.35, n=6),
        trunk(k, trop_bark, (0.3, 0, 6.5), (2.4, 0.8, 8.8), 0.25, 0.12, n=5),
        trunk(k, trop_bark, (0.3, 0, 6.8), (-1.8, -0.9, 9.0), 0.22, 0.12, n=5),
        *crown(k, "trop_crown", rng, (0.3, 0, 9.2), 3.6, 12, 1.6, trop_leaf, flat=0.6, shades=shades(k, "trop_sh", (56, 124, 36))),
    ]
    out["tropical_far"] = [trunk(k, trop_bark, (0, 0, 0), (0, 0, 7.0), 0.55, 0.35, n=4), k.ball(4.0, trop_leaf, loc=(0, 0, 9.0), scale=(1.0, 1.0, 0.65), subdiv=1)]

    # Rainforest giant: a tall pale trunk on buttress roots, a flat emergent crown.
    giant_bark = bark(k, "giant_bark", (150, 132, 104))
    giant_leaf = leaf(k, "giant_leaf", (36, 96, 36))
    g = [trunk(k, giant_bark, (0, 0, -0.3), (0, 0, 15.0), 0.75, 0.45, n=7)]
    for i in range(4):
        a = i / 4 * math.tau + 0.4
        g.append(k.rod((math.cos(a) * 1.7, math.sin(a) * 1.7, -0.2), (0, 0, 3.0), 0.22, giant_bark, n=4, r2=0.12))
    g += crown(k, "giant_crown", rng, (0, 0, 16.5), 4.8, 12, 2.1, giant_leaf, flat=0.4, shades=shades(k, "giant_sh", (36, 96, 36)))
    g += crown(k, "giant_low", rng, (1.8, 1.0, 10.5), 2.2, 4, 1.3, giant_leaf, flat=0.5, subdiv=1)
    for i in range(3):
        a = i / 3 * math.tau + 0.3
        g.append(trunk(k, giant_bark, (0, 0, 13.5), (math.cos(a) * 3.2, math.sin(a) * 3.2, 16.0), 0.25, 0.12, n=5))
    out["giant"] = g
    out["giant_far"] = [trunk(k, giant_bark, (0, 0, 0), (0, 0, 15.0), 0.7, 0.45, n=4), k.ball(5.5, giant_leaf, loc=(0, 0, 16.5), scale=(1.0, 1.0, 0.42), subdiv=1)]

    # Acacia: a forked, leaning trunk under a flat umbrella crown.
    acacia_bark = bark(k, "acacia_bark", (96, 76, 54))
    acacia_leaf = leaf(k, "acacia_leaf", (112, 130, 54))
    a = [trunk(k, acacia_bark, (0, 0, -0.2), (0.6, 0, 4.5), 0.35, 0.25, n=5)]
    a.append(trunk(k, acacia_bark, (0.6, 0, 4.4), (2.8, 0.6, 8.0), 0.22, 0.12, n=5))
    a.append(trunk(k, acacia_bark, (0.6, 0, 4.4), (-1.6, -0.5, 7.6), 0.2, 0.12, n=5))
    for c, r in (((2.4, 0.5, 8.6), 2.8), ((-1.4, -0.4, 8.2), 2.4), ((0.6, 0, 8.9), 2.0)):
        a += crown(k, "acacia_crown", rng, c, r, 5, r * 0.5, acacia_leaf, flat=0.3, subdiv=1, shades=shades(k, "acacia_sh", (112, 130, 54)))
    out["acacia"] = a
    out["acacia_far"] = [trunk(k, acacia_bark, (0, 0, 0), (0.4, 0, 7.8), 0.3, 0.2, n=4), k.cyl(4.5, 1.1, acacia_leaf, loc=(0.5, 0, 8.4), n=7)]

    # Palm: a curving ringed trunk, a crown of drooping fronds, coconuts.
    palm_bark = bark(k, "palm_bark", (138, 112, 76))
    palm_leaf = leaf(k, "palm_leaf", (56, 116, 44))
    pts = [Vector((0.9 * (i / 6) ** 2, 0, i / 6 * 11.0)) for i in range(7)]
    pal = [trunk(k, palm_bark, pts[i], pts[i + 1], 0.38 - 0.03 * i, 0.35 - 0.03 * i, n=6) for i in range(6)]
    top = pts[-1]
    for i in range(8):
        ang = i / 8 * math.tau + rng.uniform(-0.2, 0.2)
        d = Vector((math.cos(ang), math.sin(ang), 0))
        side = Vector((-d.y, d.x, 0))
        bm = bmesh.new()
        spine = [top + d * (s * 4.2) + Vector((0, 0, 1.2 * math.sin(s * 2.2) - 2.0 * s * s)) for s in (0, 0.25, 0.5, 0.75, 1.0)]
        widths = [0.25, 0.85, 0.95, 0.7, 0.1]
        left = [bm.verts.new(q + side * w_ - Vector((0, 0, 0.25 * w_))) for q, w_ in zip(spine, widths)]
        mid = [bm.verts.new(q) for q in spine]
        right = [bm.verts.new(q - side * w_ - Vector((0, 0, 0.25 * w_))) for q, w_ in zip(spine, widths)]
        for j in range(4):
            bm.faces.new((left[j], mid[j], mid[j + 1], left[j + 1]))
            bm.faces.new((mid[j], right[j], right[j + 1], mid[j + 1]))
        f = mesh_obj("frond", bm, palm_leaf)
        f.modifiers.new("s", "SOLIDIFY").thickness = 0.08
        pal.append(f)
    for i in range(3):
        ang = i / 3 * math.tau
        pal.append(k.ball(0.35, k.mat("coconut", k.srgb(96, 70, 40), 0.8), loc=top + Vector((math.cos(ang) * 0.45, math.sin(ang) * 0.45, -0.5)), subdiv=1))
    out["palm"] = pal
    out["palm_far"] = [trunk(k, palm_bark, (0, 0, 0), top, 0.35, 0.3, n=4), k.ball(3.6, palm_leaf, loc=top + Vector((0, 0, -0.3)), scale=(1.2, 1.2, 0.35), subdiv=1)]

    # Saguaro cactus: a ribbed column with two arms.
    cactus = k.mat("cactus", k.srgb(80, 118, 62), 0.7, pattern="streaks", scale=6, amount=0.25)
    c_el = [("capsule", (0, 0, 0.0), (0, 0, 6.2), 0.55), ("capsule", (0, 0, 2.6), (1.4, 0, 2.9), 0.36), ("capsule", (1.4, 0, 2.9), (1.45, 0, 4.6), 0.36), ("capsule", (0, 0, 3.4), (-1.2, 0.2, 3.6), 0.32), ("capsule", (-1.2, 0.2, 3.6), (-1.25, 0.2, 4.9), 0.32)]
    out["cactus"] = [k.blobs("cactus", c_el, cactus, res=0.12, faces=110)]
    out["cactus_far"] = [k.cyl(0.6, 6.4, cactus, loc=(0, 0, 3.2), n=5), k.cyl(0.38, 2.0, cactus, loc=(1.4, 0, 3.8), n=4)]

    # Shrubs: low clumps; dwarf shrubs of the tundra in russet and olive.
    shrub_leaf = leaf(k, "shrub_leaf", (94, 106, 54))
    out["shrub"] = crown(k, "shrub", rng, (0, 0, 1.0), 1.4, 6, 0.85, shrub_leaf, flat=0.7, subdiv=1, shades=shades(k, "shrub_sh", (94, 106, 54)))
    out["shrub_far"] = [k.ball(1.9, shrub_leaf, loc=(0, 0, 0.9), scale=(1.3, 1.1, 0.7), subdiv=1)]
    dwarf_a = leaf(k, "dwarf_leaf", (112, 84, 58))
    dwarf_b = leaf(k, "dwarf_leaf2", (96, 96, 62))
    out["dwarf_shrub"] = crown(k, "dwarf", rng, (0, 0, 0.4), 1.1, 5, 0.6, dwarf_a, flat=0.5, subdiv=1) + crown(k, "dwarf2", rng, (1.1, 0.4, 0.35), 0.8, 3, 0.5, dwarf_b, flat=0.5, subdiv=1)
    out["dwarf_shrub_far"] = [k.ball(1.3, dwarf_a, loc=(0, 0, 0.35), scale=(1.4, 1.2, 0.55), subdiv=1)]

    # Boulder: a weathered, faceted rock.
    rock = k.mat("boulder", k.srgb(126, 122, 116), 0.9, pattern="noise", scale=1.2, amount=0.45)
    bm = bmesh.new()
    bmesh.ops.create_icosphere(bm, subdivisions=2, radius=1.6)
    for v in bm.verts:
        v.co *= rng.uniform(0.82, 1.12)
        v.co.z *= 0.65
        v.co.x *= 1.3
    out["boulder"] = [mesh_obj("boulder", bm, rock, smooth=False)]
    out["boulder"][0].location.z = 0.5
    out["boulder_far"] = [k.ball(1.6, rock, loc=(0, 0, 0.5), scale=(1.3, 1.0, 0.65), subdiv=1)]
    return out, {"kind": "props", "scale": 0.03, "foliage_shade": 0.3, "ao_distance": 3.0}
