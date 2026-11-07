"""Bridges in three eras' styles: Classical (round Roman arches in warm
travertine), Medieval (pointed arches, heavy cutwaters with refuges and a
chapel on the middle pier) and Renaissance (wide flat arches in pale
dressed stone, balustrades, lamps and an obelisk).

Each style is two props the game puts together on a bridge's tile
(improvements.rs): an arm, running along +X from the tile's centre (x = 0)
to the middle of a shore edge, and a hub, the round middle pier where two
arms meet (straight across, or at an angle). The hub's showpiece stands on
its +Y side. Units are game units x 100 (exported at 0.01): the water is at
z = 0, and the deck's height must stay exactly `deck` below, which
client/src/bridges.rs mirrors so units walk on it."""

import math

import bmesh
import bpy

# The arm's length (centre to edge midpoint), the deck's height at the
# middle and at the shore, and its width. Keep in step with bridges.rs.
ARM = 86.6
TOP = 36.0
END = 5.0
WIDTH = 30.0
# The hub's radius, and where the footings go down to.
HUB = 15.0
FOOT = -8.0
# Half the masonry's width below the deck, and the parapets' height.
BODY = 13.0
PARAPET = 4.5
# The arches along an arm: the big one by the hub for boats, a small one
# by the shore; the pier between them.
BIG = (HUB, 52.0)
SMALL = (60.0, 79.0)


def deck(x):
    """The deck's surface height at distance `x` from the centre."""
    u = min(max(abs(x) / ARM, 0.0), 1.0)
    return END + (TOP - END) * max(math.cos(math.pi / 2 * u), 0.0) ** 0.6


def arch_height(style, a, b):
    """Springing and crown heights of an arch from `a` to `b` in `style`."""
    half = (b - a) / 2
    if style == "renaissance":
        return 7.0, 7.0 + half * 0.62
    if style == "medieval":
        r = (b - a) * 0.7
        return 2.0, 2.0 + math.sqrt(r * r - (r - half) ** 2)
    return 2.0, 2.0 + half


def soffit(style, a, b, x):
    """The underside of an arch from `a` to `b` at `x`: round, pointed or
    flat (an ellipse's top)."""
    half = (b - a) / 2
    mid = (a + b) / 2
    spring, crown = arch_height(style, a, b)
    d = abs(x - mid) / half
    if d >= 1.0:
        return spring
    if style == "medieval":
        # Two arcs of radius r meeting at a point over the middle.
        r = (b - a) * 0.7
        dist = half - abs(x - mid)  # from the springing on this side
        return spring + math.sqrt(max(r * r - (r - dist) ** 2, 0.0))
    return spring + (crown - spring) * math.sqrt(1.0 - d * d)


def mesh_obj(name, bm, material):
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    o = bpy.data.objects.new(name, me)
    bpy.context.collection.objects.link(o)
    o.data.materials.append(material)
    o.data.shade_flat()
    return o


def prism(k, name, outline, y0, y1, material):
    """A slab with the outline (x, z) points, from y0 to y1."""
    rings = [[(x, y, z) for x, z in outline] for y in (y0, y1)]
    return k.loft(name, rings, material, closed=True, cap=True, smooth=None)


def along(k, name, x0, x1, section, material, n=24):
    """A strip along the deck from x0 to x1: `section(x)` gives its
    cross-section as (y, z) points."""
    rings = []
    for i in range(n + 1):
        x = x0 + (x1 - x0) * i / n
        rings.append([(x, y, z) for y, z in section(x)])
    return k.loft(name, rings, material, closed=True, cap=True, smooth=None)


def body_outline(style, steps=64):
    """The masonry's side view: under the deck, down to the footings, with
    the arches cut out."""
    top = [(ARM * i / steps, deck(ARM * i / steps) - 1.5) for i in range(steps + 1)]
    bottom = [(ARM, FOOT)]
    for a, b in (SMALL, BIG):
        bottom.append((b, FOOT))
        for i in range(25):
            x = b - (b - a) * i / 24
            bottom.append((x, soffit(style, a, b, x)))
        bottom.append((a, FOOT))
    bottom.append((0.0, FOOT))
    return top + bottom


