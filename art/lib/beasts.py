"""Four-legged animals: one parametric body for horses, mules, oxen, cattle,
sheep, deer, elephants and foxes, rigged alike (body, neck, head, four legs
of two bones, tail) so the game animates them all the same way.

Metres, facing -Y; the animal's left side is +X.
"""

import math

from mathutils import Vector

SPECIES = {
    # lh: leg height (ground to belly), br/bw/bl: barrel half height, width
    # and length, neck (length, angle above horizontal, radius), head
    # (length, angle below horizontal, radius), legs (upper, lower radius).
    "horse": dict(lh=0.86, br=0.34, bw=0.28, bl=0.62, neck=(0.6, 60, 0.12), head=(0.56, 47, 0.095), legs=(0.07, 0.04), tail="hair", mane=True, ears=0.17),
    "mule": dict(lh=0.74, br=0.29, bw=0.24, bl=0.52, neck=(0.46, 45, 0.11), head=(0.46, 58, 0.08), legs=(0.058, 0.034), tail="tuft", mane=True, ears=0.2),
    "ox": dict(lh=0.74, br=0.42, bw=0.36, bl=0.75, neck=(0.34, 26, 0.22), head=(0.46, 52, 0.125), legs=(0.08, 0.05), tail="tuft", horns=0.32, ears=0.1),
    "cow": dict(lh=0.7, br=0.37, bw=0.31, bl=0.68, neck=(0.32, 26, 0.19), head=(0.44, 52, 0.11), legs=(0.07, 0.045), tail="tuft", horns=0.14, ears=0.1, udder=True),
    "sheep": dict(lh=0.42, br=0.3, bw=0.27, bl=0.4, neck=(0.2, 35, 0.12), head=(0.24, 55, 0.07), legs=(0.035, 0.026), tail="stub", wool=True, ears=0.08),
    "deer": dict(lh=0.82, br=0.2, bw=0.17, bl=0.45, neck=(0.45, 60, 0.085), head=(0.3, 60, 0.06), legs=(0.042, 0.024), tail="stub", antlers=True, ears=0.1),
    "elephant": dict(lh=1.25, br=0.68, bw=0.62, bl=0.95, neck=(0.25, 15, 0.42), head=(0.5, 70, 0.32), legs=(0.18, 0.16), tail="tuft", trunk=True, tusks=True, ears=0.45),
    "fox": dict(lh=0.24, br=0.11, bw=0.09, bl=0.3, neck=(0.14, 35, 0.06), head=(0.2, 15, 0.05), legs=(0.022, 0.016), tail="brush", ears=0.07),
}


