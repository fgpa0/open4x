"""People for the game's units: one body, posed and dressed per unit.

`figure` grows a body from metaballs, paints it by body segment (skin,
tunic, armour, trousers, boots), adds its bones to a rig and returns the
parts and its joint positions, so a unit's script can put weapons in its
hands. Helmets, shields and weapons are here too, each bound to a bone.

Coordinates are metres, facing -Y (the figure's right hand is at -X).
"""

import math

from mathutils import Vector

# Arm poses: (elbow, wrist) for the right arm (x < 0); the left mirrors x.
ARMS = {
    "hang": ((-0.25, 0.02, 1.17), (-0.28, -0.03, 0.93)),
    "hold": ((-0.27, 0.03, 1.18), (-0.27, -0.21, 1.12)),  # forearm forward, fist at the waist
    "raise": ((-0.31, -0.06, 1.26), (-0.30, -0.24, 1.46)),  # weapon ready, fist at the shoulder
    "high": ((-0.32, 0.0, 1.68), (-0.30, -0.08, 1.92)),  # fist above the head
    "shield": ((-0.29, -0.06, 1.16), (-0.22, -0.31, 1.18)),
    "reach": ((-0.21, -0.30, 1.40), (-0.17, -0.56, 1.42)),  # arm out in front
    "draw": ((-0.27, 0.02, 1.38), (-0.10, -0.18, 1.45)),  # fist at the chin
    "low_fwd": ((-0.25, -0.08, 1.12), (-0.14, -0.33, 1.04)),  # both hands on a pole at the hip
    "high_fwd": ((-0.25, -0.12, 1.20), (-0.08, -0.42, 1.18)),
    "shoulder": ((-0.30, -0.02, 1.22), (-0.24, -0.06, 1.48)),  # fist on the shoulder
}

SHOULDER = (-0.19, 0.0, 1.45)
HIP = (-0.10, 0.0, 0.94)
KNEE = (-0.11, -0.02, 0.52)
ANKLE = (-0.11, 0.01, 0.09)
TOE = (-0.115, -0.13, 0.035)


def _mirror(v, side):
    return Vector((v[0] * side, v[1], v[2]))


def seg_dist(p, a, b):
    p, a, b = Vector(p), Vector(a), Vector(b)
    ab = b - a
    t = 0.0 if ab.length_squared < 1e-12 else max(0.0, min(1.0, (p - a).dot(ab) / ab.length_squared))
    return (p - (a + ab * t)).length


