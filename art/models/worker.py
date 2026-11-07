"""Worker: a labourer in a straw hat with a pick on the shoulder."""

import people
from mathutils import Vector


def build(k):
    p = people.extend_palette(k, k.palette())
    rig = k.Rig("none")
    parts, J = people.figure(k, p, rig, arms=("shoulder", "hang"), stride=0.14, outfit={"sleeves": "long", "skirt": 0.78, "legs": "trousers"})
    parts += people.helmet(k, p, J, "straw")
    wr = J["wrist_r"]
    parts += people.tool(k, p, wr + Vector((0.0, -0.12, -0.12)), wr + Vector((0.05, 0.75, 0.28)), "pick", bone="arm_r_lo")
    apron = k.box((0.3, 0.03, 0.42), k.mat("apron", k.srgb(170, 150, 118), 0.9, pattern="weave", scale=30, amount=0.2), loc=J["hips"] + J["fwd"] * 0.14 + Vector((0, 0, -0.1)))
    k.blend(apron, ["hips", "leg_r_up", "leg_l_up"])
    parts.append(apron)
    return parts, {"kind": "unit", "scale": 0.2, "rig": rig}
