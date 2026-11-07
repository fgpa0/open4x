"""Shared building blocks for the game's models, run inside Blender.

Models are built in metres, facing -Y (glTF's +Z, the game's forward) with
the ground at z = 0, then `finish` joins them, bakes their materials and
exports a .glb scaled to game units:

- `kind="unit"`: one 512² texture (colour with baked ambient occlusion) plus
  a metallic-roughness texture; the vertex colour's red channel is the team
  mask (1 where the owner's colour goes, painted light in the texture).
- `kind="prop"`: no textures; colour and occlusion are baked into the
  vertex colours, for the scenery material that merges props into one mesh.
"""

import math
import os

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
MODELS = os.path.join(ROOT, "crates", "client", "assets", "models")
PREVIEWS = os.path.join(ROOT, "target", "art")

# ---------------------------------------------------------------- scene


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    prefs = bpy.context.preferences.addons["cycles"].preferences
    for kind in ("OPTIX", "CUDA"):
        try:
            prefs.compute_device_type = kind
            prefs.get_devices()
            for d in prefs.devices:
                d.use = d.type != "CPU"
            scene.cycles.device = "GPU"
            break
        except Exception:
            continue
    scene.cycles.samples = 64
    scene.cycles.use_denoising = True


def srgb(r, g, b):
    """A colour from 0-255 sRGB to linear, as Blender's colour inputs want."""

    def lin(c):
        c /= 255.0
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4

    return (lin(r), lin(g), lin(b), 1.0)


# ---------------------------------------------------------------- materials

_mats = {}


def mat(name, color, rough=0.7, metal=0.0, team=False, pattern=None, scale=8.0, amount=0.25, color2=None):
    """A material with some procedural variation.

    `pattern`: None, "noise" (mottled), "grain" (wood grain along the
    object's local Z), "weave" (cloth), "streaks" (weathered planks along X),
    "hair". `color2` is the second colour the pattern blends to (darker by
    default) and `amount` how far. Team materials are painted light so the
    owner's colour multiplies onto them.
    """
    if name in _mats:
        return _mats[name]
    m = bpy.data.materials.new(name)
    try:
        m.use_nodes = True
    except Exception:
        pass
    nt = m.node_tree
    nt.nodes.clear()
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    bsdf = nt.nodes.new("ShaderNodeBsdfPrincipled")
    nt.links.new(bsdf.outputs[0], out.inputs["Surface"])
    c1 = color
    c2 = color2 or tuple(c * 0.55 for c in color[:3]) + (1.0,)
    col = None
    if pattern:
        coord = nt.nodes.new("ShaderNodeTexCoord")
        ramp = nt.nodes.new("ShaderNodeValToRGB")
        ramp.color_ramp.elements[0].color = c1
        ramp.color_ramp.elements[1].color = tuple(a + (b - a) * amount for a, b in zip(c1, c2))
        if pattern == "noise":
            tex = nt.nodes.new("ShaderNodeTexNoise")
            tex.inputs["Scale"].default_value = scale
            tex.inputs["Detail"].default_value = 6.0
            ramp.color_ramp.elements[0].position = 0.3
            ramp.color_ramp.elements[1].position = 0.7
            fac = tex.outputs["Fac"]
        elif pattern in ("grain", "streaks", "hair"):
            tex = nt.nodes.new("ShaderNodeTexWave")
            tex.wave_type = "BANDS"
            tex.bands_direction = "Z" if pattern != "streaks" else "X"
            tex.inputs["Scale"].default_value = scale
            tex.inputs["Distortion"].default_value = 6.0 if pattern != "hair" else 2.0
            tex.inputs["Detail"].default_value = 3.0
            fac = tex.outputs["Fac"]
        else:  # weave
            tex = nt.nodes.new("ShaderNodeTexBrick")
            tex.inputs["Scale"].default_value = scale
            tex.inputs["Mortar Size"].default_value = 0.0
            tex.inputs["Color1"].default_value = (1, 1, 1, 1)
            tex.inputs["Color2"].default_value = (0.6, 0.6, 0.6, 1)
            fac = tex.outputs["Fac"]
            noise = nt.nodes.new("ShaderNodeTexNoise")
            noise.inputs["Scale"].default_value = scale * 0.3
            nt.links.new(coord.outputs["Object"], noise.inputs["Vector"])
            mix = nt.nodes.new("ShaderNodeMath")
            mix.operation = "MULTIPLY"
            nt.links.new(tex.outputs["Color"], mix.inputs[0])
            nt.links.new(noise.outputs["Fac"], mix.inputs[1])
            fac = mix.outputs[0]
        nt.links.new(coord.outputs["Object"], tex.inputs["Vector"])
        nt.links.new(fac, ramp.inputs["Fac"])
        col = ramp
        nt.links.new(ramp.outputs["Color"], bsdf.inputs["Base Color"])
    else:
        bsdf.inputs["Base Color"].default_value = c1
    bsdf.inputs["Roughness"].default_value = rough
    bsdf.inputs["Metallic"].default_value = metal
    m["rough"] = rough
    m["metal"] = metal
    m["team"] = 1.0 if team else 0.0
    m["color"] = list(c1)
    if col:
        col.name = "COL"
    _mats[name] = m
    return m


