# Experimental type-aware class-member refinement

Fallow's default analysis remains fast, syntactic, and independent of Node.js
or TypeScript. `dead-code --type-aware` adds an optional semantic refinement for
`unused-class-members` findings. Fallow completes its normal filtering first,
then asks a TypeScript-Go sidecar to resolve only the remaining candidates.

The sidecar is not published yet. Publication remains gated on an adjudicated
corpus accuracy review. Repository development uses the checked-in package:

```bash
npm ci --prefix tools/type-aware-sidecar
FALLOW_TYPE_AWARE_BIN="$PWD/tools/type-aware-sidecar/fallow-type-aware.mjs" \
  target/release/fallow dead-code --unused-class-members --type-aware \
  --format json --quiet
```

After publication, the intended installation is:

```bash
npm install --save-dev fallow-type-aware
FALLOW_TYPE_AWARE_BIN="$PWD/node_modules/.bin/fallow-type-aware" \
  fallow dead-code --unused-class-members --type-aware --format json --quiet
```

The sidecar pins `typescript@7.0.2` and uses `typescript/unstable/sync`, which
runs the native TypeScript-Go backend. A finding is removed only when a property
access resolves to the exact declaration path, owner, kind, line, and UTF-8 byte
column supplied by Fallow. Name-only matches are never enough.

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
confidence to an exact declaration match. Fallow also abstains
when no project can be selected or multiple explicit projects contain the same
file. Warnings and stable reason codes explain each abstention class.

Use repeatable `--type-aware-project PATH` options when automatic project
selection is ambiguous. Paths may point to an ancestor config outside `--root`:

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

## Output contract

Protocol version, sidecar version, backend version, selected configs,
per-project status, outcome counts, abstention reasons, warnings, total elapsed
time, and project setup, diagnostics, and symbol-scan timings are recorded under
`_meta.type_aware` in JSON output. The command's top-level `elapsed_ms` includes
the semantic pass. When no class-member candidates remain after normal Fallow
filtering, the sidecar is not started and `_meta.type_aware` is omitted because
no semantic pass was computed. Explicit project paths are still validated.

The experimental mode currently supports human and JSON output only. Baseline,
regression, trace, impact-closure, SARIF, audit, combined, MCP, LSP, and watch
integration remain disabled until those surfaces can preserve semantic
provenance and analysis-mode identity.

Reflection, decorators, dependency injection, computed property access, and
framework runtime registration can remain invisible to the checker. Without
`--type-aware`, the sidecar is never discovered or started and existing output
is unchanged.
