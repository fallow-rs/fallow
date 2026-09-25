# LSP internals

Use this reference for diagnostics, code actions, code lenses, hover, and LSP
lifecycle behavior.

## Ownership

- `crates/lsp/src/main.rs` is a thin binary delegator.
- `crates/lsp/src/lib.rs` owns the language server and request lifecycle.
- `crates/lsp/src/analysis.rs` calls the shared editor API and assembles an LSP
  snapshot.
- `crates/api/src/editor.rs` is the editor-facing analysis facade.
- `crates/lsp/src/diagnostics/` maps issue families to diagnostics.
- `crates/lsp/src/code_actions/`, `code_lens.rs`, and `hover.rs` own their
  protocol features.
- `crates/lsp/src/protocol.rs` owns Fallow-specific notifications and issue
  metadata projection.
- `crates/lsp/src/server_capabilities.rs` is the source of truth for advertised
  capabilities.
- `crates/types/src/issue_meta.rs` owns the shared issue catalogue.

## Invariants

- Keep analysis and fix semantics in shared APIs. The LSP adapts typed results
  to protocol objects.
- Convert paths through the LSP path helpers. Never construct document URIs by
  string concatenation.
- Publish only results that still match the current document version.
- Push and pull diagnostic clients must receive one coherent diagnostic set,
  including clears for stale findings.
- `publish.rs` decides what a run sends, and the server and the
  `lsp_save_publish` lab bench share it. A run skips a URI when its filtered
  diagnostics and its document version equal the pull-cache entry. The
  `workspace/diagnostic/refresh` request goes out only when the cache changed.
  `didClose` marks the cache entry of the URI as not pushed, because the
  server clears the push diagnostics of an open document for a pull client.
  The next run then pushes the diagnostics of the closed file again.
- Diagnostics keep stable codes, `source: "fallow"`, actionable messages, and
  project-relative evidence where appropriate.
- `initializationOptions.mutedCategories` accepts exact diagnostic codes from
  the shared issue catalogue. Known string codes are unioned with diagnostics
  disabled through `issueTypes: false`; unknown and non-string entries are
  ignored so older servers remain permissive with newer clients.
- Initialization options are read once during `initialize`. Clients must
  restart the language server after changing `mutedCategories` or
  `health.inlineComplexity`.
- `health.inlineComplexity` is opt-in and supplies threshold-exceeding function
  complexity to Code Lens. It is not a project Health report or an editor-owned
  Health view. A lens applies `health.thresholdOverrides`, and the
  `complexity-cyclomatic` and `complexity-cognitive` rules with
  `overrides[].rules` for the file. A function whose cyclomatic and cognitive
  kinds are all `off` has no lens. A lens covers only these two kinds: it has
  no CRAP score, so a function that the health report flags only through
  CRAP (or through `complexity-crap` while the other two rules are `off`) has
  a health finding but no lens.
- Security candidate diagnostics remain opt-in through the project config.
  `security-sink` and `security-client-server-leak` default to `off`, retain
  those exact diagnostic codes, and publish at information severity because
  candidates are not verified vulnerabilities.
- The `workspace/didChangeWatchedFiles` registration derives its config-file
  globs from `fallow_config::CONFIG_FILE_NAMES`, the list the loader itself
  reads, and `type_aware_resolution_file` matches the same names. Add a config
  file name there, not in `crates/lsp`. The legacy `fallow.json`-style patterns
  stay registered separately because `initializationOptions.configPath` can
  still point at one.
- Code actions must be safe, scoped, and derived from the current issue.
- Initialization options and issue metadata stay aligned with generated VS
  Code contracts.
- Shutdown must prevent late publication and clean up owned subprocess work.
- `schedule.rs` decides when a run starts and when a run is cancelled. Saves,
  watched-file changes and configuration changes start a run after 200 ms
  without a new event, or 2 s after the first uncovered event. An event during
  a run cancels it through the engine cancellation token, but after a
  cancelled run the next run always finishes. A finished run publishes even
  when newer events arrived during it, because the per-URI staleness check
  protects edited buffers. A cancelled run never publishes and returns its
  type-aware changes to the pending set. A project root stops before its
  type-aware pass, never during it, so a run cancelled in its first root
  returns the changes as they were and the next run stays incremental. A
  failed run, or a run cancelled after an earlier root finished, returns
  them as a full invalidation. The first `didOpen` still starts the startup
  run at once.
