//! Serialization types for the incremental parse cache.
//!
//! All types use bitcode `Encode`/`Decode` for fast binary serialization.

use bitcode::{Decode, Encode};

use crate::MemberKind;

/// Cache version, bump when the cache format or cached extraction semantics change.
///
/// Bumped to 89 for issue #475: extraction now strips a leading UTF-8 BOM
/// before hashing and computing line offsets, so pre-fix entries whose source
/// included a BOM carry hashes over the wrong byte sequence and would
/// fast-path into stale `member_accesses` / `exports` for any BOM-bearing
/// file. The bump invalidates user caches once on upgrade; subsequent runs
/// are warm.
///
/// Bumped to 90 for issue #540: CSS Modules class extraction now strips
/// `@layer` and `@import` at-rule preludes before scanning class names, so
/// pre-fix entries for `.module.css` files using nested cascade-layer syntax
/// (`@layer foo.bar { ... }`) carry phantom `bar` / `baz` exports that the
/// new scanner no longer produces.
///
/// Bumped to 91 for issue #549: CSS Modules class extraction now records a
/// real `Span` pointing at each class's declaration position in the source.
/// Pre-fix cache entries for `.module.css` / `.module.scss` files carry
/// `Span::default()` (start=0, end=0) on every export, which renders every
/// finding at line:1 col:0; the new scanner produces real offsets.
///
/// Bumped to 92 for issue #563: feature flag extraction recognizes additional
/// built-in SDK providers (PostHog, Vercel Flags, Optimizely, Eppo, plus more
/// ConfigCat surfaces) and Vercel `flag({ key: "..." })` object arguments, so
/// pre-fix entries can carry stale `flag_uses`.
///
/// Bumped to 93 for issue #589: Node `module.register()` loader calls now
/// emit `DynamicImportInfo.destructured_names` populated with the loader-hook
/// allowlist (current `initialize` / `resolve` / `load` / `globalPreload`
/// plus legacy `getFormat` / `getSource` / `transformSource`) for every
/// relative or `file:` specifier, including specifiers bound via
/// `new URL(..., import.meta.url)`. Pre-fix entries carry empty
/// `destructured_names` for the same source, so they would silently miss
/// the named-export credit until the file is touched.
///
/// Bumped to 94 for issue #586: Playwright helper fixture extraction recognizes
/// helpers with local setup before the final `return base.extend<T>(...)`, so
/// pre-fix entries can miss fixture definition sentinels.
///
/// Bumped to 95 for the Glimmer `<template>` scanner: imported-binding usage
/// and `MemberAccess { object: "this", member }` records for `{{this.foo}}`
/// template references are now folded into the extractor before
/// `into_module_info`. Pre-fix entries for `.gts` / `.gjs` files omit both,
/// so template-only imports surface as `unused-import` and template-only
/// class members as `unused-class-member` until the cache is re-extracted.
///
/// Bumped to 96 for issue #640: generic JSX `<script src>` and
/// `<link rel="stylesheet|modulepreload" href>` attributes no longer emit
/// synthetic `SideEffect` imports, so pre-fix entries can carry stale JSX
/// resource edges that surface as false `unresolved-imports`.
///
/// Bumped to 97 for issue #639: MDX import/export extraction now skips
/// fenced Markdown code blocks, so pre-fix entries can carry stale example
/// imports that surface as false `unresolved-imports`.
///
/// Bumped to 98 for issue #638: statically resolvable `child_process.fork()`
/// targets now emit `DynamicImportInfo` entries for local runner files.
/// Pre-fix entries omit those dynamic imports, so forked script files can be
/// reported as unused until the file is re-extracted.
///
/// Bumped to 99 for issue #605: methods reached via `new Class(...).method()`
/// receivers (direct and fluent-chain) now emit member accesses crediting the
/// constructed class. Pre-fix entries lack those accesses, so such methods can
/// be reported as unused class members until the file is re-extracted.
///
/// Bumped to 100 for issue #608: static Iconify icon strings (`icon="jam:github"`,
/// `name="ic:round-home"`) in markup now populate `iconify_prefixes` so the
/// `@iconify-json/<prefix>` package is credited. Pre-fix entries omit the field,
/// so icon-set packages can be reported as unused until the file is re-extracted.
///
/// Bumped to 101 for issue #704: SFC template tags that match no import now
/// populate `auto_import_candidates` for convention auto-import resolution.
/// Pre-fix entries omit the field, so Nuxt components consumed only via template
/// tags are not edge-credited until the file is re-extracted.
///
/// Bumped to 102 for issue #742: `FunctionComplexity` now carries an
/// `Option<String> source_hash` (content digest of the function's full-span
/// source slice) so runtime-coverage baselines survive line moves. Pre-fix
/// cache entries lack the field, so the hash is absent until re-extraction.
///
/// Bumped to 103 for issue #752: typed destructure bindings
/// (`let { resultState }: Props = $props()`, `function f({ x }: Props)`) now
/// populate `binding_target_names`, which changes the `member_accesses` emitted
/// for those files. Pre-fix cache entries lack the additional member accesses.
///
/// Bumped to 104 for issue #445: MDX, Astro, Vue/Svelte SFC, and CSS/SCSS
/// container extraction now remaps source-authored spans back to the original
/// file byte offsets. Pre-fix entries can carry synthetic extracted-buffer
/// positions, so diagnostics can point at line 1 or compacted MDX lines until
/// the file is re-extracted.
///
/// Bumped to 105 for issue #739: JS/TS and Vue/Svelte SFC script extraction
/// now populates `auto_import_candidates` from unresolved value references.
/// Pre-fix entries omit these candidates, so convention script auto-imports
/// are not edge-credited until the file is re-extracted.
///
/// Bumped to 106 for `fallow security`: JS/TS extraction now stores file-level
/// directives (`"use client"`, `"use server"`) in the parse cache so client
/// boundary detection does not depend on stale cached module info.
///
/// Bumped to 107 for issue #835: Svelte `<script src>` references no longer
/// emit synthetic imports because they are runtime markup, not bundled SFC
/// script modules. Pre-fix entries can carry stale root-relative imports that
/// surface as false `unresolved-imports`.
///
/// Bumped to 108 for three extraction-semantics changes shipping together:
/// - issue #839: `declare` ambient class properties are no longer extracted as
///   class members (they emit no JS and cannot be value-referenced), so pre-fix
///   entries carry phantom members that surface as false `unused-class-member`.
/// - issue #840: extensionless `new URL(specifier, import.meta.url)` dynamic
///   imports now persist `is_speculative = true` so a directory target
///   (`new URL('./services', import.meta.url)`) is silently dropped when the
///   resolver finds no module; pre-fix entries carry `is_speculative = false`
///   and surface as false `unresolved-imports`.
/// - issue #845: a method call on an `instanceof`-narrowed value now emits a
///   member access against the narrowed class, changing the persisted
///   `member_accesses`; pre-fix entries miss the credit and surface as false
///   `unused-class-member`.
///
/// Bumped to 109 for the data-driven security matcher catalogue: JS/TS
/// extraction now captures non-literal sink sites into `security_sinks`, each
/// carrying an `arg_kind` discriminator (template-with-substitution, concat,
/// object, call, other) so the catalogue can require unsafe SQL shapes and
/// exclude safely-parameterized `` sql`${x}` `` templates and object-form
/// `.execute({ sql, args })` arguments. Pre-109 entries lack the field, so their
/// sink sites do not feed the catalogue until the file is re-extracted.
///
/// Bumped to 110 for issue #844: `const svc = useMemo(() => new Svc())` now
/// binds the non-destructured identifier to the constructed class, so method
/// calls on it emit member accesses crediting the class. This changes the
/// persisted `member_accesses` for files using the useMemo factory shape;
/// pre-fix entries miss the credit and surface as false `unused-class-member`.
///
/// Bumped to 111 for issue #859 (untrusted-source modeling): `SinkSite` now
/// carries `arg_idents` (identifiers referenced in the sink argument) and
/// `ModuleInfo`/`CachedModule` carry `tainted_bindings` (local bindings tied to
/// the member-access path they were sourced from), so the security
/// `tainted_sink` detector can back-trace a sink argument to a known untrusted
/// source. Pre-111 entries lack both, so source-to-sink association is unset
/// until the file is re-extracted.
///
/// Bumped to 112 for issue #863 (sanitizer-aware security sinks):
/// `ModuleInfo`/`CachedModule` now carry direct sanitized sink arguments, so
/// the security `tainted_sink` detector can suppress high-confidence
/// DOMPurify-backed HTML sink candidates. Pre-112 entries lack sanitizer
/// metadata until the file is re-extracted.
///
/// Bumped to 113 for issue #863 follow-up: sanitizer metadata gained URL and
/// path domains plus guarded path backpatching. Pre-113 entries may lack those
/// sanitizer domains until the file is re-extracted.
///
/// Bumped to 114 for issue #911: Angular component properties initialized with
/// named-import `inject(Service)` now populate `ClassHeritageInfo.instance_bindings`
/// so external templates can credit service member access through the property.
/// Pre-114 entries miss the binding and can surface false `unused-class-member`
/// findings until the component file is re-extracted.
///
/// Bumped to 115 for issue #910: local typed function calls now credit concrete
/// class members when a direct `new Class()` argument or constructor-bound
/// identifier flows into a structurally typed parameter. Pre-115 entries can
/// miss those synthetic `member_accesses` and surface false
/// `unused-class-member` findings.
///
/// Bumped to 116 for issue #1302: suppression comments and `@expected-unused`
/// tags now carry optional human-authored reasons. Pre-116 entries lack those
/// reasons, so `require-suppression-reason` would report false missing-reason
/// findings until files are re-extracted.
///
/// Bumped to 117 for issue #955: Vue SFC script-side Nuxt UI icon strings now
/// populate `iconify_icon_names`, allowing declared `@iconify-json/*`
/// collections used through values like `icon: 'i-simple-icons-github'` to be
/// credited. Pre-116 entries omit those names and can surface false
/// `unused-dependency` findings until the file is re-extracted.
///
/// Bumped to 118 for issue #954: JS/TS extraction now records static
/// `pino({ transport: { target: "pkg" } })` target packages as synthetic
/// dynamic imports so runtime transport dependencies are credited. Pre-118
/// entries can surface false `unused-dependency` findings until the file is
/// re-extracted.
///
/// Bumped to 119 for issue #952: JS/TS extraction now records static package
/// path resolution references so packages consumed via package-root and
/// `pkg/package.json` lookups are credited as dependency usage. Pre-119
/// entries omit those references and can surface false `unused-dependency`
/// findings until the file is re-extracted.
///
/// Bumped to 120 for issue #953: instance methods annotated with TypeScript's
/// `this` return type now count as self-returning for constructor-rooted
/// fluent chains. Pre-120 entries can miss those self-returning flags and
/// surface false `unused-class-member` findings until the file is re-extracted.
///
/// Bumped to 121 for issue #883: framework template HTML injection sinks now
/// flow into `ModuleInfo.security_sinks` for Svelte `{@html ...}`, Vue
/// `v-html`, and Angular `[innerHTML]`. Pre-121 entries omit those sink sites
/// until the file is re-extracted.
///
/// Bumped to 122: `FunctionComplexity` now carries a `contributions` vector
/// (per-decision-point complexity breakdown) and `RequireCallInfo` carries
/// `source_span` (the specifier string-literal span so an `unresolved-import`
/// squiggly anchors under the `'./x'` specifier rather than the `require`
/// keyword). Pre-122 entries lack the breakdown (empty under
/// `health --complexity-breakdown`) and carry `Span::default()` for the
/// require specifier until the file is re-extracted.
///
/// Bumped to 123 for PR #1010: JSDoc import-type extraction now ignores prose
/// examples, including examples that contain ordinary JavaScript brace groups.
/// Pre-123 entries can carry stale type-only imports that surface as false
/// `unresolved-imports` until the file is re-extracted.
///
/// Bumped to 124 for issue #877: static `import.meta.env.SECRET` reads now
/// populate `member_accesses` as `import.meta.env` source reads for the
/// opt-in client/server security candidate detector. Pre-124 entries omit the
/// source and would miss Vite env reads until the file is re-extracted.
///
/// Bumped to 125 for issue #875: `SinkSite` now carries literal argument and
/// object-literal option metadata, allowing security catalogue rows to match
/// deterministic literal sinks such as wildcard postMessage origins,
/// permissive CORS, insecure cookie options, weak crypto algorithms, and
/// alg:none JWT options. Pre-125 entries lack that metadata until the file is
/// re-extracted.
///
/// Bumped to 126 for issue #876: `SinkSite` now carries flattened source paths
/// referenced inside sink arguments, so source-backed logging candidates can
/// match direct expressions such as `process.env.SECRET` without requiring a
/// temporary local binding. Pre-126 entries lack those paths until the file is
/// re-extracted.
///
/// Bumped to 127 for issue #898: `SinkSite` now carries complete top-level
/// object-key metadata so missing-option security rows can distinguish absent
/// keys from non-literal option values. Pre-127 entries lack that metadata until
/// the file is re-extracted.
///
/// Bumped to 128 for issue #895: JS/TS extraction now captures the exact
/// `process.env.NODE_TLS_REJECT_UNAUTHORIZED = "0"` literal assignment as a
/// security sink site. Pre-128 entries omit that sink until the file is
/// re-extracted.
///
/// Bumped to 129 for issue #901: JS/TS extraction now captures cleartext
/// request URL literals and `new WebSocket("ws://...")` as security sink sites.
/// Pre-129 entries omit those sinks until the file is re-extracted.
///
/// Bumped to 130 for issue #892: JS/TS extraction now captures static string
/// literals assigned to secret-shaped identifiers or known provider credential
/// prefixes as opt-in hardcoded-secret candidates.
/// Pre-130 entries omit those candidates until the file is re-extracted.
///
/// Bumped to 131 for issue #879: JS/TS extraction now records synthetic
/// source bindings for recognizable framework handler parameters. Pre-131
/// entries omit those bindings and cannot source-rank direct handler params.
///
/// Bumped to 132 for issue #878: JS/TS extraction now records one-hop
/// same-module helper calls that return source-backed expressions as tainted
/// bindings. Pre-132 entries miss the ranking signal until re-extracted.
///
/// Bumped to 133 for issue #901: `SinkSite` now carries integer literal
/// values and nested static object property paths for additional literal-tier
/// security rows. Pre-133 entries omit that metadata until the file is
/// re-extracted.
///
/// Bumped to 134 for issue #928: JS/TS extraction now captures risky literal
/// regex application sites in `security_sinks` so `fallow security` can report
/// source-backed ReDoS candidates. Pre-134 entries omit those sink sites until
/// the file is re-extracted.
///
/// Bumped to 135 for issue #929: JS/TS extraction now skips directly clamped
/// resource-amplification size arguments before catalogue matching. Pre-135
/// entries may retain stale clamped amplification sink candidates until the
/// file is re-extracted.
///
/// Bumped to 136 for issue #899: JS/TS extraction now emits GraphQL resolver
/// args, tRPC procedure input, and exact member source paths for local tainted
/// bindings. Pre-136 entries may miss those source-backed ranking signals until
/// the file is re-extracted.
///
/// Bumped to 137 for issue #888: JS/TS extraction now records defensive
/// security control sites for the attack-surface inventory. Pre-137 entries
/// omit those controls until the file is re-extracted.
///
/// Bumped to 138 for issue #890: `SinkSite` now carries the arg-0 URL literal
/// (`url_arg_literal`) for the secret-to-network destination signal, `import.meta.env`
/// reads are modeled as a source via the new `flatten_member_path` MetaProperty
/// arm, and public-by-convention env vars (`NEXT_PUBLIC_`, `VITE_`, ...) are no
/// longer recorded as secret sources. Pre-138 entries omit the URL signal and may
/// retain stale public-env source bindings until the file is re-extracted.
///
/// Bumped to 139 for issue #1095: JS/TS extraction now records source-backed
/// local bindings when template literals, string concatenation, or object
/// literals embed an untrusted source. Pre-139 entries miss those ranking
/// signals until the file is re-extracted.
///
/// Bumped to 140 for issue #1094: JS/TS extraction now records declarative
/// framework validation boundary controls for security surface output. Pre-140
/// entries can miss route-level validation control sites until re-extracted.
///
/// Bumped to 141 for issue #1093: `TaintedBinding` gains `source_span_start`
/// (the byte offset of the source read) so the analyze layer can anchor a taint
/// trace's source node at the real read line; pre-141 entries lack the offset.
/// Bumped to 142 for issue #1134: JS/TS extraction now stores compact
/// diagnostics for security sink-shaped callees that could not be flattened, so
/// warm-cache `fallow security` runs can report the same blind-spot metadata as
/// cold extraction.
///
/// Bumped to 143 for issue #1138: JS/TS extraction now propagates simple
/// module-scope literal constants into security sink argument metadata and
/// filters public CI metadata env vars before source matching.
///
/// Bumped to 144 for issue #1136: JS/TS sanitizer metadata now recognizes
/// proven local HTML escape helpers, renderer helpers, and SQL identifier
/// quoting helpers. Pre-144 entries can lack those sanitizer domains until the
/// file is re-extracted.
///
/// Bumped to 145 for issue #1137: `SinkSite` now carries URL construction shape
/// metadata for fixed-origin and dynamic-origin URL sink candidates.
///
/// Bumped to 146 for issue #1146: JS/TS extraction now chains tainted local
/// bindings through up to three same-module hops, so warm caches written
/// before the bump lack the chained `tainted_bindings` records.
///
/// Bumped to 147 for issue #1147: JS/TS extraction now captures deduped
/// statically flattenable callee paths (`callee_uses`) for the
/// `boundaries.calls.forbidden` detector, so warm caches written before the
/// bump would report zero forbidden-call findings.
///
/// Bumped to 148 for issue #1190: JS/TS extraction now records nested
/// Playwright fixture type-alias bindings in `member_accesses`, so warm caches
/// written before the bump can miss fixture members reached through imported
/// object type aliases.
///
/// Bumped to 149 for issue #1180: cached inline suppressions now preserve
/// scoped rule-pack policy tokens (`policy-violation:<pack>/<rule-id>`).
/// Pre-149 entries only store a broad `IssueKind` discriminant and cannot
/// round-trip scoped policy suppressions.
///
/// Bumped to 150 for issue #1210: JS/TS extraction now records Playwright
/// fixture wrapper aliases in `member_accesses`, so warm caches written before
/// the bump can miss fixture members reached through `mergeTests` or chained
/// wrapper `.extend(...)` calls.
///
/// Bumped to 151 for the server-only-import security candidate: JS/TS extraction
/// now records `next/dynamic(..., { ssr: false })` dynamic-import spans on
/// `client_only_dynamic_import_spans`, so warm caches written before the bump
/// miss the ssr:false client-only escape hatch the `client-server-leak` BFS uses
/// to exclude that edge.
///
/// Bumped to 152 for the `misplaced-directive` detector: JS/TS extraction now
/// records `"use client"` / `"use server"` directive strings written as
/// expression statements in `program.body` (misplaced) on
/// `misplaced_directives`, so warm caches written before the bump would report
/// zero misplaced-directive findings.
///
/// Bumped to 154 for the `unprovided-inject` detector: JS/TS and SFC extraction
/// now record Vue `provide`/`inject` and Svelte `setContext`/`getContext` call
/// sites on `di_key_sites` plus a `has_dynamic_provide` flag, so warm caches
/// written before the bump would report zero unprovided-inject findings.
///
/// Bumped to 155 because `di_key_sites` now drops keys bound to a module-scope
/// string-literal const (string identity, not a symbol), so a warm cache from
/// 154 would carry those dropped sites and false-flag a string-keyed inject.
///
/// Bumped to 156 because SFC markup asset references (`<img src="./logo.png">`,
/// `<source>`, `<video poster>`) now emit `SideEffect` imports, so a warm cache
/// from 155 would miss the new `unresolved-import` findings on missing assets.
///
/// Bumped to 157 because the Vue `<template>` body extractor now matches the
/// root `</template>` with nesting depth tracking instead of the first
/// `</template>`. A Vue SFC whose root template contains a nested `<template
/// #slot>` no longer has its body truncated, so component tags rendered after
/// the first nested slot are now credited; a warm cache from 156 would carry the
/// truncated template-usage set and false-flag those components / their imports.
///
/// Bumped to 158 for the `unused-component-prop` detector: Vue `<script setup>`
/// extraction now records `defineProps` declared props on `component_props`
/// (with `used_in_script` / `used_in_template`) plus the
/// `has_props_attrs_fallthrough` / `has_define_expose` / `has_define_model` /
/// `has_unharvestable_props` abstain flags, so a warm cache from 157 would
/// report zero unused-component-prop findings.
///
/// Bumped to 159 because `ComponentProp` gained a `local` field (the destructure
/// alias for a renamed prop), changing the cached wire shape; a warm 158 cache
/// would bitcode-misread it.
///
/// Bumped to 160 for the `unused-component-emit` detector: Vue `<script setup>`
/// extraction now records `defineEmits` declared events on `component_emits`
/// (with `used`) plus the `has_unharvestable_emits` / `has_dynamic_emit` /
/// `has_emit_whole_object_use` abstain flags, so a warm cache from 159 would
/// report zero unused-component-emit findings.
///
/// Bumped to 162 for `unused-load-data-key` Primitive A: a destructure off the
/// SvelteKit `data` prop local (`const { user } = data`) now emits `data.<key>`
/// member accesses (rest element records a whole-object use). A warm cache from
/// 161 lacks those accesses, so the cross-file load-data-key join would miss the
/// consumed keys.
///
/// Bumped to 163 for `unused-load-data-key` Primitive B: a SvelteKit route
/// component (`+page.svelte` / `+layout.svelte`) now credits the `data` prop as
/// a template-visible root, so `{data.x}` / `{#each data.items as i}` markup
/// reads emit `data.<key>` member accesses. A warm cache from 162 lacks those
/// template-side accesses, so the cross-file load-data-key join would miss keys
/// consumed only in markup.
///
/// Bumped to 164 for `unused-load-data-key` Primitive C: a SvelteKit global
/// page-store read in a template (`{$page.data.KEY}` / `{page.data.KEY}`) now
/// recovers the nested `page.data.<key>` member access (the template scanner
/// previously dropped the key, keeping only `page.data`). A warm cache from 163
/// lacks those project-wide global-store accesses.
///
/// Bumped (origin/main) for the `unused-load-data-key` detector: SvelteKit
/// page-load producers now harvest `load_return_keys` + `has_unharvestable_load`,
/// and every file records `has_load_data_whole_use` (the FP-1 whole-`data` pass
/// signal). A warm cache from 164 lacks all three.
///
/// Bumped (origin/main) for the typed-`data` template fix: a SvelteKit route
/// component whose `data` prop is typed (`export let data: PageData`) no longer
/// remaps its template `data.<key>` accesses onto the generated `$types` alias,
/// keeping them keyed on `data` for the load-data join. A warm cache carries the
/// remapped (`PageData.<key>`) accesses and would miss real consumer reads.
///
/// Bumped (origin/main) for #550: CSS Module class extraction now derives its
/// class set from a real CSS AST (lightningcss) for standard CSS, so warm caches
/// written by the regex-only extractor can differ on escaped class names and
/// malformed at-rule preludes.
///
/// Bumped (feat/react-health) for React/JSX structural extraction (Phase 0
/// foundation): `.jsx`/`.tsx` files now record `component_functions`,
/// `react_props`, `hook_uses`, and `render_edges`, so a warm cache lacks the
/// React IR the later React-health phases consume.
///
/// Bumped (feat/react-health) for the React `unused-component-prop` arm
/// (Phase 1): each `ComponentProp` gained a `component` field (the enclosing
/// React component name) and `react_props[].used_in_script` is now populated
/// from a used-in-body pass, so a warm cache carries props with an empty
/// `component` and always-false usage.
///
/// Bumped (feat/react-health) for React-aware complexity (Phase 2):
/// `FunctionComplexity` now carries `react_hook_count`, `react_jsx_max_depth`,
/// and `react_prop_count` descriptive fields, and the cognitive metric folds
/// deep JSX nesting, hook density, and prop count (recorded as `JsxDepth` /
/// `HookDensity` / `PropCount` contributions). A warm cache carries the pre-fold
/// cognitive scores and lacks the React descriptive counts until re-extraction.
///
/// Bumped (feat/react-health) for the prop-drilling forward signal (Phase 3):
/// `RenderEdge` gained `attr_value_roots` / `has_complex_forward`,
/// `ComponentFunction` gained `uses_clone_element` / `renders_provider` /
/// `has_children_as_function`, and `ComponentProp` gained `used_outside_forward`.
/// A warm cache lacks the per-render attribute-value roots and the
/// per-component / per-prop forward classification the prop-drilling detector
/// consumes.
///
/// Bumped to 170: `ComponentFunction` gained `is_pure_passthrough` (the
/// thin-wrapper extraction flag), a new bitcode field on a cached struct
/// persisted via `ModuleInfo`.
///
/// Bumped to 171 (feat/angular): Angular input/output IR
/// (`angular_inputs` / `angular_outputs` on `ModuleInfo`) plus the
/// `unused-component-input` / `unused-component-output` suppression tokens, and
/// the Angular `{ ...this }` spread now records a whole-component abstain marker
/// for the input/output detectors; a warm cache from 170 lacks the Angular IR
/// and the abstain marker and would report zero input/output findings or
/// false-flag spread-forwarded inputs/outputs.
///
/// Bumped to 172 (feat/vue-options-api-prop-emit): the Vue Options API
/// (`export default { props, emits, ... }` / `defineComponent({ ... })`) in a
/// non-setup `<script>` now harvests `component_props` / `component_emits` and
/// the abstain flags the same way `<script setup>` does; a warm cache from 171
/// lacks the Options-API prop/emit IR and would report zero findings on those
/// components.
///
/// Bumped to 173 (feat/svelte-runes-extraction, W1.1): two `.svelte` extraction
/// changes alter serialized module state. (1) Svelte 5's bare `<script module>`
/// attribute is now recognized as module context (was treated as the instance
/// script), so a warm cache wrongly scoped module-level declarations and credited the
/// module script's imports as template-visible. (2) The Svelte 5 `$props()` rune
/// is now harvested into `component_props` (reusing the Vue IR + abstain flags);
/// a warm cache from 172 lacks the Svelte prop IR. (`<svelte:component>` /
/// `<svelte:element>` / `<svelte:self>` were verified already credited by the
/// existing attribute-value scan, so no template-scanner change rides this bump.)
///
/// Bumped to 174 (feat/svelte-dead-event): `.svelte` extraction now records
/// `svelte_dispatched_events` (literal-arg `dispatch('<name>')` calls where
/// `dispatch` is bound from `createEventDispatcher()`), `svelte_listened_events`
/// (template `on:<name>` bindings on component tags), and `has_dynamic_dispatch`
/// (a dynamic-dispatch / whole-`dispatch`-value abstain). A warm cache from 173
/// lacks the dispatched/listened event IR and would report zero
/// `unused-svelte-event` findings.
///
/// Bumped to 175 (feat/angular-unrendered-component, W4.2): Angular extraction
/// now records `angular_component_selectors` (each `@Component({ selector })`
/// value split into a list plus the class name + span), `angular_used_selectors`
/// (custom element tags scanned from inline + external Angular templates), and
/// `angular_entry_component_refs` (route `component:` / `loadComponent`,
/// `bootstrapApplication` / `bootstrap: [...]` class references), and
/// `has_dynamic_component_render` (a `ViewContainerRef.createComponent` /
/// `*ngComponentOutlet` / `createComponent(<ident>)` project-wide abstain). A
/// warm cache from 174 lacks the selector IR and would report zero Angular
/// `unrendered-component` findings.
///
/// Bumped to 176 (feat/angular-unprovided-inject, W4.1): the `di_key_sites` set
/// now carries Angular entries (`inject(TOKEN)` / `@Inject(TOKEN)` injects and
/// `{ provide: TOKEN, ... }` provides via the new `DiFramework::Angular` variant),
/// `has_dynamic_provide` is additionally set by `importProvidersFrom` /
/// `makeEnvironmentProviders` / a `providers:` spread, and a tree-shakable
/// `new InjectionToken(..., { factory } | { providedIn })` records a self-provide.
/// A warm cache from 175 lacks the Angular DI sites and would report zero Angular
/// `unprovided-inject` findings.
///
/// Bumped to 177 (feat/sfc-template-complexity): Vue and Svelte SFC
/// `module.complexity` now carries a synthetic `<template>` `FunctionComplexity`
/// entry computed from template control flow (`v-if`/`v-for`, `{#if}`/`{#each}`)
/// plus bound-expression and interpolation complexity, mirroring Angular's
/// existing `<template>` entry. The `FunctionComplexity` shape is unchanged (only
/// an extra Vec element), so no size assertion changes. A warm cache from 176
/// lacks the SFC `<template>` entry and would under-report SFC complexity until
/// the file is re-parsed.
///
/// Bumped to 178 (feat/rsc-widen-inline-server-action): `ModuleInfo` now carries
/// `inline_server_action_exports`, the export local names of exported functions /
/// const-arrows whose body has an inline `"use server"` directive in a
/// non-`"use server"` file. The `unused-server-action` reclassifier reads it to
/// move a dead inline Server Action out of `unused-export`. A warm cache from 177
/// lacks the field and would leave such dead inline actions categorized as
/// `unused-export` until the file is re-parsed.
///
/// Bumped to 179 for issue #1270: Playwright fixture callbacks now record
/// member uses reached through branch-selected local fixture aliases. Warm
/// caches from 178 can miss those synthetic `member_accesses` and surface false
/// `unused-class-member` findings.
///
/// Bumped to 180 for issue #1281: JSX nesting depth is now descriptive
/// `react_jsx_max_depth` context only, so warm caches from 179 may carry stale
/// cognitive scores and `JsxDepth` contribution entries for React components.
///
/// Bumped to 181 for issue #1282: Pinia `storeToRefs(useStore())` and
/// `toRefs(useStore())` destructures now record store member accesses. Warm
/// caches from 180 can miss those synthetic `member_accesses` and surface false
/// `unused-store-member` findings.
///
/// Bumped (LLM-call sinks): the security sink argument collectors
/// (`collect_arg_idents` / `collect_arg_source_paths`) now recurse into array
/// elements (and the source-path collector into object properties), so taint
/// riding an object-in-array argument (`messages: [{ content: userInput }]`, the
/// canonical OpenAI / Anthropic chat shape) surfaces on `SinkSite.arg_idents` /
/// `arg_source_paths`. Warm caches lack those captured identifiers and would
/// miss source-backed candidates on the array-nested prompt shape.
///
/// Bumped for the Astro/Lit framework-health parity wave: Astro frontmatter now
/// runs the `oxc_semantic` unused-binding pass (template-used names credited), so
/// `.astro` modules carry populated `unused_import_bindings` /
/// `value_referenced_import_bindings` instead of an empty (all-referenced) set;
/// plus the Lit registered-tag / used-tag and Astro `<template>` complexity
/// extraction fields. Warm caches mask the new `unused-export` /
/// `unrendered-component` arms.
///
/// Bumped for the post-smoke-test FP fixes: standalone `.html` modules now
/// populate `used_custom_element_tags` (a root `<my-app>` rendered only in
/// `index.html` no longer false-flags), and imperative `createElement` capture
/// widened to any receiver (`opts.document.createElement(...)`).
///
/// Bumped to 185 on merging the agentic-review branch into main: the LLM-call
/// sink array recursion and the Astro/Lit parity wave land together, so warm
/// caches from either side (183 or 184) must invalidate.
///
/// Bumped to 186 for the React typed-interface / `props.x` prop harvest: a
/// component whose first param is a bare identifier typed by a same-file
/// `interface`/`type` object literal (`(props: Props) => props.x`) now harvests
/// the interface member names into `react_props` and credits `props.<name>`
/// member-access usage, where warm caches from 185 recorded the component as
/// `has_unharvestable_props` with no props.
///
/// Bumped to 187 for the React typed-prop harvest extension to
/// `forwardRef<Ref, Props>((props, ref) => ...)`: the props type now resolves
/// from the wrapper call's SECOND generic argument (a same-file
/// `interface`/`type`) when the inner `props` param carries no annotation, so a
/// generic-typed forwardRef component that warm caches from 186 recorded as
/// `has_unharvestable_props` now harvests its `react_props` and credits
/// `props.<name>` usage. The cached `ComponentProp` / `ComponentFunction` wire
/// shape is unchanged; only which components populate it changes.
///
/// Bumped to 188: `HookUse` now carries the enclosing `component` name, so the
/// descriptive per-component hook summary stays exact in multi-component files.
/// A warm cache from 187 lacks the attribution field on persisted `hook_uses`.
///
/// Bumped to 189: `ModuleInfo`/`CachedModule` now carry typed semantic facts for
/// Angular template member accesses alongside the older string payload entries.
///
/// Bumped to 190: `SemanticFact` now includes typed static-factory call member
/// access facts alongside the older factory-call string payload entries.
///
/// Bumped to 191: `SemanticFact` now includes typed fluent-chain member access
/// facts alongside the older fluent-chain string payload entries.
///
/// Bumped to 192: `SemanticFact` now includes typed Playwright fixture-use facts
/// alongside the older fixture-use string payload entries.
///
/// Bumped to 193: `SemanticFact` now includes typed Playwright fixture definition
/// facts alongside the older fixture-definition string payload entries.
///
/// Bumped to 194: `SemanticFact` now includes typed Playwright fixture alias
/// facts alongside the older fixture-alias string payload entries.
///
/// Bumped to 195: `SemanticFact` now includes typed Playwright fixture type
/// facts alongside the older fixture-type string payload entries.
///
/// Bumped to 196: `SemanticFact` now includes typed instance export binding
/// facts alongside the older instance-export string payload entries.
///
/// Bumped to 197: factory-call member accesses are now persisted only as typed
/// semantic facts; older cache payloads are reparsed by subsequent schema bumps.
///
/// Bumped to 198: fluent-chain member accesses are now persisted only as typed
/// semantic facts; older cache payloads are reparsed by subsequent schema bumps.
///
/// Bumped to 199: constructor-rooted fluent-chain member accesses are now
/// persisted only as typed semantic facts; older cache payloads are reparsed by
/// subsequent schema bumps.
///
/// Bumped to 200: instance export bindings are now persisted only as typed
/// semantic facts; older cache payloads are reparsed by subsequent schema bumps.
///
/// Bumped to 201: Playwright fixture-use member accesses are now persisted only
/// as typed semantic facts; older cache payloads are reparsed by subsequent
/// schema bumps.
///
/// Bumped to 202: Playwright fixture-definition member accesses are now
/// persisted only as typed semantic facts; older cache payloads are reparsed by
/// subsequent schema bumps.
///
/// Bumped to 203: Playwright fixture-alias member accesses are now persisted
/// only as typed semantic facts; older cache payloads are reparsed by subsequent
/// schema bumps.
///
/// Bumped to 204: Playwright fixture-type member accesses are now persisted
/// only as typed semantic facts; older cache payloads are reparsed by subsequent
/// schema bumps.
///
/// Bumped to 205: Angular template member accesses are now persisted only as
/// typed semantic facts; older cache payloads are reparsed by subsequent schema
/// bumps.
///
/// Bumped to 206: Angular `{ ...this }` spread abstains are now persisted as
/// typed semantic facts; older cache payloads are reparsed by subsequent schema
/// bumps.
///
/// Bumped to 207: dynamic custom-element render abstains are now persisted as
/// typed semantic facts.
///
/// Bumped to 208: empty cached semantic facts are omitted from the persisted
/// module payload. The in-memory `ModuleInfo` contract still exposes an empty
/// vector, but warm caches from 207 carry the old eager `Vec` field shape.
///
/// Bumped to 209: pre-typed semantic payloads are no longer decoded from cached
/// member accesses. Warm caches from 208 or earlier are reparsed so analyzers
/// consume typed semantic facts only.
///
/// Bumped to 210 (issue #1489 Case 2): a param typed as a Pinia store
/// (`ReturnType<typeof useFooStore>`, inline or aliased) now binds to the store
/// factory, so `props.store.member` / `const { m } = props.store` emit factory
/// `member_accesses` a 209 warm cache lacks.
///
/// Bumped to 211 (issue #1441, cross-module Part A): exported free-function
/// factories now persist `exported_factory_returns`, and a consumer's
/// `const x = importedFactory()` emits a typed `FactoryFnMemberAccess` semantic
/// fact so `x.member` credits the returned class across module boundaries. A
/// warm cache from 210 lacks both the new metadata and the added facts.
///
/// Bumped to 212 (issue #1489 Case 1): an inline `useFooStore().member` call
/// with no bound local now emits a factory `member_access` so the member is
/// credited; a 211 warm cache lacks it.
///
/// Bumped to 213 (issue #1641): Svelte template usage now credits
/// `bind:`/`style:`/`class:` directive shorthands (`bind:open` =
/// `bind:open={open}`) as references, so a prop used only via a shorthand
/// directive sets `used_in_template`. A warm cache from 212 carries the stale
/// (uncredited) prop-usage flags.
///
/// Bumped to 215 (issue #1638, GAP 2): a `new Class()` flowing DIRECTLY into a
/// string-coercion position (template-literal interpolation, `String(...)`
/// argument, or `+` with a string operand) now records a `Class.toString`
/// member access, so an implicitly-coerced `toString` is credited instead of
/// reported as an unused class member. A warm cache from 214 lacks the
/// synthesized `toString` accesses.
///
/// Bumped to 216 (issue #1707): a Vue `v-for` loop variable iterating over a
/// typed array / reactive array of a class (`v-for="(util) of utils"` where
/// `utils` is `Util[]` / `computed(() => Util[])`) now types the item to its
/// element class, so template member accesses on the item (`{{ util.getter }}`)
/// credit the class. A warm cache from 215 carries the stale `.vue`
/// `member_accesses` that lack the credited item-member accesses.
///
/// Bumped to 217 (issue #1707 follow-up): the same element-class inference now
/// also types JS iteration bindings, `utils.map(u => u.getter)` / `.forEach` /
/// `.filter` / etc. callback params and `for (const u of utils)` loop variables
/// over a typed array / reactive array, so member accesses on the iteration
/// variable credit the element class. A warm cache from 216 lacks the credited
/// iteration-variable member accesses.
///
/// Bumped to 218 for issue #1711: a Vue `v-for` over a `props.<field>`
/// member-expression source (where the prop is typed as an array of a class via
/// `defineProps<{ items: Util[] }>()`) now types the loop item to the element
/// class, so `.vue` `member_accesses` gain the credited item-member accesses a
/// warm 217 cache lacks.
///
/// Bumped to 219 for issue #1712: an Angular `@for` / `*ngFor` loop variable
/// over a component field typed as an array of a class (`utils: Util[]`) in an
/// inline `template:` is typed to the element class, so inline-Angular-component
/// `member_accesses` gain the remapped item-member accesses a warm 218 cache
/// lacks.
///
/// Bumped to 220 for issue #1713: a `.map()` / `.forEach()` / `for...of`
/// iteration binding in an Astro TEMPLATE `{...}` expression region (over a
/// frontmatter-typed class array) now credits the element-class members, so
/// `.astro` `member_accesses` gain the template-region item-member accesses a
/// warm 219 cache lacks.
///
/// Bumped to 221 for issue #1744: a factory function whose body yields no value
/// proof but whose explicit return-TYPE annotation names a class (`function
/// useController(): ReadyAppController { return registry.get() as ... }`) now
/// records a strict factory-return entry, so its `exported_factory_returns`
/// output credits `const c = useController(); c.method()` across the module
/// boundary; a warm 220 cache lacks that entry.
///
/// Bumped to 222 (#1742): conditional and logical dynamic `import()` arguments
/// (`import(c ? './a' : './b')`, `import(x || './b')`) are now traced, emitting one
/// `DynamicImportInfo` edge per statically-resolvable branch (plus the wrapper
/// families: `.then`, `React.lazy`/`next/dynamic`, route `loadComponent`). A warm
/// cache from 221 lacks the added per-branch dynamic-import entries.
///
/// Bumped to 223 for issues #1716, #1717, and #1718: iterable element-type
/// crediting now includes Vue `ref.value` / object-member `v-for` sources,
/// function-local JS array receivers, and Angular external `templateUrl`
/// loop-item remapping through component field array facts.
///
/// Bumped to 224 for React Router and Remix route-loader data keys: conventional
/// route modules now harvest `loader` / `clientLoader` return keys and emit
/// synthetic `useLoaderData()` member accesses. Warm 223 caches lack both.
///
/// Bumped to 225 for issue #1785: interface / type-literal-alias property
/// types now persist as `type_member_types`, compound binding targets expand
/// through them into new `member_accesses`, imported-type dead-ends emit the
/// new `TypedPropertyMemberAccess` semantic fact, INTERFACE-declared
/// Playwright fixture maps (`base.extend<MyFixtures>` where `MyFixtures` is
/// an interface) now emit the same fixture facts as the type-alias form, and
/// the SFC merge path now carries `semantic_facts`, so Vue/Svelte `<script>`
/// blocks persist every fact kind they previously dropped. Warm 224 caches
/// lack all five, leaving interface-typed DI receivers, interface-fixture POM
/// members, and SFC fact consumers falsely unused.
pub(super) const CACHE_VERSION: u32 = 225;

