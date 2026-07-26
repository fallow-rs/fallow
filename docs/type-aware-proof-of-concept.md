# Type-aware TypeScript analysis

Fallow's default analysis remains fast, syntactic, and independent of Node.js
or TypeScript. `--type-aware` opts into a bounded TypeScript-Go semantic pass
after Fallow's normal project analysis. It provides five project-wide
capabilities:

- exact symbol-use, TypeScript contract, and validated framework-contract decisions for existing dead-code candidates
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
symbol is reachable, whether a class member is required by an interface or base
class, how a public export crosses package boundaries, which tests are
affected, and where public signatures create coupling. The companion returns
only semantic evidence, completeness, omissions, and stable reason codes. It
never forwards a compiler diagnostic as a Fallow issue.

## Five semantic capabilities

Refine existing dead-code findings conservatively:

```bash
fallow dead-code --type-aware --unused-exports --unused-types \
  --unused-class-members --format json --quiet
```

Export refinement follows the checker identity through renamed barrels,
generic and conditional types, declaration merging, `import("./module").Type`,
and `typeof import("./module").Value`. Fallow only asks these semantic
questions for exports its normal module graph already considers unused.

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
fallow dead-code --type-aware --symbol-impact src/repository.ts:UserRepository.save \
  --format json --quiet
```

The class-method selector resolves the exact exported class and method. It
never chooses between overloads or same-named classes heuristically. Unsupported
or ambiguous targets remain advisory with a stable reason.

Inspect advisory public-signature type coupling without changing the health
score:

```bash
fallow health --type-aware --type-coupling --format json --quiet
```

Preview and apply class-member cleanup only when Fallow has complete
closed-world evidence and an exact declaration guard:

```bash
fallow fix --type-aware --dry-run --format json --quiet
fallow fix --type-aware --yes --format json --quiet
```

## Fail-closed behavior

Every candidate has one of five outcomes:

- `confirmed-used`: exact static references were found, so Fallow removes the
  syntactic finding
- `contract-preserved`: an interface or abstract member requires the
  declaration, an override changes inherited behavior, or a detected framework
  contract resolves to the exact package declaration, so Fallow removes the
  syntactic finding
- `confirmed-no-static-references`: every owning project was checked and no
  static references were found, so Fallow keeps the finding
- `retained-unresolved`: the exact declaration or owning project could not be
  resolved, so Fallow keeps the finding
- `retained-abstained`: dynamic behavior, unsafe syntax, incomplete coverage,
  or another known gap prevents a closed-world decision, so Fallow keeps the
  finding

`confirmed-no-static-references` is intentionally not called “proven unused”.
Reflection, framework registration, external consumers, and runtime behavior
can still exist beyond the checker-visible world. Existing export and type
removal actions remain automatically applicable only after a complete negative
semantic decision. Class-member findings additionally require complete evidence
from every owning project, no contract or dynamic gap, and a matching
declaration hash. `fallow fix` reruns the same semantic analysis and verifies
the declaration hash immediately before editing.

The current safety policy abstains for every candidate in a TypeScript project
when configuration, program, syntax, or bind diagnostics make its structure
unsafe. It also abstains per candidate for decorators, dynamic computed member
access, optional contracts, accessor pairs, overload sets, abstract
declarations, attached comments, or a dynamic import that can consume an export
without an exact static property reference. Ordinary semantic diagnostics do
not invalidate an exact declaration match, so Fallow does not request a
separate full semantic diagnostic pass before scanning. It also avoids
TypeScript's project-wide global diagnostic pass, which eagerly checks the
whole program without adding confidence to an exact declaration match. Fallow
also abstains when no project can be selected. If an external framework
declaration cannot be attributed to an exact package, Fallow records
`framework-contract-provenance` and keeps the candidate. Warnings and stable
reason codes explain each abstention class.

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

The current semantic wire protocol is version 6. Private-type-leak
reconciliation sends only Fallow's bounded candidate set and receives stable
candidate IDs. A complete response may remove an unmatched syntactic candidate.
If project selection or an entry point is incomplete, the response is partial
and Fallow retains every unconfirmed candidate. The sidecar never turns an
unexpected backend or programming failure into an `unsupported-syntax`
abstention.

Protocol version, sidecar version, backend version, selected configs,
per-project status, per-candidate decisions, contract evidence, outcome counts,
guarded-fix eligibility, abstention reasons, warnings, total elapsed time, and
project setup, diagnostics, and symbol-scan timings are recorded under
`_meta.type_aware` in JSON output. Each selected project constructs one Program.
Symbol use, trace, and impact queries share one batched source traversal per
Program. Editor sessions retain one root-bound Program across saved snapshots,
apply source changes incrementally, and fully invalidate on TypeScript, Fallow,
package, lockfile, or declaration-resolution changes. Audit head and base
remain isolated. The command's top-level `elapsed_ms` includes the semantic pass. Human,
compact, markdown, JSON, grouped JSON, and SARIF preserve the semantic analysis
identity or a bounded provenance summary.

Combined analysis, audit, MCP, LSP, VS Code, the Rust API, and Node bindings use
the same exact-version protocol. The VS Code package bundles the companion and
its TypeScript runtime. CI integrations install the matching optional package.
Baseline and regression comparisons include semantic mode identity so a
syntactic run is never silently compared with a type-aware run.

When no semantic query is needed, the companion is not started.
`_meta.type_aware.executed` is `false`, companion provenance is omitted, and the
deferred analysis identity avoids inventing a concrete TypeScript project hash.
`--type-aware-require complete` turns an
incomplete semantic result into an analysis error after the conservative result
has been produced; the default `best-effort` policy keeps it advisory.

## Corpus adjudication and release gate

The local corpus ledger uses schema version 2. Project-level feature buckets are
copied into `suggested_feature_buckets` only as review prompts. A reviewer must
record the buckets that genuinely apply to each candidate in
`adjudicated_feature_buckets`; suggestions never contribute to the GO gate.

The corpus uses clean worktrees at exact public refs and does not create or
reuse fixture-local `node_modules`. TypeScript dependency resolution is pinned
to the Fallow workspace ancestor, recorded with hashes of its `package-lock.json`
and installed package manifests and type declarations. Any other ancestor
dependency root is rejected. Published dependency paths explicitly use the
Fallow workspace root as their base. Discovery records the
manifest, release binary, full sidecar runtime, TypeScript installation,
platform, fixture commits, and dependency provenance. Partial runs must use a
separate output directory, so they cannot overwrite canonical publication
artifacts.

The evidence phase executes the exact `--sidecar-bin` artifact recorded during
discovery. It does not import semantic implementation modules from the current
workspace. The ledger records the sidecar hash, protocol version, and digests of
the complete request and normalized response sets. Raw bounded stdout and
stderr are retained under the local corpus artifact directory for auditability.
Ledger verification recomputes both digests from the pinned requests and stored
raw responses. A hash mismatch, malformed or oversized response, missing query
result, duplicate query identity, invalid source location, timeout, or non-zero
sidecar exit fails the evidence phase.

```bash
npm run type-aware:corpus -- prepare
npm run type-aware:corpus -- focused \
  --sidecar-bin tools/type-aware-sidecar/fallow-type-aware.mjs
