# Architecture Invariants

This guide states the crate and protocol boundaries contributors should check
before adding a feature. It is intentionally shorter than the repo map and
more concrete than the migration notes.

## System Overview

Fallow has three layers:

1. Fact and analysis crates build deterministic project knowledge.
2. Contract crates shape that knowledge into stable public data.
3. Protocol adapters expose the data through CLI, LSP, MCP, NAPI, editor, and
   CI surfaces.

The core crates are:

| Crate | Role |
| --- | --- |
| `fallow-types` | Shared typed contracts, issue metadata, suppressions, and envelope data. |
| `fallow-config` | Config loading and typed configuration. |
| `fallow-extract` | Parser-facing facts from source files. |
| `fallow-graph` | Module graph, dependency traversal, cycles, and impact facts. |
| `fallow-security` | Security matcher catalogue and candidate helpers. |
| `fallow-core` | Internal detector backend while engine migration continues. |
| `fallow-engine` | Session, discovery, parsing, graph construction, and typed analysis orchestration. |
| `fallow-output` | Shared output contracts, action builders, summaries, and reusable formatter pieces. |
| `fallow-api` | Supported Rust facade and programmatic workflow adapters. |

The protocol adapters are `fallow-cli`, `fallow-lsp`, `fallow-mcp`, and
`fallow-node`. They should translate options, call `fallow-api` or
`fallow-engine`, and serialize at their own boundary.

## Dependency Rules

- Foundation and analysis crates must not depend on protocol adapters.
- Protocol adapters must not call `fallow-core` directly. Use `fallow-api` or
  `fallow-engine`.
- `fallow-output` must not start analysis by depending on `fallow-core`,
  `fallow-engine`, or `fallow-api`.
- Analyzer logic belongs in the lowest crate that already owns the required
  facts. Do not put detector behavior in CLI, LSP, MCP, NAPI, or VS Code code.
- Public contract crates should avoid CLI-only assumptions. A contract should
  still make sense for API, MCP, LSP, and NAPI consumers.

Run the cheap crate-edge gate while changing workspace dependencies:

```bash
npm run check:crate-boundaries
```

The check uses `cargo metadata --no-deps` for crate dependency rules. The
`architecture_boundaries` Rust tests also guard source-level invariants such as
backend adapter containment, shared output-helper ownership, and protocol
manifest/docs drift.

## IO And Cache Rules

- Filesystem discovery, config loading, package manager detection, and parse
  cache ownership belong in session/runtime setup, not output formatting.
- Output formatting should work from typed evidence already passed to it. It
  must not crawl arbitrary project files to complete a report.
- Cache expansion needs invalidation tests and visible fallback behavior before
  it becomes part of a public workflow.
- Runtime and cloud data must enter through explicit options or typed evidence
  fields. Static analyzers should not hide network or filesystem side effects.

## Contract Rules

New issue kinds and public fields must update the contract source first:

- issue metadata in `crates/types/src/issue_meta.rs`
- output envelope types and action builders
- schema generation and generated TypeScript/NAPI artifacts
- LSP diagnostics and MCP selectors when exposed there
- `fallow explain`, docs anchors, suppressions, filters, and summary rows
- generated contract surfaces tracked by `scripts/contract-surfaces.mjs`

Machine-readable output must stay deterministic. Sort findings before
serialization, keep stable fingerprints stable, and document additive vs
breaking schema changes in `docs/backwards-compatibility.md` when behavior
changes.

Root `kind` lists and other factual protocol inventories must be generated from
or drift-tested against `docs/output-schema.json`. Do not copy a stale list into
prose without a schema-backed test.

## Testing Rules

Analyzer work should cover:

- positive minimal fixture
- negative abstain fixture
- false-positive guard
- suppression and severity/filter behavior
- output contract shape and actions
- at least one distilled framework or real-project regression when the rule
  depends on framework conventions

Protocol work should cover:

- manifest, schema, or generated-type drift checks
- the protocol-specific surface, not only the core analyzer result
- fallback behavior when the adapter shells out or downgrades evidence

Release and claim work needs real-project smoke evidence before it is described
as user-visible behavior.

## Current Exceptions

These are known migration states, not patterns to copy:

- SARIF rule-specific assembly still lives in `fallow-api`; reusable result,
  snippet, and property assembly belongs in `fallow-output`.
- `fallow-core` remains a published implementation dependency because
  `fallow-engine` still builds on it. New public surfaces should not depend on
  it directly.
- Several protocol adapters still contain hand-written guidance text where the
  audience needs nuance. Shared contract facts should come from manifests, but
  surface prose can remain local when it is intentionally different.
- `fallow-cli` still owns some CI and human-report rendering while shared
  formatter pieces move toward `fallow-output`.

When a change needs to cross one of these exceptions, name that in the PR or
design note and add a narrow test that protects the intended behavior.