def palette():
    """A shared palette, so models made separately match."""
    return {
        "hoof": mat("hoof", srgb(56, 48, 40), 0.55),
        "wood": mat("wood", srgb(122, 86, 54), 0.75, pattern="grain", scale=3.0, amount=0.35),
        "dark_wood": mat("dark_wood", srgb(78, 54, 36), 0.7, pattern="grain", scale=3.0, amount=0.35),
        "planks": mat("planks", srgb(140, 104, 66), 0.8, pattern="streaks", scale=2.0, amount=0.35),
        "iron": mat("iron", srgb(130, 132, 138), 0.38, metal=1.0, pattern="noise", scale=12, amount=0.35),
        "bronze": mat("bronze", srgb(184, 128, 64), 0.32, metal=1.0, pattern="noise", scale=10, amount=0.3),
        "leather": mat("leather", srgb(104, 66, 40), 0.6, pattern="noise", scale=14, amount=0.35),
        "skin": mat("skin", srgb(214, 168, 132), 0.55, pattern="noise", scale=20, amount=0.1),
        "hair": mat("hair", srgb(60, 42, 30), 0.6, pattern="hair", scale=8),
        "linen": mat("linen", srgb(226, 216, 192), 0.85, pattern="weave", scale=40, amount=0.2),
        "rope": mat("rope", srgb(170, 140, 96), 0.85, pattern="hair", scale=30, amount=0.4),
        "team": mat("team", srgb(232, 232, 232), 0.8, team=True, pattern="weave", scale=40, amount=0.18),
        "team_paint": mat("team_paint", srgb(236, 236, 236), 0.55, team=True, pattern="noise", scale=6, amount=0.15),
        "trousers": mat("trousers", srgb(84, 70, 56), 0.85, pattern="weave", scale=40, amount=0.25),
        "plaster": mat("plaster", srgb(236, 228, 210), 0.9, pattern="noise", scale=5, amount=0.18),
        "stone": mat("stone", srgb(150, 142, 130), 0.85, pattern="noise", scale=4, amount=0.45),
        "thatch": mat("thatch", srgb(168, 132, 74), 0.95, pattern="hair", scale=12, amount=0.45),
        "black": mat("black", srgb(28, 24, 22), 0.5),
        "white": mat("white", srgb(240, 236, 228), 0.6),
    }


# ---------------------------------------------------------------- shapes


def _finish_obj(obj, material, smooth=0.6):
    obj.data.materials.clear()
    obj.data.materials.append(material)
    if smooth is not None:
        obj.data.shade_smooth()
        obj.data.set_sharp_from_angle(angle=smooth)
    else:
        obj.data.shade_flat()
    return obj


def _place(obj, loc=(0, 0, 0), rot=(0, 0, 0), scale=(1, 1, 1)):
    obj.location = loc
    obj.rotation_euler = rot
    obj.scale = scale
    return obj


def box(size, material, loc=(0, 0, 0), rot=(0, 0, 0), bevel=0.0):
    bpy.ops.mesh.primitive_cube_add(size=1)
    o = bpy.context.object
    o.scale = size
    bpy.ops.object.transform_apply(scale=True)
    if bevel:
        mod = o.modifiers.new("bevel", "BEVEL")
        mod.width = bevel
        mod.segments = 2
        mod.limit_method = "ANGLE"
    _place(o, loc, rot)
    return _finish_obj(o, material, smooth=0.6)


def cyl(r, depth, material, loc=(0, 0, 0), rot=(0, 0, 0), n=12, r2=None, smooth=0.9):
    """A cylinder (or a cone with `r2`) along its local Z."""
    if r2 is None:
        bpy.ops.mesh.primitive_cylinder_add(vertices=n, radius=r, depth=depth)
    else:
        bpy.ops.mesh.primitive_cone_add(vertices=n, radius1=r, radius2=r2, depth=depth)
    o = _place(bpy.context.object, loc, rot)
    return _finish_obj(o, material, smooth)


def rod(a, b, r, material, n=8, r2=None):
    """A cylinder from point a to point b."""
    a, b = Vector(a), Vector(b)
    d = b - a
    o = cyl(r, d.length, material, loc=(a + b) / 2, n=n, r2=r2)
    o.rotation_euler = d.to_track_quat("Z", "Y").to_euler()
    return o


def ball(r, material, loc=(0, 0, 0), scale=(1, 1, 1), subdiv=2, rot=(0, 0, 0)):
    bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=subdiv, radius=r)
    o = _place(bpy.context.object, loc, rot, scale)
    return _finish_obj(o, material, smooth=math.pi)


def skin(name, points, edges, material, subdiv=2, smooth=math.pi):
    """An organic shape grown around a skeleton with the skin modifier.

    `points`: [(x, y, z, rx, ry)], radii across the skeleton; `edges`: index
    pairs. Point 0 is the root.
    """
    me = bpy.data.meshes.new(name)
    me.from_pydata([p[:3] for p in points], edges, [])
    o = bpy.data.objects.new(name, me)
    bpy.context.collection.objects.link(o)
    mod = o.modifiers.new("skin", "SKIN")
    mod.use_smooth_shade = True
    sv = me.skin_vertices[0].data
    for i, p in enumerate(points):
        sv[i].radius = (p[3], p[4])
    sv[0].use_root = True
    sub = o.modifiers.new("sub", "SUBSURF")
    sub.levels = subdiv
    sub.render_levels = subdiv
    bpy.context.view_layer.objects.active = o
    return _finish_obj(o, material, smooth)