npm run type-aware:corpus -- discover \
  --sidecar-bin tools/type-aware-sidecar/fallow-type-aware.mjs
npm run type-aware:corpus -- measure \
  --sidecar-bin tools/type-aware-sidecar/fallow-type-aware.mjs
npm run type-aware:corpus -- evidence \
  --sidecar-bin tools/type-aware-sidecar/fallow-type-aware.mjs
npm run type-aware:corpus -- adjudicate
npm run type-aware:corpus -- verify-ledger
npm run type-aware:corpus -- supplemental \
  --sidecar-bin tools/type-aware-sidecar/fallow-type-aware.mjs
npm run type-aware:corpus -- capabilities \
  --sidecar-bin tools/type-aware-sidecar/fallow-type-aware.mjs
npm run type-aware:corpus -- summarize \
  --sidecar-bin tools/type-aware-sidecar/fallow-type-aware.mjs
npm run type-aware:corpus -- verify-publication \
  --sidecar-bin tools/type-aware-sidecar/fallow-type-aware.mjs
```

Use `--project` only with a non-canonical `--out-dir` for iteration. The
publication gate requires the complete manifest, repeated normalized output,
complete paired measurements, source-current evidence, checked-in review
decisions, independent signoff, and explicit accuracy, abstention, runtime, and
memory thresholds. Generate the runtime-bound supplemental and capability
artifacts before the summary. The publication verifier rejects drift in the
tracked evidence, summary, supplemental smoke, and capability proof. The
supplemental command
materializes the exact pinned Vitest commit without a fixture-local dependency
install, records the approved workspace dependency root and lockfile hash,
reruns it twice, and binds its canonical candidate sets to the independently
reviewed set. Publication verification repeats that clean supplemental run
instead of trusting stored hashes.

The gate reports safe confirmation yield and independently adjudicated truth
coverage. It does not claim corpus-wide recall: retained candidates without
independent truth remain `indeterminate` and are excluded from adjudicated
accuracy metrics.

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
