# review-electron build progress

Git-durable mirror of the Progress log in `.plans/overnight-electron-build.md`.
Native Electron Fallow code-review app. Branch `feat/review-electron-app`,
stacked on `fix/review-quality` (engine E1-E8+E11).

## Status

- Phase 0 (baseline + data-source + sample app): DONE
- Phase 1 (W1 render model): DONE
- Phase 2 (Electron scaffold): DONE
- Phase 3 (W4-core walkthrough UI): DONE (vertical slice complete)
- Phase 4 (W3 agent feedback channel): DONE (acceptance proof owed to Phase 8)
- Phase 5 (W4-visual screenshot annotate): DONE
- Phase 6 (W5 grounded inspector): DONE (live flow e2e-verified in Phase 8)
- Phase 7 (W6 live-app annotate): DONE (live webview interaction e2e in Phase 8)
- Phase 8 (e2e tests): DONE (3/3 Playwright-electron; W3 acceptance still owed)
- Phase 9 (package + docs): DONE (signed macOS .app + README + STATUS)

## Log

- Worktree created @ ccaaed53f73; overnight build plan written; release CLI build
  started; sample Vite/React app fixture scaffolded.
- Phase 0 DONE: CLI built (2.100.0), review flags confirmed, audit-brief +
  walkthrough-guide fixtures captured, app manifest (package.json/tsconfig) added,
  deps install kicked off.
- deps installed (vite pinned ^5 for electron-vite peer); lockfile committed.
- Phase 1 DONE: W1 WalkthroughDocument + pure adapter (signal_id anti-hallucination
  drop); vitest 4/4 + tsc green.
- Phase 2 DONE: electron-vite scaffold (CJS), main+preload+renderer, IPC
  review:get -> runReview -> adapter; electron-vite build + tsc + vitest green.
- Phase 3 DONE: walkthrough UI (ReviewFocus/StageList/FileRow/ClearedPanel/
  DecisionList) + localStorage viewed-state; pure badge/viewed helpers tested;
  vitest 7/7 + tsc + build green. VERTICAL SLICE (0-3) complete.
- Phase 4 DONE: W3 feed channel (.fallow-review/feed.jsonl) + agent-walkthrough
  builder + review:validate IPC; vitest 10/10, tsc, build green. Live proof:
  unanchored -> rejected, stale hash -> rejected. Accepted-judgment proof owed to
  Phase 8 (decision-producing fixture).
- Phase 5 DONE: screenshot capture (capturePage) + AnnotateCanvas draw -> feed
  with imageRef; pure png/path helpers tested; vitest 13/13, tsc, build green.
- Phase 6a DONE: babel plugin stamps root-relative data-fallow-source on JSX
  (React-19-safe), source reader, factsForFile enrichment; @babel/core dep;
  vitest 21/21, tsc green. 6b (picker DOM + sample-app wiring + IPC + UI) next.
- Phase 6b DONE: picker (overlay + click -> localhost bridge), inspectServer
  (port 7787 + CORS), buildInspectorCard enrichment, inspect:selection -> UI
  InspectorCard, sample-app vite wiring (dev-only babel + picker). vitest 23/23,
  tsc, electron-vite build green. Live flow e2e in Phase 8.
- Phase 7 DONE: LiveApp <webview> live embed (webviewTag) + capturePage of the
  current state -> shared DrawableImage -> feed; right-region Live|Screenshot
  toggle. vitest 23/23, tsc, build green. sample-app deps installed.
- chore: stopped tracking out/ build output (gitignore).
- Phase 8 DONE: Playwright-electron e2e 3/3 (boot+shell; real-engine walkthrough
  load; inspector bridge -> card). Electron launches in-env. inspectServer
  hardened against port reuse. W3 accepted-judgment proof still owed.
- Phase 9 DONE: npm run package -> signed dist/mac-arm64/Fallow Review.app;
  README + worktree STATUS.md written. ALL PHASES BUILT. Loop stopping.
