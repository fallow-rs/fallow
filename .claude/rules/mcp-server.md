---
paths:
  - "crates/mcp/**"
---

# MCP constraints

- Read `docs/reference/mcp-internals.md` for the affected tool.
- Keep tools thin over public API or CLI contracts.
- Return structured JSON and preserve exit, timeout, filtering, and sibling-tool
  parity.
- Treat descriptions and parameter schemas as public agent APIs.
- Run MCP contract tests plus the corresponding CLI behavior smoke.
