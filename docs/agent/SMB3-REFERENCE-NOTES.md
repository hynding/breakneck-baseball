# Reference notes: Super Mega Baseball 3 presentation vs. Breakneck Baseball

Rewritten 2026-09-15 from an actual frame-level watch of the reference video
(https://www.youtube.com/watch?v=Bk5n5aoi7AQ — "Super Mega Baseball 3 Gameplay (PC HD)",
0:10–3:00, the top of the first inning: six plate appearances, Sawteeth batting).

**Provenance.** This version supersedes the 2026-09-15 morning draft, which was written
without decoding a single frame and got several things wrong (see §6). Method this time:
the full range at 2 fps (320 frames), scene-change detection for the cut list, and 4–10 fps
contact sheets over the six beats that matter (a fly out, a swinging strike, a taken pitch,
two infield hits with runs scoring, a foul, a strikeout). Durations *within* a beat are
good to ±0.25 s. Absolute timestamps are only good to ±1 s (seeking in the downloaded file
drifts), so events below are named by content, not by clock. Everything attributed to
Breakneck is read from the code as of commit 13880cb and is exact.

The footage never shows a pitcher-side camera: every pitch is seen from the batting camera
(both SMB3 profiles in the video are human, and the pitch-selection UI is overlaid on the
batting view). So this video calibrates the **batting experience** only; it says nothing
about a role-based view.

---

## 1. What the footage shows

| PA | Batter | Result | Beats seen |
|---|---|---|---|
| 1 | Bags (R) | fly out to RF | aim → pitch → contact → ball-follow → fielder cam → OUT #1 |
| 2 | Ronero (R) | swinging strike, then infield single | STRIKE 1 beat; ground ball → base cams → SAFE! |
| 3 | Bronco (L) | taken low pitch, then infield hit, run scores | BALL beat (no text); SAFE! → "1 - 0" |
| 4 | Nutmeg (L) | ground ball, runner safe at home | SAFE! → "2 - 0" |
| 5 | Turner (L) | (walk-up only in range) | walk-up card |
| 6 | Young Jr (R) | swinging strike (LATE), two fouls, strikeout | STRIKE/LATE, FOUL BALL ×2, OUT #3 reaction cam |

Real time is spent in two places: **watching the ball come in** (every pitch) and **between
plate appearances** (a logo wipe plus a walk-up card the player dismisses). The beats
*between pitches* are short.

---

## 2. The presentation language, as observed

### 2.1 The pitch view (fixed for the whole duel)

- One camera for the whole at-bat: **behind and beside the batter's box**, roughly at the
  batter's shoulder height, looking out at the pitcher. It never moves during the wind-up,
  the flight, or the swing.
- Composition (1080p, righty at the plate): the batter fills ~90% of the frame height on
  the **left third** (helmet just under the HUD, cleats at the bottom edge), seen from
  behind so the jersey number reads. The pitcher is a small figure ~35% down from the top
  at centre. The batter's contact circle (SMB's PCI: a dotted ring with corner brackets)
  sits at ~55% down, centre. Home plate is at ~85% down, centre-bottom. A blue ring on the
  ground marks the batter's box. A left-handed batter mirrors: batter on the right, same
  everything else.
- **The catcher and plate umpire are not drawn** in this view. The ball simply exits the
  bottom of the frame after the plate. (The earlier draft claimed the opposite.)
- The pitcher's aim marker (a ball-shaped reticle) and the pitch-type/mph card are drawn
  on this same view when a human pitches; the batter's PCI is a separate ring. The human
  pitcher in this footage takes 3–8 s to aim — that wait is the player's, not the game's.

### 2.2 The pitch flight

- Release → plate is ~0.5 s for a 95–98 mph fastball and ~0.65 s for a 79 mph curve —
  the same physics time Breakneck already uses (0.485 s at 38 m/s).
- What makes it *readable* is not duration: (a) the camera looks **along** the ball's
  path, so on screen the ball mostly **grows** rather than streaks; (b) the ball is drawn
  well over real size with a short white trail; (c) the batter's whole body and the full
  bat arc are visible beside the ball's path, so a miss is *seen* — bat here, ball there —
  not inferred from a banner.

### 2.3 The swinging-strike beat (Ronero PA 2, Young Jr PA 6) — measured

