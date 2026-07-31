# Contact Spine + Classic Timing + Juice (Batting-Feel Plan B) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Swing timing deterministically drives contact quality (Whiff/FoulTip/Weak/Solid/Perfect) with full arcade feedback — stamp, hit-stop/slow-mo, synthesized crowd, home-run moment — a CPU timing-skill dial, and a balance-sim harness that turns tuning into regression tests.

**Architecture:** Δt is measured from live ball kinematics at the swing press (`time_to_plate = (ball.z − PLATE_Z) / −vel.z`; early press ⇒ Δt < 0). A pure `rules::contact_quality(dt_ms, ruleset)` maps Δt to quality; `flow.rs`'s existing swing site consumes it — Whiff swings through (strike as today's miss), FoulTip forces a foul, Weak/Solid/Perfect scale the existing hit vector (exit multiplier + yaw offset k·Δt) — and emits `ContactEvent`, the single spine the four juice systems consume. `ai.rs` schedules its press around a hash-noise Δt (`cpu_timing_spread`). A headless CPU-vs-CPU balance harness asserts K%/runs/HR bands. Spec: docs/superpowers/specs/2026-07-30-batting-feel-design.md §2, §4-7.

**Tech Stack:** existing crates only (Bevy 0.15, Rapier). No new dependencies.

## Global Constraints

- Prefix every cargo command: `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"`
- `rules.rs` stays pure and RNG-free (Δt is input); `fx`/`audio`/`camera`/`ui` never mutate `ScoreBoard`/`Bases`; only `flow` applies rules
- All thresholds/multipliers are `Ruleset` data, per variant: `perfect_ms: 40.0, solid_ms: 90.0, foul_ms: 140.0, exit_weak: 0.65, exit_solid: 1.0, exit_perfect: 1.25, pull_yaw_per_ms: 0.006, cpu_timing_spread_ms: 70.0` (defaults; tuned in B7 against the harness — the harness is the arbiter, these numbers may change there and only there)
- Deterministic: same inputs ⇒ same outputs; CPU noise via the existing `ai::hash01`/`noise` seeded from virtual time
- Juice must not perturb the e2e suite: every juice system no-ops when the `JuiceDisabled` resource exists; the harness inserts it
- Existing e2e scripted swings may need retiming into the Solid window — outcome ASSERTIONS stay honest (retime inputs, never weaken expectations)
- Both targets after every task: `cargo check` + `cargo check --target wasm32-unknown-unknown`; clippy `-D warnings`; fmt
- wasm UI rule for the stamp UI (painted at spawn, mutate to show)

---

### Task B1: `rules::contact_quality` + `Ruleset` timing data

**Files:**
- Modify: `src/game/rules.rs` (new enum + fn + tests at the end of the pure section)
- Modify: `src/game/variant.rs` (`Ruleset` gains the 8 fields above; both variant constructors set the defaults)

**Interfaces:**
- Produces: `pub enum ContactQuality { Whiff, FoulTip, Weak, Solid, Perfect }` (`Clone, Copy, Debug, PartialEq, Eq`); `pub fn contact_quality(dt_ms: f32, rules: &Ruleset) -> ContactQuality` — |dt| ≤ perfect_ms ⇒ Perfect; ≤ solid_ms ⇒ Solid; ≤ foul_ms ⇒ FoulTip; else Whiff. (Weak is produced by the PCI adapter in Plan C via window-shrink — Classic never yields Weak; keep the variant now so the event type is stable.)
- `Ruleset` field names exactly as in Global Constraints.

- [ ] **Step 1: Failing tests** (rules.rs test module)

