"""Builds the game's models with Blender.

    blender -b --factory-startup -P art/build.py -- [name ...] [--no-preview]

Each art/models/<name>.py has `build(kit) -> (objects, options)` for one
model, or `(dict of name -> objects, options)` with `kind="props"` for a
library of props exported as named nodes. With no names, every model is
built. Models land in crates/client/assets/models, previews (the game's
camera angle) in target/art.
"""

import importlib.util
import os
import sys
import traceback

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "lib"))
import kit  # noqa: E402

MODELS = kit.MODELS

args = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
preview = "--no-preview" not in args
names = [a for a in args if not a.startswith("--")]
models = os.path.join(HERE, "models")
if not names:
    names = sorted(f[:-3] for f in os.listdir(models) if f.endswith(".py") and not f.startswith("_"))

failed = []
for name in names:
    try:
        spec = importlib.util.spec_from_file_location(name, os.path.join(models, f"{name}.py"))
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        kit.reset()
        kit._mats.clear()
        # Studies (names starting with "_") never reach the game.
        kit.MODELS = kit.PREVIEWS if name.startswith("_") else MODELS
        built, opts = module.build(kit)
        if opts.get("kind") == "props":
            opts = {k: v for k, v in opts.items() if k != "kind"}
            out = kit.finish_props(built, name, **opts)
        else:
            out = kit.finish(built, name, **opts)
        if preview:
            kit.preview(out, name)
    except Exception:
        traceback.print_exc()
        failed.append(name)
if failed:
    print(f"[art] FAILED: {' '.join(failed)}")
