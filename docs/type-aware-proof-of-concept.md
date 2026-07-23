# Type-aware TypeScript analysis

Fallow's default analysis remains fast, syntactic, and independent of Node.js
or TypeScript. `--type-aware` opts into a bounded TypeScript-Go semantic pass
after Fallow's normal project analysis. It provides five project-wide
capabilities:

- exact symbol-use confirmation for existing dead-code candidates
- symbol traces with namespaces, aliases, and re-export hops
- package API surfaces and cross-file private type leaks
- exact-symbol impact paths and targeted-test suggestions
- advisory public-signature type coupling

Repository development uses the checked-in companion:

```bash
npm ci --prefix tools/type-aware-sidecar
FALLOW_TYPE_AWARE_BIN="$PWD/tools/type-aware-sidecar/fallow-type-aware.mjs" \
  target/release/fallow dead-code --unused-class-members --type-aware \
  --format json --quiet
```

Normal npm installations receive the exact matching optional companion package:

```bash
npm install --save-dev fallow
npx fallow dead-code --unused-class-members --type-aware --format json --quiet
```

Install `fallow-type-aware` at the same version manually only when optional npm
dependencies are disabled. The `fallow` launcher discovers an exact version
match and passes its trusted executable path to the native binary.

The companion pins `typescript@7.0.2` and uses `typescript/unstable/sync`, which
runs the native TypeScript-Go backend. Fallow only changes a finding when the
checker resolves an exact declaration identity. Name-only matches are never
enough.

## Ownership boundary with tsc and Oxlint

Type-aware Fallow does not emit TypeScript compiler diagnostics or general
typed lint findings. Keep `tsc --noEmit` responsible for compiler correctness.
Keep Oxlint responsible for local lint rules, including unused ECMAScript
`#private` members. TypeScript already reports unused TypeScript `private`
members when the relevant compiler checks are enabled.

Fallow owns the project-wide questions those tools do not answer: why an exact
symbol is reachable, how a public export crosses package boundaries, which
tests are affected, and where public signatures create coupling. The companion
returns only semantic evidence, completeness, omissions, and stable reason
codes. It never forwards a compiler diagnostic as a Fallow issue.

## Five semantic capabilities

Refine existing dead-code findings conservatively:

```bash
fallow dead-code --type-aware --unused-exports --unused-types \
  --unused-class-members --format json --quiet
```

Trace one exact exported symbol through aliases and re-exports:

```bash
fallow dead-code --type-aware --trace src/api.ts:Client \
  --format json --quiet
```

Inspect the package API surface and cross-file private type leaks:

```bash
fallow dead-code --type-aware --private-type-leaks --format json --quiet
```

Find exact consumers, affected files, and targeted tests:

```bash
fallow dead-code --type-aware --symbol-impact src/api.ts:Client \
  --format json --quiet
```

Inspect advisory public-signature type coupling without changing the health
score:

```bash
fallow health --type-aware --type-coupling --format json --quiet
```

## Fail-closed behavior

Every candidate has one of three outcomes:

- `confirmed used`: exact semantic evidence was found, so Fallow removes it
- `unresolved`: a diagnostic-free project was scanned, but no exact use was found
- `abstained`: project selection or compiler state was unsafe, so Fallow keeps it

The current safety policy abstains for every candidate in a TypeScript project
when configuration, program, syntax, or bind diagnostics make its
structure unsafe. Ordinary semantic diagnostics do not invalidate an exact
declaration match, so Fallow does not request a separate full semantic
diagnostic pass before scanning. It also avoids TypeScript's project-wide
global diagnostic pass, which eagerly checks the whole program without adding
confidence to an exact declaration match. Fallow also abstains when no project
can be selected. Warnings and stable reason codes explain each abstention class.

Use repeatable `--type-aware-project PATH` options when automatic project
selection does not include a consumer project. Every healthy explicit project
is scanned, so a member declared in one referenced package can be confirmed by
a use in another. Paths may point to an ancestor config outside `--root`:

```bash
fallow dead-code --root packages/app --unused-class-members --type-aware \
  --type-aware-project ../../tsconfig.json --format json --quiet
```

Fallow never searches project `node_modules/.bin` or `PATH` for this executable.
It accepts an explicit `FALLOW_TYPE_AWARE_BIN` path, or a regular non-symlinked
sidecar next to the active Fallow executable. Requests, responses, warnings,
candidate counts, project counts, and execution time are bounded. The child
process receives a minimal environment. Relative, missing, and project-local
search-path entries are removed before its Node interpreter is resolved. Its
complete process group is terminated on timeout.