def voussoirs(k, style, a, b, mats, depth=4.2, count=17, proud=0.7):
    """The ring of wedge stones round an arch, on both faces."""
    out = []
    pts = [(a + (b - a) * i / (count * 4), 0.0) for i in range(count * 4 + 1)]
    pts = [(x, soffit(style, a, b, x)) for x, _ in pts]
    for i in range(count):
        (x0, z0), (x1, z1) = pts[i * 4], pts[i * 4 + 4]
        mx, mz = (x0 + x1) / 2, (z0 + z1) / 2
        # Outward from the arch's middle, so each stone points at it.
        ang = math.atan2(z1 - z0, x1 - x0)
        nx, nz = -math.sin(ang), math.cos(ang)
        if nz < 0:
            nx, nz = -nx, -nz
        length = math.hypot(x1 - x0, z1 - z0) * 0.92
        for s in (-1, 1):
            out.append(k.box((length, proud * 2, depth), mats[i % 2], loc=(mx + nx * depth / 2, s * BODY, mz + nz * depth / 2), rot=(0, -ang, 0)))
    return out


def cutwater(k, x, half, height, material, y_out=BODY + 6.0):
    """Pointed breakwaters on both faces of a pier `half` long."""
    out = []
    for s in (-1, 1):
        bm = bmesh.new()
        ring = [(x - half, s * BODY), (x, s * y_out), (x + half, s * BODY)]
        lo = [bm.verts.new((px, py, FOOT)) for px, py in ring]
        hi = [bm.verts.new((px, py, height)) for px, py in ring]
        bm.faces.new(lo[::-1] if s > 0 else lo)
        bm.faces.new(hi if s > 0 else hi[::-1])
        for i in range(2):
            bm.faces.new((lo[i], lo[i + 1], hi[i + 1], hi[i]))
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        out.append(mesh_obj("cutwater", bm, material))
    return out