/// Duplication token cache version. Bump when duplicate tokenization,
/// normalization, or the on-disk token cache schema changes.
///
/// Bumped to 6 for issue #1225: `ignoreImports` now excludes re-export barrels
/// and top-level static CommonJS require binding declarations.
///
/// Bumped to 7: duplicate tokenization now includes CSS-family files plus
/// Vue, Svelte, and Astro template/style regions. Warm caches from 6 can carry
/// empty CSS streams or script-only SFC/Astro streams.
///
/// Bumped to 8: duplicate token hashes now include the active source namespace
/// (`js`, `style`, or `markup`) so structurally similar code from unrelated
/// formats does not form cross-format clone groups.
///
/// Bumped to 9: CSS-family / SFC `<style>` tokens are now value-canonicalized
/// (zero-unit collapse `0px`/`0em`/`0%` -> `0`, hex-color expansion `#fff` ->
/// `#ffffff`) so near-miss / value-drifted CSS clones match. Warm v8 caches carry
/// the un-canonicalized CSS token stream and must invalidate.
pub const DUPES_CACHE_VERSION: u32 = 9;

/// Default maximum cache size (256 MB). Overridable per-project via
/// `cache.maxSizeMb` in the config file or `FALLOW_CACHE_MAX_SIZE` env var.
/// Also used as the hard ceiling on load-time deserialization as a defence
/// against pathological on-disk files.
pub const DEFAULT_CACHE_MAX_SIZE: usize = 256 * 1024 * 1024;

