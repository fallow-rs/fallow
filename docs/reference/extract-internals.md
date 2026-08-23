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
- A template pipeline that records member accesses for an import binding must
  make narrowing safe structurally, not shape by shape. Namespace-import
  narrowing (and CSS module, enum, and class member crediting) trusts the
  stream: one recorded access from a non-entry consumer narrows the target to
  the accessed members, so any mention the pipeline did not classify would
  turn a used sibling into an `unused-export` finding. Astro and MDX apply a
  completeness guard (`record_unexplained_mentions` in
  `crates/extract/src/template_expression_scan.rs`): every structured pass
  reports the byte spans it classified (Astro: component tag roots outside the
  masked `<script>` / `<style>` / comment ranges, and `{ ... }` regions the
  parser accepted; MDX: prose lines outside fenced code and inline code
  spans), and every identifier-boundary mention of an import binding outside
  those spans (a `define:vars` or `set:html` directive on a masked tag, an
  HTML comment, an attribute string, text content, a rejected region, a code
  sample, a template literal mistaken for a code span) records a whole-object
  use, which keeps the graph on mark-all for entry-point and non-entry
  consumers alike. The script side the visitor parsed (Astro frontmatter, MDX
  `import` / `export` lines) is guarded too
  (`record_unexplained_script_mentions`), because the visitor records a bare
  identifier as a whole-object use only for an allow-list of positions: for
  the bindings the graph narrows exports for (namespace imports and CSS
  module default imports) a mention outside the import declaration is
  explained only by a static dotted access whose `(root, member)` pair the
  visitor recorded or by a JSX tag root, so `const N = NS`, `NS as T`,
  `pick(NS)`, `Object.assign({}, NS)`, `[NS]`, `{ all: NS }`, and
  `export const all = NS` record a whole-object use. Class and enum bindings
  are not script-guarded (a type annotation or `new` expression names them
  bare in ordinary code), so their member crediting stays at visitor parity
  with `.tsx` on the script side while the markup guard covers them. Narrowing
  applies only when every mention of the binding in the whole file was
  structurally understood; otherwise every export is credited, which for the
  dead-code crediting consumers degrades to over-credit. Member accesses also
  feed the security secret-source index, so MDX prose records a dotted chain
  only when its root is an import local of the file: prose never creates
  member accesses on foreign roots such as `process.env`.
- The MDX line scan hands the statement lines of the whole file to the parser
  as one program, and a rejected program is an empty program, so one
  misclassified line would drop every import of the file. A line opens a
  statement only when it carries a shape a real statement has: a source
  clause, a brace specifier list, a star specifier, a string-literal
  side-effect import, or, after `export`, a brace list, a star, or a
  declaration keyword. Prose that merely opens with the word "import" or
  "export" stays prose. The classification is backed by a parse fallback: the
  scan keeps statement blocks (an opening line plus the continuation lines a
  multi-line specifier list collected), and when the parser rejects the body,
  every block it also rejects on its own is demoted to prose and the rest is
  re-parsed, so a rejected line costs only itself. Demoted lines feed the
  prose scan like any other body line, so the completeness guard above still
  sees their mentions. A source clause is a `from` bounded by whitespace on
  the left and by whitespace or its specifier quote on the right, so every
  whitespace form JavaScript accepts (`from\t'./x'`, a no-break space, a
  multi-space run) names a source, while a `from` inside a word does not.
- The parse fallback covers the dead-code path only. The duplication tokenizer
  reads MDX through `extract_mdx_statements`, which returns the classified
  statement body without a retry, so a line the classifier accepts and the
  parser then rejects still costs that MDX file its whole token stream there.
  Duplication findings inside such a file are missing rather than wrong, and
  the classifier is what keeps the common prose sentence out of that path.

## Verification

Add the smallest parser or visitor test for the syntax boundary. Add an
integration fixture when the extracted fact changes reachability or a reported
issue. Include malformed input for parser recovery changes.

```bash
cargo test -p fallow-extract
cargo test -p fallow-core
npm run verify:fast
```
