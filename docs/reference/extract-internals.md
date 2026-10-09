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
- `crates/extract/src/complexity.rs`: JavaScript and TypeScript complexity,
  including the synthetic `<module>` unit for module-scope branching.
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
- A concise arrow body (`() => expr`) is an `ArrowFunctionBody` expression
  since Oxc 0.151. Helpers that read function and arrow bodies take
  `BodyRef` (`crates/extract/src/function_body.rs`), so a concise body keeps
  the shape of the old single-statement body. The duplication token visitor
  keeps the old tokens for concise bodies, `import.meta`, `new.target` and
  qualified interface heritage names.
- `SemanticBuilder` builds `AstNodes` only with `with_build_nodes(true)`.
  Set it on each pass that reads `semantic.nodes()`.
- A `TSExternalModuleDeclaration` body (`declare module '<specifier>'`, a
  module augmentation or ambient module declaration) contributes no file-level export surface. Its
  body is still walked for `typeof import()` and type-space references, and a
  named re-export inside it becomes one type-space import per specifier so the
  target keeps its export credit. A star re-export inside it (`export *` or
  `export * as ns`) becomes one type-space namespace import with an empty
  local name and never a file-level star re-export; the graph credits the
  target's full ES star surface for that shape (see
  `docs/reference/detection-internals.md`). The `export * as ns` form adds one
  type-space default import because `ns.default` reaches the target's default
  export. `export type *` and `export type * as ns` record the same shape with
  the import's type-only-star flag set, so the graph credits the same star
  surface in the type namespace alone. Because ambient bodies are erased at
  runtime, a re-export from a bare specifier inside one counts as type-only
  package usage. Exported namespaces and `declare global` keep their existing
  behavior.
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
  (`record_unexplained_script_mentions`), because the text scan cannot see
  what the parser resolved: for the bindings the graph narrows exports for
  (namespace imports and CSS module default imports) a mention outside the
  import declaration is explained only by a static dotted access whose
  `(root, member)` pair the visitor recorded or by a JSX tag root, so
  `NS['Moon']` and `NS?.Moon`, which the visitor resolves exactly, record a
  whole-object use there. On the CSS-module side the guard is also what
  covers `const N = styles`, `pick(styles)`, `[styles]`, and
  `{ all: styles }`. Class and enum bindings
  are not script-guarded (a type annotation or `new` expression names them
  bare in ordinary code), so their member crediting stays at visitor parity
  with `.tsx` on the script side while the markup guard covers them. Narrowing
  applies only when every mention of the binding in the whole file was
  structurally understood; otherwise every export is credited, which for the
  dead-code crediting consumers degrades to over-credit. Member accesses also
  feed the security secret-source index, so MDX prose records a dotted chain
  only when its root is an import local of the file: prose never creates
  member accesses on foreign roots such as `process.env`.
- A reference to an `import * as NS` local that the visitor cannot resolve to
  one member is a whole-object use, so the graph credits every export of the
  target instead of narrowing to the dotted accesses
  (`record_bare_namespace_reference` in
  `crates/extract/src/visitor/visit_impl.rs`). The resolved positions are the
  exclusions: the object of a static or string-computed access, the root of a
  JSX member tag, the left side of a dotted type name, a destructure
  initializer, a re-export specifier local (which the graph credits through
  its own rule), and a placement in an object literal bound to a local, whose
  `api.NS.member` path the object-binding resolver follows. A test spy call
  with a static member name (`vi.spyOn(NS, 'm')`, `jest.spyOn`, a
  test-framework or global `spyOn`, the `mock.method` of `node:test`) is also
  an exclusion and records `NS.m` (`visit_impl_spy_calls.rs`). The spy API
  names are pre-registered from the program's statement list, so a top-level
  binding of the same name removes the global meaning. A bare reference
  to that local hands the namespace on in turn. The locals are pre-registered
  from the program's statement list, because an import declaration is legal
  after the code that reads it. Namespace objects bound by `require` or a
  dynamic import keep the older allow-list.
