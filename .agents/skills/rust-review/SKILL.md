---
name: rust-review
description: Review Rust code changes in fallow for correctness, performance, cache-friendly design, and project conventions. Use when changes touch Rust crates such as config, types, extract, graph, core, cli, lsp, or mcp.
---

# Rust Review

Read:
- `.agents/agents/rust-reviewer.md`
- `.agents/rules/code-quality.md`

Also read the crate-specific rule file when relevant:
- `.agents/rules/core-crate.md`
- `.agents/rules/extract-crate.md`
- `.agents/rules/graph-crate.md`
- `.agents/rules/cli-crate.md`
- `.agents/rules/lsp-server.md`
- `.agents/rules/mcp-server.md`

Review only high-confidence issues. Report:
- file and line
- what is wrong
- the concrete fix
- `APPROVE`, `CONCERN`, or `BLOCK`
