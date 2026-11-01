//! Metric and rule definitions for explainable CLI output.
//!
//! Provides structured metadata that describes what each metric, threshold,
//! and rule means, consumed by the `_meta` object in JSON output and by
//! SARIF `fullDescription` / `helpUri` fields.

use serde_json::Value;

/// Rule definition for SARIF `fullDescription` and JSON `_meta`.
pub struct RuleDef {
    /// Canonical rule id, such as `fallow/unused-export` or
    /// `security/sql-injection`; used as the SARIF rule id.
    pub id: &'static str,
    /// Coarse category label used by the sticky PR/MR comment renderer to
    /// group findings into collapsible sections (Dead code, Dependencies,
    /// Duplication, Health, Architecture, Suppressions). One source of
    /// truth so the CodeClimate / SARIF / review-envelope path and the
    /// renderer never drift; a unit test below asserts every RuleDef has
    /// a non-empty category.
    pub category: &'static str,
    /// Human-readable rule title.
    pub name: &'static str,
    /// One-line description used as the SARIF `shortDescription`.
    pub short: &'static str,
    /// Paragraph-length description used as the SARIF `fullDescription`.
    pub full: &'static str,
    /// Path under the docs site base URL for the rule's `helpUri`.
    pub docs_path: &'static str,
}