- The MDX line scan hands the statement lines of the whole file to the parser
  as one program, and a rejected program is an empty program, so one
  misclassified line would drop every import of the file. A line opens a
  statement only when it carries a shape a real statement has: a source
  clause, a brace specifier list, a star specifier, a string-literal
  side-effect import, or, after `export`, a brace list, a star, or a
  declaration keyword. Prose that merely opens with the word "import" or
  "export" stays prose. The shape list is a fast path, not the definition of
  the language: a line that opens with the keyword and matches nothing in it
  is handed to the parser on its own, and a line that parses is a statement
  whatever its shape, which keeps heads the list does not enumerate
  (`import /* c */ './x'`, a spaced dynamic `import ('./x')`) out of prose. A
  sentence cannot slip through that probe, because a sentence does not parse.
  Classification is backed by a parse fallback in the other direction: the
  scan keeps statement blocks (an opening line plus the continuation lines a
  multi-line specifier list collected), and when the parser rejects the body,
  every block it also rejects on its own is demoted to prose and the rest is
  re-parsed, so a rejected line costs only itself. Demoted lines feed the
  prose scan like any other body line, so the completeness guard above still
  sees their mentions. A source clause is a `from` bounded by whitespace on
  the left and followed by its specifier quote on the right, immediately or
  after whitespace, so every whitespace form JavaScript accepts (`from\t'./x'`,
  a no-break space, a multi-space run) names a source, while a `from` inside a
  word or inside a string does not, and a multi-line block is never ended one
  line early by prose in an object literal.
- The parse fallback covers the dead-code path only. The duplication tokenizer
  reads MDX through `extract_mdx_statements`, which returns the classified
  statement body without a retry, so a line the classifier accepts and the
  parser then rejects still costs that MDX file its whole token stream there.
  Duplication findings inside such a file are missing rather than wrong, and
  the classifier is what keeps the common prose sentence out of that path.
- Complexity extraction opens a root frame per program, so decision points
  outside every function are counted instead of dropped by the
  `stack.last_mut()` guard every counter writer uses. The frame is emitted as a
  synthetic `<module>` unit only when it actually branched, anchored at its
  first contributing construct and sized from its first to its last
  contribution, with `source_hash: None`. A file with no module-scope decision
  point produces no unit, so its numbers are unchanged. The unit is
  aggregate-only downstream: vital signs, file-score totals, and branching
  conservation count it, while findings, large functions, CRAP, the component
  rollup, the runtime-coverage static payload, and the editor code lens all
  exclude it.

## Embedded-source and import regression boundaries

Asset references in HTML, JSX and HTML tagged templates must come from literal
markup with the required element or tag context. Remote URLs, unrelated
components and unproven interpolated paths must not invent local graph edges.
Keep comment and source-offset handling aligned across `html.rs`, the visitor
and SFC extraction. CSS masking replaces excluded regions with equal-length
whitespace; selector spans must still index the original source. Cascade-layer
and import preludes are not class selectors, while real scope selectors remain
visible.

Vue setup bindings and Svelte instance-script bindings have different template
visibility from ordinary Vue scripts and Svelte module scripts. External script
and style references remain graph edges. SFC style imports carry their style
context into resolution so a stylesheet cannot resolve to a same-named
component through the JavaScript extension order. Preserve template-local
bindings, whole-object credit and framework metadata when changing scanners.

Import facts also come from JSDoc type references, import-then-export syntax,
namespace destructuring and proven Node child-process entry calls. These paths
must retain import provenance, type/value meaning and lexical binding identity.
Do not replace their source tests with tests of a parallel scanner. Angular
metadata and injection-token/interface bridging must stay provenance-gated;
unknown framework behavior must not invent member-use evidence.

A per-file `@jsxImportSource <source>` pragma is an import fact too. With the
automatic JSX runtime, the compiler imports `jsx`, `jsxs` and `Fragment` from
`<source>/jsx-runtime`, so the visitor records these three named imports with
no local binding. The visitor records them only when the file has a JSX
element or fragment, as the compiler does. Only comments before the first
statement hold the pragma, as in TypeScript, so a `@jsxImportSource` line in a
later doc comment adds no edge. Inside those comments, the Oxc rules apply: the
last value wins, and `@jsxRuntime classic` cancels the pragma.
The dev runtime (`<source>/jsx-dev-runtime`) is not recorded, because a build
can omit it and a missing relative dev runtime would be a false unresolved
import. The tsconfig `jsxImportSource` option only credits the package as a
referenced dependency.

