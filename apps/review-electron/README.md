# Fallow Review (Electron)

A native desktop app for reviewing code changes (especially agent-authored ones),
grounded in Fallow's deterministic engine and fed back to a coding agent.

It turns `fallow review` output into a guided walkthrough (Review Focus, ordered
stages, per-file facts, a "cleared" panel), lets you screenshot / annotate the
running app, pick a component on screen to see its grounded facts, and routes all
of that back to the agent via a local feed.

## Run

```bash
npm install
npm run dev        # launch the Electron app (electron-vite dev)
npm run build      # build main + preload + renderer into out/
npm run package    # build + electron-builder (unsigned macOS .app, dir target)
npm test           # vitest unit tests
npm run test:e2e   # Playwright-electron e2e
npm run typecheck  # tsc --noEmit
```

Set `FALLOW_BIN` to a `fallow` binary if it is not on `PATH`:

```bash
FALLOW_BIN=../../target/release/fallow npm run dev
```

The app reviews the current working directory (`process.cwd()`); launch it from
the repo you want to review.

## Architecture

- **main/** Electron main process.
  - `review.ts` spawns `fallow review --format json` (and `--walkthrough-guide` /
    `--walkthrough-file`) and normalizes the output.
  - `inspectServer.ts` a localhost bridge (port 7787) the in-page picker POSTs to.
  - `inspect.ts` / `enrich.ts` join a selection to grounded facts.
  - `capture.ts` / `shots.ts` screenshot + annotated-shot persistence.
  - `feed.ts` the `.fallow-review/feed.jsonl` agent feed.
- **preload/** `contextBridge` exposing a typed `window.fallow` API.
- **model/** surface-agnostic types + the pure `toWalkthroughDocument` adapter
  (drops decisions lacking a Fallow `signal_id`: anti-hallucination).
- **renderer/** React UI: ReviewFocus, StageList/FileRow (+ badges), ClearedPanel,
  DecisionList, InspectorCard, DrawableImage, AnnotateCanvas, LiveApp.
- **inspector/** the grounded inspector:
  - `babelInspectorSource.ts` stamps root-relative `data-fallow-source` on JSX in
    dev (React-19-safe; same path-space as `fallow review` for a correct join).
  - `picker.ts` in-page overlay; click reads the source and POSTs to the bridge.
- **fixtures/sample-app/** a small Vite + React app as the inspect/annotate target.

## Data flow

`fallow review --format json` -> `toWalkthroughDocument` -> renderer walkthrough.
Picker click -> `data-fallow-source` -> bridge -> `buildInspectorCard` (facts from
the latest review) -> `inspect:selection` -> InspectorCard. Annotations + selections
-> `.fallow-review/feed.jsonl`; signal_id-anchored judgments validate via
`fallow review --walkthrough-file` (the verifier is the graph, not a second model).