/// Rule definitions for every dead-code family finding.
pub const CHECK_RULES: &[RuleDef] = &[
    RuleDef {
        id: "fallow/unused-file",
        category: "Dead code",
        name: "Unused Files",
        short: "File is not reachable from any entry point",
        full: "Source files that are not imported by any other module and are not entry points (scripts, tests, configs). These files can safely be deleted. Detection uses graph reachability from configured entry points.",
        docs_path: "explanations/dead-code#unused-files",
    },
    RuleDef {
        id: "fallow/unused-export",
        category: "Dead code",
        name: "Unused Exports",
        short: "Export is never imported",
        full: "Named exports that are never imported by any other module in the project. Includes both direct exports and re-exports through barrel files. The export may still be used locally within the same file.",
        docs_path: "explanations/dead-code#unused-exports",
    },
    RuleDef {
        id: "fallow/unused-type",
        category: "Dead code",
        name: "Unused Type Exports",
        short: "Type export is never imported",
        full: "Type-only exports (interfaces, type aliases, enums used only as types) that are never imported. These do not generate runtime code but add maintenance burden.",
        docs_path: "explanations/dead-code#unused-types",
    },
    RuleDef {
        id: "fallow/private-type-leak",
        category: "Dead code",
        name: "Private Type Leaks",
        short: "Exported signature references a private type",
        full: "Exported values or types whose public TypeScript signature references a same-file type declaration that is not exported. Consumers cannot name that private type directly, so the backing type should be exported or removed from the public signature.",
        docs_path: "explanations/dead-code#private-type-leaks",
    },
    RuleDef {
        id: "fallow/deprecated-export-in-use",
        category: "Dead code",
        name: "Deprecated Exports in Use",
        short: "Export marked @deprecated is still referenced",
        full: "Exports whose leading JSDoc carries @deprecated and that still have at least one consumer in a reachable file. Each finding lists the exact consumer count, a sample of up to 10 consumers sorted by path and line, and the deprecation message. A deprecated export with no consumer is reported as an unused export instead, marked deprecated. A consumer in an unreachable file does not count, because that file is dead code itself. `public_api` is true when the export lives in an entry point or a re-export chain reaches one: external consumers are not visible, so the finding makes no removal claim. The rule is opt-in and defaults to off. A per-path override resolves on the file that declares the export, not on the consumer.",
        docs_path: "explanations/dead-code#deprecated-exports-in-use",
    },
    RuleDef {
        id: "fallow/unused-dependency",
        category: "Dependencies",
        name: "Unused Dependencies",
        short: "Dependency listed but never imported",
        full: "Packages listed in dependencies that are never imported or required by any source file. Framework plugins and CLI tools may be false positives; use the ignore_dependencies config to suppress.",
        docs_path: "explanations/dead-code#unused-dependencies",
    },
    RuleDef {
        id: "fallow/unused-dev-dependency",
        category: "Dependencies",
        name: "Unused Dev Dependencies",
        short: "Dev dependency listed but never imported",
        full: "Packages listed in devDependencies that are never imported by test files, config files, or scripts. Build tools and jest presets that are referenced only in config may appear as false positives.",
        docs_path: "explanations/dead-code#unused-devdependencies",
    },
    RuleDef {
        id: "fallow/unused-optional-dependency",
        category: "Dependencies",
        name: "Unused Optional Dependencies",
        short: "Optional dependency listed but never imported",
        full: "Packages listed in optionalDependencies that are never imported. Optional dependencies are typically platform-specific; verify they are not needed on any supported platform before removing.",
        docs_path: "explanations/dead-code#unused-optionaldependencies",
    },
    RuleDef {
        id: "fallow/type-only-dependency",
        category: "Dependencies",
        name: "Type-only Dependencies",
        short: "Production dependency only used via type-only imports",
        full: "Production dependencies that are only imported via `import type` statements. These can be moved to devDependencies since they generate no runtime code and are stripped during compilation.",
        docs_path: "explanations/dead-code#type-only-dependencies",
    },
    RuleDef {
        id: "fallow/test-only-dependency",
        category: "Dependencies",
        name: "Test-only Dependencies",
        short: "Production dependency only imported by test files",
        full: "Production dependencies that are only imported from test files. These can usually move to devDependencies because production entry points do not require them at runtime.",
        docs_path: "explanations/dead-code#test-only-dependencies",
    },
    RuleDef {
        id: "fallow/dev-dependency-in-production",
        category: "Dependencies",
        name: "Dev Dependencies Used in Production",
        short: "devDependency imported by production code with a runtime import",
        full: "A package in `devDependencies` that is imported by production (non-test, non-config) source code via a runtime/value import. Whether it should be promoted to `dependencies` depends on the deployment: a production-only install (`pnpm install --prod`) omits devDependencies, so an import resolved at runtime breaks, while a build that inlines the package into its output resolves nothing at runtime and promoting it can instead make the deployment require an install it did not need. This is the promote-side mirror of `test-only-dependency` and `type-only-dependency`. A dev dependency imported from production code only via `import type` is NOT flagged (types are erased at build time), and a package also listed in `dependencies`, `peerDependencies`, or `optionalDependencies` is left alone because another manifest section provides it at runtime.",
        docs_path: "explanations/dead-code#dev-dependencies-in-production",
    },
    RuleDef {
        id: "fallow/unused-enum-member",
        category: "Dead code",
        name: "Unused Enum Members",
        short: "Enum member is never referenced",
        full: "Enum members that are never referenced in the codebase. Uses scope-aware binding analysis to track all references including computed access patterns.",
        docs_path: "explanations/dead-code#unused-enum-members",
    },
    RuleDef {
        id: "fallow/unused-class-member",
        category: "Dead code",
        name: "Unused Class Members",
        short: "Class member is never referenced",
        full: "Class methods and properties that are never referenced outside the class. Private members are checked within the class scope; public members are checked project-wide.",
        docs_path: "explanations/dead-code#unused-class-members",
    },
    RuleDef {
        id: "fallow/unused-store-member",
        category: "Dead code",
        name: "Unused Store Members",
        short: "Store member is never accessed by any consumer",
        full: "Pinia store members (a `state` / `getters` / `actions` key, or a setup-store returned key) declared but never accessed by any consumer project-wide. The store binding is imported (so the module is reachable) yet a specific member is dead. Defaults to warn, not error: a store has an open declaration surface (plugins, dynamic dispatch) so confidence is lower. Activates only when pinia or @pinia/nuxt is a declared dependency.",
        docs_path: "explanations/dead-code#unused-store-members",
    },
    RuleDef {
        id: "fallow/unresolved-import",
        category: "Dead code",
        name: "Unresolved Imports",
        short: "Import could not be resolved",
        full: "Import specifiers that could not be resolved to a file on disk. Common causes: deleted files, typos in paths, missing path aliases in tsconfig, or uninstalled packages.",
        docs_path: "explanations/dead-code#unresolved-imports",
    },
    RuleDef {
        id: "fallow/unlisted-dependency",
        category: "Dependencies",
        name: "Unlisted Dependencies",
        short: "Dependency used but not in package.json",
        full: "Packages imported without an applicable dependency declaration in package.json. This includes workspace packages imported from the repository root: npm workspace links make a package resolvable, but do not declare a dependency. Package self-references are allowed. Private packages and tooling may use ancestor declarations; publishable runtime code must declare its own dependencies.",
        docs_path: "explanations/dead-code#unlisted-dependencies",
    },
    RuleDef {
        id: "fallow/duplicate-export",
        category: "Dead code",
        name: "Duplicate Exports",
        short: "Export name appears in multiple modules",
        full: "The same export name is defined in multiple modules. Consumers may import from the wrong module, leading to subtle bugs. Consider renaming or consolidating.",
        docs_path: "explanations/dead-code#duplicate-exports",
    },
    RuleDef {
        id: "fallow/circular-dependency",
        category: "Architecture",
        name: "Circular Dependencies",
        short: "Circular dependency chain detected",
        full: "A cycle in the module import graph. Circular dependencies cause undefined behavior with CommonJS (partial modules) and initialization ordering issues with ESM. Break cycles by extracting shared code.",
        docs_path: "explanations/dead-code#circular-dependencies",
    },
    RuleDef {
        id: "fallow/re-export-cycle",
        category: "Architecture",
        name: "Re-Export Cycles",
        short: "Two or more barrel files re-export from each other in a loop",
        full: "A barrel file re-exports from another barrel that ultimately re-exports back. When this happens, imports from any file in the loop may silently come up empty, because the re-export chain has no terminating module to resolve names against. To fix this: open any one file in the loop and remove the `export * from` (or `export { ... } from`) statement that points back into the cycle. Any single removal will break the cycle and restore working re-exports. A self-loop (a single barrel re-exporting from itself, often a rename leftover) is reported under the same rule with kind `self-loop`.",
        docs_path: "explanations/dead-code#re-export-cycles",
    },
    RuleDef {
        id: "fallow/package-cycle",
        category: "Architecture",
        name: "Package Cycles",
        short: "Two or more workspace packages import each other in a loop",
        full: "Workspace packages whose resolved imports form a cycle. Each workspace package is a node and each resolved import from one package to another is an edge, so a package cycle can exist when no file-level cycle exists. Imports from test, spec, story, fixture and tooling config files are not edges, because those files are not part of the package build. Packages in a cycle cannot be built in dependency order. Each hop shows one example import and a `type_only` flag; a type-only hop still matters for declaration builds. To fix this, remove the imports on one hop, for example by moving the shared code to a package that both packages import.",
        docs_path: "explanations/dead-code#package-cycles",
    },
    RuleDef {
        id: "fallow/boundary-violation",
        category: "Architecture",
        name: "Boundary Violations",
        short: "Import crosses a configured architecture boundary",
        full: "A module imports from a zone that its configured boundary rules do not allow. Boundary checks help keep layered architecture, feature slices, and package ownership rules enforceable.",
        docs_path: "explanations/dead-code#boundary-violations",
    },
    RuleDef {
        id: "fallow/boundary-coverage",
        category: "Architecture",
        name: "Boundary Coverage",
        short: "Source file matches no configured architecture boundary zone",
        full: "A reachable source file is not assigned to any configured boundary zone while boundaries.coverage.requireAllFiles is enabled. Add the file to a zone pattern, move it under an existing zone, or allow-list generated and intentionally unzoned paths with boundaries.coverage.allowUnmatched.",
        docs_path: "explanations/dead-code#boundary-violations",
    },
    RuleDef {
        id: "fallow/boundary-call-violation",
        category: "Architecture",
        name: "Boundary Call Violation",
        short: "Zoned file calls a callee its zone forbids",
        full: "A file classified into a boundary zone calls or constructs a callee matching one of the zone's boundaries.calls.forbidden patterns. The check is syntactic: it matches the written callee path and the import-resolved canonical path of calls and new expressions, and it only applies to files classified into a zone. Move the call behind an allowed abstraction, or adjust the zone's forbidden patterns if the rule was wrong.",
        docs_path: "explanations/dead-code#boundary-violations",
    },
    RuleDef {
        id: "fallow/policy-violation",
        category: "Policy",
        name: "Policy Violation",
        short: "Usage violates a configured rule-pack policy",
        full: "A call, import, export, catalogue-derived effect, or graph-resolved GDP proof producer violates a configured rule pack (the rulePacks config key). Packs are pure declarative data. Call rules match calls and new expressions, on written plus import-resolved canonical callees. Effect rules match calls only. Import rules match the raw specifier. GDP rules trace @gdp-ts/core proof producers through unambiguous re-exports and restrict which files may create proof factories, optionally by exact static proof kind. This is syntactic ownership analysis, not a runtime authorization guarantee. Follow the rule's message, move a producer into an allowed file, scope the rule with files/exclude globs, or adjust its severity.",
        docs_path: "explanations/dead-code#policy-violations",
    },
    RuleDef {
        id: "fallow/stale-suppression",
        category: "Suppressions",
        name: "Stale Suppressions",
        short: "Suppression comment or tag no longer matches any issue",
        full: "A fallow-ignore-next-line, fallow-ignore-file, or @expected-unused suppression that no longer matches any active issue. The underlying problem was fixed but the suppression was left behind. Remove it to keep the codebase clean.",
        docs_path: "explanations/dead-code#stale-suppressions",
    },
    RuleDef {
        id: "fallow/missing-suppression-reason",
        category: "Suppressions",
        name: "Missing Suppression Reason",
        short: "Suppression comment omits a required reason",
        full: "A fallow-ignore-next-line or fallow-ignore-file suppression omits the explanatory reason required by the requireSuppressionReason rule. Add a short reason after the suppression token, or remove the suppression if the issue is no longer intentional.",
        docs_path: "explanations/dead-code#stale-suppressions",
    },
    RuleDef {
        id: "fallow/unused-catalog-entry",
        category: "Dependencies",
        name: "Unused catalog entry",
        short: "Catalog entry not referenced by any workspace package",
        full: "An entry in a package manager catalog (`pnpm-workspace.yaml` `catalog:` / `catalogs:` or Bun root `package.json` `workspaces.catalog` / `workspaces.catalogs`) that no workspace package.json dependency and no pnpm override (`pnpm-workspace.yaml` `overrides`, skipped when `packageManager` names a pnpm version that ignores it: pnpm before 10.5.1, or pnpm 10 or earlier when the root `package.json` declares non-empty `pnpm.overrides` or `resolutions`; root `package.json` `pnpm.overrides`, skipped when `packageManager` names pnpm 11 or later; root `package.json` `resolutions`, read only when `packageManager` names pnpm 10 or earlier) references via the `catalog:` protocol. Catalog entries are leftover dependency metadata once a package is removed from every consumer; delete the entry to keep the catalog truthful. See also: fallow/unresolved-catalog-reference (the inverse: consumer references a catalog that does not declare the package).",
        docs_path: "explanations/dead-code#unused-catalog-entries",
    },
    RuleDef {
        id: "fallow/empty-catalog-group",
        category: "Dependencies",
        name: "Empty catalog group",
        short: "Named catalog group has no entries",
        full: "A named group under `catalogs:` in `pnpm-workspace.yaml` or Bun root `package.json` has no package entries. Empty named groups are leftover catalog structure after the last entry is removed. The default `catalog` map is intentionally ignored because some projects keep it as a stable hook.",
        docs_path: "explanations/dead-code#empty-catalog-groups",
    },
    RuleDef {
        id: "fallow/unresolved-catalog-reference",
        category: "Dependencies",
        name: "Unresolved catalog reference",
        short: "package.json references a catalog that does not declare the package",
        full: "A workspace package.json declares a dependency with the `catalog:` or `catalog:<name>` protocol, but the catalog has no entry for that package. The package manager install will fail until the catalog is fixed. To fix: add the package to the named catalog, switch the reference to a different catalog that does declare it, or remove the reference and pin a hardcoded version. Scope: the detector scans `dependencies`, `devDependencies`, `peerDependencies`, and `optionalDependencies` in every workspace `package.json`, plus the pnpm override values in `pnpm-workspace.yaml` `overrides` (skipped when `packageManager` names a pnpm version that ignores it: pnpm before 10.5.1, or pnpm 10 or earlier when the root `package.json` declares non-empty `pnpm.overrides` or `resolutions`), root `package.json` `pnpm.overrides` (skipped when `packageManager` names pnpm 11 or later, because pnpm 11 does not read it), and root `package.json` `resolutions` (read only when `packageManager` names pnpm 10 or earlier), using `pnpm-workspace.yaml` catalogs when present and Bun root `package.json` catalogs otherwise. See also: fallow/unused-catalog-entry (the inverse: catalog entries no consumer references).",
        docs_path: "explanations/dead-code#unresolved-catalog-references",
    },
    RuleDef {
        id: "fallow/unused-dependency-override",
        category: "Dependencies",
        name: "Unused dependency override",
        short: "Package-manager override target is not declared or resolved",
        full: "An entry in `pnpm-workspace.yaml`'s `overrides:` section (skipped when `packageManager` names a pnpm version that ignores it: pnpm before 10.5.1, or pnpm 10 or earlier when the root `package.json` declares non-empty `pnpm.overrides` or `resolutions`), `package.json#pnpm.overrides` (skipped when `packageManager` names pnpm 11 or later, which does not read it), npm or Bun's top-level `package.json#overrides`, or Bun's `package.json#resolutions` whose target package is not declared by any workspace package and is not present in the active readable lockfile (`pnpm-lock.yaml`, `package-lock.json`, `npm-shrinkwrap.json`, or `bun.lock`). Override entries linger after their target package leaves the resolved dependency tree. pnpm and npm projects without a readable lockfile fall back to workspace package manifests and keep a `hint` so transitive CVE pins can be reviewed before removal. Bun projects with only binary `bun.lockb` fail closed: fallow reports a workspace diagnostic and emits no unused-override finding until a readable `bun.lock` exists. To fix: delete the entry, refresh the active lockfile if it is stale, or add the entry to `ignoreDependencyOverrides` when the override is intentionally retained. See also: fallow/misconfigured-dependency-override.",
        docs_path: "explanations/dead-code#unused-dependency-overrides",
    },
    RuleDef {
        id: "fallow/misconfigured-dependency-override",
        category: "Dependencies",
        name: "Misconfigured dependency override",
        short: "Package-manager override has an unparsable key or value",
        full: "An entry in `pnpm-workspace.yaml#overrides` (skipped when `packageManager` names a pnpm version that ignores it: pnpm before 10.5.1, or pnpm 10 or earlier when the root `package.json` declares non-empty `pnpm.overrides` or `resolutions`), `package.json#pnpm.overrides` (skipped when `packageManager` names pnpm 11 or later, which does not read it), npm or Bun's top-level `package.json#overrides`, or Bun's `package.json#resolutions` whose key or value cannot be interpreted in that source's override grammar. Common shapes include an empty key, an empty value, a malformed version selector (`@types/react@<<18`), or an unbalanced parent matcher (`react>`). The active package manager may reject or ignore the entry. To fix: correct the key or value according to that package manager's grammar, or remove the entry. See also: fallow/unused-dependency-override.",
        docs_path: "explanations/dead-code#misconfigured-dependency-overrides",
    },
    RuleDef {
        id: "fallow/invalid-client-export",
        category: "Policy",
        name: "Invalid client export",
        short: "\"use client\" file exports a server-only / route-config name",
        full: "A file carrying the `\"use client\"` directive also exports a Next.js server-only or route-segment config name (such as `metadata`, `generateMetadata`, `revalidate`, `generateStaticParams`, or a route HTTP method like `GET`/`POST`). Next.js rejects this combination at build time. Move the server-only export to a non-client module (a server component, a `route.ts`, or a separate config file), or remove the `\"use client\"` directive if the module does not need to be a client boundary. The check runs only when the project declares `next`.",
        docs_path: "explanations/dead-code#invalid-client-exports",
    },
    RuleDef {
        id: "fallow/mixed-client-server-barrel",
        category: "Policy",
        name: "Mixed client/server barrel",
        short: "Barrel re-exports both a \"use client\" module and a server-only module",
        full: "A barrel file (a module whose exports are `export ... from` re-exports) forwards a name from a `\"use client\"` module alongside a name from a server-only module (one carrying `\"use server\"`, importing the `server-only` package, or importing a server-only Next.js API such as `next/headers`). Importing one name from such a barrel drags the other's directive context across the React Server Components boundary, the documented Next.js App Router footgun. Type-only re-exports are ignored (erased at build), and a barrel re-exporting a client module alongside an ordinary undirected utility does NOT flag. To fix: split the barrel so client and server-only modules are re-exported from separate entry points. The check runs only when the project declares `next`.",
        docs_path: "explanations/dead-code#mixed-client-server-barrels",
    },
    RuleDef {
        id: "fallow/misplaced-directive",
        category: "Policy",
        name: "Misplaced directive",
        short: "\"use client\" / \"use server\" directive is not in the leading position and is ignored",
        full: "A `\"use client\"` or `\"use server\"` directive string appears as an expression statement after a non-directive statement (an `import`, a `const`). React Server Components bundlers only honor a directive in the leading prologue, before any other statement; once any statement precedes it the string is parsed as an ordinary expression and SILENTLY IGNORED. The intended client/server boundary never takes effect, so the file is treated as a server module. To fix: move the directive to the very top of the file, above every import. The check runs only when the project declares `next`.",
        docs_path: "explanations/dead-code#misplaced-directives",
    },
    RuleDef {
        id: "fallow/unprovided-inject",
        category: "Dead code",
        name: "Unprovided injects",
        short: "inject() / getContext() reads a key that no provide() / setContext() supplies",
        full: "A Vue `inject(KEY)` or Svelte `getContext(KEY)` reads a dependency-injection key (an imported or module-local symbol) that no matching `provide(KEY)` / `setContext(KEY)` supplies anywhere in the project. The read resolves to undefined at runtime, surfaced only at render. To fix: add a matching provider for the key, or remove the dead inject. Defaults to warn, not error: a provider may live outside the analyzed graph (an app-level provide registered elsewhere, a plugin, a host application). String-literal keys and keys imported from a package are abstained.",
        docs_path: "explanations/dead-code#unprovided-injects",
    },
    RuleDef {
        id: "fallow/unrendered-component",
        category: "Dead code",
        name: "Unrendered components",
        short: "A Vue / Svelte component is reachable through a barrel but rendered nowhere",
        full: "A Vue or Svelte single-file component (the default export of a `.vue` / `.svelte` file) is reachable in the module graph (a barrel re-exports it) but instantiated NOWHERE in the project: no `<Tag>`, no `:is` / `this=` binding, no `components` / `app.component` registration, no `h()` / auto-import use, and no script value-read. It survives unused-file (the barrel keeps it reachable) and unused-export (the re-export counts as a use), yet no file actually renders it. To fix: render the component somewhere, or delete it and drop the dead re-export. Defaults to warn, not error: a component can be rendered reflectively (a dynamic `<component :is>` resolved from a non-literal value), so analyzer confidence is lower. Components that are themselves entry points (route pages, layouts, `App.vue`) and components re-exported from a non-private package entry point are abstained.",
        docs_path: "explanations/dead-code#unrendered-components",
    },
    RuleDef {
        id: "fallow/absent-component-prop",
        category: "Dead code",
        name: "Absent optional component props",
        short: "Known reachable callers omit an optional prop consumed inside its component",
        full: "A used optional component input is supplied by none of its inspected reachable callers. The finding includes the declaration, framework, default presence and caller locations. Enable absent-component-props explicitly; it defaults to off. Supported contracts cover React, Preact, Solid, Qwik, Vue, Svelte, Astro, Angular, Lit and Ember. Unknown spreads, dynamic or escaped component references, ambiguous bindings, incomplete contracts and externally exposed APIs abstain. Review defaults and API intent manually; static analysis does not prove runtime unreachability.",
        docs_path: "explanations/dead-code#absent-component-props",
    },
    RuleDef {
        id: "fallow/unused-component-prop",
        category: "Dead code",
        name: "Unused component props",
        short: "A Vue, Svelte, or React component prop is referenced nowhere in its own component",
        full: "A declared component prop referenced nowhere inside its own component, in these framework shapes: a Vue `<script setup>` defineProps prop, a Svelte 5 `$props()` prop, or a React/Preact prop destructured from a component's first parameter and read nowhere in its body. Framework type checkers check caller-side prop correctness, not this in-component dead-input direction. Conservative: Vue abstains on `$attrs` fallthrough, whole-object props use, defineExpose, defineModel, and imported prop-type aliases; Svelte abstains on rest, computed, nested, and whole-object `$props()` shapes; React abstains on rest spread (`{...rest}`), props forwarded by spread, props passed wholesale to a hook, `forwardRef` / imported-interface props, and exported public-API component props. Default warn; suppress or remove the prop.",
        docs_path: "explanations/dead-code#unused-component-props",
    },
    RuleDef {
        id: "fallow/unused-component-emit",
        category: "Dead code",
        name: "Unused component emits",
        short: "A Vue <script setup> defineEmits event is emitted nowhere in its own component",
        full: "A Vue `<script setup>` defineEmits declared event that is emitted nowhere in its own component (no `emit('<name>')` call). vue-tsc / Volar check caller-side emit correctness, not this in-component dead-output direction. Conservative: abstains on `$attrs` fallthrough, whole-object emit use, defineExpose, defineModel, and imported emit-type aliases. Default warn; suppress or remove the emit.",
        docs_path: "explanations/dead-code#unused-component-emits",
    },
    RuleDef {
        id: "fallow/unused-component-input",
        category: "Dead code",
        name: "Unused component inputs",
        short: "An Angular @Input() / signal input() / model() input is read nowhere in its own component",
        full: "An Angular `@Input()` / signal `input()` / `model()` declared input that is read nowhere in its own component (neither the inline / external template nor the class body). The Angular compiler never flags a declared-but-unread `@Input`, and there is no `@angular-eslint` rule for it. Conservative: usage detection over-credits by design (a template sentinel ref, any class-body member access by that name, or a bare identifier read counts as used), and the whole component abstains on an unresolved `extends` heritage clause (a base class in another file may read the input). A `model()` is recorded as an input only. Default warn; suppress or remove the input. The check runs only when the project declares `@angular/core`.",
        docs_path: "explanations/dead-code#unused-component-inputs",
    },
    RuleDef {
        id: "fallow/unused-component-output",
        category: "Dead code",
        name: "Unused component outputs",
        short: "An Angular @Output() / signal output() output is emitted nowhere in its own component",
        full: "An Angular `@Output()` / signal `output()` declared output that is emitted nowhere in its own component (no `this.<output>.emit(...)`). The Angular compiler never flags a declared-but-unemitted `@Output`, and there is no `@angular-eslint` rule for it. Conservative: usage detection over-credits by design (a `this.<output>.emit` call site, or any value read of `this.<output>` that might forward it, counts as used), and the whole component abstains on an unresolved `extends` heritage clause. A `model()`-derived implicit output is never flagged. Default warn; suppress or remove the output. The check runs only when the project declares `@angular/core`.",
        docs_path: "explanations/dead-code#unused-component-outputs",
    },
    RuleDef {
        id: "fallow/unused-svelte-event",
        category: "Dead code",
        name: "Unused Svelte events",
        short: "A Svelte component dispatches a createEventDispatcher event whose name is listened to nowhere in the project",
        full: "A Svelte component that dispatches a custom event via `createEventDispatcher` (`const dispatch = createEventDispatcher(); dispatch('save')`) whose event name is listened to NOWHERE in the analyzed project. This is the cross-file dead-OUTPUT direction: the component fires an event nothing handles. No native tool covers the listener side: eslint-plugin-svelte and svelte-check are single-file / type-only. fallow builds a project-wide listened-event set from every component-tag `on:<name>` binding (event forwarding, an `on:<name>` with no handler, counts as a listen), then flags a dispatched event whose name is in no listened set. Conservative (zero false positives): the whole component abstains on a dynamic `dispatch(<nonLiteral>)` (the event name is unknowable) or a `dispatch` reference forwarded as a value; a DOM `on:click` on a lowercase element is NOT a custom event and is ignored; and any listener on any component anywhere credits the name (the liberal over-credit, false-negative-safe direction). Default warn; remove the dispatched event or wire a listener. The check runs only when the project declares `svelte`.",
        docs_path: "explanations/dead-code#unused-svelte-events",
    },
    RuleDef {
        id: "fallow/unused-effect-layer",
        category: "Dead code",
        name: "Unused Effect layers",
        short: "An Effect layer is composed only into layers that nothing uses",
        full: "An Effect `Layer` definition (a module-scope const or a class static property such as `static layer = Layer.effect(this, make)`) whose every reference sits inside other layer definitions that are themselves unused. The layer never reaches `Effect.provide`, `ManagedRuntime.make`, `Layer.launch` or any other runtime use. fallow builds a layer graph from `Layer.*` initializers and `.pipe(Layer.*)` chains. Any value reference outside a layer definition is a root, including references in functions and tests. A layer with zero references is not reported here: `unused-export` or `unused-class-member` reports it, so the rule reports the tail of a dead composition and names the unused consumers. Default warn. The check runs only when the project declares `effect`.",
        docs_path: "explanations/dead-code#unused-effect-layers",
    },
    RuleDef {
        id: "fallow/unused-tagged-error",
        category: "Dead code",
        name: "Unused tagged errors",
        short: "An Effect tagged error class is referenced only as a type or a catchTag string, and no code constructs or fails with it",
        full: "A class that extends `Data.TaggedError(\"X\")` or `Schema.TaggedError<Self>()(\"X\", ...)` that type positions or `catchTag(\"X\")` handlers reference, but that no code reads as a value. Any value reference counts as a producer: `new X()`, `X.make`, `Schema.Union(X)`, an API or RPC error declaration that decodes the error at a boundary, or passing the class to a function. Only type positions and catch-tag strings do not count. A class with zero references is left to `unused-export`. The finding lists the `catchTag` handlers of the tag. Default warn. The check runs only when the project declares `effect`.",
        docs_path: "explanations/dead-code#unused-tagged-errors",
    },
    RuleDef {
        id: "fallow/unproduced-catch-tag",
        category: "Dead code",
        name: "Unproduced catch tags",
        short: "A catchTag handler names a project error tag that no code produces",
        full: "A `catchTag(\"X\", ...)` or `catchTags({ X: ... })` handler where every project class with tag `X` is never constructed or failed with, and no object literal `_tag: \"X\"` or tagged constructor produces the tag. A tag that no project class declares can come from a package, so the rule abstains. Effect built-in tags such as `TimeoutException` are never reported. The rule reports a handler only when no `unused-tagged-error` finding covers its classes: the error rule is off, or its finding is suppressed. Default warn. The check runs only when the project declares `effect`.",
        docs_path: "explanations/dead-code#unproduced-catch-tags",
    },
    RuleDef {
        id: "fallow/unused-server-action",
        category: "Dead code",
        name: "Unused server actions",
        short: "A Next.js Server Action exported from a \"use server\" file is referenced by no code in the project",
        full: "A Next.js Server Action (an export of a `\"use server\"` file) that no code in the project references: no import-and-call, no `action={fn}` JSX binding, no `<form action={fn}>`. This is the cross-graph \"declared but zero consumers\" direction, reclassified out of `unused-export` for `\"use server\"` files so the finding carries the action-specific signal. eslint-plugin-next is single-file and cannot see cross-file usage. It does NOT mean the endpoint is unreachable: Next.js still registers a generated action id, so it stays POST-able; it means no project code references it (likely forgotten or dead, and a candidate for removal to shrink surface area). Default warn; wire the action to a consumer or remove it. The check runs only when the project declares `next`.",
        docs_path: "explanations/dead-code#unused-server-actions",
    },
    RuleDef {
        id: "fallow/unused-load-data-key",
        category: "Dead code",
        name: "Unused load data keys",
        short: "A SvelteKit load() return-object key is read by no consumer",
        full: "A SvelteKit route `load()` (in `+page.ts` / `+page.server.ts` and the `.js` variants) returns an object whose keys become the route's `data` prop. A returned key that NO consumer reads is dead: it runs a real server-side fetch / DB cost on every request for data nothing renders. fallow checks two channels: the sibling `+page.svelte`'s `data.<key>` reads (route-pinned), and project-wide `page.data.<key>` (Svelte 5 `$app/state`) / `$page.data.<key>` (Svelte 4 `$app/stores`) reads in any component. `svelte-check` types `data` via generated `$types` but never flags an unread RETURNED key. The detector abstains (never false-flags) on a spread / non-literal / multi-return / computed-key / wrapped `load`, on a sibling that passes the whole `data` object opaquely, on a `+page.server.ts` whose universal `+page.ts` sibling forwards its `data`, and project-wide when any whole-object use of `page.data` / `$page.data` is seen. Default warn; delete the key or wire a consumer. A load fetch can have side effects, so there is no safe auto-fix. The check runs only when the project declares `@sveltejs/kit`.",
        docs_path: "explanations/dead-code#unused-load-data-keys",
    },
    RuleDef {
        id: "fallow/prop-drilling",
        category: "Dead code",
        name: "Prop drilling",
        short: "A React/Preact prop is forwarded unchanged through 3+ pass-through components to a distant consumer",
        full: "A React/Preact prop is received by a component, forwarded UNCHANGED to a child, and forwarded again through two or more intermediate \"pass-through\" components until a component that substantively uses it. The high-confidence signal is that the received identifier appears ONLY as the root of forwarded child-JSX attribute values (so `<Child userName={user.name}/>` counts: the prop `user` is projected forward), not the attribute name matching. fallow emits located per-chain records (the source, each pass-through hop, and the consumer with file + line + component name) so CI and an agent can act: colocate the consumer with the data, lift the value to a React context/provider at a mid-chain hop, or compose the component so the intermediates no longer thread the prop. This is a graph-derived health signal, not a correctness error. The rule defaults to OFF (opt-in), like private-type-leak and the security rules: enable it with `prop-drilling: \"warn\"` in `rules`. Set `prop-drilling: \"error\"` to enforce remaining chains in CI. Warnings remain advisory even with strict flags. File overrides resolve at the source hop and can activate the rule when the base setting is off. Suppressed or baselined chains do not gate. Zero false positives by construction: any `{...props}` spread, `cloneElement`, element-as-prop / render-prop / children-as-function, or context `*.Provider` anywhere in the chain abstains the whole chain, as does an ambiguous or unresolvable hop. The check runs only when the project declares `react` / `react-dom` / `next` / `preact`.",
        docs_path: "explanations/dead-code#prop-drilling",
    },
    RuleDef {
        id: "fallow/thin-wrapper",
        category: "Dead code",
        name: "Thin wrapper",
        short: "A React/Preact component whose whole body is a single spread-forwarded child render (a candidate for inlining)",
        full: "A React/Preact component whose ENTIRE body is structural indirection: it returns exactly one capitalized component element that forwards the component's own props via a bare spread (`return <Child {...props}/>`), with no host-element wrapper, no extra children, no named attributes alongside the spread, no hooks, no branching, and no other statements. Such a component adds nothing of its own: it is a CANDIDATE for inlining at its call sites or deleting, not a correctness error. fallow emits a located per-wrapper record (file + line + the wrapper and child component names) so CI and an agent can act. The rule defaults to OFF (opt-in), like prop-drilling and the security rules: enable it with `thin-wrapper: \"warn\"` in `rules`. Zero false positives by construction: a `forwardRef` / `memo` wrapper (the sanctioned way to make a child ref-able or set a perf boundary), an EXPORTED component (a public-API re-brand / encapsulation), a context `*.Provider` wrapper, a `cloneElement` / render-prop forward, a wrapper that passes ANY named attribute alongside the spread (a fixed configuration), a self-render, or an unresolvable / member-expression child all abstain. A TypeScript-only type-narrowing wrapper (`const StrictButton = (p: StrictProps) => <Button {...p}/>`) is a known limitation under ADR-001's syntactic analysis; suppress it with the inline comment. The check runs only when the project declares `react` / `react-dom` / `next` / `preact`.",
        docs_path: "explanations/dead-code#thin-wrapper",
    },
    RuleDef {
        id: "fallow/duplicate-prop-shape",
        category: "Dead code",
        name: "Duplicate prop shape",
        short: "Three or more React/Preact components across two or more files declare an identical prop-name set (a missing shared Props type)",
        full: "Three or more distinct React/Preact components, living in two or more files, whose statically-harvested prop NAME set is byte-for-byte IDENTICAL after (a) excluding a fixed denylist of ubiquitous DOM / render-passthrough prop names (className, style, id, children, key, ref, the common event handlers, plus data-* / aria-* by prefix) and (b) requiring the REMAINING significant set to have four or more members. Identity is over NAMES only, never types (ADR-001 cannot resolve types). This is a structural-refactor health signal: the recurring shape is a missing shared abstraction, so extract one shared `Props` type (or a base component) that every member reuses. It is never a correctness error and never an auto-fix. fallow emits one located record per participating component, each naming the shared `shape`, the `group_size`, and the OTHER members in `sharing_components`. The rule defaults to OFF (opt-in), like thin-wrapper and the security rules: enable it with `duplicate-prop-shape: \"warn\"` in `rules`. Anti-noise gates (defended as rule-of-three plus a denylist-survivor floor, not tuned magic): the four-significant-prop floor turns `{label, onClick}` buttons into non-findings; the three-component floor is the rule-of-three abstraction trigger; the two-file floor keeps a local same-shaped variant pair (a render-prop pair, a Foo/FooImpl split) unflagged. A component whose props are not fully harvestable (a rest/spread signature, a forwardRef/memo over an imported interface) ABSTAINS, because a partial prop set can never be proven identical. Exact full-set identity ONLY: a superset / subset relationship does NOT group, so a four-prop group and a five-prop superset form TWO findings (the price of zero invalid groups: the finding always fits one extracted shared type). The check runs only when the project declares `react` / `react-dom` / `next` / `preact`.",
        docs_path: "explanations/dead-code#duplicate-prop-shape",
    },
    RuleDef {
        id: "fallow/route-collision",
        category: "Policy",
        name: "Route collision",
        short: "Two or more Next.js App Router route files resolve to the same URL",
        full: "Two or more App Router route files (a `page` or a `route` handler) resolve to the SAME URL within one app-root. Route groups `(name)` and parallel slots `@name` do not change the URL, so `app/(marketing)/about/page.tsx` and `app/(shop)/about/page.tsx` both own `/about`. Next.js fails the build (\"You cannot have two parallel pages that resolve to the same path\") because a URL can have at most one owner, whether a Page or a Route Handler. fallow surfaces every colliding file at once; the build error names only one. Buckets are scoped per app-root (per workspace package), so a monorepo with several independent Next apps sharing a path is not flagged. Files under a private `_folder` or an intercepting marker `(.)`/`(..)`/`(...)` are excluded. There is no safe auto-fix: move or merge one of the files so each URL has a single owner. The check runs only when the project declares `next`.",
        docs_path: "explanations/dead-code#route-collisions",
    },
    RuleDef {
        id: "fallow/dynamic-segment-name-conflict",
        category: "Policy",
        name: "Dynamic segment name conflict",
        short: "Sibling Next.js dynamic route segments use different slug names at the same position",
        full: "Two or more sibling dynamic route segments at the same App Router tree position use different param spellings (`[id]` vs `[slug]`, or a catch-all `[...x]` vs an optional catch-all `[[...x]]`). Next.js throws \"You cannot use different slug names for the same dynamic path\" at dev and production runtime when the position is hit, because one position must resolve to a single param name. `next build` does NOT catch this (the build succeeds), so CI passes while the route crashes on its first request; fallow's static catch closes that gap. Route groups are transparent to the position and parallel slots fork it, so only genuinely-sibling segments conflict. To fix: rename the dynamic segments at the position to one consistent slug name. The check runs only when the project declares `next`.",
        docs_path: "explanations/dead-code#dynamic-segment-name-conflicts",
    },
];

