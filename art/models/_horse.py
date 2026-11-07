"""Study: a horse on its own, for working on the animal's anatomy."""

import beasts
import people


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("none")
    coat = k.mat("bay", k.srgb(128, 76, 42), 0.78, pattern="noise", scale=5, amount=0.35, color2=k.srgb(84, 46, 26))
    p["mane"] = k.mat("points", k.srgb(46, 34, 26), 0.7, pattern="hair", scale=10, amount=0.3)
    parts, A = beasts.quadruped(k, p, "horse", coat, rig=rig, points=p["mane"], prefix="horse_", faces=4500)
    return parts, {"kind": "unit", "scale": 0.2, "rig": rig}