def figure(k, p, rig, *, parent=None, prefix="", at=(0, 0, 0), yaw=0.0, arms=("hang", "hang"), mounted=None, stride=0.0, outfit=None):
    """A person standing at `at` (feet on z = at.z), or riding with hips at
    `mounted`. `arms` are poses for the right and left arm (names in ARMS or
    (elbow, wrist) tuples, given as for the right arm). Returns (parts, J),
    J holding world positions of joints (`wrist_r`, `wrist_l`, `head`...).

    outfit keys: tunic (material), armour (None|"mail"|"plate"|"leather"),
    sleeves ("short"|"long"), legs ("trousers"|"bare"), boots (bool),
    skirt (hem height or None), skin, beard (bool), hair (material or None),
    belt (bool).
    """
    o = {"tunic": p["team"], "armour": None, "sleeves": "short", "legs": "trousers", "boots": True, "skirt": 0.66, "skin": p["skin"], "beard": False, "hair": p["hair"], "belt": True}
    o.update(outfit or {})
    at = Vector(at)
    rot = _yaw(yaw)

    def w(v):
        """Local (figure) coordinates to world."""
        return at + rot @ Vector(v)

    J = {}
    J["hips"] = Vector((0, 0, 0.98))
    J["spine"] = Vector((0, 0, 1.18))
    J["chest"] = Vector((0, 0, 1.36))
    J["neck"] = Vector((0, 0, 1.52))
    J["head"] = Vector((0, -0.01, 1.68))
    for name, side in (("r", 1), ("l", -1)):
        pose = arms[0] if name == "r" else arms[1]
        elbow, wrist = ARMS[pose] if isinstance(pose, str) else pose
        J[f"shoulder_{name}"] = _mirror(SHOULDER, side)
        J[f"elbow_{name}"] = _mirror(elbow, side)
        J[f"wrist_{name}"] = _mirror(wrist, side)
        J[f"hand_{name}"] = J[f"wrist_{name}"] + (J[f"wrist_{name}"] - J[f"elbow_{name}"]).normalized() * 0.07
        step = stride * (1 if name == "l" else -1)
        J[f"hip_{name}"] = _mirror(HIP, side)
        J[f"knee_{name}"] = _mirror(KNEE, side) + Vector((0, -step * 0.5, 0))
        J[f"ankle_{name}"] = _mirror(ANKLE, side) + Vector((0, -step, 0))
        J[f"toe_{name}"] = _mirror(TOE, side) + Vector((0, -step, 0))
    if mounted is not None:
        # Astride: hips on the saddle, thighs spread forward, shins down the flanks.
        seat = rot.inverted() @ (Vector(mounted) - at)
        shift = seat - J["hips"]
        for key in list(J):
            if not key.startswith(("hip_", "knee_", "ankle_", "toe_")):
                J[key] = J[key] + shift
        for name, side in (("r", 1), ("l", -1)):
            J[f"hip_{name}"] = seat + _mirror((-0.11, 0.0, -0.04), side)
            J[f"knee_{name}"] = seat + _mirror((-0.30, -0.26, -0.16), side)
            J[f"ankle_{name}"] = seat + _mirror((-0.35, -0.14, -0.58), side)
            J[f"toe_{name}"] = seat + _mirror((-0.36, -0.28, -0.63), side)
    J = {key: w(v) for key, v in J.items()}

    # Bones.
    P = prefix
    rig.bone(P + "hips", parent, J["hips"], J["spine"], radius=0.1)
    rig.bone(P + "spine", P + "hips", J["spine"], J["chest"], radius=0.11)
    rig.bone(P + "chest", P + "spine", J["chest"], J["neck"], radius=0.12)
    rig.bone(P + "head", P + "chest", J["neck"], J["head"] + Vector((0, 0, 0.08)), radius=0.07)
    for n in ("r", "l"):
        rig.bone(P + f"arm_{n}_up", P + "chest", J[f"shoulder_{n}"], J[f"elbow_{n}"], radius=0.035)
        rig.bone(P + f"arm_{n}_lo", P + f"arm_{n}_up", J[f"elbow_{n}"], J[f"hand_{n}"], radius=0.03)
        rig.bone(P + f"leg_{n}_up", P + "hips", J[f"hip_{n}"], J[f"knee_{n}"], radius=0.05)
        rig.bone(P + f"leg_{n}_lo", P + f"leg_{n}_up", J[f"knee_{n}"], J[f"ankle_{n}"], radius=0.04)
        rig.bone(P + f"foot_{n}", P + f"leg_{n}_lo", J[f"ankle_{n}"], J[f"toe_{n}"], radius=0.03)

    # The body, one smooth mesh.
    sx = rot @ Vector((1, 0, 0))
    el = [
        ("ellipsoid", J["hips"], (0.16, 0.11, 0.12)),
        ("capsule", J["hips"], J["chest"], 0.125),
        ("ellipsoid", J["chest"] + Vector((0, 0, 0.0)), (0.18, 0.115, 0.15)),
        ("capsule", J["shoulder_r"] + sx * 0.02, J["shoulder_l"] - sx * 0.02, 0.07),
        ("capsule", J["chest"] + Vector((0, 0, 0.1)), J["neck"] + Vector((0, 0, 0.06)), 0.055),
        ("ellipsoid", J["head"], (0.088, 0.1, 0.112)),
        ("ball", J["head"] + rot @ Vector((0, -0.035, -0.07)), 0.068),
        ("ball", J["head"] + rot @ Vector((0, -0.098, 0.0)), 0.019),
    ]
    for n in ("r", "l"):
        el += [
            ("capsule", J[f"shoulder_{n}"], J[f"elbow_{n}"], 0.05),
            ("capsule", J[f"elbow_{n}"], J[f"wrist_{n}"], 0.04),
            ("ellipsoid", J[f"hand_{n}"] - (J[f"hand_{n}"] - J[f"wrist_{n}"]) * 0.3, (0.04, 0.04, 0.04)),
            ("capsule", J[f"hip_{n}"], J[f"knee_{n}"], 0.075),
            ("capsule", J[f"knee_{n}"], J[f"ankle_{n}"], 0.054),
            ("capsule", J[f"ankle_{n}"] + Vector((0, 0, 0.02)), J[f"toe_{n}"], 0.045),
        ]
    body = k.blobs(prefix + "body", el, o["tunic"], res=0.011, faces=2200)

    torso = {None: o["tunic"], "mail": p["mail"], "plate": p["iron"], "leather": p["leather"], "lamellar": p["mail"]}[o["armour"]]
    legs = p["trousers"] if o["legs"] == "trousers" else o["skin"]
    boot = p["leather"] if o["boots"] else o["skin"]
    segs = [
        ("head", J["neck"] + Vector((0, 0, 0.1)), J["head"] + Vector((0, 0, 0.05)), o["skin"]),
        ("torso", J["hips"] + Vector((0, 0, 0.04)), J["chest"] + Vector((0, 0, 0.06)), torso),
        ("pelvis", J["hips"] - Vector((0, 0, 0.06)), J["hips"], o["tunic"] if o["skirt"] else legs),
    ]
    for n in ("r", "l"):
        segs += [
            ("upper_arm", J[f"shoulder_{n}"], J[f"elbow_{n}"], torso if o["armour"] in ("mail", "plate") else o["tunic"]),
            ("fore_arm", J[f"elbow_{n}"] + (J[f"wrist_{n}"] - J[f"elbow_{n}"]) * 0.25, J[f"wrist_{n}"], o["tunic"] if o["sleeves"] == "long" else o["skin"]),
            ("hand", J[f"wrist_{n}"], J[f"hand_{n}"], p["leather"] if o["armour"] == "plate" else o["skin"]),
            ("thigh", J[f"hip_{n}"], J[f"knee_{n}"], legs),
            ("shin", J[f"knee_{n}"] + (J[f"ankle_{n}"] - J[f"knee_{n}"]) * 0.2, J[f"ankle_{n}"], boot if o["boots"] else legs),
            ("foot", J[f"ankle_{n}"], J[f"toe_{n}"], boot),
        ]
    mats = []
    for s in segs:
        if s[3] not in mats:
            mats.append(s[3])

    def pick(c, nrm):
        best = min(segs, key=lambda s: seg_dist(c, s[1], s[2]))
        return mats.index(best[3])

    k.paint(body, mats, pick)
    groups = [P + b for b in ("hips", "spine", "chest", "head", "arm_r_up", "arm_r_lo", "arm_l_up", "arm_l_lo", "leg_r_up", "leg_r_lo", "leg_l_up", "leg_l_lo", "foot_r", "foot_l")]
    k.blend(body, groups)
    parts = [body]

    # Hair or beard, skirt, belt.
    up = Vector((0, 0, 1))
    fwd = rot @ Vector((0, -1, 0))
    if o["hair"]:
        hair = k.ball(0.1, o["hair"], loc=J["head"] + up * 0.02 - fwd * 0.012, scale=(0.95, 1.0, 0.92))
        k.bind(hair, P + "head")
        parts.append(hair)
    if o["beard"]:
        beard = k.ball(0.06, o["hair"] or p["hair"], loc=J["head"] + fwd * 0.06 - up * 0.075, scale=(1.05, 0.7, 0.95))
        k.bind(beard, P + "head")
        parts.append(beard)
    if o["skirt"] and mounted is None:
        top, hem = J["hips"].z + 0.06, at.z + o["skirt"]
        skirt = k.cyl(0.25, top - hem, o["tunic"], loc=(J["hips"].x, J["hips"].y, (top + hem) / 2), n=18, r2=0.165)
        k.blend(skirt, [P + "hips", P + "leg_r_up", P + "leg_l_up"])
        parts.append(skirt)
    elif o["skirt"]:
        skirt = k.cyl(0.19, 0.2, o["tunic"], loc=J["hips"] - up * 0.02, n=16, r2=0.155)
        k.bind(skirt, P + "hips")
        parts.append(skirt)
    if o["belt"]:
        belt = k.cyl(0.152, 0.05, p["leather"], loc=J["hips"] + up * 0.1, n=16)
        k.bind(belt, P + "spine")
        parts.append(belt)
    if o["armour"] == "plate":
        for n in ("r", "l"):
            pad = k.ball(0.085, p["iron"], loc=J[f"shoulder_{n}"] + up * 0.02, scale=(1.1, 1.0, 0.75))
            k.bind(pad, P + f"arm_{n}_up")
            parts.append(pad)
    J["fwd"], J["up"], J["side"] = fwd, up, sx
    J["prefix"] = P
    return parts, J