/// Look up a rule definition by its SARIF rule ID across all rule sets.
#[must_use]
pub fn rule_by_id(id: &str) -> Option<&'static RuleDef> {
    CHECK_RULES
        .iter()
        .chain(HEALTH_RULES.iter())
        .chain(DUPES_RULES.iter())
        .chain(FLAGS_RULES.iter())
        .chain(SECURITY_RULES.iter())
        .chain(AUDIT_RULES.iter())
        .find(|r| r.id == id)
}

/// Every registered rule in registry order: dead-code, health, duplication,
/// flags, security, then audit. The order is the `fallow schema`
/// `issue_types` order and the `fallow://issue-types` resource order.
pub fn all_rules() -> impl Iterator<Item = &'static RuleDef> {
    CHECK_RULES
        .iter()
        .chain(HEALTH_RULES.iter())
        .chain(DUPES_RULES.iter())
        .chain(FLAGS_RULES.iter())
        .chain(SECURITY_RULES.iter())
        .chain(AUDIT_RULES.iter())
}

/// The command family a rule reports under (`dead-code`, `health`, `dupes`,
/// `flags`, `security`, or `audit`), derived from the registry array that owns it.
/// Membership is by id (unique across registries); the registries are
/// `const` slices, so pointer identity is not stable across uses.
#[must_use]
pub fn rule_command(rule: &RuleDef) -> &'static str {
    let owns = |rules: &[RuleDef]| rules.iter().any(|candidate| candidate.id == rule.id);
    if owns(HEALTH_RULES) {
        "health"
    } else if owns(DUPES_RULES) {
        "dupes"
    } else if owns(FLAGS_RULES) {
        "flags"
    } else if owns(SECURITY_RULES) {
        "security"
    } else if owns(AUDIT_RULES) {
        "audit"
    } else {
        "dead-code"
    }
}