def quadruped(k, p, kind, coat, *, rig=None, parent=None, prefix="", at=(0, 0, 0), yaw=0.0, points=None, faces=4500, extra=None):
    """An animal standing at `at` facing -Y turned by `yaw`. `coat` is its
    material, `points` the darker lower legs and muzzle. With a `rig`, adds
    bones (prefix + body, neck, head, fl_up, fl_lo, fr_..., hl_..., hr_...,
    tail) and binds the parts. Returns (parts, A) with points such as
    `seat` (on its back) and `muzzle`."""
    s = dict(SPECIES[kind])
    s.update(extra or {})
    lh, br, bw, bl = s["lh"], s["br"], s["bw"], s["bl"]
    cz = lh + br
    nl, na, nt = s["neck"]
    hl, ha, ht = s["head"]
    lu, ll = s["legs"]
    na, ha = math.radians(na), math.radians(ha)
    from mathutils import Matrix

    rot = Matrix.Rotation(yaw, 3, "Z")
    at = Vector(at)

    def w(v):
        return at + rot @ Vector(v)

    n0 = Vector((0, -bl * 0.95, cz + br * 0.45))
    n1 = n0 + Vector((0, -math.cos(na) * nl, math.sin(na) * nl))
    h1 = n1 + Vector((0, -math.cos(ha) * hl, -math.sin(ha) * hl))
    tail0 = Vector((0, bl * 1.3, cz + br * 0.35))
    el = [
        ("ellipsoid", (0, 0, cz), (bw, bl * 0.85, br)),
        ("ellipsoid", (0, -bl * 0.75, cz + 0.02 * lh), (bw * 0.88, bl * 0.4, br * 1.08), 1.0, (0.3, 0, 0)),
        ("ellipsoid", (0, bl * 0.85, cz + br * 0.12), (bw * 1.0, bl * 0.5, br * 0.95)),
        ("ellipsoid", (0, bl * 1.12, cz - br * 0.25), (bw * 0.78, bl * 0.25, br * 0.9)),
        ("ellipsoid", (0, -bl * 0.55, cz + br * 0.82), (bw * 0.4, bl * 0.35, br * 0.3)),
        ("ellipsoid", (0, 0, cz - br * 0.55), (bw * 0.8, bl * 0.7, br * 0.5)),
        ("ellipsoid", n0 + Vector((0, 0.05, -0.04)), (nt * 1.2, nt * 1.4, nt * 1.6), 1.0, (-0.5, 0, 0), 1.6),
        ("capsule", n0, n1, nt),
    ]
    if kind not in ("horse", "mule"):
        el.append(("capsule", n0 + Vector((0, 0.1, 0.05)), n1 + Vector((0, 0.06, 0.02)), nt * 0.8))
    else:
        # A deep neck, narrowing to the throat below the jaw.
        el.append(("capsule", n0 + Vector((0, 0.12, 0.08)), n1 + Vector((0, 0.1, 0.0)), nt * 0.7, 1.4))
    # The head: along its axis `ax`, `face` is the front of the face (the
    # forehead and nose), `side` the cheeks.
    ax = (h1 - n1).normalized()
    face = Vector((0, ax.z, -ax.y))
    tilt = (math.atan2(ax.z, ax.y), 0, 0)
    equine = kind in ("horse", "mule")
    if equine:
        # Metaballs swell where they overlap, so the head's parts are drawn
        # small and soft (low stiffness, the last number).
        el += [
            ("ellipsoid", n1 + ax * hl * 0.16 + face * ht * 0.1, (ht * 1.05, hl * 0.22, ht * 0.95), 1.0, tilt, 1.8),  # skull
            ("ellipsoid", n1 + ax * hl * 0.22 - face * ht * 1.15, (ht * 1.05, hl * 0.22, ht * 1.2), 1.0, tilt, 2.0),  # jowls
            ("capsule", n1 + ax * hl * 0.3 + face * ht * 0.1, h1 - ax * hl * 0.06, ht * 0.6, 1.6),  # nose
            ("capsule", n1 + ax * hl * 0.4 - face * ht * 0.45, h1 - ax * hl * 0.12 - face * ht * 0.3, ht * 0.55, 1.4),
            ("ellipsoid", h1, (ht * 0.72, ht * 0.75, ht * 0.8), 1.0, tilt, 1.5),  # muzzle
            ("ellipsoid", h1 - face * ht * 0.75 - ax * ht * 0.3, (ht * 0.6, ht * 0.65, ht * 0.4), 1.0, tilt, 1.3),  # lower lip
        ]
        for sx in (-1, 1):
            el.append(("ball", n1 + ax * hl * 0.28 + face * ht * 0.32 + Vector((sx * ht * 0.62, 0, 0)), ht * 0.26, 1.0))  # brow
    elif kind in ("ox", "cow"):
        # Cattle: a broad, flat forehead between the horns, a short face, a
        # wide square muzzle and a dewlap hanging under the throat.
        el += [
            ("ellipsoid", n1 + ax * hl * 0.12 + face * ht * 0.1, (ht * 1.15, hl * 0.2, ht * 0.85), 1.0, tilt, 1.8),  # forehead
            ("ellipsoid", n1 + ax * hl * 0.18 - face * ht * 0.6, (ht * 1.0, hl * 0.2, ht * 0.85), 1.0, tilt, 1.6),  # cheeks
            ("capsule", n1 + ax * hl * 0.3, h1 - ax * hl * 0.1, ht * 0.72, 1.6),  # face
            ("ellipsoid", h1, (ht * 0.95, ht * 0.7, ht * 0.75), 1.0, tilt, 1.7),  # muzzle
            ("ellipsoid", n0 + Vector((0, -0.12, -nt * 0.9)), (nt * 0.35, nt * 0.9, nt * 0.6), 1.0, (0.4, 0, 0), 1.3),  # dewlap
        ]
    else:
        el += [
            ("capsule", n1 + Vector((0, 0.02, 0.02)), h1, ht),
            ("ball", n1 + Vector((0, -0.02, -0.04)), ht * 1.25),
            ("ellipsoid", h1, (ht * 0.95, ht * 1.0, ht * 0.9)),
        ]
    if kind in ("horse", "mule", "deer"):
        # Shoulder blades, chest muscles and haunches, soft so they only shape the surface.
        for sx in (-1, 1):
            el.append(("ellipsoid", (sx * bw * 0.55, -bl * 0.62, cz + br * 0.3), (bw * 0.22, bl * 0.2, br * 0.6), 1.0, (-0.45, 0, 0), 0.9))
            el.append(("ball", (sx * bw * 0.32, -bl * 1.02, cz - br * 0.2), br * 0.32, 0.9))
            el.append(("ellipsoid", (sx * bw * 0.6, bl * 0.82, cz + br * 0.1), (bw * 0.24, bl * 0.35, br * 0.6), 1.0, (0, 0, 0), 0.9))
    if s.get("mane"):
        el.append(("capsule", n0 + Vector((0, 0.08, br * 0.5)), n1 + Vector((0, 0.02, nt * 0.6)), nt * 0.45))
    if s.get("udder"):
        el.append(("ellipsoid", (0, bl * 0.55, lh + 0.02), (bw * 0.35, bl * 0.18, br * 0.3)))
    legs = {}
    yf, yh = -bl * 0.72, bl * 0.86
    for side, sx in (("l", 1), ("r", -1)):
        x = bw * 0.55 * sx
        top, knee, fet, hoof = Vector((x, yf, cz - br * 0.4)), Vector((x, yf - 0.02, lh * 0.55)), Vector((x, yf, lh * 0.17)), Vector((x, yf - 0.03, ll * 0.9))
        legs["f" + side] = (top, knee, fet, hoof)
        el += [
            ("ellipsoid", (x * 0.95, yf, cz - br * 0.55), (lu * 1.25, lu * 1.4, br * 0.6)),
            ("capsule", top, knee, lu),
            ("ball", knee, lu * 0.9),
            ("capsule", knee, fet, ll),
            ("ball", fet, ll * 1.25),
            ("capsule", fet, hoof, ll),
        ]
        top, hock, fet, hoof = Vector((x, yh - bl * 0.05, cz - br * 0.15)), Vector((x, yh + lh * 0.26, lh * 0.57)), Vector((x, yh + lh * 0.18, lh * 0.17)), Vector((x, yh + lh * 0.16, ll * 0.9))
        legs["h" + side] = (top, hock, fet, hoof)
        el += [
            ("ellipsoid", (x * 0.92, yh, cz - br * 0.2), (lu * 1.6, bl * 0.4, br * 0.8)),
            ("capsule", top, hock, lu * 1.05),
            ("ball", hock, lu * 0.85),
            ("capsule", hock, fet, ll),
            ("ball", fet, ll * 1.25),
            ("capsule", fet, hoof, ll),
        ]
    res = max(0.005, min(0.016, lh * 0.014))
    if s.get("wool"):
        el = [(e[0], e[1], tuple(v * 1.0 for v in e[2]) if isinstance(e[2], tuple) else e[2], *e[3:]) for e in el]
    world = []
    for e in el:
        e = list(e)
        if e[0] == "capsule":
            e[1], e[2] = w(e[1]), w(e[2])
        else:
            e[1] = w(e[1])
            if e[0] == "ellipsoid" and len(e) > 4:
                eul = Vector(e[4])
                e[4] = (eul.x, eul.y, eul.z + yaw)
            elif e[0] == "ellipsoid":
                e += [1.0, (0, 0, yaw)]
        world.append(tuple(e))
    body = k.blobs(prefix + kind, world, coat, res=res, faces=faces)
    pts = points or coat
    hoofm = p.get("hoof") or k.mat("hoof", k.srgb(56, 48, 40), 0.55)
    low = lh * (0.3 if points else 0.0)
    hz = ll * 1.8

    def pick(c, nrm):
        local = rot.inverted() @ (Vector(c) - at)
        if local.z < hz and kind not in ("elephant",):
            return 2
        if local.z < low:
            return 1
        if points and (local - h1).length < ht * 0.75:
            return 1
        return 0

    k.paint(body, [coat, pts, hoofm], pick)
    from mathutils.bvhtree import BVHTree

    surface = BVHTree.FromPolygons([v.co.copy() for v in body.data.vertices], [list(f.vertices) for f in body.data.polygons])
    parts = [body]
    extras = []
    # Wool coat over sheep: a lumpy fleece around the body.
    if s.get("wool"):
        fleece = []
        for i in range(14):
            a = i / 14 * math.tau
            fleece.append(("ball", w((math.cos(a) * bw * 0.85, (i % 5 - 2) * bl * 0.38, cz + math.sin(a) * br * 0.75)), br * 0.5))
        fleece.append(("ellipsoid", w((0, 0, cz + 0.02)), (bw * 1.15, bl * 1.15, br * 1.08), 1.0, (0, 0, yaw)))
        fl = k.blobs(prefix + "fleece", fleece, p.get("wool") or k.mat("wool", k.srgb(236, 230, 214), 0.95, pattern="noise", scale=30, amount=0.25), res=0.02, faces=900)
        parts.append(fl)
        extras.append(("body", [fl]))
    # Mane strip and forelock.
    mane_mat = p.get("mane") or pts
    if s.get("mane"):
        el_m = []
        crest = [n1 + (n0 + Vector((0, 0.2, br * 0.5)) - n1) * (i / 6) + Vector((0.03, 0.02, nt * (0.95 - 0.1 * i / 6))) for i in range(7)]
        for a_, b_ in zip(crest, crest[1:]):
            el_m.append(("capsule", w(a_), w(b_), nt * 0.24, 1.6))
        el_m.append(("ellipsoid", w(n1 + face * ht * 0.45 + ax * hl * 0.12), (ht * 0.35, ht * 0.9, ht * 0.25), 1.0, (tilt[0], 0, yaw)))  # forelock
        m = k.blobs(prefix + "mane", el_m, mane_mat, res=0.01, faces=700)
        parts.append(m)
        extras.append(("neck", [m]))
    # Tail.
    t1 = tail0 + Vector((0, 0.12, -br * 0.9))
    if s["tail"] == "hair":
        tail = k.blobs(prefix + "tail", [("capsule", w(tail0), w(tail0 + Vector((0, 0.12, -0.15))), 0.04), ("ellipsoid", w(tail0 + Vector((0, 0.17, -0.45))), (0.045, 0.06, 0.3)), ("capsule", w(tail0 + Vector((0, 0.15, -0.3))), w(tail0 + Vector((0, 0.2, -0.75))), 0.035)], mane_mat, res=0.01, faces=500)
        t1 = tail0 + Vector((0, 0.2, -0.75))
    elif s["tail"] == "brush":
        tail = k.blobs(prefix + "tail", [("capsule", w(tail0), w(tail0 + Vector((0, 0.3, -0.12))), 0.05), ("ellipsoid", w(tail0 + Vector((0, 0.28, -0.12))), (0.06, 0.16, 0.06))], coat, res=0.008, faces=300)
        t1 = tail0 + Vector((0, 0.38, -0.14))
    elif s["tail"] == "tuft":
        tail = k.rod(w(tail0), w(t1), max(0.012, br * 0.05), coat, n=6)
        tail2 = k.ball(max(0.03, br * 0.12), pts, loc=w(t1), scale=(0.8, 0.8, 1.6))
        parts.append(tail2)
        extras.append(("tail", [tail2]))
    else:
        tail = k.ball(br * 0.25, coat, loc=w(tail0), scale=(0.8, 1.0, 1.2))
        t1 = tail0 + Vector((0, 0.1, -0.1))
    parts.append(tail)
    extras.append(("tail", [tail]))
    # Head parts: ears, eyes, horns, antlers, tusks, trunk.
    head_parts = []
    side = Vector((1, 0, 0))
    eye_m = p.get("eye") or k.mat("eye", k.srgb(22, 16, 12), 0.12)
    for sx in (-1, 1):
        if equine:
            eye = n1 + ax * hl * 0.28 + face * ht * 0.2
            r = ht * 0.27
        else:
            eye = n1 + (h1 - n1) * 0.22 + face * ht * 0.45
            r = max(0.012, ht * 0.22)
        # Sit the eye on the head's actual surface: cast a ray in from the side.
        target = w(eye)
        out_dir = rot @ (side * sx)
        hit, *_ = surface.ray_cast(target + out_dir * 2.0, -out_dir)
        centre = (hit - out_dir * r * 0.35) if hit is not None else w(eye + side * sx * ht)
        e_ = k.ball(r, eye_m, loc=centre, scale=(0.6, 1.0, 0.85), subdiv=2)
        e_.rotation_euler = (0, 0, yaw)
        head_parts.append(e_)
        if equine:
            # Nostrils: dark, flared slits on the front of the muzzle.
            head_parts.append(k.ball(ht * 0.17, eye_m, loc=w(h1 + ax * ht * 0.3 + face * ht * 0.45 + side * sx * ht * 0.42), scale=(0.55, 0.7, 1.0), subdiv=2))
        ear = s.get("ears", 0.1)
        if equine:
            base = n1 + face * ht * 0.2 + side * sx * ht * 0.5 + ax * hl * 0.02
            tip = base + Vector((sx * ear * 0.15, -ear * 0.25, ear))
            e = k.blobs(prefix + f"ear{sx}", [("ellipsoid", w(base + (tip - base) * 0.45), (ear * 0.2, ear * 0.13, ear * 0.5), 1.0, (-0.25, sx * 0.15, yaw))], coat, res=0.007, faces=120)
        elif kind == "elephant":
            e = k.blobs(prefix + f"ear{sx}", [("ellipsoid", w(n1 + Vector((sx * ht * 1.0, 0.12, -0.1))), (0.06, ht * 0.9, ear), 1.0, (0, 0, yaw))], coat, res=0.025, faces=200)
        else:
            e = k.cyl(ear * 0.3, ear, coat, loc=w(n1 + Vector((sx * ht * 0.6, 0.03, ht * 0.75 + ear * 0.35))), rot=(0.4, sx * 0.5, yaw), n=8, r2=ear * 0.05)
        head_parts.append(e)
        if s.get("horns"):
            hr = s["horns"]
            base = n1 + Vector((sx * ht * 0.7, 0.0, ht * 0.75))
            mid = base + Vector((sx * hr * 0.7, -hr * 0.05, hr * 0.1))
            horn = p.get("horn_light") or p["bone"]
            head_parts.append(k.rod(w(base), w(mid), 0.045, horn, n=8, r2=0.03))
            head_parts.append(k.rod(w(mid), w(mid + Vector((sx * hr * 0.2, -hr * 0.15, hr * 0.55))), 0.03, horn, n=8, r2=0.006))
        if s.get("antlers"):
            base = n1 + Vector((sx * ht * 0.5, 0.0, ht * 0.8))
            tip = base + Vector((sx * 0.15, 0.05, 0.4))
            head_parts.append(k.rod(w(base), w(tip), 0.014, p["bone"], n=5, r2=0.006))
            for f in (0.35, 0.65, 0.9):
                q = base + (tip - base) * f
                head_parts.append(k.rod(w(q), w(q + Vector((sx * 0.04, -0.12, 0.1))), 0.009, p["bone"], n=4, r2=0.004))
        if s.get("tusks"):
            base = h1 + Vector((sx * ht * 0.45, -0.05, 0.05))
            head_parts.append(k.rod(w(base), w(base + Vector((sx * 0.08, -0.45, 0.2))), 0.05, p["bone"], n=8, r2=0.015))
    if s.get("trunk"):
        tr = [h1, h1 + Vector((0, -0.15, -0.35)), h1 + Vector((0, -0.12, -0.75)), h1 + Vector((0, -0.2, -1.0))]
        head_parts.append(k.blobs(prefix + "trunk", [("capsule", w(a), w(b), ht * (0.42 - 0.08 * i)) for i, (a, b) in enumerate(zip(tr, tr[1:]))], coat, res=0.025, faces=400))
    parts += head_parts
    extras.append(("head", head_parts))

    A = {
        "seat": w((0, -bl * 0.15, cz + br + 0.02)),
        "back": cz + br,
        "muzzle": w(h1),
        "chest": w((0, -bl * 1.05, cz)),
        "rump": w((0, bl * 1.2, cz)),
        "girth": (bw, br, cz, bl),
        "w": w,
        "rot": rot,
    }
    if rig is not None:
        P = prefix
        rig.bone(P + "body", parent, w((0, -bl * 0.3, cz)), w((0, bl * 0.9, cz)), radius=min(bw, br) * 0.75)
        rig.bone(P + "neck", P + "body", w(n0), w(n1), radius=nt * 0.6)
        rig.bone(P + "head", P + "neck", w(n1), w(h1), radius=ht * 0.6)
        for leg, (top, mid, fet, hoof) in legs.items():
            rig.bone(P + leg + "_up", P + "body", w(top), w(mid), radius=lu * 0.6)
            rig.bone(P + leg + "_lo", P + leg + "_up", w(mid), w(hoof), radius=ll * 0.6)
        rig.bone(P + "tail", P + "body", w(tail0), w(t1), radius=0.02)
        k.blend(body, [P + b for b in ("body", "neck", "head", "fl_up", "fl_lo", "fr_up", "fr_lo", "hl_up", "hl_lo", "hr_up", "hr_lo")])
        for bone, objs in extras:
            k.bind(objs, P + bone)
    return parts, A


