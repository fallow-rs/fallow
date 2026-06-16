//! Metric and rule definitions for explainable CLI output.
//!
//! Provides structured metadata that describes what each metric, threshold,
//! and rule means, consumed by the `_meta` object in JSON output and by
//! SARIF `fullDescription` / `helpUri` fields.

use std::collections::BTreeMap;
use std::process::ExitCode;

use colored::Colorize;
use fallow_config::OutputFormat;
use fallow_types::envelope::{Meta, MetaRule};
use serde_json::{Value, json};

const DOCS_BASE: &str = "https://docs.fallow.tools";

/// Docs URL for the dead-code (check) command.
pub const CHECK_DOCS: &str = "https://docs.fallow.tools/cli/dead-code";

/// Docs URL for the health command.
pub const HEALTH_DOCS: &str = "https://docs.fallow.tools/cli/health";

/// Docs URL for the dupes command.
pub const DUPES_DOCS: &str = "https://docs.fallow.tools/cli/dupes";

/// Docs URL for the runtime coverage setup command's agent-readable JSON.
pub const COVERAGE_SETUP_DOCS: &str = "https://docs.fallow.tools/cli/coverage#agent-readable-json";

/// Docs URL for `fallow coverage analyze --format json --explain`.
pub const COVERAGE_ANALYZE_DOCS: &str = "https://docs.fallow.tools/cli/coverage#analyze";

/// Docs URL for the security command.
pub const SECURITY_DOCS: &str = "https://docs.fallow.tools/cli/security";

/// `_meta` description for the per-finding `actions[]` array shared across
/// `check`, `health`, and `dupes` JSON output.
const ACTIONS_FIELD_DEFINITION: &str = "Per-finding fix and suppression suggestions. Each entry carries a `type` discriminant (kebab-case) plus a per-action `auto_fixable` bool. Consumers dispatch on `type` to choose the remediation and filter on `auto_fixable` of each individual entry.";

/// `_meta` description for the per-action `auto_fixable` bool. Calls out the
/// per-finding (not per-action-type) evaluation rule and the currently active
/// per-instance flips so agents know to branch on the field value of EACH
/// finding's action, not on the action `type` alone.
const ACTIONS_AUTO_FIXABLE_FIELD_DEFINITION: &str = "Evaluated PER FINDING, not per action type. The same `type` may carry `auto_fixable: true` on one finding and `auto_fixable: false` on another when per-instance guards in the `fallow fix` applier discriminate. Filter on this bool of each individual action, not on `type` alone. Current per-instance flips: (1) `remove-catalog-entry` is `true` only when the finding's `hardcoded_consumers` array is empty (else fallow fix skips the entry to avoid breaking `pnpm install`); (2) the primary dependency action flips between `remove-dependency` (`auto_fixable: true`) and `move-dependency` (`auto_fixable: false`) based on `used_in_workspaces`; (3) `add-to-config` for `ignoreExports` is `true` when fallow fix can safely apply the action, which means EITHER a fallow config file already exists OR no config exists and the working directory is NOT inside a monorepo subpackage (the applier then creates `.fallowrc.json` using `fallow init`'s framework-aware scaffolding and layers the new rules on top); `false` inside a monorepo subpackage with no workspace-root config because the applier refuses to fragment per-package configs; (4) `update-catalog-reference` is always `false` today (catalog-switching applier not yet wired). All `suppress-line` and `suppress-file` actions are uniformly `false`.";

/// Rule definition for SARIF `fullDescription` and JSON `_meta`.
pub struct RuleDef {
    pub id: &'static str,
    /// Coarse category label used by the sticky PR/MR comment renderer to
    /// group findings into collapsible sections (Dead code, Dependencies,
    /// Duplication, Health, Architecture, Suppressions). One source of
    /// truth so the CodeClimate / SARIF / review-envelope path and the
    /// renderer never drift; a unit test below asserts every RuleDef has
    /// a non-empty category.
    pub category: &'static str,
    pub name: &'static str,
    pub short: &'static str,
    pub full: &'static str,
    pub docs_path: &'static str,
}

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
        full: "Packages that are imported in source code but not listed in package.json. These work by accident (hoisted from another workspace package or transitive dep) and will break in strict package managers.",
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
        full: "A file classified into a boundary zone calls a callee matching one of the zone's boundaries.calls.forbidden patterns. The check is syntactic: it matches the written callee path and the import-resolved canonical path, and it only applies to files classified into a zone. Move the call behind an allowed abstraction, or adjust the zone's forbidden patterns if the rule was wrong.",
        docs_path: "explanations/dead-code#boundary-violations",
    },
    RuleDef {
        id: "fallow/policy-violation",
        category: "Policy",
        name: "Policy Violation",
        short: "Banned call or import matched a rule-pack rule",
        full: "A call site or import matched a banned-call or banned-import rule from a configured rule pack (the rulePacks config key). Packs are pure declarative data; the check is syntactic and does not follow aliased or re-bound callees, and import matching uses the raw specifier. Replace the banned usage per the rule's message, scope the rule with files/exclude globs, or adjust its severity.",
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
        id: "fallow/unused-catalog-entry",
        category: "Dependencies",
        name: "Unused pnpm catalog entry",
        short: "Catalog entry in pnpm-workspace.yaml not referenced by any workspace package",
        full: "An entry in the `catalog:` or `catalogs:` section of pnpm-workspace.yaml that no workspace package.json references via the `catalog:` protocol. Catalog entries are leftover dependency metadata once a package is removed from every consumer; delete the entry to keep the catalog truthful. See also: fallow/unresolved-catalog-reference (the inverse: consumer references a catalog that does not declare the package).",
        docs_path: "explanations/dead-code#unused-catalog-entries",
    },
    RuleDef {
        id: "fallow/empty-catalog-group",
        category: "Dependencies",
        name: "Empty pnpm catalog group",
        short: "Named catalog group in pnpm-workspace.yaml has no entries",
        full: "A named group under `catalogs:` in pnpm-workspace.yaml has no package entries. Empty named groups are leftover catalog structure after the last entry is removed. The top-level `catalog:` map is intentionally ignored because some projects keep it as a stable hook.",
        docs_path: "explanations/dead-code#empty-catalog-groups",
    },
    RuleDef {
        id: "fallow/unresolved-catalog-reference",
        category: "Dependencies",
        name: "Unresolved pnpm catalog reference",
        short: "package.json references a catalog that does not declare the package",
        full: "A workspace package.json declares a dependency with the `catalog:` or `catalog:<name>` protocol, but the catalog has no entry for that package. `pnpm install` will fail with ERR_PNPM_CATALOG_ENTRY_NOT_FOUND_FOR_CATALOG_PROTOCOL. To fix: add the package to the named catalog, switch the reference to a different catalog that does declare it, or remove the reference and pin a hardcoded version. Scope: the detector scans `dependencies`, `devDependencies`, `peerDependencies`, and `optionalDependencies` in every workspace `package.json`. See also: fallow/unused-catalog-entry (the inverse: catalog entries no consumer references).",
        docs_path: "explanations/dead-code#unresolved-catalog-references",
    },
    RuleDef {
        id: "fallow/unused-dependency-override",
        category: "Dependencies",
        name: "Unused pnpm dependency override",
        short: "pnpm.overrides entry targets a package not declared or resolved",
        full: "An entry in `pnpm-workspace.yaml`'s `overrides:` section, or the root `package.json`'s `pnpm.overrides` block, whose target package is not declared by any workspace package and is not present in `pnpm-lock.yaml`. Override entries linger after their target package leaves the resolved dependency tree. For projects without a readable lockfile, fallow falls back to workspace package.json manifests and keeps a `hint` so transitive CVE pins can be reviewed before removal. To fix: delete the entry, refresh `pnpm-lock.yaml` if it is stale, or add the entry to `ignoreDependencyOverrides` when the override is intentionally retained. See also: fallow/misconfigured-dependency-override.",
        docs_path: "explanations/dead-code#unused-dependency-overrides",
    },
    RuleDef {
        id: "fallow/misconfigured-dependency-override",
        category: "Dependencies",
        name: "Misconfigured pnpm dependency override",
        short: "pnpm.overrides entry has an unparsable key or value",
        full: "An entry in `pnpm-workspace.yaml`'s `overrides:` or `package.json`'s `pnpm.overrides` whose key or value does not parse as a valid pnpm override spec. Common shapes: empty key, empty value, malformed version selector on the target (`@types/react@<<18`), unbalanced parent matcher (`react>`), or unsupported `npm:alias@` syntax in the version (only the `-`, `$ref`, and `npm:alias` pnpm idioms are allowed). pnpm rejects the workspace at install time with a parser error. To fix: correct the key/value shape, or remove the entry. See also: fallow/unused-dependency-override.",
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
        id: "fallow/unused-component-prop",
        category: "Dead code",
        name: "Unused component props",
        short: "A Vue defineProps prop or React component prop is referenced nowhere in its own component",
        full: "A declared component prop referenced nowhere inside its own component, in either of two framework shapes: a Vue `<script setup>` defineProps prop (unused in neither script nor template), or a React/Preact prop destructured from a component's first parameter and read nowhere in its body. vue-tsc / Volar / tsc check caller-side prop correctness, not this in-component dead-input direction. Conservative: Vue abstains on `$attrs` fallthrough, whole-object props use, defineExpose, defineModel, and imported prop-type aliases; React abstains on rest spread (`{...rest}`), props forwarded by spread, props passed wholesale to a hook, `forwardRef` / imported-interface props, and exported public-API component props. Default warn; suppress or remove the prop.",
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
        full: "A React/Preact prop is received by a component, forwarded UNCHANGED to a child, and forwarded again through two or more intermediate \"pass-through\" components until a component that substantively uses it. The high-confidence signal is that the received identifier appears ONLY as the root of forwarded child-JSX attribute values (so `<Child userName={user.name}/>` counts: the prop `user` is projected forward), not the attribute name matching. fallow emits located per-chain records (the source, each pass-through hop, and the consumer with file + line + component name) so CI and an agent can act: colocate the consumer with the data, lift the value to a React context/provider at a mid-chain hop, or compose the component so the intermediates no longer thread the prop. This is a graph-derived health signal, not a correctness error. The rule defaults to OFF (opt-in), like private-type-leak and the security rules: enable it with `prop-drilling: \"warn\"` in `rules`. Zero false positives by construction: any `{...props}` spread, `cloneElement`, element-as-prop / render-prop / children-as-function, or context `*.Provider` anywhere in the chain abstains the whole chain, as does an ambiguous or unresolvable hop. The check runs only when the project declares `react` / `react-dom` / `next` / `preact`.",
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
        .find(|r| r.id == id)
}