/// The rule id without its `fallow/` or `security/` namespace prefix, which
/// is the canonical issue code (`unused-export`, `sql-injection`).
#[must_use]
pub fn bare_rule_id(rule: &RuleDef) -> &'static str {
    rule.id.split_once('/').map_or(rule.id, |(_, bare)| bare)
}

/// The `rules.*` config key whose default severity gates findings of `rule`:
/// the rule's own `config_key` for a 1:1 rule, else the key of the shared rule
/// its suppression token names (every tainted-sink catalogue category is gated
/// by `security-sink`, coverage findings by `coverage-gaps`). `None` for
/// findings with no `rules.*` gate at all (complexity and duplication
/// metrics, refactoring targets, runtime-coverage verdicts); a suppression
/// token that is not a `rules.*` key (`code-duplication`, `feature-flag`)
/// yields `None` too, so every returned key resolves in
/// [`crate::schemas::default_rule_severities`].
///
/// Shared by `fallow schema` and the `fallow://issue-types` MCP resource so
/// the two never disagree on which default severity a finding inherits.
#[must_use]
pub fn rule_severity_key(rule: &RuleDef) -> Option<&'static str> {
    let bare = bare_rule_id(rule);
    let candidate = fallow_types::issue_meta::issue_meta_by_code(bare)
        .and_then(|meta| meta.config_key.or(meta.suppress_token))
        .or_else(|| match rule_command(rule) {
            "security" => Some(if bare == "client-server-leak" {
                "security-client-server-leak"
            } else {
                "security-sink"
            }),
            "health" if matches!(bare, "untested-file" | "untested-export") => {
                Some("coverage-gaps")
            }
            _ => None,
        })?;
    crate::schemas::is_rule_severity_key(candidate).then_some(candidate)
}

/// Build the docs URL for a rule.
#[must_use]
pub fn rule_docs_url(rule: &RuleDef) -> String {
    let docs_path = rule_result_meta(rule).map_or(rule.docs_path, |meta| meta.meta_docs_path);
    fallow_output::rule_docs_url(docs_path)
}

fn rule_result_meta(rule: &RuleDef) -> Option<&'static fallow_types::issue_meta::IssueResultMeta> {
    let code = rule.id.strip_prefix("fallow/")?;
    fallow_types::issue_meta::issue_result_meta_by_code(code)
}

fn rule_explain_name(rule: &RuleDef) -> &'static str {
    rule_result_meta(rule).map_or(rule.name, |meta| meta.meta_name)
}

fn rule_explain_summary(rule: &RuleDef) -> &'static str {
    rule_result_meta(rule).map_or(rule.short, |meta| meta.sarif_description)
}

/// Extra educational content for the standalone `fallow explain <issue-type>`
/// command. Kept separate from [`RuleDef`] so SARIF and `_meta` payloads remain
/// compact while terminal users and agents can ask for worked examples on
/// demand.
pub struct RuleGuide {
    /// Worked code example illustrating the issue.
    pub example: &'static str,
    /// Step-by-step remediation guidance.
    pub how_to_fix: &'static str,
}

/// Look up an issue type from a user-facing token.
///
/// Accepts canonical SARIF ids (`fallow/unused-export`), issue tokens
/// (`unused-export`), and common CLI filter spellings (`unused-exports`).
#[must_use]
pub fn rule_by_token(token: &str) -> Option<&'static RuleDef> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(rule) = rule_by_id(trimmed) {
        return Some(rule);
    }
    let normalized = trimmed
        .strip_prefix("fallow/")
        .unwrap_or(trimmed)
        .trim_start_matches("--")
        .replace('_', "-")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-");
    if let Some(rule) = dead_code_registry_rule(&normalized) {
        return Some(rule);
    }
    if let Some(rule) = rule_by_id(&format!("fallow/{normalized}")) {
        return Some(rule);
    }
    let alias = health_alias_id(&normalized).or_else(|| security_alias_id(&normalized));
    if let Some(id) = alias
        && let Some(rule) = rule_by_id(id)
    {
        return Some(rule);
    }
    let security_token = normalized.strip_prefix("security-").unwrap_or(&normalized);
    let security_id = format!("security/{security_token}");
    if let Some(rule) = rule_by_id(&security_id) {
        return Some(rule);
    }
    let singular = normalized
        .strip_suffix('s')
        .filter(|_| normalized != "unused-class")
        .unwrap_or(&normalized);
    let singular_security_token = singular.strip_prefix("security-").unwrap_or(singular);
    let singular_security_id = format!("security/{singular_security_token}");
    if let Some(rule) = rule_by_id(&singular_security_id) {
        return Some(rule);
    }
    let id = format!("fallow/{singular}");
    rule_by_id(&id).or_else(|| {
        CHECK_RULES
            .iter()
            .chain(HEALTH_RULES.iter())
            .chain(DUPES_RULES.iter())
            .chain(FLAGS_RULES.iter())
            .chain(SECURITY_RULES.iter())
            .chain(AUDIT_RULES.iter())
            .find(|rule| {
                rule.docs_path.ends_with(&normalized)
                    || rule.docs_path.ends_with(singular)
                    || rule_result_meta(rule).is_some_and(|meta| {
                        meta.meta_docs_path.ends_with(&normalized)
                            || meta.meta_docs_path.ends_with(singular)
                            || meta.meta_name.eq_ignore_ascii_case(trimmed)
                    })
                    || rule.name.eq_ignore_ascii_case(trimmed)
            })
    })
}

fn dead_code_registry_rule(normalized: &str) -> Option<&'static RuleDef> {
    let meta = fallow_types::issue_meta::issue_meta_for_contract_token(normalized)?;
    CHECK_RULES
        .iter()
        .find(|rule| rule.id.strip_prefix("fallow/") == Some(meta.code))
}

fn health_alias_id(normalized: &str) -> Option<&'static str> {
    match normalized {
        "complexity" | "high-complexity" => Some("fallow/high-complexity"),
        "cyclomatic" | "high-cyclomatic" | "high-cyclomatic-complexity" => {
            Some("fallow/high-cyclomatic-complexity")
        }
        "cognitive" | "high-cognitive" | "high-cognitive-complexity" => {
            Some("fallow/high-cognitive-complexity")
        }
        "crap" | "high-crap" | "high-crap-score" => Some("fallow/high-crap-score"),
        "long-file" | "file-lines" | "max-file-lines" | "complexity-file-lines" => {
            Some("fallow/long-file")
        }
        "duplication" | "dupes" | "code-duplication" => Some("fallow/code-duplication"),
        "feature-flag" | "feature-flags" | "flags" => Some("fallow/feature-flag"),
        "flag-retirement" | "flag-retirement-candidate" | "retirement" => {
            Some("fallow/flag-retirement-candidate")
        }
        _ => None,
    }
}

fn security_alias_id(normalized: &str) -> Option<&'static str> {
    match normalized {
        "security"
        | "security-candidate"
        | "security-candidates"
        | "tainted-sink"
        | "tainted-sinks"
        | "security-sink"
        | "security-sinks" => Some("security/tainted-sink"),
        "client-server-leak"
        | "client-server-leaks"
        | "security-client-server-leak"
        | "security-client-server-leaks" => Some("security/client-server-leak"),
        "hardcoded-secret" | "hardcoded-secrets" | "hard-coded-secret" | "hard-coded-secrets" => {
            Some("security/hardcoded-secret")
        }
        _ => None,
    }
}

/// Return worked-example and fix guidance for a rule.
#[must_use]
pub fn rule_guide(rule: &RuleDef) -> RuleGuide {
    source_dead_code_rule_guide(rule.id)
        .or_else(|| member_import_rule_guide(rule.id))
        .or_else(|| architecture_rule_guide(rule.id))
        .or_else(|| catalog_rule_guide(rule.id))
        .or_else(|| health_runtime_rule_guide(rule.id))
        .or_else(|| duplication_rule_guide(rule.id))
        .or_else(|| security_rule_guide(rule.id))
        .or_else(|| audit_rule_guide(rule.id))
        .unwrap_or_else(fallback_rule_guide)
}

fn audit_rule_guide(id: &str) -> Option<RuleGuide> {
    Some(match id {
        "fallow/weakened-tests" => RuleGuide {
            example: "A change turns `it('charges tax', ...)` into `it.skip('charges tax', ...)`, or lowers `coverage.thresholds.lines` from 90 to 70.",
            how_to_fix: "Decide if the weaker test or threshold is intended. If it is, suppress the finding with `// fallow-ignore-next-line weakened-tests` (or `fallow-ignore-file` for a file-level kind), or set the rule to `off` for the path in `overrides`.",
        },
        "fallow/added-suppression" => RuleGuide {
            example: "A change adds `// @ts-expect-error` above a call instead of a correct type.",
            how_to_fix: "Decide if the suppression is necessary. If it is, keep it and suppress the finding with `// fallow-ignore-next-line added-suppression`, or turn the rule off for the path.",
        },
        "fallow/test-only-branch" => RuleGuide {
            example: "A change adds `if (process.env.NODE_ENV === 'test') return cached;` to `src/pricing.ts`.",
            how_to_fix: "Prefer a seam that tests can inject, such as a parameter or a module mock. If the branch is intended, suppress it with `// fallow-ignore-next-line test-only-branch`.",
        },
        "fallow/diverged-clone-sibling" => RuleGuide {
            example: "`src/a/total.ts` and `src/b/total.ts` held the same function. The change fixes a bug in the first copy only.",
            how_to_fix: "Check the unchanged copies in `related`. Apply the same change there, or extract a shared function. If the copies must differ, suppress with `// fallow-ignore-next-line diverged-clone-sibling`.",
        },
        "fallow/added-dependency" => RuleGuide {
            example: "A change adds `\"left-pad\": \"^1.3.0\"` to `dependencies`.",
            how_to_fix: "Check that the package exists, is maintained, and is not already covered by a dependency of the project. Suppress through `overrides` because `package.json` cannot hold a comment.",
        },
        _ => return None,
    })
}

