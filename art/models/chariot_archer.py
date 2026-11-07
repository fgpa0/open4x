"""Chariot Archer: a light two-wheeled chariot drawn by a horse, an archer
aboard drawing his bow."""

import math

import beasts
import engines
import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("bow")
    coat = k.mat("dun", k.srgb(170, 132, 84), 0.78, pattern="noise", scale=5, amount=0.3, color2=k.srgb(120, 90, 56))
    p["mane"] = k.mat("dark_mane", k.srgb(46, 36, 28), 0.7, pattern="hair", scale=10, amount=0.3)
    parts, A = beasts.quadruped(k, p, "horse", coat, rig=rig, prefix="horse_", points=p["mane"], at=(0, -1.15, 0))
    parts += beasts.tack(k, p, A, prefix="horse_", saddle=False)
    # The chariot: a car on an axle with two big wheels, a pole to the yoke.
    axle_z, r = 0.62, 0.62
    car = Vector((0, 0.95, axle_z + 0.18))
    rig.bone("cart_body", None, car, car + Vector((0, 0.5, 0)), radius=0.3)
    rig.bone("wheel_l", "cart_body", (0.62, 1.0, axle_z))
    rig.bone("wheel_r", "cart_body", (-0.62, 1.0, axle_z))
    floor = k.box((1.0, 0.75, 0.06), p["planks"], loc=car, bevel=0.01)
    front = k.loft("car_front", [[(math.sin(a) * 0.5, 0.95 - math.cos(a) * 0.38, car.z + z) for a in [(-1.4 + 2.8 * i / 12) for i in range(13)]] for z in (0.02, 0.85)], p["team_paint"], closed=False, cap=False)
    mod = front.modifiers.new("s", "SOLIDIFY")
    mod.thickness = 0.03
    rail = k.loft("car_rail", [[(math.sin(a) * 0.5, 0.95 - math.cos(a) * 0.38, car.z + 0.86) for a in [(-1.4 + 2.8 * i / 12) for i in range(13)]], [(math.sin(a) * 0.53, 0.95 - math.cos(a) * 0.41, car.z + 0.86) for a in [(-1.4 + 2.8 * i / 12) for i in range(13)]]], p["bronze"], closed=False, cap=False)
    axle = k.rod((-0.7, 1.0, axle_z), (0.7, 1.0, axle_z), 0.04, p["dark_wood"])
    pole = k.rod((0, 0.7, car.z - 0.02), A["chest"] + Vector((0, 0.1, 0.35)), 0.04, p["dark_wood"])
    yoke = k.rod(A["chest"] + Vector((-0.35, 0.15, 0.5)), A["chest"] + Vector((0.35, 0.15, 0.5)), 0.04, p["dark_wood"])
    k.bind([floor, front, rail, axle, pole, yoke], "cart_body")
    parts += [floor, front, rail, axle, pole, yoke]
    parts += engines.wheel(k, p, (0.62, 1.0, axle_z), r, "wheel_l")
    parts += engines.wheel(k, p, (-0.62, 1.0, axle_z), r, "wheel_r")
    # The archer stands in the car.
    body, J = people.figure(k, p, rig, parent="cart_body", at=(0, 1.0, car.z + 0.03), yaw=0.35, arms=("draw", "reach"), outfit={"sleeves": "long", "skirt": 0.6, "beard": True})
    parts += body
    parts += people.helmet(k, p, J, "cap")
    parts += people.bow(k, p, J["hand_l"], (0, 0.05, 1), J["fwd"], height=1.2, recurve=True, bone="arm_l_lo")
    parts += people.quiver(k, p, Vector((0.45, 1.25, car.z + 0.5)), "cart_body")
    return parts, {"kind": "unit", "scale": 0.155, "rig": rig}