/// Trigger LRU eviction when the serialized cache exceeds 80% of the cap.
/// Basis points (1/100 of a percent) for integer arithmetic without floats.
pub(super) const EVICTION_TRIGGER_BPS: usize = 8000;

/// Evict down to 60% of the cap so subsequent saves leave headroom.
pub(super) const EVICTION_TARGET_BPS: usize = 6000;

/// Promote the eviction log from `debug!` to `info!` when at least 25% of
/// entries are removed in a single save. Default-noise concerns mean
/// small-turnover saves should not be visible without `RUST_LOG=debug`.
pub(super) const EVICTION_SIGNIFICANT_BPS: usize = 2500;

/// Import kind discriminant for `CachedImport`:
/// 0 = Named, 1 = Default, 2 = Namespace, 3 = `SideEffect`.
pub(super) const IMPORT_KIND_NAMED: u8 = 0;
pub(super) const IMPORT_KIND_DEFAULT: u8 = 1;
pub(super) const IMPORT_KIND_NAMESPACE: u8 = 2;
pub(super) const IMPORT_KIND_SIDE_EFFECT: u8 = 3;

macro_rules! assert_cached_type_size {
    ($ty:ty, $size:expr) => {
        const _: () = assert!(
            std::mem::size_of::<$ty>() == $size,
            concat!(
                stringify!($ty),
                " size changed; bump CACHE_VERSION if the cached wire shape or extraction semantics changed, then update this assertion"
            )
        );
    };
}