fn source_dead_code_rule_guide(id: &str) -> Option<RuleGuide> {
    Some(match id {
        "fallow/unused-file" => RuleGuide {
            example: "src/old-widget.ts is not imported by any entry point, route, script, or config file.",
            how_to_fix: "Delete the file if it is genuinely dead. If a framework loads it implicitly, add the right plugin/config pattern or mark it in alwaysUsed.",
        },
        "fallow/unused-export" => RuleGuide {
            example: "export const formatPrice = ... exists in src/money.ts, but no module imports formatPrice.",
            how_to_fix: "Remove the export or make it file-local. If it is public API, import it from an entry point or add an intentional suppression with context.",
        },
        "fallow/unused-type" => RuleGuide {
            example: "export interface LegacyProps is exported, but no module imports the type.",
            how_to_fix: "Remove the type export, inline it, or keep it behind an explicit API entry point when consumers rely on it.",
        },
        "fallow/private-type-leak" => RuleGuide {
            example: "export function makeUser(): InternalUser exposes InternalUser even though InternalUser is not exported.",
            how_to_fix: "Export the referenced type, change the public signature to an exported type, or keep the helper private.",
        },
        "fallow/deprecated-export-in-use" => RuleGuide {
            example: "/** @deprecated Use formatMoney. */ export function formatPrice() is still imported by src/cart.ts and src/checkout.ts.",
            how_to_fix: "Move each consumer to the replacement the message names, then remove the export. For the full consumer list, run `fallow dead-code --trace <file>:<export>`. Enable the rule with `deprecated-exports-in-use: \"warn\"` in `rules`, or pass `--deprecated-exports-in-use` for one run.",
        },
        "fallow/unused-dependency"
        | "fallow/unused-dev-dependency"
        | "fallow/unused-optional-dependency" => RuleGuide {
            example: "package.json lists left-pad, but no source, script, config, or plugin-recognized file imports it.",
            how_to_fix: "Remove the dependency after checking runtime/plugin usage. If another workspace uses it, move the dependency to that workspace.",
        },
        "fallow/type-only-dependency" => RuleGuide {
            example: "zod is in dependencies but only appears in import type declarations.",
            how_to_fix: "Move the package to devDependencies unless runtime code imports it as a value.",
        },
        "fallow/test-only-dependency" => RuleGuide {
            example: "vitest is listed in dependencies, but only test files import it.",
            how_to_fix: "Move the package to devDependencies unless production code imports it at runtime.",
        },
        "fallow/dev-dependency-in-production" => RuleGuide {
            example: "yaml is in devDependencies, but src/config.ts imports { parse } from 'yaml' and runs it at runtime.",
            how_to_fix: "Move the package to dependencies when the deployment installs them, so a production-only install keeps it. Leave it in devDependencies if the build inlines it into the output, if the only production imports are `import type`, or if another manifest section (dependencies / peer / optional) already provides it.",
        },
        _ => return None,
    })
}

fn member_import_rule_guide(id: &str) -> Option<RuleGuide> {
    Some(match id {
        "fallow/unused-enum-member" => RuleGuide {
            example: "Status.Legacy remains in an exported enum, but no code reads that member.",
            how_to_fix: "Remove the member after checking serialized/API compatibility, or suppress it with a reason when external data still uses it.",
        },
        "fallow/unused-class-member" => RuleGuide {
            example: "class Parser has a public parseLegacy method that is never called in the project.",
            how_to_fix: "Remove or privatize the member. For reflection/framework lifecycle hooks, configure or suppress the intentional entry point.",
        },
        "fallow/unused-store-member" => RuleGuide {
            example: "useCartStore declares a discountTotal getter that no component, composable, or other store ever reads.",
            how_to_fix: "Remove the unused state property, getter, or action. If it is consumed reflectively (a Pinia plugin, $onAction, or dynamic dispatch), suppress the line with // fallow-ignore-next-line unused-store-member.",
        },
        "fallow/unprovided-inject" => RuleGuide {
            example: "A component calls inject(ThemeKey) (Vue) or getContext(ThemeKey) (Svelte) with an imported symbol key, but no provide(ThemeKey) / setContext(ThemeKey) exists anywhere in the project.",
            how_to_fix: "Add a matching provide() / setContext() for the key, or remove the dead inject() / getContext(). If a provider lives outside the analyzed graph (an app-level provide registered elsewhere, a plugin, a host app), suppress the line with // fallow-ignore-next-line unprovided-inject.",
        },
        "fallow/unrendered-component" => RuleGuide {
            example: "components/Orphan.vue is re-exported from a barrel (export { default as Orphan } from './Orphan.vue') but no template, registration, h() call, or dynamic import ever renders it.",
            how_to_fix: "Render the component where it belongs, or delete it and remove the dead barrel re-export. If it is rendered reflectively (a dynamic <component :is> from a non-literal value), suppress the line with // fallow-ignore-next-line unrendered-component.",
        },
        "fallow/absent-component-prop" => RuleGuide {
            example: "Card declares highlight?: boolean with a false default, reads highlight in its render body, and every inspected reachable caller renders <Card /> without highlight.",
            how_to_fix: "Review inspected callers, defaults and API intent. Keep intentional stable props, update the component manually, or add a declaration-line fallow-ignore-next-line absent-component-prop comment. This candidate has no automatic fix.",
        },
        "fallow/unused-component-prop" => RuleGuide {
            example: "Widget.vue declares defineProps<{ size: string }>(), or a React Widget({ size }) destructures `size`, but `size` is referenced nowhere in the component (Vue: its script or template; React: its function body or JSX).",
            how_to_fix: "Remove the unused prop, or reference it in the component (Vue: the script / template; React: the function body or JSX). If the prop is part of a deliberately-stable public component API, suppress the line with // fallow-ignore-next-line unused-component-prop.",
        },
        "fallow/unused-component-emit" => RuleGuide {
            example: "Widget.vue declares defineEmits<{ close: [] }>() but `emit('close')` is called nowhere in the component's script.",
            how_to_fix: "Remove the unused emit, or emit it in the script. If the emit is part of a deliberately-stable public component API, suppress the line with // fallow-ignore-next-line unused-component-emit.",
        },
        "fallow/unused-component-input" => RuleGuide {
            example: "user-card.component.ts declares @Input() size: string (or size = input<string>()) but `size` is read nowhere in the template or the class body.",
            how_to_fix: "Remove the unused input, or read it in the template or class body. If the input is part of a deliberately-stable public component API, suppress the line with // fallow-ignore-next-line unused-component-input.",
        },
        "fallow/unused-component-output" => RuleGuide {
            example: "user-card.component.ts declares @Output() close = new EventEmitter<void>() (or close = output<void>()) but `this.close.emit(...)` is called nowhere in the class.",
            how_to_fix: "Remove the unused output, or emit it from the class. If the output is part of a deliberately-stable public component API, suppress the line with // fallow-ignore-next-line unused-component-output.",
        },
        "fallow/unused-svelte-event" => RuleGuide {
            example: "Child.svelte calls const dispatch = createEventDispatcher(); dispatch('dead'), but no parent listens for it (no <Child on:dead> anywhere in the project).",
            how_to_fix: "Remove the dispatched event, or listen for it on the component (<Child on:dead={...}> or forward it via <Child on:dead>). If the event is dispatched reflectively (a dynamic name) or is part of a deliberately-stable public component API, suppress the line with // fallow-ignore-next-line unused-svelte-event.",
        },
        "fallow/unused-effect-layer" => RuleGuide {
            example: "services.ts defines const CacheLive = Cache.layer.pipe(Layer.provide(Db.layer)) and export const OrphanLive = Layer.mergeAll(CacheLive), but nothing provides OrphanLive.",
            how_to_fix: "Remove the unused consumer layers first, then this layer, or compose the layer into a layer that reaches a runtime. If code outside the analyzed project composes it, suppress the line with // fallow-ignore-next-line unused-effect-layer.",
        },
        "fallow/unused-tagged-error" => RuleGuide {
            example: "errors.ts declares class Expired extends Data.TaggedError(\"Expired\"), signatures list it in the error channel and a handler calls catchTag(\"Expired\"), but no code constructs Expired or fails with it.",
            how_to_fix: "Fail with the error where the condition occurs, or remove the class, its type references and its catchTag handlers. If consumers outside the project construct or decode it, suppress the line with // fallow-ignore-next-line unused-tagged-error.",
        },
        "fallow/unproduced-catch-tag" => RuleGuide {
            example: "main.ts calls Effect.catchTag(\"Expired\", ...), the only class with tag Expired is never constructed, and the unused-tagged-error finding for it is suppressed.",
            how_to_fix: "Remove the handler, or fail with the error where the condition occurs. If a package or code outside the project produces the tag, suppress the line with // fallow-ignore-next-line unproduced-catch-tag.",
        },
        "fallow/unused-server-action" => RuleGuide {
            example: "app/actions.ts has \"use server\" and exports submitForm, but no component imports it, binds it via action={submitForm}, or uses it in <form action={submitForm}>.",
            how_to_fix: "Wire the action to a consumer (an import-and-call, an action={fn} binding, or a <form action={fn}>), or remove it. If it is invoked reflectively (an action registry dispatching by id, or a non-JS caller), suppress the line with // fallow-ignore-next-line unused-server-action.",
        },
        "fallow/unused-load-data-key" => RuleGuide {
            example: "src/routes/blog/+page.ts returns { posts, draftCount } but +page.svelte only reads data.posts and no component reads page.data.draftCount.",
            how_to_fix: "Delete the unused key from the load() return (and skip its fetch), or wire a consumer (read data.<key> in +page.svelte, or page.data.<key> in a shared component). If the load fetch has a side effect you must keep, suppress the line with // fallow-ignore-next-line unused-load-data-key.",
        },
        "fallow/prop-drilling" => RuleGuide {
            example: "Page receives `user` and renders <Layout user={user}/>; Layout only re-passes it to <Sidebar user={user}/>; Sidebar only re-passes it to <Profile user={user}/>, which finally reads user.name. The prop is drilled through Layout and Sidebar untouched.",
            how_to_fix: "Collapse the chain: colocate the consumer with the data, lift the value into a React context/provider at a mid-chain hop and consume it there, or compose the component (pass the rendered child as children) so the intermediates no longer thread the prop. Enable the rule with rules.prop-drilling = \"warn\" (it defaults to off). To accept one chain, suppress the source prop with // fallow-ignore-next-line prop-drilling.",
        },
        "fallow/thin-wrapper" => RuleGuide {
            example: "const ButtonWrapper = (props) => <Button {...props}/>; the wrapper has no own markup, hooks, or logic, so it only re-points at Button.",
            how_to_fix: "Inline the wrapper at its call sites (use <Button .../> directly) or delete it. Keep it only if it is a deliberate seam (a planned divergence point, a public-API re-brand): an exported wrapper already abstains. Enable the rule with rules.thin-wrapper = \"warn\" (it defaults to off). To accept one wrapper, suppress it with // fallow-ignore-next-line thin-wrapper above the component definition.",
        },
        "fallow/duplicate-prop-shape" => RuleGuide {
            example: "FieldText, FieldNumber, and FieldSelect (across three files) each declare exactly { name, label, value, onChange, error }. The five significant prop names are identical, so they form one duplicate-prop-shape group.",
            how_to_fix: "Extract one shared Props type (e.g. type FieldProps = { name; label; value; onChange; error }) that every member reuses, or a base component the variants compose. Keep them separate only if a per-variant prop divergence is planned. Enable the rule with rules.duplicate-prop-shape = \"warn\" (it defaults to off). To accept one member, suppress it with // fallow-ignore-next-line duplicate-prop-shape above the component definition; the suppressed member still appears in its siblings' sharing_components because the group is real regardless of suppression.",
        },
        "fallow/unresolved-import" => RuleGuide {
            example: "src/app.ts imports ./routes/admin, but no matching file exists after extension and index resolution.",
            how_to_fix: "Fix the specifier, restore the missing file, install the package, or align tsconfig path aliases with the runtime resolver.",
        },
        "fallow/unlisted-dependency" => RuleGuide {
            example: "Root src/app.ts imports @acme/shared, an npm workspace package. The root package.json lists workspaces but does not list @acme/shared as a dependency, so the import is unlisted even when npm links it into node_modules.",
            how_to_fix: "Declare the imported package in the owning package.json, including the root manifest for root code. Use dependencies for runtime needs or devDependencies for tooling/tests, with a version accepted by your package manager. Self-references need no declaration. For deliberately ambient packages, use ignoreDependencies as an explicit opt-out.",
        },
        "fallow/duplicate-export" => RuleGuide {
            example: "Button is exported from both src/ui/button.ts and src/components/button.ts.",
            how_to_fix: "Rename or consolidate the exports so consumers have one intentional import target.",
        },
        _ => return None,
    })
}

