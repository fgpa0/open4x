"""Town buildings: the keep at a city's centre, houses of several kinds, a
wall segment and a tower. Roofs are flagged for the owner's colour (painted
light, the game tints them), windows for the glow at night. Metres;
exported at 0.01 (a house is ~0.08 game units across before the town's
size factor)."""

import math
import random

import bmesh
import bpy
from mathutils import Euler, Vector


def mesh_obj(name, bm, material):
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    o = bpy.data.objects.new(name, me)
    bpy.context.collection.objects.link(o)
    o.data.materials.append(material)
    o.data.shade_flat()
    return o


def gable(k, w, d, h, roof, walls, pitch=0.85, overhang=0.35, loc=(0, 0, 0)):
    """Walls and a gable roof, the ridge along X."""
    loc = Vector(loc)
    out = [k.box((w, d, h), walls, loc=loc + Vector((0, 0, h / 2)))]
    rh = d / 2 * pitch
    rings = []
    for x in (-w / 2 - overhang, w / 2 + overhang):
        rings.append([loc + Vector((x, -d / 2 - overhang, h - 0.25)), loc + Vector((x, 0, h + rh)), loc + Vector((x, d / 2 + overhang, h - 0.25)), loc + Vector((x, 0, h + rh - 0.35))])
    r = k.loft("roof", rings, roof, closed=True, cap=True, smooth=None)
    out.append(r)
    for s in (-1, 1):
        bm = bmesh.new()
        a = bm.verts.new(loc + Vector((s * w / 2, -d / 2, h)))
        b = bm.verts.new(loc + Vector((s * w / 2, d / 2, h)))
        c = bm.verts.new(loc + Vector((s * w / 2, 0, h + rh)))
        bm.faces.new((a, b, c) if s > 0 else (c, b, a))
        out.append(mesh_obj("gable", bm, walls))
    return out, h + rh


def hip_roof(k, w, d, h, roof, overhang=0.35, loc=(0, 0, 0), rise=0.8):
    loc = Vector(loc)
    bm = bmesh.new()
    x, y = w / 2 + overhang, d / 2 + overhang
    rh = min(w, d) / 2 * rise
    ridge = max(0.0, (w - d) / 2)
    base = [bm.verts.new(loc + Vector(v)) for v in ((-x, -y, h - 0.2), (x, -y, h - 0.2), (x, y, h - 0.2), (-x, y, h - 0.2))]
    r0, r1 = bm.verts.new(loc + Vector((-ridge, 0, h + rh))), bm.verts.new(loc + Vector((ridge, 0, h + rh)))
    bm.faces.new((base[0], base[1], r1, r0))
    bm.faces.new((base[1], base[2], r1))
    bm.faces.new((base[2], base[3], r0, r1))
    bm.faces.new((base[3], base[0], r0))
    bm.faces.new(list(reversed(base)))
    return mesh_obj("hip", bm, roof)


def windows(k, mat, w, d, z, count, loc=(0, 0, 0), sides=("front", "back"), size=(0.7, 0.9)):
    loc = Vector(loc)
    out = []
    for side in sides:
        for i in range(count):
            x = (i + 0.5) / count * w - w / 2
            if side in ("front", "back"):
                y = (-d / 2 - 0.05) if side == "front" else (d / 2 + 0.05)
                out.append(k.box((size[0], 0.12, size[1]), mat, loc=loc + Vector((x, y, z))))
            else:
                xx = (-w / 2 - 0.05) if side == "left" else (w / 2 + 0.05)
                yy = (i + 0.5) / count * d - d / 2
                out.append(k.box((0.12, size[0], size[1]), mat, loc=loc + Vector((xx, yy, z))))
    return out


def timbers(k, mat, w, d, h, loc=(0, 0, 0)):
    """Dark timber framing on the long walls."""
    loc = Vector(loc)
    out = []
    for y in (-d / 2 - 0.04, d / 2 + 0.04):
        out.append(k.box((w + 0.1, 0.1, 0.22), mat, loc=loc + Vector((0, y, h - 0.1))))
        out.append(k.box((w + 0.1, 0.1, 0.22), mat, loc=loc + Vector((0, y, h * 0.5))))
        for x in (-w / 2, 0, w / 2):
            out.append(k.box((0.22, 0.1, h), mat, loc=loc + Vector((x, y, h / 2))))
        for x in (-w / 4, w / 4):
            brace = k.box((0.16, 0.1, h * 0.62), mat, loc=loc + Vector((x, y, h * 0.27)), rot=(0, 0.6 if x < 0 else -0.6, 0))
            out.append(brace)
    return out


