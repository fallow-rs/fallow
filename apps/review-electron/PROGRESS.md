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
- Round 5 (annotate drawing tools, was 5/10): the drawing surface was red-only
  with no undo/clear, drawing directly onto the canvas (no stroke model). Rebuilt
  DrawableImage around a stroke model (base image kept in a ref, strokes replayed
  on redraw): a signal-palette pen picker (red/amber/green/blue swatches, selected
  ring), undo (pop last stroke), and clear, all above the canvas; "save annotation"
  -> "send to agent". Zero inline styles (Tailwind bg-fallow-* swatches + resolved
  stroke colors). Added 07-inspector (bridge POST) + 08-annotate QA captures so the
  inspector card and drawing toolbar are now screenshot-verified. e2e green 5/5.
- Round 6 (diff path de-dup, MED): in diff mode the file path rendered twice (the
  main mode toolbar AND DiffView's own sticky header). Removed it from the main
  toolbar so DiffView owns the path + stats header, matching live/shot where each
  sub-component owns its own toolbar. Main mode bar is now just the segmented
  control across all three modes. format/lint/tsc/build/e2e 5/5.
- Round 7 (segmented-control consistency, MED): the AgentPanel backend picker
  (claude code / codex / opencode) used a different visual pattern (separate pills,
  bg-primary active) than the diff/live/screenshot mode control (unified bg-muted
  container, bg-background active) for the same pick-one interaction. Unified the
  backend picker to the segmented-control treatment so both read identically.
  format/lint/tsc/build/e2e 5/5.
- Round 8 (review header hierarchy, MED): the agent-review CTA card sat ABOVE the
  verdict, so the most important fact (pass/fail + risk) was visually subordinate
  to a secondary action, and the stats were a run-on muted sentence. Reordered so
  ReviewFocus leads the sidebar when a review is loaded (agent card drops to
  secondary), and restyled the summary into a cloud-dashboard KPI-style stat strip:
  verdict pill + commit hash on one row, then aligned files / risk / effort cells
  (uppercase muted labels over mono values, risk color-toned). Verdict pill is now
  semibold. The review state reads as the anchor. format/lint/tsc/build/vitest/e2e 5/5.
- Round 9 (keyboard focus states, MED a11y): the custom <button> elements (mode
  control, agent backend picker, cleared-panel toggle, file-open row, annotation
  pen swatches) had no visible keyboard-focus indicator, only the shadcn Button /
  Checkbox primitives did. Added a consistent focus-visible:ring-2 ring-ring/60
  (outline-none) to all of them, matching the shadcn focus pattern. Added a
  10-focus QA capture that presses Tab (to enter keyboard modality so
  :focus-visible matches) then focuses a file row; ring confirmed rendering.
  format/lint/tsc/build/vitest/e2e 5/5.
- Round 10 (review loading state, MED): during the multi-second review the sidebar
  showed the "load a review to see what to look at first" empty prompt, which
  contradicts the in-flight load (header already says "reviewing"). Split the
  not-loaded branch into loading vs empty: loading now shows a centered spinner +
  "running fallow review…" + a descriptive hint. Added an 11-loading capture
  (screenshots the state before the real review resolves). format/lint/build/e2e 5/5.
- Round 11 (cleared-panel breakdown, MED): the expanded "fallow handled N" list
  rendered each row as a left-aligned "count label" pair, against the design
  language's right-aligned counts. Reflowed to an aligned mini-table: label left
  (indented under the header), count right in mono tabular-nums. Added a
  cleared-toggle testid + 12-cleared capture; verified the breakdown (dead-code 61,
  duplication 3, complexity 13) reads as a clean column. format/lint/tsc/build/vitest/e2e 5/5.
- Round 12 (file-row path readability, HIGH): deeply-nested rows showed the full
  path (apps/review-electron/src/renderer/src/components/...) which (a) truncated
  the FILENAME, the most important part, and (b) duplicated the stage-group header
  that already shows the module dir. FileRow now takes baseDir (the stage dir),
  strips that prefix, and renders the residual dir as the only shrinkable span so
  the filename never truncates; full path + reason move to the hover title.
  Result: rows show clean filenames (App.tsx, walkthrough.ts ↓17) grouped under
  their module header, hub metrics aligned right. format/lint/tsc/build/vitest/e2e 5/5.
- Round 13 (review error state, MED): a failed `fallow review` showed a cramped
  red box pinned to the top of the sidebar while the "load a review" empty prompt
  still rendered below it. Promoted error to a first-class centered state (red
  TriangleAlert in a tinted ring, "review failed", the actual message, retry
  button) mutually exclusive with loading/empty, matching the live/screenshot error
  pattern. Added a second shots test that launches with a bad FALLOW_BIN to capture
  13-review-error deterministically. format/lint/tsc/build/vitest/e2e (6 e2e) green.
- Round 14 (diff empty/error states, MED consistency): the DiffView "no textual
  diff" and load-error states were plain top-left text, the last surfaces not on
  the centered empty/error pattern. Reworked both to centered cards (FileX for
  no-diff, red TriangleAlert for error) matching live/screenshot/review. Not
  screenshot-triggerable in an all-additions review, but identical to five already
  verified centered states; the happy diff path re-captured and the e2e text
  selectors (/@@|no textual diff/) still match. format/lint/tsc/build/vitest/e2e green.
- Round 15 (agent-panel status signal, MED): the agent run result rendered both
  success and "error: ..." in identical muted text, against color-only-as-signal.
  Modeled status as a typed running|ok|error value rendered with a tone + icon
  (muted spinner / green check / red triangle). Not screenshot-captured (running
  it spawns a real agent backend, too flaky for e2e); existing 6 e2e prove no
  regression and it reuses the verified signal palette. format/lint/tsc/build/vitest/e2e green.
- Round 16 (inspector bullet color discipline, MED): the InspectorCard rendered a
  fallow-blue bullet on every fact, decorative color (identical marker, no signal)
  against color-only-as-signal. Muted the bullets (text-muted-foreground/50) so the
  sidebar's only colors are genuine signals (verdict/risk red, cleared-check green,
  fan-in amber). Verified via 07-inspector. format/lint/tsc/build/e2e green.
