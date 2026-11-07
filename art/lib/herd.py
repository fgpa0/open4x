"""Animals that graze and wander on their tiles (resources and pastures):
one rigged quadruped per model, at the same size as the static props in
works.py, so the game's animator can walk them and lower their heads."""

import beasts
import people

# Props' size factor in works.py (BEAST), per kind.
BEAST = 0.021
SIZE = {"cow": 1.0, "cow_pied": 1.0, "sheep": 1.7, "horse": 1.0, "deer": 1.25, "fox": 0.9, "elephant": 0.75}


def coats(k, p):
    return {
        "cow": (k.mat("cow_brown", k.srgb(96, 70, 52), 0.85, pattern="noise", scale=3, amount=0.3), k.mat("cow_dark", k.srgb(60, 46, 38), 0.8)),
        "cow_pied": (k.mat("cow_pied", k.srgb(236, 230, 220), 0.85, pattern="noise", scale=1.2, amount=0.9, color2=k.srgb(40, 34, 30)), k.mat("cow_dark2", k.srgb(48, 40, 36), 0.8)),
        "sheep": (k.mat("sheep_face", k.srgb(60, 52, 46), 0.8), None),
        "horse": (k.mat("bay", k.srgb(128, 76, 42), 0.78, pattern="noise", scale=5, amount=0.35, color2=k.srgb(84, 46, 26)), p["mane"]),
        "deer": (k.mat("deer", k.srgb(156, 110, 66), 0.85, pattern="noise", scale=4, amount=0.3), None),
        "fox": (k.mat("fox", k.srgb(196, 98, 42), 0.85, pattern="noise", scale=6, amount=0.25), None),
        "elephant": (k.mat("elephant", k.srgb(132, 128, 124), 0.9, pattern="noise", scale=2, amount=0.3), None),
    }


def build_animal(k, name):
    p = people.extend_palette(k, k.palette())
    p["horn_light"] = k.mat("horn_light", k.srgb(226, 214, 186), 0.5)
    p["wool"] = k.mat("wool", k.srgb(236, 230, 214), 0.95, pattern="noise", scale=30, amount=0.25)
    p["mane"] = k.mat("points", k.srgb(46, 34, 26), 0.7, pattern="hair", scale=10, amount=0.3)
    coat, points = coats(k, p)[name]
    kind = "cow" if name == "cow_pied" else name
    rig = k.Rig("none")
    parts, _ = beasts.quadruped(k, p, kind, coat, rig=rig, prefix="a_", points=points, faces=1400 if kind != "elephant" else 2000)
    return parts, {"kind": "unit", "scale": BEAST * SIZE[name], "rig": rig, "res": 256}
