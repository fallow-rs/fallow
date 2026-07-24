# Repository map

Use this file for a fast architectural map before editing or reviewing.

## Top-level layout

- `crates/config/`: configuration, framework presets, packages, and workspace
  metadata.
- `crates/types/`: shared discovery, extraction, reporting, and suppression
  types.
- `crates/extract/`: Oxc parsing, AST extraction, caches, complexity, and
  component file handling.
- `crates/graph/`: import resolution, module graph construction, reachability,
  re-export propagation, and cycles.
- `crates/core/`: detector backend, built-in plugins, and analysis-specific
  cross-reference logic.
- `crates/engine/`: discovery, command-neutral sessions, duplication, health,
  security orchestration, and typed results.
- `crates/output/`: typed output contracts, envelopes, schemas, and integration
  helpers.
- `crates/api/`: public programmatic API and typed run entry points.
- `crates/cli/`: CLI commands, fixes, terminal output, and format dispatch.
- `crates/lsp/`: diagnostics, code actions, code lens, and hover.
- `crates/mcp/`: agent-facing MCP tools over shared APIs with explicit CLI
  fallbacks.
- `crates/napi/`: Node.js bindings over the public programmatic API.
- `crates/multicall/`: packaged binary entry that dispatches CLI, LSP, and MCP
  modes.
- `crates/license/`: offline license verification and feature entitlements.
- `crates/security/`: data-driven security source and sink catalogue.
- `crates/v8-coverage/`: V8 coverage parsing and normalization.
- `crates/benchmarks/`: internal Rust benchmark support.
- `editors/vscode/`: VS Code extension and LSP client.
- `viz-frontend/`: browser visualization source, tests, and generated assets.
- `action/`: GitHub Action, scripts, jq filters, and tests.
- `ci/`: GitLab template, scripts, jq filters, and tests.
- `tests/fixtures/`: integration fixtures shared across crates.

## Pipeline

```text
config -> discovery -> extract -> resolve -> graph -> analyze -> output
```

Find the first incorrect stage before editing:

- Parser or syntax extraction problems usually start in `crates/extract/`.
- Resolution and workspace linkage usually start in
  `crates/graph/src/resolve/`.
- Reachability and re-export propagation belong in `crates/graph/src/graph/`.
- Detection and cross-reference behavior belongs in `crates/core/src/analyze/`
  or command-neutral engine code.
- Contract shape belongs in `crates/output/` and format assembly in
  `crates/api/` or `crates/cli/`.
- Host-specific behavior belongs in MCP, LSP, editor, action, or CI adapters.

## High-value paths

### Extract

- `crates/extract/src/lib.rs`: parse entry point and cache-aware dispatch.
- `crates/extract/src/visitor/`: import, export, member, and dynamic patterns.
- `crates/extract/src/parse.rs`: parser and semantic pass.
- `crates/extract/src/cache/`: incremental parse cache.
- `crates/extract/src/complexity.rs`: complexity visitor.
- `crates/extract/src/sfc.rs` and `sfc_template/`: Vue and Svelte behavior.

### Graph

- `crates/graph/src/project.rs`: file registry and workspace metadata.
- `crates/graph/src/resolve/`: module resolution and aliases.
- `crates/graph/src/graph/build.rs`: graph edges and references.
- `crates/graph/src/graph/reachability.rs`: entry-point reachability.
- `crates/graph/src/graph/re_exports/`: barrel propagation.
- `crates/graph/src/graph/cycles.rs`: strongly connected components.

### Engine, API, and output

- `crates/engine/src/session.rs`: resolved project and session state.
- `crates/engine/src/results.rs`: engine result carriers.
- `crates/engine/src/duplicates.rs` and `duplication_detector/`: duplication
  orchestration and detection.
- `crates/engine/src/health/`: scoring, hotspots, targets, and coverage gaps.
- `crates/api/src/runtime/`: typed programmatic run entry points.
- `crates/output/src/issue_contract.rs`: output-facing issue metadata.
- `crates/output/src/root_envelopes.rs`: root envelope policy.

### Integrations

- `crates/cli/src/report/`: CLI output formats.
- `crates/mcp/src/tools/`: MCP tools and adapters.
- `crates/lsp/src/diagnostics/`: diagnostics by issue family.
- `crates/napi/src/lib.rs`: Node.js API bindings.
- `editors/vscode/src/`: editor client and commands.
- `crates/engine/src/viz.rs`: command-neutral visualization graph data.
- `crates/cli/src/viz.rs`: visualization command and asset serving.
- `viz-frontend/src/`: browser rendering and interaction.
- `action.yml`, `action/scripts/`, `action/jq/`: GitHub Action.
- `ci/gitlab-ci.yml`, `ci/scripts/`, `ci/jq/`: GitLab CI.

## System invariants

- Fallow uses syntactic analysis without the TypeScript compiler.
- Use `FxHashMap` and `FxHashSet` for project collections.
- Re-export chains and workspaces are first-class graph behavior.
- Output is a public contract for agents and integrations.
- Keep analysis logic out of thin LSP and MCP adapters.

See [architecture invariants](../architecture-invariants.md) for dependency and
ownership rules.