def build(k):
    rng = random.Random(11)
    p = k.palette()
    roof = k.flag(k.mat("roof_tiles", k.srgb(236, 230, 224), 0.8, pattern="streaks", scale=3.0, amount=0.25), "team")
    glass = k.flag(k.mat("glass", k.srgb(40, 34, 30), 0.3), "window")
    plaster = [k.mat(f"plaster{i}", k.srgb(*c), 0.9, pattern="noise", scale=1.5, amount=0.15) for i, c in enumerate([(232, 222, 200), (220, 206, 180), (206, 196, 178)])]
    stone = k.mat("town_stone", k.srgb(186, 176, 160), 0.9, pattern="noise", scale=1.2, amount=0.35)
    dark_stone = k.mat("wall_stone", k.srgb(150, 142, 130), 0.9, pattern="noise", scale=1.0, amount=0.35)
    timber = k.mat("timber", k.srgb(70, 50, 36), 0.85, pattern="grain", scale=3, amount=0.25)
    door = k.mat("door", k.srgb(96, 64, 40), 0.8)
    out = {}

    # Cottage: plaster and timber, a steep gable roof.
    parts, _ = gable(k, 7.0, 5.0, 3.2, roof, plaster[0])
    parts += timbers(k, timber, 7.0, 5.0, 3.2)
    parts += windows(k, glass, 7.0, 5.0, 1.9, 2)
    parts.append(k.box((1.0, 0.15, 2.0), door, loc=(0.0, -2.55, 1.0)))
    parts.append(k.box((0.6, 0.6, 2.2), stone, loc=(2.4, 1.0, 4.6)))
    out["house_cottage"] = parts

    # Townhouse: two storeys, the upper one jettied out.
    parts = [k.box((6.0, 5.4, 3.0), plaster[1], loc=(0, 0, 1.5))]
    up, top = gable(k, 6.6, 6.0, 2.8, roof, plaster[1], pitch=1.0, loc=(0, 0, 3.0))
    parts += up
    parts += timbers(k, timber, 6.6, 6.0, 2.8, loc=(0, 0, 3.0))
    parts += windows(k, glass, 6.0, 5.4, 1.8, 2)
    parts += windows(k, glass, 6.6, 6.0, 4.5, 3)
    parts.append(k.box((1.0, 0.15, 2.0), door, loc=(-1.6, -2.75, 1.0)))
    out["house_town"] = parts

    # Longhouse: low and long, a big roof.
    parts, _ = gable(k, 11.0, 5.0, 2.6, roof, plaster[2], pitch=1.1)
    parts += windows(k, glass, 11.0, 5.0, 1.5, 3)
    parts.append(k.box((1.2, 0.15, 1.8), door, loc=(-3.0, -2.55, 0.9)))
    parts.append(k.box((2.0, 1.4, 1.2), k.mat("woodpile", k.srgb(150, 104, 62), 0.9, pattern="grain", scale=4, amount=0.3), loc=(6.4, 0.5, 0.6)))
    out["house_long"] = parts

    # Stone house with a hipped roof and a chimney.
    parts = [k.box((6.5, 6.0, 4.0), stone, loc=(0, 0, 2.0))]
    parts.append(hip_roof(k, 6.5, 6.0, 4.0, roof))
    parts += windows(k, glass, 6.5, 6.0, 2.4, 2, sides=("front", "back", "left"))
    parts.append(k.box((1.1, 0.15, 2.1), door, loc=(0.0, -3.05, 1.05)))
    parts.append(k.box((0.7, 0.7, 3.0), stone, loc=(-2.0, 1.6, 5.6)))
    out["house_stone"] = parts

    # Workshop: an L of a house and a lean-to shed.
    parts, _ = gable(k, 6.0, 5.0, 3.0, roof, plaster[0])
    parts += windows(k, glass, 6.0, 5.0, 1.8, 2)
    parts.append(k.box((1.0, 0.15, 2.0), door, loc=(1.6, -2.55, 1.0)))
    parts.append(k.box((3.5, 4.0, 2.0), p["planks"], loc=(4.6, 0.5, 1.0)))
    lean = k.box((4.0, 4.6, 0.25), timber, loc=(4.7, 0.5, 2.4), rot=(0, 0.25, 0))
    parts.append(lean)
    parts.append(k.box((1.4, 1.0, 1.0), p["wood"], loc=(5.2, -2.0, 0.5)))
    out["house_workshop"] = parts

    # The keep: a square stone tower with a pyramid roof, corner turrets,
    # a banner, and a hall against it.
    parts = [k.box((14.0, 14.0, 18.0), stone, loc=(0, 0, 9.0))]
    parts.append(k.box((15.0, 15.0, 1.0), dark_stone, loc=(0, 0, 18.0)))
    for x in (-6.5, 6.5):
        for y in (-6.5, 6.5):
            parts.append(k.cyl(2.0, 22.0, stone, loc=(x, y, 11.0), n=10, smooth=0.5))
            parts.append(k.cyl(2.4, 4.0, roof, loc=(x, y, 24.0), n=10, r2=0.1, smooth=None))
    parts.append(k.cyl(10.0, 9.0, roof, loc=(0, 0, 23.0), n=4, r2=0.2, rot=(0, 0, math.pi / 4), smooth=None))
    for z in (6.0, 12.0):
        parts += windows(k, glass, 14.0, 14.0, z, 3, sides=("front", "back", "left", "right"), size=(1.0, 2.0))
    parts.append(k.box((3.0, 0.4, 4.5), door, loc=(0, -7.1, 2.25)))
    hall, _ = gable(k, 12.0, 8.0, 6.0, roof, plaster[0], loc=(0, -12.0, 0))
    parts += hall
    parts += windows(k, glass, 12.0, 8.0, 3.5, 4, loc=(0, -12.0, 0), sides=("front",), size=(1.0, 1.8))
    parts.append(k.rod((0, 0, 27.0), (0, 0, 36.0), 0.25, timber, n=6))
    flag = k.sheet("banner", 5.0, 3.0, k.flag(k.mat("banner_cloth", k.srgb(240, 240, 240), 0.8, pattern="weave", scale=4, amount=0.1), "team"), nx=6, ny=4, bulge=0.4, thickness=0.1)
    flag.rotation_euler = (0, 0, math.pi / 2)
    flag.location = (0, 2.6, 34.3)
    parts.append(flag)
    out["keep"] = parts

    # Wall: a 10 m crenellated stretch, along X, and a round tower.
    parts = [k.box((10.0, 2.2, 6.0), dark_stone, loc=(0, 0, 3.0))]
    for i in range(5):
        parts.append(k.box((1.1, 2.4, 1.0), dark_stone, loc=(-4.0 + i * 2.0, 0, 6.5)))
    out["wall"] = parts
    parts = [k.cyl(3.2, 9.0, dark_stone, loc=(0, 0, 4.5), n=14, smooth=0.5)]
    parts.append(k.cyl(3.6, 1.0, dark_stone, loc=(0, 0, 9.3), n=14, smooth=0.5))
    parts.append(k.cyl(3.9, 4.5, roof, loc=(0, 0, 12.0), n=14, r2=0.1, smooth=None))
    parts += [k.box((0.6, 0.15, 1.0), glass, loc=(math.cos(a) * 3.22, math.sin(a) * 3.22, 6.0), rot=(0, 0, a + math.pi / 2)) for a in (0.5, 2.6, 4.7)]
    out["tower"] = parts
    ancient(k, p, out, rng, glass)
    classical(k, p, out, glass)
    renaissance(k, p, out, roof, glass, stone, dark_stone, door)
    industrial(k, p, out, roof, glass, stone, door)
    modern(k, p, out, roof, glass, door)
    return out, {"kind": "props", "scale": 0.01, "ao_distance": 3.0}