def drape(k, name, A, y0, y1, deg, material, thick, lift=0.0, grow=1.04):
    """Cloth laid over an animal's back (saddle cloths, barding, packs),
    from y0 to y1 along its body (fractions of its half length)."""
    bw, br, cz, bl = A["girth"]
    rings = []
    for j in range(7):
        y = (y0 + (y1 - y0) * j / 6) * bl
        ring = []
        for i in range(13):
            a = math.radians(-deg + 2 * deg * i / 12)
            ring.append(A["w"]((bw * grow * math.sin(a), y, cz + lift + br * grow * math.cos(a))))
        rings.append(ring)
    o = k.loft(name, rings, material, closed=False, cap=False)
    mod = o.modifiers.new("solid", "SOLIDIFY")
    mod.thickness = thick
    return o


def tack(k, p, A, cloth=None, prefix="", saddle=True):
    """Bridle, reins, saddle cloth (in `cloth`, the owner's colour by
    default) and saddle, bound to the animal's bones."""
    P = prefix
    w = A["w"]
    bw, br, cz, bl = A["girth"]
    out = []
    c = drape(k, "cloth", A, -0.72, 0.32, 72, cloth or p["team"], 0.018, grow=1.06)
    out.append(c)
    if saddle:
        out.append(drape(k, "saddle", A, -0.5, 0.08, 42, p["leather"], 0.04, grow=1.1))
        out.append(k.box((0.16, 0.06, 0.13), p["leather"], loc=w((0, -bl * 0.55, cz + br + 0.08)), rot=(0.3, 0, 0), bevel=0.02))
        out.append(k.box((0.24, 0.05, 0.12), p["leather"], loc=w((0, bl * 0.12, cz + br + 0.07)), rot=(-0.3, 0, 0), bevel=0.02))
    k.bind(out, P + "body")
    m = A["muzzle"]
    bridle = [k.rod(m + Vector((0.08, 0.1, 0.03)), m + Vector((-0.08, 0.1, 0.03)), 0.012, p["leather"])]
    k.bind(bridle, P + "head")
    return out + bridle