def arm(k, style, m):
    """One arm in `style`, materials in `m`."""
    out = [prism(k, "body", body_outline(style), -BODY, BODY, m["stone"])]
    # The roadway, between the parapets.
    w = WIDTH / 2 - 1.6
    out.append(along(k, "road", 0.0, ARM, lambda x: [(-w, deck(x) - 1.6), (w, deck(x) - 1.6), (w, deck(x)), (-w, deck(x))], m["road"], n=40))
    # A string course under the parapets, standing out from the walls.
    out.append(along(k, "course", HUB, ARM, lambda x: [(-WIDTH / 2, deck(x) - 3.4), (WIDTH / 2, deck(x) - 3.4), (WIDTH / 2, deck(x) - 1.2), (-WIDTH / 2, deck(x) - 1.2)], m["trim"], n=40))
    for a, b in (BIG, SMALL):
        out += voussoirs(k, style, a, b, (m["trim"], m["stone2"]), depth=4.6 if style != "renaissance" else 3.6)
    # The pier between the arches, and its cutwaters.
    px = (BIG[1] + SMALL[0]) / 2
    half = (SMALL[0] - BIG[1]) / 2
    if style == "medieval":
        # Cutwaters rising to the deck, where they make refuges.
        h = deck(px)
        out += cutwater(k, px, half + 1.0, h - 1.2, m["stone2"], y_out=BODY + 8.0)
        for s in (-1, 1):
            for i in range(3):
                t = (i - 1) * (half * 0.8)
                out.append(k.box((2.6, 1.8, PARAPET), m["trim"], loc=(px + t, s * (BODY + 7.0 - abs(i - 1) * 3.5), h + PARAPET / 2 - 0.4)))
    else:
        out += cutwater(k, px, half, arch_height(style, *SMALL)[0] + 2.0, m["stone2"])
    if style == "classical":
        # A small flood arch through the spandrel over the pier, as on Roman bridges.
        for s in (-1, 1):
            out.append(k.box((3.4, 1.4, 6.0), m["dark"], loc=(px, s * (BODY + 0.1), deck(px) - 9.0)))
            out.append(k.cyl(1.7, 1.4, m["dark"], loc=(px, s * (BODY + 0.1), deck(px) - 6.0), rot=(math.pi / 2, 0, 0), n=10))
    if style == "renaissance":
        # A round window over the pier, framed in a ring of stone.
        for s in (-1, 1):
            out.append(k.cyl(3.2, 1.2, m["trim"], loc=(px, s * (BODY + 0.3), deck(px) - 10.0), rot=(math.pi / 2, 0, 0), n=16))
            out.append(k.cyl(2.2, 1.4, m["dark"], loc=(px, s * (BODY + 0.35), deck(px) - 10.0), rot=(math.pi / 2, 0, 0), n=16))
    # The abutment's wing walls, splaying out along the bank.
    for s in (-1, 1):
        out.append(k.box((9.0, 3.0, deck(ARM) - FOOT + 2.0), m["stone"], loc=(ARM - 3.0, s * (BODY + 2.5), (deck(ARM) + FOOT) / 2 + 1.0), rot=(0, 0, s * 0.35)))
    # Parapets (or balustrades) from the hub to the shore.
    for s in (-1, 1):
        y0, y1 = s * (WIDTH / 2 - 1.6), s * (WIDTH / 2)
        lo, hi = min(y0, y1), max(y0, y1)
        if style == "renaissance":
            out.append(along(k, "plinth", HUB, ARM, lambda x: [(lo, deck(x) - 0.2), (hi, deck(x) - 0.2), (hi, deck(x) + 1.0), (lo, deck(x) + 1.0)], m["trim"], n=32))
            out.append(along(k, "rail", HUB, ARM, lambda x: [(lo - 0.2, deck(x) + PARAPET - 1.0), (hi + 0.2, deck(x) + PARAPET - 1.0), (hi + 0.2, deck(x) + PARAPET), (lo - 0.2, deck(x) + PARAPET)], m["trim"], n=32))
            n = 26
            for i in range(n):
                x = HUB + 1.5 + (ARM - HUB - 3.0) * i / (n - 1)
                out.append(k.cyl(0.55, PARAPET - 2.0, m["stone"], loc=(x, (lo + hi) / 2, deck(x) + PARAPET / 2), n=6, r2=0.4))
            # Lamps on the parapets over the pier.
            out.append(k.box((2.2, 2.2, PARAPET + 1.0), m["trim"], loc=(px, (lo + hi) / 2, deck(px) + PARAPET / 2)))
            out.append(k.cyl(0.35, 8.0, m["iron"], loc=(px, (lo + hi) / 2, deck(px) + PARAPET + 4.5), n=6))
            out.append(k.box((1.6, 1.6, 2.0), m["lamp"], loc=(px, (lo + hi) / 2, deck(px) + PARAPET + 9.0)))
            out.append(k.cyl(1.4, 0.8, m["iron"], loc=(px, (lo + hi) / 2, deck(px) + PARAPET + 10.4), n=6, r2=0.2))
        else:
            out.append(along(k, "parapet", HUB, ARM, lambda x: [(lo, deck(x) - 0.2), (hi, deck(x) - 0.2), (hi, deck(x) + PARAPET - 0.8), (lo, deck(x) + PARAPET - 0.8)], m["stone"], n=32))
            out.append(along(k, "coping", HUB, ARM, lambda x: [(lo - 0.4, deck(x) + PARAPET - 0.8), (hi + 0.4, deck(x) + PARAPET - 0.8), (hi + 0.4, deck(x) + PARAPET), (lo - 0.4, deck(x) + PARAPET)], m["trim"], n=32))
    # Bollards where the bridge meets the road ashore.
    for s in (-1, 1):
        out.append(k.box((3.0, 3.0, PARAPET + 1.6), m["trim"], loc=(ARM - 1.5, s * (WIDTH / 2 - 0.8), deck(ARM) + PARAPET / 2)))
    return out


