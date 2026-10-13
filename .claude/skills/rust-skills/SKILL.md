---
name: rust-skills
description: Rust code-review and refactoring guidelines (265 rules, loaded one rule file at a time). Use when reviewing or refactoring Rust, writing `unsafe`, optimizing a hot path, or designing a new public type/API in this crate. Not needed for routine edits that follow the surrounding code. Invoke with /rust-skills.
---

# Rust guidelines (project index)

The full upstream skill is vendored at `.agents/skills/rust-skills/` (pinned by
`skills-lock.json`; never edit it — the lock hashes it). Its own `SKILL.md` is a
~38 KB index; this file replaces it as the entry point so a Rust task doesn't
pay for 265 rule summaries up front. **Read individual rule files on demand:**
`.agents/skills/rust-skills/rules/<rule-id>.md`.

## Where this crate departs from the generic rules

A Bevy game, not a library — these generic rules do **not** apply here:

- `err-thiserror-lib` / `err-anyhow-app` / `err-context-chain` — systems don't
  return errors; failure paths log and fall back (`settings::load_settings` is the model).
- `async-*`, `conc-*`, `own-arc-shared`, `own-mutex-interior` — no async runtime;
  sharing goes through ECS resources/components, not locks.
- `doc-all-public`, `lint-missing-docs`, `api-*` — the facade (`game::<module>`)
  is internal; document *why*, as the surrounding code does.
- `test-integration-dir`, `test-cfg-test-module` — this repo's own test layout
  wins: unit tests in sibling `<name>.test.rs`, integration in `tests/e2e/` (CLAUDE.md).
- `opt-*` / `perf-release-profile` — profiles are tuned in `Cargo.toml`
  (`wasm-release` is size-first); use the `bevy-perf` skill for runtime work.

## The rules that earn their keep here

| Task | Rule files to read |
|---|---|
| Per-frame system code | `anti-format-hot-path`, `mem-avoid-format`, `mem-reuse-collections`, `perf-iter-lazy`, `anti-collect-intermediate`, `own-borrow-over-clone` |
| Game-state modeling | `type-enum-states`, `type-newtype-ids`, `pat-exhaustive-enum`, `pat-let-else`, `type-no-stringly` |
| Float math (physics, timing windows) | `num-float-compare`, `num-saturating-clamp`, `num-cast-try-from`, `num-overflow-explicit` |
| Panics vs fallbacks | `err-no-unwrap-prod`, `err-expect-bugs-only`, `anti-unwrap-abuse`, `anti-panic-expected` |
| Settings / `.ron` / serde | `serde-default-compat`, `serde-try-from-validate`, `serde-rename-all` |
| `unsafe` (only the settings env seam today) | `unsafe-minimize-scope`, `unsafe-safety-comment` |
| Reviews | `anti-*` (15 files), `lint-warn-suspicious`, `lint-warn-perf` |

For anything else, `ls .agents/skills/rust-skills/rules/ | grep <prefix>-` —
prefixes: `own err mem unsafe api async conc opt num type trait conv const serde
pat macro closure coll name test doc obs perf proj lint anti`.