def pennant(k, at, height, team):
    """A pole with a small flag in the owner's colour."""
    at = Vector(at)
    pole = k.rod(at, at + Vector((0, 0, height)), 0.08, k.mat("pole", k.srgb(96, 70, 46), 0.9), n=5)
    f = k.sheet("pennant", 1.6, 0.9, team, nx=3, ny=2, bulge=0.15, thickness=0.05)
    f.rotation_euler = (0, 0, math.pi / 2)
    f.location = at + Vector((0, 0.8, height - 0.5))
    return [pole, f]


def ancient(k, p, out, rng, glass):
    """Ancient Era: thatched round huts and longhouses of wattle and daub,
    a chieftain's hall, a palisade of sharpened logs."""
    thatch = k.mat("town_thatch", k.srgb(168, 134, 80), 0.95, pattern="hair", scale=6, amount=0.4)
    daub = k.mat("daub", k.srgb(176, 146, 108), 0.95, pattern="noise", scale=1.5, amount=0.3)
    logs = k.mat("palisade_log", k.srgb(120, 90, 60), 0.9, pattern="grain", scale=3, amount=0.3)
    team = k.flag(k.mat("cloth_team", k.srgb(240, 240, 240), 0.85, pattern="weave", scale=4, amount=0.1), "team")
    glow = glass  # doorways glow from the hearth at night

    parts = [k.cyl(3.2, 2.2, daub, loc=(0, 0, 1.1), n=14, smooth=0.6)]
    parts.append(k.cyl(4.0, 3.8, thatch, loc=(0, 0, 3.9), n=14, r2=0.2, smooth=0.6))
    parts.append(k.box((1.1, 0.4, 1.7), glow, loc=(0, -3.1, 0.85)))
    parts += pennant(k, (2.6, 2.6, 0), 4.5, team)
    out["ancient_roundhouse"] = parts

    parts, _ = gable(k, 9.0, 4.6, 1.8, thatch, daub, pitch=1.5, overhang=0.7)
    parts.append(k.box((1.0, 0.3, 1.5), glow, loc=(-2.5, -2.4, 0.75)))
    parts += pennant(k, (4.8, 0, 0), 5.0, team)
    out["ancient_longhouse"] = parts

    parts = [k.cyl(2.4, 1.8, daub, loc=(0, 0, 0.9), n=12, smooth=0.6)]
    parts.append(k.cyl(3.0, 3.0, thatch, loc=(0, 0, 3.2), n=12, r2=0.15, smooth=0.6))
    parts.append(k.box((0.9, 0.3, 1.4), glow, loc=(0, -2.35, 0.7)))
    parts.append(k.box((2.2, 1.2, 0.8), k.mat("wood_stack", k.srgb(140, 100, 62), 0.9, pattern="grain", scale=4, amount=0.3), loc=(2.6, 1.0, 0.4)))
    out["ancient_hut"] = parts

    # The chieftain's hall: a long, high-roofed hall, carved gable posts,
    # a great banner, a ring of standing stones in front.
    parts, top = gable(k, 18.0, 8.0, 3.4, thatch, daub, pitch=1.4, overhang=1.0)
    for x in (-9.5, 9.5):
        parts.append(k.rod((x, 0, 3.0), (x * 1.04, 0, top + 1.6), 0.25, logs, n=6))
    parts.append(k.box((2.2, 0.4, 2.6), glow, loc=(0, -4.1, 1.3)))
    parts += pennant(k, (0, -6.0, 0), 10.0, team)
    for i in range(7):
        a = i / 7 * math.tau
        parts.append(k.box((0.9, 0.6, 2.2 + rng.uniform(-0.3, 0.5)), k.mat("standing", k.srgb(140, 136, 128), 0.9, pattern="noise", scale=1.0, amount=0.4), loc=(math.cos(a) * 3.0 - 5.0, math.sin(a) * 3.0 - 9.0, 1.1), rot=(0, 0, a)))
    out["ancient_hall"] = parts

    # Palisade: a 10 m stretch of sharpened logs, and a watchtower.
    parts = []
    for i in range(12):
        x = -5.0 + (i + 0.5) * 10.0 / 12
        h = 5.0 + rng.uniform(-0.4, 0.4)
        parts.append(k.cyl(0.42, h, logs, loc=(x, 0, h / 2), n=6))
        parts.append(k.cyl(0.42, 0.9, logs, loc=(x, 0, h + 0.45), n=6, r2=0.02))
    parts.append(k.box((10.0, 0.25, 0.3), logs, loc=(0, 0.45, 3.4)))
    out["palisade"] = parts
    parts = [k.rod((x, y, 0), (x * 0.8, y * 0.8, 7.5), 0.3, logs, n=5) for x in (-1.6, 1.6) for y in (-1.6, 1.6)]
    parts.append(k.box((3.8, 3.8, 0.4), logs, loc=(0, 0, 7.4)))
    parts.append(k.box((3.8, 3.8, 1.2), k.mat("tower_rail", k.srgb(120, 90, 60), 0.9, pattern="grain", scale=3, amount=0.3), loc=(0, 0, 8.2)))
    parts.append(k.cyl(3.4, 2.4, thatch, loc=(0, 0, 10.0), n=4, r2=0.1, rot=(0, 0, math.pi / 4), smooth=None))
    parts += pennant(k, (0, 0, 10.5), 3.0, team)
    out["palisade_tower"] = parts