def hub(k, style, m):
    """The round middle pier where arms meet, topped with the era's
    showpiece on its +Y side."""
    out = [k.cyl(HUB, TOP - 1.6 - FOOT, m["stone"], loc=(0, 0, (TOP - 1.6 + FOOT) / 2), n=24, smooth=None)]
    out.append(k.cyl(HUB - 0.4, 1.6, m["road"], loc=(0, 0, TOP - 0.8), n=24, smooth=None))
    out.append(k.cyl(HUB + 1.0, 2.2, m["trim"], loc=(0, 0, TOP - 2.3), n=24, smooth=None))
    # Cutwaters at the waterline, both ways along +Y and -Y.
    for s in (-1, 1):
        out.append(k.box((HUB * 1.1, HUB * 1.1, 10.0), m["stone2"], loc=(0, s * HUB * 0.55, FOOT + 5.0 + 4.0), rot=(0, 0, math.pi / 4)))
    if style == "classical":
        # An honorific column with a bronze eagle.
        y = HUB - 3.5
        out.append(k.box((5.0, 5.0, 3.0), m["trim"], loc=(0, y, TOP + 1.5)))
        out.append(k.cyl(1.6, 18.0, m["stone"], loc=(0, y, TOP + 12.0), n=12))
        out.append(k.box((4.4, 4.4, 1.4), m["trim"], loc=(0, y, TOP + 21.7)))
        out.append(k.ball(1.6, m["bronze"], loc=(0, y, TOP + 24.0), scale=(1.0, 1.3, 1.0)))
        for s in (-1, 1):
            out.append(k.box((4.0, 1.2, 0.5), m["bronze"], loc=(s * 2.4, y, TOP + 25.0), rot=(0, s * -0.5, 0)))
    elif style == "medieval":
        # A chapel built out over the cutwater, with a bell turret.
        y = HUB + 3.0
        w, d, h = 12.0, 11.0, 11.0
        out.append(k.box((w, d, h), m["stone2"], loc=(0, y, TOP + h / 2 - 0.4)))
        rings = []
        for x in (-w / 2 - 0.8, w / 2 + 0.8):
            rings.append([(x, y - d / 2 - 0.8, TOP + h - 0.8), (x, y, TOP + h + 6.0), (x, y + d / 2 + 0.8, TOP + h - 0.8), (x, y, TOP + h + 5.0)])
        out.append(k.loft("chapel_roof", rings, m["slate"], closed=True, cap=True, smooth=None))
        out.append(k.box((3.0, 3.0, 4.0), m["stone2"], loc=(w / 2 - 2.0, y, TOP + h + 4.0)))
        out.append(k.cyl(2.2, 4.0, m["slate"], loc=(w / 2 - 2.0, y, TOP + h + 8.0), n=4, r2=0.1, rot=(0, 0, math.pi / 4)))
        # A pointed door facing the deck, and a support under the overhang.
        out.append(k.box((3.0, 0.6, 5.0), m["dark"], loc=(0, y - d / 2 - 0.1, TOP + 2.5)))
        out.append(k.box((w * 0.8, 6.0, 6.0), m["stone2"], loc=(0, y + d / 2 - 2.0, TOP - 4.0), rot=(0.6, 0, 0)))
    else:
        # A balustraded bay with an obelisk on a pedestal.
        y = HUB - 4.0
        out.append(k.box((6.0, 6.0, 4.0), m["trim"], loc=(0, y, TOP + 2.0)))
        out.append(k.cyl(2.2, 16.0, m["stone"], loc=(0, y, TOP + 12.0), n=4, r2=0.9, rot=(0, 0, math.pi / 4)))
        out.append(k.cyl(1.0, 2.6, m["bronze"], loc=(0, y, TOP + 21.3), n=4, r2=0.05, rot=(0, 0, math.pi / 4)))
        for i in range(9):
            a = math.pi * (0.1 + 0.8 * i / 8)
            out.append(k.cyl(0.55, PARAPET - 1.0, m["stone"], loc=(math.cos(a) * (HUB - 0.8), math.sin(a) * (HUB - 0.8), TOP + PARAPET / 2), n=6, r2=0.4))
    return out


def build(k):
    styles = {
        "classical": dict(stone=(204, 190, 160), stone2=(186, 172, 144), trim=(222, 210, 184), road=(150, 138, 120)),
        "medieval": dict(stone=(146, 140, 130), stone2=(126, 120, 112), trim=(166, 160, 150), road=(120, 110, 98)),
        "renaissance": dict(stone=(222, 214, 196), stone2=(200, 192, 174), trim=(236, 230, 216), road=(140, 130, 118)),
    }
    out = {}
    for style, c in styles.items():
        m = {
            name: k.mat(f"{style}_{name}", k.srgb(*rgb), 0.85, pattern="noise", scale=0.4, amount=0.3)
            for name, rgb in c.items()
        }
        m["dark"] = k.mat("bridge_dark", k.srgb(24, 22, 20), 0.9)
        m["bronze"] = k.mat("bridge_bronze", k.srgb(96, 120, 92), 0.4, metal=0.6)
        m["iron"] = k.mat("bridge_iron", k.srgb(40, 40, 42), 0.5, metal=0.8)
        m["lamp"] = k.mat("bridge_lamp", k.srgb(250, 214, 140), 0.4)
        m["slate"] = k.mat("bridge_slate", k.srgb(78, 76, 84), 0.8, pattern="noise", scale=0.6, amount=0.25)
        out[f"bridge_{style}_arm"] = (arm(k, style, m), 0.01)
        out[f"bridge_{style}_hub"] = (hub(k, style, m), 0.01)
    return out, {"kind": "props", "scale": 1.0, "ao_distance": 12.0, "ground": False}
