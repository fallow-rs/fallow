# MCP internals

Use this reference for MCP tool contracts, typed execution, CLI fallback, and
subprocess safety.

## Sources of truth

- `crates/types/src/mcp_manifest.rs` owns the tool inventory and shared
  metadata.
- `crates/mcp/src/server/mod.rs` registers the server and tool router.
- `crates/mcp/src/params.rs` owns request parameter types.
- `crates/mcp/src/tools/` owns tool-specific argument building and execution.
- `crates/mcp/src/tools/api_runtime.rs` runs supported tools through typed
  APIs.
- `crates/mcp/src/tools/fallback_policy.rs` makes CLI fallback explicit.
- `crates/mcp/src/tools/code_mode.rs` and `code_mode_subprocess.rs` own code
  execution and subprocess isolation.
- `crates/process/` owns shared descendant cleanup for MCP and type-aware
  sidecars.

Do not hand-copy the complete tool list into durable prose. Read
`MCP_TOOLS` or generated user documentation for the current inventory.

## Resources

Resources are the server's read-only, cacheable reference channel: compile-time
material an agent lists once (`resources/list`, `resources/templates/list`)
and reads by URI (`resources/read`) with no subprocess and no analysis run;
clients cache by URI. (Hosts such as Claude Code still read a resource through
their own resource tool, so this is not a saved tool call.) Tools remain the
surface for anything that depends on a project root.

Sources of truth:

- `crates/types/src/mcp_manifest.rs` owns the resource catalogue
  (`MCP_RESOURCES`: URI, name, description, MIME type, template flag) and the
  URI scheme constants. Catalogue order is wire order.
- `crates/mcp/src/resources.rs` renders every payload in-process and owns the
  `fallow://explain/{issue_type}` template expansion. The logic lives in free
  functions because rmcp's `RequestContext` cannot be constructed in tests;
  the `ServerHandler` methods in `crates/mcp/src/server/mod.rs` are one-line
  delegators.
- Payload data comes from shared crates only: `fallow_types::mcp_manifest`
  (tools), `fallow_api::explain` plus `fallow_types::issue_meta` (issue types,
  explain index, explain documents), `fallow_types::task_matrix` (task
  matrix), and `fallow_api::schemas` (config, plugin, and rule-pack JSON
  Schemas, plus zero-config rule severities). `fallow schema` reads the same
  sources, so the `mcp_resources`, `mcp_tools`, `issue_types`, and
  `task_matrix` manifest blocks and the resources agree by construction.

Contract rules:

- Every resource is `application/json`. The server version travels in the
  content item's `_meta.fallow_version`, never inside the payload, so the
  schema documents stay valid strict JSON Schema and a cached copy is still
  self-describing; clients cache by URI and invalidate on server version.
  rmcp 3.1 exposes `ttl_ms` / `cache_scope` hints on the initialize result
  only, so per-resource caching stays client-side.
- The catalogue is compile-time constant. Declare neither `subscribe` nor
  `listChanged`; a client that sees them opens subscriptions the server
  never notifies.
- Static resources carry an exact `size` and `audience: ["assistant"]`
  annotations with a higher `priority` on `fallow://tools` and
  `fallow://task-matrix` than on the schemas. No `lastModified`: compiled-in
  data has no meaningful mtime.
- The URI family is the authority (`fallow://tools`, not `fallow:///tools`).
  The exact strings are pinned as literals in
  `crates/mcp/src/server/tests/resources.rs`; the difference is invisible in
  prose and breaks template matching on the wire.
- Schema resources are byte-for-byte the CLI schema documents
  (`fallow config-schema`, `fallow plugin-schema`, `fallow rule-pack-schema`);
  the explain template is exactly the `fallow_explain` tool payload.
- Unknown URIs and unknown issue types return a structured
  `resource_not_found` error whose `data` lists the known URIs and templates,
  or the nearest explain URIs plus the index URI. The wire code depends on the
  negotiated protocol: rmcp sends `-32002` to peers on protocol versions
  before 2026-07-28 and rewrites it to `-32602` (invalid params) for
  2026-07-28 and later; `data` is preserved either way, so clients should key
  on `data`, not the code.
