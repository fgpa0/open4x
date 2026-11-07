"""Props for improvements and resources: buildings and gear of farms,
mines, camps, mills, plantations and quarries, animals, crops, rocks,
ores and sea life. Metres; each prop carries its own scale, matched to the
sizes the placement code in improvements.rs expects."""

import math
import random

import beasts
import bmesh
import bpy
import engines
import people
import ships
from mathutils import Vector

# Scales: buildings and big gear, small gear, animals.
BUILDING = 0.0105
GEAR = 0.022
BEAST = 0.021


def mesh_obj(name, bm, material, smooth=True):
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    o = bpy.data.objects.new(name, me)
    bpy.context.collection.objects.link(o)
    o.data.materials.append(material)
    if smooth:
        o.data.shade_smooth()
        o.data.set_sharp_from_angle(angle=0.7)
    return o


def rock(k, name, rng, r, material, squash=0.7, subdiv=2, loc=(0, 0, 0)):
    bm = bmesh.new()
    bmesh.ops.create_icosphere(bm, subdivisions=subdiv, radius=r)
    for v in bm.verts:
        v.co *= rng.uniform(0.8, 1.15)
        v.co.z *= squash
    o = mesh_obj(name, bm, material)
    o.location = Vector(loc) + Vector((0, 0, r * squash * 0.55))
    return o


def gable_house(k, p, w, d, h, roof, walls, pitch=0.8, overhang=0.25, loc=(0, 0, 0), yaw=0.0, chimney=False):
    """A rectangular building with a gable roof (ridge along X)."""
    loc = Vector(loc)
    out = [k.box((w, d, h), walls, loc=loc + Vector((0, 0, h / 2)), bevel=0.04)]
    rh = d / 2 * pitch
    rings = []
    for x in (-w / 2 - overhang, w / 2 + overhang):
        rings.append([loc + Vector((x, -d / 2 - overhang, h - 0.05)), loc + Vector((x, 0, h + rh + 0.05)), loc + Vector((x, d / 2 + overhang, h - 0.05)), loc + Vector((x, 0, h + rh - 0.2))])
    r = k.loft("roof", rings, roof, closed=True, cap=True, smooth=0.3)
    out.append(r)
    gable = []
    for s in (-1, 1):
        bm = bmesh.new()
        a = bm.verts.new(loc + Vector((s * w / 2, -d / 2, h)))
        b = bm.verts.new(loc + Vector((s * w / 2, d / 2, h)))
        c = bm.verts.new(loc + Vector((s * w / 2, 0, h + rh)))
        bm.faces.new((a, b, c) if s > 0 else (c, b, a))
        gable.append(mesh_obj("gable", bm, walls, smooth=False))
    out += gable
    if chimney:
        out.append(k.box((0.5, 0.5, rh + 0.9), p["stone"], loc=loc + Vector((w * 0.3, d * 0.15, h + rh * 0.5 + 0.3))))
    if yaw:
        k.pivot(out, loc, __import__("mathutils").Euler((0, 0, yaw)))
    return out


def windmill(k, p):
    parts = [k.cyl(3.3, 0.8, p["stone"], loc=(0, 0, 0.4), n=20, r2=3.2, smooth=0.5)]
    parts.append(k.cyl(3.0, 8.4, p["plaster"], loc=(0, 0, 5.0), n=20, r2=2.3, smooth=0.5))
    parts.append(k.box((1.1, 0.3, 1.9), p["dark_wood"], loc=(0, -3.0, 1.75), rot=(-0.05, 0, 0), bevel=0.06))
    for z, a in ((4.6, 0.7), (6.8, -0.6), (7.2, 2.4)):
        r = 3.0 - (z - 0.8) / 8.4 * 0.7
        parts.append(k.box((0.55, 0.3, 0.75), p["black"], loc=(r * math.sin(a), -r * math.cos(a), z), rot=(0, 0, a), bevel=0.04))
    parts.append(k.cyl(2.75, 0.25, p["dark_wood"], loc=(0, 0, 9.25), n=20, smooth=0.5))
    # A thatched cap, boat-shaped and pointed at the sails.
    rings = []
    for j in range(9):
        t = j / 8
        rr = 2.6 * math.sin(math.pi * (0.08 + 0.92 * t)) ** 0.6
        rings.append([(rr * math.cos(a), (t - 0.5) * 5.4 + 0.3, 9.3 + rr * 0.95 * max(0.0, math.sin(a))) for a in [i / 12 * math.pi for i in range(13)]])
    cap = k.loft("cap", rings, p["thatch"], closed=False, cap=False, smooth=0.6)
    cap.modifiers.new("s", "SOLIDIFY").thickness = 0.2
    parts.append(cap)
    parts.append(k.rod((0, 0, 10.2), (0, -2.6, 10.0), 0.35, p["dark_wood"], n=10))
    return parts


