# Fallow type-aware sidecar proof of concept

This private reference sidecar lets the experimental `fallow dead-code --type-aware`
path ask TypeScript 7's native type checker which syntactic unused class-member
candidates have a confirmed property-access use.

It is deliberately narrow. The sidecar accepts one versioned JSON request on stdin,
selects TypeScript's default project for every candidate file, and writes one JSON
response to stdout. It only confirms dot-property accesses whose resolved declaration
matches the candidate path, owner class, member name, and member kind. Everything else
stays unresolved so Fallow keeps the original finding.

## Run locally

```sh
npm ci
npm test
./fallow-type-aware.mjs < request.json
```

The implementation pins `typescript@7.0.2` because `typescript/unstable/sync` is an
explicitly unstable API. This package is a development proof of concept, not a shipped
runtime dependency.