assert_cached_type_size!(CachedModule, 1336);
assert_cached_type_size!(CachedNamespaceObjectAlias, 72);
assert_cached_type_size!(CachedLocalTypeDeclaration, 32);
assert_cached_type_size!(CachedPublicSignatureTypeReference, 56);
assert_cached_type_size!(CachedSuppression, 88);
assert_cached_type_size!(CachedUnknownSuppressionKind, 56);
assert_cached_type_size!(CachedExport, 136);
assert_cached_type_size!(CachedImport, 96);
assert_cached_type_size!(CachedDynamicImport, 88);
assert_cached_type_size!(CachedRequireCall, 88);
assert_cached_type_size!(CachedReExport, 88);
assert_cached_type_size!(CachedMember, 64);
assert_cached_type_size!(CachedDynamicImportPattern, 56);
assert_cached_type_size!(crate::MemberAccess, 48);
assert_cached_type_size!(fallow_types::extract::SemanticFact, 96);
assert_cached_type_size!(fallow_types::extract::CalleeUse, 32);
assert_cached_type_size!(fallow_types::extract::MisplacedDirectiveSite, 8);
assert_cached_type_size!(fallow_types::extract::SinkSite, 216);
assert_cached_type_size!(fallow_types::extract::FunctionComplexity, 96);
assert_cached_type_size!(fallow_types::extract::ComplexityContribution, 16);
assert_cached_type_size!(fallow_types::extract::FlagUse, 80);
assert_cached_type_size!(fallow_types::extract::ClassHeritageInfo, 96);
assert_cached_type_size!(fallow_types::extract::FactoryReturnExport, 48);
assert_cached_type_size!(fallow_types::extract::TypeMemberTypeEntry, 72);
assert_cached_type_size!(fallow_types::extract::LoadReturnKey, 32);