def _yaw(a):
    from mathutils import Matrix

    return Matrix.Rotation(a, 3, "Z")


# ---------------------------------------------------------------- helmets


def helmet(k, p, J, kind, bone=None, color=None):
    """A helmet or hat on the figure's head."""
    h = J["head"]
    up, fwd = J["up"], J["fwd"]
    bone = bone or J["prefix"] + "head"
    out = []
    metal = color or p["iron"]
    if kind == "cone":
        out.append(k.cyl(0.115, 0.17, metal, loc=h + up * 0.1, n=18, r2=0.012))
        out.append(k.cyl(0.118, 0.03, p["bronze"], loc=h + up * 0.03, n=18))
        out.append(k.box((0.018, 0.012, 0.08), metal, loc=h + fwd * 0.105 - up * 0.01))
    elif kind == "crest":  # bronze helmet with a horsehair crest in the owner's colour
        out.append(k.ball(0.118, p["bronze"], loc=h + up * 0.035, scale=(1.0, 1.05, 0.95)))
        for s in (-1, 1):
            out.append(k.box((0.02, 0.07, 0.1), p["bronze"], loc=h + J["side"] * s * 0.095 + fwd * 0.03 - up * 0.04))
        crest = k.blobs("crest", [("ellipsoid", h + up * 0.17 - fwd * 0.02, (0.025, 0.14, 0.06))], p["team"], res=0.012, faces=300)
        out.append(crest)
    elif kind == "kettle":  # brimmed iron hat
        out.append(k.ball(0.115, metal, loc=h + up * 0.045, scale=(1.0, 1.0, 0.85)))
        out.append(k.cyl(0.19, 0.016, metal, loc=h + up * 0.03, n=20, r2=0.17))
    elif kind == "morion":  # crested, upswept brim
        out.append(k.ball(0.115, metal, loc=h + up * 0.05, scale=(0.95, 1.05, 0.95)))
        out.append(k.box((0.012, 0.2, 0.08), metal, loc=h + up * 0.15))
        brim = k.loft("brim", [[(math.cos(a) * r * 0.8, math.sin(a) * r, 0.03 * math.cos(a) ** 2 * 3 + (0.0 if r < 0.15 else 0.025)) for a in [i / 24 * math.tau for i in range(24)]] for r in (0.11, 0.2)], metal, closed=True, cap=False)
        mod = brim.modifiers.new("s", "SOLIDIFY")
        mod.thickness = 0.01
        brim.location = h + up * 0.02
        brim.rotation_euler = (0, 0, math.atan2(fwd.x, -fwd.y))
        out.append(brim)
    elif kind == "great":  # closed great helm
        out.append(k.cyl(0.115, 0.24, metal, loc=h + up * 0.03, n=18, r2=0.105))
        out.append(k.ball(0.105, metal, loc=h + up * 0.15, scale=(1.0, 1.0, 0.4)))
        out.append(k.box((0.17, 0.015, 0.016), p["black"], loc=h + fwd * 0.112 + up * 0.03))
        out.append(k.box((0.012, 0.012, 0.11), p["black"], loc=h + fwd * 0.114 - up * 0.02))
    elif kind == "cap":  # leather cap
        out.append(k.ball(0.108, p["leather"], loc=h + up * 0.04, scale=(1.0, 1.0, 0.8)))
    elif kind == "hood":
        out.append(k.blobs("hood", [("ellipsoid", h + up * 0.02 - fwd * 0.02, (0.115, 0.12, 0.12)), ("ellipsoid", h - fwd * 0.07 - up * 0.08, (0.13, 0.08, 0.1))], color or p["linen_dark"], res=0.012, faces=500))
        out.append(k.box((0.14, 0.02, 0.17), p["black"], loc=h + fwd * 0.07 - up * 0.01))
    elif kind == "hat":  # wide-brimmed felt hat
        out.append(k.cyl(0.1, 0.11, color or p["felt"], loc=h + up * 0.1, n=16, r2=0.085))
        out.append(k.cyl(0.21, 0.014, color or p["felt"], loc=h + up * 0.055, n=20))
    elif kind == "straw":
        out.append(k.cyl(0.24, 0.09, p["straw"], loc=h + up * 0.08, n=18, r2=0.05))
    elif kind == "tricorne":
        out.append(k.cyl(0.1, 0.09, p["felt"], loc=h + up * 0.09, n=14, r2=0.09))
        tri = k.cyl(0.2, 0.045, p["felt"], loc=h + up * 0.09, n=3, smooth=None)
        tri.rotation_euler = (0, 0, math.atan2(fwd.x, -fwd.y) + math.pi / 2)
        out.append(tri)
    elif kind == "headband":
        out.append(k.cyl(0.098, 0.03, color or p["team"], loc=h + up * 0.04, n=16))
        out.append(k.box((0.015, 0.04, 0.16), p["feather"], loc=h - fwd * 0.07 + up * 0.14, rot=(0.3, 0, 0)))
    k.bind(out, bone)
    return out


