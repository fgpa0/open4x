"""Cannon: a bronze gun on a field carriage with two big wheels, a gunner
with a rammer beside it."""

import math

import engines
import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("cannon")
    axle = Vector((0, -0.2, 0.62))
    rig.bone("frame", None, axle, axle + Vector((0, 1.4, -0.4)), radius=0.3)
    rig.bone("wheel_l", "frame", axle + Vector((0.55, 0, 0)))
    rig.bone("wheel_r", "frame", axle + Vector((-0.55, 0, 0)))
    rig.bone("barrel", "frame", axle + Vector((0, 0, 0.22)), axle + Vector((0, -1.4, 0.3)))
    carriage = []
    for s in (1, -1):
        cheek = engines.beam(k, p, axle + Vector((s * 0.2, -0.3, 0.12)), Vector((s * 0.12, 1.55, 0.12)), 0.16, p["team_paint"])
        carriage.append(cheek)
    carriage.append(k.rod(axle + Vector((-0.6, 0, 0)), axle + Vector((0.6, 0, 0)), 0.06, p["iron"]))
    carriage.append(engines.beam(k, p, (-0.16, 1.45, 0.12), (0.16, 1.45, 0.12), 0.12))
    k.bind(carriage, "frame")
    gun_axis = Vector((0, -1, 0.06)).normalized()
    breech = axle + Vector((0, 0.55, 0.22))
    gun = [k.cyl(0.2, 0.4, p["bronze"], loc=breech + gun_axis * 0.0, n=18, rot=gun_axis.to_track_quat("Z", "Y").to_euler())]
    gun.append(k.rod(breech, breech + gun_axis * 1.95, 0.17, p["bronze"], n=18, r2=0.11))
    gun.append(k.cyl(0.14, 0.12, p["bronze"], loc=breech + gun_axis * 1.95, n=18, rot=gun_axis.to_track_quat("Z", "Y").to_euler()))
    gun.append(k.ball(0.14, p["bronze"], loc=breech - gun_axis * 0.22))
    for f in (0.35, 1.0):
        gun.append(k.cyl(0.2 - f * 0.04, 0.07, p["bronze"], loc=breech + gun_axis * f, n=18, rot=gun_axis.to_track_quat("Z", "Y").to_euler()))
    gun.append(k.cyl(0.04, 0.5, p["iron"], loc=axle + Vector((0, 0, 0.22)), rot=(0, math.pi / 2, 0), n=8))
    k.bind(gun, "barrel")
    parts = carriage + gun
    parts += engines.wheel(k, p, axle + Vector((0.55, 0, 0)), 0.62, "wheel_l", spokes=12)
    parts += engines.wheel(k, p, axle + Vector((-0.55, 0, 0)), 0.62, "wheel_r", spokes=12)
    balls = [k.ball(0.11, p["iron"], loc=(0.75 + 0.2 * (i % 2), 0.9 + 0.2 * (i // 2), 0.11)) for i in range(3)]
    k.bind(balls, "frame")
    parts += balls
    body, J = people.figure(k, p, rig, at=(-1.0, -0.35, 0), yaw=-0.4, arms=("hold", "hang"), stride=0.1, outfit={"sleeves": "long", "skirt": 0.6, "beard": True})
    parts += body
    parts += people.helmet(k, p, J, "tricorne")
    wr = J["wrist_r"]
    parts += people.tool(k, p, wr + Vector((0, 0.1, -1.0)), wr + Vector((0, -0.15, 1.0)), "staff", bone="arm_r_lo")
    return parts, {"kind": "unit", "scale": 0.17, "rig": rig}