- Adding a resource: add the `MCP_RESOURCES` row, add its renderer arm in
  `crates/mcp/src/resources.rs`, pin the URI in the resource tests, and run
  `npm run generate:contracts` so `capabilities.json` and the generated
  `references/mcp.md` resource table update. `manifest_sync.rs` fails until
  the manifest and the live catalogue agree on names, URIs, MIME types, and
  template placement.

## Contract rules

- Prefer typed API execution. Use CLI subprocess fallback only where the policy
  explicitly allows it.
- Return structured results and structured errors. Do not require clients to
  parse human output.
- Use JSON, quiet mode, and explanation metadata for CLI-backed analysis.
- Keep parameter names, defaults, license metadata, read-only status, and tool
  descriptions synchronized with the shared manifest.
- Preserve project-relative paths in analysis results.
- Mutation tools expose preview and explicit confirmation semantics.
- Apply bounded timeouts and clean up the complete owned process tree on
  completion, cancellation, or timeout.
- Never inherit unbounded environment or filesystem authority into code mode.
- The Code Mode allowlist lives in `fallow_types::mcp_manifest`: a tool is
  reachable from `code_execute` exactly when its `McpToolInfo` row carries a
  `code_mode_alias`, and `CODE_MODE_ONLY_TOOLS` holds the few host helpers with
  no standalone tool. The sandbox bindings, the `fallow schema` `mcp_tools`
  rows, and the `fallow://tools` resource are all projections of that field, so
  an agent on those surfaces reads reachability from data instead of the
  `code_execute` description. The generated skill reference
  (`npm/fallow/skills/fallow/references/mcp.md`) does not carry the column; an
  agent reading only that file still has to read the description.
  Drift tests bind the manifest to the `CodeModeTool` enum in both directions.
- Each Code Mode host call has exactly one backing, `CodeModeBacking::Api` or
  `CodeModeBacking::Subprocess`, derived from the single `api_route` match that
  also performs the dispatch, so no tool can be listed as in-process without a
  route. `fallow-api` has no cancellation, so whole-project analyses (`analyze`,
  `find_dupes`, `check_health`, `audit`) take the killable subprocess even
  though a typed route exists for them: a timeout has to stop the work, not just
  stop waiting for it. An in-process call that does outlive `timeout_ms` is
  counted as abandoned, and while abandoned work is still running later host
  calls fall back to the subprocess, which bounds how much orphaned analysis a
  long-lived server can accumulate. Cancellation in `fallow-api` is what would
  remove the split.
- Host calls are memoized per snippet, keyed on the tool plus its merged params
  in canonical form (object keys sorted, array order preserved), so
  `{ a: 1, b: 2 }` and `{ b: 2, a: 1 }` are one entry. A hit spawns nothing,
  spends no `max_host_calls` slot, and charges no output bytes, but still
  appears in `calls[]` with `cache_hit: true` so the trace stays honest. Only
  successful calls are memoized, the cache lives in `CodeModeState`, and it
  dies with the snippet: nothing persists between `code_execute` invocations.
  The cache is therefore bounded by `MAX_HOST_CALLS` entries and by
  `max_output_bytes` in total size.
- The `calls[]` trace is bounded independently, at `MAX_RECORDED_CALLS`
  entries, reported as `limits.max_recorded_calls`. Dispatches and refusals
  each have a budget, but a memo hit spends neither, so without this bound a
  snippet looping one cached call would grow the response envelope without
  limit while every documented limit still reported as respected. Past the
  bound the host calls still run and still return; only their trace entries
  are dropped, and the response carries `calls_omitted` with the count. The
  field is absent when nothing was dropped, so an ordinary response keeps the
  shape its consumers already parse.
- `fallow.all(requests)` is the fan-out. It runs in Rust, not in JS: the
  sandbox stays synchronous and promise-free, and the call blocks until every
  element has resolved. Elements are returned positionally aligned with the
  requests as `{ ok, value }` or `{ ok, error }`, so a batch degrades per
  element rather than as a whole; only whole-batch problems (a non-array
  argument, an element that is not an object carrying a `tool` string, more
  elements than `max_host_calls`, a batch bigger than the remaining budget, an
  expired deadline) throw. Every element goes through the
  same `resolve` (tool allowlist, `root` merging, memo key), the same shared
  deadline, and the same output accounting as a single call, and the accounting
  is applied in element order so the response does not depend on which element
  finished first. The output budget is shared, not per element: what is left of
  `max_output_bytes` is divided evenly across the dispatches a batch makes, so a
  fan-out reads at most `max_output_bytes` in total. Dividing up front rather
  than charging on arrival is what keeps the outcome independent of which worker
  reported first. Subprocess-backed elements overlap on a worker pool capped at
  `MAX_BATCH_CONCURRENCY`; in-process elements run one at a time on the calling
  thread, because `fallow-api` has no cancellation and two concurrent in-process
  analyses would be exactly the uncancellable pile-up the abandoned-call
  accounting exists to prevent.