def blobs(name, elements, material, res=0.025, faces=6000, threshold=0.6):
    """A smooth organic body from metaballs, decimated to about `faces`.

    Elements: ("ball", centre, r), ("ellipsoid", centre, (sx, sy, sz), r=1,
    euler=(0,0,0)), ("capsule", a, b, r), each with an optional trailing
    stiffness (default 2) — negative stiffness carves. Sizes are what an
    element shows on its own (a metaball's surface lies at 0.575 of its
    radius); where elements meet they blend and swell a little.
    """
    grow = 1 / 0.575
    mb = bpy.data.metaballs.new(name)
    mb.resolution = res
    mb.render_resolution = res
    mb.threshold = threshold
    from mathutils import Euler

    for e in elements:
        kind = e[0]
        if kind == "ball":
            el = mb.elements.new(type="BALL")
            el.co, el.radius = e[1], e[2] * grow
            stiff = e[3] if len(e) > 3 else 2.0
        elif kind == "ellipsoid":
            el = mb.elements.new(type="ELLIPSOID")
            el.co = e[1]
            el.size_x, el.size_y, el.size_z = (v * grow for v in e[2])
            el.radius = e[3] if len(e) > 3 else 1.0
            if len(e) > 4:
                el.rotation = Euler(e[4]).to_quaternion()
            stiff = e[5] if len(e) > 5 else 2.0
        else:
            a, b = Vector(e[1]), Vector(e[2])
            el = mb.elements.new(type="CAPSULE")
            el.co = (a + b) / 2
            el.size_x = (b - a).length / 2
            el.radius = e[3] * grow
            el.rotation = Vector((1, 0, 0)).rotation_difference((b - a).normalized())
            stiff = e[4] if len(e) > 4 else 2.0
        el.use_negative = stiff < 0
        el.stiffness = abs(stiff)
    o = bpy.data.objects.new(name, mb)
    bpy.context.collection.objects.link(o)
    bpy.ops.object.select_all(action="DESELECT")
    o.select_set(True)
    bpy.context.view_layer.objects.active = o
    bpy.ops.object.convert(target="MESH")
    o = bpy.context.object
    o.name = name
    have = len(o.data.polygons)
    if have > faces:
        mod = o.modifiers.new("dec", "DECIMATE")
        mod.ratio = faces / have
    return _finish_obj(o, material, smooth=math.pi)


def union(name, elements, material, res=0.02, faces=5000, smooth=6, smooth_factor=0.6):
    """A smooth organic body as the exact union of primitives (same element
    format as `blobs`, sizes as given): voxel-remeshed so they fuse, then
    smoothed at the seams (keeping volume) and decimated to about `faces`.
    Unlike metaballs, overlapping parts don't swell each other."""
    from mathutils import Euler, Matrix

    bm = bmesh.new()

    def sphere(m):
        g = bmesh.ops.create_uvsphere(bm, u_segments=18, v_segments=12, radius=1.0)
        bmesh.ops.transform(bm, matrix=m, verts=g["verts"])

    for e in elements:
        kind = e[0]
        if kind == "ball":
            sphere(Matrix.Translation(e[1]) @ Matrix.Diagonal((e[2], e[2], e[2], 1)))
        elif kind == "ellipsoid":
            r = e[3] if len(e) > 3 else 1.0
            rot = Euler(e[4]).to_matrix().to_4x4() if len(e) > 4 else Matrix.Identity(4)
            sx, sy, sz = (v * r for v in e[2])
            sphere(Matrix.Translation(e[1]) @ rot @ Matrix.Diagonal((sx, sy, sz, 1)))
        else:
            a, b, r = Vector(e[1]), Vector(e[2]), e[3]
            d = b - a
            if d.length > 1e-6:
                g = bmesh.ops.create_cone(bm, cap_ends=True, segments=14, radius1=r, radius2=r, depth=d.length)
                m = Matrix.Translation((a + b) / 2) @ d.to_track_quat("Z", "Y").to_matrix().to_4x4()
                bmesh.ops.transform(bm, matrix=m, verts=g["verts"])
            for c in (a, b):
                sphere(Matrix.Translation(c) @ Matrix.Diagonal((r, r, r, 1)))
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    o = bpy.data.objects.new(name, me)
    bpy.context.collection.objects.link(o)
    bpy.ops.object.select_all(action="DESELECT")
    o.select_set(True)
    bpy.context.view_layer.objects.active = o
    rm = o.modifiers.new("remesh", "REMESH")
    rm.mode = "VOXEL"
    rm.voxel_size = res
    bpy.ops.object.modifier_apply(modifier=rm.name)
    if smooth:
        sm = o.modifiers.new("smooth", "LAPLACIANSMOOTH")
        sm.iterations = smooth
        sm.lambda_factor = smooth_factor
        sm.use_volume_preserve = True
        bpy.ops.object.modifier_apply(modifier=sm.name)
    have = len(o.data.polygons)
    if have > faces:
        mod = o.modifiers.new("dec", "DECIMATE")
        mod.ratio = faces / have
    return _finish_obj(o, material, smooth=math.pi)