/// Cached data for a single module.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedModule {
    /// xxh3 hash of the file content.
    pub content_hash: u64,
    /// File modification time in nanoseconds for fast cache validation.
    /// When mtime+size match the on-disk file, we skip reading file content entirely.
    pub mtime_ns: u64,
    /// File size in bytes for fast cache validation.
    pub file_size: u64,
    /// Seconds-since-epoch at the time this entry was last WRITTEN
    /// (first parse or content-change refresh). NOT updated on cache-hit
    /// reads: `update_cache` already iterates every in-scope file every run,
    /// so refreshing on read would collapse the LRU to "last run this file
    /// was discovered" for every retained entry. With write-only refresh,
    /// the LRU genuinely targets stale (in-scope-but-unchanged-for-many-runs)
    /// entries. Used by `CacheStore::save` for write-time eviction ordering.
    pub last_access_secs: u64,
    /// Exported symbols.
    pub exports: Vec<CachedExport>,
    /// Import specifiers.
    pub imports: Vec<CachedImport>,
    /// Re-export specifiers.
    pub re_exports: Vec<CachedReExport>,
    /// Dynamic import specifiers.
    pub dynamic_imports: Vec<CachedDynamicImport>,
    /// `require()` specifiers.
    pub require_calls: Vec<CachedRequireCall>,
    /// Package names statically referenced through package path resolution.
    pub package_path_references: Box<[String]>,
    /// Static member accesses (e.g., `Status.Active`).
    pub member_accesses: Vec<crate::MemberAccess>,
    /// Typed semantic facts produced by extraction for cross-layer analysis.
    /// `None` means no facts, which keeps the common warm-cache payload lean.
    pub semantic_facts: Option<Box<[fallow_types::extract::SemanticFact]>>,
    /// Identifiers used as whole objects (Object.values, for..in, spread, etc.).
    pub whole_object_uses: Box<[String]>,
    /// Dynamic import patterns with partial static resolution.
    pub dynamic_import_patterns: Vec<CachedDynamicImportPattern>,
    /// Whether this module uses CJS exports.
    pub has_cjs_exports: bool,
    /// Whether this module declares at least one Angular `@Component({
    /// templateUrl: ... })` decorator. Mirrors `ModuleInfo.has_angular_component_template_url`
    /// so the CRAP-inherit walker's gate survives a warm-cache load.
    pub has_angular_component_template_url: bool,
    /// Local names of import bindings that are never referenced in this file.
    pub unused_import_bindings: Vec<String>,
    /// Local import bindings referenced from type positions.
    pub type_referenced_import_bindings: Vec<String>,
    /// Local import bindings referenced from value positions.
    pub value_referenced_import_bindings: Vec<String>,
    /// Inline suppression directives.
    pub suppressions: Vec<CachedSuppression>,
    /// Suppression tokens that did not parse to any known `IssueKind`. See #449.
    pub unknown_suppression_kinds: Vec<CachedUnknownSuppressionKind>,
    /// Pre-computed line-start byte offsets for O(log N) byte-to-line/col conversion.
    pub line_offsets: Vec<u32>,
    /// Per-function complexity metrics.
    pub complexity: Vec<fallow_types::extract::FunctionComplexity>,
    /// Feature flag use sites.
    pub flag_uses: Vec<fallow_types::extract::FlagUse>,
    /// Heritage metadata for exported classes.
    pub class_heritage: Vec<fallow_types::extract::ClassHeritageInfo>,
    /// Exported free-function factories that provably return one class instance
    /// (`export function useApi() { return new RESTApi() }`). Compacted to `None`
    /// when empty so the common no-factory module pays no payload. See #1441 Part A.
    pub exported_factory_returns: Option<Box<[fallow_types::extract::FactoryReturnExport]>>,
    /// Named-type property types declared by top-level interfaces and
    /// type-literal aliases (`interface Opts { c: OptDep }`). Compacted to
    /// `None` when empty so the common no-interface module pays no payload.
    /// See issue #1785.
    pub type_member_types: Option<Box<[fallow_types::extract::TypeMemberTypeEntry]>>,
    /// Angular `InjectionToken<Interface>` `(token, interface)` pairs (#920).
    pub injection_tokens: Vec<(String, String)>,
    /// Local type-capable declarations.
    pub local_type_declarations: Vec<CachedLocalTypeDeclaration>,
    /// Type references from exported public signatures.
    pub public_signature_type_references: Vec<CachedPublicSignatureTypeReference>,
    /// Namespace-import aliases re-exported through an object literal
    /// (`export const API = { foo }` where `foo` is `import * as foo from './bar'`).
    pub namespace_object_aliases: Vec<CachedNamespaceObjectAlias>,
    /// Iconify collection prefixes found in static icon props (issue #608).
    pub iconify_prefixes: Vec<String>,
    /// Nuxt UI icon class suffixes found in static script-side icon properties
    /// (issue #955).
    pub iconify_icon_names: Vec<String>,
    /// Bare identifier names that are candidates for convention auto-import
    /// resolution (issue #704). Content-local, so they round-trip through the
    /// cache; resolution against the plugin table happens at graph-build time.
    pub auto_import_candidates: Vec<String>,
    /// File-level string directives (`"use client"`, `"use server"`). Content-local,
    /// round-trips through the cache so the security `client-server-leak` detector
    /// sees directives on warm-cache loads.
    pub directives: Vec<String>,
    /// Byte-offset starts of `next/dynamic(..., { ssr: false })` dynamic imports.
    /// Content-local, round-trips so the security `client-server-leak` BFS sees
    /// the ssr:false client-only escape hatch on warm-cache loads.
    pub client_only_dynamic_import_spans: Vec<u32>,
    /// Captured security sink sites (category-blind). Round-trips through the
    /// cache so the catalogue-driven `tainted_sink` detector sees sinks on
    /// warm-cache loads.
    pub security_sinks: Vec<fallow_types::extract::SinkSite>,
    /// Count of sink-shaped nodes whose callee could not be flattened to a
    /// static path. Round-trips so the in-band blind-spot count is stable.
    pub security_sinks_skipped: u32,
    /// Span-level diagnostics for skipped security sink callees.
    pub security_unresolved_callee_sites: Vec<fallow_types::extract::SkippedSecurityCalleeSite>,
    /// Local bindings tied to the member-access path they were sourced from.
    /// Round-trips so the security `tainted_sink` source-to-sink association
    /// sees source-tainted bindings on warm-cache loads.
    pub tainted_bindings: Vec<fallow_types::extract::TaintedBinding>,
    /// Direct sink arguments recognized as sanitizer calls.
    pub sanitized_sink_args: Vec<fallow_types::extract::SanitizedSinkArg>,
    /// Defensive control call sites for security surface output.
    pub security_control_sites: Vec<fallow_types::extract::SecurityControlSite>,
    /// Deduped statically flattenable callee paths. Round-trips so the
    /// `boundaries.calls.forbidden` detector sees call sites on warm-cache
    /// loads.
    pub callee_uses: Vec<fallow_types::extract::CalleeUse>,
    /// Misplaced `"use client"` / `"use server"` directive sites.
    /// Round-trips so the `misplaced-directive` detector sees them on
    /// warm-cache loads.
    pub misplaced_directives: Vec<fallow_types::extract::MisplacedDirectiveSite>,
    /// Export local names of inline `"use server"` body Server Actions.
    /// Round-trips so the `unused-server-action` reclassifier sees them on
    /// warm-cache loads.
    pub inline_server_action_exports: Vec<String>,
    /// Vue `provide`/`inject` and Svelte `setContext`/`getContext` key sites.
    /// Round-trips so the `unprovided-inject` detector sees them on warm-cache
    /// loads.
    pub di_key_sites: Vec<fallow_types::extract::DiKeySite>,
    /// Whether the module had an unknowable-key provide. Round-trips so the
    /// `unprovided-inject` project-wide abstain holds on warm-cache loads.
    pub has_dynamic_provide: bool,
    /// Vue `<script setup>` `defineProps` and Svelte 5 `$props()` declared props.
    /// Round-trips so the `unused-component-prop` detector sees them on
    /// warm-cache loads.
    pub component_props: Vec<fallow_types::extract::ComponentProp>,
    /// Whether the template spreads `$attrs`/`$props`/`props` or the
    /// `defineProps` return is rest-destructured. Round-trips for the abstain.
    pub has_props_attrs_fallthrough: bool,
    /// Whether the SFC calls `defineExpose(...)`. Round-trips for the abstain.
    pub has_define_expose: bool,
    /// Whether the SFC calls `defineModel(...)`. Round-trips for the abstain.
    pub has_define_model: bool,
    /// Whether `defineProps` had an unharvestable type-reference argument.
    /// Round-trips for the abstain.
    pub has_unharvestable_props: bool,
    /// Vue `<script setup>` `defineEmits` declared events. Round-trips so the
    /// `unused-component-emit` detector sees them on warm-cache loads.
    pub component_emits: Vec<fallow_types::extract::ComponentEmit>,
    /// Angular component/directive inputs (`@Input()` decorators and signal
    /// `input()` / `model()` initializers). Round-trips so the
    /// `unused-component-input` detector sees them on warm-cache loads.
    pub angular_inputs: Vec<fallow_types::extract::AngularInputMember>,
    /// Angular component/directive outputs (`@Output()` decorators and signal
    /// `output()` / `outputFromObservable()` initializers). Round-trips so the
    /// `unused-component-output` detector sees them on warm-cache loads.
    pub angular_outputs: Vec<fallow_types::extract::AngularOutputMember>,
    /// Angular `@Component` declarations with their `selector` value(s).
    /// Round-trips so the Angular `unrendered-component` arm sees them on
    /// warm-cache loads.
    pub angular_component_selectors: Vec<fallow_types::extract::AngularComponentSelector>,
    /// Lit / web-component custom elements registered in this file. Round-trips so
    /// the Lit `unrendered-component` arm sees them on warm-cache loads.
    pub registered_custom_elements: Vec<fallow_types::extract::RegisteredCustomElement>,
    /// Custom-element tag names used (rendered) in this file's `html` templates.
    /// Round-trips for the Lit `unrendered-component` rendered-tag union.
    pub used_custom_element_tags: Vec<String>,
    /// Custom element selector tags referenced in this file's Angular templates.
    /// Round-trips for the Angular `unrendered-component` used-selector union.
    pub angular_used_selectors: Vec<String>,
    /// Angular route / bootstrap component class references. Round-trips for the
    /// Angular `unrendered-component` entry-point abstain.
    pub angular_entry_component_refs: Vec<String>,
    /// Whether this file dynamically renders a component (project-wide abstain
    /// signal for the Angular `unrendered-component` detector). Round-trips.
    pub has_dynamic_component_render: bool,
    /// Whether `defineEmits` had an unharvestable argument. Round-trips for the
    /// abstain.
    pub has_unharvestable_emits: bool,
    /// Whether an `emit(<nonLiteral>)` call was seen. Round-trips for the abstain.
    pub has_dynamic_emit: bool,
    /// Whether the emit binding was used as a whole value. Round-trips for the
    /// abstain.
    pub has_emit_whole_object_use: bool,
    /// SvelteKit `load()` return-object keys. Round-trips so the
    /// `unused-load-data-key` detector sees them on warm-cache loads.
    pub load_return_keys: Vec<fallow_types::extract::LoadReturnKey>,
    /// Whether this file's `load()` body could not be harvested safely.
    /// Round-trips for the abstain.
    pub has_unharvestable_load: bool,
    /// Whether this file passes the whole `data` object opaquely. Round-trips
    /// for the `unused-load-data-key` abstain.
    pub has_load_data_whole_use: bool,
    /// React/JSX component definitions. Round-trips so the React-health phases
    /// see them on warm-cache loads.
    pub component_functions: Vec<fallow_types::extract::ComponentFunction>,
    /// React component props. Round-trips so the React `unused-component-prop`
    /// arm sees them on warm-cache loads.
    pub react_props: Vec<fallow_types::extract::ComponentProp>,
    /// React hook call sites. Round-trips for the complexity-fold phase.
    pub hook_uses: Vec<fallow_types::extract::HookUse>,
    /// React render edges (child name captured; resolution deferred to graph
    /// build). Round-trips so the render graph survives a warm cache.
    pub render_edges: Vec<fallow_types::extract::RenderEdge>,
    /// Svelte custom events dispatched via `dispatch('<name>')`. Round-trips so
    /// the `unused-svelte-event` detector sees them on warm-cache loads.
    pub svelte_dispatched_events: Vec<fallow_types::extract::DispatchedEvent>,
    /// Svelte template `on:<name>` listener names on component tags. Round-trips
    /// so the project-wide listened set is correct on warm-cache loads.
    pub svelte_listened_events: Vec<String>,
    /// Whether a `dispatch(<nonLiteral>)` call or whole-`dispatch`-value use was
    /// seen. Round-trips for the `unused-svelte-event` abstain.
    pub has_dynamic_dispatch: bool,
}

