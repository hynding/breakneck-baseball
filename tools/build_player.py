"""Builds assets-src/player.blend from scratch: a low-poly skinned baseball
player with every clip in the game's CLIP_TABLE, rigid-weighted per part
(deterministic — no auto-weight solve).

Conventions (mirrors the Rust contract in src/game/model_assets.rs):
  * 1 unit = 1 m, feet at origin, 1.85 m tall, faces -Y in Blender
    (glTF Y-up export turns -Y-forward into the game's +Z-forward).
  * .L at +X / .R at -X (matches the game rig's ArmL at +0.36 X).
  * Clip lengths exactly match AnimClip::duration() at 24 fps.
  * Limb keyframe angles are lifted straight from the old procedural
    limb_pose()/root_drop()/root_pitch() in src/game/animation.rs; Hips
    pitch signs are FLIPPED vs the game (Blender bone-local +rx leans the
    -Y-facing character backward).

Run: blender --background --python tools/build_player.py
"""
import math
import os

import bpy

FPS = 24
ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
OUT = os.path.join(ROOT, "assets-src", "player.blend")

# name: (head, tail, parent)
BONES = {
    "Hips":       ((0, 0, 0.90), (0, 0, 1.15), None),
    "Spine":      ((0, 0, 1.15), (0, 0, 1.50), "Hips"),
    "Head":       ((0, 0, 1.50), (0, 0, 1.85), "Spine"),
    "UpperArm.L": ((0.24, 0, 1.45), (0.24, 0, 1.15), "Spine"),
    "LowerArm.L": ((0.24, 0, 1.15), (0.24, 0, 0.88), "UpperArm.L"),
    "UpperArm.R": ((-0.24, 0, 1.45), (-0.24, 0, 1.15), "Spine"),
    "LowerArm.R": ((-0.24, 0, 1.15), (-0.24, 0, 0.88), "UpperArm.R"),
    "UpperLeg.L": ((0.10, 0, 0.90), (0.10, 0, 0.50), "Hips"),
    "LowerLeg.L": ((0.10, 0, 0.50), (0.10, 0, 0.05), "UpperLeg.L"),
    "UpperLeg.R": ((-0.10, 0, 0.90), (-0.10, 0, 0.50), "Hips"),
    "LowerLeg.R": ((-0.10, 0, 0.50), (-0.10, 0, 0.05), "UpperLeg.R"),
    "Bat":        ((-0.30, 0, 0.88), (-0.30, -0.35, 1.55), "LowerArm.R"),
}

# Base colours are placeholders — the game re-tints JerseyBody/Cap per team.
MATERIALS = {
    "JerseyBody": (0.80, 0.80, 0.85, 1.0),
    "Skin":       (0.87, 0.67, 0.50, 1.0),
    "Cap":        (0.15, 0.20, 0.50, 1.0),
    "Bat":        (0.72, 0.50, 0.28, 1.0),
}

# (primitive, location, half-dims/radii, material, bone, rotation_euler)
PARTS = [
    ("cube",      (0, 0, 0.975),        (0.15, 0.11, 0.075), "JerseyBody", "Hips",       (0, 0, 0)),
    ("cube",      (0, 0, 1.30),         (0.18, 0.12, 0.22),  "JerseyBody", "Spine",      (0, 0, 0)),
    ("uv_sphere", (0, 0, 1.66),         (0.16, 0.16, 0.16),  "Skin",       "Head",       (0, 0, 0)),
    ("cylinder",  (0, 0, 1.82),         (0.17, 0.17, 0.045), "Cap",        "Head",       (0, 0, 0)),
    ("cube",      (0, -0.16, 1.80),     (0.13, 0.09, 0.015), "Cap",        "Head",       (0, 0, 0)),
    ("cylinder",  (0.24, 0, 1.30),      (0.055, 0.055, 0.16),"JerseyBody", "UpperArm.L", (0, 0, 0)),
    ("cylinder",  (0.24, 0, 1.01),      (0.05, 0.05, 0.14),  "Skin",       "LowerArm.L", (0, 0, 0)),
    ("cylinder",  (-0.24, 0, 1.30),     (0.055, 0.055, 0.16),"JerseyBody", "UpperArm.R", (0, 0, 0)),
    ("cylinder",  (-0.24, 0, 1.01),     (0.05, 0.05, 0.14),  "Skin",       "LowerArm.R", (0, 0, 0)),
    ("cylinder",  (0.10, 0, 0.70),      (0.07, 0.07, 0.21),  "JerseyBody", "UpperLeg.L", (0, 0, 0)),
    ("cylinder",  (0.10, 0, 0.27),      (0.065, 0.065, 0.23),"JerseyBody", "LowerLeg.L", (0, 0, 0)),
    ("cylinder",  (-0.10, 0, 0.70),     (0.07, 0.07, 0.21),  "JerseyBody", "UpperLeg.R", (0, 0, 0)),
    ("cylinder",  (-0.10, 0, 0.27),     (0.065, 0.065, 0.23),"JerseyBody", "LowerLeg.R", (0, 0, 0)),
    # Bat roughly along its bone (tilted forward-up out of the right hand).
    ("cylinder",  (-0.30, -0.17, 1.21), (0.032, 0.032, 0.42),"Bat",        "Bat",        (math.radians(-27), 0, 0)),
]

