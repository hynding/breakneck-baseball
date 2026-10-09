#!/bin/bash
# SessionStart hook: provisions a Claude Code *cloud* container so `cargo
# check`/`test`/`clippy` and the wasm build all work. A fresh container ships
# only the native Rust toolchain: no Bevy system libraries, no wasm target, no
# wasm-bindgen, and a cold `target/`. Local (macOS) sessions exit immediately.
#
# Idempotent: every step checks before it installs, so a cached container
# re-runs this in a second or two.
set -euo pipefail

[ "${CLAUDE_CODE_REMOTE:-}" = "true" ] || exit 0
cd "${CLAUDE_PROJECT_DIR:-$(dirname "$0")/../..}"

log() { printf '[session-start] %s\n' "$*" >&2; }
SUDO=""
[ "$(id -u)" = 0 ] || SUDO="sudo"

# 1. Bevy's native audio/input/display backends probe these at build-script
#    time, so even `cargo check` needs them on Linux. Same list as ci.yml.
missing=""
for pkg in libasound2-dev libudev-dev libwayland-dev pkg-config; do
  dpkg -s "$pkg" >/dev/null 2>&1 || missing="$missing $pkg"
done
if [ -n "$missing" ]; then
  log "installing system packages:$missing"
  $SUDO apt-get update -qq
  # shellcheck disable=SC2086
  DEBIAN_FRONTEND=noninteractive $SUDO apt-get install -y -qq --no-install-recommends $missing >/dev/null
fi

# 2. The second compilation target and the lint/format components.
rustup target list --installed | grep -qx wasm32-unknown-unknown \
  || rustup target add wasm32-unknown-unknown
rustup component list --installed | grep -q '^clippy' || rustup component add clippy
rustup component list --installed | grep -q '^rustfmt' || rustup component add rustfmt

# 3. wasm-bindgen-cli must match the `wasm-bindgen` crate in Cargo.lock exactly
#    (see CLAUDE.md). Prebuilt release binary: seconds, versus minutes for
#    `cargo install`, which stays as the fallback.
want=$(awk '$0 == "name = \"wasm-bindgen\"" { getline; gsub(/version = |"/, ""); print; exit }' Cargo.lock)
have=$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)
if [ -n "$want" ] && [ "$want" != "$have" ]; then
  log "installing wasm-bindgen-cli $want (had: ${have:-none})"
  tmp=$(mktemp -d)
  url="https://github.com/wasm-bindgen/wasm-bindgen/releases/download/$want/wasm-bindgen-$want-x86_64-unknown-linux-musl.tar.gz"
  if curl -sSfL "$url" | tar xz -C "$tmp"; then
    mkdir -p "$HOME/.cargo/bin"
    install -m 755 "$tmp/wasm-bindgen-$want-x86_64-unknown-linux-musl/wasm-bindgen" "$HOME/.cargo/bin/"
  else
    cargo install wasm-bindgen-cli --version "$want" --locked
  fi
  rm -rf "$tmp"
fi

# 4. Line tables only for debug builds (same trade as ci.yml): panics keep
#    file:line, the debug tree shrinks several-fold, and the container's disk
#    allowance survives a full test build. User-level cargo config rather than
#    an env var, so every cargo invocation (Bash, the Stop hook's clippy, the
#    warm-up below) shares one fingerprint instead of rebuilding each other.
mkdir -p "$HOME/.cargo"
if ! grep -qs 'breakneck-session-start' "$HOME/.cargo/config.toml"; then
  cat >> "$HOME/.cargo/config.toml" <<'TOML'

# breakneck-session-start: cloud containers only (written by .claude/hooks/session-start.sh)
[profile.dev]
debug = "line-tables-only"
TOML
fi

# 5. Warm the build tree in the background so the first test run starts from
#    compiled dependencies. Detached: the session doesn't wait on it, and a
#    `cargo` command run meanwhile simply blocks on the build-directory lock
#    and then reuses the result. Progress: target/.session-warm.log.
mkdir -p target
if ! pgrep -f 'session-warm-build' >/dev/null 2>&1; then
  log "warming target/ in the background (tail -f target/.session-warm.log)"
  nohup setsid nice -n 10 bash -c '
    : session-warm-build
    cargo test --no-run && cargo clippy --all-targets
    echo "session-warm-build exit=$?"
  ' >target/.session-warm.log 2>&1 </dev/null &
fi
