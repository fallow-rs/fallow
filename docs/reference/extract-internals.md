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
- `crates/extract/src/template_complexity/`: synthetic `<template>` complexity
  for Angular, Vue, Svelte, and Astro, over a shared JS-expression engine.
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
- A string-literal-named `TSModuleDeclaration` body (module augmentation or
  ambient module declaration) contributes no file-level export surface. Its
  body is still walked for `typeof import()` and type-space references, and a
  named re-export inside it becomes one type-space import per specifier so the
  target keeps its export credit. A star re-export inside it (`export *` or
  `export * as ns`) becomes one type-space namespace import with an empty
  local name and never a file-level star re-export; the graph credits the
  target's full ES star surface for that shape (see
  `docs/reference/detection-internals.md`). The `export * as ns` form adds one
  type-space default import because `ns.default` reaches the target's default
  export. `export type *` inside the body keeps its file-level type-only star
  re-export, because the whole-module import shape carries no type modifier.
  Because ambient bodies are erased at runtime, a re-export from a bare
  specifier inside one counts as type-only package usage. Exported namespaces
  and `declare global` keep their existing behavior.
- A namespace declared without the `export` keyword (`namespace Foo {}`,
  `declare namespace Foo {}`, legacy `module Foo {}`, dotted
  `namespace A.B.C {}`, and namespaces nested in those or in `declare global`)
  is a local binding. Its inner `export` declarations are members of that
  binding, not file-level exports, and are attached to no owner because a
  local namespace cannot merge with an exported one. The body is still walked
  so imports referenced inside it keep their credit. `export namespace Foo`
  keeps recording one export with the inner declarations as members.

## Verification

Add the smallest parser or visitor test for the syntax boundary. Add an
integration fixture when the extracted fact changes reachability or a reported
issue. Include malformed input for parser recovery changes.

```bash
cargo test -p fallow-extract
cargo test -p fallow-core
npm run verify:fast
```