- Hardening pass: Electron security checklist applied (sandbox, contextIsolation,
  CSP on file://, setWindowOpenHandler deny, will-navigate block, will-attach-
  webview strip, permission deny-all, single-instance, ready-to-show,
  @electron-toolkit/utils). tsc + vitest + build + e2e 3/3 green under all of it.
- codiff-aligned tooling: oxlint (+ .oxlintrc, npm run lint, clean) + oxfmt
  (npm run format) + React Compiler (babel-plugin-react-compiler). 4 oxlint
  warnings fixed (toSorted, hoisted helper, addEventListener). tsc lib -> ES2023.
  oxlint + tsc + build + e2e 3/3 all green. Forge/Tailwind/multi-window skipped.

## POLISH ROUND (shadcn + diff views) - plan: .plans/polish-shadcn-diff.md
- P0 DONE: shadcn foundation. Tailwind v4 (@tailwindcss/vite) + zinc/neutral dark
  theme (copied from fallow-cloud feat/dashboard-shadcn, fonts @import dropped for
  offline+CSP) + cn util + components.json + `@` alias + dark default. CSS bundle
  emitted; tsc + build + oxlint + e2e 3/3 green. P1 = port screens to shadcn.
- P1 DONE: full shadcn port. Sidebar (App, ReviewFocus, StageList, FileRow,
  ClearedPanel, DecisionList, InspectorCard) + right region (AnnotateCanvas,
  LiveApp, DrawableImage) all on shadcn + Tailwind; theme.ts deleted; zero JSX
  inline styles. Conventions applied (lowercase, mono tabular-nums, Badge variants
  for signal). tsc + oxlint + build + e2e 3/3 green. Next: P2 codiff-style diff
  views (@pierre/diffs + git:diff IPC).
- P2 DONE: codiff-style diff views. diff:get IPC (git diff vs review base) +
  hand-rolled unified-diff parser (lib/diff.ts, tested) + shadcn DiffView (unified,
  mono, line-number gutters, fallow-green/red signal, +N/-N stats, binary
  fallback); clickable file row -> diff mode. e2e opens a diff (4/4). split/
  markdown/image/deferred = optional codiff parity, deferred. Next: P3 skills layer.
- P3 DONE: agent skills layer (codiff-style). backends registry (claude-code/codex/
  opencode) + buildAgentPrompt + extractAgentJson (pure, tested) + agentRun (guide
  -> spawn -> graph validation) + AgentPanel UI. runGuide returns digest+schemaShape.
  vitest 32, lint/typecheck/build/e2e 4/4 green. Next: P4 JSONC config.
- P4 DONE: JSONC config (~/.fallow-review/config.jsonc). config.ts (stripJsonc +
  parseConfig + loadConfig, pure, tested) + schema + example; main wires FALLOW_BIN
  + inspectPort + config:get + fs.watch hot-reload; renderer reads defaults
  (backend, url). vitest 35, lint/typecheck/build/e2e 4/4 green. Next: P5 design QA.
- P5 DONE: design QA. shots.e2e.ts captures 4 screens; round-1 critique + fixes
  (effort humanize, sidebar header divider, screenshot empty-state hint); re-shot.
  Ratings >= 8 all screens (walkthrough 8.5, diff 8.5, screenshot 8, live 8),
  consistent. lint/tsc/e2e 5/5 green. Next: P6 finalize.

## REDESIGN ROUND (user: "still ugly, go through it thoroughly")
- The minimal pass read as cramped/flat/colorless. Full visual overhaul: real
  header + toolbar bars (h-12), proper type scale + spacing rhythm, lucide icons
  throughout, colored verdict/risk/signal pills, an "agent review" card, file rows
  with file icons + dir/name split + hover + note-on-hover (status badges -> row
  dimming, not repeated chips), segmented mode control (diff/live/screenshot),
  diff toolbar shows path + stats, empty/loading states. Re-screenshotted +
  reviewed: now reads as a real product. e2e decoupled from copy (testids:
  review-loaded, file-open, mode-*); timeouts raised for parallel-build CPU
  contention. oxlint + tsc + build + e2e 5/5 green. (Stray app process holding the
  bridge port was the only test-4 flake; killed.)