# ---------------------------------------------------------------- shields


def shield(k, p, centre, facing, kind="round", size=1.0, bone=None, emblem=True):
    """A shield at `centre`, its face turned towards `facing` (a direction)."""
    facing = Vector(facing).normalized()
    q = facing.to_track_quat("Z", "Y")
    out = []
    if kind == "round":
        face = k.cyl(0.3 * size, 0.035, p["team_paint"], loc=centre, n=28)
        rim = k.cyl(0.312 * size, 0.024, p["bronze"], loc=centre - facing * 0.008, n=28)
        boss = k.ball(0.075 * size, p["bronze"], loc=centre + facing * 0.02, scale=(1, 1, 0.55))
        out += [face, rim, boss]
    elif kind == "scutum":  # tall curved rectangle
        rings = []
        for j in range(6):
            z = (j / 5 - 0.5) * 0.95 * size
            rings.append([(math.sin(a) * 0.36 * size, -math.cos(a) * 0.12 * size + 0.12 * size, z) for a in [(-0.85 + 1.7 * i / 10) for i in range(11)]])
        face = k.loft("scutum", rings, p["team_paint"], closed=False, cap=False)
        mod = face.modifiers.new("s", "SOLIDIFY")
        mod.thickness = 0.03
        boss = k.ball(0.07 * size, p["bronze"], loc=(0, 0.015, 0), scale=(1, 0.5, 1))
        stripe = k.box((0.05 * size, 0.02, 0.9 * size), p["gold_paint"], loc=(0, 0.005, 0))
        out += [face, boss, stripe]
        for o in out:
            o.rotation_mode = "QUATERNION"
        _orient(out, centre, facing, axis="-Y")
    elif kind == "kite":
        bm_rings = []
        for j in range(9):
            t = j / 8
            z = (0.5 - t) * 1.0 * size
            wdt = 0.27 * size * (math.sin(min(1.0, (1 - t) * 1.4) * math.pi / 2) if t > 0.25 else 1.0 - (0.25 - t) * 1.0)
            bm_rings.append([(math.sin(a) * wdt, -math.cos(a) * 0.08 * size + 0.08 * size, z) for a in [(-1.0 + 2.0 * i / 10) for i in range(11)]])
        face = k.loft("kite", bm_rings, p["team_paint"], closed=False, cap=False)
        mod = face.modifiers.new("s", "SOLIDIFY")
        mod.thickness = 0.03
        cross_v = k.box((0.05 * size, 0.02, 0.8 * size), p["gold_paint"], loc=(0, 0.004, 0.05 * size))
        cross_h = k.box((0.4 * size, 0.02, 0.05 * size), p["gold_paint"], loc=(0, 0.004, 0.25 * size))
        out += [face, cross_v, cross_h]
        _orient(out, centre, facing, axis="-Y")
    if kind == "round":
        for o in out:
            o.rotation_mode = "QUATERNION"
            o.rotation_quaternion = q
    if bone:
        k.bind(out, bone)
    return out


