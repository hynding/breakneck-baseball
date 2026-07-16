# Playable Baseball Game — Design

**Date:** 2026-07-16
**Status:** Approved scope, implementation pending

## Goal

Turn the current static skeleton (rendered field, physics ball, immobile
capsule players, orbital camera) into a **playable arcade baseball game** that
supports:

- **1 player vs CPU** — human controls the Home team, CPU controls the Away team.
- **2 players** — Player 1 = Home, Player 2 = Away, each on their own game
  controller (keyboard is the fallback when a controller is absent).

"Playable" means a complete game loop: pitch → swing (timing + aim) → ball in
play → automatic fielding → automatic base running → outs/hits/runs → count,
half-innings, and a final score with a game-over screen.

## Scope decisions (locked)

| Decision | Choice |
|----------|--------|
| Gameplay depth | **Arcade full game.** Human controls pitching (aim + pitch type + release) and batting (timing + aim). Fielding and base running are **automated** (arcade assist, RBI-Baseball style). |
| 2-player control split | **Fixed teams:** P1 = Home, P2 = Away. Whichever team is batting drives offense; the other drives defense. |
| Camera | **Gameplay camera** (behind-catcher for the pitch, auto-follows the ball in play) with a **toggle** back to the existing free orbital camera. |
| Innings | Regulation **9 innings**, defined as a constant so it is trivially adjustable. Walk-off and extra-innings tie-break rules applied. |

## Non-goals (YAGNI)

- Manual fielder control / manual throw-to-base selection.
- Manual base-runner control (steals, tag-ups, leadoffs).
- Player rosters, stats, batting order depth beyond a simple rotating lineup.
- Animations/skeletal models (capsules stay); art polish.
- Networked multiplayer, saves, difficulty menus.

## Architecture

The existing plugin/event/state seams are kept. New behavior is added as
systems and two new sub-plugins. Cross-module communication stays event-driven.

### New / changed modules

| Module | Responsibility |
|--------|----------------|
| `game/mod.rs` | Register new plugins; own new shared resources (`GameConfig`, `Controllers`, `PlayState`, `Bases`, `Lineup`). Replace the auto-`enter_playing` with menu-gated start. |
| `game/input.rs` (new) | Map each team's input source (Gamepad / Keyboard / CPU) into a per-team `TeamIntent` each frame. Handles controller detection & hotplug, keyboard fallback. |
| `game/menu.rs` (new) | Main-menu UI (title + "1 Player" / "2 Players" + live controller status) and the game-over screen (final score + restart). Drives `GameState` transitions. |
| `game/flow.rs` (new) | The at-bat / play state machine (`PlayState`), pitch-outcome and hit-outcome resolution, ball/strike/walk/strikeout logic, automatic base running, scoring, half-inning and game-over transitions, between-play result banners. |
| `game/player.rs` | Pitcher aim + release driven by `TeamIntent`; batter swing (timing window) driven by `TeamIntent`; CPU AI for both roles. Fielders get simple "move toward the ball's projected landing spot" visual movement. |
| `game/ball.rs` | Contact detection during the swing window; convert swing quality + aim + pitch into a `HitEvent`; expose ball landing/outcome helpers. Keep drag & reset. |
| `game/camera.rs` | Add gameplay camera modes (pitch view, ball-follow) and an orbit toggle key/button; keep orbital controls for the manual mode. |
| `game/ui.rs` | Extend HUD: base-runner diamond, batting team indicator, contextual control prompts, and transient result banners ("STRIKE!", "HOME RUN!", "OUT"). |

### Core data model

```rust
enum Team { Home, Away }

enum GameMode { OnePlayer, TwoPlayers }

struct GameConfig { mode: GameMode, innings: u32 }   // innings default = 9

enum InputSource { Gamepad(Entity), Keyboard, Cpu }

// Which source drives each team. In OnePlayer, Away = Cpu.
struct Controllers { home: InputSource, away: InputSource }

// Normalized per-team input, produced by input.rs each frame.
struct TeamIntent {
    aim: Vec2,        // left stick / WASD / arrows  (−1..1)
    action: bool,     // just-pressed primary button (pitch release / swing)
    action_held: bool,
    secondary: bool,  // pitch-type cycle / bunt, etc.
    toggle_cam: bool,
}

// The at-bat / play sub-state (lives inside GameState::Playing).
enum PlayState {
    PrePitch,     // defense aiming; offense set
    Pitch,        // ball traveling to plate; swing window open
    InPlay,       // ball hit; fielding + baserunning resolving
    Result,       // brief banner + reset (timer-driven)
}

struct Runner { team: Team }
struct Bases { first: Option<Runner>, second: Option<Runner>, third: Option<Runner> }

struct Lineup { home_spot: u8, away_spot: u8 }   // rotating 1..9 batter index
```

`ScoreBoard` (existing) keeps innings/half/balls/strikes/outs/runs. Which team
is at bat derives from `top_of_inning` (top = Away bats, bottom = Home bats).

