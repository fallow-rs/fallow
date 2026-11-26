# `dead-code`: Dead Code Analysis

Analyzes the project for unused files, exports, dependencies, types, members, and more. Running `fallow` with no subcommand runs all analyses (dead code + duplication + complexity). Use `fallow dead-code` for dead code only.

## Flags

<!-- generated:flags:dead-code:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--include-dupes` | `bool` | `false` | Cross-reference with duplication findings |
| `--trace` | `string` | - | Trace export usage chain |
| `--trace-file` | `string` | - | Show all edges for a file |
| `--trace-dependency` | `string` | - | Trace where a dependency is used |
| `--impact-closure` | `string` | - | Compute the impact closure for a file (the transitive affected-but-not-in-diff set + coordination gap). Walks reverse-deps and re-export chains; powers the `inspect_target` MCP tool |
| `--symbol-impact` | `string` | - | Compute exact-symbol consumers, affected files, and targeted tests |
| `--file` | `string` | - | Scope output to specific files. Only issues in the specified files are reported. Project-wide dependency issues are suppressed. Warns on non-existent paths. Useful for lint-staged |
| `--finding-id` | `string` | - | Only report the findings with these `finding_id` values. Repeat the flag or pass a comma-separated list. Ids stay the same under every filter. The JSON output adds `finding_id_query`: a missing id means "resolved" only when `conclusive` is true |
| `--top` | `string` | - | Show only the top N items per category |

Common global flags for this command: [`--format`](global-flags.md), [`--quiet`](global-flags.md), [`--output-file`](global-flags.md), [`--changed-since`](global-flags.md), [`--max-file-size`](global-flags.md), [`--production`](global-flags.md), [`--no-production`](global-flags.md), [`--production-dead-code`](global-flags.md), [`--baseline`](global-flags.md), [`--save-baseline`](global-flags.md), [`--workspace`](global-flags.md), [`--changed-workspaces`](global-flags.md), [`--include-entry-exports`](global-flags.md).
<!-- generated:flags:dead-code:end -->
## Issue Type Filters

<!-- generated:flags:dead-code-filters:start -->
| Flag | Issue Type |
|---|---|
| `--unused-files` | Unused files |
| `--unused-exports` | Unused exports |
| `--unused-deps` | Dependency findings: unused dependencies, dev dependencies and optional dependencies, plus type-only, test-only and dev-in-production dependencies |
| `--unused-types` | Unused types |
| `--private-type-leaks` | Opt-in API hygiene check (default `off`) for exported signatures that reference same-file private types. Storybook `*.stories.*` story files and framework routing convention files (Next.js App + Pages Router, Gatsby, Remix v2, TanStack Router, Expo Router) are skipped to avoid noise. Enable via this flag or `private-type-leaks: "warn"` / `"error"` in [`rules`](configuration.md). |
| `--deprecated-exports-in-use` | Export marked @deprecated is still referenced |
| `--unused-enum-members` | Unused enum members |
| `--unused-class-members` | Unused class members |
| `--unused-store-members` | Unused Pinia store members |
| `--unused-namespace-members` | Namespace member is never referenced |
| `--unused-protected-class-members` | Protected class member is never referenced |
| `--namespace-escapes` | Namespace binding is used as a whole value, so its exports are not checked |
| `--test-only-exports` | Export is used only by test-side files |
| `--unprovided-injects` | inject() / getContext() reads a key that no provide() / setContext() supplies |
| `--unrendered-components` | A Vue / Svelte component is reachable through a barrel but rendered nowhere |
| `--unused-component-props` | A Vue defineProps prop or React component prop is referenced nowhere in its own component |
| `--absent-component-props` | Known reachable callers omit an optional prop consumed inside its component |
| `--constant-parameters` | Each visible call passes a function parameter in the same way |
| `--unused-component-emits` | A Vue <script setup> defineEmits event is emitted nowhere in its own component |
| `--unused-component-inputs` | An Angular @Input() / signal input() / model() is read nowhere in its own component (class body or template); needs `@angular/core` dep |
| `--unused-component-outputs` | An Angular @Output() / signal output() is emitted (.emit()) nowhere in its own component; needs `@angular/core` dep |
| `--unused-svelte-events` | A Svelte createEventDispatcher event is listened to nowhere in the project |
| `--unused-effect-layers` | An Effect layer is composed only into layers that nothing uses |
| `--unused-tagged-errors` | An Effect tagged error class is referenced only as a type or a catchTag string, and no code constructs or fails with it |
| `--unproduced-catch-tags` | A catchTag handler names a project error tag that no code produces |
| `--unused-server-actions` | A Next.js Server Action exported from a "use server" file is referenced by no code in the project |
| `--unused-load-data-keys` | A SvelteKit load() or React Router / Remix loader return-object key is read by no consumer |
| `--unresolved-imports` | Unresolved imports |
| `--unlisted-deps` | Unlisted dependencies |
| `--unlisted-binaries` | Binary used but provided by no installed package |
| `--duplicate-exports` | Duplicate exports |
| `--duplicate-export-aliases` | One module exports one binding under two or more names |
| `--stale-suppressions` | Stale suppression comments or `@expected-unused` JSDoc tags |
| `--unused-catalog-entries` | Unused pnpm catalog entries |
| `--empty-catalog-groups` | Empty named pnpm catalog groups |
| `--unresolved-catalog-references` | Package references to missing pnpm catalog entries |
| `--unused-dependency-overrides` | Unused package-manager dependency overrides |
| `--misconfigured-dependency-overrides` | Misconfigured package-manager dependency overrides |
<!-- generated:flags:dead-code-filters:end -->

`--deprecated-exports-in-use` is an opt-in migration sweep (default `off`). Each finding has the exact `consumer_count` and a sample of up to 10 consumers. `fallow dead-code --trace FILE:EXPORT` lists all of them. Enable it with this flag or with `deprecated-exports-in-use: "warn"` or `"error"` in [`rules`](configuration.md).

Each consumer in `--trace FILE:EXPORT` and each coordination gap in `--impact-closure` has `search_visibility` (`found`, `renamed`, `missed`). A `grep -w` for the export name does not find a `missed` consumer. When `search_as` is present, search for that name in that consumer file. A coordination gap with more than one consumed symbol names the symbol of these fields in `search_symbol`.

## Production mode

`--production` (or `production: true`, or `production: { deadCode: true }` in the config) drops each file that matches a built-in production pattern before analysis. The patterns are fixed:

- test and spec files: `**/*.test.*`, `**/*.spec.*`, `**/*.e2e.*`, `**/*.e2e-spec.*`, `**/*.bench.*`, `**/*.fixture.*`;
- stories: `**/*.stories.*`, `**/*.story.*`;
- test directories: `**/__tests__/**`, `**/__mocks__/**`, `**/__snapshots__/**`, `**/__fixtures__/**`, `**/test-d/**`, `**/test/**`, `**/tests/**`;
- tooling config at the project root: `*.config.*`;
- dotfiles: `**/.*.js`, `**/.*.ts`, `**/.*.mjs`, `**/.*.cjs`.

A directory pattern also matches production source in a directory with that name, for example `src/components/runner/test/Runner.vue`. Fallow then does not analyze the file, so an export or a parameter that only this file uses can appear in a finding. When a production file imports such a file, `workspace_diagnostics[]` carries one `production-excluded-file-imported` entry for each pattern. The entry has `pattern`, `file_count`, and `files` (the first five files). The human output prints the same warning on stderr.

`ignorePatterns` cannot bring a file back that a production pattern drops. To analyze the file, move it out of the matched path (for example, rename the `test` directory), or run that analysis without production mode: `--no-production`, or `production: { deadCode: false }` in the config.

## Examples

```bash
# Full analysis with JSON output
fallow dead-code --format json --quiet