def _orient(objs, centre, facing, axis="-Y"):
    """Turns objects built facing `axis` (about the origin) to face `facing`
    and moves them to `centre`."""
    import bpy
    from mathutils import Matrix

    bpy.context.view_layer.update()
    q = Vector(facing).to_track_quat(axis, "Z")
    m = Matrix.Translation(centre) @ q.to_matrix().to_4x4()
    for o in objs:
        o.matrix_world = m @ o.matrix_world


# ---------------------------------------------------------------- weapons


def spear(k, p, a, b, r=0.016, head="leaf", bone=None):
    a, b = Vector(a), Vector(b)
    d = (b - a).normalized()
    out = [k.rod(a, b, r, p["wood"])]
    if head == "leaf":
        tip = k.ball(0.035, p["iron"], loc=b + d * 0.09, scale=(0.45, 0.2, 2.8))
    else:  # pike: long narrow point
        tip = k.ball(0.022, p["iron"], loc=b + d * 0.1, scale=(0.5, 0.5, 5.0))
    tip.rotation_mode = "QUATERNION"
    tip.rotation_quaternion = d.to_track_quat("Z", "Y")
    out.append(tip)
    if bone:
        k.bind(out, bone)
    return out


def blade(k, p, grip, direction, length=0.75, width=0.055, bone=None, guard=True, metal=None):
    """A sword: grip at `grip`, blade along `direction`."""
    grip, d = Vector(grip), Vector(direction).normalized()
    out = [k.rod(grip - d * 0.1, grip + d * 0.04, 0.018, p["leather"], n=6)]
    out.append(k.ball(0.028, p["bronze"], loc=grip - d * 0.12))
    out.append(_blade_mesh(k, metal or p["steel"], grip + d * 0.06, d, length, width))
    if guard:
        g = k.box((0.2, 0.025, 0.025), p["bronze"], loc=grip + d * 0.05)
        g.rotation_mode = "QUATERNION"
        g.rotation_quaternion = d.to_track_quat("Z", "Y")
        out.append(g)
    if bone:
        k.bind(out, bone)
    return out


