# Batting Feel & Settings — Design

**Date:** 2026-07-30
**Status:** Approved design, pending implementation plans

## Goal

Make the pitch-vs-swing duel a genuine skill contest with arcade-grade feedback:
swing timing deterministically drives contact outcomes, three selectable
batting input styles share one contact model, and a new settings system makes
the styles (and future options) configurable per player and persistent.

## Decisions

| # | Decision |
|---|---|
| 1 | **Timing drives outcomes.** Swing timing vs plate arrival is a real input to contact quality — deterministic, `rules.rs` stays RNG-free (Δt is player input, not randomness) |
| 2 | **Three batting styles, player-selectable:** Classic timing, Swing meter (hold-release), PCI cursor — the same trio-over-one-model architecture MLB The Show ships (Zone/Directional/Analog) |
| 3 | **Settings screen + persistence:** **S** from the main menu; native → JSON in the platform config dir, wasm → `localStorage`. Per-player batting style (P2 defaults to P1). A **volume row ships in the first plan** so crowd audio never lands without a knob |
| 4 | **All four juice elements:** contact stamp + zone flash, hit-stop + slow-mo, synthesized crowd + contact audio, home-run moment |
| 5 | **CPU skill dial:** deterministic hash-noise timing spread on `Ruleset` — balance default now, difficulty lever later |
| 6 | **Balance-sim harness:** headless CPU-vs-CPU stat regression (new) — tuning is tested, not vibed |

## 1. Settings infrastructure (`settings.rs`, new module + plugin)

- `Settings` resource: `{ batting_style: [BattingStyle; 2], volume: f32, .. }`,
  serde round-tripped through one persistence seam:
  - native: JSON at the platform config directory (`dirs`-style lookup);
  - wasm: `window.localStorage` via a `web-sys` wasm-only dependency (joins the
    existing `[target.'cfg(target_arch = "wasm32")']` section; lockfile stays
    committed, wasm-bindgen pin unaffected).
- **S** on the main menu opens the settings screen: keyboard/gamepad driven
  (Up/Down row, Left/Right cycle value, Esc back), themed from `UiTheme`,
  spawned at startup and shown by child mutation per the CLAUDE.md wasm UI
  rule. Saves on every change; load-or-default at startup.
