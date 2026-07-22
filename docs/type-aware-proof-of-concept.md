# Type-aware class-member proof of concept

Fallow's default analysis remains syntactic and does not require Node.js or the
TypeScript compiler. This proof adds an explicit experimental refinement for
`unused-class-members` findings:

```bash
npm ci --prefix tools/type-aware-sidecar
FALLOW_TYPE_AWARE_BIN="$PWD/tools/type-aware-sidecar/fallow-type-aware.mjs" \
  fallow dead-code --unused-class-members --type-aware --format json --quiet
```

The reference sidecar pins `typescript@7.0.2` and uses
`typescript/unstable/sync`, which runs the native TypeScript-Go backend. Fallow
runs its normal analysis and filters first, then sends only the remaining class
member candidates to the sidecar. A finding is removed only when the checker
resolves a property access to that exact declaration. Unresolved candidates
remain findings.

This proof validates semantic value and the backend-neutral protocol. It does
not provide production packaging, persistent configuration, caching, MCP or
editor integration, or broader type-aware issue families. Candidate filtering
reduces protocol traffic, but the sidecar still loads the applicable TypeScript
programs. Reflection, decorators, dependency injection, computed property
access, and framework runtime registration can remain invisible to the
checker.

When requested, protocol, discovery, or process failures exit with code 2.
Ordinary TypeScript source diagnostics are returned as bounded warnings and do
not abort the refinement. JSON output records the backend, compiler version,
selected project configs, candidate counts, bounded warnings, and elapsed time
under `_meta.type_aware`. Projects without a `tsconfig.json` use the explicit
`<inferred>` project marker.

The environment override is `FALLOW_TYPE_AWARE_BIN`. Without `--type-aware`,
the sidecar is never discovered or started and existing output is unchanged.