impl CachedModule {
    /// Source metadata fingerprint stored with this cache entry.
    ///
    #[must_use]
    pub fn source_fingerprint(&self) -> fallow_types::source_fingerprint::SourceFingerprint {
        fallow_types::source_fingerprint::SourceFingerprint::new(self.mtime_ns, self.file_size)
    }
}

/// Cached namespace-object alias.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedNamespaceObjectAlias {
    /// Canonical export name on this module.
    pub via_export_name: String,
    /// Dotted suffix of the property path relative to the export.
    pub suffix: String,
    /// Local name of the namespace import on this module.
    pub namespace_local: String,
}

/// Cached local type declaration.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedLocalTypeDeclaration {
    /// Local declaration name.
    pub name: String,
    /// Byte offset of the declaration span start.
    pub span_start: u32,
    /// Byte offset of the declaration span end.
    pub span_end: u32,
}

/// Cached public signature type reference.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedPublicSignatureTypeReference {
    /// Exported symbol whose signature contains the reference.
    pub export_name: String,
    /// Referenced type name.
    pub type_name: String,
    /// Byte offset of the reference span start.
    pub span_start: u32,
    /// Byte offset of the reference span end.
    pub span_end: u32,
}

/// Cached suppression directive.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedSuppression {
    /// 1-based line this suppression applies to. 0 = file-wide.
    pub line: u32,
    /// 1-based line where the comment itself appears.
    pub comment_line: u32,
    /// 0 = suppress all, otherwise `IssueKind` discriminant.
    pub kind: u8,
    /// Rule-pack name for scoped policy suppressions. Empty for all other
    /// suppression targets.
    pub policy_pack: String,
    /// Rule id for scoped policy suppressions. Empty for all other suppression
    /// targets.
    pub policy_rule_id: String,
    /// Human-authored reason after `--`, when present.
    pub reason: Option<String>,
}

