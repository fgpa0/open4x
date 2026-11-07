"""Ox cart: an ox drawing a two-wheeled cart of sacks and barrels under a
cloth in the owner's colour, its driver walking alongside. Seen on the roads
between a civ's cities (ambient traffic, not a unit)."""

import math

import beasts
import engines
import people
import ships
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("none")
    coat = k.mat("ox", k.srgb(150, 118, 84), 0.85, pattern="noise", scale=4, amount=0.3, color2=k.srgb(110, 84, 60))
    parts, A = beasts.quadruped(k, p, "ox", coat, rig=rig, prefix="ox_", points=k.mat("ox_dark", k.srgb(70, 54, 42), 0.8), at=(0, -1.25, 0))
    # The cart: a plank bed on an axle with two solid-rimmed wheels, shafts to a yoke.
    axle_z, r = 0.55, 0.55
    bed = Vector((0, 1.05, axle_z + 0.25))
    rig.bone("cart_body", None, bed, bed + Vector((0, 0.6, 0)), radius=0.4)
    rig.bone("wheel_l", "cart_body", (0.72, 1.05, axle_z))
    rig.bone("wheel_r", "cart_body", (-0.72, 1.05, axle_z))
    cart = [k.box((1.2, 1.7, 0.08), p["planks"], loc=bed, bevel=0.015)]
    for s in (-1, 1):
        cart.append(k.box((0.06, 1.7, 0.32), p["planks"], loc=bed + Vector((s * 0.6, 0, 0.18)), bevel=0.01))
    cart.append(k.box((1.2, 0.06, 0.32), p["planks"], loc=bed + Vector((0, 0.85, 0.18)), bevel=0.01))
    cart.append(k.rod((-0.8, 1.05, axle_z), (0.8, 1.05, axle_z), 0.045, p["dark_wood"]))
    for s in (-1, 1):
        cart.append(k.rod(bed + Vector((s * 0.45, -0.8, -0.02)), A["chest"] + Vector((s * 0.3, 0.2, 0.1)), 0.035, p["dark_wood"], n=5))
    cart.append(k.rod(A["chest"] + Vector((-0.42, 0.05, 0.62)), A["chest"] + Vector((0.42, 0.05, 0.62)), 0.05, p["dark_wood"], n=6))
    # The load: sacks, a barrel and a crate under a cloth in the team colour.
    sack = k.mat("sack", k.srgb(196, 174, 132), 0.95, pattern="weave", scale=20, amount=0.3)
    for x, y in ((-0.28, -0.45), (0.28, -0.4), (-0.25, 0.1)):
        cart.append(k.ball(0.26, sack, loc=bed + Vector((x, y, 0.28)), scale=(1.0, 1.2, 0.8)))
    cart += ships.barrel(k, p, tuple(bed + Vector((0.28, 0.25, 0.05))), r=0.22, h=0.5)
    cart += ships.crate(k, p, tuple(bed + Vector((0.0, 0.6, 0.05))), size=0.42)
    cloth = k.sheet("cart_cloth", 1.1, 0.9, p["team"], nx=4, ny=4, bulge=0.25, thickness=0.03)
    cloth.rotation_euler = (math.pi / 2, 0, 0)
    cloth.location = bed + Vector((0, 0.35, 0.5))
    cart.append(cloth)
    k.bind(cart, "cart_body")
    parts += cart
    parts += engines.wheel(k, p, (0.72, 1.05, axle_z), r, "wheel_l", spokes=6)
    parts += engines.wheel(k, p, (-0.72, 1.05, axle_z), r, "wheel_r", spokes=6)
    # The driver walks beside the ox with a goad.
    body, J = people.figure(k, p, rig, at=(-0.85, -1.3, 0), arms=("hold", "hang"), stride=0.18, outfit={"tunic": k.mat("drover", k.srgb(120, 100, 72), 0.9, pattern="weave", scale=20, amount=0.3), "sleeves": "long", "skirt": 0.3, "beard": True})
    parts += body
    parts += people.helmet(k, p, J, "hat", color=k.mat("straw_hat", k.srgb(200, 170, 100), 0.9, pattern="weave", scale=30, amount=0.3))
    wr = J["wrist_r"]
    parts += people.tool(k, p, wr + Vector((0, 0.1, -0.8)), wr + Vector((0.1, -0.5, 0.8)), "staff", bone="arm_r_lo")
    return parts, {"kind": "unit", "scale": 0.16, "rig": rig}