def classical(k, p, out, glass):
    """Classical Era: whitewashed houses round courtyards under terracotta
    tiles, awnings in the owner's colour, a columned temple, ashlar walls."""
    tile = k.mat("terracotta", k.srgb(184, 92, 60), 0.85, pattern="streaks", scale=3.0, amount=0.3)
    white = k.mat("whitewash", k.srgb(236, 230, 214), 0.9, pattern="noise", scale=1.5, amount=0.12)
    marble = k.mat("temple_marble", k.srgb(236, 232, 222), 0.6, pattern="noise", scale=1.0, amount=0.12)
    ashlar = k.mat("ashlar", k.srgb(206, 190, 160), 0.9, pattern="noise", scale=1.0, amount=0.3)
    team = k.flag(k.mat("awning_team", k.srgb(240, 240, 240), 0.85, pattern="weave", scale=4, amount=0.1), "team")
    door = k.mat("door_c", k.srgb(110, 72, 44), 0.8)

    def awning(x, y, w):
        a = k.box((w, 1.4, 0.12), team, loc=(x, y - 0.6, 2.4), rot=(0.35, 0, 0))
        return [a]

    parts, _ = gable(k, 7.0, 6.0, 3.2, tile, white, pitch=0.45, overhang=0.4)
    parts += windows(k, glass, 7.0, 6.0, 2.2, 2, size=(0.6, 0.7))
    parts.append(k.box((1.1, 0.15, 2.1), door, loc=(0, -3.05, 1.05)))
    parts += awning(0, -3.1, 3.0)
    out["classical_house"] = parts

    # A villa: two wings and a wall closing a courtyard.
    parts, _ = gable(k, 10.0, 4.0, 3.4, tile, white, pitch=0.45, loc=(0, 3.0, 0))
    wing, _ = gable(k, 4.0, 6.0, 3.0, tile, white, pitch=0.45, loc=(-3.0, -2.0, 0))
    parts += wing
    parts.append(k.box((6.0, 0.4, 2.2), white, loc=(2.0, -4.8, 1.1)))
    parts.append(k.box((1.8, 0.5, 1.8), door, loc=(2.0, -4.85, 0.9)))
    parts.append(k.cyl(0.9, 0.5, k.mat("pool", k.srgb(80, 130, 150), 0.1), loc=(2.0, -1.5, 0.25), n=10))
    parts += windows(k, glass, 10.0, 4.0, 2.2, 3, loc=(0, 3.0, 0), sides=("back",), size=(0.6, 0.7))
    parts += awning(-3.0, -5.1, 2.6)
    out["classical_villa"] = parts

    # An insula: a three-storey block with shops below.
    parts = [k.box((8.0, 7.0, 8.0), k.mat("insula", k.srgb(214, 178, 140), 0.9, pattern="noise", scale=1.5, amount=0.15), loc=(0, 0, 4.0))]
    parts.append(hip_roof(k, 8.0, 7.0, 8.0, tile, rise=0.35))
    for z in (4.2, 6.6):
        parts += windows(k, glass, 8.0, 7.0, z, 4, sides=("front", "back", "left"), size=(0.6, 0.9))
    for x in (-2.5, 0.0, 2.5):
        parts.append(k.box((1.6, 0.15, 2.0), door, loc=(x, -3.55, 1.0)))
    parts += awning(0, -3.6, 7.0)
    out["classical_insula"] = parts

    # The temple: a raised stepped platform, columns all round, a pediment,
    # a roof in the owner's colour.
    parts = [k.box((16.0 - i * 0.8, 24.0 - i * 0.8, 0.6), marble, loc=(0, 0, 0.3 + i * 0.6)) for i in range(3)]
    for x in (-6.4, -3.2, 0.0, 3.2, 6.4):
        for y in (-10.4, 10.4):
            parts.append(k.cyl(0.6, 8.0, marble, loc=(x, y, 5.8), n=10, smooth=0.9))
    for y in (-6.9, -3.5, 0.0, 3.5, 6.9):
        for x in (-6.4, 6.4):
            parts.append(k.cyl(0.6, 8.0, marble, loc=(x, y, 5.8), n=10, smooth=0.9))
    parts.append(k.box((10.0, 17.0, 8.0), white, loc=(0, 0, 5.8)))
    parts.append(k.box((15.0, 23.0, 1.2), marble, loc=(0, 0, 10.4)))
    temple_roof = k.flag(k.mat("temple_roof", k.srgb(236, 230, 224), 0.8, pattern="streaks", scale=3.0, amount=0.25), "team")
    roof, _ = gable(k, 23.0, 15.0, 0.1, temple_roof, marble, pitch=0.3, overhang=0.3, loc=(0, 0, 11.0))
    k.pivot(roof, (0, 0, 11.0), Euler((0, 0, math.pi / 2)))
    parts += roof
    parts.append(k.box((3.0, 0.3, 5.5), door, loc=(0, -8.6, 4.6)))
    out["temple"] = parts

    parts = [k.box((10.0, 2.6, 7.0), ashlar, loc=(0, 0, 3.5))]
    for i in range(5):
        parts.append(k.box((1.2, 2.8, 1.0), ashlar, loc=(-4.0 + i * 2.0, 0, 7.5)))
    out["classical_wall"] = parts
    parts = [k.box((6.0, 6.0, 11.0), ashlar, loc=(0, 0, 5.5))]
    parts.append(hip_roof(k, 6.0, 6.0, 11.0, tile, rise=0.5, overhang=0.3))
    parts += windows(k, glass, 6.0, 6.0, 8.0, 1, sides=("front", "back", "left", "right"), size=(0.6, 1.2))
    out["classical_tower"] = parts


