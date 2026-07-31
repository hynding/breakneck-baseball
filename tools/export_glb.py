"""Exports assets-src/player.blend -> src/game/models/player.glb with pinned
settings (NLA tracks -> one named animation per clip). Never export by hand —
this script IS the export settings.

Run: blender --background assets-src/player.blend --python tools/export_glb.py
"""
import os

import bpy

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
OUT = os.path.join(ROOT, "src", "game", "models", "player.glb")

os.makedirs(os.path.dirname(OUT), exist_ok=True)
bpy.ops.export_scene.gltf(
    filepath=OUT,
    export_format="GLB",
    export_yup=True,                    # Blender Z-up -> glTF Y-up (-Y fwd -> +Z fwd)
    export_animation_mode="NLA_TRACKS", # one animation per NLA track, named
    export_skins=True,
    export_materials="EXPORT",
    export_apply=False,
)
print(f"wrote {OUT}")
