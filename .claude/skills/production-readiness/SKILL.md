---
name: production-readiness
description: Use before shipping or announcing a release, when asked "is this ready to ship", "audit the web build", or "what's blocking release", and after any change to web/, .github/workflows/, or the wasm profile. Runs a web-first audit (GitHub Pages is the live target) across load size, robustness, input, performance, release process, and accessibility.
---

# Production Readiness Audit

Web-first: the shipping artifact is the GitHub Pages deploy of `web/`, and desktop-fine
problems (silent panics, autoplay gates, context loss, 30 MB downloads) are web-fatal.

The full checklist — every item with what / how / pass bar — is
`reference/checklist.md`. Work through it top to bottom; don't sample.

## Procedure

1. Build the real artifact: `cargo build --profile wasm-release --target wasm32-unknown-unknown`
   (+ wasm-bindgen per `/run-web`). Measure, don't estimate — sizes, load times, fps.
2. For browser items, use the Chrome DevTools MCP tools against the local serve (throttling,
   console, device emulation); confirm anything deploy-specific (compression headers) against
   the live Pages URL.
3. Static items (workflows, README, licenses, serde coverage) are file reads — cite the file
   and line in the finding.
4. Score each checklist item pass / fail / not-verifiable-this-session. Never mark a pass you
   didn't observe.

## Output

Append to `TODO.md` under `## Production readiness audit <YYYY-MM-DD>`, same numbering and
severity scheme as the playtest review (`ship-blocker` / `polish` / `nice`), ranked most
severe first, each finding naming the owning file/module. **Every `ship-blocker` gets a
one-paragraph proposed approach** — enough that the next session can start implementing
without re-deriving the plan.

This is an audit, not a fix session: file findings, change nothing (the sole exception:
adding the findings to `TODO.md`).