def renaissance(k, p, out, roof, glass, stone, dark_stone, door):
    """Renaissance Era: tall stone townhouses with dormers, a domed palace,
    low thick walls with angled bastions."""
    cream = k.mat("cream", k.srgb(232, 220, 190), 0.85, pattern="noise", scale=1.5, amount=0.12)
    brick = k.mat("brick", k.srgb(176, 104, 78), 0.9, pattern="noise", scale=3.0, amount=0.2)
    copper = k.mat("dome_copper", k.srgb(108, 160, 140), 0.5, metal=0.4, pattern="noise", scale=2.0, amount=0.2)
    trim = k.mat("trim", k.srgb(240, 236, 226), 0.7)

    def townhouse(w, d, floors, walls, step_gable=False):
        h = floors * 3.0
        parts, top = gable(k, w, d, h, roof, walls, pitch=1.3, overhang=0.25)
        for f in range(floors):
            parts += windows(k, glass, w, d, 1.9 + f * 3.0, 3, size=(0.8, 1.3))
        parts.append(k.box((w + 0.2, d + 0.2, 0.3), trim, loc=(0, 0, 3.0)))
        parts.append(k.box((1.2, 0.2, 2.3), door, loc=(0, -d / 2 - 0.05, 1.15)))
        for x in (-w / 4, w / 4):
            parts.append(k.box((1.0, 1.6, 1.2), walls, loc=(x, -d / 4, h + 0.9)))
            parts.append(k.box((0.6, 0.12, 0.7), glass, loc=(x, -d / 4 - 0.82, h + 0.9)))
        parts.append(k.box((0.7, 0.7, 2.6), brick, loc=(w * 0.35, d * 0.2, top - 0.4)))
        return parts

    out["ren_townhouse"] = townhouse(6.5, 7.0, 3, cream)
    out["ren_brick"] = townhouse(6.0, 8.0, 3, brick)
    out["ren_tall"] = townhouse(5.5, 6.5, 4, cream)

    # The palace: a main block with a dome over the centre, two wings, a
    # forecourt, a banner on the lantern.
    parts = [k.box((26.0, 12.0, 12.0), cream, loc=(0, 0, 6.0))]
    parts.append(hip_roof(k, 26.0, 12.0, 12.0, roof, rise=0.45))
    for x in (-15.0, 15.0):
        parts.append(k.box((8.0, 20.0, 10.0), cream, loc=(x, -4.0, 5.0)))
        parts.append(hip_roof(k, 8.0, 20.0, 10.0, roof, rise=0.5, loc=(x, -4.0, 0)))
        parts += windows(k, glass, 8.0, 20.0, 6.5, 4, loc=(x, -4.0, 0), sides=("left", "right"), size=(1.0, 1.8))
    for z in (3.5, 8.0):
        parts += windows(k, glass, 26.0, 12.0, z, 7, sides=("front", "back"), size=(1.0, 2.0))
    parts.append(k.cyl(5.0, 4.0, cream, loc=(0, 0, 16.0), n=16, smooth=0.8))
    dome = k.ball(5.4, copper, loc=(0, 0, 18.0), scale=(1, 1, 1.15), subdiv=3)
    parts.append(dome)
    parts.append(k.cyl(1.2, 3.0, trim, loc=(0, 0, 25.5), n=10))
    parts.append(k.box((4.0, 0.4, 5.0), door, loc=(0, -6.1, 2.5)))
    for x in (-3.0, -1.0, 1.0, 3.0):
        parts.append(k.cyl(0.5, 9.0, trim, loc=(x, -6.6, 4.5), n=8))
    parts.append(k.box((9.0, 2.0, 1.2), trim, loc=(0, -6.6, 9.6)))
    parts.append(k.rod((0, 0, 27.0), (0, 0, 33.0), 0.2, door, n=6))
    flag = k.sheet("banner", 4.0, 2.4, k.flag(k.mat("banner_cloth2", k.srgb(240, 240, 240), 0.8, pattern="weave", scale=4, amount=0.1), "team"), nx=6, ny=4, bulge=0.4, thickness=0.1)
    flag.rotation_euler = (0, 0, math.pi / 2)
    flag.location = (0, 2.1, 31.6)
    parts.append(flag)
    out["palace"] = parts

    # A low thick rampart and an arrow-headed bastion.
    parts = [k.box((10.0, 5.0, 4.0), dark_stone, loc=(0, 0, 2.0))]
    parts.append(k.box((10.0, 1.2, 1.0), stone, loc=(0, -2.0, 4.5)))
    out["rampart"] = parts
    bm = bmesh.new()
    pts = [(-4, 3), (4, 3), (4, 0), (0, -5), (-4, 0)]
    low = [bm.verts.new((x * 1.0, y * 1.0, 0)) for x, y in pts]
    high = [bm.verts.new((x * 0.85, y * 0.85, 5.0)) for x, y in pts]
    n = len(pts)
    for i in range(n):
        j = (i + 1) % n
        bm.faces.new((low[i], low[j], high[j], high[i]))
    bm.faces.new(list(reversed(low)))
    bm.faces.new(high)
    bastion = [mesh_obj("bastion", bm, dark_stone)]
    bastion.append(k.box((1.5, 1.5, 1.6), stone, loc=(0, -3.6, 5.6)))
    out["bastion"] = bastion