fn architecture_rule_guide(id: &str) -> Option<RuleGuide> {
    Some(match id {
        "fallow/circular-dependency" => RuleGuide {
            example: "src/a.ts imports src/b.ts, and src/b.ts imports src/a.ts.",
            how_to_fix: "Extract shared code to a third module, invert the dependency, or split initialization-time side effects from type-only contracts.",
        },
        "fallow/boundary-violation" => RuleGuide {
            example: "features/billing imports app/admin even though the configured boundary only allows imports from shared and entities.",
            how_to_fix: "Move the shared contract to an allowed zone, invert the dependency, or update the boundary config only if the architecture rule was wrong.",
        },
        "fallow/boundary-coverage" => RuleGuide {
            example: "src/generated/client.ts is reachable but does not match any boundaries.zones[].patterns entry.",
            how_to_fix: "Add the file to the intended zone pattern, move it under a zoned directory, or add a generated-file glob to boundaries.coverage.allowUnmatched.",
        },
        "fallow/boundary-call-violation" => RuleGuide {
            example: "src/domain/policy.ts calls execSync from node:child_process while boundaries.calls.forbidden bans child_process.* from the domain zone.",
            how_to_fix: "Move the call into a zone that may perform the effect, route it through an allowed abstraction, or narrow the forbidden pattern if the rule was wrong. To suppress, use the boundary family token: `// fallow-ignore-next-line boundary-violation` governs import, coverage, and call findings alike (the rule-id-shaped `boundary-call-violation` is accepted as an alias).",
        },
        "fallow/policy-violation" => RuleGuide {
            example: "src/app.ts imports moment while a rule pack bans the moment specifier with the message 'Use date-fns.'",
            how_to_fix: "Follow the rule's message to replace banned usage or remove a banned export. For a GDP proof producer, move proof factory creation into an allowed file, or correct the rule's allowedFiles and optional proofKinds if the ownership policy was wrong. To waive one rule, use `// fallow-ignore-next-line policy-violation:<pack>/<rule-id>` or the file-level form. Use bare `policy-violation` only when you intend to suppress every rule-pack finding at that scope.",
        },
        "fallow/stale-suppression" => RuleGuide {
            example: "// fallow-ignore-next-line unused-export remains above an export that is now used.",
            how_to_fix: "Remove the suppression. If a different issue is still intentional, replace it with a current, specific suppression.",
        },
        "fallow/missing-suppression-reason" => RuleGuide {
            example: "// fallow-ignore-next-line unused-export appears without the required explanatory reason.",
            how_to_fix: "Add a concise reason after the suppression token, or remove the suppression if the issue is no longer intentional.",
        },
        _ => return None,
    })
}

fn catalog_rule_guide(id: &str) -> Option<RuleGuide> {
    Some(match id {
        "fallow/unused-catalog-entry" => RuleGuide {
            example: "The catalog source declares `catalog: { is-even: ^1.0.0 }`, but no workspace package.json declares `\"is-even\": \"catalog:\"`.",
            how_to_fix: "Delete the entry from the catalog source file. If any consumer uses a hardcoded version (surfaced in `hardcoded_consumers`), switch that consumer to `catalog:` first to keep versions aligned.",
        },
        "fallow/empty-catalog-group" => RuleGuide {
            example: "The catalog source declares `catalogs: { react17: {} }` after the last react17 entry was removed.",
            how_to_fix: "Delete the empty named group from the catalog source file. Comments between the deleted header and the next sibling can stay in place for manual review.",
        },
        "fallow/unresolved-catalog-reference" => RuleGuide {
            example: "packages/app/package.json declares `\"old-react\": \"catalog:react17\"`, but `catalogs.react17` in the catalog source does not declare `old-react`. The package manager install will fail.",
            how_to_fix: "If `available_in_catalogs` is non-empty, change the reference to one of those catalogs (e.g. `catalog:react18`). Otherwise add the package to the named catalog in the catalog source, or remove the catalog reference and pin a hardcoded version. For staged migrations where the catalog edit lands separately, add the (package, catalog, consumer) triple to `ignoreCatalogReferences` in your fallow config.",
        },
        "fallow/unused-dependency-override" => RuleGuide {
            example: "The root package.json declares `overrides: { axios: ^1.6.0 }`, but no workspace package.json declares `axios` and the active lockfile does not resolve it.",
            how_to_fix: "Delete the entry from its reported declaration source. If the finding is caused by a stale or missing readable lockfile, refresh the active lockfile and rerun fallow. If the override is intentionally retained, add it to `ignoreDependencyOverrides` in your fallow config.",
        },
        "fallow/misconfigured-dependency-override" => RuleGuide {
            example: "The root package.json declares `overrides: { \"axios\": \"\" }`. An empty override value is invalid and the active package manager will reject or ignore it.",
            how_to_fix: "Fix the key or value to match the active package manager's grammar. For pnpm this includes bare or scoped package names, version selectors, parent matchers, and the supported `-`, `$ref`, and `npm:alias` values. For npm or Bun, preserve the supported nested object shape. Remove experimental entries that should not remain.",
        },
        _ => return None,
    })
}

fn health_runtime_rule_guide(id: &str) -> Option<RuleGuide> {
    Some(match id {
        "fallow/high-cyclomatic-complexity"
        | "fallow/high-cognitive-complexity"
        | "fallow/high-complexity" => RuleGuide {
            example: "A function contains several nested conditionals, loops, and early exits, exceeding the configured complexity threshold. fallow also flags synthetic template-family findings on markup: Angular .html templates and inline `@Component({ template: ... })` literals, Vue SFC `<template>` blocks, Svelte markup, and Astro markup, plus each top-level Svelte `{#snippet name(...)}` block as its own `<snippet:NAME>` unit. `<component>` rollup findings combine the worst class method with its template.",
            how_to_fix: "For function findings, extract named helpers, split independent branches, flatten guard clauses, and add tests around the behavior before refactoring. For `<template>` findings, flatten nested control flow and move branch selection out of the markup: Angular `@if`/`@for`/`@switch`/`@defer` and legacy `*ngIf`/`*ngFor` into computed signals on the component class, Vue `v-if`/`v-else-if`/`v-for` into computed properties, Svelte `{#if}`/`{:else if}`/`{#each}`/`{#await}`/`{#key}` into derived state, Astro control flow into props resolved in the frontmatter. In Svelte, moving a repeated or deeply nested block into a top-level `{#snippet}` gives it its own `<snippet:NAME>` unit with nesting rebased to zero, so the in-file extraction moves the score; splitting a large template into child components lowers the score in every dialect. For `<component>` rollup findings, attack the larger half first; the per-half breakdown lives in `component_rollup`. To report these findings without a failed run, set the `complexity-cyclomatic` or `complexity-cognitive` rule to `warn`.",
        },
        "fallow/high-crap-score" => RuleGuide {
            example: "A complex function has little or no matching Istanbul coverage, so its CRAP score crosses the configured gate. Synthetic template-family units (the `<template>` units on Angular, Vue, Svelte and Astro markup, and Svelte `<snippet:NAME>` units) are not scored on this dimension: a template carries no direct coverage of its own, so fallow gates templates on the cyclomatic and cognitive dimensions instead.",
            how_to_fix: "Add focused tests for the risky branches first, then simplify the function if the score remains high. Template findings never carry this rule, so a `maxCrap` override scoped to a template unit buys nothing and can be removed. To report these findings without a failed run, set the `complexity-crap` rule to `warn`.",
        },
        "fallow/long-file" => RuleGuide {
            example: "A module grew to 900 physical lines while `health.maxFileLines` is 500. Blank lines and comment lines count; a final newline does not add a line.",
            how_to_fix: "Move cohesive groups of exports into their own modules, and remove unused code. In `fallow audit`, a long file that did not grow stays inherited, so you can enable the threshold on an existing project and stop further growth. To report these findings without a failed run, set the `complexity-file-lines` rule to `warn`. To raise the ceiling for generated files, use `thresholdOverrides[].maxFileLines`.",
        },
        "fallow/refactoring-target" => RuleGuide {
            example: "A file combines high complexity density, churn, fan-in, and dead-code signals.",
            how_to_fix: "Start with the listed evidence: remove dead exports, extract complex functions, then reduce fan-out or cycles in small steps.",
        },
        "fallow/untested-file" | "fallow/untested-export" => RuleGuide {
            example: "Production-reachable code has no dependency path from discovered test entry points.",
            how_to_fix: "Add or wire a test that imports the runtime path, or update entry-point/test discovery if the existing test is invisible to fallow.",
        },
        "fallow/runtime-safe-to-delete"
        | "fallow/runtime-review-required"
        | "fallow/runtime-low-traffic"
        | "fallow/runtime-coverage-unavailable"
        | "fallow/runtime-coverage" => RuleGuide {
            example: "Runtime coverage shows a function was never called, barely called, or could not be matched during the capture window.",
            how_to_fix: "Treat high-confidence cold static-dead code as delete candidates. For advisory or unavailable coverage, inspect seasonality, workers, source maps, and capture quality first.",
        },
        _ => return None,
    })
}

fn duplication_rule_guide(id: &str) -> Option<RuleGuide> {
    Some(match id {
        "fallow/code-duplication" => RuleGuide {
            example: "Two files contain the same normalized token sequence across a multi-line block.",
            how_to_fix: "Extract the shared logic when the duplicated behavior should evolve together. Leave it duplicated when the similarity is accidental and likely to diverge.",
        },
        _ => return None,
    })
}

fn security_rule_guide(id: &str) -> Option<RuleGuide> {
    Some(match id {
        "security/tainted-sink" => RuleGuide {
            example: "A non-literal request field reaches a catalogue sink such as security/sql-injection or security/dangerous-html. The finding is a candidate, not proof of exploitability.",
            how_to_fix: "Trace the source, sink, sanitization, and runtime context. Fix confirmed issues with parameterization, escaping, validation, or safer APIs, and suppress only reviewed false positives with context.",
        },
        "security/client-server-leak" => RuleGuide {
            example: "A module marked `use client` imports code that reads a non-public `process.env` or `import.meta.env` value through a static path.",
            how_to_fix: "Keep non-public env reads on the server side, move the value behind an API boundary, or rename only intentionally public values to the framework's public prefix.",
        },
        "security/hardcoded-secret" => RuleGuide {
            example: "A provider-prefixed token-shaped literal is assigned to a secret-shaped variable, and the hardcoded-secret category is explicitly included.",
            how_to_fix: "Rotate real credentials, move them to a secret manager or environment variable, and keep test-only literals clearly fake so they do not resemble provider tokens.",
        },
        id if id.starts_with("security/") => RuleGuide {
            example: "A `fallow security` candidate uses this catalogue category as its SARIF rule id, for example security/sql-injection for a matched SQL sink.",
            how_to_fix: "Review the candidate trace before acting. Confirm attacker control, missing sanitization, and reachable runtime context, then fix with the category-appropriate safer API or add a reviewed suppression.",
        },
        _ => return None,
    })
}

fn fallback_rule_guide() -> RuleGuide {
    RuleGuide {
        example: "Run the relevant command with --format json --quiet --explain to inspect this rule in context.",
        how_to_fix: "Use the issue action hints, source location, and docs URL to decide whether to remove, move, configure, or suppress the finding.",
    }
}

/// Build the typed standalone explain output for a user-facing issue token.
///
/// # Errors
///
/// Returns a structured programmatic error when the token does not map to a
/// registered rule.
pub fn explain_issue_type(
    issue_type: &str,
) -> Result<fallow_output::ExplainOutput, crate::ProgrammaticError> {
    let Some(rule) = rule_by_token(issue_type) else {
        return Err(unknown_explain_error(issue_type));
    };
    let guide = rule_guide(rule);
    Ok(fallow_output::ExplainOutput {
        id: rule.id.to_string(),
        name: rule_explain_name(rule).to_string(),
        summary: rule_explain_summary(rule).to_string(),
        rationale: rule.full.to_string(),
        example: guide.example.to_string(),
        how_to_fix: guide.how_to_fix.to_string(),
        docs: rule_docs_url(rule),
    })
}

/// Serialize standalone explain output using the programmatic API contract.
///
/// # Errors
///
/// Returns a structured programmatic error for unknown rule tokens or JSON
/// serialization failures.
pub fn serialize_explain_programmatic_json(
    issue_type: &str,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, crate::ProgrammaticError> {
    let output = explain_issue_type(issue_type)?;
    fallow_output::serialize_explain_json_output(output, analysis_run_id).map_err(|error| {
        crate::ProgrammaticError::new(format!("JSON serialization error: {error}"), 2)
            .with_code("json_serialization")
    })
}

/// Structured error for an unrecognized `fallow explain` issue type, with
/// suggestions matched to whether the token looks security-related.
#[must_use]
pub fn unknown_explain_error(issue_type: &str) -> crate::ProgrammaticError {
    if let Some(nearest) = nearest_explain_token(issue_type) {
        return crate::ProgrammaticError::new(
            format!("unknown issue type '{issue_type}'. Did you mean '{nearest}'?"),
            2,
        )
        .with_code("unknown_issue_type")
        .with_help(format!("fallow explain {nearest}"));
    }
    let message = if looks_security_explain_token(issue_type) {
        format!(
            "unknown issue type '{issue_type}'. Try values like tainted-sink, client-server-leak, hardcoded-secret, sql-injection, or security/sql-injection"
        )
    } else {
        format!(
            "unknown issue type '{issue_type}'. Try values like unused files, unused-export, high complexity, or code duplication"
        )
    };
    crate::ProgrammaticError::new(message, 2).with_code("unknown_issue_type")
}

/// The registered token closest to an unrecognized one, over the same
/// vocabulary [`rule_by_token`] accepts (bare ids and namespaced rule ids).
/// A hardcoded example list cannot name the token that is one edit away, and
/// that is the token the caller almost always meant.
fn nearest_explain_token(issue_type: &str) -> Option<&'static str> {
    let normalized = issue_type
        .trim()
        .strip_prefix("fallow/")
        .unwrap_or_else(|| issue_type.trim())
        .trim_start_matches("--")
        .replace('_', "-");
    let candidates: Vec<&'static str> = all_rules()
        .flat_map(|rule| [bare_rule_id(rule), rule.id])
        .collect();
    fallow_config::levenshtein::closest_match(&normalized, candidates)
}

