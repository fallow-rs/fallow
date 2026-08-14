# fallow-type-aware

Optional TypeScript-Go semantic refinement sidecar for Fallow. It accepts one
versioned JSON request on stdin and writes one JSON response to stdout.

The sidecar is deliberately narrower than a general type-aware linter. Protocol
v6 accepts a bounded set of tagged `symbol-use`, `symbol-trace`, `api-surface`,
`symbol-impact`, and `type-coupling` queries. Each selected TypeScript project
creates one Program. Symbol use, trace, and impact queries share one indexed
source traversal per Program.

Symbol identities include the canonical project-relative path, value or type
namespace, declaration kind, exported and local name, one-based line,
zero-based UTF-8 byte column, and optional owner. Results keep their semantic
assertion separate from `complete`, `partial`, or `unavailable` status. Evidence
and every operation-specific array are deterministic and bounded, with totals,
omissions, reason codes, actions, and truncation reported explicitly.

Unsafe project state never manufactures certainty. Structural diagnostics,
unknown identities, missing projects, unsupported syntax, dynamic behavior,
and capacity limits retain syntactic findings or produce an explicit advisory
gap. Required interfaces, abstract members, and overrides are returned as exact
contract evidence. Complete negative evidence carries an exact UTF-8 byte span
and SHA-256 declaration guard, but Fallow owns the final decision and fix
policy. The sidecar does not emit TypeScript compiler diagnostics as Fallow
findings and does not implement generic typed lint rules.

The raw TypeScript-Go host cannot currently expose Svelte virtual-module named
exports. If source code imports or re-exports such a name and checker resolution
has no declaration target, the sidecar returns
`svelte-virtual-module-exports` instead of claiming complete evidence. Run
`svelte-check` for framework diagnostics; Fallow stays fail-closed until a
supported host seam can preserve virtual source identity and mappings.

## Run locally

```sh
npm ci
npm test
./fallow-type-aware.mjs < request.json
```

The implementation pins `typescript@7.0.2` because `typescript/unstable/sync`
is an explicitly unstable API. That backend detail is contained behind
Fallow's stable, exact-version protocol. Package version, protocol version, and
TypeScript backend version are validated independently. See
[`docs/type-aware-analysis.md`](../../docs/type-aware-analysis.md)
for the Fallow integration contract, safety policy, and current limitations.

## Release

Publication remains a separate release action. The package is designed to be
installed as an exact-version optional companion and launched through a
verified absolute path supplied by Fallow. It does not search the analyzed
project or arbitrary PATH entries for a backend.