def industrial(k, p, out, roof, glass, stone, door):
    """Industrial Era: brick terraces and tenements with chimney pots, a mill
    with a tall smokestack, a town hall with a clock tower."""
    brick = k.mat("ind_brick", k.srgb(150, 78, 58), 0.9, pattern="noise", scale=3.5, amount=0.25)
    sooty = k.mat("sooty_brick", k.srgb(112, 64, 52), 0.9, pattern="noise", scale=3.0, amount=0.3)
    trim = k.mat("ind_trim", k.srgb(226, 218, 200), 0.75)
    iron = k.mat("ind_iron", k.srgb(60, 62, 68), 0.5, metal=0.6)
    clock = k.mat("clock_face", k.srgb(240, 236, 220), 0.6)

    def terrace(n, floors, walls):
        """A row of `n` narrow houses under one roof, a chimney stack between each."""
        w, d, h = 4.2 * n, 7.0, floors * 3.0
        parts, top = gable(k, w, d, h, roof, walls, pitch=0.9, overhang=0.2)
        for f in range(floors):
            parts += windows(k, glass, w, d, 1.9 + f * 3.0, 2 * n, size=(0.8, 1.4))
        for i in range(n):
            x = -w / 2 + 4.2 * (i + 0.5)
            parts.append(k.box((1.1, 0.2, 2.2), door, loc=(x - 1.0, -d / 2 - 0.05, 1.1)))
            parts.append(k.box((1.4, 0.3, 0.15), trim, loc=(x - 1.0, -d / 2 - 0.15, 2.3)))
        for i in range(n + 1):
            x = -w / 2 + 4.2 * i
            parts.append(k.box((0.9, 1.6, 2.4), walls, loc=(x, 0, top - 0.2)))
            for j in (-0.4, 0.4):
                parts.append(k.cyl(0.18, 0.6, sooty, loc=(x, j, top + 1.2), n=8))
        parts.append(k.box((w + 0.1, d + 0.1, 0.25), trim, loc=(0, 0, 3.0)))
        return parts

    out["ind_terrace"] = terrace(3, 2, brick)
    out["ind_terrace_long"] = terrace(4, 2, sooty)

    # A tenement: four storeys, a flat roof behind a parapet, chimney stacks.
    w, d, h = 9.0, 8.0, 12.0
    parts = [k.box((w, d, h), brick, loc=(0, 0, h / 2))]
    for sx, sy, bw, bd in ((0, -1, w + 0.4, 0.4), (0, 1, w + 0.4, 0.4), (-1, 0, 0.4, d + 0.4), (1, 0, 0.4, d + 0.4)):
        parts.append(k.box((bw, bd, 0.9), trim, loc=(sx * w / 2, sy * d / 2, h + 0.45)))
    parts.append(k.box((w, d, 0.3), roof, loc=(0, 0, h + 0.15)))
    for f in range(4):
        parts += windows(k, glass, w, d, 1.9 + f * 3.0, 4, sides=("front", "back", "left", "right"), size=(0.9, 1.5))
    parts.append(k.box((1.6, 0.2, 2.6), door, loc=(0, -d / 2 - 0.05, 1.3)))
    for x in (-w * 0.3, w * 0.3):
        parts.append(k.box((1.0, 2.0, 2.2), sooty, loc=(x, 0, h + 1.6)))
    out["ind_tenement"] = parts

    # A mill: a long brick shed under a saw-tooth roof, a tall smokestack.
    parts = [k.box((16.0, 9.0, 6.0), brick, loc=(0, 0, 3.0))]
    for i in range(4):
        x = -6.0 + i * 4.0
        bm = bmesh.new()
        pts = [(x - 2.0, -4.5, 6.0), (x + 2.0, -4.5, 6.0), (x + 2.0, -4.5, 8.5), (x - 2.0, 4.5, 6.0), (x + 2.0, 4.5, 6.0), (x + 2.0, 4.5, 8.5)]
        v = [bm.verts.new(q) for q in pts]
        bm.faces.new((v[0], v[1], v[2]))
        bm.faces.new((v[5], v[4], v[3]))
        bm.faces.new((v[0], v[2], v[5], v[3]))
        bm.faces.new((v[1], v[4], v[5], v[2]))
        bm.faces.new((v[0], v[3], v[4], v[1]))
        parts.append(mesh_obj("sawtooth", bm, roof))
        parts.append(k.box((0.15, 8.6, 2.2), glass, loc=(x + 1.95, 0, 7.3)))
    parts += windows(k, glass, 16.0, 9.0, 3.2, 6, sides=("front", "back"), size=(1.2, 2.2))
    parts.append(k.box((3.0, 0.2, 4.0), door, loc=(-5.5, -4.55, 2.0)))
    parts.append(k.cyl(1.2, 18.0, sooty, loc=(9.5, 2.5, 9.0), n=14, r2=0.85))
    parts.append(k.cyl(1.0, 0.8, iron, loc=(9.5, 2.5, 18.2), n=14))
    for z in (4.0, 9.0, 14.0):
        parts.append(k.cyl(1.05 - z * 0.013, 0.3, trim, loc=(9.5, 2.5, z), n=14))
    out["ind_mill"] = parts

    # The town hall: a broad brick hall with stone dressings, a clock tower
    # rising from the middle with a pointed roof and a flag.
    parts = [k.box((22.0, 12.0, 10.0), brick, loc=(0, 0, 5.0))]
    parts.append(hip_roof(k, 22.0, 12.0, 10.0, roof, rise=0.5))
    for z in (3.0, 7.0):
        parts += windows(k, glass, 22.0, 12.0, z, 7, sides=("front", "back"), size=(1.0, 2.0))
    parts.append(k.box((22.4, 12.4, 0.5), trim, loc=(0, 0, 5.2)))
    parts.append(k.box((3.6, 0.4, 4.0), door, loc=(0, -6.1, 2.0)))
    parts.append(k.box((6.0, 1.0, 0.6), trim, loc=(0, -6.6, 4.4)))
    parts.append(k.box((5.0, 5.0, 14.0), brick, loc=(0, 0, 13.0)))
    parts.append(k.box((5.4, 5.4, 0.6), trim, loc=(0, 0, 20.2)))
    for a in range(4):
        ang = a * math.pi / 2
        parts.append(k.cyl(1.4, 0.15, clock, loc=(math.sin(ang) * 2.58, -math.cos(ang) * 2.58, 17.5), rot=(math.pi / 2, 0, ang), n=16))
        parts.append(k.box((0.12, 0.1, 1.0), iron, loc=(math.sin(ang) * 2.7, -math.cos(ang) * 2.7, 17.8), rot=(0, 0, ang)))
    parts.append(k.cyl(3.6, 6.0, roof, loc=(0, 0, 23.5), n=4, r2=0.1, rot=(0, 0, math.pi / 4), smooth=None))
    parts += pennant(k, (0, 0, 26.0), 5.0, k.flag(k.mat("hall_flag", k.srgb(240, 240, 240), 0.8, pattern="weave", scale=4, amount=0.1), "team"))
    out["town_hall"] = parts