def windmill_sails(k, p):
    """The sails turning about the hub at the origin, facing -Y."""
    parts = [k.ball(0.55, p["dark_wood"])]
    for i in range(4):
        a = math.radians(20 + 90 * i)
        d = Vector((math.sin(a), 0.0, math.cos(a)))
        side = Vector((math.cos(a), 0.0, -math.sin(a)))
        parts.append(k.rod((0, 0, 0), d * 9.2, 0.17, p["wood"], n=6))
        for f in (0.3, 0.55, 0.8, 1.0):
            c = d * (2.0 + 7.2 * f) + side * 0.9
            parts.append(k.rod(c - side * 0.95, c + side * 0.95, 0.07, p["wood"], n=5))
        rail = side * 1.8
        parts.append(k.rod(rail + d * 2.0, rail + d * 9.2, 0.07, p["wood"], n=5))
        cloth = k.sheet("sail", 1.6, 6.6, p["linen"], nx=2, ny=6, bulge=0.12, thickness=0.05)
        cloth.rotation_euler = (0, a, 0)
        cloth.location = d * 5.6 + side * 0.95 + Vector((0, -0.15, 0))
        parts.append(cloth)
    return parts


def animal(k, p, kind, coat, points=None, rng=None, graze=False, extra=None):
    ex = dict(extra or {})
    if graze:
        # Head down to the grass.
        spec = beasts.SPECIES[kind]
        ex["neck"] = (spec["neck"][0], -15, spec["neck"][2])
        ex["head"] = (spec["head"][0], 80, spec["head"][2])
    parts, _ = beasts.quadruped(k, p, kind, coat, points=points, faces=700 if kind != "elephant" else 1200, extra=ex)
    return parts