TAU = 2 * math.pi
# clip: (seconds, loop, {bone: {channel: [(fraction, value), ...]}})
# channels: rx/ry/rz = pose-bone rotation_euler (radians, XYZ mode);
#           dz = Hips drop in metres (bone-local Y == world Z for Hips).
# Values mirror the old limb_pose()/root_drop()/root_pitch() tables.
CLIPS = {
    "Idle": (1.0, True, {
        "Spine": {"rx": [(0, 0), (0.5, 0.03), (1, 0)]},
    }),
    "WindUp": (0.5, False, {
        "UpperArm.R": {"rx": [(0, 0), (1, -2.6)]},
        "UpperArm.L": {"rx": [(0, 0), (1, -1.2)]},
        "UpperLeg.L": {"rx": [(0, 0), (1, 1.0)]},
    }),
    "ThrowRelease": (0.22, False, {
        "UpperArm.R": {"rx": [(0, -2.6), (1, 0.8)]},
        "UpperArm.L": {"rx": [(0, -1.2), (1, 0.0)]},
        "UpperLeg.L": {"rx": [(0, 1.0), (1, 0.0)]},
    }),
    "RunCycle": (0.45, True, {
        "UpperArm.L": {"rx": [(0, 0), (0.25, 0.9), (0.5, 0), (0.75, -0.9), (1, 0)]},
        "UpperLeg.R": {"rx": [(0, 0), (0.25, 0.9), (0.5, 0), (0.75, -0.9), (1, 0)]},
        "UpperArm.R": {"rx": [(0, 0), (0.25, -0.9), (0.5, 0), (0.75, 0.9), (1, 0)]},
        "UpperLeg.L": {"rx": [(0, 0), (0.25, -0.9), (0.5, 0), (0.75, 0.9), (1, 0)]},
        "LowerLeg.L": {"rx": [(0, 0.4), (1, 0.4)]},
        "LowerLeg.R": {"rx": [(0, 0.4), (1, 0.4)]},
    }),
    "ScoopBall": (0.32, False, {
        "UpperArm.L": {"rx": [(0, 0), (0.5, 1.6), (1, 0)]},
        "UpperArm.R": {"rx": [(0, 0), (0.5, 1.6), (1, 0)]},
        "UpperLeg.L": {"rx": [(0, 0), (0.5, 0.4), (1, 0)]},
        "UpperLeg.R": {"rx": [(0, 0), (0.5, 0.4), (1, 0)]},
        "Hips": {"dz": [(0, 0), (0.5, -0.26), (1, 0)]},
    }),
    "GloveUp": (0.28, False, {
        "UpperArm.L": {"rx": [(0, 0), (1, -2.9)]},
    }),
    "CatcherCrouch": (1.2, True, {
        "UpperLeg.L": {"rx": [(0, 1.35), (1, 1.35)]},
        "UpperLeg.R": {"rx": [(0, 1.35), (1, 1.35)]},
        "LowerLeg.L": {"rx": [(0, -1.9), (1, -1.9)]},
        "LowerLeg.R": {"rx": [(0, -1.9), (1, -1.9)]},
        "UpperArm.L": {"rx": [(0, -1.15), (0.25, -1.11), (0.5, -1.15), (0.75, -1.19), (1, -1.15)]},
        "UpperArm.R": {"rx": [(0, -0.55), (0.25, -0.59), (0.5, -0.55), (0.75, -0.51), (1, -0.55)]},
        "Hips": {"dz": [(0, -0.22), (1, -0.22)]},
    }),
    "Dive": (0.5, False, {
        "UpperArm.L": {"rx": [(0, 0), (1, -2.6)]},
        "UpperArm.R": {"rx": [(0, 0), (1, -2.6)]},
        "UpperLeg.L": {"rx": [(0, 0), (1, 0.5)]},
        "UpperLeg.R": {"rx": [(0, 0), (1, 0.5)]},
        # game root_pitch +1.25 face-first => Blender Hips rx NEGATIVE
        "Hips": {"dz": [(0, 0), (1, -0.38)], "rx": [(0, 0), (1, -1.25)]},
    }),
    "Slide": (0.6, False, {
        "UpperLeg.L": {"rx": [(0, 0), (1, -1.2)]},
        "UpperLeg.R": {"rx": [(0, 0), (1, -1.2)]},
        "UpperArm.L": {"rx": [(0, 0), (1, -0.7)]},
        "UpperArm.R": {"rx": [(0, 0), (1, -0.7)]},
        # game root_pitch -0.85 lean-back => Blender Hips rx POSITIVE
        "Hips": {"dz": [(0, 0), (1, -0.30)], "rx": [(0, 0), (1, 0.85)]},
    }),
    # Horizontal sweep = bone-local rz for a straight-down arm bone.
    "BatterSwing": (0.42, False, {
        "UpperArm.L": {"rx": [(0, -0.5), (1, -0.5)], "rz": [(0, 0), (0.5, 1.9), (1, 0)]},
        "UpperArm.R": {"rx": [(0, -0.5), (1, -0.5)], "rz": [(0, 0), (0.5, 1.9), (1, 0)]},
        "Bat": {"rz": [(0, -1.7), (0.4, 1.7), (1, -0.5)]},
    }),
}

