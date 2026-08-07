# Detection internals

Use this reference to locate a false positive, false negative, or new analyzer
in the current pipeline.

## Pipeline ownership

```text
config -> discovery -> extraction -> resolution -> graph -> detection -> output
```

- `crates/config/` resolves configuration, workspaces, rules, and external
  plugins.
- `crates/engine/src/discover.rs` owns the public discovery path; the shared
  walk lives in `crates/core/src/discover/walk.rs` behind the core backend
  adapter.
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
- Exact duplication uses the configured strict, mild, weak, or semantic token
  normalization. The opt-in `duplicates.near` pass is orthogonal: it compares
  function-like spans with semantic shingles while retaining exact results from
  the configured mode. Candidate generation is deterministically bounded and
  reports skipped comparisons instead of hiding incomplete work.
- Clone fingerprints are based on normalized token sequences, not raw source
  text. Formatting, comments, and line endings therefore preserve identity.
  Reviewed clone keys append the surviving instance count so token changes and
  added or removed copies trigger review again.
- Extraction changes that alter `ModuleInfo` semantics require the parse cache
  version to move with the changed semantics.
- Security findings are verification candidates until an agent or human
  confirms the evidence.

## Mock-aware test reachability

Test-reachability masking (issues #2031, #2068, #2082) removes a mocked module
from a test root's coverage credit only when the mock provably replaces the
original module without ever loading it. Every idiom is proven through
span-provenance (`crates/extract` `compute_semantic_usage` mock-API reference
spans) plus the abstain-by-default closed-factory proof; anything unproven
keeps coverage credit. Per-idiom semantics decisions:

- `vi.mock` / `jest.mock` with a statically closed factory masks. Proven
  receivers are the literal `vi` / `jest` globals, aliased named imports from
  `vitest` / `@jest/globals`, and `ns.vi` through a `vitest` namespace import.
- Automock (`vi.mock` / `jest.mock` without a factory) never masks, by
  decision. Vitest derives the auto-mocked shape by importing the original
  module, so its top-level code executes at collection time; file-level
  masking cannot express "module evaluated, exports stubbed". When a
  `__mocks__` sibling exists the original may not load, but the manual mock
  itself may load the original (`importActual`), and proving it does not would
  need a cross-file factory proof. The target and the speculative `__mocks__`
  sibling still receive dynamic-import credit edges.
- `vi.doMock` / `jest.doMock` never masks, by decision. The call is not
  hoisted, affects only module requests evaluated after it runs, and usually
  sits inside a test callback whose execution and ordering relative to the
  file's dynamic imports is runtime scheduling. It contributes credit edges
  only (target plus speculative `__mocks__` sibling), gated behind the same
  provenance proof for aliased and namespace receivers, so a manual mock
  referenced only through `doMock` does not surface as an unused file.
- `vi.unmock` / `jest.unmock` clears an earlier mask (fail-open).
  `doUnmock` is ignored entirely: it only affects later dynamic imports, so it
  must not clear a sound hoisted mask.
- Unresolved bare `vi` (Vitest `globals: true`) stays unproven for masking;
  without reading the Vitest config the safe direction is abstention. Its
  credit edges are still pushed eagerly.

Credit edges have their own abstentions, separate from masking. The mock
target must be a static string (string literal, expressionless template
literal, or `import('...')` with a string-literal source); a templated
``vi.doMock(`./adapters/${name}`)`` credits nothing. The speculative `__mocks__`
sibling is synthesized only for path-shaped specifiers containing a `/`
(`./services/api` credits `./services/__mocks__/api`); a bare package
specifier (`vi.mock('axios')`) credits the package itself but gets no
root-level `__mocks__/axios.ts` sibling edge, so a root manual mock referenced
only through a bare-specifier mock can still surface as an unused file.
"Anything unproven keeps coverage credit" describes masking abstention only;
it does not imply a credit edge exists.

## Script indirection crediting

`crates/core/src/scripts/` credits dependencies, config files, and entry files
from package.json scripts and from CI workflow commands. A package-manager
invocation of a declared script (`npm run lint -- --format gha`,
`yarn lint --fix`) is resolved to that script's body with the call-site
arguments appended, so binaries and flag values behind the indirection are
credited. The indirection is only followed when the call site adds arguments,
because a plain `npm run build` reaches a body that is already analyzed on its
own.

The script catalog separates names from bodies. Names are always the full set of
declared scripts, because a package manager resolves `pnpm <name>` to the script
and never to a same-named binary. Bodies are only the scripts the current run
analyzes, so a production run cannot enter a body that script filtering skipped.
A name declared with different bodies in several workspace packages keeps its
name but permanently loses its body.

### CLI flag-value crediting

Some CLIs load an npm package because a flag value names it: `eslint --format
gha` loads `eslint-formatter-gha`, `mocha --reporter mochawesome` loads
`mochawesome`, `node -r dotenv/config` loads `dotenv`. Fallow credits these
packages from a closed convention table of `(binary, flag)` pairs;
`crates/core/data/cli_flag_credits.toml` is the authoritative list of covered
binaries, flags, resolution rules, and tool built-ins. This is why a package
used only through such a flag (mochawesome, dotenv, a SARIF formatter in CI) is
not reported as an unused dependency.

Two invariants matter to users:

- A credit can only remove an unused-dependency finding for a dependency the
  project already declares.
- The table is never consulted by unlisted-dependency detection, so a
  synthesized name can never invent a finding.

Values that name files rather than packages abstain: relative and absolute
paths, and any unscoped value whose final path segment ends in a script
extension (`mocha --require test/setup.js` credits nothing, because tools
resolve an existing relative file before treating the value as a module).
Built-in values listed per tool (`mocha --reporter spec`, `jest --reporters
default`) also credit nothing.

Expansion is bounded twice. `MAX_SCRIPT_INDIRECTION_DEPTH` caps the depth of a
single chain, and `MAX_SCRIPT_EXPANSIONS` caps the total number of bodies
expanded for one command, because the cycle guard only rejects names on the
current path and mutually calling scripts otherwise fan out per path. Both
bounds are deliberate ceilings: a project that exceeds them loses crediting for
the deepest calls rather than degrading analysis time.

## Adding or changing an analyzer

Follow [analyzer authoring](../analyzer-authoring.md). Update the shared issue
metadata, output actions, schemas, editor surfaces, MCP metadata, suppression
handling, and fixtures as one contract.

For correctness work, prove the exact syntax with a focused fixture, then run a
representative public project before the broad repository verification.
