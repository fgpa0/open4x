"""Artillery: a long steel breech-loader on a carriage with a shield and
spoked wheels, painted field green, a gunner with a shell beside it."""

import math

import engines
import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("cannon")
    green = k.mat("field_green", k.srgb(92, 104, 72), 0.6, pattern="noise", scale=6, amount=0.2)
    axle = Vector((0, -0.1, 0.6))
    rig.bone("frame", None, axle, axle + Vector((0, 1.6, -0.4)), radius=0.3)
    rig.bone("wheel_l", "frame", axle + Vector((0.6, 0, 0)))
    rig.bone("wheel_r", "frame", axle + Vector((-0.6, 0, 0)))
    rig.bone("barrel", "frame", axle + Vector((0, 0, 0.25)), axle + Vector((0, -1.8, 0.35)))
    frame = []
    frame.append(engines.beam(k, p, axle + Vector((0, -0.1, 0.1)), Vector((0, 2.0, 0.08)), 0.16, green))
    frame.append(k.box((1.3, 0.05, 0.75), green, loc=axle + Vector((0, -0.35, 0.45)), bevel=0.02))
    frame.append(k.box((1.3, 0.06, 0.08), p["team_paint"], loc=axle + Vector((0, -0.37, 0.84))))
    frame.append(k.rod(axle + Vector((-0.66, 0, 0)), axle + Vector((0.66, 0, 0)), 0.05, p["iron"]))
    frame.append(k.box((0.3, 0.2, 0.06), p["iron"], loc=(0, 1.95, 0.05)))
    k.bind(frame, "frame")
    gun_axis = Vector((0, -1, 0.08)).normalized()
    breech = axle + Vector((0, 0.45, 0.25))
    rot = gun_axis.to_track_quat("Z", "Y").to_euler()
    gun = [k.cyl(0.17, 0.5, p["steel"], loc=breech, n=16, rot=rot)]
    gun.append(k.rod(breech, breech + gun_axis * 2.4, 0.11, p["steel"], n=16, r2=0.08))
    gun.append(k.cyl(0.1, 0.1, p["steel"], loc=breech + gun_axis * 2.4, n=16, rot=rot))
    gun.append(k.cyl(0.13, 0.3, green, loc=breech + gun_axis * 0.5, n=12, rot=rot))
    k.bind(gun, "barrel")
    parts = frame + gun
    parts += engines.wheel(k, p, axle + Vector((0.6, 0, 0)), 0.6, "wheel_l", spokes=14)
    parts += engines.wheel(k, p, axle + Vector((-0.6, 0, 0)), 0.6, "wheel_r", spokes=14)
    shells = [k.cyl(0.08, 0.32, p["bronze"], loc=(0.9 + 0.18 * (i % 2), 0.9 + 0.18 * (i // 2), 0.16), n=10, r2=0.04) for i in range(3)]
    k.bind(shells, "frame")
    parts += shells
    body, J = people.figure(k, p, rig, at=(-1.1, 0.4, 0), yaw=-0.5, arms=("hold", "hold"), stride=0.1, outfit={"sleeves": "long", "skirt": 0.84})
    parts += body
    h, up, fwd = J["head"], J["up"], J["fwd"]
    cap = [k.cyl(0.1, 0.12, p["black"], loc=h + up * 0.11, n=16, r2=0.09), k.cyl(0.103, 0.03, p["team"], loc=h + up * 0.07, n=16)]
    cap.append(k.box((0.15, 0.07, 0.012), p["black"], loc=h + up * 0.06 + fwd * 0.1, rot=(-0.25, 0, 0)))
    k.bind(cap, J["prefix"] + "head")
    parts += cap
    shell = k.cyl(0.08, 0.32, p["bronze"], loc=(J["wrist_r"] + J["wrist_l"]) / 2 + Vector((0, 0, 0.02)), rot=(math.pi / 2, 0, 0), n=10)
    k.bind([shell], "arm_r_lo")
    parts.append(shell)
    return parts, {"kind": "unit", "scale": 0.16, "rig": rig}