CHANNEL = {"rx": ("rotation_euler", 0), "ry": ("rotation_euler", 1),
           "rz": ("rotation_euler", 2), "dz": ("location", 1)}  # dz: bone-local Y


def clean_scene():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.context.scene.render.fps = FPS


def make_materials():
    mats = {}
    for name, rgba in MATERIALS.items():
        m = bpy.data.materials.new(name)
        m.use_nodes = True
        m.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = rgba
        m.node_tree.nodes["Principled BSDF"].inputs["Roughness"].default_value = 0.8
        mats[name] = m
    return mats


def make_armature():
    arm = bpy.data.armatures.new("PlayerArmature")
    obj = bpy.data.objects.new("PlayerRig", arm)
    bpy.context.collection.objects.link(obj)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.mode_set(mode="EDIT")
    ebs = {}
    for name, (head, tail, parent) in BONES.items():
        eb = arm.edit_bones.new(name)
        eb.head, eb.tail = head, tail
        if parent:
            eb.parent = ebs[parent]
        ebs[name] = eb
    bpy.ops.object.mode_set(mode="OBJECT")
    return obj


def make_body(mats):
    parts = []
    for prim, loc, dims, mat, bone, rot in PARTS:
        if prim == "cube":
            bpy.ops.mesh.primitive_cube_add(location=loc)
            bpy.context.object.scale = dims
        elif prim == "uv_sphere":
            bpy.ops.mesh.primitive_uv_sphere_add(location=loc, radius=dims[0],
                                                 segments=12, ring_count=8)
        else:
            bpy.ops.mesh.primitive_cylinder_add(location=loc, radius=dims[0],
                                                depth=dims[2] * 2, vertices=10)
        ob = bpy.context.object
        ob.rotation_euler = rot
        bpy.ops.object.transform_apply(location=False, rotation=True, scale=True)
        ob.data.materials.append(mats[mat])
        vg = ob.vertex_groups.new(name=bone)
        vg.add(range(len(ob.data.vertices)), 1.0, "REPLACE")  # rigid weight
        parts.append(ob)
    for ob in parts:
        ob.select_set(True)
    bpy.context.view_layer.objects.active = parts[0]
    bpy.ops.object.join()
    body = bpy.context.object
    body.name = "PlayerBody"
    return body


def skin(body, rig):
    body.parent = rig
    mod = body.modifiers.new("Armature", "ARMATURE")
    mod.object = rig


def bake_clips(rig):
    rig.animation_data_create()
    for pb in rig.pose.bones:
        pb.rotation_mode = "XYZ"
    for name, (seconds, loop, channels) in CLIPS.items():
        action = bpy.data.actions.new(name)
        rig.animation_data.action = action
        for pb in rig.pose.bones:  # reset pose between actions
            pb.rotation_euler = (0, 0, 0)
            pb.location = (0, 0, 0)
        last = 1 + round(seconds * FPS)
        for bone, chans in channels.items():
            pb = rig.pose.bones[bone]
            for chan, keys in chans.items():
                attr, idx = CHANNEL[chan]
                for frac, value in keys:
                    getattr(pb, attr)[idx] = value
                    pb.keyframe_insert(attr, index=idx, frame=1 + round(frac * (last - 1)))
        action.use_fake_user = True
        track = rig.animation_data.nla_tracks.new()
        track.name = name
        track.strips.new(name, start=1, action=action)
    rig.animation_data.action = None


def main():
    clean_scene()
    mats = make_materials()
    rig = make_armature()
    body = make_body(mats)
    skin(body, rig)
    bake_clips(rig)
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=OUT)
    print(f"wrote {OUT}")


main()
