---
paths:
  - "crates/mcp/**"
---

# MCP constraints

Read `docs/reference/mcp-internals.md` for typed execution, CLI fallback,
resources, cancellation, telemetry, and Code Mode boundaries. Read the live
`fallow_types::mcp_manifest::MCP_TOOLS` and `MCP_RESOURCES` for the catalogue;
keep schema and router parity tests authoritative.

- Keep adapters thin over shared API contracts. Conditional CLI routes are
  owned by `crates/mcp/src/tools/fallback_policy.rs`.
- Preserve structured errors, project-relative paths, filtering, and sibling
  tool parity. API deadlines do not promise to stop blocking work immediately.
- Treat descriptions and parameter schemas as public agent contracts.
- Scope `find_similar_code` with `paths`. Inspect candidates
  before judging equivalence; discovery never authorizes model setup or edits.
- Run MCP contract tests and the corresponding CLI behavior smoke.