- `session_store.rs` keeps one `EditorAnalysisSession` for each project root
  between runs, for a client that registers watched files. A run takes the
  session out of the store, walks the project again, and parses only the
  files whose fingerprint changed. When the file set changed, the session
  writes its modules to the persisted parse cache and parses through that
  cache. A finished or cancelled run puts the session back. A failed run
  drops it. `didChangeConfiguration`, and a watched-file event or a save for
  a config input (`session_input_file`), mark the store stale, and the next
  run loads each session again. A kept session writes its incremental parses
  to the persisted cache when the store drops it and at shutdown. The cache
  entry of a module keeps the fingerprint that was read before its parse, so
  a later edit misses the cache. `FALLOW_LSP_REUSE_SESSION=0` turns reuse
  off.
- `initializationOptions.prewarm` (off by default) parses the project at
  `initialized` into the kept sessions, so the first run parses nothing. It
  runs only when sessions are kept and the workspace root has a
  `package.json`. The prewarm holds the analysis slot, so the first run waits
  for it. It publishes nothing and leaves the startup gate armed: the first
  `didOpen` still starts the first run. The shutdown flag stops the parse,
  and a stopped prewarm keeps no session.

## Diagnostic metadata and document staleness

`diagnostic_filter::attach_changed_since_data` adds `changedSince` only when
the filter was applied. It merges into an existing object instead of erasing
metadata such as `circularDependency: { cycleId, fileCount }`. Circular
findings share a cycle identifier and use each import edge for their ranges;
legacy results without edges retain the first-file fallback.

`document_state::uri_is_stale` compares the captured disk-match state and
version with the live document. A dirty initial buffer, a newer version, or a
document closed during analysis prevents publication. A document opened during
the run is publishable only if its current text matches disk. Files absent from
both snapshots, including project manifests, remain valid cross-file targets.
Keep these checks shared by publishing and cached diagnostic cleanup.

A run reads the file of an open document only when the buffer is not known to
match the disk. `DocumentState::known_clean` is set by `didSave` and by a disk
read that confirms the match for that version. An edit makes a new state
without the flag, and a watched-file event for the URI clears it. The reads
run on the blocking pool after the documents lock is dropped. A watched-file
event bumps a disk generation under the documents write lock, and the run
compares that generation under the same lock before it sets the flag. So a
read that is older than a watched-file event never marks a buffer clean.

## Editor parity boundary

Editor analysis resolves configured rule severities through the same engine pass
as the CLI. `EditorAnalysisSession` applies
`fallow_engine::dead_code::apply_rule_severities` to every project slice it
analyzes, and again after type-aware refinement because reconciliation can add
findings. Per-path `overrides[].rules` therefore reach inline diagnostics, the
CLI, and the sidebar identically, and each project root is filtered with its own
config before a multi-root session merges the outputs. Do not reintroduce rule
filtering in `crates/lsp`; the session hands back an already-resolved result set.
The programmatic runtime behind MCP resolves severities at the same two points,
so every reporting surface narrows to the same set.

The shared LSP owns diagnostics, hover, quick fixes, Code Lens, and their
initialization contract. Host-specific sidebars, status items, full Health
reports, and full Security reports are outside that protocol. Editors without
custom UI contribution points should expose the shared LSP features and direct
users to a separately installed `fallow` CLI for the complete project reports.

When documenting CLI invocation from an editor, preserve the process contract:
run from the project root, treat exit `1` as a successful analysis with
findings, and treat exit `2` as a configuration, input, or execution error.
`fallow security` is advisory unless `--fail-on-issues`, an error-severity
security rule, or an explicit security gate changes its exit behavior.

## Verification

```bash
cargo test -p fallow-lsp
pnpm --dir editors/vscode run check:contracts
npm run verify:fast
```

Add protocol-level coverage for capability, URI, versioning, or push/pull
changes.
