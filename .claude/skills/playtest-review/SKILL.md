---
name: playtest-review
description: Use when asked to "review the game", "what should I work on next", "how does it look/play", before any release, or after changes under src/game/present/. Drives the browser build through a fixed moment list, scores each moment on a rubric, and produces a ranked TODO.md work queue with severity tags.
---

# Playtest Review

Produces the user's work queue: a ranked, severity-tagged findings list in `TODO.md`. Not a
code review — this evaluates what a player *sees*. Findings point at the owning module; bugs
found along the way are filed, never fixed mid-review.

## Capture procedure

1. Build & serve the web target via the `/run-web` skill — `--profile wasm-release` for a
   release-representative pass (debug build acceptable for a quick pass; say which you used).
2. Load `http://localhost:8080` in the browser (Chrome DevTools MCP tools). Allow generous
   load time; check `list_console_messages` for panics/warnings **at every moment below**.
3. Stage moments by playing (menu keys: I innings, T theme, S settings, V duel view,
   Esc/P pause) or via the debug build's Scenario tab (`cargo run --features "dev debug"`,
   F1 → Scenario), which applies `sim/scenario.rs` presets: bases-loaded full count, DP setup
   R1, steal duel R1, tag-up R3, dropped-third, walk-off bottom 9. The Scenario tab is native
   only — on web, stage by playing.
4. Screenshot each moment to `docs/agent/playtest/<date>/<nn>-<moment>.png`.
5. If browser tooling is unavailable: capture natively, or fall back to a **static code review
   against the rubric — and label the output as such** in TODO.md.

## Moment list

Capture all of these; skip only with a stated reason.

1. Main menu (theme cycle T at least once)
2. Settings screen (S)
3. First pitch from catcher POV (default duel view)
4. Each of the four `DuelView` framings (V): catcher POV / behind-pitcher / batting zoom / broadcast plate
5. A Perfect contact — hit-stop + slow-mo tail (watch for it; `ContactEvent` fires regardless)
6. A fly ball with the landing ring up
7. A home run: trot, fireworks, orbit camera
8. A force play at second (stage: DP setup R1, ground ball)
9. A pickoff attempt (stage: steal duel R1; hold Down as offense, action as defense)
10. The pause/substitution board (Esc/P while ball dead)
11. GAME OVER (1-inning game gets there fastest — menu key I)

## Rubric

Score each moment 1–5 per dimension; note what would make it a 5.

| Dimension | 5 means |
|---|---|
| Readability | every actor and the ball are findable at typical laptop distance |
| Camera intent | the cut/framing explains the play — you can tell what mattered |
| Outcome clarity | a new player could say *why* it was an out/safe/foul |
| HUD legibility | corner boxes readable, hierarchy right, nothing overlaps the action |
| Timing / dead air | pauses feel deliberate; no unexplained waits between plays |
| Theme consistency | colours/materials match the active `Theme`; no hardcoded strays |
| Audio cues (event check) | expected events fired for the moment (crowd/crack/pop) — you can't hear; verify events fire (console/log or code path), and note it as unheard |

## Output format

Append to `TODO.md` under `## Playtest review <YYYY-MM-DD>`, numbered continuing TADA.md's
style (`N. [ ] <finding>`), ranked most severe first. Each item:

```
N. [ ] <severity: ship-blocker|polish|nice> <dimension> — <finding, one sentence>.
   Screenshot: docs/agent/playtest/<date>/<file>. Proposed fix: <one line, naming the owning module>.
```

Severity: `ship-blocker` = a new player is confused or the game looks broken; `polish` =
noticeably rough but playable; `nice` = would elevate, not required. File bugs discovered
along the way as items too — do not fix anything during a review pass.
