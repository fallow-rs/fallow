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
