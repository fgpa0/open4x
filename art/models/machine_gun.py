"""Machine Gun: a water-cooled gun on a low tripod behind a steel shield,
a gunner sitting at it with his hands on the grips, a loader kneeling with
the ammunition belt, sandbags in front and boxes of rounds."""

import math

import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("gun")
    olive = k.mat("olive_drab", k.srgb(96, 92, 62), 0.85, pattern="weave", scale=40, amount=0.2)
    helmet_m = k.mat("helmet_steel", k.srgb(88, 92, 70), 0.6, metal=0.3, pattern="noise", scale=10, amount=0.25)
    gun_m = k.mat("gun_metal", k.srgb(58, 60, 62), 0.45, metal=0.8, pattern="noise", scale=10, amount=0.2)
    sand = k.mat("sandbag", k.srgb(170, 152, 112), 0.95, pattern="weave", scale=30, amount=0.35)
    brass = k.mat("brass", k.srgb(200, 160, 70), 0.3, metal=1.0)
    p["trousers"] = olive

    # The gun: a tripod, the water jacket and barrel, grips, a shield, all
    # on the frame; the barrel recoils on its own bone.
    pivot = Vector((0, -0.35, 0.62))
    rig.bone("frame", None, Vector((0, 0, 0.1)), pivot, radius=0.3)
    rig.bone("barrel", "frame", pivot, pivot + Vector((0, -1.0, 0.0)))
    frame = []
    for a in (math.radians(90 + 130), math.radians(90 - 130), math.radians(-90)):
        foot = pivot + Vector((math.cos(a) * 0.7, -math.sin(a) * 0.7 * (1 if a > 0 else 1), -0.62))
        frame.append(k.rod(pivot - Vector((0, 0, 0.05)), Vector((foot.x, foot.y, 0.02)), 0.025, gun_m, n=6))
    frame.append(k.box((0.12, 0.14, 0.1), gun_m, loc=pivot - Vector((0, 0, 0.04))))
    shield = k.box((0.72, 0.03, 0.5), olive, loc=pivot + Vector((0, -0.42, 0.12)), rot=(0.12, 0, 0), bevel=0.02)
    frame.append(shield)
    frame.append(k.box((0.74, 0.035, 0.05), p["team_paint"], loc=pivot + Vector((0, -0.44, 0.34)), rot=(0.12, 0, 0)))
    k.bind(frame, "frame")
    gun = [k.box((0.14, 0.42, 0.16), gun_m, loc=pivot + Vector((0, 0.08, 0.06)), bevel=0.02)]
    gun.append(k.rod(pivot + Vector((0, -0.12, 0.08)), pivot + Vector((0, -0.85, 0.08)), 0.055, gun_m, n=14))
    for y in (-0.3, -0.5, -0.7):
        gun.append(k.cyl(0.06, 0.02, p["black"], loc=pivot + Vector((0, y, 0.08)), rot=(math.pi / 2, 0, 0), n=14))
    gun.append(k.rod(pivot + Vector((0, -0.85, 0.08)), pivot + Vector((0, -1.0, 0.08)), 0.02, gun_m, n=8))
    for s in (-1, 1):
        gun.append(k.rod(pivot + Vector((s * 0.05, 0.3, 0.06)), pivot + Vector((s * 0.05, 0.36, 0.0)), 0.014, p["wood"], n=5))
    # The belt of rounds feeding in from the left.
    for i in range(9):
        t = i / 8
        at = pivot + Vector((-0.1 - t * 0.35, 0.05 + t * 0.12, 0.08 - t * 0.38))
        gun.append(k.cyl(0.014, 0.07, brass, loc=at, rot=(0, math.pi / 2, 0), n=6))
    k.bind(gun, "barrel")
    parts = frame + gun

    # Sandbags in a low arc in front.
    bags = []
    for row in range(2):
        n = 6 - row
        for i in range(n):
            a = math.radians(-50 + 100 * (i + 0.5 * row) / 5)
            r = 1.1
            bags.append(k.ball(0.17, sand, loc=(math.sin(a) * r, -math.cos(a) * r - 0.2, 0.09 + row * 0.15), scale=(1.5, 0.75, 0.55), rot=(0, 0, a), subdiv=2))
    k.bind(bags, "frame")
    parts += bags
    boxes = [k.box((0.3, 0.14, 0.18), olive, loc=(-0.72 + 0.3 * i, 0.32 + 0.3 * i, 0.09), rot=(0, 0, 0.3 * i), bevel=0.015) for i in range(2)]
    k.bind(boxes, "frame")
    parts += boxes

    def helmet(J):
        h, up, side, fwd = J["head"], J["up"], J["side"], J["fwd"]
        helm = [k.ball(0.122, helmet_m, loc=h + up * 0.05, scale=(1.0, 1.05, 0.72))]
        helm.append(k.cyl(0.18, 0.014, helmet_m, loc=h + up * 0.035, n=22, r2=0.16))
        helm.append(k.cyl(0.124, 0.022, p["team"], loc=h + up * 0.06, n=20))
        k.bind(helm, J["prefix"] + "head")
        return helm

    # The gunner, sitting behind the gun with both hands on the grips.
    grip_r, grip_l = pivot + Vector((-0.06, 0.38, 0.02)), pivot + Vector((0.06, 0.38, 0.02))
    seat = Vector((0, 0.9, 0.3))
    arms = (((-0.24, -0.25, 0.98), tuple(grip_r - seat + Vector((0, 0, 0.98)))), ((-0.24, -0.25, 0.98), tuple(Vector((-grip_l.x, grip_l.y, grip_l.z)) - seat + Vector((0, 0, 0.98)))))
    body, J = people.figure(k, p, rig, parent="frame", prefix="g_", at=(0, 0.9, 0), mounted=seat, arms=arms, outfit={"sleeves": "long", "skirt": 0.86})
    parts += body + helmet(J)
    # The loader, sitting on an ammunition box beside it and feeding the belt.
    body, J2 = people.figure(k, p, rig, parent="frame", prefix="l_", at=(-0.7, 0.3, 0), yaw=-1.1, mounted=(-0.7, 0.3, 0.32), arms=("low_fwd", "low_fwd"), outfit={"sleeves": "long", "skirt": 0.86})
    parts += body + helmet(J2)
    return parts, {"kind": "unit", "scale": 0.2, "rig": rig}