def _blade_mesh(k, material, base, d, length, width, point=0.32):
    """A sword blade from `base` along `d`: a flat diamond section with a
    fuller-like ridge, tapering to a point over its last `point` part."""
    import bmesh
    import bpy

    bm = bmesh.new()
    thick = width * 0.22
    rings = []
    steps = [0.0, 0.4, 1.0 - point, 1.0 - point * 0.55, 1.0 - point * 0.2]
    for t in steps:
        taper = 1.0 if t <= 1.0 - point else (1.0 - t) / point * 1.05
        hw = width / 2 * taper * (1.0 - 0.12 * t)
        th = thick * (0.4 + 0.6 * taper)
        rings.append([bm.verts.new((x, y, t * length)) for x, y in ((-hw, 0), (0, -th), (hw, 0), (0, th))])
    tip = bm.verts.new((0, 0, length))
    for a, b in zip(rings, rings[1:]):
        for i in range(4):
            j = (i + 1) % 4
            bm.faces.new((a[i], a[j], b[j], b[i]))
    for i in range(4):
        bm.faces.new((rings[-1][i], rings[-1][(i + 1) % 4], tip))
    bm.faces.new(list(reversed(rings[0])))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    me = bpy.data.meshes.new("blade")
    bm.to_mesh(me)
    bm.free()
    o = bpy.data.objects.new("blade", me)
    bpy.context.collection.objects.link(o)
    o.data.materials.append(material)
    o.data.shade_flat()
    o.rotation_mode = "QUATERNION"
    o.rotation_quaternion = Vector(d).to_track_quat("Z", "Y")
    o.location = base
    return o