- Round 17 (purposeful brand mark, MED): the header and load-empty state used a
  generic Sparkles (AI-cliche, decorative) against "lucide icons used
  purposefully". Swapped to Telescope, which reads as look-ahead / focus on what
  matters and pairs with the "see what to look at first" copy. Verified via
  11-loading header. format/lint/tsc/build/vitest/e2e green.
- CLEAN PASS 1/2: harsh full-screen evaluation over all 13 captured states
  (walkthrough, diff unified+highlighted, diff empty/error, inspector, live
  loaded/loading/error, screenshot/annotate + drawing tools, focus ring, review
  loading, cleared breakdown, review error). No HIGH/MED visual issues found; the
  only remaining nit (the live/screenshot "go" button's refresh icon) is LOW and
  defensible (it reloads the typed URL). No code changes this pass. One more clean
  pass confirms the stop condition.
- CLEAN PASS 2/2: independent harsh re-evaluation (annotate drawing tools, focus
  ring, live error, inspector, verdict header, file rows, diff). No HIGH/MED
  issues. Two consecutive clean passes reached. No code changes this pass.

## STATUS: polish loop complete (2026-06-21)
Stop condition met: two consecutive full-screen passes with zero HIGH/MED visual
issues. 18 polish chunks shipped this loop, each signed and gated
(oxfmt + oxlint + tsc + electron-vite build + vitest + Playwright e2e green):
- live + screenshot + diff + review surfaces now share ONE centered
  empty/loading/error language (no more white webview void, no plain top-left text)
- file rows: stage-dir prefix stripped (filenames never truncate), fan-in blast-
  radius surfaced as a graded mono metric (amber hubs), single-line + de-noised
- diff: GitHub/codiff layout (sign gutter, dual line numbers, readable code) plus
  a pure zero-dep JS/TS syntax highlighter; duplicate path removed
- sidebar leads with the verdict (KPI stat strip), agent backend picker unified as
  a segmented control, cleared breakdown aligned as a count table, agent-run status
  toned as a signal
- annotation tools rebuilt (pen colors, undo, clear); keyboard-focus rings on every
  custom button; purposeful Telescope brand mark; color reserved for signal only
QA coverage grew from 4 to 13 screenshot states (added live-error, annotate,
diff-mixed, focus, loading, cleared, review-error). e2e 3 -> 6.
Only remaining nit is LOW (the live/screenshot "go" button reuses a refresh icon,
which is defensible since it reloads the typed URL). Loop stopped; not rescheduled.

## FOLLOW-UP: native-quality error messages (user-reported)
The review-error state still showed the raw IPC string ("Error invoking remote
method 'review:get': Error: spawn fallow ENOENT") -- the round-13 work fixed the
error LAYOUT but not the message CONTENT. Fixed both layers, native-app style:
- main/errors.ts (pure, tested): describeExecError maps ENOENT/EACCES/non-zero-exit
  from spawn/exec into clean copy ("Couldn't find the \"fallow\" binary. Set
  FALLOW_BIN or add fallow to your PATH."); describeLoadError maps Chromium
  ERR_CONNECTION_REFUSED / ERR_NAME_NOT_RESOLVED / other ERR_* into human text.
  Wired into review.ts (all 3 fallow exec calls + a JSON-parse guard), capture.ts
  (loadURL), agentRun.ts (agent spawn).
- renderer lib/errors.ts (pure, tested): errorMessage() strips Electron's
  "invoking remote method '...': Error:" wrapper; used by App (review), DiffView,
  AnnotateCanvas, LiveApp. Agent-run errors already return a clean string.
Verified via 13-review-error: now "review failed" + "Couldn't find the
\"fallow-bin\" binary. Make sure ... PATH." + retry. vitest 51, lint/tsc/build/e2e 6
green. (The user's screenshot was a stale build with the pre-round-13 top-bar
layout; current build is the centered state with the clean message.)

## LOOP RESTART (user-requested)
- Round 18 (verify error copy end-to-end): added a 14-shot-error capture that
  drives a screenshot capture against an unreachable URL. Confirms describeLoadError
  + errorMessage render cleanly in the UI: "couldn't capture" + "Couldn't load
  http://localhost:1 (unsafe port)." + retry, no IPC/Chromium noise. Both error
  surfaces (13-review-error, 14-shot-error) now screenshot-verified.
  format/lint/tsc/build/vitest (51) /e2e (6) green. Walkthrough + screenshot empty
  re-reviewed: no HIGH/MED.
- Round 19 (impact-first file ordering, HIGH; user-reported): the sidebar rendered
  files in the engine's module-grouped `partition.order`, which put low-impact
  config/e2e/fixtures at the TOP and buried the real hubs (src/main, src/model)
  lower. Root cause: the engine returns TWO orderings, and the app used the wrong
  one. `direction.units` IS ranked by impact (inspect.ts budget 12, backends 11,
  ...), but `partition` (module grouping) is not, and the adapter sorted stages by
  partition.order. Fixed the adapter to order stages by max attention desc (tie =
  engine sequence) and files within a stage by attention desc, re-applying the
  impact ranking the engine already computes. Now src/main leads with
  inspect.ts/backends.ts on top. New adapter test locks it in. vitest 51,
  format/lint/tsc/build/e2e (6) green.
- Round 20 (fix the ordering inversion round 19 introduced, MED/HIGH): sorting by
  the engine's attention total (a capped fan_io, e.g. 10) while the row DISPLAYS
  raw importers (e.g. 17) produced a non-monotonic down-N column: walkthrough.ts
  (17 importers) ranked below inspect.ts (5), and 3-importer files sat below
  2-importer ones within a group. Fixed the sort key to match the displayed
  signal: security taint > risk zone > fan-in (importers) > attention, applied to
  files-within-stage and to stage order (stage = max rank of its files). Extracted
  parseFanInOut into the model so the adapter (sort) and badges (display) share one
  parser; hoisted rankOf / compareRankDesc / byRankDesc / maxRank to module scope
  (oxlint clean). The down-N column is now monotonic and the biggest hubs lead in
  amber. New adapter test asserts fan-in beats the capped total. vitest 52,
  format/lint/tsc/build/e2e (6) green.
- Round 21 (theme-matched scrollbars, MED; user-reported): Chromium's chunky light
  default scrollbars clashed with the dark app on the sidebar and diff. Styled
  ::-webkit-scrollbar in global.css: a thin (10px, ~6px visible) rounded thumb at
  color-mix(muted-foreground 22%) floating via a transparent border +
  background-clip:padding-box, brightening to 42% on hover, over a transparent
  track. The bars now recede into the zinc-dark theme. format/lint/tsc/build/e2e green.