fn looks_security_explain_token(issue_type: &str) -> bool {
    let normalized = issue_type.trim().to_ascii_lowercase().replace('_', "-");
    normalized.contains("security")
        || normalized.contains("secret")
        || normalized.contains("sink")
        || normalized.contains("cwe")
        || normalized.contains("client-server")
        || normalized.contains("injection")
}

/// Rule definitions for complexity and health findings.
pub const HEALTH_RULES: &[RuleDef] = &[
    RuleDef {
        id: "fallow/high-cyclomatic-complexity",
        category: "Health",
        name: "High Cyclomatic Complexity",
        short: "Function has high cyclomatic complexity",
        full: "McCabe cyclomatic complexity exceeds the configured threshold. Cyclomatic complexity counts the number of independent paths through a function (1 + decision points: if/else, switch cases, loops, ternary, logical operators). High values indicate functions that are hard to test exhaustively. fallow also emits this rule on synthetic `<template>` findings across every supported template dialect (Angular .html templates and inline `@Component({ template: ... })` literals, Vue SFC `<template>` blocks, Svelte markup, Astro markup), counting each dialect's control-flow blocks (Angular `@if`/`@else if`/`@for`/`@case`/`@defer (when ...)` and legacy `*ngIf`/`*ngFor`; Vue `v-if`/`v-else-if`/`v-for`; Svelte `{#if}`/`{:else if}`/`{#each}`/`{#await}`/`{#key}`; the Astro equivalents) plus ternary and logical operators inside bound attributes and interpolations; on each top-level Svelte `{#snippet name(...)}` block, emitted as its own `<snippet:NAME>` unit; and on synthetic `<component>` rollup findings whose `cyclomatic` is the worst class method's score plus the template's. Ranking and `--targets` use the rollup total; JSON exposes the per-half breakdown under `component_rollup`.",
        docs_path: "explanations/health#cyclomatic-complexity",
    },
    RuleDef {
        id: "fallow/high-cognitive-complexity",
        category: "Health",
        name: "High Cognitive Complexity",
        short: "Function has high cognitive complexity",
        full: "SonarSource cognitive complexity exceeds the configured threshold. Unlike cyclomatic complexity, cognitive complexity penalizes nesting depth and non-linear control flow (breaks, continues, early returns). It measures how hard a function is to understand when reading sequentially. fallow also emits this rule on synthetic `<template>` findings across every supported template dialect (Angular .html templates and inline `@Component({ template: ... })` literals, Vue SFC `<template>` blocks, Svelte markup, Astro markup), where nesting penalties accumulate on stacked control-flow blocks: Angular `@if`/`@for`/`@switch`, Vue `v-if`/`v-for`, Svelte `{#if}`/`{#each}`/`{#await}`; on each top-level Svelte `{#snippet name(...)}` block, emitted as its own `<snippet:NAME>` unit with nesting rebased to zero so in-file snippet extraction lowers the parent template's score; and on synthetic `<component>` rollup findings whose `cognitive` is the worst class method's score plus the template's. Ranking and `--targets` use the rollup total; JSON exposes the per-half breakdown under `component_rollup`.",
        docs_path: "explanations/health#cognitive-complexity",
    },
    RuleDef {
        id: "fallow/high-complexity",
        category: "Health",
        name: "High Complexity (Both)",
        short: "Function exceeds both complexity thresholds",
        full: "Function exceeds both cyclomatic and cognitive complexity thresholds. This is the strongest signal that a function needs refactoring, it has many paths AND is hard to understand. The same rule fires on synthetic `<template>` findings in every supported template dialect (Angular .html templates and inline `@Component({ template: ... })` literals, Vue SFC `<template>` blocks, Svelte markup, Astro markup) when both metrics exceed their thresholds, and on synthetic `<component>` rollup findings whose totals are the worst class method's score plus the template's. Ranking and `--targets` use the rollup totals; JSON exposes the per-half breakdown under `component_rollup`.",
        docs_path: "explanations/health#complexity-metrics",
    },
    RuleDef {
        id: "fallow/high-crap-score",
        category: "Health",
        name: "High CRAP Score",
        short: "Function has a high CRAP score (complexity combined with low coverage)",
        full: "The function's CRAP (Change Risk Anti-Patterns) score meets or exceeds the configured threshold. CRAP combines cyclomatic complexity with test coverage using the Savoia and Evans (2007) formula: `CC^2 * (1 - coverage/100)^3 + CC`. High CRAP indicates changes to this function carry high risk because it is complex AND poorly tested. Pair with `--coverage` for accurate per-function scoring; without it fallow estimates coverage from the module graph. This rule fires on functions only: synthetic template-family units (`<template>`, Svelte `<snippet:NAME>`) are excluded from the CRAP dimension, because a template is exercised only through its component and carries no coverage the formula could measure. Templates gate on the cyclomatic and cognitive dimensions instead.",
        docs_path: "explanations/health#crap-score",
    },
    RuleDef {
        id: "fallow/long-file",
        category: "Health",
        name: "Long File",
        short: "File has more lines than the file-length threshold",
        full: "The file has more physical lines than `health.maxFileLines` (or `--max-file-lines`, or a `thresholdOverrides[].maxFileLines` entry for the file). Every line counts, also blank lines and comment lines; a final newline does not add a line. The threshold is off (`0`) by default. The finding is a complexity finding with `exceeded: \"file_lines\"`, the name `<file>`, the line count in `line_count`, and the ceiling in `max_file_lines`. The `complexity-file-lines` rule sets its gate severity. In `fallow audit`, a long file that did not grow by at least 5 lines stays inherited, and one that grew is worsened.",
        docs_path: "explanations/health#file-length",
    },
    RuleDef {
        id: "fallow/refactoring-target",
        category: "Health",
        name: "Refactoring Target",
        short: "File identified as a high-priority refactoring candidate",
        full: "File identified as a refactoring candidate based on a weighted combination of complexity density, churn velocity, dead code ratio, fan-in (blast radius), and fan-out (coupling). Categories: urgent churn+complexity, break circular dependency, split high-impact file, remove dead code, extract complex functions, reduce coupling.",
        docs_path: "explanations/health#refactoring-targets",
    },
    RuleDef {
        id: "fallow/css-token-drift",
        category: "Health",
        name: "CSS Token Drift",
        short: "CSS or CSS-in-JS hardcoded styling value bypasses the design token system",
        full: "A styling value appears to bypass the project's design token system, for example a Tailwind arbitrary value in markup. The finding is advisory by default and does not affect the audit verdict unless rules.css-token-drift is set to error. Verify the value and replace it with an existing scale token when appropriate.",
        docs_path: "explanations/health#css-token-drift",
    },
    RuleDef {
        id: "fallow/css-duplicate-block",
        category: "Health",
        name: "CSS Duplicate Block",
        short: "CSS or CSS-in-JS declaration block is duplicated across rules",
        full: "A style rule declaration block is repeated across selectors, suggesting copy-pasted styling that can often be consolidated. The finding is advisory by default and does not affect the audit verdict unless rules.css-duplicate-block is set to error.",
        docs_path: "explanations/health#css-duplicate-block",
    },
    RuleDef {
        id: "fallow/css-selector-complexity",
        category: "Health",
        name: "CSS Selector Complexity",
        short: "CSS selector, nesting, or important usage is structurally complex",
        full: "A CSS or CSS-in-JS rule crosses the structural floor for selector specificity, selector complexity, nesting depth, or important usage. The finding is advisory by default and does not affect the audit verdict unless rules.css-selector-complexity is set to error.",
        docs_path: "explanations/health#css-selector-complexity",
    },
    RuleDef {
        id: "fallow/css-dead-surface",
        category: "Health",
        name: "CSS Dead Surface",
        short: "CSS or CSS-in-JS surface appears unused",
        full: "A styling surface appears unused in the analyzed project, such as a scoped SFC class with no component-local use. The finding is advisory by default and does not affect the audit verdict unless rules.css-dead-surface is set to error. Verify dynamic consumers before deleting.",
        docs_path: "explanations/health#css-dead-surface",
    },
    RuleDef {
        id: "fallow/css-broken-reference",
        category: "Health",
        name: "CSS Broken Reference",
        short: "CSS or CSS-in-JS reference resolves to no stylesheet definition",
        full: "A styling reference appears unresolved, such as a class token or keyframes name that has no stylesheet definition in the analyzed project. The finding is advisory by default and does not affect the audit verdict unless rules.css-broken-reference is set to error. Verify external or CSS-in-JS definitions before fixing.",
        docs_path: "explanations/health#css-broken-reference",
    },
    RuleDef {
        id: "fallow/untested-file",
        category: "Health",
        name: "Untested File",
        short: "Runtime-reachable file has no test dependency path",
        full: "A file is reachable from runtime entry points but not from any discovered test entry point. This indicates production code that no test imports, directly or transitively, according to the static module graph.",
        docs_path: "explanations/health#coverage-gaps",
    },
    RuleDef {
        id: "fallow/untested-export",
        category: "Health",
        name: "Untested Export",
        short: "Runtime-reachable export has no test dependency path",
        full: "A value export is reachable from runtime entry points but no test-reachable module references it. This is a static test dependency gap rather than line coverage, and highlights exports exercised only through production entry paths.",
        docs_path: "explanations/health#coverage-gaps",
    },
    RuleDef {
        id: "fallow/runtime-safe-to-delete",
        category: "Health",
        name: "Production Safe To Delete",
        short: "Statically unused AND never invoked in production with V8 tracking",
        full: "The function is both statically unreachable in the module graph and was never invoked during the observed runtime coverage window. This is the highest-confidence delete signal fallow emits.",
        docs_path: "explanations/health#runtime-coverage",
    },
    RuleDef {
        id: "fallow/runtime-review-required",
        category: "Health",
        name: "Production Review Required",
        short: "Statically used but never invoked in production",
        full: "The function is reachable in the module graph (or exercised by tests / untracked call sites) but was not invoked during the observed runtime coverage window. Needs a human look: may be seasonal, error-path only, or legitimately unused.",
        docs_path: "explanations/health#runtime-coverage",
    },
    RuleDef {
        id: "fallow/runtime-low-traffic",
        category: "Health",
        name: "Production Low Traffic",
        short: "Function was invoked below the low-traffic threshold",
        full: "The function was invoked in production but below the configured `--low-traffic-threshold` fraction of total trace count (spec default 0.1%). Effectively dead for the current period.",
        docs_path: "explanations/health#runtime-coverage",
    },
    RuleDef {
        id: "fallow/runtime-coverage-unavailable",
        category: "Health",
        name: "Runtime Coverage Unavailable",
        short: "Runtime coverage could not be resolved for this function",
        full: "The function could not be matched to a V8-tracked coverage entry. Common causes: the function lives in a worker thread (separate V8 isolate), it is lazy-parsed and never reached the JIT tier, or its source map did not resolve to the expected source path. This is advisory, not a dead-code signal.",
        docs_path: "explanations/health#runtime-coverage",
    },
    RuleDef {
        id: "fallow/runtime-coverage",
        category: "Health",
        name: "Runtime Coverage",
        short: "Runtime coverage finding",
        full: "Generic runtime-coverage finding for verdicts not covered by a more specific rule. Covers the forward-compat `unknown` sentinel; the CLI filters `active` entries out of `runtime_coverage.findings` so the surfaced list stays actionable.",
        docs_path: "explanations/health#runtime-coverage",
    },
    RuleDef {
        id: "fallow/coverage-intelligence-risky-change",
        category: "Health",
        name: "Coverage Intelligence Risky Change",
        short: "Changed hot path combines high CRAP and low test coverage",
        full: "Coverage intelligence combined change scope, runtime hot-path evidence, low test coverage, and high CRAP into a risky-change finding. Add focused tests or split the change before merging.",
        docs_path: "explanations/health#coverage-intelligence",
    },
    RuleDef {
        id: "fallow/coverage-intelligence-delete",
        category: "Health",
        name: "Coverage Intelligence Delete",
        short: "Static and runtime evidence indicate code can be deleted",
        full: "Coverage intelligence combined static unused status, runtime cold evidence, and lack of test reachability into a high-confidence delete recommendation.",
        docs_path: "explanations/health#coverage-intelligence",
    },
    RuleDef {
        id: "fallow/coverage-intelligence-review",
        category: "Health",
        name: "Coverage Intelligence Review",
        short: "Cold reachable uncovered code needs owner review",
        full: "Coverage intelligence found code that is statically reachable but cold in runtime evidence, uncovered by tests, and ownership-risky. Route it to an owner before changing or deleting it.",
        docs_path: "explanations/health#coverage-intelligence",
    },
    RuleDef {
        id: "fallow/coverage-intelligence-refactor",
        category: "Health",
        name: "Coverage Intelligence Refactor",
        short: "Hot covered code has high CRAP and should be refactored carefully",
        full: "Coverage intelligence found hot production code that is covered by tests but still has high CRAP. Refactor carefully while preserving behavior.",
        docs_path: "explanations/health#coverage-intelligence",
    },
];