/// Build the docs URL for a rule.
#[must_use]
pub fn rule_docs_url(rule: &RuleDef) -> String {
    format!("{DOCS_BASE}/{}", rule.docs_path)
}

/// Extra educational content for the standalone `fallow explain <issue-type>`
/// command. Kept separate from [`RuleDef`] so SARIF and `_meta` payloads remain
/// compact while terminal users and agents can ask for worked examples on
/// demand.
pub struct RuleGuide {
    pub example: &'static str,
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
    let alias = dead_code_alias_id(&normalized)
        .or_else(|| catalog_alias_id(&normalized))
        .or_else(|| health_alias_id(&normalized))
        .or_else(|| security_alias_id(&normalized));
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
            .find(|rule| {
                rule.docs_path.ends_with(&normalized)
                    || rule.docs_path.ends_with(singular)
                    || rule.name.eq_ignore_ascii_case(trimmed)
            })
    })
}

fn dead_code_alias_id(normalized: &str) -> Option<&'static str> {
    match normalized {
        "unused-files" => Some("fallow/unused-file"),
        "unused-exports" => Some("fallow/unused-export"),
        "unused-types" => Some("fallow/unused-type"),
        "private-type-leaks" => Some("fallow/private-type-leak"),
        "unused-deps" | "unused-dependencies" => Some("fallow/unused-dependency"),
        "unused-dev-deps" | "unused-dev-dependencies" => Some("fallow/unused-dev-dependency"),
        "unused-optional-deps" | "unused-optional-dependencies" => {
            Some("fallow/unused-optional-dependency")
        }
        "type-only-deps" | "type-only-dependencies" => Some("fallow/type-only-dependency"),
        "test-only-deps" | "test-only-dependencies" => Some("fallow/test-only-dependency"),
        "unused-enum-members" => Some("fallow/unused-enum-member"),
        "unused-class-members" => Some("fallow/unused-class-member"),
        "unused-store-members" => Some("fallow/unused-store-member"),
        "unprovided-injects" | "unprovided-inject" => Some("fallow/unprovided-inject"),
        "unrendered-components" | "unrendered-component" => Some("fallow/unrendered-component"),
        "unused-component-props" | "unused-component-prop" => Some("fallow/unused-component-prop"),
        "unused-component-emits" | "unused-component-emit" => Some("fallow/unused-component-emit"),
        "unused-component-inputs" | "unused-component-input" => {
            Some("fallow/unused-component-input")
        }
        "unused-component-outputs" | "unused-component-output" => {
            Some("fallow/unused-component-output")
        }
        "unused-server-actions" | "unused-server-action" => Some("fallow/unused-server-action"),
        "unused-load-data-keys" | "unused-load-data-key" => Some("fallow/unused-load-data-key"),
        "prop-drilling" => Some("fallow/prop-drilling"),
        "thin-wrapper" | "thin-wrappers" => Some("fallow/thin-wrapper"),
        "duplicate-prop-shape" | "duplicate-prop-shapes" => Some("fallow/duplicate-prop-shape"),
        "unresolved-imports" => Some("fallow/unresolved-import"),
        "unlisted-deps" | "unlisted-dependencies" => Some("fallow/unlisted-dependency"),
        "duplicate-exports" => Some("fallow/duplicate-export"),
        "circular-deps" | "circular-dependencies" => Some("fallow/circular-dependency"),
        "boundary-violations" => Some("fallow/boundary-violation"),
        "boundary-coverage" | "boundary-coverage-violations" => Some("fallow/boundary-coverage"),
        "boundary-calls" | "boundary-call-violations" => Some("fallow/boundary-call-violation"),
        "policy-violation" | "policy-violations" => Some("fallow/policy-violation"),
        "stale-suppressions" => Some("fallow/stale-suppression"),
        _ => None,
    }
}

fn catalog_alias_id(normalized: &str) -> Option<&'static str> {
    match normalized {
        "unused-catalog-entries" | "unused-catalog-entry" | "catalog" => {
            Some("fallow/unused-catalog-entry")
        }
        "empty-catalog-groups" | "empty-catalog-group" | "empty-catalog" => {
            Some("fallow/empty-catalog-group")
        }
        "unresolved-catalog-references" | "unresolved-catalog-reference" | "unresolved-catalog" => {
            Some("fallow/unresolved-catalog-reference")
        }
        "unused-dependency-overrides"
        | "unused-dependency-override"
        | "unused-override"
        | "unused-overrides" => Some("fallow/unused-dependency-override"),
        "misconfigured-dependency-overrides"
        | "misconfigured-dependency-override"
        | "misconfigured-override"
        | "misconfigured-overrides" => Some("fallow/misconfigured-dependency-override"),
        _ => None,
    }
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
        "duplication" | "dupes" | "code-duplication" => Some("fallow/code-duplication"),
        "feature-flag" | "feature-flags" | "flags" => Some("fallow/feature-flag"),
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
        .unwrap_or_else(fallback_rule_guide)
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
            example: "src/api.ts imports undici, but the nearest package.json does not list undici.",
            how_to_fix: "Add the package to dependencies/devDependencies in the workspace that imports it instead of relying on hoisting or transitive deps.",
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
            how_to_fix: "Replace the banned call or import with the alternative named in the rule's message. To waive one rule, use `// fallow-ignore-next-line policy-violation:<pack>/<rule-id>` or the file-level form. Use bare `policy-violation` only when you intend to suppress every rule-pack finding at that scope.",
        },
        "fallow/stale-suppression" => RuleGuide {
            example: "// fallow-ignore-next-line unused-export remains above an export that is now used.",
            how_to_fix: "Remove the suppression. If a different issue is still intentional, replace it with a current, specific suppression.",
        },
        _ => return None,
    })
}

fn catalog_rule_guide(id: &str) -> Option<RuleGuide> {
    Some(match id {
        "fallow/unused-catalog-entry" => RuleGuide {
            example: "pnpm-workspace.yaml declares `catalog: { is-even: ^1.0.0 }`, but no workspace package.json declares `\"is-even\": \"catalog:\"`.",
            how_to_fix: "Delete the entry from pnpm-workspace.yaml. If any consumer uses a hardcoded version (surfaced in `hardcoded_consumers`), switch that consumer to `catalog:` first to keep versions aligned.",
        },
        "fallow/empty-catalog-group" => RuleGuide {
            example: "pnpm-workspace.yaml declares `catalogs: { react17: {} }` after the last react17 entry was removed.",
            how_to_fix: "Delete the empty named group header from pnpm-workspace.yaml. Comments between the deleted header and the next sibling can stay in place for manual review.",
        },
        "fallow/unresolved-catalog-reference" => RuleGuide {
            example: "packages/app/package.json declares `\"old-react\": \"catalog:react17\"`, but `catalogs.react17` in pnpm-workspace.yaml does not declare `old-react`. `pnpm install` will fail.",
            how_to_fix: "If `available_in_catalogs` is non-empty, change the reference to one of those catalogs (e.g. `catalog:react18`). Otherwise add the package to the named catalog in pnpm-workspace.yaml, or remove the catalog reference and pin a hardcoded version. For staged migrations where the catalog edit lands separately, add the (package, catalog, consumer) triple to `ignoreCatalogReferences` in your fallow config.",
        },
        "fallow/unused-dependency-override" => RuleGuide {
            example: "pnpm-workspace.yaml declares `overrides: { axios: ^1.6.0 }`, but no workspace package.json declares `axios` and `pnpm-lock.yaml` does not resolve it.",
            how_to_fix: "Delete the entry from `pnpm-workspace.yaml` or `package.json#pnpm.overrides`. If the finding is caused by a stale or missing lockfile, refresh `pnpm-lock.yaml` and rerun fallow. If the override is intentionally retained, add it to `ignoreDependencyOverrides` in your fallow config.",
        },
        "fallow/misconfigured-dependency-override" => RuleGuide {
            example: "pnpm-workspace.yaml declares `overrides: { \"@types/react@<<18\": \"18.0.0\" }`. The doubled `<<` is not a valid pnpm version selector and pnpm will reject the workspace at install time.",
            how_to_fix: "Fix the key/value to match pnpm's override grammar: bare names (`axios`), scoped names (`@types/react`), targets with version selectors (`@types/react@<18`), parent matchers (`react>react-dom`), and parent chains with selectors on either side. Allowed value idioms: bare version range, `-` (delete), `$ref`, and `npm:alias`. If the entry was experimental, remove it.",
        },
        _ => return None,
    })
}

