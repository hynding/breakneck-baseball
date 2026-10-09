#!/bin/bash
# Stop hook: lint what this turn changed, and hand any clippy warning back to
# Claude before the turn ends. CI runs `clippy --all-targets -D warnings`, so a
# warning left here is a red CI run later.
#
# - Skips entirely unless a .rs file under src/ or tests/ is newer than the last
#   clean run (question-only turns cost nothing).
# - Skips while the session-start warm build holds the cargo lock, and gives up
#   gracefully on a cold tree rather than being killed at the hook timeout.
# - Blocks the stop at most once per turn (`stop_hook_active`), so an
#   unfixable warning can't loop.
set -uo pipefail
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH" # macOS rustup lives outside PATH
cd "${CLAUDE_PROJECT_DIR:-.}" || exit 0

input=$(cat)
already_blocked=$(printf '%s' "$input" | jq -r '.stop_hook_active // false' 2>/dev/null)

stamp=target/.claude-clippy-clean
if [ -f "$stamp" ] && [ -z "$(find src tests -name '*.rs' -newer "$stamp" -print -quit 2>/dev/null)" ]; then
  exit 0
fi
warm_pid=$(cat target/.session-warm.pid 2>/dev/null)
if [ -n "$warm_pid" ] && grep -qa session-warm "/proc/$warm_pid/cmdline" 2>/dev/null; then
  echo "clippy skipped: the session-start warm build is still running (target/.session-warm.log)"
  exit 0
fi

mkdir -p target
started=$(mktemp target/.claude-clippy.XXXXXX)
out=$(timeout 150 cargo clippy --all-targets --message-format short 2>&1)
status=$?
if [ "$status" = 124 ]; then
  rm -f "$started"
  echo "clippy timed out (cold build?) — run \`cargo clippy --all-targets\` before committing"
  exit 0
fi
findings=$(printf '%s\n' "$out" | grep -E '(^|: )(warning|error)(\[|:)' \
  | grep -v 'warning: the following packages contain code' \
  | grep -vE '^(warning|error): .*generated [0-9]+ (warning|error)' | head -20)

if [ -z "$findings" ] && [ "$status" = 0 ]; then
  mv "$started" "$stamp" # its mtime is when this run began
  exit 0
fi
rm -f "$started"
[ -n "$findings" ] || findings=$(printf '%s\n' "$out" | tail -12)
if [ "$already_blocked" = "true" ]; then
  printf 'clippy (still failing):\n%s\n' "$findings"
  exit 0
fi
jq -n --arg r "cargo clippy --all-targets reports issues (CI denies warnings). Fix them, or say why not:
$findings" '{decision: "block", reason: $r}'
