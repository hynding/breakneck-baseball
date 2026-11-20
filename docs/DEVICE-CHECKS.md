# Device checks for graphics quality

The Auto / Low / Medium / High setting (`src/game/present/quality.rs`) is
unit-tested and runs headless in Chromium, but a cloud session has no phone
and no GPU. This page says what the automated checks cover and what still
needs a person with a device.

## How Auto picks a tier

| Device | Auto picks | MSAA | Shadow map | 3D render density |
|---|---|---|---|---|
| Phone or tablet (browser reports `pointer: coarse`) | Low | off | 512 | 1 render pixel per CSS pixel, and at most 75% of the screen's physical pixels |
| Desktop browser (mouse or trackpad) | Medium | 2x | 1024 | up to 1.5 per CSS pixel |
| Native, discrete GPU | High | 4x | 2048 | full |
| Native, integrated GPU | Medium | 2x | 1024 | up to 1.5 per CSS pixel |
| Native, software renderer | Low | off | 512 | as Low above |

**Adaptive fallback.** While the setting is Auto, the game watches real
frame times. After a 6-second settle, it averages frames over 4-second
windows; two slow windows in a row (mean frame over 25 ms, i.e. under 40
fps) step the tier down one level, never below Low and never back up on its
own. The settings row then reads e.g. "Auto: Low (slow GPU), no AA, 50%
res", and the browser console logs `graphics: frames averaged 31.2 ms; Auto
steps down to Low`. Picking a tier by hand, or cycling back to Auto, clears
the step.

## What the automated checks cover

- **Unit tests** (`quality.test.rs`): tier resolution per platform, MSAA,
  shadow map and render scale per tier, the settings row text, and the
  fallback's frame budget (smooth runs never step, sustained slow frames step
  one tier at a time to Low, boot stutter and tab-switch hitches don't count,
  a device crawling at 2.5 fps still steps).
- **Headless browser run** (`node tools/web_quality_check.mjs <url>`):
  loads the web build in Chromium with software WebGL2 twice, as a desktop
  browser and as an emulated iPhone 13 (touch, coarse pointer, 3x screen).
  It records the tier Auto picked, any step-down, the frame rate, panics and
  a screenshot. Software rendering is far slower than any real GPU, so its
  frame rates say nothing about real devices; what it proves is that both
  paths boot, pick the right tier, render, and fall back when frames are
  slow.

### First headless run (2026-11-14, debug wasm, SwiftShader)

| Profile | Auto picked | Fallback | Frame rate in a game |
|---|---|---|---|
| Desktop browser, 1280×720, mouse | Medium | stepped to Low after ~14 s (frames averaged ~240 ms) | ~0.5–2.5 fps (software rendering) |
| Emulated iPhone 13, touch, DPR 3 | Low | already at the floor | ~9 fps (software rendering) |

No panics in either. Two things it surfaced that need a real phone:

- **The canvas renders at 1x on a 3x phone.** Under emulation the canvas
  backing store was 390×664 for a 390×664 CSS viewport at
  `devicePixelRatio` 3, so the whole frame (UI included) rendered at a third
  of the screen's resolution and the HUD text looked soft. That may be an
  emulation artifact, so check it on a real phone (step 2 below).
- **The HUD crowds a portrait phone.** In portrait the pitching card covers
  the touch pad and the at-bat card sits under the scoreboard. Landscape is
  the intended orientation, but portrait is what a phone opens in.

## Frame cost per tier

Every presentation change in the graphics and camera plan states what it
costs per tier. The game prints a `bb-perf` line every 5 s when asked:
`?perf` on the web, `BREAKNECK_PERF=1` natively (`0` or `false` turns it off). `?quality=low|medium|high`
(natively `BREAKNECK_QUALITY=...`) pins a tier for that run only, without
touching the saved setting. Each line gives the tier, the mean and worst
frame, the entity count, and the meshes visible to any view (the main
camera or a shadow cascade), a proxy for draw cost rather than a draw count.

`node tools/frame_cost.mjs <url> [out-dir] [seconds-per-tier] [WxH]` runs
all three tiers in a one-player game and prints the medians. Compare runs
from the same machine only: under software WebGL the absolute times say
nothing about a real device. The per-tier allowances each feature reads
(crowd size, particles, fog, sky, bloom, replay length) live in
`RenderBudget` in `src/game/present/quality.rs`.

### Baseline (2026-11-20, Bevy 0.18, debug wasm, SwiftShader, 1280×720, 40 s per tier)

| Tier | Mean frame (ms) | Worst frame (ms) | Entities | Meshes visible |
| --- | --- | --- | --- | --- |
| Low | 438.7 | 461.0 | 1024 | 151 |
| Medium | 975.7 | 1007.2 | 1024 | 151 |
| High | 982.8 | 1010.4 | 1024 | 151 |

Low's lower render scale and lack of MSAA halve the software-rendered frame;
Medium and High differ only in MSAA 2x vs 4x and the shadow map here.

### Look pass (task 3; same machine, 640×360, 25 s per tier)

| Tier | Before (ms) | After (ms) | What it added |
| --- | --- | --- | --- |
| Low | 97.6 | 122.1 | Fog and colour grading |
| Medium | 238.2 | 304.3 | + gradient skybox |
| High | 247.7 | 344.2 | + HDR and bloom (since turned off, see below) |

Software WebGL is bound by per-pixel ALU, so fog and grading look dearer
here than on a GPU. The sky was first a 700 m dome mesh and cost about
90 ms at Medium, because it shaded the whole screen before the field
covered it. As a `Skybox` it shades only the visible sky, after the
opaque pass, and costs about 9 ms. HDR and bloom on High are now off:
the scene is over-exposed (a 50k lux sun at Bevy's default exposure), and
HDR tonemapping greyed the sky toward purple. They return with the
exposure rework in the time-of-day task. A real-device check of Low (fog on a
weak phone) is still owed; see below.

### Ball readability (task 4; same machine and settings as the look pass)

| Tier | Before (ms) | After (ms) |
| --- | --- | --- |
| Low | 122.1 | 120.0 |
| Medium | 304.3 | 299.9 |
| High | 344.2 | 340.0 |

No measurable cost: one shadow disc, one proxy sphere, and the existing
flight ghosts. A home-run fly at Low in an 844×390 window (292 px of 3D)
shows the ball as a 4–5 px disc against the sky and its shadow on the
grass past the wall. To repeat it, build with `--features autoplay`, wait
for a `bb-contact` console line with an angle over 20°, and screenshot
1–3 s later.

## What still needs a person with a device

Use the GitHub Pages build for the web checks and `cargo run --release` for
native.

1. **Phone, Auto (iPhone Safari, then an Android in Chrome).** Settings
   should read "Auto: Low". Play a full inning and note whether it stays
   smooth through a home run, the camera cuts, and a play at the plate. If
   the row later reads "(slow GPU)" on a phone that already started at Low,
   nothing more can step down: note the phone model, because the next lever
   is a lower Low render scale.
2. **Phone sharpness at Low.** On a 3x screen, Low renders the 3D view at a
   third of the physical resolution. Check the ball is easy to track out of
   the pitcher's hand and the players aren't a smear. Menu and HUD text
   should stay crisp, because the UI renders at full resolution. If the 3D
   view is too soft, the dial is the Low density cap in
   `QualityTier::render_scale` (1.0 now; 1.25 would be a modest step up).
   Also check the canvas really renders at the screen's density: with Safari's
   Web Inspector (or Chrome's remote debugging) attached, run
   `[document.querySelector('canvas').width, innerWidth, devicePixelRatio]`
   in the console. On an iPhone in portrait the first number should be about
   `innerWidth × devicePixelRatio` (e.g. 1170 for 390 × 3). If it equals
   `innerWidth`, the game is rendering at 1x (what the headless emulation
   showed), and the HUD text will look soft: report it.
3. **Switch tiers mid-game on the phone.** Cycle Low → Medium → High → Auto
   from the pause menu. The UI must never go blank (the wasm/WebGL2 failure
   the single render route exists to avoid), and the view should re-sharpen
   or soften right away with no black frame that sticks. (HDR and bloom
   are off on every tier for now; when they return on High, check the
   floodlights glow and no frame goes blank, since they need float render
   targets, `EXT_color_buffer_float`, that some phone drivers lack.)
4. **Rotate the phone at each tier.** Portrait to landscape and back. The 3D
   view should refill the screen at the new size, with nothing stretched or
   letterboxed. In portrait, note whether the HUD cards overlap each other
   or the touch pad (they did under emulation).
5. **Shadows and edges at Low.** With a 512 shadow map and no MSAA, look at
   the players' shadows (blocky, or shimmering as they run?) and the foul
   lines and bases (jagged?). Decide whether that's acceptable.
6. **iPad.** With a keyboard and trackpad attached the browser may report a
   fine pointer, which picks Medium; check Medium holds up there (if it
   doesn't, the fallback should step it to Low within ~15 s). Without the
   keyboard it should pick Low.
7. **Desktop browser, Auto → Medium.** On a laptop with integrated graphics,
   check an inning plays smoothly. Then pick High and note whether it drops
   frames.
8. **Native desktop.** On a discrete GPU, Auto should read High. On a
   MacBook Air or similar, it should read Medium.
9. **Heat and battery.** Play about 10 minutes on the phone at Auto and note
   whether it gets hot or starts to stutter late in the session (thermal
   throttling). If it throttles hard, the fallback should step the tier down
   on its own; note whether it did.
10. **Persistence.** Pick a non-Auto tier, reload the page, and confirm it's
    still selected.

If something looks off, tell the thread the device, the browser, the tier,
what the settings row read, and what you saw (a screenshot helps).