```rust
    #[test]
    fn contact_quality_windows_are_data_driven() {
        let r = test_ruleset(); // existing helper; extend it with the new defaults
        use ContactQuality::*;
        assert_eq!(contact_quality(0.0, &r), Perfect);
        assert_eq!(contact_quality(-39.9, &r), Perfect);
        assert_eq!(contact_quality(40.1, &r), Solid);
        assert_eq!(contact_quality(-90.0, &r), Solid);
        assert_eq!(contact_quality(90.1, &r), FoulTip);
        assert_eq!(contact_quality(-140.0, &r), FoulTip);
        assert_eq!(contact_quality(140.1, &r), Whiff);
        assert_eq!(contact_quality(999.0, &r), Whiff);
    }
```

(If `test_ruleset()` doesn't exist, find how rules tests build a `Ruleset` today and follow that idiom.)

- [ ] **Step 2:** `cargo test --lib rules` → compile FAIL. **Step 3:** implement (doc comment cites spec §2 and notes Weak is adapter-produced). **Step 4:** tests PASS. **Step 5:** both targets + full lib suite. **Step 6:** commit `feat: contact-quality windows as Ruleset data`.

---

### Task B2: Δt capture, `ContactEvent`, quality→physics in the Classic path

**Files:**
- Modify: `src/game/flow.rs` — the `Phase::Pitch` swing site (~flow.rs:478-569: today a swing inside the Z band produces contact via the aim mapping; a swing outside produces the miss/strike path)
- Modify: `src/game/ball.rs` if the hit-vector construction lives there (it consumes `HitEvent` — the yaw/exit modifiers belong where the hit velocity is BUILT; read both files and put the modifier where the vector is constructed, documenting the choice)

**Interfaces:**
- Consumes: `contact_quality`, `Ruleset` fields (B1); the flying ball's `Velocity`/`Transform` (ball query alias exists), `PLATE_Z` const.
- Produces:
  - `pub struct ContactEvent { pub quality: rules::ContactQuality, pub batting_team: Team, pub dt_ms: f32 }` (Bevy `Event`, registered in FlowPlugin) — fired on EVERY judged swing incl. Whiff
  - Δt definition (doc-commented at the capture site): `dt_ms = -1000.0 * (ball.z - PLATE_Z) / vel.z.min(-f32::EPSILON)` sign-adjusted so **early press ⇒ negative**, late ⇒ positive; computed at the press instant
  - Behavior: Whiff ⇒ exactly today's swing-and-miss path + event; FoulTip ⇒ force the foul branch (existing foul handling) with pull-side sign from dt; Solid/Perfect ⇒ existing contact with `exit_mult` applied to the hit speed and `yaw_offset = pull_yaw_per_ms * dt_ms` added to the hit direction (sign: negative dt (early) pulls toward −X for the right-handed batter — verify against the aim.x negation convention noted in CLAUDE.md and unit-test the sign)

- [ ] **Step 1:** Staged failing test in `tests/e2e_advanced_rules.rs` style (new file `tests/e2e_contact_timing.rs`): script a pitch and press at the measured perfect instant (drive until ball.z within the tightest band of the plate, then press) → assert a `ContactEvent{quality: Perfect}` was recorded (capture via a DriveGame reader system writing to a test resource, following how other e2e capture events) and the ball's post-contact speed exceeds a deliberately-late Solid swing's from an identical scripted pitch. Also assert a very-early press (press while ball.z > plate + big margin) yields Whiff + no ball-in-play.
- [ ] **Step 2:** RED. **Step 3:** implement per Interfaces (smallest change to the existing swing site; keep the spatial band as the OUTER eligibility check and let quality refine within it — document that Whiff now covers band-misses AND timing-misses uniformly). **Step 4:** GREEN + full `cargo test` — expect some scripted-swing e2e retiming: the harness scripts press `action` during `Phase::Pitch` continuously or at coarse times; adjust ONLY press timing (e.g. press when ball is within the solid distance of the plate: `z < PLATE_Z + vel*solid_window`) so prior outcomes reproduce; NEVER touch outcome assertions. List every retimed site in the report. **Step 5:** gates. **Step 6:** commit `feat: swing timing drives contact through ContactEvent`.

---

### Task B3: CPU timing dial

**Files:**
- Modify: `src/game/ai.rs` (the batter decision site — `decided_swing` state exists ~ai.rs:34)

**Interfaces:**
- Consumes: `cpu_timing_spread_ms` (B1), `hash01/noise` (existing), ball kinematics (same Δt math as B2 — extract the Δt helper to `flow.rs` as `pub(crate) fn swing_dt_ms(ball_z: f32, vel_z: f32) -> f32` in B2 and reuse it here; add that to B2's Produces).
- Produces: when the CPU decides to swing, it draws `target_dt = noise(seed) * cpu_timing_spread_ms` once per pitch (deterministic seed from pitch instant) and presses `action` on the first frame where `swing_dt_ms(...) >= target_dt` — i.e. the CPU now has human-like timing scatter.

- [ ] Steps: failing check via the balance-harness precursor — a focused staged test asserting CPU swings across 20 scripted pitches produce ≥2 distinct qualities (deterministic spread ⇒ variety); implement; `cargo test` green (e2e_cpu may retime itself naturally — CPU outcomes may shift; e2e_cpu asserts a half-inning completes, not specific outcomes — verify and report); gates; commit `feat: CPU batters swing with a deterministic timing spread`.

---

### Task B4: Stamp + zone flash

**Files:**
- Modify: `src/game/ui.rs` (stamp element painted at spawn near the zone box area) and/or `src/game/fx.rs` (zone flash on the 3D zone frame — read how field.rs's zone box entity is identified; tint its material briefly)

**Interfaces:**
- Consumes: `ContactEvent` (B2), `BannerTone` colors from `Theme`.
- Produces: on ContactEvent, a stamp text (PERFECT! / EARLY / LATE / FOUL TIP / and nothing extra for Whiff beyond the existing strike flow) shown for ~0.8 s then blanked (timer resource; wasm rule: painted at spawn, text-mutation only). EARLY vs LATE from `dt_ms` sign for Solid; Perfect always "PERFECT!". Zone flash: the zone-box material's emissive/tint pulses once on Solid/Perfect (mutate the existing material handle; restore by timer).

- [ ] Steps: staged test asserting the stamp text resource/entity content flips on a scripted Perfect and blanks after the timer (drive frames); implement; gates; commit `feat: contact stamp and zone flash teach the timing windows`.

---

### Task B5: Hit-stop + slow-mo + `JuiceDisabled`

**Files:**
- Create: `src/game/juice.rs` (new small module + plugin registered in GamePlugin's trailing group) — owns time-scale effects and the `JuiceDisabled` resource
- Modify: `tests/common/mod.rs` (harness inserts `JuiceDisabled`)

**Interfaces:**
- Consumes: `ContactEvent`.
- Produces: `pub struct JuiceDisabled;` (Resource). On Solid: 4 frames at `Time<Virtual>` relative_speed 0.05 then restore to 1.0. On Perfect: the freeze then 0.5 s (virtual) at 0.3, then restore. A watchdog system ALWAYS restores speed 1.0 when its frame/timer budget elapses regardless of state (never sticks), and on entering `Phase` result/reset. Skip everything when `JuiceDisabled` exists or `GameState != Playing`.

- [ ] Steps: unit-style app test (MinimalPlugins + the plugin) asserting relative_speed returns to 1.0 within the budget after a synthetic Perfect event, and that with JuiceDisabled inserted the speed never changes; implement; full suite green (harness now inserts JuiceDisabled — confirm zero e2e timing drift); gates; commit `feat: hit-stop and Perfect slow-mo behind a JuiceDisabled seam`.

---

### Task B6: Crowd + contact audio

**Files:**
- Modify: `src/game/audio.rs` (synthesis + event hookups only — follow the existing "synthesize at startup into in-memory WAVs, play off events" idiom; the `wav` feature note in CLAUDE.md)

**Interfaces:**
- Consumes: `ContactEvent`, existing outcome/banner events for strikeout/deep-fly hooks (read what audio.rs already listens to).
- Produces: three new synthesized assets — crowd murmur loop (played on game start, low volume, LOOP playback), roar swell one-shot (Perfect or deep-fly), groan one-shot (swinging strikeout = Whiff that ends an at-bat with K); bat-crack transient variants: sharper/louder for Perfect, standard Solid, dull FoulTip (parameterize the existing crack synth by quality). All respect `GlobalVolume` (they already do via PlaybackSettings).

- [ ] Steps: unit test asserting the new synth functions produce non-empty, clamped sample buffers of expected duration (pure-function tests, like any existing audio tests — check idiom); staged: fire synthetic ContactEvents headless and assert the corresponding `AudioPlayer` entities spawn (audio spawning is ECS-visible even windowless); implement; gates; commit `feat: synthesized crowd bed and quality-keyed contact audio`.

---

### Task B7: Balance-sim harness + tuning pass

**Files:**
- Create: `tests/balance_sim.rs` (+ shares `tests/common/mod.rs`)
- Possibly modify: `src/game/variant.rs` default tuning numbers ONLY (this is the sanctioned tuning site)

**Interfaces:**
- Consumes: the full game headless (CPU vs CPU 2P? — no: 1P has a human side. Check how e2e_cpu drives a CPU half-inning; extend the same driver so BOTH teams are CPU-driven via `Intents` for full games).
- Produces: `balance_bands_hold` — runs N=20 deterministic 1-inning games (varying a fixed seed offset per game through the AI noise seeds — e.g. advance virtual time differently per game start), accumulates K per batter-PA, runs/team/game, HR/team/game, and asserts: K% in 15..=30, runs/team/game in 3.0..=8.0 scaled for 1 inning (i.e. per-9 extrapolation: bands divided by 9 with generous slack — document the math), HR/team/game-per-9 in 0.5..=2.5. Also `#[ignore]`d `balance_sim_long` (N=100) for deep tuning.
- Tune `Ruleset` defaults until bands hold; every change is a one-line diff in variant.rs recorded in the report with before/after stats.

- [ ] Steps: write harness (RED likely on bands); tune; document final stats table in the report; full suite + gates; commit `feat: balance-sim harness pins the offensive economy`.

---

### Task B8: Home-run moment + docs

**Files:**
- Modify: `src/game/fx.rs` (fireworks: scale up the existing spark burst, triggered on HR outcome + `quality == Perfect` remembered from the contact), `src/game/camera.rs` (orbit the trot: during the Result phase of a HR, lerp the broadcast rig around the running batter — reuse the orbit math from CameraMode::Orbit with an automated azimuth sweep; restore normal framing at phase end), `src/game/audio.rs` (crowd peak = roar at higher gain)
- Modify: `CLAUDE.md` (one-paragraph batting-feel note: timing windows as Ruleset data, ContactEvent spine, JuiceDisabled harness seam, balance harness, V/S keys unchanged), `TADA.md` if TODO listed items land

**Interfaces:** consumes `ContactEvent` (stash last quality on `Play` in flow — flow may store `last_contact_quality: Option<ContactQuality>`, cleared per at-bat; fx/camera read it with the HR outcome).

- [ ] Steps: staged test — scripted HR (existing e2e_full_game has HR scripting? check; else script a Perfect max-exit pull) asserts the fireworks entities spawn and camera rig enters/exits the orbit without stalling RunnersSettled (the result pause already waits for the trot); implement; gates incl. wasm; docs; commit `feat: the home-run moment`.

---

## Self-check before final review

Full `cargo test` (now incl. balance_sim), both targets, clippy -D warnings, fmt; controller visual pass: stamp/flash/slow-mo/HR moment in-browser; balance stats table in the ledger.

## Out of scope (Plan C)

SwingMeter and PciCursor adapters (consume `Settings::batting_style`; Classic remains the only live style this plan — the settings rows exist but route every style to Classic until Plan C, with a code comment saying so at the routing site).
