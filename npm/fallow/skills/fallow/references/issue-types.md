# Fallow Issue Types

Every issue type fallow can report, with its dead-code filter flag, whether `fallow fix` removes it, its suppression comment, and what it means. SKILL.md keeps only the filter-flag and suppression conventions plus a pointer; load this file when you need a specific type.

`fallow explain <issue-type>` prints the same description for one type without running analysis, and the MCP server serves the catalogue as the `fallow://issue-types` resource.

The `generated:issue-types` table below is regenerated from `fallow schema` by scripts/generate-agent-docs.mjs; edit the curated Description cells in place, never the identity columns.

## Catalogue

<!-- generated:issue-types:start -->
| Type | Filter flag | Fixable | Suppress comment | Description |
|---|---|---|---|---|
| `unused-file` | `--unused-files` | - | `// fallow-ignore-file unused-file` | Files unreachable from entry points |
| `unused-export` | `--unused-exports` | yes | `// fallow-ignore-next-line unused-export` | Symbols never imported elsewhere |
| `unused-type` | `--unused-types` | - | `// fallow-ignore-next-line unused-type` | Type aliases and interfaces |
| `private-type-leak` | `--private-type-leaks` | - | `// fallow-ignore-next-line private-type-leak` | Opt-in API hygiene check (default `off`) for exported signatures whose type references a same-file private type |
| `deprecated-export-in-use` | `--deprecated-exports-in-use` | - | `// fallow-ignore-next-line deprecated-export-in-use` | Export marked @deprecated is still referenced; Opt-in migration sweep; the rule defaults to off |
| `unused-dependency` | `--unused-deps` | yes | - | Packages in `dependencies` never imported. In monorepos, internal workspace package names (e.g., `@repo/ui`) declared in another workspace's `package.json` but never imported are reported here too. `--unused-deps` also covers the dev/optional/type-only/test-only sibling rows below. |
| `unused-dev-dependency` | `--unused-deps` | yes | - | Packages in `devDependencies` never imported by test files, config files, or scripts |
| `unused-optional-dependency` | `--unused-deps` | yes | - | Packages in `optionalDependencies` never imported (often platform-specific; verify before removing) |
| `type-only-dependency` | `--unused-deps` | - | - | Production dependency only used via type-only imports; Only reported in --production mode; --unused-deps scopes it together with the other dependency kinds |
| `test-only-dependency` | `--unused-deps` | - | - | Production deps only imported from test files (should be devDependencies) |
| `dev-dependency-in-production` | `--unused-deps` | - | - | devDependency imported by production code with a runtime import |
| `unused-enum-member` | `--unused-enum-members` | yes | `// fallow-ignore-next-line unused-enum-member` | Enum values never referenced |
| `unused-class-member` | `--unused-class-members` | - | `// fallow-ignore-next-line unused-class-member` | Methods and properties |
| `unused-store-member` | `--unused-store-members` | - | `// fallow-ignore-next-line unused-store-member` | Pinia store state/getter/action (needs `pinia` dep) |
| `unresolved-import` | `--unresolved-imports` | - | `// fallow-ignore-next-line unresolved-import` | Imports that can't be resolved |
| `unlisted-dependency` | `--unlisted-deps` | - | - | Used packages missing from package.json. In monorepos, importing a workspace package from a workspace whose own `package.json` does not list it is reported here too; self-references stay allowed without requiring a package to depend on itself. |
| `duplicate-export` | `--duplicate-exports` | - | `// fallow-ignore-file duplicate-export` | Same symbol exported from multiple modules |
| `circular-dependency` | `--circular-deps` | - | `// fallow-ignore-next-line circular-dependency` | Import cycles in the module graph |
| `re-export-cycle` | `--re-export-cycles` | - | `// fallow-ignore-file re-export-cycle` | Barrel files re-exporting from each other in a loop (`kind: "multi-node"`) or a barrel re-exporting from itself (`kind: "self-loop"`). Chain propagation through the loop is a structural no-op so imports through any member may silently come up empty. Default `warn`. Distinct from `circular-dependencies` (runtime cycles, sometimes intentional). File-scoped suppression only: `// fallow-ignore-file re-export-cycle` on any member breaks the cycle. |
| `boundary-violation` | `--boundary-violations` | - | `// fallow-ignore-next-line boundary-violation` | Imports crossing architecture zone boundaries. Presets: `layered`, `hexagonal`, `feature-sliced`, `bulletproof`; `autoDiscover` can create one zone per feature directory; per-rule `allowTypeOnly: [zones]` admits `import type` / `export type` crossings while still blocking value imports. Optional sections: `boundaries.coverage.requireAllFiles` reports unzoned source files (`allowUnmatched` globs exempt intentional ones), and `boundaries.calls.forbidden` bans callee patterns per zone (segment-aware and import-resolved, so `child_process.*` covers `node:child_process` named/namespace/default imports; direct callees only, zoned files only). The whole family shares the `boundary-violation` rule and suppression token (`boundary-call-violation` and `boundary-call-violations` accepted as aliases); start the rule at `warn` for a staged rollout |
| `boundary-coverage` | `--boundary-violations` | - | `// fallow-ignore-file boundary-violation` | Source file matches no configured architecture boundary zone; Requires boundaries.coverage.requireAllFiles |
| `boundary-call-violation` | `--boundary-violations` | - | `// fallow-ignore-next-line boundary-call-violation` | Zoned file calls a callee its zone forbids; Requires boundaries.calls.forbidden patterns |
| `policy-violation` | `--policy-violations` | - | `// fallow-ignore-next-line policy-violation` | Calls, imports, or catalogue-derived effects banned by a declarative rule pack (`rulePacks` config key lists standalone JSON/JSONC files of `banned-call`, `banned-import`, and `banned-effect` rules; pure data, no project code executes). Findings identified as `<pack>/<rule-id>`. Default `warn` master; per-rule `severity` overrides per finding and the exit gate reads the effective severity. Invalid or missing packs fail config load with exit 2. `fallow rule-pack-schema` prints the pack JSON Schema. Use the scoped token to suppress one rule; bare `policy-violation` still covers every pack rule on the line or file. |
| `stale-suppression` | `--stale-suppressions` | - | - | `fallow-ignore` comments or `@expected-unused` JSDoc tags that no longer match any issue |
| `missing-suppression-reason` | `--stale-suppressions` | - | - | Suppression comment omits a required reason |
| `unused-catalog-entry` | `--unused-catalog-entries` | yes | - | `pnpm-workspace.yaml` entries no workspace package.json references via `catalog:` (default `warn`) |
| `empty-catalog-group` | `--empty-catalog-groups` | - | - | Named `catalogs.<name>:` groups in `pnpm-workspace.yaml` with no entries. Top-level `catalog:` placeholders are ignored. Default `warn`. |
| `unresolved-catalog-reference` | `--unresolved-catalog-references` | - | - | `package.json` references to `catalog:` / `catalog:<name>` whose catalog does not declare the package; `pnpm install` would fail. Default `error`. Suppress via `ignoreCatalogReferences: [{ package, catalog?, consumer? }]` in fallow config (package.json has no comment syntax). |
| `unused-dependency-override` | `--unused-dependency-overrides` | - | - | Entries in `pnpm-workspace.yaml#overrides`, `package.json#pnpm.overrides`, npm or Bun `package.json#overrides`, or Bun `package.json#resolutions` whose target package is not declared by any workspace `package.json` and is not present in the active readable lockfile. Default `warn`. pnpm and npm projects without a readable lockfile degrade to a manifest-only fallback with a verification `hint`; Bun projects with only binary `bun.lockb` fail closed and emit no finding. Suppress via `ignoreDependencyOverrides: [{ package, source? }]` in fallow config. |
| `misconfigured-dependency-override` | `--misconfigured-dependency-overrides` | - | - | Package-manager override entries whose key is unparsable or whose value is missing or empty. The active package manager may reject or ignore the entry. Default `error`. Suppression: same `ignoreDependencyOverrides` config rule. |
| `invalid-client-export` | - | - | `// fallow-ignore-next-line invalid-client-export` | "use client" file exports a server-only / route-config name; Requires the project to declare next |
| `mixed-client-server-barrel` | - | - | `// fallow-ignore-next-line mixed-client-server-barrel` | Barrel re-exports both a "use client" module and a server-only module; Requires the project to declare next |
| `misplaced-directive` | - | - | `// fallow-ignore-next-line misplaced-directive` | "use client" / "use server" directive is not in the leading position and is ignored; Requires the project to declare next |
| `unprovided-inject` | `--unprovided-injects` | - | `// fallow-ignore-next-line unprovided-inject` | inject() / getContext() reads a key that no provide() / setContext() supplies |
| `unrendered-component` | `--unrendered-components` | - | `// fallow-ignore-next-line unrendered-component` | A Vue / Svelte component is reachable through a barrel but rendered nowhere |
| `unused-component-prop` | `--unused-component-props` | - | `// fallow-ignore-next-line unused-component-prop` | A Vue defineProps prop or React component prop is referenced nowhere in its own component |
| `unused-component-emit` | `--unused-component-emits` | - | `// fallow-ignore-next-line unused-component-emit` | A Vue <script setup> defineEmits event is emitted nowhere in its own component |
| `unused-component-input` | `--unused-component-inputs` | - | `// fallow-ignore-next-line unused-component-input` | An Angular @Input() / signal input() / model() is read nowhere in its own component (class body or template); needs `@angular/core` dep |
| `unused-component-output` | `--unused-component-outputs` | - | `// fallow-ignore-next-line unused-component-output` | An Angular @Output() / signal output() is emitted (.emit()) nowhere in its own component; needs `@angular/core` dep |
| `unused-svelte-event` | `--unused-svelte-events` | - | `// fallow-ignore-next-line unused-svelte-event` | A Svelte createEventDispatcher event is listened to nowhere in the project; needs `svelte` dep |
| `unused-server-action` | `--unused-server-actions` | - | `// fallow-ignore-next-line unused-server-action` | A Next.js Server Action exported from a "use server" file is referenced by no code in the project |
| `unused-load-data-key` | `--unused-load-data-keys` | - | `// fallow-ignore-next-line unused-load-data-key` | A SvelteKit load() return-object key is read by no consumer (needs @sveltejs/kit dep) |
| `prop-drilling` | - | - | `// fallow-ignore-next-line prop-drilling` | A React/Preact prop is forwarded unchanged through 3+ pass-through components to a distant consumer; Opt-in: set rules.prop-drilling to warn or error to enable. Defaults to off. |
| `thin-wrapper` | - | - | `// fallow-ignore-next-line thin-wrapper` | A React/Preact component whose whole body is a single spread-forwarded child render (a candidate for inlining); Opt-in: set rules.thin-wrapper to warn or error to enable. Defaults to off. |
| `duplicate-prop-shape` | - | - | `// fallow-ignore-next-line duplicate-prop-shape` | Three or more React/Preact components across two or more files declare an identical prop-name set (a missing shared Props type); Opt-in: set rules.duplicate-prop-shape to warn or error to enable. Defaults to off. |
| `route-collision` | - | - | `// fallow-ignore-file route-collision` | Two or more Next.js App Router route files resolve to the same URL |
| `dynamic-segment-name-conflict` | - | - | `// fallow-ignore-file dynamic-segment-name-conflict` | Sibling Next.js dynamic route segments use different slug names at the same position |
| `high-cyclomatic-complexity` | `--complexity` | - | `// fallow-ignore-next-line complexity` | Function has high cyclomatic complexity |
| `high-cognitive-complexity` | `--complexity` | - | `// fallow-ignore-next-line complexity` | Function has high cognitive complexity |
| `high-complexity` | `--complexity` | - | `// fallow-ignore-next-line complexity` | Function exceeds both complexity thresholds |
| `high-crap-score` | `--complexity` | - | `// fallow-ignore-next-line complexity` | Function has a high CRAP score (complexity combined with low coverage) |
| `refactoring-target` | `--targets` | - | - | File identified as a high-priority refactoring candidate |
| `css-token-drift` | - | - | `// fallow-ignore-next-line css-token-drift` | CSS or CSS-in-JS hardcoded styling value bypasses the design token system |
| `css-duplicate-block` | - | - | `// fallow-ignore-next-line css-duplicate-block` | CSS or CSS-in-JS declaration block is duplicated across rules |
| `css-selector-complexity` | - | - | `// fallow-ignore-next-line css-selector-complexity` | CSS selector, nesting, or important usage is structurally complex |
| `css-dead-surface` | - | - | `// fallow-ignore-next-line css-dead-surface` | CSS or CSS-in-JS surface appears unused |
| `css-broken-reference` | - | - | `// fallow-ignore-next-line css-broken-reference` | CSS or CSS-in-JS reference resolves to no stylesheet definition |
| `untested-file` | `--coverage-gaps` | - | `// fallow-ignore-file coverage-gaps` | Runtime-reachable file has no test dependency path |
| `untested-export` | `--coverage-gaps` | - | `// fallow-ignore-file coverage-gaps` | Runtime-reachable export has no test dependency path |
| `code-duplication` | - | - | `// fallow-ignore-next-line code-duplication` | Duplicated code block; Reported by fallow dupes (and bare fallow / fallow audit) |
| `feature-flag` | - | - | `// fallow-ignore-next-line feature-flag` | Detected feature flag pattern; Reported by fallow flags |
| `tainted-sink` | - | - | `// fallow-ignore-next-line security-sink` | Syntactic security sink candidates require verification |
| `client-server-leak` | - | - | `// fallow-ignore-file security-client-server-leak` | Client-bound code reaches a non-public env read |
| `hardcoded-secret` | - | - | `// fallow-ignore-next-line security-sink` | Provider-prefixed or contextual secret literals require verification; Include-required category: enable via security.categories.include |

Runtime-coverage verdicts and the full security sink catalogue are listed by `fallow schema` (`issue_types`).
<!-- generated:issue-types:end -->