- Rows at launch: **P1 Batting Style**, **P2 Batting Style** (defaults to
  P1's until changed), **Volume** (master, drives `bevy_audio` global volume).
  The row list is data so future options (difficulty, camera default) are
  one-line additions.
- PCI's row label carries a "(gamepad recommended)" hint — see §3.

## 2. The contact spine (rules + flow/ball)

- At release the engine already predicts the plate crossing (catcher mitt,
  HBP, landing logic); store the pitch's **plate ETA** on `Play`.
- A swing input records its instant; **Δt = swing_instant − eta**. First
  swing input per pitch counts; a swing with no contactable Δt is the whiff.
- New pure fn `rules::contact_quality(dt, ruleset) -> ContactQuality`
  (`Whiff | FoulTip | Weak | Solid | Perfect`), window half-widths as
  `Ruleset` data (defaults: Perfect ±40 ms, Solid ±90 ms, FoulTip ±140 ms;
  outside = Whiff). Cited to the design rationale in doc comments; tunable
  per variant like every other rule.
- Deterministic physics mapping in the existing hit-vector code (`ball.rs`):
  - exit-speed multiplier per quality (`Ruleset` data: Weak 0.65, Solid 1.0,
    Perfect 1.25);
  - direction yaw offset **k·Δt** (early pulls toward the batter's pull side,
    late pushes opposite field; k is `Ruleset` data);
  - FoulTip forces a foul spray to the pull side by Δt sign.
- New `ContactEvent { quality, batting_team, deep_hint }` — the single spine
  all presentation consumes. `fx`/`audio`/`camera` never mutate game state;
  only flow applies rules (unchanged law).
- **Depth split, stated:** Classic and Meter are timing-only (location-blind,
  as today's contact is); PCI adds location skill on top. This is a real
  difference in style depth and is intentional.

## 3. Three batting styles as input adapters

Each style is a front-end producing the same `SwingInput { swing_instant,
aim, pci_offset: Option<Vec2> }`; the batting team's configured style routes
that player's input. CPU always uses Classic semantics.

- **Classic:** press = swing instant (today's verb, now timing-scored).
- **Swing meter:** hold to load (the stance waggle visibly deepens — reuses
  `BattingStance` posing), release = swing instant; still holding past the
  FoulTip window = whiff. No conflict with Down-hold runner controls
  (different axes/buttons).
- **PCI cursor:** during flight the aim input moves a small cursor inside the
  zone box — **velocity-based glide** (smooth while held), not 8-way digital
  jumps, so keyboard is playable; gamepad analog is the recommended input and
  the settings row says so. Contact quality degrades with cursor-to-ball
  distance at the cross — specifically, the effective timing windows shrink
  linearly with miss distance (dead-center = full windows; at the cursor
  radius the Perfect window reaches 0 and Solid is halved; beyond it, best
  case FoulTip) — and hit direction derives from contact-point offset + Δt
  instead of raw aim.

## 4. Juice systems (consume `ContactEvent` only)

- **Stamp + zone flash:** PERFECT/EARLY/LATE stamp beside the zone box in
  `BannerTone` colors; the zone box flashes on the timing window. Painted at
  spawn, mutated to show (wasm UI rule).
- **Hit-stop + slow-mo:** 3–5 frames near-freeze on Solid+, ~0.5 s at 30 %
  speed on Perfect, via `Time<Virtual>` relative-speed; always restored by a
  watchdog (never sticks). **Called out:** during Perfect slow-mo the human
  defense gets ~3× real time to aim a throw on exactly the best-hit balls —
  accepted deliberately as mild compensation for the buffed offense.
  A `JuiceDisabled` resource (inserted by the test harness) no-ops these
  systems so the 240 Hz virtual-time e2e suite is unaffected.
- **Crowd + contact audio:** synthesized only, in `audio.rs`'s idiom (no
  asset files): looping murmur bed, roar swell on Perfect/deep, groan on a
  swinging strikeout; bat-crack transient hardness keyed by quality. Master
  volume from §1.
- **Home-run moment:** Perfect + over-the-fence → scaled-up `fx` sparks as
  fireworks, a camera orbit of the trot during the (already runner-settled)
  result pause, crowd peak.

## 5. CPU skill dial

`ai.rs` draws its Δt from deterministic hash-noise with spread
`cpu_timing_spread` on `Ruleset`. One tuned default now; Easy/Normal/Hard
later is just presets of this (and possibly window widths).

## 6. Balance-sim harness (new, load-bearing)

A headless CPU-vs-CPU stat harness in the e2e suite's style: run N
deterministic games (seeded rosters/decisions as today), accumulate K%, BA,
HR/game, runs/game, and assert they land in target bands (initial bands:
K% 15–30 %, runs/team/game 3–8, HR/team/game 0.5–2.5). Window/multiplier
tuning happens against this harness; it stays as a regression test so future
physics or AI changes can't silently break game balance. Runtime budget:
keep N small enough for CI (e.g. 20 games ≈ tens of seconds at 240 Hz
virtual time) with a `--ignored` long-run variant for deep tuning.

## 7. Testing

- Unit: quality windows/signs/multipliers, adapter state machines (meter
  hold/release edges, PCI scaling), settings round-trip (native tmp file),
  volume clamp.
- Staged e2e: scripted press at exact ETA → Perfect `ContactEvent` + longer
  carry than a Weak swing; late press → opposite-field yaw sign; meter
  release timing; settings screen toggling a style routes the other adapter.
- Balance harness per §6. Harness inserts `JuiceDisabled`.
- Existing scripted-swing e2e tests get retimed into the Solid window where
  needed — outcome assertions themselves stay honest.

## 8. Phasing (three implementation plans from this one spec)

- **Plan A — Settings:** `settings.rs`, screen, persistence (both targets),
  `BattingStyle` enum + rows incl. volume. Independently shippable.
- **Plan B — Spine + Classic + juice + CPU dial + balance harness:** the
  core experience change, tuned against the new harness.
- **Plan C — Meter + PCI:** the two additional adapters on the proven spine.

## Risks & deliberate trade-offs

- **Offensive economy shifts** (whiffs exist, Perfect adds 25 % exit speed) —
  contained by the balance harness and data-tunable windows.
- **Slow-mo defense advantage** — accepted, documented in §4.
- **PCI on keyboard** — mitigated by velocity glide; honestly labeled
  gamepad-recommended.
- **Settings dep surface** — `web-sys` (wasm-only) + a config-dir lookup
  (native-only); both target-scoped per the dual-target rules.
