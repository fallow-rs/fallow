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