A Vitest config can set the source for a group of files with
`oxc.jsx.importSource` (or the older `esbuild.jsxImportSource`). Vitest runs
the transform in dev mode, so such a file imports `jsxDEV` and `Fragment`
from `<source>/jsx-dev-runtime`. Extraction stays config-blind: the visitor
sets `jsx_runtime_from_config` on a file with JSX and with no
`@jsxImportSource` or `@jsxRuntime classic` pragma. The Vitest plugin reads
the source, the `test.include` globs and the `test.exclude` globs of each test
project into a `JsxImportSourceRule`.

The plugin follows the Vitest 5 project model:

- Without `test.projects`, the config is the only test project. With
  `test.projects`, the root config gives no rule, because it is then not a
  test project.
- An inline project merges the declaring config unless `extends` is `false`.
  Vitest 4 merged it only with `extends: true`. The plugin reads the Vitest
  major from `node_modules/vitest/package.json`, else from the declared
  `vitest` range, in the directories from the config up to the root. On a
  major below 5, a project without `extends` merges nothing. When no major is
  known, or a range has no upper major (`>=4`, `latest`), the plugin follows
  Vitest 5.
- `extends: '<path>'` merges the named config file instead. The plugin reads
  that file one level deep and credits it as used. A path to the declaring
  config is the same as `extends: true`.
- A merge concatenates `include` and `exclude`, with the base values first,
  as Vite `mergeConfig` does. Other values of the project replace the base
  values.
- A project without `include` uses the Vitest default include. A project
  without `exclude` uses the Vitest default exclude
  (`**/node_modules/**`, `**/.git/**`). A config `exclude` replaces the
  default.
- A negated `include` entry is an exclude. Vitest ignores a negated `exclude`
  entry, so the plugin ignores it too. An exclude pattern that names a
  directory also excludes the files in it.
- The globs are relative to `test.dir`, else to `test.root` or the Vite
  `root`, else to the config directory. The plugin stores them relative to the
  config directory. A project directory outside the config directory gives no
  rule.
- A `test.projects` string that names a config file with a name such as
  `vitest.e2e.config.ts` is read for its rules, which match relative to the
  directory of that file. The config patterns already find a file with a
  standard name. A glob entry is expanded on disk, without `node_modules`.
  A glob that starts with `**` is not followed, because it walks the whole
  tree on each run.
  A glob with a brace group is not followed, because the glob matcher has no
  brace support.
- Vitest does not load a `vite.config.*` when a `vitest.config.*` is in the
  same directory. Such a vite config gives no rule, unless the vitest config
  imports it, for example to pass it to `mergeConfig`.

The classic runtime, `jsx: 'preserve'` and `oxc: false` give no rule. A
package source gives no rule and only credits the package. A synthetic edge
from test files alone would make a runtime dependency of the app look
test-only.

A project that sets no import source uses the `react` runtime, because Vite
uses `react` by default. The plugin records this as a credit rule in
`jsx_package_credits`, not as an edge rule. The analysis credits `react` as used
only when a flagged module matches the rule, so a project without JSX test
files still reports an unused `react`. The credit goes to one
manifest only, as for an import: the nearest manifest of the module
(its workspace, the ancestor workspaces, then the root) that declares the
package. A JSX test file in one workspace thus does not hide an unused
`react` in a different workspace. When a
tsconfig sets `jsxImportSource` or a `jsx` mode without an automatic runtime
import, Vite uses the tsconfig settings for TypeScript files. The analysis
then skips the credit rule for TypeScript files below the directory of that
tsconfig. The check uses the directory only, not the `include` of the
tsconfig.

The resolver adds the edge to each flagged module that an include glob
matches and no exclude glob matches, relative to the config directory. A
relative source resolves from the module first and then from the config
directory, as in Vite. A source that resolves from neither place adds no
edge, so a config value never causes an unresolved import. The edge rules,
with their include and exclude globs, are part of the graph cache key. The
credit rules add no edge, so they are not part of it.

## Verification

Add the smallest parser or visitor test for the syntax boundary. Add an
integration fixture when the extracted fact changes reachability or a reported
issue. Include malformed input for parser recovery changes.

```bash
cargo test -p fallow-extract
cargo test -p fallow-core
npm run verify:fast
```