def paint(obj, materials, pick):
    """Sets each face's material: `pick(centre, normal)` returns an index
    into `materials` (applied after the object's modifiers)."""
    apply_all(obj)
    me = obj.data
    me.materials.clear()
    for m in materials:
        me.materials.append(m)
    for poly in me.polygons:
        poly.material_index = pick(poly.center, poly.normal)
    return obj


def chain(points):
    """Edges joining consecutive points from index `start`."""
    return [(i, i + 1) for i in range(len(points) - 1)]


def loft(name, rings, material, closed=True, cap=True, smooth=math.pi):
    """A surface through rings of points (each a list of xyz, same length)."""
    bm = bmesh.new()
    verts = [[bm.verts.new(p) for p in ring] for ring in rings]
    n = len(rings[0])
    for a, b in zip(verts, verts[1:]):
        for i in range(n if closed else n - 1):
            j = (i + 1) % n
            bm.faces.new((a[i], a[j], b[j], b[i]))
    if cap:
        bm.faces.new(list(reversed(verts[0])))
        bm.faces.new(verts[-1])
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    o = bpy.data.objects.new(name, me)
    bpy.context.collection.objects.link(o)
    return _finish_obj(o, material, smooth)


def sheet(name, w, h, material, nx=10, ny=10, bulge=0.0, loc=(0, 0, 0), rot=(0, 0, 0), thickness=0.0, sag=0.0):
    """A cloth sheet in the XZ plane, bellied out towards -Y by `bulge`."""
    bm = bmesh.new()
    grid = []
    for j in range(ny + 1):
        row = []
        for i in range(nx + 1):
            u, v = i / nx, j / ny
            x = (u - 0.5) * w
            z = (v - 0.5) * h - sag * math.sin(math.pi * u) * (1 - v)
            y = -bulge * math.sin(math.pi * u) * math.sin(math.pi * (0.15 + 0.85 * v))
            row.append(bm.verts.new((x, y, z)))
        grid.append(row)
    for j in range(ny):
        for i in range(nx):
            bm.faces.new((grid[j][i], grid[j][i + 1], grid[j + 1][i + 1], grid[j + 1][i]))
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    o = bpy.data.objects.new(name, me)
    bpy.context.collection.objects.link(o)
    if thickness:
        mod = o.modifiers.new("solid", "SOLIDIFY")
        mod.thickness = thickness
    _place(o, loc, rot)
    return _finish_obj(o, material, smooth=math.pi)


def mirror_x(objs):
    """Copies of objects mirrored across x = 0."""
    out = []
    for o in objs:
        c = o.copy()
        c.data = o.data.copy()
        bpy.context.collection.objects.link(c)
        c.location.x = -c.location.x
        c.rotation_euler.y = -c.rotation_euler.y
        c.rotation_euler.z = -c.rotation_euler.z
        out.append(c)
    return out


def apply_all(o):
    bpy.ops.object.select_all(action="DESELECT")
    o.select_set(True)
    bpy.context.view_layer.objects.active = o
    for m in list(o.modifiers):
        bpy.ops.object.modifier_apply(modifier=m.name)
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)


def join(objs, name):
    for o in objs:
        apply_all(o)
    bpy.ops.object.select_all(action="DESELECT")
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]
    bpy.ops.object.join()
    o = bpy.context.object
    o.name = name
    return o


def transform(objs, matrix):
    """Applies a matrix to finished objects (to pose a group)."""
    bpy.context.view_layer.update()
    for o in objs:
        o.matrix_world = matrix @ o.matrix_world


def pivot(objs, about, euler):
    """Rotates a group of objects about a point."""
    m = Matrix.Translation(about) @ euler.to_matrix().to_4x4() @ Matrix.Translation(-Vector(about))
    transform(objs, m)


# ---------------------------------------------------------------- rigs


class Rig:
    """Bones for a skinned model, posed as the model is built.

    Each bone has a head (its pivot), an optional tail (for weighting; by
    default its first child's head) and a radius: vertices weight to the
    bones they are nearest, measured from the bone's surface, so a thick
    torso bone wins over a thin arm passing close by. `style` tells the
    game's animator how the unit attacks (melee, thrust, bow, gun, sling,
    siege, cannon, none).
    """

    def __init__(self, style="none"):
        self.bones = []
        self.style = style

    def bone(self, name, parent, head, tail=None, radius=0.0):
        assert all(b["name"] != name for b in self.bones), name
        assert parent is None or self.has(parent), f"{name}: parent {parent} must come first"
        self.bones.append({"name": name, "parent": parent, "head": Vector(head), "tail": Vector(tail) if tail else None, "radius": radius})
        return name

    def index(self, name):
        return next(i for i, b in enumerate(self.bones) if b["name"] == name)

    def has(self, name):
        return any(b["name"] == name for b in self.bones)

    def segment(self, i):
        b = self.bones[i]
        if b["tail"] is not None:
            return b["head"], b["tail"]
        child = next((c for c in self.bones if c["parent"] == b["name"]), None)
        return b["head"], child["head"] if child else b["head"]


def bind(objs, bone):
    """Binds objects rigidly to one bone."""
    for o in objs if isinstance(objs, (list, tuple)) else [objs]:
        o["bone"] = bone
    return objs


