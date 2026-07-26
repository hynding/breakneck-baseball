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
    # Knob coincides with the right hand (LowerArm.R's own tail) — a fixed
    # offset here would put the knob that far from the hand in EVERY pose,
    # not just at rest — with the barrel rising back over the right
    # shoulder. BattingStance/BatterSwing raise the whole arm+bat assembly
    # to chest height by rotating the arm chain; see the CLIPS entries below.
    "Bat":        ((-0.24, -0.01, 0.885), (-0.18, 0.24, 1.55), "LowerArm.R"),
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
]


def _bat_mesh_part():
    """Bat mesh part computed straight from the `Bat` bone's own head/tail so
    a rest reposition never needs a hand-recomputed rotation: a cylinder
    (local +Z along its length) centred on the bone and rotated to track it.
    """
    import mathutils

    head, tail, _ = BONES["Bat"]
    head, tail = mathutils.Vector(head), mathutils.Vector(tail)
    direction = tail - head
    midpoint = (head + tail) / 2
    rot = direction.to_track_quat("Z", "Y").to_euler()
    return (
        "cylinder",
        tuple(midpoint),
        (0.032, 0.032, direction.length / 2),
        "Bat",
        "Bat",
        (rot.x, rot.y, rot.z),
    )


PARTS.append(_bat_mesh_part())

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
    # Held right-handed stance: both arms raised, bat up off the right
    # shoulder, knees softened, torso coiled toward the catcher — a subtle
    # breathing sway (same pattern as CatcherCrouch) so the hold doesn't read
    # as frozen. BatterSwing below starts from this same arm/leg posture.
    # Both UpperArm channels below (rx forward-up pitch AND rz sideways
    # swing — see the probe notes in this file's header) were grid-searched
    # (a throwaway probe script, not committed) against LowerArm.L's tail vs
    # Bat's head world position: this combo lands the hands within ~1 cm of
    # the knob, well inside the brief's ~10 cm arcade-fidelity bar.
    "BattingStance": (1.2, True, {
        "UpperArm.R": {
            "rx": [(0, -0.95), (0.25, -0.92), (0.5, -0.95), (0.75, -0.98), (1, -0.95)],
            "rz": [(0, -0.8), (1, -0.8)],
        },
        "UpperArm.L": {
            "rx": [(0, -0.95), (0.25, -0.92), (0.5, -0.95), (0.75, -0.98), (1, -0.95)],
            "rz": [(0, 0.85), (1, 0.85)],
        },
        # The bat's own local rx tilts the barrel back up off the parent
        # forearm's raised angle (without it the whole assembly reads flat
        # and horizontal, not "up over the shoulder").
        "Bat": {"rx": [(0, 0.6), (1, 0.6)]},
        "UpperLeg.L": {"rx": [(0, 0.3), (1, 0.3)]},
        "UpperLeg.R": {"rx": [(0, 0.3), (1, 0.3)]},
        "LowerLeg.L": {"rx": [(0, -0.3), (1, -0.3)]},
        "LowerLeg.R": {"rx": [(0, -0.3), (1, -0.3)]},
        "Spine": {"ry": [(0, 0.25), (0.25, 0.29), (0.5, 0.25), (0.75, 0.21), (1, 0.25)]},
    }),
    # From the BattingStance hold (frac-0 keys match its constant values so
    # the driver's cross-fade has nothing to fight): hips/torso lead the
    # turn, both arms hold a LEVEL rx through the zone (0 to ~0.6 — the bat
    # travels flat, not in an overhead chop) while rz drives the actual
    # sweep, then rx lifts in the last stretch for a high follow-through.
    # End-pose values (rx=-2.2, rz=+-0.8) are grid-searched the same way as
    # the stance: hands stay within ~1 cm of the knob at the high finish
    # too, not just at the start.
    "BatterSwing": (0.42, False, {
        "Hips": {"ry": [(0, 0), (0.7, 0.55), (1, 0.8)]},
        "Spine": {"ry": [(0, 0.25), (0.7, 0.7), (1, 0.95)]},
        "UpperArm.R": {
            "rx": [(0, -0.95), (0.7, -0.95), (1, -2.2)],
            "rz": [(0, -0.8), (0.7, 0.8), (1, 0.8)],
        },
        "UpperArm.L": {
            "rx": [(0, -0.95), (0.7, -0.95), (1, -2.2)],
            "rz": [(0, 0.85), (0.7, -0.8), (1, -0.8)],
        },
        "Bat": {"rx": [(0, 0.6), (1, 0.4)]},
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