def axe(k, p, grip, direction, length=0.7, bone=None):
    grip, d = Vector(grip), Vector(direction).normalized()
    top = grip + d * (length - 0.15)
    out = [k.rod(grip - d * 0.15, top + d * 0.08, 0.022, p["wood"])]
    side = d.cross(Vector((0, 0, 1))).normalized() if abs(d.z) < 0.95 else Vector((1, 0, 0))
    head = k.box((0.03, 0.13, 0.15), p["iron"], loc=top + side * 0.0)
    head.rotation_mode = "QUATERNION"
    head.rotation_quaternion = d.to_track_quat("Z", "Y")
    out.append(head)
    if bone:
        k.bind(out, bone)
    return out


def club(k, p, grip, direction, length=0.7, bone=None):
    grip, d = Vector(grip), Vector(direction).normalized()
    out = [k.rod(grip - d * 0.1, grip + d * length, 0.025, p["dark_wood"], r2=0.05)]
    for i in range(3):
        spike = k.ball(0.02, p["bone"], loc=grip + d * (length - 0.1 * i) + Vector((0.04, 0, 0)).cross(d) * (1 if i % 2 else -1))
        out.append(spike)
    if bone:
        k.bind(out, bone)
    return out


def bow(k, p, grip, up, facing, height=1.3, recurve=False, bone=None):
    """A bow held at `grip`, its limbs along `up`, curving away from the
    archer towards `facing`."""
    grip, up, fwd = Vector(grip), Vector(up).normalized(), Vector(facing).normalized()
    pts = []
    for i in range(9):
        t = i / 8 * 2 - 1
        bend = 0.12 * (1 - t * t)
        if recurve:
            bend += -0.06 * max(0.0, abs(t) - 0.75) / 0.25
        pts.append(grip + up * t * height / 2 + fwd * bend)
    out = []
    for a, b in zip(pts, pts[1:]):
        out.append(k.rod(a, b, 0.016, p["dark_wood"] if not recurve else p["horn"], n=6))
    out.append(k.rod(pts[0], pts[-1], 0.004, p["linen"], n=4))
    out.append(k.cyl(0.022, 0.12, p["leather"], loc=grip, n=8, rot=up.to_track_quat("Z", "Y").to_euler()))
    if bone:
        k.bind(out, bone)
    return out


def quiver(k, p, at, bone):
    at = Vector(at)
    out = [k.cyl(0.055, 0.5, p["leather"], loc=at, rot=(0.35, 0.25, 0), n=10)]
    for i in range(4):
        out.append(k.cyl(0.012, 0.08, p["feather"], loc=at + Vector((0.02 * (i % 2) - 0.01, 0.08 + 0.02 * i, 0.27)), n=4))
    k.bind(out, bone)
    return out


def crossbow(k, p, grip, facing, bone=None):
    grip, d = Vector(grip), Vector(facing).normalized()
    side = d.cross(Vector((0, 0, 1))).normalized()
    out = [k.rod(grip - d * 0.25, grip + d * 0.42, 0.028, p["wood"], n=6)]
    tip = grip + d * 0.4
    prod = [tip + side * s * 0.33 - d * 0.08 * abs(s) ** 2 for s in (-1, -0.5, 0, 0.5, 1)]
    for a, b in zip(prod, prod[1:]):
        out.append(k.rod(a, b, 0.016, p["steel"], n=6))
    out.append(k.rod(prod[0], grip + d * 0.05, 0.004, p["linen"], n=4))
    out.append(k.rod(prod[-1], grip + d * 0.05, 0.004, p["linen"], n=4))
    if bone:
        k.bind(out, bone)
    return out


