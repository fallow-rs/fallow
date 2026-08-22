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
  reads the env vars at the adapter boundary and loads the config's `health`
  section through `fallow_api::load_health_config` only when a higher layer
  leaves an input unset, so a typed call and its CLI fallback score CRAP from
  the same map (#2368). Two known costs of that boundary: the lazy load parses
  the project config once more than the analysis context does (a second remote
  fetch for a config using `extends` over HTTPS with
  `allow_remote_extends`), and `FALLOW_INVALID_COVERAGE_PATH` always reports
  `context: health.coverage` because the existence check runs inside
  `validate_complexity_options`, which does not receive the winning layer.
  Only the sibling `FALLOW_INVALID_COVERAGE_ROOT` names its layer.
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
execution path changes.