def blend(objs, bones):
    """Lets objects weight smoothly among the named bones."""
    for o in objs if isinstance(objs, (list, tuple)) else [objs]:
        o["bones"] = " ".join(bones)
    return objs


def _hints(objs, rig):
    """Stores each object's binding in an integer point attribute, so it
    survives the join: >= 0 rigid to that bone, < 0 a group to blend in."""
    groups = []
    for o in objs:
        if "bone" in o:
            h = rig.index(o["bone"])
        elif "bones" in o:
            group = [rig.index(n) for n in o["bones"].split()]
            if group not in groups:
                groups.append(group)
            h = -1 - groups.index(group)
        else:
            h = 0
        a = o.data.attributes.new("hint", "INT", "POINT")
        a.data.foreach_set("value", np.full(len(o.data.vertices), h, dtype=np.int32))
    return groups


def _weights(obj, rig, groups):
    me = obj.data
    n = len(me.vertices)
    pos = np.empty(n * 3, dtype=np.float32)
    me.vertices.foreach_get("co", pos)
    pos = pos.reshape(n, 3)
    hint = np.empty(n, dtype=np.int32)
    me.attributes["hint"].data.foreach_get("value", hint)
    nb = len(rig.bones)
    dist = np.empty((n, nb), dtype=np.float32)
    for i in range(nb):
        a, b = (np.array(v, dtype=np.float32) for v in rig.segment(i))
        ab = b - a
        t = np.clip(((pos - a) @ ab) / max(float(ab @ ab), 1e-9), 0.0, 1.0)
        d = np.linalg.norm(pos - (a + t[:, None] * ab), axis=1) - rig.bones[i]["radius"]
        dist[:, i] = np.maximum(d, 0.004)
    joints = np.zeros((n, 3), dtype=np.float32)
    weights = np.zeros((n, 3), dtype=np.float32)
    for v in range(n):
        h = hint[v]
        if h >= 0:
            joints[v, 0], weights[v, 0] = h, 1.0
            continue
        cand = groups[-1 - h]
        d = dist[v, cand]
        order = np.argsort(d)[:3]
        w = 1.0 / d[order] ** 4
        w /= w.sum()
        for k, o in enumerate(order):
            joints[v, k], weights[v, k] = cand[o], w[k]
    a = me.attributes.new("_joints", "FLOAT_VECTOR", "POINT")
    a.data.foreach_set("vector", joints.ravel())
    a = me.attributes.new("_weights", "FLOAT_VECTOR", "POINT")
    a.data.foreach_set("vector", weights.ravel())
    me.attributes.remove(me.attributes["hint"])


def _rig_extras(rig, scale):
    """The rig as a string for the node's extras: the attack style, then
    `name parent x y z` per bone in glTF space (Y up, scaled)."""
    out = [rig.style]
    for b in rig.bones:
        h = b["head"] * scale
        parent = rig.index(b["parent"]) if b["parent"] else -1
        out.append(f"{b['name']} {parent} {h.x:.5f} {h.z:.5f} {-h.y:.5f}")
    return ";".join(out)


# ---------------------------------------------------------------- baking


def _emit_setup(m, channel):
    """Rewires a material to emit one of its channels for baking."""
    nt = m.node_tree
    out = next(n for n in nt.nodes if n.type == "OUTPUT_MATERIAL")
    em = nt.nodes.new("ShaderNodeEmission")
    em.name = "BAKE_EMIT"
    if channel == "color":
        col = nt.nodes.get("COL")
        if col:
            nt.links.new(col.outputs["Color"], em.inputs["Color"])
        else:
            em.inputs["Color"].default_value = tuple(m["color"])
    elif channel == "orm":
        # glTF metallic-roughness: G roughness, B metallic.
        em.inputs["Color"].default_value = (0.0, m["rough"], m["metal"], 1.0)
    nt.links.new(em.outputs[0], out.inputs["Surface"])


def _emit_clear(m):
    nt = m.node_tree
    out = next(n for n in nt.nodes if n.type == "OUTPUT_MATERIAL")
    bsdf = next(n for n in nt.nodes if n.type == "BSDF_PRINCIPLED")
    em = nt.nodes.get("BAKE_EMIT")
    if em:
        nt.nodes.remove(em)
    nt.links.new(bsdf.outputs[0], out.inputs["Surface"])


def _bake_target(m, image):
    nt = m.node_tree
    node = nt.nodes.get("BAKE_IMG") or nt.nodes.new("ShaderNodeTexImage")
    node.name = "BAKE_IMG"
    node.image = image
    for n in nt.nodes:
        n.select = False
    node.select = True
    nt.nodes.active = node


def _ground(size=60.0, at=(0, 0, 0)):
    bpy.ops.mesh.primitive_plane_add(size=size, location=at)
    g = bpy.context.object
    g.name = "bake_ground"
    g.data.materials.append(mat("ground", (0.5, 0.5, 0.5, 1)))
    return g


def _bake(obj, kind, target, samples):
    scene = bpy.context.scene
    scene.cycles.samples = samples
    scene.render.bake.target = target
    scene.render.bake.margin = 6
    scene.render.bake.use_clear = True
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.bake(type="EMIT" if kind == "EMIT" else "AO")