### Control scheme

**Defense (team in field):**
- Left stick / arrows (or WASD) — aim where the pitch crosses the plate.
- Primary button (A / South, or Space) — throw the pitch.
- Secondary (X / West, or Shift) — cycle pitch type (fastball / changeup / breaking).

**Offense (team at bat):**
- Left stick / arrows (or WASD) — aim the swing (pull / center / opposite field).
- Primary button (A / South, or Space) — swing. Contact quality = how close the
  press is to the ball reaching the plate; a miss with the ball in the zone = strike.

**Global:** Start/Enter — pause & menu confirm; Select/Back or C — toggle camera.

Keyboard fallback: a single player on keyboard uses WASD + Space + Shift +
Enter + C. In 2-player keyboard-only (no controllers), P1 uses WASD/Space, P2
uses Arrows/Numpad-Enter — but the expected 2P setup is two controllers.

### Play-loop flow (`flow.rs` state machine)

```
PrePitch --(defense releases pitch)--> Pitch
Pitch:
  - ball travels; offense may swing inside the timing window
  - swing + good timing  -> emit HitEvent -> InPlay
  - swing + poor/no contact, ball in zone -> strike (++strikes)
  - no swing, ball out of zone -> ball (++balls)
  - 3 strikes -> strikeout (out); 4 balls -> walk (force-advance) ; then Result
Pitch --(swing connects)--> InPlay
InPlay:
  - compute outcome from launch vector/landing:
      foul            -> strike (unless 2 strikes -> stays 2) ; Result
      caught fly       -> out ; Result
      grounder fielded -> force logic: out or single ; Result
      gap / over fence -> single / double / triple / HR by distance ; Result
  - advance runners per hit value; runners crossing home -> runs
InPlay --(outcome resolved)--> Result
Result --(short timer + reset ball/positions)-->
  - 3 outs -> flip half-inning (reset count/outs/bases)
  - end of bottom of final inning (or walk-off / tie-break) -> GameState::GameOver
  - otherwise -> PrePitch (next batter)
```

### CPU AI (arcade, tunable)

- **Defense:** after a short delay, aims near the strike zone with jitter, cycles
  a random pitch type, and releases. Occasionally aims a ball on purpose.
- **Offense:** swings when the pitch is a likely strike, with timing noise so it
  sometimes whiffs/fouls; takes obvious balls. A `skill` scalar (0..1) controls
  jitter magnitude so difficulty is one knob.

### Error handling & edge cases

- **No controllers connected in 2P:** fall back to split keyboard; menu shows the
  live controller count so the player knows.
- **Controller unplugged mid-game:** that team's `InputSource` reverts to keyboard
  (or CPU in 1P) via `GamepadConnectionEvent`; game does not crash.
- **Ball reset:** existing out-of-bounds reset stays as a safety net; normal play
  resets happen in the `Result` phase.
- **Extra innings / walk-off:** if tied after `innings`, keep playing full extra
  innings; if Home leads after the top of/going into the bottom of the final (or
  extra) inning is decided, end immediately (walk-off).

## Testing / verification

No unit-test harness exists in the crate; gameplay is real-time and
physics-driven, so verification is primarily **build + interactive**:

1. `cargo check` and `cargo check --target wasm32-unknown-unknown` (dual-target
   gate from CLAUDE.md) after each milestone.
2. `cargo run` native smoke test: play a full half-inning by keyboard, confirm
   pitch/swing/hit/out/run/count/half-inning transitions and the game-over screen.
3. Controller test: connect one/two gamepads, confirm detection, P1/P2 mapping,
   and hotplug fallback.
4. `/run-web` skill: confirm it still builds and runs in the browser (gamepad via
   the browser Gamepad API through Bevy).
5. Pure-logic helpers (hit-value classification, runner advancement, count→result
   transitions) are written as free functions so they *can* get `#[cfg(test)]`
   unit tests; add a small `#[test]` module for runner-advancement math since that
   is the most error-prone piece.

## Build sequence (milestones)

1. **Input abstraction + menu**: `input.rs`, `menu.rs`, `GameConfig`/`Controllers`,
   menu-gated start, controller detection. (Game still just pitches, but from
   real per-team input and mode selection.)
2. **Pitch & swing duel**: pitcher aim/release + batter swing timing + contact →
   `HitEvent`; ball/strike/walk/strikeout; count HUD. (Playable batting duel.)
3. **Ball-in-play resolution + base running + scoring**: `flow.rs` outcomes,
   `Bases`, runs, half-inning/game-over flow, result banners, base diamond HUD.
4. **CPU AI**: defense and offense AI with a skill knob; wire 1P mode.
5. **Camera + polish**: gameplay camera modes, orbit toggle, control-prompt HUD,
   fielder "move to ball" visual, tuning pass. Dual-target + web verification.