- `max_output_bytes` bounds two independent things: the total fallow JSON read
  by host calls, and the serialized snippet result. The result is the only part
  of a Code Mode response that enters the calling agent's context, so an
  oversized one is refused with `ok:false`, `truncated:true`, `result_bytes`,
  and a short `result_preview`, never returned whole and never truncated into
  invalid JSON. The envelope's `error` string is clamped the same way, at
  `max_output_bytes` or a 4 KiB floor, whichever is larger, so a structured
  programmatic error stays readable while a thrown megabyte does not reach the
  agent.
- The sandbox denies dynamic code compilation, not just the `Function` global.
  Undefining the binding leaves the intrinsic reachable through
  `(function () {}).constructor`, so a hardening prelude replaces the
  `constructor` slot on the function, async-function, generator-function, and
  async-generator-function prototypes with a non-configurable `undefined`
  before the snippet runs. `harden_globals` stays a denylist, so a test
  enumerates `globalThis` own property names against a reviewed allowlist: a
  runtime upgrade that adds a global fails the build instead of widening the
  sandbox silently.
- Host calls refused before dispatch (unknown or unsupported tool name,
  malformed params, an output budget with nothing left to spend) run no
  analysis and read no output, so they do not spend the `max_host_calls`
  budget. The budget counts dispatches rather than `calls[]` entries, so
  neither a rejection nor a memo hit shrinks what a later distinct call can
  spend. Every such refusal is charged to `max_rejected_host_calls` instead,
  which is what keeps a snippet looping over bad names, or over a spent output
  budget, from spending an analysis budget it never used; the recorded tool
  name is clamped as well, so an unvalidated `fallow.run` argument cannot
  inflate the response envelope. Bounding the trace itself is a separate
  concern, handled by `max_recorded_calls` above.
- Keep tool ordering deterministic.
- The `audit` and `check_health` typed routes resolve Istanbul coverage
  through `fallow_api::coverage::resolve_coverage_inputs` with the CLI's
  precedence (tool parameter, then `FALLOW_COVERAGE` / `FALLOW_COVERAGE_ROOT`,
  then `health.coverage` / `health.coverageRoot`). `tools/api_runtime.rs`
  reads the env vars once at the adapter boundary
  (`resolve_typed_coverage_inputs` takes the layer as `None` on every
  production call and injects it only in tests) and loads the config's
  `health` section through `fallow_api::load_health_config` only when a higher
  layer leaves an input unset, so a typed call and its CLI fallback score CRAP
  from the same map (#2368). Three known costs of that boundary: the lazy load
  parses the project config once more than the analysis context does (a second
  remote fetch for a config using `extends` over HTTPS with
  `allow_remote_extends`); `FALLOW_INVALID_COVERAGE_PATH` always reports
  `context: health.coverage` because the existence check runs inside
  `validate_complexity_options`, which does not receive the winning layer,
  while the sibling `FALLOW_INVALID_COVERAGE_ROOT` names its layer; and
  coverage resolution runs ahead of the `run_health` / `run_audit` option
  validation, so a call that is invalid in two ways (a rejected coverage input
  and, say, `threads: 0`) reports the coverage error, which is also the order
  the CLI reports them in.
- `trace_symbol` and `symbol_impact` expose exact TypeScript evidence for
  Fallow-owned project questions. They do not expose compiler diagnostics or
  typed lint findings.

## Verification

```bash
cargo test -p fallow-mcp
cargo test -p fallow-types
npm run generate:contracts:check
npm run verify:fast
```

Tool changes require a protocol-level test plus a real MCP invocation when the
execution path changes. Resource changes are covered by
`crates/mcp/src/server/tests/resources.rs` (catalogue, reader, errors) and the
spawned-binary `crates/mcp/tests/resources.rs` (initialize, `resources/list`,
`resources/templates/list`, `resources/read`).