| t | what is on screen |
|---|---|
| 0.00 | bat passes, ball crosses the PCI (the miss is visible) |
| +0.1–0.2 | **"STRIKE 1"** text mid-screen above the plate; **timing text ("LATE")** just under it; **"97 mph!"** pill at the plate. Count ticks now. |
| +0.2 → +0.7 | same camera, batter's follow-through and recovery play out, text held |
| ≈ +0.8 | **dip to black** (~0.2 s: the frame darkens with the batter already back in stance) |
| ≈ +1.0 | fade-in on the next pre-pitch: aim marker, new count, pitch count "P: n" |

Total miss → next pre-pitch ≈ **1.0 s**. The hold is short; it is *full* (follow-through
+ three pieces of text) and *punctuated* (the dip). The dip also hides every reset —
nothing is seen teleporting.

### 2.4 The taken-pitch beat (Bronco PA 3, a low ball)

- Ball crosses low, exits the frame. **No large text, no dip.** The count ticks, the
  batter stays in stance, the aim marker is back on screen within ~1 s. A ball is the
  quietest event in the game.

### 2.5 Contact: cuts, not glides — measured (Bags PA 1, Bronco PA 3, Young Jr PA 6)

| t | what is on screen |
|---|---|
| 0.00 | contact in the batting view (bat blur, ball trail; on a strong hit a burst VFX) |
| +0.0–0.25 | sometimes a **one-frame contact insert** from another angle (tight behind the batter, or from the 1B side with the catcher in frame) |
| ≤ +0.25 | **hard cut** to the ball-follow shot |

There is **no plate hold**. (The earlier draft assumed 0.4–0.7 s; it is at most one frame
of insert.) The batter is never seen breaking from the box in the batting view.

### 2.6 The play shots and the result hold — measured

- **Fly ball (PA 1)**: cut to an elevated shot *behind the mound* looking out along the
  ball's line; the camera pushes/pans with the ball for ~2.5 s; as the ball descends it has
  become a **fielder cam** (behind/above the RF, ball large, a landing marker on the grass).
  Catch → **"OUT #1"** at top centre → hold ≈ **1.5 s** on the fielder (he lobs the ball
  in) → team-logo wipe.
- **Ground ball (PA 2, 3, 4)**: cut to the high behind-home broadcast shot (≈15 m up,
  behind the plate, whole infield) as the ball leaves; cut to an **along-the-baseline shot**
  (low, behind/beside the runner going to 1B, fielder and throw in frame); cut back to the
  high shot as the throw arrives; **"SAFE!"** in that shot. If a run scored, "SAFE!" is
  replaced ~1 s later by the **score, "1 - 0"**, for ~1 s. Hold after the call ≈ **2–3 s**
  total, then the wipe.
- **Foul ball (PA 6)**: cut to the broadcast/3B-side shot, then to a foul-territory shot;
  **"FOUL BALL"** red text at top centre; ≈1.5 s; dip to black; back to the batting view.
- **Strikeout (PA 6)**: cut to a **side-on plate cam** (from the 1B side, low, ~4 m): batter,
  catcher with the ball, umpire behind. **"OUT #3"**, "LATE", "100 mph!". The batter turns
  and walks toward the dugout; the camera swings behind him; ≈ **2.5–3 s** then the
  half-inning transition.
- Throw meter ("POW: 54") and the controlled fielder's card appear on every fielding shot.
- The only continuously moving camera is the ball-follow. Every other framing change is
  a cut. Each shot has one subject.

### 2.7 Between plate appearances

- After the result hold: a ~1.5 s **team-logo wipe** (shark logo, diagonal panels), then
  the **"Up to Bat" card scene**: a close, low camera on the new batter at the plate
  (catcher and umpire *are* visible here), the batter card (name, order, bats L/R, mojo,
  fitness, power/contact/speed bars), a walk-up animation (stretches, bat waggle), and
  "Play Ball!" on A / "Substitute" on Y. In the footage this runs 6–8 s per batter and
  ends on the player's press. Then a fade-in to the batting view.
- This is where SMB3 spends its seconds. It is also where every state reset happens
  off-camera.

### 2.8 HUD and text

