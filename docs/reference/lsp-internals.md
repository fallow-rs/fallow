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
- The component health signals (`prop-drilling`, `thin-wrapper`,
  `duplicate-prop-shape`) publish at hint severity. Their rules default to
  `off`, so the diagnostics show only in a project that turned a rule on.
  They stay out of the pull request surfaces (CodeClimate, GitHub annotations
  and summary, PR comment, review), because those surfaces gate on changes
  and the signals do not gate.
- A config pattern that matched nothing (`ignoreFindings`,
  `ignoreDependencies`) is an information diagnostic with the `unnecessary`
  tag on the entry in the config file that declares the list. The code is the
  `workspace_diagnostics[]` kind and the message is the entry `message`.
  `FallowConfig::locate_list_entry` finds the entry through the `extends`
  merge order. A pattern without a local entry (for example from a remote
  `extends` config) goes to the output log, once per changed set.
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
  run loads each session again. `SESSION_INPUT_FILE_NAMES` feeds both
  `session_input_file` and the watched-file globs, so the two lists cannot
  drift. A kept session also keeps its `ConfigSources`: the content of the
  config file and of each local `extends` target, read before and after the
  load. `ConfigSources` also keeps a snapshot of the other inputs that
  config resolution read (`fallow_config::ConfigInputs`): the external plugin
  files (`plugins` paths, `.fallow/plugins/`, root `fallow-plugin-*`), the
  rule packs, and the child folders of each `autoDiscover` folder. The
  engine reads them just before config resolution and the LSP reads them
  after the load. When the two reads differ, an input changed during the
  load, and the next run loads the session again. A run
  compares them with the disk and loads the session again when one differs,
  because a `configPath` file, an `extends` target, a configured plugin file
  or a rule pack can have any name and no watched glob covers it. The
  default plugin locations are also watched session inputs. A session that
  puts the estimated memory of all kept sessions over 512 MB (the estimate
  and limit of the MCP warm parse store) is not kept, so each run for that
  root loads its session. A session kept for other settings goes back to
  the caller, which writes its parse cache outside the store lock. An
  incremental parse records the read failures and parse degradations of the
  whole project again, so a fixed file loses its entry. A kept session writes its
  incremental parses to the persisted cache when the store drops it and at
  shutdown. Shutdown turns the store off, so a run in flight writes its own
  session. The cache entry of a module keeps the fingerprint that was read
  before its parse, so a later edit misses the cache. When a cached
  fingerprint has no ctime (Windows), `refresh_discovery` drops the modules,
  and the run parses through the persisted cache, which compares content
  hashes. `FALLOW_LSP_REUSE_SESSION=0` turns reuse off.
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

The initialization option `packageBaselines: false` sets
`ChangeScopeRequest::no_package_baselines`, so no project reads the map.
Each project resolves one `ChangeScope` after workspace discovery, with the
engine rule that the CLI and the programmatic API use. When
`workspaces.changedSince` is configured and no global editor ref was requested,
that scope holds the package baselines. `EditorAnalysisSession::apply_change_scope`
narrows dead-code findings and clone groups after the type-aware pass, and
inline complexity uses the same scope. Each published document receives its owning package's ref in
`data.changedSince`; a document in an unlisted package or at the project root
receives no ref. A cross-package finding can therefore appear in documents
with different metadata. `fallow/analysisComplete` reports the configured
`packageBaselines` in stable path order using the output contract's
`workspace_root` and `reference` row. A global editor ref keeps its existing
applied or dropped status and suppresses package resolution, even when the
global ref is invalid.

Each dead-code diagnostic sets `data.findingId` to the `finding_id` of its
finding. The value has the form `dc1:<rule-token>:<16 hex digits>`, with a
`~k` suffix for findings that share a subject. It is the same value as the
`finding_id` field of the JSON output, so an editor client can join a
diagnostic to a CLI, MCP or CI report. Use `diagnostics::finding_data` for a new producer and
`diagnostics::with_finding_id` when the producer already sets `data`: both
merge the key into the object and keep the other keys. A finding without an
id keeps `data` absent. The id does not depend on the line or column, so it
stays the same when code above the finding moves. A change to the identity
parts of a rule changes the `dc1` scheme and is a breaking change. One finding
can give more than one diagnostic (for example one per cycle member), and each
of them carries the same id. Security diagnostics do not carry `findingId`
yet: the security id is stamped in the CLI, so the LSP results do not have it.

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
