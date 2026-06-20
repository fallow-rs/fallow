# Fallow Review (Electron): overnight build STATUS

Branch `feat/review-electron-app`, stacked on `fix/review-quality` (the agentic
review engine E1-E8+E11). Local only: not pushed, no PR. App lives in
`apps/review-electron/`.

A native Electron app to review code changes (especially agent-authored), grounded
in Fallow's deterministic engine and fed back to a coding agent.

## How to run

```bash
cd apps/review-electron
npm install
FALLOW_BIN=../../target/release/fallow npm run dev   # launch the app
```

A packaged, signed macOS app was also produced at
`apps/review-electron/dist/mac-arm64/Fallow Review.app` (gitignored).
Launch it from the repo you want to review (it reviews `process.cwd()`).

## What works (verified)

- Unit tests pass (vitest): adapter normalization + anti-hallucination, badges,
  viewed-state, png decode, agent-walkthrough builder, fact enrichment,
  inspector-card, babel source-stamp.
- Type-check clean (tsc), electron-vite build clean.
- Playwright-electron e2e pass: the app boots and renders; "Load review" runs the
  REAL engine and the grounded walkthrough renders; the inspector bridge round-trip
  (selection to grounded facts to InspectorCard) works in the running app.
- electron-builder produces a runnable macOS `.app`.
- Live agent round-trip proven against the engine: an unanchored `signal_id` is
  rejected (`unanchored-signal-id`); a stale graph hash is rejected (`stale`).

## Phases (all built; per-phase detail in apps/review-electron/PROGRESS.md)

0 baseline + sample app · 1 W1 render model · 2 Electron scaffold · 3 walkthrough
UI + viewed-state (vertical slice) · 4 W3 agent feed channel + validation · 5
screenshot + canvas annotation · 6 grounded inspector (source-stamp + picker +
bridge + card) · 7 live-app webview embed + live-state annotation · 8
Playwright-electron e2e · 9 package + docs.

## Decisions taken

- Electron, per your choice. This deliberately skipped the W0 GUI go/no-go gate
  and the W6 Tauri-vs-Electron gate from the W-series plan (the plan's default was
  local-first / Tauri). Easy to revisit: the engine, model, and renderer are
  surface-agnostic; only `src/main` + the webview embed are Electron-specific.
- CommonJS app (not `type: module`) to avoid ESM-preload pitfalls.
- React 19 dropped fiber `_debugSource`, so the inspector uses a Babel plugin that
  stamps root-relative `data-fallow-source` (locatorjs-style), kept in the same
  path-space as `fallow review` output for a correct join.

## Pending / owed (honest)

- W3 ACCEPTED-judgment proof: needs a decision-producing fixture (the current diff
  emits zero `signal_id`s, so only rejection is demonstrable here). Rejection is
  e2e/CLI-proven; the builder is unit-tested.
- Deeper GUI-interaction e2e (live webview picker click; screenshot freehand draw)
  is covered by unit tests + the bridge e2e, not by full GUI driving.
- Inspector card shows `file:line` (+ optional component); component-name inference
  from the DOM is not wired (the picker sends file:line:col).
- Not pushed; no PR (your call on landing).