def modern(k, p, out, roof, glass, door):
    """Modern Era: concrete apartment slabs with bands of windows and
    balconies, an office tower stepping back as it rises, shops under
    awnings, suburban houses, and a civic tower with a spire at the centre."""
    concrete = k.mat("concrete", k.srgb(196, 192, 182), 0.9, pattern="noise", scale=2.0, amount=0.18)
    pale = k.mat("mod_render", k.srgb(226, 220, 204), 0.85, pattern="noise", scale=1.5, amount=0.12)
    tan = k.mat("mod_brick", k.srgb(184, 140, 104), 0.9, pattern="noise", scale=3.5, amount=0.22)
    steel = k.mat("mod_steel", k.srgb(90, 96, 104), 0.4, metal=0.7)
    deco = k.mat("deco_stone", k.srgb(214, 202, 176), 0.8, pattern="noise", scale=1.2, amount=0.2)

    def flat_top(w, d, h, walls, parapet=0.8):
        """A box with a parapet, the roof in the owner's colour."""
        parts = [k.box((w, d, h), walls, loc=(0, 0, h / 2))]
        parts.append(k.box((w - 0.6, d - 0.6, 0.3), roof, loc=(0, 0, h + 0.1)))
        for sx, sy, bw, bd in ((0, -1, w, 0.4), (0, 1, w, 0.4), (-1, 0, 0.4, d), (1, 0, 0.4, d)):
            parts.append(k.box((bw, bd, parapet), walls, loc=(sx * (w / 2 - 0.2), sy * (d / 2 - 0.2), h + parapet / 2)))
        return parts

    def bands(w, d, z0, floors, step, walls_mat, sides=("front", "back", "left", "right")):
        """Ribbon windows: a long strip of glass on every floor."""
        parts = []
        for f in range(floors):
            z = z0 + f * step
            if "front" in sides or "back" in sides:
                for y in (-d / 2 - 0.05, d / 2 + 0.05):
                    parts.append(k.box((w - 1.2, 0.12, 1.3), glass, loc=(0, y, z)))
            if "left" in sides:
                for x in (-w / 2 - 0.05, w / 2 + 0.05):
                    parts.append(k.box((0.12, d - 1.2, 1.3), glass, loc=(x, 0, z)))
        return parts

    # An apartment slab: six storeys, balconies on the front.
    w, d, h = 14.0, 8.0, 18.0
    parts = flat_top(w, d, h, concrete)
    parts += bands(w, d, 2.0, 6, 3.0, concrete, sides=("front", "back"))
    for f in range(1, 6):
        for x in (-4.5, 0.0, 4.5):
            parts.append(k.box((3.2, 1.2, 0.2), concrete, loc=(x, -d / 2 - 0.6, f * 3.0 + 0.9)))
            parts.append(k.box((3.2, 0.1, 0.9), steel, loc=(x, -d / 2 - 1.15, f * 3.0 + 1.4)))
    parts.append(k.box((2.4, 0.2, 2.6), door, loc=(0, -d / 2 - 0.05, 1.3)))
    parts.append(k.box((2.0, 2.0, 2.2), concrete, loc=(4.0, 1.0, h + 1.1)))
    out["mod_apartment"] = parts

    # A shorter block in warm brick, four storeys.
    w, d, h = 11.0, 9.0, 12.5
    parts = flat_top(w, d, h, tan, parapet=0.6)
    for f in range(4):
        parts += windows(k, glass, w, d, 1.9 + f * 3.0, 5, sides=("front", "back", "left", "right"), size=(1.1, 1.5))
    parts.append(k.box((2.0, 0.2, 2.5), door, loc=(0, -d / 2 - 0.05, 1.25)))
    parts.append(k.box((3.0, 1.2, 0.2), concrete, loc=(0, -d / 2 - 0.6, 2.8)))
    out["mod_block"] = parts

    # An office tower: a glass and steel shaft on a stone base, stepping back.
    parts = [k.box((12.0, 12.0, 6.0), deco, loc=(0, 0, 3.0))]
    parts.append(k.box((12.1, 12.1, 0.4), steel, loc=(0, 0, 6.0)))
    parts += windows(k, glass, 12.0, 12.0, 3.0, 3, sides=("front", "back", "left", "right"), size=(2.6, 3.4))
    for (wd, z0, z1) in ((10.0, 6.0, 26.0), (7.0, 26.0, 34.0)):
        parts.append(k.box((wd, wd, z1 - z0), pale, loc=(0, 0, (z0 + z1) / 2)))
        n = int((z1 - z0) / 3.0)
        for f in range(n):
            parts += windows(k, glass, wd, wd, z0 + 1.8 + f * 3.0, 4 if wd > 8 else 3, sides=("front", "back", "left", "right"), size=(1.6, 1.8))
        parts.append(k.box((wd + 0.4, wd + 0.4, 0.4), roof, loc=(0, 0, z1)))
    parts.append(k.rod((0, 0, 34.0), (0, 0, 40.0), 0.15, steel, n=6))
    out["mod_office"] = parts

    # A row of shops: plate glass under striped awnings, flats above.
    w, d, h = 15.0, 7.0, 7.0
    parts = flat_top(w, d, h, pale, parapet=0.9)
    for i in range(3):
        x = -5.0 + i * 5.0
        parts.append(k.box((3.8, 0.12, 2.4), glass, loc=(x, -d / 2 - 0.05, 1.6)))
        aw = k.box((4.2, 1.6, 0.15), roof, loc=(x, -d / 2 - 0.75, 3.3), rot=(-0.35, 0, 0))
        parts.append(aw)
    parts += windows(k, glass, w, d, 5.2, 6, sides=("front",), size=(1.2, 1.4))
    parts.append(k.box((w + 0.2, 0.3, 0.8), steel, loc=(0, -d / 2 - 0.15, 3.9)))
    out["mod_shops"] = parts

    # A suburban house: a low hipped roof, a porch and a garage.
    parts = [k.box((9.0, 7.0, 3.0), pale, loc=(0, 0, 1.5))]
    parts.append(hip_roof(k, 9.0, 7.0, 3.0, roof, rise=0.45, overhang=0.6))
    parts += windows(k, glass, 9.0, 7.0, 1.7, 3, size=(1.4, 1.2))
    parts.append(k.box((1.0, 0.15, 2.1), door, loc=(-1.5, -3.55, 1.05)))
    parts.append(k.box((4.0, 5.0, 2.6), tan, loc=(6.0, 1.0, 1.3)))
    parts.append(k.box((4.4, 5.4, 0.3), roof, loc=(6.0, 1.0, 2.7)))
    parts.append(k.box((3.0, 0.12, 2.0), steel, loc=(6.0, -1.55, 1.0)))
    parts.append(k.box((0.6, 0.6, 1.6), tan, loc=(-3.0, 1.5, 4.6)))
    out["mod_house"] = parts

    # The civic tower: a stone hall with a stepped art deco tower, a clock,
    # a spire and the flag.
    parts = [k.box((24.0, 14.0, 9.0), deco, loc=(0, 0, 4.5))]
    parts.append(k.box((24.4, 14.4, 0.6), steel, loc=(0, 0, 9.0)))
    parts.append(k.box((23.6, 13.6, 0.3), roof, loc=(0, 0, 9.3)))
    for z in (2.6, 6.4):
        parts += windows(k, glass, 24.0, 14.0, z, 8, sides=("front", "back"), size=(1.2, 2.4))
    for x in (-4.0, -1.3, 1.3, 4.0):
        parts.append(k.box((1.2, 1.2, 7.0), deco, loc=(x, -7.4, 3.5)))
    parts.append(k.box((4.0, 0.3, 3.4), door, loc=(0, -7.05, 1.7)))
    for wd, z0, z1 in ((9.0, 9.0, 24.0), (7.0, 24.0, 31.0), (5.0, 31.0, 36.0)):
        parts.append(k.box((wd, wd, z1 - z0), deco, loc=(0, 0, (z0 + z1) / 2)))
        for a in range(4):
            ang = a * math.pi / 2
            for i in (-1, 0, 1):
                off = Vector((math.cos(ang) * i * wd * 0.28, math.sin(ang) * i * wd * 0.28, 0))
                at = Vector((math.sin(ang) * (wd / 2 + 0.05), -math.cos(ang) * (wd / 2 + 0.05), (z0 + z1) / 2)) + off
                parts.append(k.box((0.7, 0.12, (z1 - z0) * 0.7), glass, loc=at, rot=(0, 0, ang)))
        parts.append(k.box((wd + 0.3, wd + 0.3, 0.5), roof, loc=(0, 0, z1)))
    clock = k.mat("mod_clock", k.srgb(240, 236, 220), 0.6)
    for a in range(4):
        ang = a * math.pi / 2
        parts.append(k.cyl(1.6, 0.15, clock, loc=(math.sin(ang) * 3.6, -math.cos(ang) * 3.6, 27.5), rot=(math.pi / 2, 0, ang), n=16))
    parts.append(k.cyl(1.6, 8.0, steel, loc=(0, 0, 40.0), n=8, r2=0.05, smooth=None))
    parts += pennant(k, (3.0, -3.0, 36.0), 5.0, k.flag(k.mat("civic_flag", k.srgb(240, 240, 240), 0.8, pattern="weave", scale=4, amount=0.1), "team"))
    out["civic_tower"] = parts