- Persistent, small, top-centre: score, team logos, B/S/O dots, inning, "pressure".
  Bottom-right (batting): pitcher card and batter card with stat bars. Bottom-left on
  fielding shots: the controlled fielder's card. Bottom-right on fielding shots: a base
  diagram with runner markers.
- Transient, large: STRIKE n / FOUL BALL / OUT #n / SAFE! / score "1 - 0" at top or mid
  centre; timing text (EARLY/LATE) under the strike text; the pitch mph pill at the plate.
  "OUT #n" doubles as the out counter. Nothing transient outlives its beat.

---

## 3. Breakneck today (exact, from the code)

### 3.1 Duel camera
- `DuelView` default `CatcherPov`: eye `(-0.4, 1.4, -1.15)`, target `(0, 0.2, 4)`, 80°
  vertical FOV (`present/camera/mod.rs`, `core/variant.rs`). The batter (always at +x; the
  roster has no handedness) stands under a metre from the lens, so the swing crosses the
  frame in a few frames and the ball flies *into* the lens. Catcher and umpire are hidden
  outright (`rigs::hide_occluders`).
- `BattingZoom`: eye `(0.8, 1.7, -3.2)`, target `(0.1, 1.0, 12.0)`, 65°. This is already
  close to SMB3's batting view; the catcher/umpire fall inside the 4 m occlusion cone
  (`framing::OCCLUSION_NEAR`) and are hidden, which SMB3 also does.
- `V` cycles four views; the choice persists across half-innings.
- The ball mesh is already 2.7× real radius (`theme.ball.visual_scale`), and the pitch has
  a ghost trail (`sim/ball.rs`).

### 3.2 The take/miss beat
- Ball hidden at glove range (`catcher_receives`), judged ≈0.13 s past the plate, banner
  + count repaint + `Phase::Result` on the same frame, re-shown parked at the glove.
- `RESULT_SECS` = 1.2 s for every outcome (`PaceTuning.result_secs`); nothing moves in
  it. At Result end the ball **teleports** to the mound (`result_phase`), then the CPU
  waits 0.7–1.2 s (`ai.rs`) before its wind-up.
- Banner lifetime 1.6 s (outlives the phase); contact stamp 0.8 s; **no stamp on a
  `Whiff`** (`banner.rs::show_contact_stamp`). No pitch-speed readout.

### 3.3 The contact beat
- `BALL_FOLLOW_DELAY` = 1.0 s hold on the duel framing after contact, during which the
  real batter is swapped for the ghost runner at `RUN_OUT_DELAY` = 0.15 s (the 0.42 s
  `BatterSwing` is cut off).
- Then a single smoothed rig **glides** (time constant 0.2 s; the code comment says
  "glide, never cut") to a landing-zone shot; the same rig glides to the wide home
  framing for the 1.2 s result pause. No fielder cam, no base cam, no reaction cam.
- `LiveBallEvent::Thrown { base }` already decides the race at the throw and announces on
  `Settled` — the hook a base cam needs exists and is unused.

---

## 4. Why "there's no time to observe the hit or miss"

The pitch flight time is the same as SMB3's. The difference is that Breakneck's default
view makes the decisive 100 ms unreadable and then denies any confirmation:

1. **Foreshortening**: from `CatcherPov` the ball's screen motion is a streak into the lens
   and the batter is a blur at the edge. SMB3 looks along the path from 2.5 m back with the
   whole batter in frame, so the ball grows and the bat's arc is visible.
2. **The miss is never confirmed**: no EARLY/LATE on a whiff, no speed, the ball vanishes
   0.4 m short of the mitt, and the banner is the only evidence.
3. **The beat has no shape**: 1.2 s of a static frame, then a teleport, then a wind-up.
   SMB3's beat is *shorter* but has a follow-through, three pieces of text, and a dip to
   black that punctuates it and hides the reset.
4. **After contact, the wrong second is held**: the plate is held for 1.0 s while the
   batter has already been swapped for a ghost, then the camera glides 20–30 m. SMB3 cuts
   within a quarter second and never shows a glide.

Note what the previous pacing pass (TADA 89–95) established still holds: dead air was the
enemy then, and it still is — none of the plans below lengthens a wait without filling it.

---

## 5. Plans

Ranked by impact on the user's complaint per hour of work. Each keeps `core/rules/`
untouched (no balance impact; `tests/balance_sim.rs` reads outcomes, not wall time).
Suggested order: A → B → C → D → E.