/// Rule definitions for duplication findings.
pub const DUPES_RULES: &[RuleDef] = &[RuleDef {
    id: "fallow/code-duplication",
    category: "Duplication",
    name: "Code Duplication",
    short: "Duplicated code block",
    full: "A block of code that appears in multiple locations with identical or near-identical token sequences. Clone detection uses normalized token comparison, abstracting identifiers and literals in non-strict modes. Duplicated line counts and percentages include every instance, including the first. Overlapping line ranges in the same file count once. Redundant token counts exclude one retained copy per clone group.",
    docs_path: "explanations/duplication#clone-groups",
}];

/// Rule definitions for the diff-risk rules of `fallow audit`.
pub const AUDIT_RULES: &[RuleDef] = &[
    RuleDef {
        id: "fallow/weakened-tests",
        category: "Audit",
        name: "Weakened Tests",
        short: "The change weakens tests or a coverage threshold",
        full: "`fallow audit` compares the base and head text of each changed file. The `kind` field tells how the change weakened the tests: `skip-only` (a new `.skip`, `.only`, `xit` or `fit` marker), `removed-tests` (fewer test cases in a file, while the total of all changed test files also dropped), `assertion-drop` (fewer `expect`, `assert` or `should` calls in a test file with no test case removed), or `coverage-threshold` (a coverage or quality threshold that is less strict). A drop does not report when the lost test lines name code that the change removed, such as a deleted function or a deleted fixture directory: the tests went with that code. A name of a deleted file that a changed file still holds, a name that a deleted file only imports, a directory that still exists, and a reserved word such as `import` do not count. An assertion drop reports only when assertions are at least half of the code lines that the file lost, so a test rewrite that moves setup checks into a helper does not report. The finding is a fact about the change, not a proof that coverage dropped. Defaults to `warn`.",
        docs_path: "cli/audit#diff-risk-rules",
    },
    RuleDef {
        id: "fallow/added-suppression",
        category: "Audit",
        name: "Added Suppressions",
        short: "The change adds a suppression comment",
        full: "`fallow audit` reports a comment line that the change adds with a lint, type-check, coverage or Fallow suppression: `eslint-disable`, `oxlint-disable`, `biome-ignore`, `deno-lint-ignore`, `@ts-ignore`, `@ts-expect-error`, `@ts-nocheck`, `istanbul ignore`, `c8 ignore`, `v8 ignore` or `fallow-ignore`. A moved comment is not added. A `fallow-ignore` comment that names only diff-risk rules does not count. Files below a fixture or generated directory (`fixture`, `fixtures`, `__fixtures__`, `__snapshots__`, `generated`, `__generated__`) do not report, because a tool writes them or a test reads them as data. Defaults to `warn`.",
        docs_path: "cli/audit#diff-risk-rules",
    },
    RuleDef {
        id: "fallow/test-only-branch",
        category: "Audit",
        name: "Test-Only Branches",
        short: "The change adds a test-environment branch to source code",
        full: "`fallow audit` reports a line that the change adds to a non-test source file and that checks the test environment: `process.env.NODE_ENV` compared with `'test'`, `process.env.VITEST` or `process.env.JEST_WORKER_ID`. Such a branch lets the tests take a path that production does not take. A Vitest in-source test block (`if (import.meta.vitest)`) is test code and is not reported. Tool config files are skipped. A check that only keeps `console` output quiet in tests (`if (process.env.NODE_ENV !== 'test') console.warn(...)`), and a check that picks an env file (`'.env.test'`), do not report. Defaults to `warn`.",
        docs_path: "cli/audit#diff-risk-rules",
    },
    RuleDef {
        id: "fallow/diverged-clone-sibling",
        category: "Audit",
        name: "Diverged Clone Siblings",
        short: "The change edits one copy of duplicated code but not its siblings",
        full: "`fallow audit` reports a base clone group where the change edited exactly one copy, the edit broke the clone, and every other copy kept its text. The siblings can need the same change. A cosmetic edit that keeps the clone, a removed copy, a change that edits every copy, and an edit to a test or story copy do not report. Needs the base snapshot of `--gate new-only`. Opt-in: defaults to `off`, because copies often differ on purpose, for example an input and an output type, or a legacy copy. Set it to `warn` or `error` to review every diverged copy.",
        docs_path: "cli/audit#diff-risk-rules",
    },
    RuleDef {
        id: "fallow/added-dependency",
        category: "Audit",
        name: "Added Dependencies",
        short: "The change adds a dependency",
        full: "`fallow audit` reports a dependency that the change adds to a `package.json`. A new dependency is often correct, so the rule defaults to `off`. Set it to `warn` or `error` to review every new package. An invented package name already fails as an unresolved import or an unlisted dependency.",
        docs_path: "cli/audit#diff-risk-rules",
    },
];

/// Rule definitions for feature-flag findings.
pub const FLAGS_RULES: &[RuleDef] = &[
    RuleDef {
        id: "fallow/feature-flag",
        category: "Flags",
        name: "Feature Flags",
        short: "Detected feature flag pattern",
        full: "A feature flag pattern detected by `fallow flags`: environment-variable checks, flag SDK calls (LaunchDarkly, Unleash, and similar), or config-object lookups. Long-lived flags accumulate dead branches; review old flags for retirement and pair with dead-code analysis to find branches that can no longer execute.",
        docs_path: "cli/flags",
    },
    RuleDef {
        id: "fallow/flag-retirement-candidate",
        category: "Flags",
        name: "Flag Retirement Candidate",
        short: "Feature flag is a retirement candidate",
        full: "A feature flag that `fallow flags --retirement` lists with at least one retirement reason: one read site, reads only in tests, a literal constant, identical or empty guard branches, guarded dead code, a definition that no code reads, or a vendor state from `--flag-state` (rolled out, archived, missing in the vendor export, or in the export only). The report is advisory. A person decides to remove the flag; Fallow does not remove code.",
        docs_path: "cli/flags#retirement-report",
    },
];

macro_rules! security_catalogue_rule {
    ($id:literal, $name:literal, $cwe:literal) => {
        RuleDef {
            id: concat!("security/", $id),
            category: "Security",
            name: $name,
            short: concat!("Catalogue security candidate for CWE-", $cwe),
            full: concat!(
                $name,
                " is a data-driven `fallow security` tainted-sink catalogue category with CWE-",
                $cwe,
                " metadata. fallow reports it as an unverified candidate when a captured sink shape matches this category. Use it to understand or filter `security/",
                $id,
                "` findings, then inspect the trace, source, sink, sanitization, and application context before treating it as exploitable."
            ),
            docs_path: "cli/security",
        }
    };
}

/// Rule definitions for security candidate findings, including the
/// data-driven tainted-sink catalogue categories.
pub const SECURITY_RULES: &[RuleDef] = &[
    RuleDef {
        id: "security/tainted-sink",
        category: "Security",
        name: "Tainted Sink Candidates",
        short: "Syntactic security sink candidates require verification",
        full: "The `tainted-sink` family covers data-driven `fallow security` catalogue categories. These findings are unverified candidates, not confirmed vulnerabilities. fallow can connect known source signals to captured sink shapes and add CWE metadata, but it does not prove attacker control, missing sanitization, exploitability, or business impact.",
        docs_path: "cli/security",
    },
    RuleDef {
        id: "security/client-server-leak",
        category: "Security",
        name: "Client-server Secret Leak Candidates",
        short: "Client-bound code reaches a non-public env read",
        full: "`client-server-leak` reports a candidate when a `use client` module can transitively reach a static non-public `process.env` or `import.meta.env` read. Public-by-convention env prefixes are treated as public. The finding is advisory and still needs bundle, framework, and runtime verification before treating it as a real exposure.",
        docs_path: "cli/security",
    },
    RuleDef {
        id: "security/hardcoded-secret",
        category: "Security",
        name: "Hardcoded Secret Candidates",
        short: "Provider-prefixed or contextual secret literals require verification",
        full: "`hardcoded-secret` reports opt-in candidates for provider-prefixed or contextual secret-shaped literals. The category is include-required and only runs when listed in `security.categories.include`. It avoids raw entropy alone, but every result still requires review, secret rotation decisions, and context before acting.",
        docs_path: "cli/security",
    },
    security_catalogue_rule!("dangerous-html", "Dangerous HTML sink", "79"),
    security_catalogue_rule!(
        "template-escape-bypass",
        "Template escape bypass sink",
        "79"
    ),
    security_catalogue_rule!("command-injection", "OS command injection sink", "78"),
    security_catalogue_rule!("code-injection", "Code injection sink", "94"),
    security_catalogue_rule!("dynamic-regex", "Dynamic regular expression sink", "1333"),
    security_catalogue_rule!("redos-regex", "ReDoS regex sink", "1333"),
    security_catalogue_rule!(
        "resource-amplification",
        "Resource amplification sink",
        "400"
    ),
    security_catalogue_rule!("dynamic-module-load", "Dynamic module load sink", "95"),
    security_catalogue_rule!("sql-injection", "SQL injection sink", "89"),
    security_catalogue_rule!("ssrf", "Server-side request forgery sink", "918"),
    security_catalogue_rule!(
        "secret-to-network",
        "Secret reaches a network request",
        "201"
    ),
    security_catalogue_rule!("path-traversal", "Path traversal sink", "22"),
    security_catalogue_rule!(
        "header-injection",
        "HTTP response header injection sink",
        "113"
    ),
    security_catalogue_rule!("open-redirect", "Open redirect sink", "601"),
    security_catalogue_rule!(
        "postmessage-wildcard-origin",
        "Wildcard postMessage target origin",
        "346"
    ),
    security_catalogue_rule!("tls-validation-disabled", "TLS validation disabled", "295"),
    security_catalogue_rule!("cleartext-transport", "Cleartext transport URL", "319"),
    security_catalogue_rule!(
        "electron-unsafe-webpreferences",
        "Unsafe Electron BrowserWindow preferences",
        "1188"
    ),
    security_catalogue_rule!(
        "world-writable-permission",
        "World-writable chmod mode",
        "732"
    ),
    security_catalogue_rule!(
        "insecure-temp-file",
        "Predictable temporary file path",
        "377"
    ),
    security_catalogue_rule!(
        "mysql-multiple-statements",
        "MySQL multiple statements enabled",
        "89"
    ),
    security_catalogue_rule!("permissive-cors", "Permissive CORS policy", "942"),
    security_catalogue_rule!("insecure-cookie", "Insecure cookie options", "614"),
    security_catalogue_rule!("mass-assignment", "Mass assignment sink", "915"),
    security_catalogue_rule!("weak-crypto", "Runtime-selectable crypto algorithm", "327"),
    security_catalogue_rule!("insecure-randomness", "Insecure randomness sink", "338"),
    security_catalogue_rule!("jwt-alg-none", "JWT alg none", "347"),
    security_catalogue_rule!(
        "jwt-verify-missing-algorithms",
        "JWT verify missing algorithms allowlist",
        "347"
    ),
    security_catalogue_rule!("deprecated-cipher", "Deprecated cipher constructor", "327"),
    security_catalogue_rule!(
        "unsafe-buffer-alloc",
        "Unsafe Buffer allocation sink",
        "1188"
    ),
    security_catalogue_rule!(
        "unsafe-deserialization",
        "Unsafe deserialization sink",
        "502"
    ),
    security_catalogue_rule!(
        "angular-trusted-html",
        "Angular bypassSecurityTrust sink",
        "79"
    ),
    security_catalogue_rule!("nextjs-open-redirect", "Next.js open redirect sink", "601"),
    security_catalogue_rule!("dom-document-write", "DOM document.write sink", "79"),
    security_catalogue_rule!("jquery-html", "jQuery .html() sink", "79"),
    security_catalogue_rule!(
        "route-send-file",
        "Route file-send path traversal sink",
        "22"
    ),
    security_catalogue_rule!("webview-injection", "WebView injected-script sink", "94"),
    security_catalogue_rule!("prototype-pollution", "Prototype pollution sink", "1321"),
    security_catalogue_rule!("zip-slip", "Archive path-traversal (zip-slip) sink", "22"),
    security_catalogue_rule!("nosql-injection", "NoSQL injection sink", "943"),
    security_catalogue_rule!("ssti", "Server-side template injection sink", "1336"),
    security_catalogue_rule!("xxe", "XML external entity (XXE) sink", "611"),
    security_catalogue_rule!("secret-pii-log", "Secret or PII logged", "532"),
    security_catalogue_rule!("xpath-injection", "XPath injection sink", "643"),
    security_catalogue_rule!(
        "llm-call-injection",
        "Untrusted input reaches an LLM call",
        "1427"
    ),
];

/// Build the `_meta` object for `fallow security --format json --explain`.
#[must_use]
pub fn security_meta() -> fallow_types::envelope::Meta {
    fallow_output::security_meta(SECURITY_RULES.iter().map(|rule| {
        fallow_output::SecurityRuleMeta {
            id: rule.id,
            name: rule.name,
            description: rule.full,
            docs_path: rule.docs_path,
        }
    }))
}

/// Build the `_meta` object for `fallow coverage setup --json --explain`.
#[must_use]
pub fn coverage_setup_meta() -> Value {
    fallow_output::coverage_setup_meta()
}

/// Build the `_meta` object for `fallow coverage analyze --format json --explain`.
#[must_use]
pub fn coverage_analyze_meta() -> Value {
    fallow_output::coverage_analyze_meta()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "registry tests intentionally index fixture JSON"
)]
#[path = "explain_tests.rs"]
mod tests;
