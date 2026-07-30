"""Renders QA stills of every baked clip in assets-src/player.blend so pose
signs can be judged without launching the game: each action at mid- and
end-fraction from a side camera (character's front faces image-LEFT), plus a
top view for the horizontal-sweep clips.

Run: blender --background assets-src/player.blend --python tools/render_pose_sheet.py -- <outdir>
"""
import os
import sys

import bpy

argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
OUT = argv[0] if argv else "/tmp/pose-sheet"
os.makedirs(OUT, exist_ok=True)

FPS = 24
# (clip, [fractions]) — mid-pose and near-end pose.
SHOTS = {
    "Idle": [0.5],
    "WindUp": [0.5, 0.95],
    "ThrowRelease": [0.5, 0.95],
    "RunCycle": [0.25, 0.75],
    "ScoopBall": [0.5],
    "GloveUp": [0.95],
    "CatcherCrouch": [0.5],
    "Dive": [0.95],
    "Slide": [0.95],
    "BattingStance": [0.5],
    "BatterSwing": [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9],
}
TOP_VIEW = {"BatterSwing", "RunCycle", "BattingStance"}

rig = bpy.data.objects["PlayerRig"]

# Isolate the active action: NLA strips would otherwise stack under it.
for track in rig.animation_data.nla_tracks:
    track.mute = True

scene = bpy.context.scene
scene.render.engine = "CYCLES"
scene.cycles.samples = 16
scene.render.resolution_x = 320
scene.render.resolution_y = 320
scene.render.film_transparent = False
scene.world = scene.world or bpy.data.worlds.new("World")
scene.world.use_nodes = True
scene.world.node_tree.nodes["Background"].inputs[0].default_value = (0.9, 0.9, 0.95, 1)

sun = bpy.data.objects.new("QASun", bpy.data.lights.new("QASun", "SUN"))
sun.rotation_euler = (0.9, 0.2, 0.4)
scene.collection.objects.link(sun)


def add_cam(name, location, rotation):
    cam = bpy.data.objects.new(name, bpy.data.cameras.new(name))
    cam.data.type = "ORTHO"
    cam.data.ortho_scale = 3.2
    cam.location = location
    cam.rotation_euler = rotation
    scene.collection.objects.link(cam)
    return cam


import math

# Side: on +X looking -X, Z up → character's front (-Y) is image-left.
side = add_cam("QASide", (4, 0, 1.0), (math.radians(90), 0, math.radians(90)))
# Top: above looking down, front (-Y) is image-up.
top = add_cam("QATop", (0, 0, 4), (0, 0, math.radians(180)))

for name, fractions in SHOTS.items():
    # A bone with no fcurve in this action would otherwise keep whatever
    # pose Python last wrote to it while baking (bake_clips resets before
    # each action too, but that reset is invisible here since we're not
    # re-running the bake) — reset first so untouched bones read as rest.
    for pb in rig.pose.bones:
        pb.rotation_euler = (0, 0, 0)
        pb.location = (0, 0, 0)
    action = bpy.data.actions[name]
    rig.animation_data.action = action
    seconds = (action.frame_range[1] - 1) / FPS
    for f in fractions:
        frame = 1 + round(f * seconds * FPS)
        scene.frame_set(frame)
        cams = [side] + ([top] if name in TOP_VIEW else [])
        for cam in cams:
            scene.camera = cam
            tag = "top" if cam is top else "side"
            scene.render.filepath = os.path.join(OUT, f"{name}_{int(f*100):02d}_{tag}.png")
            bpy.ops.render.render(write_still=True)
            print(f"rendered {scene.render.filepath}")

print(f"pose sheet in {OUT}")
