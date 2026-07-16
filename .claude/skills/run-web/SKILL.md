---
name: run-web
description: Use when building, serving, or testing the browser/WASM version of Breakneck Baseball — including "run the game in the browser", verifying a change works on the web target, or debugging wasm-bindgen version mismatch errors.
---

# Run Breakneck Baseball in the Browser

## Overview

Builds the wasm32 target, generates JS bindings, and serves `web/` locally. The wasm-bindgen CLI version MUST match the `wasm-bindgen` crate version in `Cargo.lock` — a mismatch fails at bindgen time with a "schema version" error.

## Workflow

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"

# 1. Build (debug ≈ 2 min cold; use --profile wasm-release for a small binary)
cargo build --target wasm32-unknown-unknown

# 2. Generate bindings into web/out/
wasm-bindgen --out-dir web/out --target web \
  target/wasm32-unknown-unknown/debug/breakneck-baseball.wasm

# 3. Serve (run in background) and open http://localhost:8080
python3 -m http.server --directory web 8080
```

For `--profile wasm-release`, the wasm path changes to `target/wasm32-unknown-unknown/wasm-release/`.

## Verifying it runs

Use the Chrome DevTools MCP tools: navigate to `http://localhost:8080`, wait for the canvas, and check `list_console_messages` for panics. A debug wasm is ~85 MB, so allow generous load time; wasm-release is far smaller.

## Common Mistakes

- **wasm-bindgen version mismatch**: after `cargo update`, reinstall with `cargo binstall wasm-bindgen-cli --version <Cargo.lock version> -y`.
- **Serving the wrong directory**: serve `web/` (contains `index.html`), not `web/out/`.
- **Port already in use**: a previous server may still be running; kill it or pick another port.