def _pixels(image):
    a = np.empty(image.size[0] * image.size[1] * 4, dtype=np.float32)
    image.pixels.foreach_get(a)
    return a.reshape(image.size[1], image.size[0], 4)


def _to_srgb(c):
    c = np.clip(c, 0.0, 1.0)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * np.power(c, 1 / 2.4) - 0.055)


def _team_mask(obj):
    """Per-corner team mask (1 on faces with a team material) as COLOR_0."""
    me = obj.data
    attr = me.color_attributes.new("team", "BYTE_COLOR", "CORNER")
    team = [m["team"] if m else 0.0 for m in me.materials]
    cols = np.zeros((len(me.loops), 4), dtype=np.float32)
    cols[:, 3] = 1.0
    for poly in me.polygons:
        t = team[poly.material_index]
        for li in poly.loop_indices:
            cols[li, 0] = t
    attr.data.foreach_set("color", cols.ravel())
    me.color_attributes.active_color = attr
    return attr


def _world(ao_distance):
    scene = bpy.context.scene
    scene.world = scene.world or bpy.data.worlds.new("w")
    scene.world.light_settings.distance = ao_distance


def _export(objs, path, textured, rig=False):
    bpy.ops.object.select_all(action="DESELECT")
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]
    os.makedirs(MODELS, exist_ok=True)
    bpy.ops.export_scene.gltf(
        filepath=path,
        use_selection=True,
        export_format="GLB",
        export_yup=True,
        export_apply=True,
        export_normals=True,
        export_texcoords=textured,
        export_materials="EXPORT" if textured else "NONE",
        export_vertex_color="NAME",
        export_vertex_color_name="team" if textured else "Col",
        export_all_vertex_colors=False,
        export_attributes=rig,
        export_extras=rig,
        export_image_format="AUTO",
    )


def finish(objs, name, kind="unit", scale=1.0, rig=None, res=512, ao_distance=0.6, ao_strength=0.85):
    """Joins, bakes and exports a unit (`kind="unit"`, textured, optionally
    skinned to `rig`) or a single prop (`kind="prop"`); returns the object."""
    if kind == "prop":
        return finish_props({name: objs}, name, scale=scale, ao_distance=ao_distance, ao_strength=ao_strength)[0]
    groups = _hints(objs, rig) if rig else None
    obj = join(objs, name)
    mats = list(obj.data.materials)
    ground = _ground()
    _world(ao_distance)
    if rig:
        _weights(obj, rig, groups)

    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.uv.smart_project(angle_limit=math.radians(60), island_margin=0.004, scale_to_bounds=False)
    bpy.ops.uv.pack_islands(margin=0.004, rotate=True)
    bpy.ops.object.mode_set(mode="OBJECT")

    imgs = {k: bpy.data.images.new(f"{name}_{k}", res, res, alpha=False, float_buffer=True) for k in ("color", "orm", "ao")}
    for channel in ("color", "orm"):
        for m in mats:
            _emit_setup(m, channel)
            _bake_target(m, imgs[channel])
        _bake(obj, "EMIT", "IMAGE_TEXTURES", 1)
        for m in mats:
            _emit_clear(m)
    for m in mats:
        _bake_target(m, imgs["ao"])
    _bake(obj, "AO", "IMAGE_TEXTURES", 128)

    color = _pixels(imgs["color"])
    ao = _pixels(imgs["ao"])[..., :1]
    color[..., :3] = _to_srgb(color[..., :3] * (1.0 - ao_strength * (1.0 - ao)))
    tex = bpy.data.images.new(f"{name}_albedo", res, res, alpha=False)
    tex.pixels.foreach_set(color.ravel())
    orm = bpy.data.images.new(f"{name}_mr", res, res, alpha=False)
    # Before the pixels: changing a generated image's colour space resets it.
    orm.colorspace_settings.name = "Non-Color"
    orm.pixels.foreach_set(_pixels(imgs["orm"]).ravel())

    # One material with the baked textures replaces the originals.
    final = bpy.data.materials.new(f"{name}_baked")
    try:
        final.use_nodes = True
    except Exception:
        pass
    nt = final.node_tree
    bsdf = next(n for n in nt.nodes if n.type == "BSDF_PRINCIPLED")
    t1 = nt.nodes.new("ShaderNodeTexImage")
    t1.image = tex
    nt.links.new(t1.outputs["Color"], bsdf.inputs["Base Color"])
    t2 = nt.nodes.new("ShaderNodeTexImage")
    t2.image = orm
    sep = nt.nodes.new("ShaderNodeSeparateColor")
    nt.links.new(t2.outputs["Color"], sep.inputs["Color"])
    nt.links.new(sep.outputs["Green"], bsdf.inputs["Roughness"])
    nt.links.new(sep.outputs["Blue"], bsdf.inputs["Metallic"])
    _team_mask(obj)
    obj.data.materials.clear()
    obj.data.materials.append(final)

    bpy.data.objects.remove(ground)
    obj.scale = (scale, scale, scale)
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.transform_apply(scale=True)
    if rig:
        obj["rig"] = _rig_extras(rig, scale)
    _export([obj], os.path.join(MODELS, f"{name}.glb"), True, rig=bool(rig))
    tris = sum(len(p.vertices) - 2 for p in obj.data.polygons)
    print(f"[art] {name}: {tris} triangles, {len(obj.data.vertices)} vertices, {len(rig.bones) if rig else 0} bones")
    return obj


