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
- A name supplied by two different `export *` sources is not exported by the
  barrel at all (ECMA-262 ResolveExport returns `ambiguous`). The contributing
  declarations are therefore unknown rather than dead. Unused exports, member
  findings, unrendered components, and unprovided injects abstain instead of
  attributing the barrel mistake to a source file. `export type *` collides the
  same way: it drops the value namespace, so two value declarations behind it
  collide in type space alone and type-space abstention applies there too.
  `ModuleGraph::ambiguous_star_exports` exposes collisions for reporting, while
  `ModuleGraph::ambiguity_participants` identifies their canonical declarations
  for detector gates.
- `default` is one importable name that either side may spell two ways, and
  reference attachment matches on the name, not on the spelling (issue #2374).
  An export declares it as `export default x` or as `export { x as default }`;
  an import names it as `import x from './impl'`, as
  `import { default as x } from './impl'`, or as the ambient
  `declare module '<specifier>' { export { default } from './impl' }` form,
  which records one named type-space import per specifier. A CommonJS
  `exports.default = x` declares the same binding and is recorded under the
  same written name. Every pairing credits the target's default export, and a
  `export { default } from` chain carries that credit hop by hop. A plain
  `export *` still never forwards `default`, so the shape that reaches a
  default through a star is the namespace object, not the star surface.
- That name collapse is scoped to modules whose `default` really is a default
  export. A CSS Module exports one `ExportName::Named` per class and never an
  `ExportName::Default`, so a class spelled `.default` stays an ordinary name:
  a plain `import styles from './x.module.css'` binds the whole class map and
  names no single class, and `narrow_css_module_references` remains the only
  thing that credits classes, from the member accesses the consumer writes.
  `NamedDefaultSpelling::for_target` decides that from the target path with
  `is_css_module_stylesheet`, which tracks the extractor's own CSS Module set
  (`.module.css`, `.module.scss`, `.module.sass`, `.module.less`). The
  narrowing gate `is_css_module_path` stays on the narrower `.css` / `.scss`
  pair, so `.less` and `.sass` class maps keep their default slot empty without
  gaining member narrowing.
- An ambient-module star re-export (`export *` or `export * as ns` inside a
  `declare module '...'` body, recorded as a type-only namespace import
  without a local binding) credits the full ES star surface of its target:
  every named export in both the type and the value namespace, never the
  target's `default`, and, through the exposed namespace closure below, the
  same surface of every module the target reaches through its own `export *`
  chain plus every export (`default` included) of each `export * as sub`
  source along that chain, recursively. `export * as ns` adds a type-only
  default import for `ns.default`. Every other type-only import credits type
  space only: a bound `import type { x }`, the ambient named re-export
  (#2349), explicitly type-only ambient re-exports, and `import()` type
  references in TypeScript and JSDoc, so the value half of a same-name type
  and value pair reached only that way still reports.
- The exposed namespace closure
  (`ModuleGraph::collect_exposed_namespace_targets`, issues #2357, #2372,
  #2373) is seed-agnostic. Its seeds are every target whose whole namespace
  object a consumer observes in Phase 2 (an ambient-module star, a
  dynamic-import pattern match through `import()`, `import.meta.glob`, or
  `require.context`, a bare side-effect `require('./barrel')` with no binding,
  and a namespace import the graph cannot narrow: a whole-object use, a
  binding handed on without member access, or a binding exported under its own
  name from any module, entry point included) plus every `export * as ns`
  source whose name reaches a consumer the graph cannot enumerate.
- A name reaches such a consumer three ways, and the closure applies the same
  test to all three: it arrives on an entry point's own export surface (the
  public API, `default` included), on a module already in the closure (which
  exposes what its own exposure allows), or at a name some importer uses as a
  whole object (`import { sub } from './barrel'` plus `Object.values(sub)`).
  The search runs outward from each namespace edge along named and plain-star
  re-exports, so its cost scales with the number of `export * as` edges rather
  than with the number of names an entry point re-exports. Every hop must
  uniquely forward the binding, the same rule the Phase 2c walk applies, so a
  barrel that declares its own `ns`, or that receives `ns` from two stars at
  once, exports a different binding under that name and the chain stops
  there. That rule is not restated: `ModuleGraph::forwards_binding` picks the
  namespace and then calls Phase 2c's own `uniquely_forwards_binding`, so the
  closure and the phase it pre-computes for cannot drift apart on what a hop
  forwards. Sitting on an entry point's plain-`export *` closure is not on its
  own proof that a name survives to the entry, so no hop is skipped for it.
  The forwarding check reads the value namespace whenever the source exports
  the name there and the type namespace otherwise, so
  `export type { ns } from './barrel'` on an entry point does not put the
  value namespace object on the surface. Plain-star hops and the closure's own
  chain walk stay namespace-agnostic, like the pre-existing entry-star
  closure.
- The closure follows `export *` and `export * as` chains from each member,
  and carries how much of each member is exposed. A member whose whole
  namespace object is observed exposes every export; a member reached through
  a plain `export *` exposes every export except `default`, which no plain
  `export *` forwards. Star propagation treats a member like an entry barrel
  for its `export *` sources (named exports in both namespaces, never
  `default`) and namespace re-export propagation credits every export of its
  `export * as` sources (`default` included) whenever the member exposes the
  name that `export * as` uses. `export * as default` therefore only carries a
  namespace object onward from an entry point itself or from a member whose
  whole namespace object is observed.
- The namespace-edge seeds and the chain walk run to a fixpoint against each
  other: a target that joins the closure can itself expose the name a further
  `export * as ns` edge forwards to it, and that edge only qualifies once the
  target is a member. Each round widens the closure or stops, so the walk
  terminates, and the closure Phase 2c reads already contains every target
  Phase 2c would credit in full instead of stopping one namespace level short
  of it. The reachability prune the search uses only grows, so a round extends
  it from the members the previous round added instead of rebuilding it, and
  each re-export edge is walked at most once across all rounds. The rest of a
  round is a rescan of the namespace edges still pending, which a chain shaped
  to resolve exactly one edge per round can drive up; real barrel trees settle
  whole subtrees per round and stay in the single digits.
- A member-narrowed namespace import (`ns.one()`) never seeds the closure, a
  binding placed in an exported object literal (`export const API = { ns }`)
  seeds it only when it is also used as a whole object or exported under its
  own name (the namespace-object alias phase follows `API.ns.<member>`
  precisely while the direct-export mark-all stays as before), and a namespace
  re-export on a barrel off the entry surface with no consumer exposes nothing.
  The seed's own credit keeps its shape: a runtime whole-module edge credits
  the namespace object, `default` included; the ambient star form credits the
  star surface without `default`.
- Reachability gates the seeds issues #2372 and #2373 add, and nothing else.
  A target no entry point reaches, observed only by a consumer in this graph,
  is not seeded: that consumer is unreachable too, the report already calls
  the target an unused file, and crediting its chain would only stack
  unused-export rows underneath the unused-file rows. The same holds for an
  `export * as ns` source no entry point reaches. The test applies to the
  seed, not to the observer, so a whole-object use inside an unreachable file
  still suppresses the reachable target's whole chain, the same way the
  pre-existing mark-all already suppresses the target's direct exports from
  such a file; deleting the unused file the report names brings the chain back
  as unused exports on the next run.
- The ambient seeds are not gated, and neither is the chain walk. A
  `declare module 'pkg'` body states the shape of an external module id: its
  observers are importers of that id, outside this graph, so where the shim
  and its target sit inside the graph says nothing about who looks. The chain
  behind an unreachable shim routinely re-enters a module an entry point
  imports directly, and gating it reported exports on files the report calls
  reachable. A re-export edge makes its source reachable whenever the barrel
  is, so only an ambient chain can ever walk out of an unreachable member in
  the first place; a hop that lands on an unreachable module credits it
  through a re-export reference the unused-export detector already discounts
  when nothing reachable reads it. The mark-all sites that feed the closure
  keep crediting the target's own direct exports as before, reachable or not.
- Two seed properties are deliberate and visible in reports. The seed is
  namespace-agnostic, so `export type { ns }` seeds it exactly like
  `export { ns }` and the chain is credited in the value namespace as well:
  `typeof ns.member` keeps a value declaration reachable through a type-only
  re-export. And the seed does not ask whether the re-export itself has a
  consumer, so a namespace binding exported under its own name credits the
  chain behind it even when the report calls that very export unused, the same
  self-inconsistency the unreachable-observer case has.
- The exposed namespace closure is computed once per graph build, in
  `ModuleGraph::build`, and threaded into both phases that read it (Phase 2c
  namespace re-export propagation and the Phase 4 entry-star seed). It depends
  only on `ModuleNode::re_exports`, the entry-point flags, the consumers'
  whole-object uses, and entry-point reachability, none of which any phase
  after Phase 2 mutates. Reachability reads the edge list alone, so the build
  computes it once, before the closure, and hands the same bitset to
  `mark_reachable`.
- Styling and CSS-in-JS extraction must preserve source line mapping.
- Duplication token or normalization changes require the duplication cache
  version to move with the changed semantics.
- Exact duplication uses the configured strict, mild, weak, or semantic token
  normalization. The opt-in `duplicates.near` pass is orthogonal: it compares
  function-like spans with semantic shingles while retaining exact results from
  the configured mode. Candidate generation is deterministically bounded and
  reports skipped comparisons instead of hiding incomplete work.
- Near groups carry a required similarity inside the engine and convert to the
  shared output type only at the detector boundary. `CloneGroup::kind()` is the
  canonical way to distinguish exact and near groups after that conversion.
- Extracted-fragment language selection is centralized in the duplication
  tokenizer. Fingerprints preserve extension-specific tokenization, while
  function fragments use the closest JavaScript parser mode for their source.
- Clone-group spread is the exact maximum of same-file interval distance and
  lexical parent-directory tree distance. Its implementation must remain
  equivalent to the pairwise definition without doing pairwise path work.
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
(`./services/api` credits `./services/__mocks__/api`); a sibling that stays in
package space after alias substitution is dropped so it cannot fabricate a
phantom `@scope/__mocks__` package (issue #2213). A bare package specifier
(`vi.mock('axios')`, `jest.mock('@scope/pkg')`) additionally synthesizes a
root-level `__mocks__/<specifier>` candidate; the resolver probes ancestor
`__mocks__` directories of the test file up to the analysis root and credits
the manual mock file when it exists, so root-level node-module mocks do not
surface as unused files (issue #2225). A root mock with no matching
factory-less mock call keeps surfacing under Vitest, which applies manual
mocks only through `vi.mock`; under Jest the plugin's `__mocks__` entry
patterns keep every manual mock used, matching Jest's automatic node-module
mocking. "Anything unproven keeps coverage credit" describes masking
abstention only; it does not imply a credit edge exists.

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

Script-referenced files are support roots by default: they stay reachable for
dead-code analysis without contributing production reachability to dependency,
health, or security analysis. The npm `start`, `prestart`, and `poststart`
lifecycle is the narrow runtime exception, including declared scripts reached
through package-manager indirection. Projects deployed through a custom script
name should declare the deployed file in `entry` when no manifest, framework,
or infrastructure entry already identifies it as runtime code.

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