fn health_runtime_rule_guide(id: &str) -> Option<RuleGuide> {
    Some(match id {
        "fallow/high-cyclomatic-complexity"
        | "fallow/high-cognitive-complexity"
        | "fallow/high-complexity" => RuleGuide {
            example: "A function contains several nested conditionals, loops, and early exits, exceeding the configured complexity threshold. fallow also flags synthetic `<template>` findings on Angular .html templates and inline `@Component({ template: ... })` literals, and `<component>` rollup findings that combine the worst class method with its template.",
            how_to_fix: "For function findings, extract named helpers, split independent branches, flatten guard clauses, and add tests around the behavior before refactoring. For `<template>` findings, split the template into child components, hoist data into the component class as computed signals, or replace nested `@if`/`@for` with a flatter structure. For `<component>` rollup findings, attack the larger half first; the per-half breakdown lives in `component_rollup`.",
        },
        "fallow/high-crap-score" => RuleGuide {
            example: "A complex function has little or no matching Istanbul coverage, so its CRAP score crosses the configured gate.",
            how_to_fix: "Add focused tests for the risky branches first, then simplify the function if the score remains high.",
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

/// Run the standalone explain subcommand.
#[must_use]
pub fn run_explain(issue_type: &str, output: OutputFormat) -> ExitCode {
    let Some(rule) = rule_by_token(issue_type) else {
        let message = if looks_security_explain_token(issue_type) {
            format!(
                "unknown issue type '{issue_type}'. Try values like tainted-sink, client-server-leak, hardcoded-secret, sql-injection, or security/sql-injection"
            )
        } else {
            format!(
                "unknown issue type '{issue_type}'. Try values like unused files, unused-export, high complexity, or code duplication"
            )
        };
        return crate::error::emit_error(&message, 2, output);
    };
    let guide = rule_guide(rule);
    match output {
        OutputFormat::Json => {
            let envelope = crate::output_envelope::ExplainOutput {
                id: rule.id.to_string(),
                name: rule.name.to_string(),
                summary: rule.short.to_string(),
                rationale: rule.full.to_string(),
                example: guide.example.to_string(),
                how_to_fix: guide.how_to_fix.to_string(),
                docs: rule_docs_url(rule),
            };
            match crate::output_envelope::serialize_root_output(
                crate::output_envelope::FallowOutput::Explain(envelope),
            ) {
                Ok(value) => crate::report::emit_json(&value, "explain"),
                Err(e) => {
                    crate::error::emit_error(&format!("JSON serialization error: {e}"), 2, output)
                }
            }
        }
        OutputFormat::Human => print_explain_human(rule, &guide),
        OutputFormat::Compact => print_explain_compact(rule),
        OutputFormat::Markdown => print_explain_markdown(rule, &guide),
        OutputFormat::Sarif
        | OutputFormat::CodeClimate
        | OutputFormat::PrCommentGithub
        | OutputFormat::PrCommentGitlab
        | OutputFormat::ReviewGithub
        | OutputFormat::ReviewGitlab
        | OutputFormat::Badge => crate::error::emit_error(
            "explain supports human, compact, markdown, and json output",
            2,
            output,
        ),
    }
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

fn print_explain_human(rule: &RuleDef, guide: &RuleGuide) -> ExitCode {
    println!("{}", rule.name.bold());
    println!("{}", rule.id.dimmed());
    println!();
    println!("{}", rule.short);
    println!();
    println!("{}", "Why it matters".bold());
    println!("{}", rule.full);
    println!();
    println!("{}", "Example".bold());
    println!("{}", guide.example);
    println!();
    println!("{}", "How to fix".bold());
    println!("{}", guide.how_to_fix);
    println!();
    println!("{} {}", "Docs:".dimmed(), rule_docs_url(rule).dimmed());
    ExitCode::SUCCESS
}

fn print_explain_compact(rule: &RuleDef) -> ExitCode {
    println!("explain:{}:{}:{}", rule.id, rule.short, rule_docs_url(rule));
    ExitCode::SUCCESS
}

fn print_explain_markdown(rule: &RuleDef, guide: &RuleGuide) -> ExitCode {
    println!("# {}", rule.name);
    println!();
    println!("`{}`", rule.id);
    println!();
    println!("{}", rule.short);
    println!();
    println!("## Why it matters");
    println!();
    println!("{}", rule.full);
    println!();
    println!("## Example");
    println!();
    println!("{}", guide.example);
    println!();
    println!("## How to fix");
    println!();
    println!("{}", guide.how_to_fix);
    println!();
    println!("[Docs]({})", rule_docs_url(rule));
    ExitCode::SUCCESS
}

pub const HEALTH_RULES: &[RuleDef] = &[
    RuleDef {
        id: "fallow/high-cyclomatic-complexity",
        category: "Health",
        name: "High Cyclomatic Complexity",
        short: "Function has high cyclomatic complexity",
        full: "McCabe cyclomatic complexity exceeds the configured threshold. Cyclomatic complexity counts the number of independent paths through a function (1 + decision points: if/else, switch cases, loops, ternary, logical operators). High values indicate functions that are hard to test exhaustively. fallow also emits this rule on synthetic `<template>` findings (Angular .html templates and inline `@Component({ template: ... })` literals), counting template control-flow blocks (`@if`, `@else if`, `@for`, `@case`, `@defer (when ...)`, legacy `*ngIf`/`*ngFor`) plus ternary and logical operators inside bound attributes and `{{ }}` interpolations; and on synthetic `<component>` rollup findings whose `cyclomatic` is the worst class method's score plus the template's. Ranking and `--targets` use the rollup total; JSON exposes the per-half breakdown under `component_rollup`.",
        docs_path: "explanations/health#cyclomatic-complexity",
    },
    RuleDef {
        id: "fallow/high-cognitive-complexity",
        category: "Health",
        name: "High Cognitive Complexity",
        short: "Function has high cognitive complexity",
        full: "SonarSource cognitive complexity exceeds the configured threshold. Unlike cyclomatic complexity, cognitive complexity penalizes nesting depth and non-linear control flow (breaks, continues, early returns). It measures how hard a function is to understand when reading sequentially. fallow also emits this rule on synthetic `<template>` findings (Angular .html templates and inline `@Component({ template: ... })` literals), where nesting penalties accumulate on stacked `@if`/`@for`/`@switch` blocks; and on synthetic `<component>` rollup findings whose `cognitive` is the worst class method's score plus the template's. Ranking and `--targets` use the rollup total; JSON exposes the per-half breakdown under `component_rollup`.",
        docs_path: "explanations/health#cognitive-complexity",
    },
    RuleDef {
        id: "fallow/high-complexity",
        category: "Health",
        name: "High Complexity (Both)",
        short: "Function exceeds both complexity thresholds",
        full: "Function exceeds both cyclomatic and cognitive complexity thresholds. This is the strongest signal that a function needs refactoring, it has many paths AND is hard to understand. The same rule fires on synthetic `<template>` findings (Angular .html templates and inline `@Component({ template: ... })` literals) when both metrics exceed their thresholds, and on synthetic `<component>` rollup findings whose totals are the worst class method's score plus the template's. Ranking and `--targets` use the rollup totals; JSON exposes the per-half breakdown under `component_rollup`.",
        docs_path: "explanations/health#complexity-metrics",
    },
    RuleDef {
        id: "fallow/high-crap-score",
        category: "Health",
        name: "High CRAP Score",
        short: "Function has a high CRAP score (complexity combined with low coverage)",
        full: "The function's CRAP (Change Risk Anti-Patterns) score meets or exceeds the configured threshold. CRAP combines cyclomatic complexity with test coverage using the Savoia and Evans (2007) formula: `CC^2 * (1 - coverage/100)^3 + CC`. High CRAP indicates changes to this function carry high risk because it is complex AND poorly tested. Pair with `--coverage` for accurate per-function scoring; without it fallow estimates coverage from the module graph.",
        docs_path: "explanations/health#crap-score",
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

pub const DUPES_RULES: &[RuleDef] = &[RuleDef {
    id: "fallow/code-duplication",
    category: "Duplication",
    name: "Code Duplication",
    short: "Duplicated code block",
    full: "A block of code that appears in multiple locations with identical or near-identical token sequences. Clone detection uses normalized token comparison: identifier names and literals are abstracted away in non-strict modes.",
    docs_path: "explanations/duplication#clone-groups",
}];

pub const FLAGS_RULES: &[RuleDef] = &[RuleDef {
    id: "fallow/feature-flag",
    category: "Flags",
    name: "Feature Flags",
    short: "Detected feature flag pattern",
    full: "A feature flag pattern detected by `fallow flags`: environment-variable checks, flag SDK calls (LaunchDarkly, Unleash, and similar), or config-object lookups. Long-lived flags accumulate dead branches; review old flags for retirement and pair with dead-code analysis to find branches that can no longer execute.",
    docs_path: "cli/flags",
}];

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
];

/// Build the `_meta` object for `fallow dead-code --format json --explain`.
#[must_use]
pub fn check_meta() -> Value {
    let rules: Value = CHECK_RULES
        .iter()
        .map(|r| {
            (
                r.id.replace("fallow/", ""),
                json!({
                    "name": r.name,
                    "description": r.full,
                    "docs": rule_docs_url(r)
                }),
            )
        })
        .collect::<serde_json::Map<String, Value>>()
        .into();

    json!({
        "docs": CHECK_DOCS,
        "rules": rules,
        "field_definitions": {
            "actions[]": ACTIONS_FIELD_DEFINITION,
            "actions[].auto_fixable": ACTIONS_AUTO_FIXABLE_FIELD_DEFINITION
        }
    })
}

/// Build the sectioned `_meta` object for bare `fallow --format json --explain`.
#[must_use]
pub fn combined_meta(include_check: bool, include_dupes: bool, include_health: bool) -> Value {
    let mut sections = serde_json::Map::new();
    if include_check {
        sections.insert("check".to_string(), check_meta());
    }
    if include_dupes {
        sections.insert("dupes".to_string(), dupes_meta());
    }
    if include_health {
        sections.insert("health".to_string(), health_meta());
    }
    Value::Object(sections)
}

/// Build the `_meta` object for `fallow health --format json --explain`.
#[must_use]
pub fn health_meta() -> Value {
    json!({
        "docs": HEALTH_DOCS,
        "field_definitions": {
            "actions[]": ACTIONS_FIELD_DEFINITION,
            "actions[].auto_fixable": ACTIONS_AUTO_FIXABLE_FIELD_DEFINITION
        },
        "metrics": health_metrics()
    })
}

fn health_metrics() -> Value {
    let mut metrics = serde_json::Map::new();
    for section in [
        health_size_complexity_metrics(),
        health_quality_metrics(),
        health_coupling_metrics(),
        health_render_fan_in_metrics(),
        health_churn_metrics(),
        health_refactoring_rank_metrics(),
        health_refactoring_confidence_metrics(),
        health_risk_metrics(),
        health_contributor_metrics(),
        health_ownership_metrics(),
        health_runtime_verdict_metrics(),
        health_runtime_observation_metrics(),
        health_runtime_production_metrics(),
    ] {
        let Value::Object(section) = section else {
            continue;
        };
        metrics.extend(section);
    }
    Value::Object(metrics)
}

fn health_size_complexity_metrics() -> Value {
    json!({
            "cyclomatic": {
                "name": "Cyclomatic Complexity",
                "description": "McCabe cyclomatic complexity: 1 + number of decision points (if/else, switch cases, loops, ternary, logical operators). Measures the number of independent paths through a function.",
                "range": "[1, \u{221e})",
                "interpretation": "lower is better; default threshold: 20"
            },
            "cognitive": {
                "name": "Cognitive Complexity",
                "description": "SonarSource cognitive complexity: penalizes nesting depth and non-linear control flow (breaks, continues, early returns). Measures how hard a function is to understand when reading top-to-bottom.",
                "range": "[0, \u{221e})",
                "interpretation": "lower is better; default threshold: 15"
            },
            "line_count": {
                "name": "Function Line Count",
                "description": "Number of lines in the function body.",
                "range": "[1, \u{221e})",
                "interpretation": "context-dependent; long functions may need splitting"
            },
            "lines": {
                "name": "File Line Count",
                "description": "Total lines of code in the file (from line offsets). Provides scale context for other metrics: a file with 0.4 complexity density at 80 LOC is different from 0.4 density at 800 LOC.",
                "range": "[1, \u{221e})",
                "interpretation": "context-dependent; large files may benefit from splitting even if individual functions are small"
            }
    })
}

fn health_quality_metrics() -> Value {
    json!({
            "maintainability_index": {
                "name": "Maintainability Index",
                "description": "Composite score: 100 - (complexity_density \u{00d7} 30 \u{00d7} dampening) - (dead_code_ratio \u{00d7} 20) - min(ln(fan_out+1) \u{00d7} 4, 15), where dampening = min(lines/50, 1.0). Clamped to [0, 100]. Higher is better.",
                "range": "[0, 100]",
                "interpretation": "higher is better; <40 poor, 40\u{2013}70 moderate, >70 good"
            },
            "complexity_density": {
                "name": "Complexity Density",
                "description": "Total cyclomatic complexity divided by lines of code. Measures how densely complex the code is per line.",
                "range": "[0, \u{221e})",
                "interpretation": "lower is better; >1.0 indicates very dense complexity"
            },
            "dead_code_ratio": {
                "name": "Dead Code Ratio",
                "description": "Fraction of value exports (excluding type-only exports like interfaces and type aliases) with zero references across the project.",
                "range": "[0, 1]",
                "interpretation": "lower is better; 0 = all exports are used"
            }
    })
}

fn health_coupling_metrics() -> Value {
    json!({
            "fan_in": {
                "name": "Fan-in (Importers)",
                "description": "Number of files that import this file. High fan-in means high blast radius \u{2014} changes to this file affect many dependents.",
                "range": "[0, \u{221e})",
                "interpretation": "context-dependent; high fan-in files need careful review before changes"
            },
            "fan_out": {
                "name": "Fan-out (Imports)",
                "description": "Number of files this file directly imports. High fan-out indicates high coupling and change propagation risk.",
                "range": "[0, \u{221e})",
                "interpretation": "lower is better; MI penalty caps at ~40 imports"
            },
    })
}

fn health_render_fan_in_metrics() -> Value {
    json!({
            "max_render_fan_in": {
                "name": "Render Fan-in (Blast Radius)",
                "description": "DESCRIPTIVE, NOT A RULE. The component-graph analogue of module fan-in: where module fan-in counts importing MODULES, render fan-in counts distinct render LOCATIONS of a React/Preact component (a shared <Button> is rendered in far more places than it is imported). The headline `max_render_fan_in` is the highest DISTINCT-PARENTS count across components (the honest edit-ripple count); each top component also reports `render_sites` as secondary \u{201c}incl. repeats\u{201d} context (one parent rendering a child five times is five sites but one distinct parent). Test / spec / story / fixture files are excluded (a test rendering <Page> 146 times is not blast radius). Undercount-safe: a child rendered via a JSX spread / dynamic / member-expression tag is not resolved, so a high-fan-in component can only be undersold. Computed only on React/Preact projects; absent otherwise.",
                "range": "[0, \u{221e})",
                "interpretation": "context-dependent; a high distinct-parents component edit-ripples to many render locations. Descriptive only, never a gate or finding"
            }
    })
}

fn health_churn_metrics() -> Value {
    json!({
            "score": {
                "name": "Hotspot Score",
                "description": "normalized_churn \u{00d7} normalized_complexity \u{00d7} 100, where normalization is against the project maximum. Identifies files that are both complex AND frequently changing.",
                "range": "[0, 100]",
                "interpretation": "higher = riskier; prioritize refactoring high-score files"
            },
            "weighted_commits": {
                "name": "Weighted Commits",
                "description": "Recency-weighted commit count using exponential decay with 90-day half-life. Recent commits contribute more than older ones.",
                "range": "[0, \u{221e})",
                "interpretation": "higher = more recent churn activity"
            },
            "trend": {
                "name": "Churn Trend",
                "description": "Compares recent vs older commit frequency within the analysis window. accelerating = recent > 1.5\u{00d7} older, cooling = recent < 0.67\u{00d7} older, stable = in between.",
                "values": ["accelerating", "stable", "cooling"],
                "interpretation": "accelerating files need attention; cooling files are stabilizing"
            }
    })
}

fn health_refactoring_rank_metrics() -> Value {
    json!({
            "priority": {
                "name": "Refactoring Priority",
                "description": "Weighted score: complexity density (30%), hotspot boost (25%), dead code ratio (20%), fan-in (15%), fan-out (10%). Fan-in and fan-out normalization uses adaptive percentile-based thresholds (p95 of the project distribution). Does not use the maintainability index to avoid double-counting.",
                "range": "[0, 100]",
                "interpretation": "higher = more urgent to refactor"
            },
            "efficiency": {
                "name": "Efficiency Score",
                "description": "priority / effort_numeric (Low=1, Medium=2, High=3). Surfaces quick wins: high-priority, low-effort targets rank first. Default sort order.",
                "range": "[0, 100] \u{2014} effective max depends on effort: Low=100, Medium=50, High\u{2248}33",
                "interpretation": "higher = better quick-win value; targets are sorted by efficiency descending"
            },
            "effort": {
                "name": "Effort Estimate",
                "description": "Heuristic effort estimate based on file size, function count, and fan-in. Thresholds adapt to the project\u{2019}s distribution (percentile-based). Low: small file, few functions, low fan-in. High: large file, high fan-in, or many functions with high density. Medium: everything else.",
                "values": ["low", "medium", "high"],
                "interpretation": "low = quick win, high = needs planning and coordination"
            },
    })
}

fn health_refactoring_confidence_metrics() -> Value {
    json!({
            "confidence": {
                "name": "Confidence Level",
                "description": "Reliability of the recommendation based on data source. High: deterministic graph/AST analysis (dead code, circular deps, complexity). Medium: heuristic thresholds (fan-in/fan-out coupling). Low: depends on git history quality (churn-based recommendations).",
                "values": ["high", "medium", "low"],
                "interpretation": "high = act on it, medium = verify context, low = treat as a signal, not a directive"
            },
            "health_score": {
                "name": "Health Score",
                "description": "Project-level aggregate score computed from vital signs: dead code, complexity, maintainability, hotspots, unused dependencies, and circular dependencies. Penalties subtracted from 100. Missing metrics (from pipelines that didn't run) don't penalize. Use --score to compute the score; add --hotspots, or --targets with --score, when the score should include the churn-backed hotspot penalty.",
                "range": "[0, 100]",
                "interpretation": "higher is better; A (85\u{2013}100), B (70\u{2013}84), C (55\u{2013}69), D (40\u{2013}54), F (0\u{2013}39)"
            }
    })
}

fn health_risk_metrics() -> Value {
    json!({
            "crap_max": {
                "name": "Untested Complexity Risk (CRAP)",
                "description": "Change Risk Anti-Patterns score (Savoia & Evans, 2007). Formula: CC\u{00b2} \u{00d7} (1 - cov/100)\u{00b3} + CC. Default model (static_estimated): estimates per-function coverage from export references \u{2014} directly test-referenced exports get 85%, indirectly test-reachable functions get 40%, untested files get 0%. Provide --coverage <path> with Istanbul-format coverage-final.json (from Jest, Vitest, c8, nyc) for exact per-function CRAP scores.",
                "range": "[1, \u{221e})",
                "interpretation": "lower is better; >=30 is high-risk (CC >= 5 without test path)"
            },
    })
}

fn health_contributor_metrics() -> Value {
    json!({
            "bus_factor": {
                "name": "Bus Factor",
                "description": "Avelino truck factor: the minimum number of distinct contributors who together account for at least 50% of recency-weighted commits to this file in the analysis window. Bot authors are excluded.",
                "range": "[1, \u{221e})",
                "interpretation": "lower is higher knowledge-loss risk; 1 means a single contributor covers most of the recent history"
            },
            "contributor_count": {
                "name": "Contributor Count",
                "description": "Number of distinct authors who touched this file in the analysis window after bot-pattern filtering.",
                "range": "[0, \u{221e})",
                "interpretation": "higher generally indicates broader knowledge spread; pair with bus_factor for context"
            },
            "share": {
                "name": "Contributor Share",
                "description": "Recency-weighted share of total weighted commits attributed to a single contributor. Rounded to three decimals.",
                "range": "[0, 1]",
                "interpretation": "share close to 1.0 indicates dominance and pairs with low bus_factor"
            },
    })
}

fn health_ownership_metrics() -> Value {
    json!({
            "stale_days": {
                "name": "Stale Days",
                "description": "Days since this contributor last touched the file. Computed at analysis time.",
                "range": "[0, \u{221e})",
                "interpretation": "high stale_days on the top contributor often correlates with ownership drift"
            },
            "drift": {
                "name": "Ownership Drift",
                "description": "True when the file's original author (earliest first commit in the window) differs from the current top contributor, the file is at least 30 days old, and the original author's recency-weighted share is below 10%.",
                "values": [true, false],
                "interpretation": "true means the original author is no longer maintaining; route reviews to the current top contributor"
            },
            "unowned": {
                "name": "Unowned (Tristate)",
                "description": "true = a CODEOWNERS file exists but no rule matches this file; false = a rule matches; null = no CODEOWNERS file was discovered for the repository (cannot determine).",
                "values": [true, false, null],
                "interpretation": "true on a hotspot is a review-bottleneck risk; null means the signal is unavailable, not absent"
            }
    })
}

fn health_runtime_verdict_metrics() -> Value {
    json!({
            "runtime_coverage_verdict": {
                "name": "Runtime Coverage Verdict",
                "description": "Overall verdict across all runtime-coverage findings. `clean` = nothing cold; `cold-code-detected` = one or more tracked functions had zero invocations; `hot-path-touched` = a function modified in the current change set is on the hot path (requires `--diff-file` or `--changed-since` to fire; without a change scope the verdict cannot promote); `license-expired-grace` = analysis ran but the license is in its post-expiry grace window; `unknown` = verdict could not be computed (degenerate input).",
                "values": ["clean", "hot-path-touched", "cold-code-detected", "license-expired-grace", "unknown"],
                "interpretation": "`cold-code-detected` is the primary actionable signal in standalone analysis; `hot-path-touched` is promoted to primary in PR context (when a change scope is supplied) so reviewers see the diff-tied signal first. `signals[]` carries the full unprioritized set."
            },
    })
}

fn health_runtime_observation_metrics() -> Value {
    json!({
            "runtime_coverage_state": {
                "name": "Runtime Coverage State",
                "description": "Per-function observation: `called` = V8 saw at least one invocation; `never-called` = V8 tracked the function but it never ran; `coverage-unavailable` = the function was not in the V8 tracking set (e.g., lazy-parsed, worker thread, dynamic code); `unknown` = forward-compat sentinel for newer sidecar states.",
                "values": ["called", "never-called", "coverage-unavailable", "unknown"],
                "interpretation": "`never-called` in combination with static `unused` is the highest-confidence delete signal"
            },
            "runtime_coverage_confidence": {
                "name": "Runtime Coverage Confidence",
                "description": "Confidence in a runtime-coverage finding. `high` = tracked by V8 with a statistically meaningful observation volume; `medium` = either low observation volume or indirect evidence; `low` = minimal data; `unknown` = insufficient information to classify.",
                "values": ["high", "medium", "low", "unknown"],
                "interpretation": "high = act on it; medium = verify context; low = treat as a signal only"
            }
    })
}

fn health_runtime_production_metrics() -> Value {
    json!({
            "production_invocations": {
                "name": "Production Invocations",
                "description": "Observed invocation count for the function over the collected coverage window. For `coverage-unavailable` findings this is `0` and semantically means `null` (not tracked). Absolute counts are not directly comparable across services without normalizing by trace_count.",
                "range": "[0, \u{221e})",
                "interpretation": "0 + tracked = cold path; 0 + untracked = unknown; high + never-called cannot occur by definition"
            },
            "percent_dead_in_production": {
                "name": "Percent Dead in Production",
                "description": "Fraction of tracked functions with zero observed invocations, multiplied by 100. Computed before any `--top` truncation so the summary total is stable regardless of display limits.",
                "range": "[0, 100]",
                "interpretation": "lower is better; values above ~10% on a long-running service indicate a large cleanup opportunity"
            }
    })
}

/// Build the `_meta` object for `fallow dupes --format json --explain`.
#[must_use]
pub fn dupes_meta() -> Value {
    json!({
        "docs": DUPES_DOCS,
        "field_definitions": {
            "actions[]": ACTIONS_FIELD_DEFINITION,
            "actions[].auto_fixable": ACTIONS_AUTO_FIXABLE_FIELD_DEFINITION
        },
        "metrics": {
            "duplication_percentage": {
                "name": "Duplication Percentage",
                "description": "Fraction of total source tokens that appear in at least one clone group. Computed over the full analyzed file set.",
                "range": "[0, 100]",
                "interpretation": "lower is better"
            },
            "token_count": {
                "name": "Token Count",
                "description": "Number of normalized source tokens in the clone group. Tokens are language-aware (keywords, identifiers, operators, punctuation). Higher token count = larger duplicate.",
                "range": "[1, \u{221e})",
                "interpretation": "larger clones have higher refactoring value"
            },
            "line_count": {
                "name": "Line Count",
                "description": "Number of source lines spanned by the clone instance. Approximation of clone size for human readability.",
                "range": "[1, \u{221e})",
                "interpretation": "larger clones are more impactful to deduplicate"
            },
            "clone_groups": {
                "name": "Clone Groups",
                "description": "A set of code fragments with identical or near-identical normalized token sequences. Each group has 2+ instances across different locations.",
                "interpretation": "each group is a single refactoring opportunity"
            },
            "clone_groups_below_min_occurrences": {
                "name": "Clone Groups Below minOccurrences",
                "description": "Number of clone groups detected but hidden by the `duplicates.minOccurrences` filter. Always 0 (or absent) when the filter is at its default of 2. Pre-filter group count = `clone_groups + clone_groups_below_min_occurrences`.",
                "range": "[0, \u{221e})",
                "interpretation": "high values suggest noisy pair-only duplication; lower `minOccurrences` to inspect"
            },
            "clone_families": {
                "name": "Clone Families",
                "description": "Groups of clone groups that share the same set of files. Indicates systematic duplication patterns (e.g., mirrored directory structures).",
                "interpretation": "families suggest extract-module refactoring opportunities"
            }
        }
    })
}

/// Build the `_meta` object for `fallow security --format json --explain`.
#[must_use]
pub fn security_meta() -> Meta {
    let rules = SECURITY_RULES
        .iter()
        .map(|rule| {
            (
                rule.id.to_string(),
                MetaRule {
                    name: Some(rule.name.to_string()),
                    description: Some(rule.full.to_string()),
                    docs: Some(rule_docs_url(rule)),
                },
            )
        })
        .collect();

    Meta {
        docs: Some(SECURITY_DOCS.to_string()),
        telemetry: None,
        field_definitions: BTreeMap::from([
            (
                "version".to_string(),
                "fallow CLI version that produced this output.".to_string(),
            ),
            (
                "elapsed_ms".to_string(),
                "Wall-clock milliseconds spent producing the security report.".to_string(),
            ),
            (
                "config".to_string(),
                "Privacy-safe config context relevant to security candidate generation."
                    .to_string(),
            ),
            (
                "config.rules.*.configured".to_string(),
                "Severity from resolved config before the security command forced default-off rules on."
                    .to_string(),
            ),
            (
                "config.rules.*.effective".to_string(),
                "Severity used for this security command run.".to_string(),
            ),
            (
                "config.categories_include".to_string(),
                "Configured security category include list. null means unset, [] means explicitly empty."
                    .to_string(),
            ),
            (
                "config.categories_exclude".to_string(),
                "Configured security category exclude list. null means unset, [] means explicitly empty."
                    .to_string(),
            ),
            (
                "security_findings[]".to_string(),
                "Unverified security candidates for downstream human or agent verification."
                    .to_string(),
            ),
            (
                "summary.security_findings".to_string(),
                "Number of security candidates after all filters, gates, and scopes.".to_string(),
            ),
            (
                "summary.by_severity".to_string(),
                "Fixed high, medium, and low severity counts for summary JSON.".to_string(),
            ),
            (
                "summary.by_category".to_string(),
                "Candidate counts by catalogue category, or by kind for uncategorized findings."
                    .to_string(),
            ),
            (
                "summary.by_reachability".to_string(),
                "Fixed reachability and source-backed ranking-signal counts for summary JSON."
                    .to_string(),
            ),
            (
                "summary.by_runtime_state".to_string(),
                "Fixed production-runtime coverage state counts for summary JSON.".to_string(),
            ),
            (
                "unresolved_edge_files".to_string(),
                "Number of client files whose import cone contains dynamic edges the graph could not follow."
                    .to_string(),
            ),
            (
                "unresolved_callee_sites".to_string(),
                "Number of sink-shaped nodes whose callee could not be flattened to a static path."
                    .to_string(),
            ),
        ]),
        metrics: BTreeMap::new(),
        rules,
    }
}

/// Build the `_meta` object for `fallow coverage setup --json --explain`.
#[must_use]
pub fn coverage_setup_meta() -> Value {
    json!({
        "docs_url": COVERAGE_SETUP_DOCS,
        "field_definitions": {
            "schema_version": "Coverage setup JSON contract version. Stays at \"1\" for additive opt-in fields such as _meta.",
            "framework_detected": "Primary detected runtime framework for compatibility with single-app consumers. In workspaces this mirrors the first emitted runtime member; unknown means no runtime member was detected.",
            "package_manager": "Detected package manager used for install and run commands, or null when no package manager signal was found.",
            "runtime_targets": "Union of runtime targets across emitted members.",
            "members[]": "Per-runtime-workspace setup recipes. Pure aggregator roots and build-only libraries are omitted.",
            "members[].name": "Workspace package name from package.json, or the root directory name when package.json has no name.",
            "members[].path": "Workspace path relative to the command root. The root package is represented as \".\".",
            "members[].framework_detected": "Runtime framework detected for that member.",
            "members[].package_manager": "Package manager detected for that member, or inherited from the workspace root when no member-specific signal exists.",
            "members[].runtime_targets": "Runtime targets produced by that member.",
            "members[].files_to_edit": "Files in that member that should receive runtime beacon setup code.",
            "members[].snippets": "Copy-paste setup snippets for that member, with paths relative to the command root.",
            "members[].dockerfile_snippet": "Environment snippet for file-system capture in that member's containerized Node runtime, or null when not applicable.",
            "members[].warnings": "Actionable setup caveats discovered for that member.",
            "config_written": "Always null for --json because JSON setup is side-effect-free and never writes configuration.",
            "files_to_edit": "Compatibility copy of the primary member's files, with workspace prefixes when the primary member is not the root.",
            "snippets": "Compatibility copy of the primary member's snippets, with workspace prefixes when the primary member is not the root.",
            "dockerfile_snippet": "Environment snippet for file-system capture in containerized Node runtimes, or null when not applicable.",
            "commands": "Package-manager commands needed to install the runtime beacon and sidecar packages.",
            "next_steps": "Ordered setup workflow after applying the emitted snippets.",
            "warnings": "Actionable setup caveats discovered while building the recipe."
        },
        "enums": {
            "framework_detected": ["nextjs", "nestjs", "nuxt", "sveltekit", "astro", "remix", "vite", "plain_node", "unknown"],
            "runtime_targets": ["node", "browser"],
            "package_manager": ["npm", "pnpm", "yarn", "bun", null]
        },
        "warnings": {
            "No runtime workspace members were detected": "The root appears to be a workspace, but no runtime-bearing package was found. The payload emits install commands only.",
            "No local coverage artifact was detected yet": "Run the application with runtime coverage collection enabled, then re-run setup or health with the produced capture path.",
            "Package manager was not detected": "No packageManager field or known lockfile was found. Commands fall back to npm.",
            "Framework was not detected": "No known framework dependency or runtime script was found. Treat the recipe as a generic Node setup and adjust the entry path as needed."
        }
    })
}

/// Build the `_meta` object for `fallow coverage analyze --format json --explain`.
#[must_use]
pub fn coverage_analyze_meta() -> Value {
    json!({
        "docs_url": COVERAGE_ANALYZE_DOCS,
        "field_definitions": {
            "schema_version": "Standalone coverage analyze envelope version. \"1\" for the current shape.",
            "version": "fallow CLI version that produced this output.",
            "elapsed_ms": "Wall-clock milliseconds spent producing the report.",
            "runtime_coverage": "Same RuntimeCoverageReport block emitted by `fallow health --runtime-coverage`.",
            "runtime_coverage.summary.data_source": "Which evidence source produced the report. local = on-disk artifact via --runtime-coverage <path>; cloud = explicit pull via --cloud / --runtime-coverage-cloud / FALLOW_RUNTIME_COVERAGE_SOURCE=cloud.",
            "runtime_coverage.summary.last_received_at": "ISO-8601 timestamp of the newest runtime payload included in the report. Null for local artifacts that do not carry receipt metadata.",
            "runtime_coverage.summary.capture_quality": "Capture-window telemetry derived from the runtime evidence. lazy_parse_warning trips when more than 30% of tracked functions are V8-untracked, which usually indicates a short observation window.",
            "runtime_coverage.findings[].id": "Per-finding SUPPRESSION key (fallow:prod:<hash>). Hashes file + function + the current line, so it changes when the function moves. Use it to suppress one finding at its current location.",
            "runtime_coverage.findings[].stable_id": "Cross-surface JOIN key (fallow:fn:<hash>) from fallow_cov_protocol::function_identity_id, hashing file + name + start_line. The same function shares ONE value across findings, hot paths, blast-radius, and importance entries (the per-finding id uses a per-surface salt and differs), and across V8/Istanbul/oxc producers (columns are excluded from the hash). Like id, it changes when the function's file, name, or start line changes: it is a cross-surface/cross-producer join key, NOT a line-move-immune one. Omitted from the JSON entirely (not emitted as null) when the producing surface or an un-migrated cloud supplied no FunctionIdentity. New baselines key on this when present to align with the cross-surface join key; the grace-window reader accepts the legacy id too.",
            "runtime_coverage._matching": "Function-identity fallback order when joining runtime evidence to local static analysis: (1) exact stable_id match (fallow:fn:<hash>) when both sides carry one; (2) exact (path, name, start_line); (3) fuzzy nearest candidate within a line tolerance. Baseline suppression accepts BOTH the stable_id and the legacy fallow:prod: id during the grace window, so baselines written before this version keep suppressing.",
            "runtime_coverage.findings[].evidence.static_status": "used = the function is reachable in the AST module graph; unused = it is dead by static analysis.",
            "runtime_coverage.findings[].evidence.test_coverage": "covered = the local test suite hits the function; not_covered otherwise.",
            "runtime_coverage.findings[].evidence.v8_tracking": "tracked = V8 observed the function during the capture window; untracked otherwise.",
            "runtime_coverage.findings[].actions[].type": "Suggested follow-up identifier. delete-cold-code is emitted on safe_to_delete; review-runtime on review_required.",
            "runtime_coverage.blast_radius[]": "First-class blast-radius entries with stable fallow:blast IDs, static caller count, traffic-weighted caller reach, optional cloud deploy touch count, and low/medium/high risk band.",
            "runtime_coverage.importance[]": "First-class production-importance entries with stable fallow:importance IDs, invocations, cyclomatic complexity, owner count, 0-100 importance score, and templated reason.",
            "runtime_coverage.warnings[].code": "Stable warning identifier. cloud_functions_unmatched flags entries dropped because no AST/static counterpart was found locally."
        },
        "enums": {
            "data_source": ["local", "cloud"],
            "report_verdict": ["clean", "hot-path-touched", "cold-code-detected", "license-expired-grace", "unknown"],
            "finding_verdict": ["safe_to_delete", "review_required", "coverage_unavailable", "low_traffic", "active", "unknown"],
            "static_status": ["used", "unused"],
            "test_coverage": ["covered", "not_covered"],
            "v8_tracking": ["tracked", "untracked"],
            "action_type": ["delete-cold-code", "review-runtime"]
        },
        "warnings": {
            "no_runtime_data": "Cloud returned an empty runtime window. Either the period is too narrow or no traces have been ingested yet.",
            "cloud_functions_unmatched": "One or more cloud-side functions could not be matched against the local AST/static index and were dropped from findings. Common causes: stale runtime data after a rename/move, file path mismatch between deploy and repo, or analysis run on the wrong commit."
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_by_id_finds_check_rule() {
        let rule = rule_by_id("fallow/unused-file").unwrap();
        assert_eq!(rule.name, "Unused Files");
    }

    #[test]
    fn rule_by_id_finds_health_rule() {
        let rule = rule_by_id("fallow/high-cyclomatic-complexity").unwrap();
        assert_eq!(rule.name, "High Cyclomatic Complexity");
    }

    #[test]
    fn rule_by_id_finds_dupes_rule() {
        let rule = rule_by_id("fallow/code-duplication").unwrap();
        assert_eq!(rule.name, "Code Duplication");
    }

    #[test]
    fn rule_by_id_finds_security_rule() {
        let rule = rule_by_id("security/tainted-sink").unwrap();
        assert_eq!(rule.name, "Tainted Sink Candidates");
    }

    #[test]
    fn rule_by_id_returns_none_for_unknown() {
        assert!(rule_by_id("fallow/nonexistent").is_none());
        assert!(rule_by_id("").is_none());
    }

    #[test]
    fn rule_docs_url_format() {
        let rule = rule_by_id("fallow/unused-export").unwrap();
        let url = rule_docs_url(rule);
        assert!(url.starts_with("https://docs.fallow.tools/"));
        assert!(url.contains("unused-exports"));
    }

    #[test]
    fn check_rules_all_have_fallow_prefix() {
        for rule in CHECK_RULES {
            assert!(
                rule.id.starts_with("fallow/"),
                "rule {} should start with fallow/",
                rule.id
            );
        }
    }

    #[test]
    fn check_rules_all_have_docs_path() {
        for rule in CHECK_RULES {
            assert!(
                !rule.docs_path.is_empty(),
                "rule {} should have a docs_path",
                rule.id
            );
        }
    }

    #[test]
    fn check_rules_no_duplicate_ids() {
        let mut seen = rustc_hash::FxHashSet::default();
        for rule in CHECK_RULES
            .iter()
            .chain(HEALTH_RULES)
            .chain(DUPES_RULES)
            .chain(FLAGS_RULES)
            .chain(SECURITY_RULES)
        {
            assert!(seen.insert(rule.id), "duplicate rule id: {}", rule.id);
        }
    }

    #[test]
    fn check_meta_has_docs_and_rules() {
        let meta = check_meta();
        assert!(meta.get("docs").is_some());
        assert!(meta.get("rules").is_some());
        let rules = meta["rules"].as_object().unwrap();
        assert_eq!(rules.len(), CHECK_RULES.len());
        assert!(rules.contains_key("unused-file"));
        assert!(rules.contains_key("unused-export"));
        assert!(rules.contains_key("unused-type"));
        assert!(rules.contains_key("unused-dependency"));
        assert!(rules.contains_key("unused-dev-dependency"));
        assert!(rules.contains_key("unused-optional-dependency"));
        assert!(rules.contains_key("unused-enum-member"));
        assert!(rules.contains_key("unused-class-member"));
        assert!(rules.contains_key("unresolved-import"));
        assert!(rules.contains_key("unlisted-dependency"));
        assert!(rules.contains_key("duplicate-export"));
        assert!(rules.contains_key("type-only-dependency"));
        assert!(rules.contains_key("circular-dependency"));
    }

    #[test]
    fn check_meta_documents_per_finding_auto_fixable() {
        let meta = check_meta();
        let defs = meta["field_definitions"].as_object().unwrap();
        let note = defs["actions[].auto_fixable"].as_str().unwrap();
        assert!(
            note.contains("PER FINDING"),
            "auto_fixable note must call out per-finding evaluation"
        );
        assert!(
            note.contains("remove-catalog-entry"),
            "auto_fixable note must cite remove-catalog-entry per-instance flip"
        );
        assert!(
            note.contains("used_in_workspaces"),
            "auto_fixable note must cite the dependency-action per-instance flip"
        );
        assert!(
            note.contains("ignoreExports"),
            "auto_fixable note must cite the duplicate-exports config-fixable flip"
        );
        assert!(defs.contains_key("actions[]"));
    }

    #[test]
    fn health_and_dupes_meta_share_actions_field_definitions() {
        for meta in [health_meta(), dupes_meta()] {
            let defs = meta["field_definitions"].as_object().unwrap();
            assert_eq!(
                defs["actions[]"].as_str().unwrap(),
                ACTIONS_FIELD_DEFINITION,
            );
            assert_eq!(
                defs["actions[].auto_fixable"].as_str().unwrap(),
                ACTIONS_AUTO_FIXABLE_FIELD_DEFINITION,
            );
        }
    }

    #[test]
    fn check_meta_rule_has_required_fields() {
        let meta = check_meta();
        let rules = meta["rules"].as_object().unwrap();
        for (key, value) in rules {
            assert!(value.get("name").is_some(), "rule {key} missing 'name'");
            assert!(
                value.get("description").is_some(),
                "rule {key} missing 'description'"
            );
            assert!(value.get("docs").is_some(), "rule {key} missing 'docs'");
        }
    }

    #[test]
    fn health_meta_has_metrics() {
        let meta = health_meta();
        assert!(meta.get("docs").is_some());
        let metrics = meta["metrics"].as_object().unwrap();
        assert!(metrics.contains_key("cyclomatic"));
        assert!(metrics.contains_key("cognitive"));
        assert!(metrics.contains_key("maintainability_index"));
        assert!(metrics.contains_key("complexity_density"));
        assert!(metrics.contains_key("fan_in"));
        assert!(metrics.contains_key("fan_out"));
    }

    #[test]
    fn dupes_meta_has_metrics() {
        let meta = dupes_meta();
        assert!(meta.get("docs").is_some());
        let metrics = meta["metrics"].as_object().unwrap();
        assert!(metrics.contains_key("duplication_percentage"));
        assert!(metrics.contains_key("token_count"));
        assert!(metrics.contains_key("clone_groups"));
        assert!(metrics.contains_key("clone_families"));
    }

    #[test]
    fn coverage_setup_meta_has_docs_fields_enums_and_warnings() {
        let meta = coverage_setup_meta();
        assert_eq!(meta["docs_url"], COVERAGE_SETUP_DOCS);
        assert!(
            meta["field_definitions"]
                .as_object()
                .unwrap()
                .contains_key("members[]")
        );
        assert!(
            meta["field_definitions"]
                .as_object()
                .unwrap()
                .contains_key("config_written")
        );
        assert!(
            meta["field_definitions"]
                .as_object()
                .unwrap()
                .contains_key("members[].package_manager")
        );
        assert!(
            meta["field_definitions"]
                .as_object()
                .unwrap()
                .contains_key("members[].warnings")
        );
        assert!(
            meta["enums"]
                .as_object()
                .unwrap()
                .contains_key("framework_detected")
        );
        assert!(
            meta["warnings"]
                .as_object()
                .unwrap()
                .contains_key("No runtime workspace members were detected")
        );
        assert!(
            meta["warnings"]
                .as_object()
                .unwrap()
                .contains_key("Package manager was not detected")
        );
    }

    #[test]
    fn coverage_analyze_meta_documents_data_source_and_action_vocabulary() {
        let meta = coverage_analyze_meta();
        assert_eq!(meta["docs_url"], COVERAGE_ANALYZE_DOCS);
        let fields = meta["field_definitions"].as_object().unwrap();
        assert!(fields.contains_key("runtime_coverage.summary.data_source"));
        assert!(fields.contains_key("runtime_coverage.summary.last_received_at"));
        assert!(fields.contains_key("runtime_coverage.findings[].evidence.test_coverage"));
        assert!(fields.contains_key("runtime_coverage.findings[].actions[].type"));
        let enums = meta["enums"].as_object().unwrap();
        assert_eq!(enums["data_source"], json!(["local", "cloud"]));
        assert_eq!(enums["test_coverage"], json!(["covered", "not_covered"]));
        assert_eq!(enums["v8_tracking"], json!(["tracked", "untracked"]));
        assert_eq!(
            enums["action_type"],
            json!(["delete-cold-code", "review-runtime"])
        );
        let warnings = meta["warnings"].as_object().unwrap();
        assert!(warnings.contains_key("cloud_functions_unmatched"));
    }

    #[test]
    fn health_rules_all_have_fallow_prefix() {
        for rule in HEALTH_RULES {
            assert!(
                rule.id.starts_with("fallow/"),
                "health rule {} should start with fallow/",
                rule.id
            );
        }
    }

    #[test]
    fn health_rules_all_have_docs_path() {
        for rule in HEALTH_RULES {
            assert!(
                !rule.docs_path.is_empty(),
                "health rule {} should have a docs_path",
                rule.id
            );
        }
    }

    #[test]
    fn health_rules_all_have_non_empty_fields() {
        for rule in HEALTH_RULES {
            assert!(
                !rule.name.is_empty(),
                "health rule {} missing name",
                rule.id
            );
            assert!(
                !rule.short.is_empty(),
                "health rule {} missing short description",
                rule.id
            );
            assert!(
                !rule.full.is_empty(),
                "health rule {} missing full description",
                rule.id
            );
        }
    }

    #[test]
    fn dupes_rules_all_have_fallow_prefix() {
        for rule in DUPES_RULES {
            assert!(
                rule.id.starts_with("fallow/"),
                "dupes rule {} should start with fallow/",
                rule.id
            );
        }
    }

    #[test]
    fn dupes_rules_all_have_docs_path() {
        for rule in DUPES_RULES {
            assert!(
                !rule.docs_path.is_empty(),
                "dupes rule {} should have a docs_path",
                rule.id
            );
        }
    }

    #[test]
    fn dupes_rules_all_have_non_empty_fields() {
        for rule in DUPES_RULES {
            assert!(!rule.name.is_empty(), "dupes rule {} missing name", rule.id);
            assert!(
                !rule.short.is_empty(),
                "dupes rule {} missing short description",
                rule.id
            );
            assert!(
                !rule.full.is_empty(),
                "dupes rule {} missing full description",
                rule.id
            );
        }
    }

    #[test]
    fn security_rules_all_have_security_prefix() {
        for rule in SECURITY_RULES {
            assert!(
                rule.id.starts_with("security/"),
                "security rule {} should start with security/",
                rule.id
            );
        }
    }

    #[test]
    fn security_rules_all_have_docs_path() {
        for rule in SECURITY_RULES {
            assert_eq!(
                rule.docs_path, "cli/security",
                "security rule {} should point at security docs",
                rule.id
            );
        }
    }

    #[test]
    fn security_rules_all_have_non_empty_fields() {
        for rule in SECURITY_RULES {
            assert!(
                !rule.name.is_empty(),
                "security rule {} missing name",
                rule.id
            );
            assert!(
                !rule.short.is_empty(),
                "security rule {} missing short description",
                rule.id
            );
            assert!(
                !rule.full.is_empty(),
                "security rule {} missing full description",
                rule.id
            );
        }
    }

    #[test]
    fn check_rules_all_have_non_empty_fields() {
        for rule in CHECK_RULES {
            assert!(!rule.name.is_empty(), "check rule {} missing name", rule.id);
            assert!(
                !rule.short.is_empty(),
                "check rule {} missing short description",
                rule.id
            );
            assert!(
                !rule.full.is_empty(),
                "check rule {} missing full description",
                rule.id
            );
        }
    }

    #[test]
    fn rule_docs_url_health_rule() {
        let rule = rule_by_id("fallow/high-cyclomatic-complexity").unwrap();
        let url = rule_docs_url(rule);
        assert!(url.starts_with("https://docs.fallow.tools/"));
        assert!(url.contains("health"));
    }

    #[test]
    fn rule_docs_url_dupes_rule() {
        let rule = rule_by_id("fallow/code-duplication").unwrap();
        let url = rule_docs_url(rule);
        assert!(url.starts_with("https://docs.fallow.tools/"));
        assert!(url.contains("duplication"));
    }

    #[test]
    fn rule_docs_url_security_rule() {
        let rule = rule_by_id("security/sql-injection").unwrap();
        let url = rule_docs_url(rule);
        assert_eq!(url, "https://docs.fallow.tools/cli/security");
    }

    #[test]
    fn health_meta_all_metrics_have_name_and_description() {
        let meta = health_meta();
        let metrics = meta["metrics"].as_object().unwrap();
        for (key, value) in metrics {
            assert!(
                value.get("name").is_some(),
                "health metric {key} missing 'name'"
            );
            assert!(
                value.get("description").is_some(),
                "health metric {key} missing 'description'"
            );
            assert!(
                value.get("interpretation").is_some(),
                "health metric {key} missing 'interpretation'"
            );
        }
    }

    #[test]
    fn health_meta_has_all_expected_metrics() {
        let meta = health_meta();
        let metrics = meta["metrics"].as_object().unwrap();
        let expected = [
            "cyclomatic",
            "cognitive",
            "line_count",
            "lines",
            "maintainability_index",
            "complexity_density",
            "dead_code_ratio",
            "fan_in",
            "fan_out",
            "score",
            "weighted_commits",
            "trend",
            "priority",
            "efficiency",
            "effort",
            "confidence",
            "bus_factor",
            "contributor_count",
            "share",
            "stale_days",
            "drift",
            "unowned",
            "runtime_coverage_verdict",
            "runtime_coverage_state",
            "runtime_coverage_confidence",
            "production_invocations",
            "percent_dead_in_production",
        ];
        for key in &expected {
            assert!(
                metrics.contains_key(*key),
                "health_meta missing expected metric: {key}"
            );
        }
    }

    #[test]
    fn dupes_meta_all_metrics_have_name_and_description() {
        let meta = dupes_meta();
        let metrics = meta["metrics"].as_object().unwrap();
        for (key, value) in metrics {
            assert!(
                value.get("name").is_some(),
                "dupes metric {key} missing 'name'"
            );
            assert!(
                value.get("description").is_some(),
                "dupes metric {key} missing 'description'"
            );
        }
    }

    #[test]
    fn dupes_meta_has_line_count() {
        let meta = dupes_meta();
        let metrics = meta["metrics"].as_object().unwrap();
        assert!(metrics.contains_key("line_count"));
    }

    #[test]
    fn check_docs_url_valid() {
        assert!(CHECK_DOCS.starts_with("https://"));
        assert!(CHECK_DOCS.contains("dead-code"));
    }

    #[test]
    fn health_docs_url_valid() {
        assert!(HEALTH_DOCS.starts_with("https://"));
        assert!(HEALTH_DOCS.contains("health"));
    }

    #[test]
    fn dupes_docs_url_valid() {
        assert!(DUPES_DOCS.starts_with("https://"));
        assert!(DUPES_DOCS.contains("dupes"));
    }

    #[test]
    fn check_meta_docs_url_matches_constant() {
        let meta = check_meta();
        assert_eq!(meta["docs"].as_str().unwrap(), CHECK_DOCS);
    }

    #[test]
    fn health_meta_docs_url_matches_constant() {
        let meta = health_meta();
        assert_eq!(meta["docs"].as_str().unwrap(), HEALTH_DOCS);
    }

    #[test]
    fn dupes_meta_docs_url_matches_constant() {
        let meta = dupes_meta();
        assert_eq!(meta["docs"].as_str().unwrap(), DUPES_DOCS);
    }

    #[test]
    fn rule_by_id_finds_all_check_rules() {
        for rule in CHECK_RULES {
            assert!(
                rule_by_id(rule.id).is_some(),
                "rule_by_id should find check rule {}",
                rule.id
            );
        }
    }

    #[test]
    fn rule_by_id_finds_all_health_rules() {
        for rule in HEALTH_RULES {
            assert!(
                rule_by_id(rule.id).is_some(),
                "rule_by_id should find health rule {}",
                rule.id
            );
        }
    }

    #[test]
    fn rule_by_id_finds_all_dupes_rules() {
        for rule in DUPES_RULES {
            assert!(
                rule_by_id(rule.id).is_some(),
                "rule_by_id should find dupes rule {}",
                rule.id
            );
        }
    }

    #[test]
    fn rule_by_id_finds_all_security_rules() {
        for rule in SECURITY_RULES {
            assert!(
                rule_by_id(rule.id).is_some(),
                "rule_by_id should find security rule {}",
                rule.id
            );
        }
    }

    #[test]
    fn check_rules_count() {
        assert_eq!(CHECK_RULES.len(), 43);
    }

    #[test]
    fn health_rules_count() {
        assert_eq!(HEALTH_RULES.len(), 16);
    }

    #[test]
    fn dupes_rules_count() {
        assert_eq!(DUPES_RULES.len(), 1);
    }

    #[test]
    fn flags_rules_count() {
        assert_eq!(FLAGS_RULES.len(), 1);
    }

    #[test]
    fn security_rules_count() {
        assert_eq!(
            SECURITY_RULES.len(),
            matcher_entries_from_security_catalogue().len() + 3
        );
    }

    #[test]
    fn security_rules_cover_every_catalogue_matcher() {
        let mut rule_ids = rustc_hash::FxHashSet::default();
        for rule in SECURITY_RULES {
            rule_ids.insert(rule.id);
        }

        for matcher in matcher_entries_from_security_catalogue() {
            let rule_id = format!("security/{}", matcher.id);
            assert!(
                rule_ids.contains(rule_id.as_str()),
                "security matcher {} has no explain rule",
                matcher.id
            );
        }
    }

    #[test]
    fn security_catalogue_rules_match_catalogue_title_and_cwe() {
        for matcher in matcher_entries_from_security_catalogue() {
            let rule_id = format!("security/{}", matcher.id);
            let rule = rule_by_id(&rule_id)
                .unwrap_or_else(|| panic!("security matcher {} has no explain rule", matcher.id));
            let cwe = format!("CWE-{}", matcher.cwe);
            assert_eq!(
                rule.name, matcher.title,
                "security matcher {} has stale explain title",
                matcher.id
            );
            assert!(
                rule.short.contains(&cwe),
                "security matcher {} explain summary does not mention {cwe}",
                matcher.id
            );
            assert!(
                rule.full.contains(&cwe),
                "security matcher {} explain rationale does not mention {cwe}",
                matcher.id
            );
        }
    }

    /// Every registered rule must declare a category. The PR/MR sticky
    /// renderer reads this via `category_for_rule`; without an entry the
    /// rule silently falls into the "Dead code" default and reviewers may
    /// see it grouped under an unexpected section. Catching this here is
    /// the same pattern as `check_rules_count` for the rule count itself.
    #[test]
    fn every_rule_declares_a_category() {
        let allowed = [
            "Dead code",
            "Dependencies",
            "Duplication",
            "Health",
            "Architecture",
            "Suppressions",
            "Security",
            "Policy",
            "Flags",
        ];
        for rule in CHECK_RULES
            .iter()
            .chain(HEALTH_RULES)
            .chain(DUPES_RULES)
            .chain(FLAGS_RULES)
            .chain(SECURITY_RULES)
        {
            assert!(
                !rule.category.is_empty(),
                "rule {} has empty category",
                rule.id
            );
            assert!(
                allowed.contains(&rule.category),
                "rule {} has unrecognised category {:?}; add to allowlist or pick from {:?}",
                rule.id,
                rule.category,
                allowed
            );
        }
    }

    #[derive(Debug)]
    struct MatcherEntry {
        id: &'static str,
        title: &'static str,
        cwe: &'static str,
    }

    fn matcher_entries_from_security_catalogue() -> Vec<MatcherEntry> {
        let toml = include_str!("../../core/data/security_matchers.toml");
        let mut entries = Vec::new();
        let mut in_matcher = false;
        let mut id = None;
        let mut title = None;
        let mut cwe = None;

        for line in toml.lines() {
            let trimmed = line.trim();
            if trimmed == "[[matcher]]" {
                if let (Some(id), Some(title), Some(cwe)) = (id.take(), title.take(), cwe.take()) {
                    entries.push(MatcherEntry { id, title, cwe });
                }
                in_matcher = true;
                continue;
            }
            if trimmed.starts_with("[[") {
                if let (Some(id), Some(title), Some(cwe)) = (id.take(), title.take(), cwe.take()) {
                    entries.push(MatcherEntry { id, title, cwe });
                }
                in_matcher = false;
                continue;
            }
            if !in_matcher {
                continue;
            }
            if let Some(value) = trimmed
                .strip_prefix("id = \"")
                .and_then(|value| value.strip_suffix('"'))
            {
                id = Some(value);
            } else if let Some(value) = trimmed
                .strip_prefix("title = \"")
                .and_then(|value| value.strip_suffix('"'))
            {
                title = Some(value);
            } else if let Some(value) = trimmed.strip_prefix("cwe = ") {
                cwe = Some(value);
            }
        }

        if let (Some(id), Some(title), Some(cwe)) = (id.take(), title.take(), cwe.take()) {
            entries.push(MatcherEntry { id, title, cwe });
        }

        let mut seen = rustc_hash::FxHashSet::default();
        entries
            .into_iter()
            .filter(|entry| seen.insert(entry.id))
            .collect()
    }
}