def build(k):
    rng = random.Random(3)
    p = people.extend_palette(k, k.palette())
    out = {}
    out["windmill"] = (windmill(k, p), BUILDING)
    out["windmill_sails"] = (windmill_sails(k, p), BUILDING)

    # Mine: an adit framed in timber in a bank of rock, a cart, a spoil heap,
    # a winch shed.
    rock_m = k.mat("rock", k.srgb(112, 104, 96), 0.9, pattern="noise", scale=1.0, amount=0.45)
    adit = [rock(k, "bank", rng, 3.0, rock_m, squash=0.55, loc=(0, 1.2, 0))]
    adit.append(k.box((2.2, 1.2, 2.2), p["black"], loc=(0, -0.6, 1.1)))
    for s in (-1, 1):
        adit.append(k.box((0.3, 0.3, 2.5), p["dark_wood"], loc=(s * 1.25, -1.2, 1.25), bevel=0.04))
    adit.append(k.box((3.1, 0.35, 0.35), p["dark_wood"], loc=(0, -1.2, 2.55), bevel=0.04))
    out["mine_adit"] = (adit, BUILDING * 1.3)
    cart = [k.box((1.3, 1.8, 0.75), p["planks"], loc=(0, 0, 0.75), bevel=0.04)]
    for x in (-0.55, 0.55):
        for y in (-0.6, 0.6):
            cart.append(k.cyl(0.32, 0.12, p["iron"], loc=(x * 1.12, y, 0.32), rot=(0, math.pi / 2, 0), n=12))
    cart.append(rock(k, "ore", rng, 0.6, k.mat("ore", k.srgb(80, 70, 66), 0.8, pattern="noise", scale=4, amount=0.4), squash=0.6, loc=(0, 0, 1.0)))
    out["mine_cart"] = (cart, GEAR * 0.85)
    out["spoil_heap"] = ([rock(k, "heap", rng, 2.4, k.mat("spoil", k.srgb(96, 88, 80), 0.95, pattern="noise", scale=3, amount=0.4), squash=0.5)], BUILDING * 1.6)
    shed = gable_house(k, p, 4.0, 3.2, 2.2, p["dark_wood"], p["planks"], pitch=0.7)
    shed.append(k.cyl(0.5, 2.4, p["wood"], loc=(0, -2.0, 1.2), rot=(0, math.pi / 2, 0), n=10))
    out["winch_shed"] = (shed, BUILDING * 1.3)

    # Camp: hide tents, a fire, a drying rack.
    hide = k.mat("hide", k.srgb(178, 140, 96), 0.9, pattern="noise", scale=2, amount=0.35)
    tent = [k.cyl(1.9, 3.2, hide, loc=(0, 0, 1.6), n=9, r2=0.15, smooth=0.4)]
    for i in range(5):
        a = i / 5 * math.tau + 0.3
        tent.append(k.rod((math.cos(a) * 0.12, math.sin(a) * 0.12, 3.1), (math.cos(a) * 0.45, math.sin(a) * 0.45, 4.0), 0.06, p["wood"], n=4))
    tent.append(k.box((0.9, 0.2, 1.3), k.mat("tent_door", k.srgb(70, 50, 36), 0.9), loc=(0, -1.25, 0.65), rot=(0.4, 0, 0)))
    out["tent"] = (tent, GEAR * 0.6)
    fire = [k.cyl(0.8, 0.15, k.mat("ash", k.srgb(60, 56, 52), 0.95), loc=(0, 0, 0.05), n=10)]
    for i in range(7):
        a = i / 7 * math.tau
        fire.append(rock(k, "ring", rng, 0.22, p["stone"], loc=(math.cos(a) * 0.85, math.sin(a) * 0.85, 0), subdiv=1))
    for i in range(3):
        a = i / 3 * math.tau
        fire.append(k.rod((math.cos(a) * 0.5, math.sin(a) * 0.5, 0.1), (0, 0, 0.6), 0.08, p["dark_wood"], n=5))
    fire.append(k.cyl(0.3, 0.6, k.mat("flame", k.srgb(250, 150, 40), 0.5), loc=(0, 0, 0.45), n=6, r2=0.02))
    out["campfire"] = (fire, GEAR * 0.7)
    rack = [k.rod((x, 0, 0), (x, 0, 2.2), 0.06, p["wood"], n=5) for x in (-1.5, 1.5)]
    rack.append(k.rod((-1.6, 0, 2.1), (1.6, 0, 2.1), 0.05, p["wood"], n=5))
    for x in (-0.8, 0.6):
        pelt = k.sheet("pelt", 1.1, 1.5, k.mat("pelt", k.srgb(150, 104, 62), 0.9, pattern="hair", scale=10, amount=0.3), nx=3, ny=3, bulge=0.05, thickness=0.04)
        pelt.location = (x, 0, 1.35)
        rack.append(pelt)
    out["drying_rack"] = (rack, GEAR * 0.7)

    # Lumber mill: an open saw shed with a round blade, log piles, stumps.
    mill = gable_house(k, p, 6.0, 4.0, 2.6, p["dark_wood"], p["planks"], pitch=0.6)
    mill.append(k.cyl(0.9, 0.08, p["steel"], loc=(3.3, 0, 1.2), rot=(math.pi / 2, 0, 0), n=20))
    mill.append(k.box((2.0, 0.8, 0.9), p["wood"], loc=(3.6, 0, 0.45)))
    out["sawmill"] = (mill, BUILDING * 1.6)
    logs = []
    log_m = k.mat("log", k.srgb(150, 104, 62), 0.85, pattern="grain", scale=2, amount=0.3)
    end_m = k.mat("log_end", k.srgb(210, 176, 120), 0.8)
    for i, (x, z) in enumerate([(-0.75, 0.38), (0, 0.38), (0.75, 0.38), (-0.38, 1.04), (0.38, 1.04), (0, 1.7)]):
        logs.append(k.cyl(0.38, 4.0, log_m, loc=(x, 0, z), rot=(math.pi / 2, 0, 0), n=9))
        for s in (-1, 1):
            logs.append(k.cyl(0.36, 0.02, end_m, loc=(x, s * 2.0, z), rot=(math.pi / 2, 0, 0), n=9))
    out["log_pile"] = (logs, GEAR * 0.9)
    out["stump"] = ([k.cyl(0.45, 0.6, log_m, loc=(0, 0, 0.3), n=8), k.cyl(0.42, 0.02, end_m, loc=(0, 0, 0.61), n=8)], GEAR)

    # Quarry: stepped cuts, cut blocks, a wooden crane.
    pale = k.mat("pale_stone", k.srgb(212, 204, 190), 0.85, pattern="noise", scale=1.5, amount=0.25)
    steps = [k.box((7.0 - i * 1.8, 5.0 - i * 1.2, 0.9), pale, loc=(i * 0.9, 0, 0.45 + i * 0.9), bevel=0.05) for i in range(3)]
    out["quarry_cut"] = (steps, BUILDING * 1.6)
    out["stone_block"] = ([k.box((1.4, 1.0, 0.8), pale, loc=(0, 0, 0.4), bevel=0.05)], GEAR * 0.75)
    crane = [engines.beam(k, p, (0, 0, 0), (0, 0, 5.5), 0.3), engines.beam(k, p, (-0.6, 0, 5.2), (4.5, 0, 6.2), 0.22)]
    crane.append(engines.beam(k, p, (0, 0, 2.0), (2.2, 0, 5.8), 0.18))
    crane.append(k.rod((4.3, 0, 6.1), (4.3, 0, 2.2), 0.04, p["rope"], n=4))
    crane.append(k.box((0.9, 0.7, 0.6), pale, loc=(4.3, 0, 1.9)))
    for s in (-1, 1):
        crane.append(engines.beam(k, p, (0, 0, 1.6), (s * 1.6, 0.8, 0), 0.15))
    out["crane"] = (crane, BUILDING * 1.6)

    # Plantation crops.
    leafy = lambda name, rgb: k.flag(k.mat(name, k.srgb(*rgb), 0.85, pattern="noise", scale=3, amount=0.3), "foliage")  # noqa: E731
    def bush(name, leaf_rgb, fruit_rgb=None, tall=False, stake=False):
        lm = leafy(name + "_leaf", leaf_rgb)
        parts = []
        z = 1.3 if tall else 0.0
        if tall or stake:
            parts.append(k.rod((0, 0, 0), (0, 0, z + 0.6), 0.08, p["dark_wood"], n=5))
        el = [("ball", (rng.uniform(-0.4, 0.4), rng.uniform(-0.4, 0.4), z + 0.6 + rng.uniform(-0.2, 0.3)), 0.6 + rng.uniform(-0.1, 0.15)) for _ in range(5)]
        parts.append(k.blobs(name, el, lm, res=0.12, faces=90))
        if fruit_rgb:
            fm = k.mat(name + "_fruit", k.srgb(*fruit_rgb), 0.5)
            for i in range(5):
                a = i / 5 * math.tau + 0.4
                parts.append(k.ball(0.14, fm, loc=(math.cos(a) * 0.75, math.sin(a) * 0.75, z + 0.45 + 0.2 * (i % 2)), subdiv=1))
        return parts
    out["vine"] = (bush("vine", (92, 132, 58), (104, 40, 96), stake=True), GEAR * 0.75)
    out["spice_bush"] = (bush("spice", (74, 120, 50), (196, 64, 40)), GEAR * 0.75)
    out["mulberry"] = (bush("mulberry", (70, 118, 52), (246, 244, 236), tall=True), GEAR * 0.75)
    out["incense_tree"] = (bush("incense", (150, 160, 104), (214, 150, 60), tall=True), GEAR * 0.75)
    out["orchard_tree"] = (bush("orchard", (84, 136, 58), (200, 70, 60), tall=True), GEAR * 0.75)
    banana_leaf = leafy("banana_leaf", (96, 160, 64))
    ban = [k.rod((0, 0, 0), (0.1, 0, 2.2), 0.18, k.mat("banana_stem", k.srgb(110, 120, 60), 0.8), n=6, r2=0.12)]
    for i in range(6):
        a = i / 6 * math.tau
        d = Vector((math.cos(a), math.sin(a), 0))
        lf = k.sheet("banana_frond", 0.7, 2.0, banana_leaf, nx=2, ny=4, bulge=0.15, thickness=0.04)
        lf.rotation_euler = (math.radians(55), 0, a + math.pi / 2)
        lf.location = Vector((0.1, 0, 2.3)) + d * 0.8
        ban.append(lf)
    ban.append(k.ball(0.3, k.mat("banana_fruit", k.srgb(236, 210, 76), 0.6), loc=(0.35, 0, 1.7), scale=(0.7, 0.7, 1.3), subdiv=1))
    out["banana"] = (ban, GEAR * 0.75)

    # Wheat stook (a sheaf of grain) for fields.
    straw = k.mat("stook", k.srgb(222, 186, 84), 0.9, pattern="hair", scale=20, amount=0.3)
    out["stook"] = ([k.cyl(0.55, 1.4, straw, loc=(0, 0, 0.7), n=8, r2=0.12), k.cyl(0.6, 0.15, k.mat("band", k.srgb(170, 130, 60), 0.9), loc=(0, 0, 0.75), n=8)], GEAR * 1.1)

    # Animals (standing and grazing).
    cow_brown = k.mat("cow_brown", k.srgb(96, 70, 52), 0.85, pattern="noise", scale=3, amount=0.3)
    cow_pied = k.mat("cow_pied", k.srgb(236, 230, 220), 0.85, pattern="noise", scale=1.2, amount=0.9, color2=k.srgb(40, 34, 30))
    p["horn_light"] = k.mat("horn_light", k.srgb(226, 214, 186), 0.5)
    out["cow"] = (animal(k, p, "cow", cow_brown, points=k.mat("cow_dark", k.srgb(60, 46, 38), 0.8), graze=True), BEAST)
    out["cow_pied"] = (animal(k, p, "cow", cow_pied, points=k.mat("cow_dark2", k.srgb(48, 40, 36), 0.8)), BEAST)
    p["wool"] = k.mat("wool", k.srgb(236, 230, 214), 0.95, pattern="noise", scale=30, amount=0.25)
    out["sheep"] = (animal(k, p, "sheep", k.mat("sheep_face", k.srgb(60, 52, 46), 0.8), graze=True), BEAST * 1.7)
    bay = k.mat("bay", k.srgb(128, 76, 42), 0.78, pattern="noise", scale=5, amount=0.35, color2=k.srgb(84, 46, 26))
    p["mane"] = k.mat("points", k.srgb(46, 34, 26), 0.7, pattern="hair", scale=10, amount=0.3)
    out["horse"] = (animal(k, p, "horse", bay, points=p["mane"], graze=True), BEAST)
    out["deer"] = (animal(k, p, "deer", k.mat("deer", k.srgb(156, 110, 66), 0.85, pattern="noise", scale=4, amount=0.3)), BEAST * 1.25)
    out["fox"] = (animal(k, p, "fox", k.mat("fox", k.srgb(196, 98, 42), 0.85, pattern="noise", scale=6, amount=0.25)), BEAST * 0.9)
    out["elephant"] = (animal(k, p, "elephant", k.mat("elephant", k.srgb(132, 128, 124), 0.9, pattern="noise", scale=2, amount=0.3)), BEAST * 0.75)

    # Rocks and ores.
    def outcrop(name, rgb, n=4, size=1.2):
        m = k.mat(name, k.srgb(*rgb), 0.85, pattern="noise", scale=1.5, amount=0.4)
        return [rock(k, name, rng, size * rng.uniform(0.6, 1.0), m, loc=(math.cos(i * 2.4) * size * 0.9 * (i > 0), math.sin(i * 2.4) * size * 0.9 * (i > 0), 0)) for i in range(n)]
    out["rocks"] = (outcrop("granite", (132, 126, 118)), GEAR * 0.75)
    out["iron_rocks"] = (outcrop("iron_rock", (132, 72, 52)), GEAR * 0.75)
    dark = outcrop("dark_rock", (96, 88, 82))
    gold = k.mat("gold", k.srgb(255, 196, 30), 0.25, metal=1.0)
    out["gold_rocks"] = (dark + [k.ball(0.3, gold, loc=(math.cos(i * 1.3) * 1.3, math.sin(i * 1.3) * 1.3, 0.8 + 0.2 * (i % 2)), subdiv=1) for i in range(6)], GEAR * 0.75)
    marble = k.mat("marble", k.srgb(232, 228, 220), 0.4, pattern="streaks", scale=1.5, amount=0.25)
    out["marble_blocks"] = ([k.box((1.6, 1.1, 1.0), marble, loc=(0, 0, 0.5), bevel=0.04), k.box((1.2, 1.2, 0.9), marble, loc=(1.6, 0.6, 0.45), rot=(0, 0, 0.4), bevel=0.04), k.box((1.3, 0.9, 0.8), marble, loc=(0.4, 0.2, 1.4), rot=(0, 0, 0.9), bevel=0.04)], GEAR * 0.75)
    gems = outcrop("gem_rock", (96, 88, 82), n=3)
    for i, rgb in enumerate([(150, 60, 210), (40, 180, 170), (220, 40, 70), (150, 60, 210), (40, 180, 170)]):
        g = k.cyl(0.25, 1.4, k.mat(f"gem{i % 3}", k.srgb(*rgb), 0.1, metal=0.3), loc=(math.cos(i * 1.25) * 0.8, math.sin(i * 1.25) * 0.8, 1.0), rot=(rng.uniform(-0.5, 0.5), rng.uniform(-0.5, 0.5), 0), n=6, r2=0.02, smooth=None)
        gems.append(g)
    out["gems"] = (gems, GEAR * 0.75)
    salt = k.mat("salt", k.srgb(246, 246, 244), 0.6, pattern="noise", scale=5, amount=0.1)
    out["salt"] = ([k.cyl(3.0, 0.1, k.mat("salt_pan", k.srgb(232, 230, 222), 0.4), loc=(0, 0, 0.05), n=14)] + [k.cyl(0.7, 1.0, salt, loc=(math.cos(i * 1.6) * 1.5, math.sin(i * 1.6) * 1.5, 0.5), n=7, r2=0.05) for i in range(4)], GEAR * 0.75)

    # Sea life: a leaping fish over ripples, an open clam with a pearl.
    fish_m = k.mat("fish", k.srgb(176, 190, 200), 0.3, metal=0.5)
    fish = [k.blobs("fish", [("ellipsoid", (0, 0, 0.9), (0.5, 0.14, 0.18), 1.0, (0, 0.5, 0)), ("ellipsoid", (-0.55, 0, 1.15), (0.1, 0.03, 0.22), 1.0, (0, 0.5, 0))], fish_m, res=0.03, faces=150)]
    for r, c in ((1.2, (196, 222, 232)), (0.8, (120, 170, 196))):
        ring = k.loft("ripple", [[(math.cos(a) * rr, math.sin(a) * rr, 0.02) for a in [i / 20 * math.tau for i in range(20)]] for rr in (r, r - 0.15)], k.mat(f"ripple{r}", k.srgb(*c), 0.2), closed=True, cap=False)
        fish.append(ring)
    out["fish"] = (fish, GEAR * 0.75)
    shell = k.mat("shell", k.srgb(214, 190, 172), 0.5, pattern="streaks", scale=8, amount=0.3)
    clam = [k.ball(0.8, shell, loc=(0, 0, 0.15), scale=(1.2, 1.0, 0.3)), k.ball(0.8, shell, loc=(0, 0.5, 0.75), scale=(1.2, 1.0, 0.3), rot=(-0.9, 0, 0))]
    clam.append(k.ball(0.22, k.mat("pearl", k.srgb(250, 246, 240), 0.15, metal=0.2), loc=(0, 0, 0.4)))
    out["clam"] = (clam, GEAR * 0.75)

    # Oil: a black seep in the ground with a sheen, and the Oil Well's
    # timber derrick, a nodding pumpjack (its beam pivots on the samson post),
    # storage tanks and barrels.
    crude = k.mat("crude", k.srgb(22, 20, 22), 0.12, metal=0.3)
    sheen = k.mat("crude_sheen", k.srgb(70, 56, 90), 0.08, metal=0.6)
    seep = [k.cyl(1.4, 0.08, crude, loc=(0, 0, 0.04), n=14), k.cyl(0.8, 0.09, sheen, loc=(0.3, 0.2, 0.05), n=12)]
    seep.append(k.cyl(0.6, 0.08, crude, loc=(1.6, -0.6, 0.04), n=10))
    seep += [rock(k, "seep_rock", rng, 0.35, k.mat("tar_rock", k.srgb(70, 62, 56), 0.7, pattern="noise", scale=3, amount=0.4), loc=(math.cos(i * 2.1) * 1.7, math.sin(i * 2.1) * 1.5, 0), subdiv=1) for i in range(3)]
    out["oil_seep"] = (seep, GEAR * 0.75)
    derrick = []
    H, W = 12.0, 2.4
    for sx in (-1, 1):
        for sy in (-1, 1):
            derrick.append(engines.beam(k, p, (sx * W, sy * W, 0), (sx * 0.4, sy * 0.4, H), 0.22, p["dark_wood"]))
    for z in (2.5, 5.0, 7.5, 10.0):
        f = W - (W - 0.4) * z / H
        for (ax, ay), (bx, by) in (((-1, -1), (1, -1)), ((1, -1), (1, 1)), ((1, 1), (-1, 1)), ((-1, 1), (-1, -1))):
            derrick.append(engines.beam(k, p, (ax * f, ay * f, z), (bx * f, by * f, z), 0.14, p["wood"]))
    for z0, z1 in ((0.0, 2.5), (2.5, 5.0), (5.0, 7.5), (7.5, 10.0)):
        f0, f1 = W - (W - 0.4) * z0 / H, W - (W - 0.4) * z1 / H
        derrick.append(engines.beam(k, p, (-f0, -f0, z0), (f1, -f1, z1), 0.1, p["wood"]))
        derrick.append(engines.beam(k, p, (f0, f0, z0), (-f1, f1, z1), 0.1, p["wood"]))
    derrick.append(k.box((1.4, 1.4, 0.4), p["dark_wood"], loc=(0, 0, H + 0.2)))
    derrick.append(k.cyl(0.35, 0.2, p["iron"], loc=(0, 0, H + 0.55), rot=(0, math.pi / 2, 0), n=12))
    derrick.append(k.box((2.0, 2.0, 0.25), p["planks"], loc=(0, 0, 0.12)))
    derrick += gable_house(k, p, 3.0, 2.4, 2.2, p["dark_wood"], p["planks"], pitch=0.6, loc=(3.4, 0.0, 0))
    out["derrick"] = (derrick, BUILDING * 1.3)
    jack_m = k.mat("pumpjack", k.srgb(58, 60, 64), 0.45, metal=0.7)
    jack = [k.box((5.0, 1.2, 0.4), p["stone"], loc=(0.5, 0, 0.2))]
    for s in (-1, 1):
        jack.append(k.rod((0.3, s * 0.45, 0.4), (0.0, s * 0.12, 3.2), 0.1, jack_m, n=6))
        jack.append(k.rod((-0.4, s * 0.45, 0.4), (0.0, s * 0.12, 3.2), 0.1, jack_m, n=6))
    jack.append(k.box((4.6, 0.3, 0.45), jack_m, loc=(-0.2, 0, 3.35), rot=(0, 0.12, 0)))
    head = k.cyl(0.9, 0.35, jack_m, loc=(-2.5, 0, 3.0), rot=(math.pi / 2, 0, 0), n=16)
    jack.append(head)
    jack.append(k.rod((-3.35, 0, 2.9), (-3.35, 0, 0.6), 0.04, p["steel"], n=4))
    jack.append(k.cyl(0.2, 0.6, jack_m, loc=(-3.35, 0, 0.3), n=8))
    jack.append(k.box((1.0, 1.0, 1.0), k.mat("jack_motor", k.srgb(150, 54, 40), 0.6), loc=(2.4, 0, 0.9)))
    jack.append(k.cyl(0.7, 0.2, jack_m, loc=(2.0, 0.55, 1.4), rot=(math.pi / 2, 0, 0), n=14))
    out["pumpjack"] = (jack, BUILDING * 1.3)
    tank_m = k.mat("oil_tank", k.srgb(196, 190, 178), 0.5, metal=0.4)
    tanks = [k.cyl(1.6, 2.6, tank_m, loc=(0, 0, 1.3), n=20), k.cyl(1.62, 0.3, crude, loc=(0, 0, 2.6), n=20, r2=0.9)]
    tanks.append(k.box((0.1, 0.4, 2.6), p["iron"], loc=(1.6, 0, 1.3)))
    for i in range(3):
        tanks += ships.barrel(k, p, (2.4 + (i % 2) * 0.9, -0.8 + i * 0.8, 0), r=0.4, h=0.9)
    out["oil_tanks"] = (tanks, GEAR * 0.9)

    # Border posts: a palisade fort and a watchtower. Roofs and cloth take
    # the owner's colour.
    team_roof = k.flag(k.mat("post_roof", k.srgb(236, 230, 224), 0.8, pattern="streaks", scale=3.0, amount=0.25), "team")
    team_cloth = k.flag(k.mat("post_cloth", k.srgb(240, 240, 240), 0.85, pattern="weave", scale=4, amount=0.1), "team")
    stake = k.mat("stake", k.srgb(132, 96, 60), 0.8, pattern="grain", scale=2, amount=0.35)
    fort = []
    R = 11.0
    gate = -math.pi / 2  # the gate faces -Y (towards the camera)
    n = int(math.tau * R / 0.9)
    for i in range(n):
        a = i / n * math.tau
        if abs(math.remainder(a - gate, math.tau)) < 0.2:
            continue
        x, y = math.cos(a) * R, math.sin(a) * R
        h = 4.6 + rng.uniform(-0.3, 0.3)
        fort.append(k.cyl(0.45, h, stake, loc=(x, y, h / 2), n=6, smooth=None))
        fort.append(k.cyl(0.45, 0.9, stake, loc=(x, y, h + 0.45), n=6, r2=0.03, smooth=None))
    # A walkway inside the wall, and an earth bank outside it.
    bank = k.mat("bank", k.srgb(118, 100, 74), 0.95, pattern="noise", scale=2, amount=0.35)
    fort.append(k.loft("bank", [[(math.cos(t) * rr, math.sin(t) * rr, z) for t in [i / 32 * math.tau for i in range(32)]] for rr, z in ((R + 2.4, 0.0), (R + 0.6, 1.0), (R - 0.2, 1.0))], bank, closed=True, cap=False))
    for i in range(16):
        a = i / 16 * math.tau
        if abs(math.remainder(a - gate, math.tau)) < 0.3:
            continue
        fort.append(k.box((2.2, 1.4, 0.25), p["planks"], loc=(math.cos(a) * (R - 1.3), math.sin(a) * (R - 1.3), 3.0), rot=(0, 0, a + math.pi / 2)))
    # Towers at the corners and either side of the gate.
    for a in [gate - 0.42, gate + 0.42, gate + math.pi * 0.75, gate - math.pi * 0.75]:
        x, y = math.cos(a) * R, math.sin(a) * R
        for sx in (-1, 1):
            for sy in (-1, 1):
                fort.append(k.rod((x + sx * 1.3, y + sy * 1.3, 0), (x + sx * 1.1, y + sy * 1.1, 7.0), 0.22, p["dark_wood"], n=5))
        fort.append(k.box((3.0, 3.0, 2.0), p["planks"], loc=(x, y, 7.6), bevel=0.04))
        fort.append(k.cyl(2.6, 2.6, team_roof, loc=(x, y, 9.9), rot=(0, 0, math.pi / 4), n=4, r2=0.08, smooth=None))
    # The gate: posts, a lintel and doors.
    gx, gy = math.cos(gate) * R, math.sin(gate) * R
    for s in (-1, 1):
        fort.append(k.box((0.7, 0.7, 6.0), p["dark_wood"], loc=(gx + s * 2.0, gy, 3.0), bevel=0.04))
        fort.append(k.box((1.7, 0.3, 4.2), p["planks"], loc=(gx + s * 1.0, gy - 0.2, 2.1), rot=(0, 0, s * 0.5)))
    fort.append(k.box((5.0, 0.8, 0.8), p["dark_wood"], loc=(gx, gy, 6.0), bevel=0.04))
    # Inside: a barracks, a store and the flag.
    fort += gable_house(k, p, 7.0, 4.0, 2.8, team_roof, p["planks"], pitch=0.7, loc=(0, 3.5, 0))
    fort += gable_house(k, p, 4.0, 3.0, 2.2, p["thatch"], p["planks"], pitch=0.8, loc=(-4.5, -2.0, 0), yaw=0.6)
    fort.append(k.rod((3.5, -2.5, 0), (3.5, -2.5, 11.0), 0.12, p["dark_wood"], n=6))
    banner = k.sheet("fort_banner", 3.2, 2.0, team_cloth, nx=6, ny=4, bulge=0.3, thickness=0.06)
    banner.location = (5.1, -2.5, 10.0)
    fort.append(banner)
    out["fort"] = (fort, BUILDING)

    tower = []
    for sx in (-1, 1):
        for sy in (-1, 1):
            tower.append(k.rod((sx * 2.4, sy * 2.4, 0), (sx * 1.5, sy * 1.5, 9.4), 0.25, p["dark_wood"], n=6))
    for z0, z1 in ((0.6, 4.6), (4.6, 8.4)):
        f0, f1 = 2.4 - (2.4 - 1.5) * z0 / 9.4, 2.4 - (2.4 - 1.5) * z1 / 9.4
        for (ax, ay), (bx, by) in (((-1, -1), (1, -1)), ((1, -1), (1, 1)), ((1, 1), (-1, 1)), ((-1, 1), (-1, -1))):
            tower.append(k.rod((ax * f0, ay * f0, z0), (bx * f1, by * f1, z1), 0.12, p["wood"], n=4))
    tower.append(k.box((4.4, 4.4, 0.4), p["planks"], loc=(0, 0, 9.6), bevel=0.04))
    for sx in (-1, 1):
        for sy in (-1, 1):
            tower.append(k.rod((sx * 2.0, sy * 2.0, 9.8), (sx * 2.0, sy * 2.0, 12.0), 0.14, p["dark_wood"], n=5))
    for i, (w, d, x, y) in enumerate(((4.2, 0.18, 0, -2.0), (4.2, 0.18, 0, 2.0), (0.18, 4.2, -2.0, 0), (0.18, 4.2, 2.0, 0))):
        tower.append(k.box((w, d, 0.18), p["wood"], loc=(x, y, 10.8)))
    tower.append(k.cyl(3.6, 2.4, team_roof, loc=(0, 0, 13.2), rot=(0, 0, math.pi / 4), n=4, r2=0.1, smooth=None))
    for s in (-1, 1):
        tower.append(k.rod((s * 0.45, -2.9, 0), (s * 0.45, -2.1, 9.5), 0.07, p["wood"], n=4))
    for i in range(12):
        t = (i + 0.5) / 12
        tower.append(k.rod((-0.45, -2.9 + 0.8 * t, 9.5 * t), (0.45, -2.9 + 0.8 * t, 9.5 * t), 0.05, p["wood"], n=4))
    tower.append(k.rod((1.8, 1.8, 12.0), (1.8, 1.8, 15.5), 0.08, p["dark_wood"], n=5))
    pennant = k.sheet("tower_pennant", 1.8, 1.0, team_cloth, nx=4, ny=3, bulge=0.2, thickness=0.05)
    pennant.location = (2.7, 1.8, 15.0)
    tower.append(pennant)
    out["watchtower"] = (tower, BUILDING)

    # Fishing boats: a moored skiff with a heap of net, a weir of stakes and
    # wattle, and a line of net floats. They stand at the water line.
    net_m = k.mat("net", k.srgb(150, 130, 96), 0.95, pattern="weave", scale=40, amount=0.5)
    cork_m = k.mat("cork", k.srgb(206, 150, 74), 0.8)
    skiff = ships.ship_boat(k, p, (0, 0, -0.25), length=4.2)
    skiff.append(k.box((1.1, 0.2, 0.06), p["planks"], loc=(0, 0.5, 0.25)))
    skiff.append(k.ball(0.45, net_m, loc=(0.0, -1.1, 0.25), scale=(1.0, 1.1, 0.4)))
    skiff.append(k.rod((0.0, 0.2, 0.2), (0.0, 0.2, 2.6), 0.05, p["dark_wood"], n=5))
    skiff.append(k.cyl(0.11, 1.5, k.mat("furled", k.srgb(214, 196, 160), 0.9), loc=(0, 0.2, 2.2), rot=(0, math.pi / 2, 0), n=8))
    for s_ in (-1, 1):
        skiff.append(k.rod((s_ * 0.55, 0.8, 0.4), (s_ * 1.9, 1.6, -0.1), 0.035, p["wood"], n=4))
    out["skiff"] = (skiff, GEAR * 0.9)
    weir = []
    wattle = k.mat("wattle", k.srgb(136, 106, 70), 0.95, pattern="weave", scale=14, amount=0.45)
    n = 11
    for i in range(n):
        a = -1.1 + i / (n - 1) * 3.6
        x, y = math.cos(a) * 3.0, math.sin(a) * 3.0
        weir.append(k.rod((x, y, -0.8), (x, y, 1.3 + rng.uniform(-0.15, 0.15)), 0.09, p["dark_wood"], n=5))
        if i + 1 < n:
            b_ = -1.1 + (i + 1) / (n - 1) * 3.6
            mid = (a + b_) / 2
            seg = k.box((0.12, 3.0 * (b_ - a) * 1.02, 0.9), wattle, loc=(math.cos(mid) * 3.0, math.sin(mid) * 3.0, 0.25), rot=(0, 0, mid))
            weir.append(seg)
    out["fish_weir"] = (weir, GEAR * 0.75)
    floats = [k.rod((-3.0, 0, 0.12), (3.0, 0, 0.12), 0.025, p["rope"], n=4)]
    for i in range(7):
        floats.append(k.ball(0.22, cork_m, loc=(-3.0 + i, 0, 0.12), scale=(1.0, 1.0, 0.6), subdiv=1))
    floats.append(k.rod((3.0, 0, -0.4), (3.0, 0, 1.4), 0.05, p["dark_wood"], n=5))
    floats.append(k.box((0.5, 0.04, 0.32), k.flag(k.mat("float_flag", k.srgb(240, 240, 240), 0.8), "team"), loc=(3.27, 0, 1.25)))
    out["net_floats"] = (floats, GEAR * 0.8)

    # Ambient life (the game moves these): a fish leaping clear of the water,
    # a whale's back and its tail flukes (pivoting at the tail stock, which
    # the body's tail end meets), a lamp post for roads near towns.
    out["leaping_fish"] = ([k.blobs("leaper", [("ellipsoid", (0, 0, 0), (0.12, 0.5, 0.16), 1.0, (0, 0, 0)), ("ellipsoid", (0, 0.55, 0.0), (0.02, 0.16, 0.2), 1.0, (0, 0, 0))], fish_m, res=0.03, faces=150)], GEAR * 1.2)
    whale_m = k.mat("whale", k.srgb(58, 66, 78), 0.45, pattern="noise", scale=2, amount=0.3, color2=k.srgb(40, 46, 56))
    belly_m = k.mat("whale_belly", k.srgb(168, 172, 170), 0.5)
    body = [k.blobs("whale", [("ellipsoid", (0, -0.5, 0), (1.5, 5.0, 1.3), 1.0, (0, 0, 0)), ("ellipsoid", (0, 3.5, 0.1), (0.7, 2.6, 0.6), 1.0, (0, 0, 0)), ("ellipsoid", (0, 2.4, 1.1), (0.12, 0.5, 0.45), 1.0, (0.4, 0, 0))], whale_m, res=0.12, faces=900)]
    body.append(k.ball(1.0, belly_m, loc=(0, -2.0, -0.75), scale=(1.2, 2.6, 0.5)))
    for s in (-1, 1):
        body.append(k.ball(0.5, whale_m, loc=(s * 1.6, -2.2, -0.5), scale=(1.4, 0.5, 0.12), rot=(0, s * 0.4, 0)))
    out["whale_body"] = (body, BEAST * 0.8)
    fluke = [k.blobs("fluke", [("ellipsoid", (s * 1.0, 0.8, 0), (1.2, 0.55, 0.1), 1.0, (0, 0, -s * 0.35)) for s in (-1, 1)] + [("ellipsoid", (0, 0.2, 0), (0.35, 0.6, 0.25), 1.0, (0, 0, 0))], whale_m, res=0.06, faces=300)]
    out["whale_fluke"] = (fluke, BEAST * 0.8)
    post = [k.rod((0, 0, 0), (0, 0, 2.6), 0.07, p["dark_wood"], n=6), k.rod((0, 0, 2.5), (0.45, 0, 2.5), 0.04, p["dark_wood"], n=5)]
    post.append(k.box((0.3, 0.3, 0.38), p["iron"], loc=(0.45, 0, 2.2), bevel=0.03))
    post.append(k.box((0.22, 0.22, 0.26), k.mat("lamp_glass", k.srgb(250, 210, 140), 0.3), loc=(0.45, 0, 2.2)))
    out["lamp_post"] = (post, GEAR * 0.9)
    return out, {"kind": "props", "scale": 1.0, "ao_distance": 1.5, "foliage_shade": 0.3}