### Plan A — Make the SMB3 batting view the default duel view

Goal: every pitch is watched from behind and beside the batter, whole batter in frame,
ball growing toward a centred zone.

1. Retune `BattingZoom` (or add `DuelView::BehindBatter` and make it the default) to the
   SMB3 composition. Starting point for Standard: eye ≈ `(0.9, 1.6, -2.6)`, target chosen
   so that the pitcher's release point projects ~35% from the top, the zone centre ~55%,
   the front of the plate ~85%; vertical FOV ≈ 55–60°. Front-yard scales as the other views
   do (`FieldSpec`). Pin the composition in `framing.test.rs` with `framed_ndc_y` for both
   parks and 16:9 / 4:3 (the batter's helmet top, the zone centre, the plate).
2. Keep the catcher/umpire hidden here (the 4 m cone already does it; SMB3 hides them
   too). Make sure the *batter* is never an occluder (only `CatcherRole`/`PlateUmpire` are,
   today — keep it that way).
3. Default: `BehindBatter` for every mode (human batting, human pitching, CPU vs CPU). The
   footage shows the pitcher's aim UI drawn on this same view, so the pitch aim reticle in
   `present/field/zone.rs` needs no change. `V` still cycles; `CatcherPov` stays as an
   option, no longer the default. A setting can pin a view.
4. Check the ball's read from the new distance: `theme.ball.visual_scale` is 2.7 already;
   verify the pitch trail reads at 2.6 m and tune `visual_scale` (not the collider) if the
   ball looks small at the zone.

Owners: `present/camera/{mod,framing,rigs}.rs`, `core/variant.rs`, `meta/settings/`.
Tests: `e2e_camera_views` (cycle order / default), new framing pins. Risk: the PCI reticle
and the zone box were tuned for the POV eye — they are 3D, so they scale, but re-check the
reticle's size against the ball at the new distance.

### Plan B — Shape the take/miss beat like SMB3's

Goal: a miss is *seen*, *explained*, and *punctuated* in ~1 s; a ball is quiet.

1. **Timing stamp on every swing**, including `Whiff` (EARLY/LATE by the sign of the swing
   `dt_ms`), placed under the strike text as SMB3 does. `show_contact_stamp` returns
   `None` for `Whiff` today — that is the one-line root of "no feedback on the swing I
   most need feedback on".
2. **Pitch speed pill** at the plate ("97 mph") on every pitch result; the release speed
   is known (`PitchKind::speed()` × `PaceTuning`).
3. **Per-outcome pause lengths** in `PaceTuning` instead of one `result_secs`: ball ≈0.8 s
   (no big text — a small BALL n near the plate), swinging/called strike ≈1.0 s, foul
   ≈1.6 s, strikeout ≈2.5 s (Plan E's shot), HBP ≈2.0 s. Banner lifetime becomes ≤ the
   pause so nothing outlives its beat.
4. **Dip to black** (~0.25 s) at Result end for strike and foul outcomes (not balls), then
   `PrePitch`. Implementation: one full-screen black node painted **opaque at game start**
   (a fade-in from black on PLAY BALL, which is a free improvement) and driven by alpha
   afterwards — never spawned at alpha 0 (the wasm invariant in `CLAUDE.md`). The dip is
   where the mound reset and the batter's stance reset happen, so the teleport is never
   seen. Reduce-motion: keep the dip (it is less motion, not more).
5. **Fill the hold**: let `BatterSwing` → `RecoverSwing` finish in frame (they already
   chain); do not start the ghost run-out on a whiff (n/a) and do not repaint the count
   before the text lands (repaint on the same frame as the text is fine — SMB3 does).

Owners: `sim/flow/{pitch,result}.rs`, `present/ui/{banner,hud}.rs`, `core/variant.rs`
(`PaceTuning`), a new `present/ui/transition.rs` for the dip. Tests: `e2e_contact_stamp`
(whiff case), a new `e2e_call_beat` asserting text → dip → `PrePitch` order and the pause
table, a unit test on the per-outcome table. Risk: e2e helpers that advance a fixed frame
budget to reach `PrePitch` must be checked when any pause grows (`tests/common/mod.rs`);
the Coach's `result_stuck` tolerance reads `pace.result_secs` (`core/coach/checks.rs`) and
must read the per-outcome value.

### Plan C — Cut-based shot list after contact

Goal: replace the plate hold + glide with SMB3's grammar: cut at ≤0.25 s, one subject per
shot, the call announced in the shot where the play ends.

1. `Shot` enum in `present/camera/rigs.rs`: `Duel`, `ContactInsert`, `BallFollow`,
   `FielderCam { entity }`, `BaseCam { base }`, `ResultHold`, `TrotOrbit` (exists).
   `BroadcastRig::cut()` snaps eye/target/fov; the existing exponential smoothing remains
   only *within* `BallFollow` (tracking the ball) and `FielderCam`.
2. Selection from flow signals only:
   - contact → `Duel` for ≤0.25 s (optionally a 0.2 s `ContactInsert`: tight behind the
     batter, or from the 1B side for variety) → cut to `BallFollow`.
   - `BallFollow`: elevated behind the mound looking along the ball's line for flies (start
     ≈ `(0, 6, 8)` looking at the ball, pull with it); the high behind-home broadcast eye
     for grounders. It re-predicts the landing every frame as today.
   - `ContactClass::CatchableFly` with a chaser assigned → cut to `FielderCam` as the ball
     apexes (behind/above the chaser, landing ring in frame).
   - `LiveBallEvent::Thrown { base }` → cut to `BaseCam { base }`: low, along the baseline,
     runner + bag + incoming throw. The call is announced on `Settled`, so SAFE!/OUT lands
     *in this shot*.
   - call → `ResultHold` on the current shot: out ≈1.5 s, hit ≈2.0 s, plus ≈1.0 s of the
     score text if a run scored; then cut to `Duel` at `PrePitch` (or the wipe, Plan D).
   - HR keeps the trot orbit.
3. `BALL_FOLLOW_DELAY` 1.0 → 0.25; `RUN_OUT_DELAY` 0.15 → 0.25 so the swap happens at the
   cut, never in frame. "OUT" banners become "OUT #n" (SMB3 uses the text as the out
   counter; it reads better than a dot).

Owners: `present/camera/`, `sim/runner.rs` (one constant), `present/ui/banner.rs` (OUT #n).
Tests: pure unit tests for shot selection (phase + events → shot), `e2e_home_run_moment`
unchanged, a new `e2e_base_cam` (DP-setup scenario, ground ball, assert the shot at the
throw). Risk: any test reading `Camera3d` transform must be re-pinned; the Coach reads
nothing from the camera.

### Plan D — Between plate appearances: wipe + walk-up card

Goal: spend the seconds where SMB3 spends them, and hide every reset there.

1. When a Result ends a plate appearance (out, hit, walk, HBP): a ~1 s **wipe** (a
   theme-tinted panel sweep — `BannerTone`/theme-owned colours) into a **walk-up scene**: a
   low, close camera on the incoming batter at the plate with the catcher and umpire
   visible (a fifth framing in `FieldSpec`), a batter card (name, order, jersey number;
   stat bars if the roster has them), one fidget clip (`FidgetBatTap`/`FidgetHalfSwing`
   exist) then the stance loop.
2. Dismissed by any confirm/swing input, or after ≈3 s for CPU/autoplay (`Director`
   scripts must not stall: the auto-dismiss is the default in headless runs).
3. The mound reset, runner cleanup, and the ball's return all happen behind the wipe.
4. wasm: the card is a UI root spawned mid-`Playing` — paint the root at game start with a
   `BackgroundColor` and show/hide by mutating children (`ui::hidden_tint`), per the
   invariant.

Owners: `sim/flow/result.rs` (a `WalkUp` sub-phase or a flag on `PrePitch`),
`present/ui/walkup.rs`, `present/camera/`, `meta/input.rs` (dismiss routes through
`Intents` so scripts cover it). Tests: `e2e_cpu_timing` re-pinned, matrix test for the
auto-dismiss, a unit test that a mid-PA pitch never triggers the wipe. Risk: the CPU pitch
delay must not start until the walk-up ends.

### Plan E — Strikeout and scoring beats

1. **Strikeout**: cut to a side-on plate cam (from the 1B side, ~4 m, eye ≈ `(4, 1.3, -1)`
   looking at the plate) with batter, catcher, and umpire visible (they are only hidden by
   the duel cone, which does not apply to this shot); text OUT #n + LATE/EARLY + mph; the
   batter walks toward the dugout (reuse the run-out path infra at walking speed; a
   `WalkOff` clip via the Blender pair later); hold ≈2.5 s.
2. **Run scores**: after SAFE!, show the score "1 - 0" for ≈1 s in the same shot before the
   hold ends.
3. Umpire gesture clips (`UmpStrike`/`UmpPunchOut`/`UmpSafe`) are **not** required by the
   footage — the umpire is invisible in the batting view — so they drop to "nice" and
   attach only to the K reaction cam and the base cam.

Owners: `present/camera/`, `present/ui/banner.rs`, `sim/runner.rs` (walk-off path),
later `present/animation/` + `tools/*.py`. Tests: `e2e_strikeout_beat`.

### Not planned (deliberately)

- **Lengthening `result_secs` across the board.** SMB3's per-pitch beats are ≤1 s; the
  immersion comes from the view, the text, and the cut, not from waiting.
- **Slow motion on pitch results.** Not in the footage; keep `juice.rs` as is.
- **A catcher lob-back / pitcher-receives animation.** Not shown; the dip and the wipe hide
  the reset instead, which is cheaper and what the reference does.
- **Any change to swing windows, exit speeds, or CPU timing spread.**

---

## 6. Corrections to the previous (unverified) draft

| Earlier claim | What the frames show |
|---|---|
| Catcher and umpire visible below the zone in the batting view | Not drawn at all; the ball exits the bottom of the frame |
| Result hold 1.5–2.5 s after a take/miss, "something moving every second" | ≈0.7 s hold + 0.2 s dip for a strike; ≈0 for a ball |
| Umpire gesture → text → count as a staged sequence | Text, timing, mph, and count all land within ~0.2 s; no umpire is visible |
| Catcher throws back, pitcher receives, batter steps out | None shown; a dip to black covers the reset |
| Plate hold of 0.4–0.7 s after contact | Cut within ≤0.25 s (sometimes one insert frame) |
| Role-based camera (batting vs pitching view) | Not verifiable here: the footage is the batting view for every pitch |
| Fixed cameras only | The batting view is fixed; the ball-follow moves continuously and every other change is a cut |

Things the draft got right and the frames confirm: cuts not glides; one subject per shot;
a fielder cam on flies and a base cam on throws; the call announced in the final shot;
result text large and transient; timing feedback on swings and misses.

---

## 7. Working files

The downloaded video, 2 fps frames, and contact sheets from this watch live under the
session scratchpad (`scratchpad/smb3/download/video.mp4`, `scratchpad/d2/`,
`scratchpad/sheets/`) and are not part of the repo. Re-extracting: one long decode
(`-ss 20 -to 180 ... fps=2`) is time-accurate; separate `-ss` seeks into this file drift
by up to a second, so time beats *within* one extraction only.

---

## 8. Implementation status (2026-09-15, same day)

| Plan | Status | Where |
|---|---|---|
| A — batting view default | done, TADA 96 | `DuelView::BattingZoom` retuned + default; framing test pins the composition |
| B — take/miss beat | done, TADA 97 | `flow::ResultBeat`, `PaceTuning::result_secs_for`, the curtain (`ui/curtain.rs`), whiff stamp, mph read-out |
| C — cut-based shots | done, TADA 98 | `camera::Shot`, `pick_shot`, fielder/base cams, 0.25 s hold |
| D — walk-up | done, TADA 99 + 102 | `Play::walkup`, `Shot::WalkUp`, `ui/walkup.rs`; the themed wipe (`WIPE_SECS`, `WipeLabel`) before it |
| E — strikeout / score | done, TADA 100 + 103 | `Shot::ReactionCam`, `PlayBanner::follow_up` score and out-count lines, the batter's walk-off on `MoveIntent`, umpire clips (`UmpStrike`/`UmpPunchOut`/`UmpSafe`) signalling every call |

Nothing from the plan is left open. TODO 105 (the balance harness was pacing-sensitive because
the CPU noise read the wall clock) was fixed by reseeding per pitch (TADA 101), which is what
allowed the 2.2 s strikeout hold the footage suggests.
