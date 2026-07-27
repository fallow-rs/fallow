# Detection internals

Use this reference to locate a false positive, false negative, or new analyzer
in the current pipeline.

## Pipeline ownership

```text
config -> discovery -> extraction -> resolution -> graph -> detection -> output
```

- `crates/config/` resolves configuration, workspaces, rules, and external
  plugins.
- `crates/engine/src/discover.rs` and `discover_walk.rs` own the public
  discovery path.
- `crates/extract/` parses source and produces syntax facts.
- `crates/graph/` resolves imports and builds reachability and re-export state.
- `crates/core/src/analyze/` detects dead-code and structural issue families.
- `crates/engine/` combines discovery, graph, core analysis, duplication,
  health, security, and cross-reference results.
- `crates/output/` and `crates/api/` turn typed results into public contracts.
- `crates/api/src/type_aware/` owns semantic requests, reconciliation,
  provenance, and sidecar transport.

Fix the earliest incorrect stage. Do not compensate for an extraction or graph
error by suppressing a downstream detector.

## Analyzer families

- Dead code and dependency findings:
  `crates/core/src/analyze/unused_deps.rs`,
  `crates/core/src/analyze/members/`, and related modules under
  `crates/core/src/analyze/`.
- Boundaries and architectural policy:
  `crates/core/src/analyze/boundary.rs`,
  `crates/core/src/analyze/boundary_calls/`, and
  `crates/core/src/analyze/policy/`.
- Framework and component intelligence:
  `crates/core/src/analyze/react_intel.rs`, route and render analyzers, and
  `crates/core/src/plugins/`.
- Duplication: `crates/engine/src/duplicates.rs` and
  `crates/engine/src/duplication_detector/`.
- Health, hotspots, ownership, targets, coverage gaps, and styling:
  `crates/engine/src/health/`.
- Security candidates: `crates/core/src/analyze/security/` with matcher data
  owned by `crates/security/`.
- Feature flags: extraction facts in `crates/extract/src/flags.rs`, detector
  behavior in `crates/core/src/analyze/feature_flags.rs`, and orchestration in
  `crates/engine/src/feature_flags.rs`.

## Accuracy invariants

- Fallow's default analysis is syntactic and has no TypeScript compiler
  dependency. Type-aware refinement is explicit opt-in through the
  version-matched sidecar and may only refine Fallow-owned project questions.
- Type-aware public API traversal follows checker symbols that resolve to
  project-local declarations. Parameter names and generic type parameters are
  not reusable private types.
- Complete semantic confirmation may remove unmatched syntactic private-leak
  guesses. Partial or unavailable evidence retains the original finding.
- Overlapping TypeScript projects deduplicate API entries and coupling edges by
  stable semantic identity.
- Prefer conservative advisory output over noisy speculation.
- Preserve entry points, re-export chains, workspace edges, type-only usage,
  framework conventions, and suppression behavior through the full pipeline.
- Keep issue identity and ordering deterministic. Baselines, audits, editor
  diagnostics, and review comments depend on stable keys.
- Styling and CSS-in-JS extraction must preserve source line mapping.
- Duplication token or normalization changes require the duplication cache
  version to move with the changed semantics.
- Extraction changes that alter `ModuleInfo` semantics require the parse cache
  version to move with the changed semantics.
- Security findings are verification candidates until an agent or human
  confirms the evidence.

## Adding or changing an analyzer

Follow [analyzer authoring](../analyzer-authoring.md). Update the shared issue
metadata, output actions, schemas, editor surfaces, MCP metadata, suppression
handling, and fixtures as one contract.

For correctness work, prove the exact syntax with a focused fixture, then run a
representative public project before the broad repository verification.