def musket(k, p, butt, muzzle, bone=None):
    butt, muzzle = Vector(butt), Vector(muzzle)
    d = (muzzle - butt).normalized()
    out = [k.rod(butt, butt + d * 0.45, 0.04, p["wood"], n=8, r2=0.026)]
    out.append(k.rod(butt + d * 0.3, muzzle, 0.016, p["iron"], n=8))
    out.append(k.box((0.03, 0.06, 0.03), p["iron"], loc=butt + d * 0.42))
    if bone:
        k.bind(out, bone)
    return out


def tool(k, p, a, b, kind="pick", bone=None):
    """A worker's tool along a-b with its head at b."""
    a, b = Vector(a), Vector(b)
    d = (b - a).normalized()
    out = [k.rod(a, b, 0.02, p["wood"])]
    if kind == "pick":
        side = d.cross(Vector((0, 0, 1))).normalized() if abs(d.z) < 0.95 else Vector((1, 0, 0))
        out.append(k.rod(b - side * 0.22 + d * 0.03, b + side * 0.22 + d * 0.03, 0.022, p["iron"], n=6, r2=0.008))
    elif kind == "staff":
        out.append(k.ball(0.035, p["wood"], loc=b))
    if bone:
        k.bind(out, bone)
    return out


def banner(k, p, base, height=2.6, bone=None):
    """A standard on a pole: base at `base`, cloth in the owner's colour."""
    base = Vector(base)
    top = base + Vector((0, 0, height))
    out = [k.rod(base, top, 0.02, p["wood"])]
    out.append(k.rod(top - Vector((0.32, 0, 0.05)), top + Vector((0.02, 0, -0.05)), 0.012, p["wood"]))
    flag = k.sheet("flag", 0.62, 0.62, p["team"], nx=6, ny=6, bulge=0.04, thickness=0.01, sag=0.06)
    flag.location = top + Vector((-0.15, 0, -0.38))
    out.append(flag)
    out.append(k.ball(0.04, p["bronze"], loc=top + Vector((0, 0, 0.04))))
    if bone:
        k.bind(out, bone)
    return out


def extend_palette(k, p):
    """Materials people need beyond the base palette."""
    p.update(
        {
            "mail": k.mat("mail", k.srgb(120, 122, 126), 0.45, metal=0.9, pattern="weave", scale=60, amount=0.4),
            "steel": k.mat("steel", k.srgb(196, 200, 208), 0.25, metal=1.0, pattern="noise", scale=8, amount=0.15),
            "felt": k.mat("felt", k.srgb(48, 40, 34), 0.9, pattern="noise", scale=10, amount=0.2),
            "straw": k.mat("straw", k.srgb(214, 184, 112), 0.9, pattern="hair", scale=30, amount=0.3),
            "feather": k.mat("feather", k.srgb(236, 232, 222), 0.8),
            "horn": k.mat("horn", k.srgb(70, 50, 36), 0.5, pattern="grain", scale=20, amount=0.3),
            "bone": k.mat("bone", k.srgb(226, 214, 186), 0.6),
            "linen_dark": k.mat("linen_dark", k.srgb(120, 100, 74), 0.9, pattern="weave", scale=40, amount=0.2),
            "gold_paint": k.mat("gold_paint", k.srgb(214, 172, 70), 0.4, metal=0.6),
            "white_paint": k.mat("white_paint", k.srgb(232, 228, 218), 0.7),
            "skin_dark": k.mat("skin_dark", k.srgb(150, 104, 72), 0.55, pattern="noise", scale=20, amount=0.1),
            "skin_light": k.mat("skin_light", k.srgb(232, 190, 160), 0.55, pattern="noise", scale=20, amount=0.1),
            "fur": k.mat("fur", k.srgb(120, 90, 60), 0.95, pattern="hair", scale=18, amount=0.4),
        }
    )
    return p