# Only unused exports
fallow dead-code --format json --quiet --unused-exports

# PR check: only changed files
fallow dead-code --format json --quiet --changed-since main --fail-on-issues

# CI mode with SARIF upload
fallow dead-code --ci

# Production-only analysis
fallow dead-code --format json --quiet --production

# Single workspace package
fallow dead-code --format json --quiet --workspace my-package

# Multiple workspaces: comma-separated
fallow dead-code --format json --quiet --workspace web,admin

# Glob (matches package name OR relative path)
fallow dead-code --format json --quiet --workspace 'apps/*'

# Exclude a workspace from the set
fallow dead-code --format json --quiet --workspace 'apps/*,!apps/legacy'

# Monorepo CI: auto-scope to workspaces containing any file changed since origin/main
fallow dead-code --format json --quiet --changed-workspaces origin/main

# Debug: trace an export
fallow dead-code --format json --quiet --trace src/utils.ts:myFunction

# Incremental adoption with baseline
fallow dead-code --format json --quiet --save-baseline fallow-baselines/dead-code.json
fallow dead-code --format json --quiet --baseline fallow-baselines/dead-code.json --fail-on-issues

# Regression detection: update regression.baseline in the discovered config
fallow dead-code --format json --quiet --save-regression-baseline
# Then compare on PRs
fallow dead-code --format json --quiet --fail-on-regression --tolerance 2%

# Scope to specific files (e.g., lint-staged)
fallow dead-code --format json --quiet --file src/utils.ts --file src/helpers.ts

# Catch typos in entry file exports
fallow dead-code --format json --quiet --include-entry-exports
```