# Prop vertex-colour alpha flags, read by the game: bare surfaces (1),
# foliage (0, snow and autumn colours), the owner's colour (0.66, tinted),
# windows (0.33, lit at night).
FLAG = {"bare": 1.0, "foliage": 0.0, "team": 0.66, "window": 0.33}


def flag(material, kind):
    material["flag"] = FLAG[kind]
    return material


def finish_props(named, name, scale=1.0, ao_distance=0.6, ao_strength=0.85, ground=True, foliage_shade=0.0):
    """Bakes a library of props (colour and occlusion into vertex colours,
    the flag in alpha) and exports them as named nodes of one .glb."""
    _world(ao_distance)
    out = []
    for i, (pname, objs) in enumerate(named.items()):
        # A prop may come with its own scale: (objects, scale).
        own = 1.0
        if isinstance(objs, tuple):
            objs, own = objs
        obj = join(objs, pname)
        # Bake each prop alone, far from the others.
        shift = Vector((i * 200.0, 0, 0))
        obj.location = shift
        g = _ground(at=shift) if ground else None
        me = obj.data
        mats = list(me.materials)
        col = me.color_attributes.new("albedo", "FLOAT_COLOR", "CORNER")
        ao = me.color_attributes.new("ao", "FLOAT_COLOR", "CORNER")
        me.color_attributes.active_color = col
        for m in mats:
            _emit_setup(m, "color")
        _bake(obj, "EMIT", "VERTEX_COLORS", 1)
        for m in mats:
            _emit_clear(m)
        me.color_attributes.active_color = ao
        _bake(obj, "AO", "VERTEX_COLORS", 128)
        a = np.empty(len(me.loops) * 4, dtype=np.float32)
        b = np.empty(len(me.loops) * 4, dtype=np.float32)
        col.data.foreach_get("color", a)
        ao.data.foreach_get("color", b)
        a, b = a.reshape(-1, 4), b.reshape(-1, 4)
        a[:, :3] *= 1.0 - ao_strength * (1.0 - b[:, :1])
        flags = [m.get("flag", 1.0) if m else 1.0 for m in mats]
        loop_flag = np.ones(len(me.loops), dtype=np.float32)
        for poly in me.polygons:
            loop_flag[list(poly.loop_indices)] = flags[poly.material_index]
        if foliage_shade:
            # Crowns darken towards their undersides and insides, which a few
            # vertices' occlusion alone can't show.
            vz = np.array([me.vertices[l.vertex_index].co.z for l in me.loops], dtype=np.float32)
            leaf = loop_flag < 0.16
            if leaf.any():
                lo, hi = vz[leaf].min(), vz[leaf].max()
                f = np.clip((vz - lo) / max(hi - lo, 1e-4), 0.0, 1.0)
                a[leaf, :3] *= (1.0 - foliage_shade + foliage_shade * f[leaf])[:, None]
        a[:, 3] = loop_flag
        c = me.color_attributes.new("Col", "FLOAT_COLOR", "CORNER")
        c.data.foreach_set("color", a.ravel())
        me.color_attributes.remove(me.color_attributes["albedo"])
        me.color_attributes.remove(me.color_attributes["ao"])
        me.color_attributes.active_color = me.color_attributes["Col"]
        me.materials.clear()
        me.materials.append(mat("prop_vc", (1, 1, 1, 1)))
        if g:
            bpy.data.objects.remove(g)
        obj.location = (0, 0, 0)
        obj.scale = (scale * own,) * 3
        bpy.ops.object.select_all(action="DESELECT")
        obj.select_set(True)
        bpy.context.view_layer.objects.active = obj
        bpy.ops.object.transform_apply(location=True, scale=True)
        tris = sum(len(p.vertices) - 2 for p in me.polygons)
        print(f"[art] {name}/{pname}: {tris} triangles")
        out.append(obj)
    _export(out, os.path.join(MODELS, f"{name}.glb"), False)
    return out


# ---------------------------------------------------------------- previews