/// Cached unknown suppression kind token (see #449).
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedUnknownSuppressionKind {
    /// 1-based line where the comment itself appears.
    pub comment_line: u32,
    /// True when the marker was `fallow-ignore-file`.
    pub is_file_level: bool,
    /// The verbatim token that did not parse.
    pub token: String,
    /// Human-authored reason after `--`, when present.
    pub reason: Option<String>,
}

/// Cached export data for a single export declaration.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedExport {
    /// Export name (or "default" for default exports).
    pub name: String,
    /// Whether this is a default export.
    pub is_default: bool,
    /// Whether this is a type-only export.
    pub is_type_only: bool,
    /// Whether this export is registered through a runtime side effect at
    /// module load time (Lit `@customElement` decorator or
    /// `customElements.define` call). Persisted so warm-cache runs continue
    /// to skip unused-export reporting for these classes.
    pub is_side_effect_used: bool,
    /// Visibility tag discriminant (0=None, 1=Public, 2=Internal, 3=Beta, 4=Alpha).
    pub visibility: u8,
    /// Human-authored reason on `@expected-unused -- <reason>`, when present.
    pub expected_unused_reason: Option<String>,
    /// The local binding name, if different.
    pub local_name: Option<String>,
    /// Byte offset of the export span start.
    pub span_start: u32,
    /// Byte offset of the export span end.
    pub span_end: u32,
    /// Members of this export (for enums and classes).
    pub members: Vec<CachedMember>,
    /// The local name of the parent class from `extends` clause, if any.
    pub super_class: Option<String>,
}