- Disk: e2e binary now defaults to the worktree's own release build (rebuilt
  ~771M, isolated from the parallel-agent checkouts).

## POLISH LOOP (autonomous, taste-driven)
- Round 1 (live surface, was 4/10): the <webview> host had no background and no
  loading/error state, so an unreachable dev server (or a fresh remount) leaked a
  full white void into the dark app. Added a `loading|ready|failed` connection
  state machine driven by did-start-loading / did-finish-load / did-fail-load
  (main-frame + non-aborted only); dark `bg-background` host; centered loading
  overlay (spinner + "connecting to <url>") and a polished error overlay (unplug
  glyph in a ringed circle, "can't reach the app", server hint, retry button).
  Also fixed shots.e2e: was clicking nonexistent `mode-screenshot` (id is `shot`)
  so screen 03 never captured; added live-url / live-go / live-overlay testids and
  a 05-live-error capture that drives the failed state deterministically. Live now
  rates ~8.5 (loaded), error state ~8.5. format/lint/tsc/build/vitest/e2e 5/5 all
  green. Next weakest: screenshot-mode empty state (sparse void, one line of hint).
- Round 2 (screenshot/annotate surface, was 5/10): the empty state was a single
  text-[11px] line crammed top-left over a void. Rebuilt it on the same centered
  idle|capturing|error pattern as the live surface for cross-surface consistency:
  ringed Camera glyph + "annotate a screenshot" heading + muted explanation + a
  discoverable centered "screenshot this url" CTA; capturing -> spinner; error ->
  ImageOff glyph + message + retry. Phase state machine replaces the ad-hoc status
  string; added shot-url / shot-capture / shot-overlay testids. Screenshot empty
  state now ~8.5, visually matched to live. format/lint/tsc/build/vitest/e2e 5/5.
- Round 3 (file rows, cross-cutting, was 5/10): every row carried a full-width
  gray reason sentence ("isolated change, no blast beyond the diff", "high fan-in
  (17 importers)"), rendered as undifferentiated prose so a 17-importer hub looked
  identical to an isolated file. The reason is NOT boilerplate; it encodes the
  blast-radius signal. Refactored badges.ts -> deriveFileSignal (parses fan-in /
  fan-out / isolated from reason + score, graded muted|elevated|hub). Rebuilt
  FileRow as single-line: filename dominates, reason -> hover tooltip, a compact
  right-aligned mono tabular-nums fan-in metric (ArrowDownToLine + count) shown
  only for fanIn>=2, amber for hubs (>=6); security/risk-zone as red/amber icons;
  deprioritized rows dimmed. Verified amber grading via a 06-files-scrolled QA shot
  (walkthrough.ts ↓17 + agent.ts ↓7 in amber, ↓2/↓5 neutral, isolated files clean).
  Rows ~halved in height, far more scannable. format/lint/tsc/build/vitest/e2e 5/5.
- Round 4a (diff layout, was 6/10): the whole code line was tinted green/red, so
  large diffs read as a garish wash and the +/- sign was crammed inline with the
  code. Restructured rows to the standard GitHub/codiff layout: dual old/new
  line-number gutters, a dedicated colored sign gutter, readable foreground code,
  and a left-accent border + subtle tint as the add/del signal (green is now
  signal, not wash). Blue @@ hunk marker, row hover highlight, and a real
  loading-diff spinner state. format/lint/tsc/build/vitest/e2e 5/5. Next: 4b
  syntax highlighting (the code text is still monochrome vs codiff).
- Round 4b (diff syntax highlighting, was 6.5/10): code text was monochrome
  foreground vs codiff's highlighted source. Added a pure, zero-dep, per-line
  JS/TS/JSON tokenizer (lib/highlight.ts, tested: keyword/string/number/comment,
  exact round-trip, never throws on unterminated input) and rendered tokens with
  existing theme tokens only (violet chart-5 keywords, fallow-green strings,
  fallow-amber numbers, muted-italic comments) so no new palette is invented and
  the diff's green/red stays the add/del signal. Diff now reads like a real
  syntax-highlighted review surface. format/lint/tsc/build/vitest/e2e 5/5.
