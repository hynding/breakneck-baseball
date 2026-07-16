# breakneck-baseball

A 3-D baseball game built in Rust using **Bevy** (game engine / wgpu rendering) and **Rapier** (physics).

## Architecture

```
src/
├── main.rs            — App entry-point; registers all plugins
└── game/
    ├── mod.rs         — GamePlugin, GameState machine, ScoreBoard resource
    ├── ball.rs        — Baseball entity, Rapier physics, PitchEvent / HitEvent
    ├── camera.rs      — Orbital stadium camera with keyboard/scroll controls
    ├── field.rs       — Baseball field geometry (diamond, bases, mound, foul poles)
    ├── player.rs      — Pitcher, Batter, Fielder components and spawn logic
    └── ui.rs          — Score-board HUD and controls hint
```

## Dependencies

| Crate | Role |
|---|---|
| `bevy` 0.15 | Game engine (ECS, windowing, **wgpu** rendering, asset loading, input) |
| `bevy_rapier3d` 0.28 | 3-D rigid-body physics (pitches, hits, ball bounces) |

## Building

### Desktop (native)

```sh
cargo build
cargo run
```

> **Linux prerequisite:** `libasound2-dev` and `libudev-dev` must be installed for Bevy's audio/input backends.

### Web (WASM)

Play the game in a browser — no installation required for end-users.

**One-time setup:**

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli
```

**Build and serve:**

```sh
cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --out-dir web/out --target web target/wasm32-unknown-unknown/release/breakneck-baseball.wasm

# Serve the web/ directory with any static file server, e.g.:
python3 -m http.server --directory web 8080
# Then open http://localhost:8080 in your browser.
```

> **Tip:** Use the `wasm-release` profile for a smaller download:
> `cargo build --profile wasm-release --target wasm32-unknown-unknown`

## Controls (in-game)

| Key | Action |
|---|---|
| `Space` | Throw a fastball (~40 m/s ≈ 90 mph) |
| `WASD` / arrow keys | Orbit the camera |
| `Q` / `E` | Zoom out / in |
| `R` | Reset camera to default position |
| Scroll wheel | Zoom |