/// Cached import data for a single import declaration.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedImport {
    /// The import specifier.
    pub source: String,
    /// For Named imports, the imported symbol name. Empty for other kinds.
    pub imported_name: String,
    /// The local binding name.
    pub local_name: String,
    /// Whether this is a type-only import.
    pub is_type_only: bool,
    /// Whether this import originated from an SFC `<style>` block / `<style src>` (CSS context).
    pub from_style: bool,
    /// Import kind: 0=Named, 1=Default, 2=Namespace, 3=SideEffect.
    pub kind: u8,
    /// Byte offset of the import span start.
    pub span_start: u32,
    /// Byte offset of the import span end.
    pub span_end: u32,
    /// Byte offset of the source string literal span start.
    pub source_span_start: u32,
    /// Byte offset of the source string literal span end.
    pub source_span_end: u32,
}

/// Cached dynamic import data.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedDynamicImport {
    /// The import specifier.
    pub source: String,
    /// Byte offset of the span start.
    pub span_start: u32,
    /// Byte offset of the span end.
    pub span_end: u32,
    /// Names destructured from the import result.
    pub destructured_names: Vec<String>,
    /// Local variable name for namespace imports.
    pub local_name: Option<String>,
    /// True when this dynamic import was synthesised by fallow (see
    /// `DynamicImportInfo::is_speculative`).
    pub is_speculative: bool,
}

/// Cached `require()` call data.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedRequireCall {
    /// The require specifier.
    pub source: String,
    /// Byte offset of the span start.
    pub span_start: u32,
    /// Byte offset of the span end.
    pub span_end: u32,
    /// Byte offset of the specifier string-literal span start.
    pub source_span_start: u32,
    /// Byte offset of the specifier string-literal span end.
    pub source_span_end: u32,
    /// Names destructured from the require result.
    pub destructured_names: Vec<String>,
    /// Local variable name for namespace requires.
    pub local_name: Option<String>,
}

/// Cached re-export data.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedReExport {
    /// The module being re-exported from.
    pub source: String,
    /// Name imported from the source.
    pub imported_name: String,
    /// Name exported from this module.
    pub exported_name: String,
    /// Whether this is a type-only re-export.
    pub is_type_only: bool,
    /// Byte offset of the re-export span start (for line-number reporting).
    pub span_start: u32,
    /// Byte offset of the re-export span end.
    pub span_end: u32,
}

/// Cached enum or class member data.
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedMember {
    /// Member name.
    pub name: String,
    /// Member kind (enum, method, or property).
    pub kind: MemberKind,
    /// Byte offset of the span start.
    pub span_start: u32,
    /// Byte offset of the span end.
    pub span_end: u32,
    /// Whether this member has decorators.
    pub has_decorator: bool,
    /// Full dotted path of each decorator (e.g. `step`, `ns.foo`).
    /// Empty for undecorated members and decorators with non-identifier
    /// expressions.
    pub decorator_names: Vec<String>,
    /// True when this is a static method that returns a fresh instance of
    /// the class: body returns `new this()` / `new <SameClassName>()`, or the
    /// declared return type matches the class name. Treated as a factory.
    /// See issues #346, #387.
    pub is_instance_returning_static: bool,
    /// True when this instance method's call result is an instance of the
    /// same class (declared return type matches the class name, or body's
    /// last statement is `return this`). Drives fluent-chain credit. See
    /// issue #387.
    pub is_self_returning: bool,
}

/// Cached dynamic import pattern data (template literals, `import.meta.glob`).
#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedDynamicImportPattern {
    /// Static prefix of the import path.
    pub prefix: String,
    /// Static suffix, if any.
    pub suffix: Option<String>,
    /// Byte offset of the span start.
    pub span_start: u32,
    /// Byte offset of the span end.
    pub span_end: u32,
}