## Output and integration contract

Protocol version, sidecar version, backend version, selected configs,
per-project status, outcome counts, abstention reasons, warnings, total elapsed
time, and project setup, diagnostics, and symbol-scan timings are recorded under
`_meta.type_aware` in JSON output. The command's top-level `elapsed_ms` includes
the semantic pass. Human, compact, markdown, JSON, grouped JSON, and SARIF
preserve the semantic analysis identity or a bounded provenance summary.

Combined analysis, audit, MCP, LSP, VS Code, the Rust API, and Node bindings use
the same exact-version protocol. The VS Code package bundles the companion and
its TypeScript runtime. CI integrations install the matching optional package.
Baseline and regression comparisons include semantic mode identity so a
syntactic run is never silently compared with a type-aware run.

When no semantic query is needed, the companion is not started and
`_meta.type_aware` is omitted because no semantic pass was computed. Explicit
project paths are still validated. `--type-aware-require complete` turns an
incomplete semantic result into an analysis error after the conservative result
has been produced; the default `best-effort` policy keeps it advisory.

## Corpus adjudication and release gate

The local corpus ledger uses schema version 2. Project-level feature buckets are
copied into `suggested_feature_buckets` only as review prompts. A reviewer must
record the buckets that genuinely apply to each candidate in
`adjudicated_feature_buckets`; suggestions never contribute to the GO gate.

The corpus uses clean worktrees at exact public refs and does not reuse fixture
`node_modules`. Discovery records the manifest, release binary, full sidecar
runtime, TypeScript installation, platform, and fixture commits. Partial runs
must use a separate output directory, so they cannot overwrite canonical
publication artifacts.

```bash
npm run type-aware:corpus -- prepare
npm run type-aware:corpus -- focused \
  --sidecar-bin tools/type-aware-sidecar/fallow-type-aware.mjs
npm run type-aware:corpus -- discover \
  --sidecar-bin tools/type-aware-sidecar/fallow-type-aware.mjs
npm run type-aware:corpus -- measure \
  --sidecar-bin tools/type-aware-sidecar/fallow-type-aware.mjs
npm run type-aware:corpus -- evidence
npm run type-aware:corpus -- adjudicate
npm run type-aware:corpus -- verify-ledger
npm run type-aware:corpus -- summarize \
  --sidecar-bin tools/type-aware-sidecar/fallow-type-aware.mjs
npm run type-aware:corpus -- supplemental \
  --sidecar-bin tools/type-aware-sidecar/fallow-type-aware.mjs
npm run type-aware:corpus -- verify-publication \
  --sidecar-bin tools/type-aware-sidecar/fallow-type-aware.mjs
```

Use `--project` only with a non-canonical `--out-dir` for iteration. The
publication gate requires the complete manifest, repeated normalized output,
complete paired measurements, source-current evidence, checked-in review
decisions, independent signoff, and explicit accuracy, abstention, runtime, and
memory thresholds. The publication verifier rejects drift in the tracked
evidence, summary, and supplemental smoke. The supplemental command
materializes the exact pinned Vitest commit without dependencies, reruns it
twice, and binds its canonical candidate sets to the independently reviewed
set. Publication verification repeats that clean supplemental run instead of
trusting stored hashes.

Feature-bucket value requires two separate candidates that are both manually
classified as used and correctly confirmed by semantic refinement. The two
candidates must provide explicitly adjudicated evidence for distinct buckets.
A single candidate tagged with multiple buckets does not satisfy the gate. The
current compact result is in
[`benchmarks/type-aware-corpus-summary.md`](../benchmarks/type-aware-corpus-summary.md),
with source coordinates and provenance in
[`benchmarks/type-aware-corpus-evidence.json`](../benchmarks/type-aware-corpus-evidence.json).
An additional clean Vitest smoke is recorded in
[`benchmarks/type-aware-supplemental-smoke.json`](../benchmarks/type-aware-supplemental-smoke.json).

Reflection, decorators, dependency injection, destructuring, dynamic computed
property access, and framework runtime registration can remain invisible to the
checker. These cases stay visible as findings. Without `--type-aware`, the
sidecar is never discovered or started and existing output is unchanged.
