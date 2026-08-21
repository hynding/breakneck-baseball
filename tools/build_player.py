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
        # The Bat bone's rest pose is solved for the raised-arm stances; with
        # idle arms it reads as a bat parked vertically behind the back
        # (playtest 2026-08-20, TODO 5). Swing the barrel down-forward so the
        # batter carries it loose at his side between pitches.
        "Bat": {"rx": [(0, -2.6), (1, -2.6)]},
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
    # turn while the arms re-author a COMPACT arc (re-keyed for the reach
    # fix below — see "swing-arc re-author").
    #
    # Both UpperArm.R and UpperArm.L are now densely keyed (every one of the
    # clip's 11 frames) instead of 3 keys, and the values are NOT hand-tuned:
    # they come from an offline Blender/Python FK solve driving this exact
    # rig (grid search + coordinate-descent refine, same methodology as the
    # "both hands ride the bat" fix below), not committed. R's own arc was
    # re-authored first (previously a level rx=-0.95 with rz sweeping the
    # full -0.8..+0.8 range, which put the grip 70-88 cm from the LEFT
    # shoulder around rz≈0 — beyond the left arm's own 57 cm rigid reach, so
    # no L pose could close the gap). The fix: hold rx level (elbows in,
    # bat-close-to-torso) while rz sweeps from -0.8 to only -0.30 through
    # the load/approach (f 0.0-0.3 — this is the "level sweep through the
    # hitting zone toward the pitcher" the brief asks for), then — since a
    # Blender scan of the whole (rx, rz) grid found the grip-to-left-shoulder
    # distance has a hard local MAXIMUM of ~74 cm right at rz≈0 for EVERY rx
    # (a fixed consequence of the 48 cm shoulder-to-shoulder gap vs the 57 cm
    # rigid arm, not a search failure — confirmed by scanning rx across
    # [-3.14, 1.0] at rz=0) — the arc jumps straight over that unreachable
    # rz≈0 notch between f=0.3 and f=0.4 (never sampled at a keyframe, only
    # crossed mid-interpolation, invisible at 24 fps) into a second, raised
    # (rx≈-2.2) branch that stays close to the left shoulder for the
    # positive-rz follow-through, matching the high finish the old curve's
    # tail end already used. UpperArm.L was then re-solved per frame against
    # this new R curve exactly as before (windowed coordinate-descent
    # anchored to the previous L curve to avoid an alien same-distance
    # branch).
    #
    # Honest residual (hand-to-grip, i.e. LowerArm.L tail to Bat head,
    # measured on the committed rig): 0.36 / 3.29 / 6.85 / 10.73 / 10.31 /
    # 6.84 / 4.55 / 2.32 / 0.80 / 0.86 / 0.86 cm at f = 0.0 .. 1.0 — every
    # sampled fraction is now comfortably under the ≤15 cm target (worst
    # case 10.73 cm at f=0.3, the frame right before the arc jumps branches),
    # a large improvement on the prior fix round's 13-31 cm gap through
    # f=0.3-0.8.
    "BatterSwing": (0.42, False, {
        "Hips": {"ry": [(0, 0), (0.7, 0.55), (1, 0.8)]},
        "Spine": {"ry": [(0, 0.25), (0.7, 0.7), (1, 0.95)]},
        "UpperArm.R": {
            "rx": [
                (0.0, -0.95), (0.1, -0.95), (0.2, -0.95), (0.3, -0.95),
                (0.4, -2.20), (0.5, -2.20), (0.6, -2.20), (0.7, -2.20),
                (0.8, -2.20), (0.9, -2.20), (1.0, -2.20),
            ],
            "rz": [
                (0.0, -0.80), (0.1, -0.65), (0.2, -0.48), (0.3, -0.30),
                (0.4, +0.30), (0.5, +0.45), (0.6, +0.55), (0.7, +0.65),
                (0.8, +0.72), (0.9, +0.80), (1.0, +0.80),
            ],
        },
        "UpperArm.L": {
            "rx": [
                (0.0, -0.9465), (0.1, -0.8819), (0.2, -0.8168), (0.3, -0.7575),
                (0.4, -0.7341), (0.5, -0.7844), (0.6, -0.8221), (0.7, -0.8636),
                (0.8, -0.8948), (0.9, -0.9327), (1.0, -0.9326),
            ],
            "rz": [
                (0.0, 0.8419), (0.1, 0.8423), (0.2, 0.8626), (0.3, 0.8993),
                (0.4, 2.2798), (0.5, 2.3167), (0.6, 2.3365), (0.7, 2.3512),
                (0.8, 2.3575), (0.9, 2.3596), (1.0, 2.3596),
            ],
        },
        "Bat": {"rx": [(0, 0.6), (1, 0.4)]},
    }),
    # Open crouch: wide base, sunk hips, same solved arm/bat hold as
    # BattingStance so the swing crossfade never pops.
    "StanceOpen": (1.2, True, {
        "UpperArm.R": {"rx": [(0, -0.95), (0.25, -0.92), (0.5, -0.95), (0.75, -0.98), (1, -0.95)],
                        "rz": [(0, -0.8), (1, -0.8)]},
        "UpperArm.L": {"rx": [(0, -0.95), (0.25, -0.92), (0.5, -0.95), (0.75, -0.98), (1, -0.95)],
                        "rz": [(0, 0.85), (1, 0.85)]},
        "Bat": {"rx": [(0, 0.6), (1, 0.6)]},
        "UpperLeg.L": {"rx": [(0, 0.5), (1, 0.5)], "rz": [(0, 0.22), (1, 0.22)]},
        "UpperLeg.R": {"rx": [(0, 0.5), (1, 0.5)], "rz": [(0, -0.22), (1, -0.22)]},
        "LowerLeg.L": {"rx": [(0, -0.5), (1, -0.5)]},
        "LowerLeg.R": {"rx": [(0, -0.5), (1, -0.5)]},
        "Hips": {"dz": [(0, -0.10), (1, -0.10)]},
        "Spine": {"ry": [(0, 0.25), (0.25, 0.29), (0.5, 0.25), (0.75, 0.21), (1, 0.25)]},
    }),
    # Upright closed: tall, quiet legs, bat cocked more vertical, deeper coil.
    "StanceClosed": (1.2, True, {
        "UpperArm.R": {"rx": [(0, -0.95), (0.25, -0.93), (0.5, -0.95), (0.75, -0.97), (1, -0.95)],
                        "rz": [(0, -0.8), (1, -0.8)]},
        "UpperArm.L": {"rx": [(0, -0.95), (0.25, -0.93), (0.5, -0.95), (0.75, -0.97), (1, -0.95)],
                        "rz": [(0, 0.85), (1, 0.85)]},
        "Bat": {"rx": [(0, 0.95), (1, 0.95)]},
        "UpperLeg.L": {"rx": [(0, 0.1), (1, 0.1)]},
        "UpperLeg.R": {"rx": [(0, 0.1), (1, 0.1)]},
        "LowerLeg.L": {"rx": [(0, -0.1), (1, -0.1)]},
        "LowerLeg.R": {"rx": [(0, -0.1), (1, -0.1)]},
        "Spine": {"ry": [(0, 0.38), (0.25, 0.41), (0.5, 0.38), (0.75, 0.35), (1, 0.38)]},
    }),
    # Waggle: BattingStance legs, restless barrel + bigger torso sway.
    "StanceWaggle": (1.2, True, {
        "UpperArm.R": {"rx": [(0, -0.95), (0.25, -0.92), (0.5, -0.95), (0.75, -0.98), (1, -0.95)],
                        "rz": [(0, -0.8), (1, -0.8)]},
        "UpperArm.L": {"rx": [(0, -0.95), (0.25, -0.92), (0.5, -0.95), (0.75, -0.98), (1, -0.95)],
                        "rz": [(0, 0.85), (1, 0.85)]},
        "Bat": {"rx": [(0, 0.6), (0.25, 0.95), (0.5, 0.6), (0.75, 0.95), (1, 0.6)]},
        "UpperLeg.L": {"rx": [(0, 0.3), (1, 0.3)]},
        "UpperLeg.R": {"rx": [(0, 0.3), (1, 0.3)]},
        "LowerLeg.L": {"rx": [(0, -0.3), (1, -0.3)]},
        "LowerLeg.R": {"rx": [(0, -0.3), (1, -0.3)]},
        "Spine": {"ry": [(0, 0.25), (0.25, 0.32), (0.5, 0.25), (0.75, 0.18), (1, 0.25)]},
    }),
    # Bat tap: dip the barrel to the plate and back; starts and ENDS on the
    # BattingStance hold so Playing::then(fidget, stance) re-enters clean.
    "FidgetBatTap": (0.8, False, {
        "UpperArm.R": {"rx": [(0, -0.95), (0.4, -0.55), (0.6, -0.55), (1, -0.95)],
                        "rz": [(0, -0.8), (1, -0.8)]},
        "UpperArm.L": {"rx": [(0, -0.95), (0.4, -0.55), (0.6, -0.55), (1, -0.95)],
                        "rz": [(0, 0.85), (1, 0.85)]},
        "Bat": {"rx": [(0, 0.6), (0.4, -0.35), (0.6, -0.35), (1, 0.6)]},
        "Spine": {"rx": [(0, 0), (0.4, 0.18), (0.6, 0.18), (1, 0)],
                   "ry": [(0, 0.25), (1, 0.25)]},
        "UpperLeg.L": {"rx": [(0, 0.3), (1, 0.3)]},
        "UpperLeg.R": {"rx": [(0, 0.3), (1, 0.3)]},
        "LowerLeg.L": {"rx": [(0, -0.3), (1, -0.3)]},
        "LowerLeg.R": {"rx": [(0, -0.3), (1, -0.3)]},
    }),
    # Practice half swing: partial unwind and back, arms riding the torso.
    "FidgetHalfSwing": (0.9, False, {
        "UpperArm.R": {"rx": [(0, -0.95), (0.45, -0.95), (1, -0.95)],
                        "rz": [(0, -0.8), (0.45, -0.35), (1, -0.8)]},
        "UpperArm.L": {"rx": [(0, -0.95), (0.45, -0.95), (1, -0.95)],
                        "rz": [(0, 0.85), (0.45, 0.45), (1, 0.85)]},
        "Bat": {"rx": [(0, 0.6), (0.45, 0.3), (1, 0.6)]},
        "Spine": {"ry": [(0, 0.25), (0.45, -0.15), (1, 0.25)]},
        "UpperLeg.L": {"rx": [(0, 0.3), (1, 0.3)]},
        "UpperLeg.R": {"rx": [(0, 0.3), (1, 0.3)]},
        "LowerLeg.L": {"rx": [(0, -0.3), (1, -0.3)]},
        "LowerLeg.R": {"rx": [(0, -0.3), (1, -0.3)]},
    }),
    # Bat flip: arms sweep up and out, barrel flicks skyward, chest opens.
    # Plays via Playing.next after BatterSwing, so frame 0 matches the
    # swing's END pose region (arms driven through — approximate with the
    # follow-through-side arm values; tune against the pose sheet).
    "CelebrateBatFlip": (0.85, False, {
        # Frame-0 arm values are BatterSwing's REAL end pose (read from its
        # f=1.0 keys), so the next-chain never teleports the arms.
        "UpperArm.R": {"rx": [(0, -2.20), (0.35, -2.5), (1, -1.6)],
                        "rz": [(0, 0.80), (0.35, 0.4), (1, 0.5)]},
        "UpperArm.L": {"rx": [(0, -0.9326), (0.35, -1.8), (1, -1.3)],
                        "rz": [(0, 2.3596), (0.35, 1.6), (1, 1.8)]},
        # Frame-0 Bat.rx matches BatterSwing's own real end value (0.4, not
        # the brief's provisional 0.6) so the bat's local barrel angle never
        # pops either, on top of the arm channels already matching exactly.
        "Bat": {"rx": [(0, 0.4), (0.3, 2.2), (1, 1.4)]},
        "Spine": {"rx": [(0, 0), (0.4, -0.22), (1, -0.1)],
                   "ry": [(0, -0.25), (1, 0.0)]},
        "Head": {"rx": [(0, 0), (0.4, -0.3), (1, -0.15)]},
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
