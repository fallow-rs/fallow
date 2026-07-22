# fallow-type-aware

Optional TypeScript-Go semantic refinement sidecar for Fallow. It accepts one
versioned JSON request on stdin and writes one JSON response to stdout.

The sidecar is deliberately narrower than a general type-aware linter. It
receives Fallow's filtered unused class-member candidates, loads each applicable
TypeScript project once, batches property-access symbol lookups, and confirms a
use only when the resolved declaration matches the exact candidate identity.
Unsafe project state produces explicit abstentions and never removes findings.

## Run locally

```sh
npm ci
npm test
./fallow-type-aware.mjs < request.json
```

The implementation pins `typescript@7.0.2` because `typescript/unstable/sync` is an
explicitly unstable API. Package version, protocol version, and TypeScript backend
version are validated independently. See
[`docs/type-aware-proof-of-concept.md`](../../docs/type-aware-proof-of-concept.md)
for the Fallow integration contract, safety policy, and current limitations.

## Release

Publication is not enabled yet. It remains gated on the adjudicated corpus
accuracy review described in the integration contract. Before the first
release, add a reviewer-protected GitHub environment, protected release tags,
and an npm trusted publisher that uses OIDC without a long-lived token.
