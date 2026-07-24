# Extraction internals

Use this reference for parsing, AST facts, embedded languages, source mapping,
and parse-cache changes.

## Ownership

- `crates/extract/src/lib.rs`: parse entry points, parallel dispatch, and
  cache-aware file processing.
- `crates/extract/src/parse.rs`: Oxc parser and semantic setup.
- `crates/extract/src/visitor/`: JavaScript and TypeScript import, export,
  member, call, and framework facts.
- `crates/extract/src/cache/`: cache types, conversion, storage, and tests.
- `crates/extract/src/complexity.rs`: JavaScript and TypeScript complexity.
- `crates/extract/src/sfc.rs`, `astro.rs`, `glimmer.rs`, `mdx.rs`, and
  `graphql.rs`: component and embedded-language extraction.
- `crates/extract/src/sfc_template/`: template-visible usage for supported
  component formats.
- `crates/extract/src/css.rs`, `css_metrics.rs`, `css_classes.rs`, and
  `css_in_js/`: CSS, CSS-in-JS, token, and styling facts.
- `crates/extract/src/source_map.rs`: source-map normalization and mapping.

Shared extraction result types live in `crates/types/src/extract.rs`.

## Invariants

- Keep extraction syntactic and tolerant of incomplete source.
- Preserve byte offsets and line numbers when lifting embedded code or styles.
- Return partial information and diagnostics when one input cannot be read or
  parsed. Do not abort unrelated files.
- Bind framework heuristics to imported symbols or other provenance. A local
  function with the same name must not activate library-specific behavior.
- Avoid filesystem and graph policy inside AST visitors.
- Change the cache version in `crates/extract/src/cache/types.rs` whenever a
  cached fact or its meaning changes. Do not document the current numeric value
  as a durable contract.
- Keep cache serialization deterministic and backwards failure safe.

## Verification

Add the smallest parser or visitor test for the syntax boundary. Add an
integration fixture when the extracted fact changes reachability or a reported
issue. Include malformed input for parser recovery changes.

```bash
cargo test -p fallow-extract
cargo test -p fallow-core
npm run verify:fast
```
