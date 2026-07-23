# Task context map

Read the rows in order for the task at hand. Skip the listed surfaces unless
the change reaches them.

| Task | Read in order | Skip by default |
|---|---|---|
| Understand the codebase | [Repository map](repo-map.md), [architecture invariants](../architecture-invariants.md) | Release and integration docs |
| Implement a feature or fix | Matching `.agents/skills` workflow, [repository map](repo-map.md), relevant nested `AGENTS.md`, [quality gates](quality-gates.md) | Unchanged output and integration lenses |
| Change extraction or analysis | [Repository map](repo-map.md), [detection internals](../reference/detection-internals.md), [extraction internals](../reference/extract-internals.md) when parsing changes, relevant crate `AGENTS.md`, [analyzer authoring](../analyzer-authoring.md), [review routing](review-routing.md) | Site and package docs |
| Change crate boundaries or public APIs | [Architecture invariants](../architecture-invariants.md), [core migration](../fallow-core-migration.md), [backwards compatibility](../backwards-compatibility.md) | Styling validation |
| Change human CLI output | [CLI internals](../reference/cli-internals.md), [review routing](review-routing.md), CLI nested `AGENTS.md`, matching CLI review skill | LSP and editor docs unless behavior is shared |
| Change JSON or CI output | [CLI internals](../reference/cli-internals.md), [backwards compatibility](../backwards-compatibility.md), [review routing](review-routing.md), relevant format review skill | Human CLI styling unless output is shared |
| Change LSP, MCP, or editor integration | [Repository map](repo-map.md), matching [LSP internals](../reference/lsp-internals.md), [MCP internals](../reference/mcp-internals.md), or [VS Code internals](../reference/vscode-internals.md), [review routing](review-routing.md), matching integration skill | Unrelated analyzers |
| Change framework plugins | [Plugin internals](../reference/plugin-internals.md), [plugin authoring](../plugin-authoring.md), relevant detection and graph references | Unrelated output formats |
| Change GitHub Action or GitLab CI | [Review routing](review-routing.md), action or CI tests, matching integration review skill | Editor integrations |
| Change docs, skills, or agent routing | [Knowledge architecture](knowledge-architecture.md), [AI tooling](ai-tooling.md), [quality gates](quality-gates.md) | Analyzer internals |
| Prepare a commit or push | [Quality gates](quality-gates.md), active workflow skill | Unrelated reference catalogues |
| Review a cross-surface change | [Review routing](review-routing.md), [quality gates](quality-gates.md), affected durable references | Unaffected specialist lenses |

Use live command help and generated contracts for volatile CLI details. Do not
copy full flag or tool catalogues into root routers.