def preview(objs, name, team=(0.45, 0.25, 0.75), size=560):
    """Renders finished models from the game's camera angle (pitch 0.95 rad
    below the horizon) to target/art/<name>.png: one model from front-left,
    front-right and behind; a library side by side from front-left. Team
    areas show in `team`."""
    objs = objs if isinstance(objs, list) else [objs]
    scene = bpy.context.scene
    scene.cycles.samples = 40
    for obj in objs:
        for m in obj.data.materials:
            _preview_material(m, team)
    # Lay a library out in rows.
    if len(objs) > 1:
        cols = math.ceil(math.sqrt(len(objs) * 1.6))
        widths = [max((Vector(o.bound_box[6]) - Vector(o.bound_box[0])).x, (Vector(o.bound_box[6]) - Vector(o.bound_box[0])).y) for o in objs]
        cell = max(widths) * 1.15
        for i, o in enumerate(objs):
            o.location = ((i % cols) * cell, (i // cols) * cell, 0)
    bpy.context.view_layer.update()
    pts = [o.matrix_world @ Vector(c) for o in objs for c in o.bound_box]
    lo = Vector((min(p.x for p in pts), min(p.y for p in pts), min(p.z for p in pts)))
    hi = Vector((max(p.x for p in pts), max(p.y for p in pts), max(p.z for p in pts)))
    centre = (lo + hi) / 2
    extent = (hi - lo).length

    bpy.ops.mesh.primitive_plane_add(size=max(200, extent * 4), location=(centre.x, centre.y, 0))
    ground = bpy.context.object
    ground.data.materials.append(mat("preview_ground", srgb(104, 128, 72), 0.95, pattern="noise", scale=0.5 / max(1.0, extent / 4), amount=0.3))
    sun_data = bpy.data.lights.new("sun", "SUN")
    sun_data.energy = 4.0
    sun_data.angle = math.radians(3)
    sun = bpy.data.objects.new("sun", sun_data)
    bpy.context.collection.objects.link(sun)
    sun.rotation_euler = (math.radians(47), 0, math.radians(-135))
    world = scene.world
    try:
        world.use_nodes = True
    except Exception:
        pass
    bg = next(n for n in world.node_tree.nodes if n.type == "BACKGROUND")
    bg.inputs["Color"].default_value = (0.55, 0.65, 0.8, 1)
    bg.inputs["Strength"].default_value = 0.9

    cam_data = bpy.data.cameras.new("cam")
    cam_data.angle = math.radians(30)
    cam = bpy.data.objects.new("cam", cam_data)
    bpy.context.collection.objects.link(cam)
    scene.camera = cam
    scene.view_settings.view_transform = "AgX"
    pitch = 0.95
    single = len(objs) == 1
    yaws = (math.radians(-35), math.radians(35), math.radians(180), "side") if single else (math.radians(-20),)
    scene.render.resolution_x = size if single else size * 2
    scene.render.resolution_y = size if single else int(size * 1.4)
    dist = extent * (1.25 if single else 1.5)
    if not single:
        cam_data.type = "ORTHO"
        cam_data.ortho_scale = max(hi.x - lo.x, (hi.y - lo.y) * 1.25) * 1.08
    os.makedirs(PREVIEWS, exist_ok=True)
    shots = []
    for yaw in yaws:
        # The game looks north (-Z in glTF, +Y here); models face -Y.
        if yaw == "side":
            d = Vector((1.0, 0.0, 0.12)).normalized() * 1.45
        else:
            d = Vector((math.sin(yaw) * math.cos(pitch), -math.cos(yaw) * math.cos(pitch), math.sin(pitch)))
        cam.location = centre + d * dist
        cam.rotation_euler = (-d).to_track_quat("-Z", "Y").to_euler()
        path = os.path.join(PREVIEWS, f"_{name}_{len(shots)}.png")
        scene.render.filepath = path
        bpy.ops.render.render(write_still=True)
        shots.append(bpy.data.images.load(path))
    strip = np.concatenate([_pixels(s) for s in shots], axis=1)
    out = bpy.data.images.new(f"{name}_preview", strip.shape[1], strip.shape[0], alpha=False)
    out.pixels.foreach_set(strip.ravel())
    out.filepath_raw = os.path.join(PREVIEWS, f"{name}.png")
    out.file_format = "PNG"
    out.save()
    for s in shots:
        os.remove(s.filepath_raw)
    print(f"[art] preview: {out.filepath_raw}")


def _preview_material(m, team):
    """Shows a baked material as the game does: textures tinted by the team
    mask, or vertex colours with flagged parts tinted."""
    nt = m.node_tree
    bsdf = next(n for n in nt.nodes if n.type == "BSDF_PRINCIPLED")
    tex = next((n for n in nt.nodes if n.type == "TEX_IMAGE" and n.image and n.image.name.endswith("_albedo")), None)
    attr = nt.nodes.new("ShaderNodeVertexColor")
    attr.layer_name = "team" if tex else "Col"
    mix = nt.nodes.new("ShaderNodeMix")
    mix.data_type = "RGBA"
    mix.inputs["A"].default_value = (1, 1, 1, 1)
    mix.inputs["B"].default_value = team + (1,)
    mul = nt.nodes.new("ShaderNodeMix")
    mul.data_type = "RGBA"
    mul.blend_type = "MULTIPLY"
    mul.inputs["Factor"].default_value = 1.0
    if tex:
        sep = nt.nodes.new("ShaderNodeSeparateColor")
        nt.links.new(attr.outputs["Color"], sep.inputs["Color"])
        nt.links.new(sep.outputs["Red"], mix.inputs["Factor"])
        nt.links.new(tex.outputs["Color"], mul.inputs["A"])
    else:
        # Team flag (alpha 0.66) gets the tint.
        near = nt.nodes.new("ShaderNodeMath")
        near.operation = "COMPARE"
        near.inputs[1].default_value = FLAG["team"]
        near.inputs[2].default_value = 0.05
        nt.links.new(attr.outputs["Alpha"], near.inputs[0])
        nt.links.new(near.outputs[0], mix.inputs["Factor"])
        nt.links.new(attr.outputs["Color"], mul.inputs["A"])
    nt.links.new(mix.outputs["Result"], mul.inputs["B"])
    nt.links.new(mul.outputs["Result"], bsdf.inputs["Base Color"])
