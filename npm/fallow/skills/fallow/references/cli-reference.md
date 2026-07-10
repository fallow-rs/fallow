# Fallow CLI Reference

Complete command and flag specifications for all fallow CLI commands.

---

## Table of Contents

- [`dead-code`: Dead Code Analysis](#dead-code-dead-code-analysis)
- [`dupes`: Duplication Detection](#dupes-duplication-detection)
- [`fix`: Auto-Remove Unused Code](#fix-auto-remove-unused-code)
- [`list`: Project Introspection](#list-project-introspection)
- [`init`: Config Generation](#init-config-generation)
- [`migrate`: Config Migration](#migrate-config-migration)
- [`health`: Function Complexity Analysis](#health-function-complexity-analysis)
- [`audit`: Changed-File Quality Gate](#audit-changed-file-quality-gate)
- [`flags`: Feature Flag Detection](#flags-feature-flag-detection)
- [`security`: Security Candidate Detection](#security-security-candidate-detection)
- [`inspect`: Target Evidence Bundle](#inspect-target-evidence-bundle)
- [`trace`: Symbol Call Chains](#trace-symbol-call-chains)
- [`decision-surface`: Structural Decisions](#decision-surface-structural-decisions)
- [`explain`: Rule Explanation](#explain-rule-explanation)
- [`schema`: CLI Introspection](#schema-cli-introspection)
- [`config-schema`: Config JSON Schema](#config-schema-config-json-schema)
- [`plugin-schema`: Plugin JSON Schema](#plugin-schema-plugin-json-schema)
- [`plugin-check`: Verify external plugins](#plugin-check-verify-external-plugins)
- [`rule-pack-schema`: Rule Pack JSON Schema](#rule-pack-schema-rule-pack-json-schema)
- [`config`: Show Resolved Config](#config-show-resolved-config)
- [Global Flags](#global-flags)
- [Environment Variables](#environment-variables)
- [Output Formats](#output-formats)
- [JSON Output Structure](#json-output-structure)
- [Configuration File Format](#configuration-file-format)
- [Inline Suppression Comments](#inline-suppression-comments)

---

## `dead-code`: Dead Code Analysis

Analyzes the project for unused files, exports, dependencies, types, members, and more. Running `fallow` with no subcommand runs all analyses (dead code + duplication + complexity). Use `fallow dead-code` for dead code only.

### Flags

<!-- generated:flags:dead-code:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--include-dupes` | `bool` | `false` | Cross-reference with duplication findings |
| `--trace` | `string` | - | Trace export usage chain |
| `--trace-file` | `string` | - | Show all edges for a file |
| `--trace-dependency` | `string` | - | Trace where a dependency is used |
| `--impact-closure` | `string` | - | Compute the impact closure for a file (the transitive affected-but-not-in-diff set + coordination gap). Walks reverse-deps and re-export chains; powers the `inspect_target` MCP tool |
| `--top` | `string` | - | Show only the top N items per category |
| `--file` | `string` | - | Scope output to specific files. Only issues in the specified files are reported. Project-wide dependency issues are suppressed. Warns on non-existent paths. Useful for lint-staged |

Common global flags for this command: [`--format`](#global-flags), [`--quiet`](#global-flags), [`--output-file`](#global-flags), [`--changed-since`](#global-flags), [`--max-file-size`](#global-flags), [`--production`](#global-flags), [`--no-production`](#global-flags), [`--production-dead-code`](#global-flags), [`--baseline`](#global-flags), [`--save-baseline`](#global-flags), [`--workspace`](#global-flags), [`--changed-workspaces`](#global-flags), [`--include-entry-exports`](#global-flags).
<!-- generated:flags:dead-code:end -->
### Issue Type Filters

<!-- generated:flags:dead-code-filters:start -->
| Flag | Issue Type |
|---|---|
| `--unused-files` | Unused files |
| `--unused-exports` | Unused exports |
| `--unused-deps` | Unused dependencies, devDependencies, optionalDependencies, type-only production deps, and test-only production deps |
| `--unused-types` | Unused types |
| `--private-type-leaks` | Opt-in API hygiene check (default `off`) for exported signatures that reference same-file private types. Storybook `*.stories.*` story files and framework routing convention files (Next.js App + Pages Router, Gatsby, Remix v2, TanStack Router, Expo Router) are skipped to avoid noise. Enable via this flag or `private-type-leaks: "warn"` / `"error"` in [`rules`](#rules-configuration). |
| `--unused-enum-members` | Unused enum members |
| `--unused-class-members` | Unused class members |
| `--unused-store-members` | Unused Pinia store members |
| `--unprovided-injects` | inject() / getContext() reads a key that no provide() / setContext() supplies |
| `--unrendered-components` | A Vue / Svelte component is reachable through a barrel but rendered nowhere |
| `--unused-component-props` | A Vue defineProps prop or React component prop is referenced nowhere in its own component |
| `--unused-component-emits` | A Vue <script setup> defineEmits event is emitted nowhere in its own component |
| `--unused-component-inputs` | An Angular @Input() / signal input() / model() is read nowhere in its own component (class body or template); needs `@angular/core` dep |
| `--unused-component-outputs` | An Angular @Output() / signal output() is emitted (.emit()) nowhere in its own component; needs `@angular/core` dep |
| `--unused-svelte-events` | A Svelte createEventDispatcher event is listened to nowhere in the project |
| `--unused-server-actions` | A Next.js Server Action exported from a "use server" file is referenced by no code in the project |
| `--unused-load-data-keys` | A SvelteKit load() return-object key is read by no consumer |
| `--unresolved-imports` | Unresolved imports |
| `--unlisted-deps` | Unlisted dependencies |
| `--duplicate-exports` | Duplicate exports |
| `--circular-deps` | Circular dependencies |
| `--re-export-cycles` | Re-export cycles (`kind: multi-node` for barrel files re-exporting from each other in a loop, `kind: self-loop` for a barrel re-exporting from itself). File-scoped finding; chain propagation through the loop is a no-op so imports may silently come up empty. Distinct from `--circular-deps` (runtime cycles). |
| `--boundary-violations` | Boundary violations (imports crossing architecture zone boundaries, unzoned source files when `boundaries.coverage.requireAllFiles` is set, and forbidden calls from `boundaries.calls.forbidden`; suppression token `boundary-violation`, with `boundary-call-violation` and `boundary-call-violations` accepted as aliases for the whole family) |
| `--policy-violations` | Rule-pack policy violations (banned calls, imports, and catalogue-derived effects declared via the `rulePacks` config key) |
| `--stale-suppressions` | Stale suppression comments or `@expected-unused` JSDoc tags |
| `--unused-catalog-entries` | Unused pnpm catalog entries |
| `--empty-catalog-groups` | Empty named pnpm catalog groups |
| `--unresolved-catalog-references` | Package references to missing pnpm catalog entries |
| `--unused-dependency-overrides` | Unused pnpm dependency overrides |
| `--misconfigured-dependency-overrides` | Malformed pnpm dependency overrides |
<!-- generated:flags:dead-code-filters:end -->
### Examples

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

# Regression detection: save baseline on main, compare on PRs
fallow dead-code --format json --quiet --save-regression-baseline
fallow dead-code --format json --quiet --fail-on-regression --tolerance 2%

# Scope to specific files (e.g., lint-staged)
fallow dead-code --format json --quiet --file src/utils.ts --file src/helpers.ts

# Catch typos in entry file exports
fallow dead-code --format json --quiet --include-entry-exports
```

---

## `dupes`: Duplication Detection

Finds code duplication and clones across the project.

By default, `fallow dupes` skips generated framework output matching `**/.next/**`, `**/.nuxt/**`, `**/.svelte-kit/**`, `**/.turbo/**`, `**/.parcel-cache/**`, `**/.vite/**`, `**/.cache/**`, `**/out/**`, and `**/storybook-static/**`. These defaults merge with `duplicates.ignore`. Set `duplicates.ignoreDefaults = false` to opt out and use only your configured ignore list. If the reported duplication percentage drops after upgrading, this generated-output filtering is the expected reason.

### Flags

<!-- generated:flags:dupes:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--mode` | `strict\|mild\|weak\|semantic` | - | Detection mode |
| `--min-tokens` | `string` | - | Minimum token count for a clone |
| `--min-lines` | `string` | - | Minimum line count for a clone |
| `--min-occurrences` | `string` | - | Minimum number of occurrences before a clone group is reported (must be ≥ 2). Raise to skip pair-only clones and focus on widespread copy-paste worth refactoring. `fallow init` writes `minOccurrences: 3` into new projects. |
| `--threshold` | `string` | - | Fail if duplication exceeds this percentage |
| `--skip-local` | `bool` | `false` | Only report cross-directory duplicates |
| `--cross-language` | `bool` | `false` | Strip type annotations for TS↔JS matching |
| `--ignore-imports` | `bool` | `false` | Exclude module wiring from clone detection |
| `--no-ignore-imports` | `bool` | `false` | Count module wiring as clone candidates (opt out of the default exclusion) |
| `--top` | `string` | - | Show only the N most-duplicated clone groups (sorted by instance count desc, tiebreak: line count desc, then path/line). Summary stats reflect the full project. |
| `--trace` | `string` | - | Deep-dive clones. `FILE:LINE` traces all clones at a location; `dup:<id>` traces a clone group by the stable fingerprint shown in the listing and on `clone_groups[].fingerprint` in JSON. Fingerprints are usually `dup:<8hex>` and widen only on rare report collisions. Trace output adds an extract-function suggestion, estimated savings, and a best-effort proposed name per group |

Common global flags for this command: [`--format`](#global-flags), [`--quiet`](#global-flags), [`--changed-since`](#global-flags), [`--baseline`](#global-flags), [`--save-baseline`](#global-flags), [`--workspace`](#global-flags), [`--changed-workspaces`](#global-flags), [`--group-by`](#global-flags), [`--explain-skipped`](#global-flags).
<!-- generated:flags:dupes:end -->
### Detection Modes

| Mode | Behavior |
|------|----------|
| `strict` | Exact token match (no normalization) |
| `mild` | Syntax normalized (whitespace, semicolons) |
| `weak` | Different literal values treated as equivalent |
| `semantic` | Renamed variables also treated as equivalent |

### Examples

```bash
# Default duplication scan
fallow dupes --format json --quiet

# Semantic mode (detects renames)
fallow dupes --format json --quiet --mode semantic

# Cross-directory only, fail at 5%
fallow dupes --format json --quiet --skip-local --threshold 5

# Trace clones at a specific location
fallow dupes --format json --quiet --trace src/utils.ts:42

# Deep-dive a clone group by its dup:<id> fingerprint (from the listing or JSON)
fallow dupes --format json --quiet --trace dup:7f3a2c1e

# Only check duplication in changed files
fallow dupes --format json --quiet --changed-since main

# Incremental CI
fallow dupes --format json --quiet --save-baseline fallow-baselines/dupes.json
fallow dupes --format json --quiet --baseline fallow-baselines/dupes.json --threshold 5
```

---

## `fix`: Auto-Remove Unused Code

Auto-removes unused exports, dependencies, enum members, and pnpm catalog entries.

### Flags

<!-- generated:flags:fix:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--dry-run` | `bool` | `false` | Show what would be removed without modifying files. For `add-to-config` actions, prints a unified-diff preview of the proposed config write; JSON mode includes the diff under a `proposed_diff` field on the fix entry. |
| `--yes` | `bool` | `false` | Skip confirmation prompt (**required** in non-TTY) |
| `--no-create-config` | `bool` | `false` | Refuse to create a new `.fallowrc.json` when none exists. The duplicate-export config-add path is skipped with `skip_reason: "no_create_config"`; source-file edits proceed normally. Use in pre-commit hooks, CI bots, and `fallow watch` where silently materialising a new top-level file would surprise the user. |

Common global flags for this command: [`--format`](#global-flags), [`--quiet`](#global-flags).
<!-- generated:flags:fix:end -->
### What gets fixed

- Unused exports (removes the `export` keyword; whole-enum block when every member is unused)
- Unused dependencies (removed from `package.json`)
- Unused enum members (removed from the declaration)
- Unused pnpm catalog entries (removed from `pnpm-workspace.yaml` by line-aware deletion). Object-form entries are removed as one block. By default, fallow also removes a contiguous YAML comment block immediately above the entry when it clearly belongs to that entry; configure this with `fix.catalog.deletePrecedingComments` (`"auto"`, `"always"`, or `"never"`). Two escape hatches keep curated comments safe regardless of policy: a `# fallow-keep` marker on any line in the block preserves it, and the `auto` policy additionally preserves section-banner blocks whose body starts with three or more `=`, `-`, `*`, `_`, `~`, `+`, or `#` characters (e.g. `# === React 18 production pins ===`). Other comments and stylistic choices are preserved. When the last entry of a catalog group is removed, the header is rewritten to `catalog: {}` / `<name>: {}` so pnpm doesn't reject the resulting null value. Entries with non-empty `hardcoded_consumers` are skipped to avoid breaking `pnpm install`; the skip is surfaced in the JSON fix output as `{"type": "remove_catalog_entry", "applied": false, "skipped": true, "skip_reason": "hardcoded_consumers", "consumers": [...]}`. The JSON action carries both `line` (first deleted line, the leading comment when policy absorbs one) and `entry_line` (the catalog entry's original 1-based line); use `entry_line` as a stable anchor across policy changes. After a successful catalog edit the CLI emits a one-line `Run pnpm install to refresh pnpm-lock.yaml` reminder, and the human stderr summary appends `(+M catalog comment lines)` to the fixed-issue count when comment lines were absorbed. The JSON envelope carries a top-level `"skipped"` count alongside `"total_fixed"` for partial-fix gating.
- Duplicate exports (appends an `ignoreExports` rule to your fallow config file). When no fallow config file exists, `.fallowrc.json` is created using the same scaffolding `fallow init` would emit (framework detection, `$schema`, `entry`, `ignorePatterns`, etc.) and the rules are layered on top. Inside a monorepo subpackage (`pnpm-workspace.yaml`, `package.json#workspaces`, `turbo.json`, `lerna.json`, or `rush.json` above the invocation directory) the create-fallback refuses to fire and emits `skip_reason: "monorepo_subpackage"` with a relative `workspace_root` path pointing at the workspace root. The applied entry carries `created_files: [".fallowrc.json"]` so consumers can detect file-creation side effects programmatically.

### On-disk drift protection

`fallow fix` captures every parsed source file's xxh3 content hash during the in-process analysis and recomputes it at fix time. Files whose hash drifted between analysis and write (parallel editor save, CI rebase, concurrent tool) are skipped with `{"type": "skipped", "path": "...", "skipped": true, "skip_reason": "content_changed"}` in the JSON output and `Skipping <path>: file content changed since fallow dead-code ran. Re-run fallow fix to refresh the analysis first.` on stderr (gated on non-quiet). A run with any content-changed skip exits with code 2 so CI does not treat the partial run as a clean no-op. The JSON envelope's top-level `skipped_content_changed: number` is always present and disjoint from `skipped` (which still tallies catalog / YAML guard skips only). Per-file writes are batched: each rewrite is staged to a sibling temp file, and the orchestrator promotes the batch only after every stage succeeds. A stage failure leaves every target file at its original content. Hash precondition covers source files (TS, JS, Vue, Svelte, Astro, MDX); `package.json` and `pnpm-workspace.yaml` are not in the captured hash map because the extract layer does not parse them, but the dep and catalog fixers re-parse those files at fix time as the natural safety net.

### Low-confidence export removals

Issue #602: `fallow fix` withholds unused-export removals when the consumer may be invisible to static analysis, because stripping a real export breaks `tsc` and the build. Two cases are skipped:

- **Off-graph consumer directories.** The file is under any of `__mocks__`, `__fixtures__`, `fixtures`, `e2e`, `e2e-tests`, `cypress`, `playwright`, `examples`, `evals`, `golden` (matched on any path segment). Catches Vitest mock aliases, off-workspace e2e suites, and fixture / golden harnesses. Plain `test` / `tests` / `__tests__` are deliberately NOT on the list, so genuinely-dead test helpers still auto-remove.
- **Files with an unresolved import.** The file itself imports something fallow could not resolve, so its local usage graph is incomplete.

JSON output carries `{"type": "skipped", "path": "...", "skipped": true, "skip_reason": "low_confidence_off_graph"}` (or `"low_confidence_unresolved_imports"`) plus a top-level counter `skipped_low_confidence_exports: number` (always present), disjoint from `skipped`. Unlike the drift and encoding skips this is INTENTIONAL and does NOT change the exit code; the export stays reported by `fallow dead-code` for manual review. High-confidence exports in normal source files are removed unchanged. The AI agent should report kept exports to the user and let them decide whether the export is truly unused before removing it by hand.

### File encoding contract

`fallow fix` is UTF-8 only. Two encoding shapes that previously caused silent corruption are handled explicitly (issue #475):

- **UTF-8 BOM round-trip.** Files with a leading UTF-8 byte-order mark (`EF BB BF`, common on Windows-authored TypeScript) are read with the BOM stripped before line-offset computation and parsing, so reported line numbers do not shift by the BOM codepoint, and the BOM is re-prepended on write so the file's encoding is preserved on round-trip. fallow neither adds nor removes a BOM; if your input has one, the output has one.

- **Mixed CRLF / LF rejection.** Files containing both `\r\n` and bare-LF line endings (common after cross-platform edits without `core.autocrlf`) are skipped instead of silently rewritten to the wrong offsets. The stderr message names the remediation: `Skipping <path>: file has mixed CRLF/LF line endings. Normalize with dos2unix or set git config core.autocrlf input, then re-run fallow fix.`. JSON output carries `{"type": "skipped", "path": "...", "skipped": true, "skip_reason": "mixed_line_endings"}` plus a top-level counter `skipped_mixed_line_endings: number` (always present) disjoint from `skipped_content_changed`. Any non-zero mixed-EOL count exits the run with code 2.

  **The skip is NOT self-healing**. Re-running `fallow fix` produces the same skip; the AI agent or user must run `dos2unix <path>` (or set `git config core.autocrlf input` and re-checkout) before fallow can act on the file. When the same file carries findings for multiple fixers (e.g. an unused export AND an unused enum member), the skip is reported once per file, not once per fixer.

### Examples

```bash
# Preview changes
fallow fix --dry-run --format json --quiet

# Apply changes (--yes required in agent/CI environments)
fallow fix --yes --format json --quiet
```

---

## `list`: Project Introspection

Inspect discovered files, entry points, detected frameworks, and architecture boundary zones.

### Flags

<!-- generated:flags:list:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--entry-points` | `bool` | `false` | List detected entry points |
| `--files` | `bool` | `false` | List all discovered files |
| `--plugins` | `bool` | `false` | List active framework plugins |
| `--boundaries` | `bool` | `false` | Show architecture boundary zones, rules, per-zone file counts, and `logical_groups[]` for `autoDiscover` parents |
| `--workspaces` | `bool` | `false` | Show discovered monorepo workspaces plus any workspace-discovery diagnostics (malformed `package.json`, unreachable glob matches, missing tsconfig references). Available as the `fallow workspaces` alias too. |

Common global flags for this command: [`--format`](#global-flags), [`--quiet`](#global-flags).
<!-- generated:flags:list:end -->
### Examples

```bash
fallow list --files --format json --quiet
fallow list --entry-points --format json --quiet
fallow list --plugins --format json --quiet
fallow list --boundaries --format json --quiet
fallow list --workspaces --format json --quiet
fallow workspaces --format json --quiet  # alias of `fallow list --workspaces`
```

The `--workspaces` JSON output carries `workspaces[]` (name, project-root-relative path, `is_internal_dependency` bool) plus `workspace_diagnostics[]`. Each diagnostic has a `kind` discriminator (`undeclared-workspace`, `malformed-package-json`, `glob-matched-no-package-json`, `malformed-tsconfig`, `tsconfig-reference-dir-missing`) with a typed payload (`error`, `pattern`, or none). The same `workspace_diagnostics[]` array is also surfaced on `fallow dead-code --format json`, `fallow dupes --format json`, and `fallow health --format json` envelopes (omitted when empty). A malformed ROOT `package.json` exits 2 at config load; everything else warns and continues.

The `--boundaries` JSON output carries `boundaries.logical_groups[]` alongside the existing `zones[]` / `rules[]` arrays. Each logical-group entry surfaces a user-authored `autoDiscover` parent zone (which expansion otherwise flattens into per-child zones like `features/auth` / `features/billing`): `name`, `children`, `auto_discover` (verbatim user strings), `status` (`ok` / `empty` / `invalid_path`), `source_zone_index`, summed `file_count`, optional `authored_rule` (the pre-expansion `{ allow, allowTypeOnly }` keyed on the parent), optional `fallback_zone` cross-reference when the parent also kept its own `patterns` (Bulletproof case), optional `merged_from` (parent zone indices when the user declared the same parent name twice; surfaces the duplicate in JSON instead of only in `tracing::warn!`), optional `original_zone_root` (echo of the parent's `root` subtree scope for monorepo patchers), and optional `child_source_indices` (parallel to `children`, attributing each child to a specific `auto_discover` entry when multiple paths were authored). The full shape is documented in `docs/output-schema.json` under `ListBoundariesOutput`.

---

## `init`: Config Generation

Creates a config file in the project root.

### Flags

<!-- generated:flags:init:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--toml` | `bool` | `false` | Create `fallow.toml` instead of `.fallowrc.json` |
| `--agents` | `bool` | `false` | Scaffold a starter `AGENTS.md` guide for coding agents. Prefills Install (from the `packageManager` field, or pnpm via `pnpm-workspace.yaml`), Test (only when exactly one of Vitest / Jest / Playwright is present), Typecheck (`tsc --noEmit` when `tsconfig.json` exists), and monorepo module-boundary lines; everything ambiguous stays blank (no lockfile sniffing). Prefilled command lines carry an HTML provenance comment. Refuses to overwrite an existing `AGENTS.md` |
| `--hooks` | `bool` | `false` | Scaffold a pre-commit git hook that runs `fallow audit --base <ref> --quiet --gate-marker pre-commit`. Alias for `fallow hooks install --target git` |
| `--branch` | `string` | - | Fallback base branch for the pre-commit hook when no upstream is set (default: `main`). Only used with `--hooks` |
| `--decline` | `bool` | `false` | Record that this project deliberately stays unconfigured: persists a decline so the first-contact setup hint and the `setup` next-step stop appearing here. Writes no config file; idempotent |

Common global flags for this command: [`--root`](#global-flags), [`--config`](#global-flags).
<!-- generated:flags:init:end -->
### Examples

```bash
fallow init              # creates .fallowrc.json with $schema
fallow init --toml       # creates fallow.toml
fallow init --agents     # scaffolds a starter AGENTS.md prefilled from detected project info (never overwrites)
fallow hooks install --target git
fallow hooks install --target git --branch develop  # fallback base branch when no upstream is set
```

## `hooks`: Managed Hook Status And Installation

```bash
fallow hooks status --format json
fallow hooks install --target git
fallow hooks install --target agent
fallow hooks uninstall --target git
fallow hooks uninstall --target agent
```

`hooks status` is read-only and reports `git`, `claude`, and `codex` surfaces. Each surface includes `installed`, `managed_block_present`, `user_edited`, and `path`; generated agent scripts also include `script_version` and `min_version_floor`. Use it before mutating setup so agents can distinguish fallow-managed artifacts from user-owned hooks or partial managed blocks.

---

## `migrate`: Config Migration

Migrates configuration from knip and/or jscpd to fallow. Auto-detects config files.

### Flags

<!-- generated:flags:migrate:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--toml` | `bool` | `false` | Output as `fallow.toml` (mutually exclusive with `--jsonc`) |
| `--jsonc` | `bool` | `false` | Write to `.fallowrc.jsonc` instead of `.fallowrc.json`. Same JSONC content either way; the `.jsonc` extension lets editors auto-detect JSON-with-comments syntax highlighting |
| `--dry-run` | `bool` | `false` | Preview without writing |
| `--from` | `string` | - | Specify source config file path |

Common global flags for this command: [`--root`](#global-flags), [`--config`](#global-flags).
<!-- generated:flags:migrate:end -->
Without `--jsonc` or `--toml`, fallow auto-mirrors the source extension: a `knip.jsonc` migration writes `.fallowrc.jsonc`, a `knip.json` migration writes `.fallowrc.json`.

### Detected Source Configs

- `knip.json`, `knip.jsonc`, `.knip.json`, `.knip.jsonc`
- `package.json` embedded `knip` field
- `.jscpd.json`
- `package.json` embedded `jscpd` field

### Examples

```bash
fallow migrate --dry-run        # preview
fallow migrate                  # auto-detect; mirrors source extension
fallow migrate --jsonc          # force .fallowrc.jsonc output
fallow migrate --toml           # output as fallow.toml
fallow migrate --from knip.jsonc
```

---

## `health`: Function Complexity & File Health Analysis

Analyzes function complexity across the project using cyclomatic and cognitive complexity metrics. By default all sections are included (health score, complexity findings, file scores, hotspots, and refactoring targets). Use `--complexity`, `--file-scores`, `--hotspots`, `--targets`, or `--score` to show only specific sections.

Angular templates contribute synthetic `<template>` complexity findings whenever they use `@if`/`@for`/`@switch`/`@case`/`@defer (when ...)`/`@let` blocks, legacy structural directives (`*ngIf`, `*ngFor`), bound attributes (`[x]`, `(x)`, `bind-x`, `on-x`), or `{{ }}` interpolations. Both standalone external `.html` files referenced via `templateUrl` AND inline `@Component({ template: \`...\` })` literals are scanned. Inline-template findings anchor at the host `.ts` file's `@Component` decorator line and emit a `suppress-line` action with `// fallow-ignore-next-line complexity` (place the comment directly above the `@Component` decorator). External-template findings emit a `suppress-file` action with `<!-- fallow-ignore-file complexity -->` (place at the top of the `.html` file; HTML cannot express line-level comments). Tagged template literals containing `${...}` interpolations and `template:` properties bound to a variable are skipped (out of scope for the first cut).

### Flags

<!-- generated:flags:health:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--max-cyclomatic` | `string` | - | Fail if any function exceeds this cyclomatic complexity |
| `--max-cognitive` | `string` | - | Fail if any function exceeds this cognitive complexity |
| `--max-crap` | `string` | - | Fail if any function has CRAP score >= threshold. CRAP combines complexity with coverage (`CC^2 * (1 - cov/100)^3 + CC`). Pair with `--coverage` for accurate per-function CRAP; without Istanbul data fallow estimates coverage from the module graph. |
| `--top` | `string` | - | Only show the top N most complex functions (and file scores/hotspots/targets) |
| `--sort` | `severity\|cyclomatic\|cognitive\|lines` | `cyclomatic` | Sort order for complexity findings |
| `--complexity` | `bool` | `false` | Show only function complexity findings. When no section flags are set, all sections are shown by default. |
| `--complexity-breakdown` | `bool` | `false` | Add a per-decision-point `contributions[]` array to each complexity finding in `--format json`. Each entry names the construct (`if`, `else-if`, `ternary`, boolean operator, loop, `case`, `catch`, `optional-chain`, ...) and carries its source line, the metric it adds to (`cyclomatic` or `cognitive`), its weight, and the nesting depth, so a consumer can explain WHY a function scored high. Off by default (no change to existing JSON/SARIF/markdown). Used by the VS Code inline editor breakdown and the MCP `check_health` `complexity_breakdown` param. |
| `--file-scores` | `bool` | `false` | Show only per-file health scores (maintainability index, LOC, fan-in, fan-out, dead code ratio, complexity density, CRAP risk). Runs the full analysis pipeline. Sorted by risk-aware triage concern: lower maintainability index and higher CRAP risk first. When no section flags are set, all sections are shown by default. |
| `--coverage-gaps` | `bool` | `false` | Show runtime files and exports that no test dependency path reaches. Opt-in (default off). Configure severity via the `coverage-gaps` rule (`error`/`warn`/`off`). |
| `--hotspots` | `bool` | `false` | Show only hotspots: files that are both complex and frequently changing. Combines git churn history with complexity data. Requires a git repository. When no section flags are set, all sections are shown by default. |
| `--ownership` | `bool` | `false` | Attach ownership signals to hotspot entries: bus factor (Avelino truck factor), contributor count, top contributor with stale-days, recent contributors (top-3), `suggested_reviewers`, declared CODEOWNERS owner, `ownership_state`, ownership drift, unowned-hotspot detection. Human output gains a project-level summary line. JSON adds `low-bus-factor`, `unowned-hotspot`, `ownership-drift` action types. Test files get a `[test]` tag. Implies `--hotspots`. Requires git. |
| `--ownership-emails` | `raw\|handle\|anonymized\|hash` | - | Privacy mode for author emails. `handle` shows the local-part only (default, with GitHub noreply unwrap and deterministic same-handle disambiguation). `anonymized` emits stable `xxh3:` pseudonyms; `hash` remains accepted as the legacy spelling. `raw` shows full addresses. Use `anonymized` in regulated environments. Implies `--ownership`. Configure default via `health.ownership.emailMode`. |
| `--targets` | `bool` | `false` | Show only refactoring targets: ranked recommendations based on complexity, coupling, churn, and dead code signals. Categories: churn+complexity, circular dep, high impact, dead code, complexity, coupling. When no section flags are set, all sections are shown by default. Each target's JSON can include `direct_callers[]` (direct importers with the symbols they import) and `clone_siblings[]` (duplicate-code siblings with stable `dup:<8hex>` fingerprints for `fallow dupes --trace`); both omitted when empty. Human output adds `importers:` / `clones:` lines only when that evidence is present. |
| `--css` | `bool` | `false` | Add structural CSS analytics: specificity hotspots, !important density, over-complex selectors, deep nesting, and conservative cleanup candidates. Standard CSS is parsed structurally; preprocessor sources are scanned only where fallow can avoid expanding Sass/Less semantics. Also derives `styling_health`, a descriptive A-F grade for CSS quality scored separately from the code `health_score` (never gates); it weights design-token drift (hardcoded value sprawl) over byte-identical repetition. |
| `--effort` | `low\|medium\|high` | - | Filter refactoring targets by effort level. Implies `--targets`. |
| `--score` | `bool` | `false` | Show only the project health score (0-100) with letter grade (A/B/C/D/F). The score is included by default when no section flags are set. JSON includes `health_score` object with `score`, `grade`, and `penalties` breakdown. As of v2.55.0, plain `--score` skips the churn-backed hotspot penalty so it does not run a `git log` shell-out per invocation; pass `--hotspots` (or `--targets` with `--score`) to include the hotspot penalty. Snapshot (`--save-snapshot`) and trend (`--trend`) flows still trigger hotspot vital signs so saved data stays complete. |
| `--min-score` | `string` | - | Fail (exit 1) only when the health score is below this threshold. Implies `--score`. Authoritative CI quality gate: when set, complexity findings are demoted to informational and the exit code is driven solely by the score, so `--min-score 0` always exits 0. Composes with `--min-severity`. |
| `--min-severity` | `moderate\|high\|critical` | - | Only exit with an error for findings at or above this severity. Composes with `--min-score` (the run fails if either gate trips). |
| `--report-only` | `bool` | `false` | Print the score and findings but never fail CI (always exit 0). Advisory mode. Mutually exclusive with `--min-score` and `--min-severity`. |
| `--since` | `string` | - | Git history window for hotspot analysis. Accepts durations (`6m`, `90d`, `1y`, `2w`) or ISO dates (`2025-06-01`). Ignored when `--churn-file` is set. |
| `--min-commits` | `string` | - | Minimum number of commits for a file to be included in hotspot ranking. |
| `--save-snapshot` | `string` | - | Save vital signs snapshot for trend tracking. Forces file-scores + hotspot computation. |
| `--trend` | `bool` | `false` | Compare current metrics against the most recent saved snapshot. Reads from `.fallow/snapshots/` and shows per-metric deltas with directional indicators (improving/declining/stable). Implies `--score`. |
| `--coverage` | `string` | - | Path to Istanbul-format coverage data (`coverage-final.json`) for accurate per-function CRAP scores. Uses `CC^2 * (1-cov/100)^3 + CC` instead of static binary model. Relative paths resolve against `--root`. Falls back to `FALLOW_COVERAGE`, then `health.coverage`, then auto-detection. |
| `--coverage-root` | `string` | - | Absolute prefix to strip from file paths in coverage data before prepending the project root. For CI/Docker environments where coverage was generated with different absolute paths. Falls back to `FALLOW_COVERAGE_ROOT`, then `health.coverageRoot`. |
| `--runtime-coverage` | `string` | - | Merge runtime-coverage input into the health report. Accepts a V8 coverage directory (`NODE_V8_COVERAGE=...`), a single V8 coverage JSON file, or an Istanbul `coverage-final.json`. One local capture is free and does not require a license; continuous/cloud or multi-capture runtime monitoring requires an active license or trial (`fallow license activate --trial --email <addr>`). JSON output gains a `runtime_coverage` object with a top-level report verdict, per-finding `verdict` (`safe_to_delete` / `review_required` / `low_traffic` / `coverage_unavailable` / `active`), a per-finding suppression `id` (`fallow:prod:<hash>`, hashes the current line), an optional cross-surface `stable_id` join key (`fallow:fn:<hash>`, hashes file + name + start line; one value per function across findings / hot-paths / blast-radius / importance and across V8/Istanbul/oxc producers), an optional content-digest `source_hash` (line-move-immune, so baselines survive a pure line shift), an evidence block, and percentile-ranked hot paths. On protocol-0.3+ sidecars the `summary` also carries an optional `capture_quality` block (`window_seconds`, `instances_observed`, `lazy_parse_warning`, `untracked_ratio_percent`) that flags short-window captures where lazy-parsed scripts may not appear. |
| `--min-invocations-hot` | `string` | `100` | Invocation threshold for hot-path classification. Takes effect only when `--runtime-coverage` is set. |
| `--min-observation-volume` | `string` | - | Minimum total trace volume before the sidecar may emit high-confidence `safe_to_delete` / `review_required` verdicts. Below this, confidence is capped at `medium`. |
| `--low-traffic-threshold` | `string` | - | Fraction of total trace count below which an invoked function is classified `low_traffic` rather than `active`. Expressed as a decimal (0.001 = 0.1%). |

Common global flags for this command: [`--format`](#global-flags), [`--quiet`](#global-flags), [`--changed-since`](#global-flags), [`--churn-file`](#global-flags), [`--workspace`](#global-flags), [`--group-by`](#global-flags), [`--baseline`](#global-flags), [`--save-baseline`](#global-flags), [`--production`](#global-flags), [`--no-production`](#global-flags), [`--explain`](#global-flags).
<!-- generated:flags:health:end -->
### Exit Codes

The gate flag in play determines what drives the exit code. Plain `fallow health` (no gate flag) stays advisory but still fails on any finding (back-compat).

| Invocation | Exit 0 when | Exit 1 when |
|------------|-------------|-------------|
| `fallow health` (no gate flag) | no function exceeds a threshold | any function exceeds a threshold |
| `--min-score N` | score >= N (findings informational) | score < N |
| `--min-severity LEVEL` | no finding at or above LEVEL | any finding at or above LEVEL |
| `--min-score N --min-severity LEVEL` | score >= N AND no finding >= LEVEL | score < N OR a finding >= LEVEL |
| `--report-only` | always | never |

`--report-only` with `--min-score` / `--min-severity` exits 2 (mutually exclusive). The `--runtime-coverage` and coverage-gap gates stay independent and are not demoted by `--min-score`. For gating only newly-introduced complexity, use `fallow audit --gate new-only`.

### Examples

```bash
# Full complexity analysis with JSON output
fallow health --format json --quiet

# Project health score with letter grade
fallow health --format json --quiet --score

# CI gate: fail if score below 70
fallow health --format json --quiet --min-score 70

# Top 10 most complex functions
fallow health --format json --quiet --top 10

# Sort by cognitive complexity
fallow health --format json --quiet --sort cognitive

# Custom thresholds
fallow health --format json --quiet --max-cyclomatic 15 --max-cognitive 10

# Per-file health scores
fallow health --format json --quiet --file-scores

# Top 20 files by triage concern
fallow health --format json --quiet --file-scores --top 20

# Only analyze files changed since main
fallow health --format json --quiet --changed-since main

# Single workspace package
fallow health --format json --quiet --workspace my-package

# Incremental adoption with baseline
fallow health --format json --quiet --save-baseline fallow-baselines/health.json
fallow health --format json --quiet --baseline fallow-baselines/health.json

# CI: fail if any function is too complex
fallow health --max-cyclomatic 25 --max-cognitive 20 --quiet

# Hotspot analysis (complex + frequently changing files)
fallow health --format json --quiet --hotspots

# Hotspots from the last year
fallow health --format json --quiet --hotspots --since 1y

# Hotspots with at least 5 commits
fallow health --format json --quiet --hotspots --min-commits 5

# Top 10 hotspots from the last 90 days
fallow health --format json --quiet --hotspots --since 90d --top 10

# Ranked refactoring recommendations
fallow health --format json --quiet --targets

# Top 5 refactoring targets
fallow health --format json --quiet --targets --top 5

# Only low-effort refactoring targets (quick wins)
fallow health --format json --quiet --effort low

# Save a vital signs snapshot for trend tracking
fallow health --format json --quiet --save-snapshot

# Save snapshot to a custom path
fallow health --format json --quiet --save-snapshot .fallow/baseline-snapshot.json

# Compare current metrics against the most recent snapshot
fallow health --format json --quiet --trend
```

### JSON Output Structure

```json
{
  "kind": "health",
  "schema_version": 7,
  "version": "3.3.0",
  "elapsed_ms": 32,
  "summary": {
    "files_analyzed": 482,
    "functions_analyzed": 3200,
    "functions_above_threshold": 3,
    "max_cyclomatic_threshold": 20,
    "max_cognitive_threshold": 15
  },
  "findings": [
    {
      "path": "src/parser.ts",
      "name": "parseExpression",
      "line": 42,
      "col": 0,
      "cyclomatic": 28,
      "cognitive": 22,
      "line_count": 95,
      "exceeded": "both"
    }
  ]
}
```

`health.thresholdOverrides[]` config entries can raise local cyclomatic, cognitive, CRAP, or unit-size (large-function line-count) ceilings for matching files and optional exact function names. When an override affects output, health JSON includes top-level `threshold_overrides[]` state entries (`active`, `stale`, or `no_match`). Complexity findings evaluated with local ceilings include `effective_thresholds` and `threshold_source: "override"` so agents can see which thresholds drove the finding and avoid treating configured exceptions as hidden suppressions.

When the unit size very-high-risk percentage is >= 3%, the JSON output includes a `large_functions` array listing functions exceeding 60 lines of code:

```json
{
  "large_functions": [
    {
      "path": "src/parser.ts",
      "name": "parseExpression",
      "line": 42,
      "line_count": 95
    }
  ]
}
```

This drill-down shows which specific functions are driving the unit size penalty in the health score, making it actionable without a separate analysis pass.

With `--file-scores`, the JSON output also includes `file_scores` array and `summary.files_scored` / `summary.average_maintainability`:

```json
{
  "summary": {
    "files_scored": 482,
    "average_maintainability": 88.5,
    "coverage_model": "static_estimated",
    "coverage_source_consistency": "uniform"
  },
  "file_scores": [
    {
      "path": "src/parser.ts",
      "fan_in": 8,
      "fan_out": 4,
      "dead_code_ratio": 0.25,
      "complexity_density": 0.22,
      "maintainability_index": 75.1,
      "total_cyclomatic": 42,
      "total_cognitive": 35,
      "function_count": 12,
      "lines": 190,
      "crap_max": 42.0,
      "crap_above_threshold": 2
    }
  ]
}
```

The `file_scores` array is sorted by risk-aware triage concern: the larger of low-MI concern and CRAP risk. This keeps files with very high untested complexity near the top even when their Maintainability Index is not the lowest.

The `crap_max` field is the highest CRAP (Change Risk Anti-Patterns) score among functions in the file, using the canonical formula `CC^2 * (1 - cov/100)^3 + CC`. The default model (`static_estimated`) estimates per-function coverage from export references: directly test-referenced = 85%, indirectly test-reachable = 40%, untested = 0%. Provide `--coverage <path>` with Istanbul-format `coverage-final.json` for exact scores (`istanbul` model). The `crap_above_threshold` field counts functions with CRAP >= 30. When `--file-scores` is active, `summary.coverage_model` indicates the model used (`"static_estimated"` or `"istanbul"`). When CRAP findings carry `coverage_source`, `summary.coverage_source_consistency` is `uniform` or `mixed`; grouped health JSON mirrors this as `groups[].coverage_source_consistency`.

Maintainability index formula: `100 - (complexity_density × 30) - (dead_code_ratio × 20) - min(ln(fan_out+1) × 4, 15)`, clamped to 0–100. Higher is better. Type-only exports are excluded from dead_code_ratio. Zero-function files (barrels) are excluded by default.

With `--hotspots`, the JSON output includes a `hotspots` array and `hotspot_summary`:

```json
{
  "hotspot_summary": {
    "since": "6m",
    "min_commits": 3,
    "files_analyzed": 482,
    "files_excluded": 312,
    "shallow_clone": false
  },
  "hotspots": [
    {
      "path": "src/parser.ts",
      "score": 92,
      "commits": 28,
      "weighted_commits": 34.5,
      "lines_added": 410,
      "lines_deleted": 180,
      "complexity_density": 0.22,
      "fan_in": 8,
      "trend": "Accelerating"
    }
  ]
}
```

Hotspot score formula: `normalized_churn × normalized_complexity × 100`, scaled 0–100. Higher means more urgent to refactor. The `trend` field indicates recent change velocity: `Accelerating` (increasing churn), `Stable` (constant), or `Cooling` (decreasing). Files below `--min-commits` are excluded. The `shallow_clone` field warns when git history is truncated (shallow clone), which may undercount commits.

With `--targets`, the JSON output includes a `targets` array with ranked refactoring recommendations:

```json
{
  "targets": [
    {
      "path": "src/parser.ts",
      "priority": 82.5,
      "efficiency": 27.5,
      "recommendation": "Split high-impact file - 25 dependents amplify every change",
      "category": "split_high_impact",
      "effort": "high",
      "confidence": "medium",
      "factors": [
        {
          "metric": "complexity_density",
          "value": 0.75,
          "threshold": 0.3,
          "detail": "density 0.75 exceeds 0.3"
        },
        {
          "metric": "fan_in",
          "value": 25.0,
          "threshold": 10.0,
          "detail": "25 files depend on this"
        }
      ]
    }
  ],
  "target_thresholds": {
    "fan_in_p95": 12.0,
    "fan_in_p75": 5.0,
    "fan_out_p95": 15.0,
    "fan_out_p90": 8
  }
}
```

Targets are sorted by `efficiency` (priority / effort_numeric) descending, surfacing quick wins first. The `target_thresholds` object exposes the adaptive percentile-based thresholds used for scoring. Priority formula: `min(complexity_density, 1) x 30 + hotspot_boost x 25 + dead_code_ratio x 20 + fan_in_norm x 15 + fan_out_norm x 10`, clamped to 0-100. Fan-in and fan-out normalization uses the project's p95 values (with floors). Categories: `urgent_churn_complexity`, `break_circular_dependency`, `split_high_impact`, `remove_dead_code`, `extract_complex_functions`, `extract_dependencies`, `add_test_coverage`. Each target includes `efficiency`, `effort` (low/medium/high), `confidence` (high/medium/low, data source reliability), and contributing `factors`.

The `add_test_coverage` category fires when a file has 2+ functions with CRAP scores >= 30 and complexity density > 0.3. The `crap_max` metric appears in contributing factors for these targets.

### Vital Signs

All `health` JSON output includes a `vital_signs` object with project-wide metrics:

```json
{
  "vital_signs": {
    "dead_file_pct": 3.2,
    "dead_export_pct": 8.1,
    "avg_cyclomatic": 4.5,
    "critical_complexity_pct": 1.2,
    "p90_cyclomatic": 12,
    "maintainability_avg": 88.5,
    "maintainability_low_pct": 4.1,
    "hotspot_count": 7,
    "hotspot_top_pct_count": 3,
    "circular_dep_count": 2,
    "circular_deps_per_k_files": 4.1,
    "unused_dep_count": 3,
    "unused_deps_per_k_files": 6.2,
    "unit_size_profile": {
      "low_risk": 82.1,
      "medium_risk": 11.4,
      "high_risk": 4.3,
      "very_high_risk": 2.2
    },
    "functions_over_60_loc_per_k": 22.0,
    "unit_interfacing_profile": {
      "low_risk": 95.6,
      "medium_risk": 3.8,
      "high_risk": 0.5,
      "very_high_risk": 0.1
    },
    "p95_fan_in": 8,
    "coupling_high_pct": 2.3
  }
}
```

Fields are `null` when the corresponding data source is not available (e.g., `hotspot_count` is null without `--hotspots` or when git is not available). Health score formula v2 also uses scale-invariant density/tail fields: `critical_complexity_pct`, `hotspot_top_pct_count`, `maintainability_low_pct`, `unused_deps_per_k_files`, `circular_deps_per_k_files`, and `functions_over_60_loc_per_k`. The `unit_size_profile` and `unit_interfacing_profile` are risk distribution histograms (low risk / medium risk / high risk / very high risk as percentages). `p95_fan_in` is the 95th percentile of incoming dependencies. `coupling_high_pct` is the percentage of files above the effective coupling threshold.

With `--score`, the JSON output includes a `health_score` object:

```json
{
  "health_score": {
    "formula_version": 2,
    "score": 76.9,
    "grade": "B",
    "penalties": {
      "dead_files": 3.1,
      "dead_exports": 6.0,
      "complexity": 0.0,
      "p90_complexity": 0.0,
      "maintainability": 0.0,
      "unused_deps": 10.0,
      "circular_deps": 4.0,
      "unit_size": 0.0,
      "coupling": 0.0,
      "duplication": 4.0
    }
  }
}
```

Score is reproducible: `100 - sum(penalties) == score`. `formula_version` identifies the scoring formula; version 2 uses scale-invariant density and tail metrics for monorepo-safe scoring. Penalty fields are absent when the pipeline didn't run. `--score` automatically runs duplication analysis; add `--hotspots` (or combine `--score --targets`) when the score should include the churn-backed hotspot penalty. Grades: A (>= 85), B (70-84), C (55-69), D (40-54), F (< 40).

### Health Trend

With `--trend`, the JSON output includes a `health_trend` object comparing current metrics against the most recent saved snapshot:

```json
{
  "health_trend": {
    "compared_to": {
      "timestamp": "2026-03-25T14:30:00Z",
      "git_sha": "a1b2c3d",
      "score": 74.2,
      "grade": "B"
    },
    "metrics": [
      {
        "name": "score",
        "label": "Health Score",
        "previous": 74.2,
        "current": 76.9,
        "delta": 2.7,
        "direction": "improving",
        "unit": ""
      },
      {
        "name": "dead_file_pct",
        "label": "Dead Files",
        "previous": 5.1,
        "current": 4.2,
        "delta": -0.9,
        "direction": "improving",
        "unit": "%",
        "previous_count": { "value": 13, "total": 255 },
        "current_count": { "value": 11, "total": 262 }
      }
    ],
    "snapshots_loaded": 3,
    "overall_direction": "improving"
  }
}
```

Metrics tracked: `score`, `dead_file_pct`, `dead_export_pct`, `avg_cyclomatic`, `maintainability_avg`, `unused_dep_count`, `circular_dep_count`, `hotspot_count`, `unit_size_very_high_pct`, `p95_fan_in`, `duplication_pct`. Each metric includes `direction` (`improving`, `declining`, `stable`). Percentage metrics include `previous_count`/`current_count` with raw numerator/denominator. `--trend` requires at least one saved snapshot in `.fallow/snapshots/`. When comparing against a snapshot from an older schema version (current: v8), the trend output warns that score deltas may reflect formula changes.

### Vital Signs Snapshots

`--save-snapshot` persists a `VitalSignsSnapshot` JSON file for trend tracking across runs. Snapshots automatically include the health score and grade. The snapshot contains more detail than the inline `vital_signs` object:

```json
{
  "snapshot_schema_version": 8,
  "timestamp": "2025-12-01T10:30:00Z",
  "vital_signs": {
    "dead_file_pct": 3.2,
    "dead_export_pct": 8.1,
    "avg_cyclomatic": 4.5,
    "critical_complexity_pct": 1.2,
    "p90_cyclomatic": 12,
    "maintainability_avg": 88.5,
    "maintainability_low_pct": 4.1,
    "hotspot_count": 7,
    "hotspot_top_pct_count": 3,
    "circular_dep_count": 2,
    "circular_deps_per_k_files": 4.1,
    "unused_dep_count": 3,
    "unused_deps_per_k_files": 6.2,
    "functions_over_60_loc_per_k": 22.0
  },
  "counts": {
    "total_files": 482,
    "dead_files": 15,
    "total_exports": 1200,
    "dead_exports": 97,
    "total_dependencies": 42,
    "unused_dependencies": 3
  },
  "git_sha": "abc1234",
  "git_branch": "main",
  "shallow_clone": false
}
```

The snapshot `snapshot_schema_version` is independent of the report `schema_version`. Default path: `.fallow/snapshots/<timestamp>.json`. The `--save-snapshot` flag forces file-scores and hotspot computation to populate all vital signs fields.

---

## `audit`: Changed-File Quality Gate

Audits changed files for dead code, complexity, duplication, and styling. Returns a verdict (pass/warn/fail). Purpose-built for PR quality gates and reviewing AI-generated code. When `--base` is not set, the base is the `git merge-base` against the branch's upstream or the remote default (`origin/HEAD`, `origin/main`, `origin/master`); set `FALLOW_AUDIT_BASE` to pin it without a flag. Defaults to `--gate new-only`, which fails only on findings introduced by the current changeset and reports inherited findings as context.

### Flags

<!-- generated:flags:audit:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--production-dead-code` | `bool` | `false` | Per-analysis production mode for the dead-code sub-analysis only |
| `--production-health` | `bool` | `false` | Per-analysis production mode for the health sub-analysis only |
| `--production-dupes` | `bool` | `false` | Per-analysis production mode for the duplication sub-analysis only |
| `--dead-code-baseline` | `string` | - | Baseline file (produced by `fallow dead-code --save-baseline`). Pre-existing dead-code issues are excluded from the verdict. |
| `--health-baseline` | `string` | - | Baseline file (produced by `fallow health --save-baseline`). Pre-existing complexity findings are excluded from the verdict. |
| `--dupes-baseline` | `string` | - | Baseline file (produced by `fallow dupes --save-baseline`). Pre-existing clone groups are excluded from the verdict. |
| `--max-crap` | `string` | - | Forwarded to the health sub-analysis. Functions meeting or exceeding this CRAP score cause audit to fail. Same formula as `health --max-crap`. Pair with coverage data for accurate per-function CRAP. |
| `--coverage` | `string` | - | Path to Istanbul-format coverage data (`coverage-final.json`) for accurate per-function CRAP scores in the health sub-analysis. Same format and semantics as `health --coverage`. Also configurable via `FALLOW_COVERAGE`. Relative paths resolve against `--root`. |
| `--coverage-root` | `string` | - | Absolute prefix to strip from file paths in coverage data before prepending the project root. Also configurable via `FALLOW_COVERAGE_ROOT`. Use when coverage was generated under a different checkout root in CI / Docker (e.g., `/home/runner/work/myapp` on GitHub Actions). |
| `--no-css` | `bool` | `false` | Disable styling analytics in audit |
| `--css-deep` | `bool` | `false` | Enable deep CSS analysis for audit explicitly: project-wide styling reachability, narrowed back to changed anchors. Deep CSS is on by default; use this to override `audit.cssDeep = false` |
| `--no-css-deep` | `bool` | `false` | Disable deep CSS analysis while keeping local styling analytics on |
| `--gate` | `new-only\|all` | - | Which findings affect the verdict. `new-only` gates only introduced findings; `all` gates every finding in changed files and skips the extra base-snapshot attribution pass. |
| `--runtime-coverage` | `string` | - | Paid runtime-coverage sidecar input. Accepts a V8 directory, a single V8 JSON file, or an Istanbul coverage map JSON. Spawns the `fallow-cov` sidecar as part of the audit pipeline so the `hot-path-touched` verdict surfaces alongside dead-code and complexity findings without requiring a second `fallow health` invocation in CI. License-gated; the verdict is informational (no exit code change) until a future `--gate hot-path-touched` knob lands |
| `--min-invocations-hot` | `string` | `100` | Threshold for hot-path classification, forwarded to the sidecar when `--runtime-coverage` is set |
| `--gate-marker` | `string` | - | Internal marker identifying a gate run (e.g. `pre-commit`), set by the generated git hook so Fallow Impact can record a containment event when the gate blocks then clears. Hidden; never changes the verdict, exit code, or output |
| `--brief` | `bool` | `false` | Render the deterministic review brief instead of the gating audit report. The brief answers "where do I look?" rather than "will CI block this?", runs the same analysis, and ALWAYS exits 0 (the verdict is carried informationally). Implied by `fallow review`. Orthogonal to `--format` |
| `--max-decisions` | `string` | `4` | Cap on the number of consequential structural decisions surfaced in the review brief's decision surface (the working-memory limit). Default 4; clamped to the 3-5 band (4 plus or minus 1). Only consulted on the brief path |
| `--walkthrough-guide` | `bool` | `false` | Emit the agent-contract WALKTHROUGH GUIDE: the current digest (brief + decision surface), the review direction, the JSON schema the agent must return, and a deterministic graph-snapshot hash pinned into the digest. The digest is built from the graph only (PR prose is never folded in, so it is injection-resistant). Implies the brief; always exits 0. A thin agent skill calls this to fetch the current guide, produces judgment JSON, then reopens with `--walkthrough-file` |
| `--walkthrough-file` | `string` | - | Ingest an agent's judgment JSON and POST-VALIDATE it against the LIVE graph. Rejects any judgment whose `signal_id` fallow did not emit (anti-hallucination); refuses the whole payload as stale when the echoed graph-snapshot hash no longer matches (the tree moved). The verifier is the graph, not a second model. Implies the brief; always exits 0. The agent's free-text framing is fenced as non-deterministic and never gates or auto-posts |
| `--walkthrough` | `bool` | `false` | Render the existing walkthrough guide as a staged HUMAN terminal tour (Stage 1 load-bearing / Stage 2 mechanical), or markdown with `--format markdown`. Implies the brief; always exits 0. `--format json --walkthrough` emits the same agent-contract JSON as `--walkthrough-guide` |
| `--mark-viewed` | `string` | - | Record one or more changed files as VIEWED in the local walkthrough viewed-state ledger (`.fallow/walkthrough-state.json`), then render the tour. Files already viewed (and still current) collapse into the Cleared panel. Repeatable. Stale marks (the tree moved) are ignored on render but never deleted. Only consulted on the `--walkthrough` path |
| `--show-cleared` | `bool` | `false` | Expand the Cleared panel in the human/markdown walkthrough tour: list each de-prioritized and already-viewed file instead of the collapsed one-line summary. Only consulted on the `--walkthrough` path |
| `--show-deprioritized` | `bool` | `false` | Expand the de-prioritized units in the review brief's weighted focus map ("show me what you de-prioritized"). The `deprioritized` escape-hatch list is ALWAYS present in `--format json` regardless; this flag only re-expands the collapse-by-default human focus render. Only consulted on the brief path |

Common global flags for this command: [`--format`](#global-flags), [`--quiet`](#global-flags), [`--changed-since`](#global-flags), [`--diff-file`](#global-flags), [`--diff-stdin`](#global-flags), [`--workspace`](#global-flags), [`--changed-workspaces`](#global-flags), [`--group-by`](#global-flags), [`--output-file`](#global-flags).
<!-- generated:flags:audit:end -->
### Verdicts

| Verdict | Exit code | When |
|---------|-----------|------|
| pass | 0 | No issues in changed files |
| warn | 0 | Issues found, all warn-severity |
| fail | 1 | Error-severity issues found |
| error | 2 | Runtime error (invalid ref, not a git repo) |

With `--gate new-only`, inherited error-severity findings can be present in the JSON output while the verdict remains `pass`; check the `attribution` object and per-finding `introduced` booleans.

### JSON contract: which fields are severity-aware

| Field | Severity-aware? | What it counts |
|-------|-----------------|----------------|
| `verdict` | **yes** | Overall outcome honoring per-rule severity (`pass` / `warn` / `fail`) |
| `attribution.*_introduced` | no | Findings introduced by the changeset under `gate: new-only`, ignoring severity |
| `summary.*` | no | All findings in changed files, ignoring severity |
| Per-finding `introduced` | no | Whether each finding was introduced by the changeset |

For CI gating and any "did this PR pass?" question, read `verdict` (or exit code). Counting introduced findings ignores severity and breaks projects with `unused-exports: warn`. For agent triage, read `verdict` first, then `attribution` for new-vs-inherited counts, then the per-category finding arrays for actionable details.

### Examples

```bash
# Auto-detect base branch
fallow audit --format json --quiet

# Explicit base ref
fallow audit --format json --quiet --base main

# Audit last 3 commits
fallow audit --format json --quiet --base HEAD~3

# Strict mode: fail on inherited findings too
fallow audit --format json --quiet --gate all

# Production code only in a monorepo workspace
fallow audit --format json --quiet --production --workspace @app/api

# Production-only health, full-tree dead-code and dupes
fallow audit --format json --quiet --production-health --workspace @app/api

# CI mode (SARIF + fail on issues + quiet)
fallow audit --ci

# Per-analysis baselines: only fail on genuinely new issues
fallow audit \
  --dead-code-baseline fallow-baselines/dead-code.json \
  --health-baseline    fallow-baselines/health.json \
  --dupes-baseline     fallow-baselines/dupes.json
# Or set these under `audit.*Baseline` in .fallowrc.json so `fallow audit` picks them up with no flags.
# The global --baseline / --save-baseline flags are REJECTED on audit (exit 2) because each sub-analysis uses a different baseline format.
```

### JSON Output Structure

```json
{
  "kind": "audit",
  "schema_version": 7,
  "version": "3.3.0",
  "command": "audit",
  "verdict": "fail",
  "changed_files_count": 12,
  "base_ref": "611d151e8250146426ff3178e94207f8a8d3cc7b",
  "base_description": "merge-base with origin/main",
  "head_sha": "d4a2f91",
  "elapsed_ms": 2140,
  "summary": {
    "dead_code_issues": 2,
    "dead_code_has_errors": true,
    "complexity_findings": 1,
    "max_cyclomatic": 28,
    "duplication_clone_groups": 0
  },
  "attribution": {
    "gate": "new-only",
    "dead_code_introduced": 2,
    "dead_code_inherited": 0,
    "complexity_introduced": 1,
    "complexity_inherited": 0,
    "duplication_introduced": 0,
    "duplication_inherited": 0
  },
  "dead_code": {
    "schema_version": 3,
    "total_issues": 2,
    "unused_exports": [{ "path": "src/api.ts", "export_name": "oldApi", "introduced": true, "actions": [...] }]
  },
  "complexity": {
    "findings": [...]
  },
  "duplication": {
    "clone_groups": []
  }
}
```

The `verdict` field is always present and is the primary decision signal. With the default `new-only` gate, the `attribution` object counts introduced vs inherited findings and audit sub-results annotate individual findings with `introduced: true/false`. With `gate=all`, audit skips that extra base-snapshot attribution pass, so introduced/inherited counts stay `0` and per-finding `introduced` fields are omitted. Dead code, complexity, and duplication sections follow their respective schemas from the individual commands. Thresholds for complexity are inherited from `fallow health` config (defaults: cyclomatic 20, cognitive 15).

Audit creates a temporary git worktree to compare against the base ref. When the current checkout has `node_modules`, audit links it into the base worktree so tsconfig `extends` chains into installed packages and path aliases resolve like the working tree. The worktree is removed on normal exit. If the process is force-killed, run `git worktree prune` to clean up stale `.git/worktrees/fallow-audit-base-*` entries.

---

## `flags`: Feature Flag Detection

Detects feature flag patterns in the codebase. Identifies environment variable flags (`process.env.FEATURE_*`), SDK calls from common providers (LaunchDarkly, Statsig, Unleash, GrowthBook, Split, PostHog, Vercel Flags, ConfigCat, Flagsmith, Optimizely, Eppo), and config object patterns (opt-in). Reports flag locations, detection confidence, and cross-references with dead code findings.

### Flags

<!-- generated:flags:flags:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--top` | `string` | - | Show only the top N flags |

Common global flags for this command: [`--format`](#global-flags), [`--quiet`](#global-flags), [`--changed-since`](#global-flags), [`--workspace`](#global-flags).
<!-- generated:flags:flags:end -->
### Examples

```bash
# Detect all feature flags with JSON output
fallow flags --format json --quiet

# Top 10 flags
fallow flags --format json --quiet --top 10

# Single workspace package
fallow flags --format json --quiet --workspace my-package
```

### JSON Output Structure

```json
{
  "schema_version": 7,
  "version": "3.3.0",
  "elapsed_ms": 116,
  "feature_flags": [],
  "total_flags": 0
}
```

---

## `security`: Security Candidate Detection

Surfaces local security candidates for agent or human verification. The first rule, `client-server-leak`, starts at `"use client"` files and reports a candidate when that client boundary directly reads, or statically imports a path to a module that reads, a non-public `process.env` value.

Findings are not confirmed vulnerabilities. Use the structural trace to verify whether the value can actually reach client-bundled code. Public env conventions (`NODE_ENV`, `NEXT_PUBLIC_*`, `VITE_*`, `NUXT_PUBLIC_*`, `REACT_APP_*`, `PUBLIC_*`, `GATSBY_*`, `EXPO_PUBLIC_*`, `STORYBOOK_*`) are excluded.

The second rule family is a data-driven `tainted-sink` catalogue: syntactic dangerous-sink candidates across the catalogue categories listed below. Most rows require a non-literal argument; narrowly literal-aware rows flag deterministic unsafe literals such as wildcard `postMessage` origins, weak crypto algorithms, disabled TLS validation, and JWT algorithm issues. Fallow prefers false-negatives over false-positives.

| Category | CWE | Sink |
|----------|-----|------|
| `dangerous-html` | 79 | `innerHTML` / `outerHTML` / `insertAdjacentHTML` / `dangerouslySetInnerHTML` |
| `template-escape-bypass` | 79 | template-engine `SafeString(...)` wrapping a non-literal value |
| `command-injection` | 78 | `child_process` `exec` / `execSync` / `spawn` / `spawnSync` (provenance-gated to `node:child_process`) |
| `code-injection` | 94 | `eval` / `vm.runInNewContext` |
| `dynamic-regex` | 1333 | `RegExp(...)` / `new RegExp(...)` with a non-literal pattern |
| `redos-regex` | 1333 | vulnerable regex literals tested with source-backed input |
| `resource-amplification` | 400 | source-backed size into `Array(...)` / `new Array(...)` / `Buffer.alloc*` / `String.prototype.repeat` / `padStart` / `padEnd` (directly `Math.min`-clamped sizes stay quiet) |
| `dynamic-module-load` | 95 | dynamic `require(...)` |
| `sql-injection` | 89 | string concat or interpolated template into `.query()` / `.execute()`, and `sql.raw(...)`. Parameterized `` sql`${x}` `` and the object form `.execute({ sql, args })` are NOT flagged |
| `ssrf` | 918 | `fetch` / `got` / `ky` / `needle` / `request` / `axios` / `superagent` / `undici` / `http(s).request` |
| `path-traversal` | 22 | `path.join` / `path.resolve` / `node:fs` path methods / route `sendFile` |
| `header-injection` | 113 | response `setHeader` / `writeHead` |
| `open-redirect` | 601 | `res.redirect` / `location.href` / `location.assign` / `window.open` |
| `postmessage-wildcard-origin` | 346 | `postMessage(..., "*")` |
| `tls-validation-disabled` | 295 | HTTPS/TLS options with `rejectUnauthorized: false`, plus `NODE_TLS_REJECT_UNAUTHORIZED = "0"` |
| `cleartext-transport` | 319 | cleartext `http://` URLs in fetch-like calls and WebSocket constructors |
| `electron-unsafe-webpreferences` | 1188 | Electron `webPreferences` with unsafe literal options |
| `world-writable-permission` | 732 | `chmod` / `chmodSync` with world-writable modes |
| `insecure-temp-file` | 377 | predictable temporary file paths in `fs` writes |
| `mysql-multiple-statements` | 89 | MySQL connection options with `multipleStatements: true` |
| `permissive-cors` | 942 | CORS wildcard origin with credentials |
| `insecure-cookie` | 614 | cookie options missing or disabling `httpOnly` / `secure` |
| `mass-assignment` | 915 | source-backed `Object.assign(target, source)` |
| `weak-crypto` | 327 | runtime-selectable hash / cipher algorithm |
| `deprecated-cipher` | 327 | `crypto.createCipher` / `createDecipher` |
| `insecure-randomness` | 338 | `crypto.pseudoRandomBytes(...)` and token-like `Math.random()` use |
| `jwt-alg-none` | 347 | JWT signing with algorithm `none` |
| `jwt-verify-missing-algorithms` | 347 | `jsonwebtoken` verify calls missing an `algorithms` allowlist |
| `unsafe-buffer-alloc` | 1188 | `Buffer.allocUnsafe` / `allocUnsafeSlow` |
| `unsafe-deserialization` | 502 | `js-yaml` `load` / `node-serialize` |
| `angular-trusted-html` | 79 | Angular `bypassSecurityTrust*` |
| `nextjs-open-redirect` | 601 | Next.js `redirect` / `permanentRedirect` |
| `dom-document-write` | 79 | `document.write` / `document.writeln` |
| `jquery-html` | 79 | jQuery `.html(value)` |
| `route-send-file` | 22 | Express / Fastify / Hono route `sendFile` |
| `webview-injection` | 94 | react-native-webview injected JavaScript |
| `prototype-pollution` | 1321 | `__proto__` writes and recursive merge sources |
| `zip-slip` | 22 | archive extraction destination paths |
| `nosql-injection` | 943 | Mongo / Mongoose query object passthrough |
| `ssti` | 1336 | template engine compile / render calls |
| `xxe` | 611 | XML parse calls |
| `secret-pii-log` | 532 | source-backed secrets or request PII reaching logs |
| `hardcoded-secret` | 798 | provider-prefix credentials and high-entropy literals assigned to secret-shaped identifiers (include-required) |
| `secret-to-network` | 201 | a non-public `process.env` / `import.meta.env` secret reaching a network call body (`fetch` / `axios` / `got` / ...) via same-identifier flow (include-required) |
| `llm-call-injection` | 1427 | an untrusted source reaching the prompt/messages argument of a known LLM-call sink (taint-path gated, pinned to distinctive LLM SDK call shapes) |
| `xpath-injection` | 643 | `xpath.select` / `select1` with a non-literal expression |

Build-config and test files are excluded from candidate generation. Security rule families default to `off` and are surfaced only by `fallow security`, never under bare `fallow` or the `audit` gate. Scope which catalogue categories run with `security.categories` include / exclude lists in config. Add project-local request object names with `security.requestReceivers`; it extends the built-in `req` / `request` / `ctx` / `context` / `event` allowlist for HTTP `query`, `params`, and `body` reads. The setting is additive only and does not gate `*.searchParams`. `hardcoded-secret` and `secret-to-network` are intentionally include-required and only run when listed in `security.categories.include` (`secret-to-network` is opt-in because legitimate auth is also a secret reaching a network call). Public-by-convention env vars (`NEXT_PUBLIC_`, `VITE_`, ...) are never treated as secrets.

### Flags

<!-- generated:flags:security:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--runtime-coverage` | `string` | - | Paid runtime-coverage sidecar input. Accepts a V8 directory, a single V8 JSON file, or an Istanbul coverage map JSON. When set, `fallow security` annotates tainted-sink candidates with production runtime state and uses that state as an additive ranking signal |
| `--min-invocations-hot` | `string` | `100` | Threshold for hot-path classification, forwarded to the sidecar when `--runtime-coverage` is set |
| `--file` | `string` | - | Scope output to candidates whose finding anchor or trace hop matches the selected file. The full graph is still analyzed |
| `--gate` | `new\|newly-reachable` | - | `new` fails (exit code **8**) only when the change introduces a NEW security-sink candidate in the changed lines. It requires a diff source (`--changed-since`, `--diff-file`, or `--diff-stdin`). `newly-reachable` fails when an existing candidate becomes reachable from entry points compared with `--changed-since <ref>`; diff-only inputs exit 2 because this mode analyzes the base tree. Human output says `REVIEW REQUIRED` (not `FAIL`); SARIF keeps every result at `level: note` with the verdict in `run.properties.fallowGate`; `--format json` carries an additive `gate` block (`mode` / `verdict` / `new_count`) |
| `--surface` | `bool` | `false` | Include the agent-facing `attack_surface[]` inventory in JSON output |

Common global flags for this command: [`--format`](#global-flags), [`--quiet`](#global-flags), [`--changed-since`](#global-flags), [`--diff-file`](#global-flags), [`--diff-stdin`](#global-flags), [`--workspace`](#global-flags), [`--changed-workspaces`](#global-flags).
<!-- generated:flags:security:end -->
### Examples

```bash
fallow security --format json --quiet
fallow security --ci --sarif-file fallow-security.sarif
git diff --unified=0 origin/main...HEAD | fallow security --diff-file -
# Regression gate: fail (exit 8) only on candidates introduced in the changed lines
fallow security --gate new --changed-since origin/main
git diff --cached --unified=0 | fallow security --gate new --diff-stdin

# Reachability gate: fail when existing sinks become entry-point reachable
fallow security --gate newly-reachable --changed-since origin/main
```

### JSON Output Structure

```json
{
  "kind": "security",
  "schema_version": "4",
  "version": "3.3.0",
  "elapsed_ms": 42,
  "config": {
    "rules": {
      "security_client_server_leak": {
        "configured": "off",
        "effective": "warn"
      },
      "security_sink": {
        "configured": "off",
        "effective": "warn"
      }
    },
    "categories_include": null,
    "categories_exclude": null
  },
  "security_findings": [],
  "unresolved_edge_files": 0,
  "unresolved_callee_sites": 0,
  "unresolved_callee_diagnostics": null
}
```

`fallow security --summary --format json --quiet` emits the same `kind`, `schema_version`, `version`, `elapsed_ms`, and `config` metadata, but replaces candidate arrays with `summary` aggregate counts:

```json
{
  "kind": "security",
  "schema_version": "4",
  "version": "3.3.0",
  "elapsed_ms": 42,
  "config": {
    "rules": {
      "security_client_server_leak": {
        "configured": "off",
        "effective": "warn"
      },
      "security_sink": {
        "configured": "off",
        "effective": "warn"
      }
    },
    "categories_include": null,
    "categories_exclude": null
  },
  "summary": {
    "security_findings": 0,
    "by_severity": {
      "high": 0,
      "medium": 0,
      "low": 0
    },
    "by_category": {},
    "by_reachability": {
      "entry_reachable": 0,
      "untrusted_source_reachable": 0,
      "arg_level": 0,
      "module_level": 0,
      "crosses_boundary": 0,
      "source_backed": 0
    },
    "by_runtime_state": {
      "runtime_hot": 0,
      "runtime_cold": 0,
      "never_executed": 0,
      "low_traffic": 0,
      "coverage_unavailable": 0,
      "runtime_unknown": 0,
      "not_collected": 0
    },
    "unresolved_edge_files": 0,
    "unresolved_callee_sites": 0,
    "attack_surface_entries": 0
  }
}
```

Each finding includes `kind`, `path`, `line`, `col`, `evidence`, `trace`, `actions`, `severity`, and optional `reachability`. `severity` is a review-priority tier (`high`, `medium`, or `low`) derived from reachability, boundary, source-backed, and runtime-hot signals; it is not a verified vulnerability verdict and does not change gate or exit semantics. SARIF maps high and medium candidates to `warning`, and low candidates to `note`. `tainted-sink` findings additionally carry `category` (the catalogue id, e.g. `"dangerous-html"`) and `cwe`; `client-server-leak` findings omit both. `tainted-sink` findings can also include `reachability.untrusted_source_trace` when a module with a known untrusted source imports the sink module; it is ranking and triage context only, not proof that a specific value reaches the sink. When set, `reachability.taint_confidence` tiers the association as `"arg-level"` (the sink argument traces to a same-module source read, strong) or `"module-level"` (only the module is import-reachable from a source, weak); tier from this field rather than the evidence text. For arg-level findings the trace's first hop points at the actual source-read line, and module-level source hops carry the role `"module-source"`. `unresolved_edge_files` (client-server-leak) and `unresolved_callee_sites` (tainted-sink) are in-band blind-spot counters: a zero finding count with a non-zero counter is not a clean bill. Suppress a verified false positive with `// fallow-ignore-file security-client-server-leak` (client-server-leak) or `// fallow-ignore-file security-sink` (any tainted-sink category).

When present, `unresolved_callee_diagnostics` adds bounded unresolved-callee metadata for follow-up review: `sampled[]` rows with `path`, `line`, `col`, `reason`, and `expression_kind`, `top_files[]` counts, `by_reason[]` counts, and the emitted sample/top-file limits. It is blind-spot metadata, not a finding list, and follows the same `--file`, `--workspace`, `--changed-since`, and `--gate new` scoping as security candidates.

Every finding also carries an agent-actionable `candidate { source_kind, sink, boundary }`, an optional `taint_flow { source, sink, path }`, and a stable `finding_id`:

- `candidate.source_kind`: the untrusted-input kind that reaches the sink, as a stable catalogue id (`"http-request-input"`, `"process-env"`, `"process-argv"`, `"message-event-data"`, `"location-input"`, ...). Absent when no source matched (always absent for `client-server-leak`). Treat an unknown id as an untrusted source of unknown kind; never drop the candidate on that basis.
- `candidate.sink`: a self-contained sink (`path`, `line`, `col`, `category`, `cwe`, `callee`, optional `url_shape`), actionable without reading the rest of the finding. URL-category sinks use `url_shape` to distinguish `fixed-origin-dynamic-path` from `dynamic-origin` when the construction is statically visible.
- `candidate.boundary`: `client_server` (a `"use client"` file in the trace), `cross_module` (the source reaches the sink across import hops), and optional `architecture_zone` (`from`/`to`) when the anchor also crosses a declared architecture boundary.
- `candidate.network`: present only on `secret-to-network` (#890) candidates. `destination` is the network call's URL when it is a static literal (usually intended auth) or absent when the destination is dynamic (the higher-signal exfil case). Use it to triage exfil from intended auth without re-reading source.
- There is no `impact` field: deciding exploitability is the verifying agent's job; `severity` is only the review-priority tier.
- `taint_flow`: present only when an untrusted source is import-reachable to the sink. `path` is the compact `{ intra_module, cross_module_hops }` shape; the full ordered hops stay in `reachability.untrusted_source_trace`.
- `finding_id`: a stable correlation id, identical across runs for the same rule/path/line and identical to the SARIF `partialFingerprints` value, for tracking a candidate across runs and joining JSON with SARIF.

---

## `inspect`: Target Evidence Bundle

Compose one evidence bundle before editing a file or exported symbol. This is the CLI equivalent of the MCP `inspect_target` tool.

### Usage

```bash
fallow inspect --file src/api.ts --format json --quiet
fallow inspect --symbol src/api.ts:fetchUser --format json --quiet
```

### Target Flags

| Flag | Description |
|------|-------------|
| `--file <PATH>` | Inspect one project-relative file |
| `--symbol <FILE:EXPORT>` | Inspect one exported symbol. Supporting dead-code, duplication, complexity, and security evidence is file-scoped in the first version |

Common global flags: `--format`, `--quiet`, `--root`, `--config`, `--workspace`, `--production`, `--no-cache`, `--threads`.

### JSON Output Structure

```json
{
  "kind": "inspect_target",
  "target": { "type": "file", "file": "src/api.ts" },
  "identity": {
    "file": "src/api.ts",
    "is_reachable": true,
    "is_entry_point": false,
    "export_count": 3,
    "import_count": 2,
    "imported_by_count": 1
  },
  "evidence": {
    "trace_file": { "status": "ok", "scope": "file", "data": {} },
    "dead_code": { "status": "ok", "scope": "file", "data": {} },
    "duplication": { "status": "ok", "scope": "project_filtered_to_file", "data": {} },
    "complexity": { "status": "ok", "scope": "project_filtered_to_file", "data": {} },
    "security": { "status": "ok", "scope": "file", "data": {} }
  },
  "warnings": []
}
```

Each evidence section carries `status` and `scope`. Non-fatal child-analysis failures become section-level errors and warnings, so callers can still use the remaining evidence.

---

## `trace`: Symbol Call Chains

Walk the callers and callees of one exported symbol through the module graph. Callers are the modules that import the symbol (walked up); callees are the symbol's module's import-symbol edges plus its intra-module call sites (walked down). Best-effort and syntactic per ADR-001: resolved and unresolved callees are reported honestly, never silently dropped. This is its own surface, never folded into the ranked review brief.

The target is a positional argument, formatted as `FILE:SYMBOL` (for example `src/utils.ts:formatDate`). When neither `--callers` nor `--callees` is given, both directions are walked.

```bash
fallow trace src/utils.ts:formatDate
fallow trace src/utils.ts:formatDate --callers --depth 3
```

<!-- generated:flags:trace:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--callers` | `bool` | `false` | Walk UP to callers (modules that import the symbol). When neither `--callers` nor `--callees` is set, both directions are walked |
| `--callees` | `bool` | `false` | Walk DOWN to callees (the symbol's module's import-symbol edges plus unresolved call sites). When neither flag is set, both are walked |
| `--depth` | `string` | - | Chain depth bound for both directions (default 2). Symbol-level is best-effort, so a shallow bound keeps the trace legible |

Common global flags for this command: [`--format`](#global-flags), [`--quiet`](#global-flags), [`--root`](#global-flags), [`--config`](#global-flags).
<!-- generated:flags:trace:end -->

---

## `decision-surface`: Structural Decisions

Surface only the consequential structural decisions a change embeds (the apex of the review brief): a ranked, capped (3 to 5, default 4) set of coupling/boundary, public-API/contract, and dependency decisions, each framed as a judgment question with the routed expert to ask, plus a trade-off clause and the count of in-repo consumers that already depend on the anchor. Runs the same changed-code analysis as `fallow review` but emits only the decisions, separable and cheap. Always exits 0 (advisory, never a gate); every decision is suppressible with `// fallow-ignore`. Use `--base` / `--changed-since` to pick the comparison point, exactly like `fallow audit`.

```bash
fallow decision-surface --base main
fallow decision-surface --base main --format json --quiet
```

<!-- generated:flags:decision-surface:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--max-decisions` | `string` | `4` | Cap on the number of surfaced decisions (the working-memory limit). Default 4; clamped to the 3-5 band (4 plus or minus 1) |

Common global flags for this command: [`--changed-since`](#global-flags), [`--format`](#global-flags), [`--quiet`](#global-flags), [`--workspace`](#global-flags), [`--root`](#global-flags), [`--config`](#global-flags).
<!-- generated:flags:decision-surface:end -->

---

## `explain`: Rule Explanation

Print rule rationale, examples, fix guidance, and docs URL for one issue type without running analysis.

### Usage

```bash
fallow explain unused-export
fallow explain fallow/code-duplication --format json --quiet
```

### Arguments

| Argument | Description |
|----------|-------------|
| `<issue-type>` | Issue type token or rule id, for example `unused-export`, `unused-exports`, `fallow/unused-dependency`, `high-complexity`, or `code-duplication`. |

### JSON Output Structure

```json
{
  "id": "fallow/unused-export",
  "name": "Unused Exports",
  "summary": "Export is never imported",
  "rationale": "Named exports that are never imported by any other module in the project. Includes both direct exports and re-exports through barrel files. The export may still be used locally within the same file.",
  "example": "export const formatPrice = ... exists in src/money.ts, but no module imports formatPrice.",
  "how_to_fix": "Remove the export or make it file-local. If it is public API, import it from an entry point or add an intentional suppression with context.",
  "docs": "https://docs.fallow.tools/explanations/dead-code#unused-exports"
}
```

MCP equivalent: `fallow_explain` with required `issue_type`.

---

## `schema`: Capability Manifest

Dumps fallow's complete capability manifest as machine-readable JSON (always JSON, regardless of `--format`). The single source of truth for agent introspection.

```bash
fallow schema
```

Top-level blocks:

- `manifest_version`: manifest shape discriminator (currently `"1"`).
- `commands` + `global_flags`: every CLI command and flag, derived live from the CLI definition.
- `issue_types`: one row per reportable issue type across ALL analyses (dead-code, health, dupes, flags, security). Each row carries `id` (the bare rule id; several rows share one suppression token, e.g. all complexity rules suppress via `complexity`), `rule_id` (SARIF id), `command`, `category`, `filter_flag` (null when none), `fixable`, `suppressible`, `suppress_comment` (copy-pasteable, null when not suppressible), `note`, `license` (`free` | `freemium`), and `docs_url`. Nullable fields are always present (null, never absent).
- `mcp_tools`: all MCP server tools with `kind` grouping (analysis/trace/fix/introspection/runtime-coverage/composition), one-line description, `cli_command` nearest CLI fallback, `key_params` (curated subset; live MCP `list_tools` schemas are authoritative), `license` + `license_note` (the 5 runtime-coverage tools are `freemium`: a single local capture is free, continuous monitoring is paid), and `read_only`.
- `plugins`: built-in framework plugin count + names, derived live from the registry.
- `environment_variables`: every user-facing `FALLOW_*` variable (internal plumbing excluded).
- `output_formats`, `exit_codes`, `severity_levels`, `suppression_comments`.

---

## `config-schema`: Config JSON Schema

Prints the JSON Schema for fallow configuration files.

```bash
fallow config-schema > schema.json
```

---

## `plugin-schema`: Plugin JSON Schema

Prints the JSON Schema for external plugin definition files.

```bash
fallow plugin-schema > plugin-schema.json
```

---

## `plugin-check`: Verify external plugins

Read-only dry-run of your external plugins. Reports, per plugin, whether it activated (with the unmet `detection`/`enabler` requirement when inactive), and for `manifestEntries` rules which manifests each matched, what it seeded (with `path_exists`), and typed warnings (`manifests-matched-none`, `when-excluded-all`, `field-path-unresolved`, `entries-empty`, `manifest-parse-failed`, `entry-outside-root`, `seeded-paths-missing`). Run it after authoring a `fallow-plugin-*.jsonc` to verify it before a full analysis. Deterministic output; always exits 0 (advisory, never a gate).

```bash
fallow plugin-check --format json
```

---

## `rule-pack-schema`: Rule Pack JSON Schema

Prints the JSON Schema for declarative rule pack files (the `rulePacks` config key), for editor autocomplete when authoring packs.

```bash
fallow rule-pack-schema > rule-pack-schema.json
```

Pack files can also reference the published schema directly: `"$schema": "https://raw.githubusercontent.com/fallow-rs/fallow/main/rule-pack-schema.json"`.

---

## `license`: Manage Continuous Runtime License

Manage the local JWT used to unlock continuous/cloud runtime monitoring. Single-capture local runtime analysis does not require a license. Verification is fully offline against an Ed25519 public key compiled into the binary. Only `--trial` and `refresh` hit the network (`api.fallow.cloud`, 5s connect / 10s total timeout).

```bash
fallow license activate --trial --email you@company.com
fallow license activate eyJhbGciOiJFZERTQSIsInR5cCI6IkpXVCJ9...
fallow license activate --from-file ./license.jwt
cat ./license.jwt | fallow license activate --stdin
fallow license status
fallow license refresh
fallow license deactivate
```

### Subcommands

| Subcommand | Purpose |
|------------|---------|
| `activate` | Install a JWT or start a 30-day trial. JWT input precedence: positional arg > `--from-file` > `--stdin`. |
| `status`   | Print tier, seats, features, days-until-expiry, and (when `refresh_after` has passed) a proactive refresh hint. |
| `refresh`  | Fetch a fresh JWT using the currently stored one as identity proof. Exit 7 on network failure. |
| `deactivate` | Remove the local license file. |

### `activate` flags

| Flag | Type | Description |
|------|------|-------------|
| `--trial` | bool | Start a 30-day email-gated trial. Requires `--email`. **Rate-limited to 5 requests per hour per IP** - in CI or behind a shared NAT, start the trial locally and set `FALLOW_LICENSE` on the runner. |
| `--email <ADDR>` | string | Email for the trial flow. On success, `trialEndsAt` is printed to stdout so you can see the trial window without decoding the JWT. |
| `--from-file <PATH>` | path | Read a JWT from a file. |
| `--stdin` | bool | Read a JWT from stdin. Conflicts with `--from-file` and positional JWT. |

### Storage precedence

1. `FALLOW_LICENSE` (env var holding the full JWT string)
2. `FALLOW_LICENSE_PATH` (env var pointing at a file)
3. `~/.fallow/license.jwt` (default; written `chmod 0600` on Unix)

### Grace ladder

| Days past `exp` | State | Behavior |
|-----------------|-------|----------|
| `<= 7` | ExpiredWarning | Analysis runs; CLI prints a refresh hint |
| `> 7, <= 30` | ExpiredWatermark | Analysis runs; output gains a visible watermark until refreshed |
| `> 30` | HardFail | Continuous/cloud runtime monitoring is blocked; run `fallow license refresh` or start a new trial |

### Actionable error messages

On HTTP error from `api.fallow.cloud`, fallow parses the `{error, message, code}` envelope and maps known codes to targeted hints:

| Operation + code | CLI message |
|------------------|-------------|
| `refresh` + `token_stale` | `your stored license is too stale to refresh. Reactivate with: fallow license activate --trial --email <addr>` |
| `refresh` + `invalid_token` | `your stored license token is missing required claims. Reactivate with: fallow license activate --trial --email <addr>` |
| `refresh` or `trial` + `unauthorized` | `authentication failed. Reactivate with: fallow license activate --trial --email <addr>` |
| `trial` + `rate_limit_exceeded` | `trial creation is rate-limited to 5 per hour per IP. Wait an hour or retry from a different network (in CI, start the trial locally and set FALLOW_LICENSE on the runner).` |

Unknown codes fall back to the backend's `message` field, or the raw body.

### Clock skew

License verification rejects JWTs whose `iat` claim is more than 24 hours in the future relative to the local system clock. The same check catches both a forward-signed JWT and a local clock behind reality. Rejection exits non-zero so paid features fail closed.

| Env var | Default | Effect |
|---------|---------|--------|
| `FALLOW_LICENSE_SKEW_TOLERANCE_SECONDS` | `86400` (24h) | Overrides the tolerance window applied to the `iat` claim. Lenient parsing: unset / empty / unparsable / negative all fall back to the default. |

Common non-user causes: CI containers without NTP, machines with a dead BIOS battery, drifted laptop clocks after long sleep.

### Exit Codes

| Code | Meaning |
|------|---------|
| `0`  | Valid license (or trial/refresh succeeded) |
| `2`  | Bad invocation (missing email for `--trial`, unreadable file) |
| `3`  | License missing, hard-fail expired, malformed JWT, or clock skew exceeds tolerance |
| `7`  | Network failure or non-success HTTP status from `api.fallow.cloud` |

---

## `telemetry`: Opt-in Product Telemetry

Manage opt-in, off-by-default product telemetry that helps prioritize agent, CI, MCP, and editor workflows. Fallow never collects repository names, file paths, package or dependency names, source code, config values, environment variable names or values, raw command lines, or raw errors. Hashing those values is not used as a workaround.

```bash
fallow telemetry status              # effective state, source, and config path
fallow telemetry enable              # opt in (user action only; agents must not run this)
fallow telemetry disable             # opt out
fallow telemetry inspect --example   # print an example payload + field purposes
```

Inspect the exact payload a real command would send, without sending it:

```bash
FALLOW_TELEMETRY=inspect fallow audit --format json --quiet
```

The inspected payload prints to stderr; stdout (including `--format json`) is untouched.

### Behavior

- **Off by default.** Precedence: `DO_NOT_TRACK` / `FALLOW_TELEMETRY_DISABLED` (kill switches) > `FALLOW_TELEMETRY_DEBUG` (forces inspect) > `FALLOW_TELEMETRY` env > CI (off unless `FALLOW_TELEMETRY` is set) > user config (`fallow telemetry enable/disable`) > off.
- **CI is off** unless `FALLOW_TELEMETRY` is explicitly set in that CI environment; a local `enable` never turns on org CI telemetry.
- **Decision status:** `fallow telemetry status --format json` includes `explicit_decision`. `false` means the user may have only seen the notice; `true` means `telemetry enable` or `telemetry disable` was explicitly run.
- **Transport:** when enabled, one small JSON event is POSTed to `https://api.fallow.cloud/v1/telemetry/events` (override with `FALLOW_API_URL`), no auth token, no cookies, on a background thread so it does not delay your command. Delivery is best-effort; errors never change output or exit code.
- **Agent source:** wrappers may set `FALLOW_AGENT_SOURCE=<allowlisted-value>` so an enabled run is attributed correctly. Allowlist: `codex`, `claude_code`, `cursor`, `copilot`, `opencode`, `aider`, `roo`, `windsurf`, `gemini` (aliases `gemini_cli`/`antigravity`), `cline`, `continue`, `zed`, `goose`, `other_known`, `unknown`, `none`. Setting it never enables telemetry and uploads no codebase content.

---

## `coverage`: Production-Coverage Workflow

Helper subcommand for runtime coverage setup, focused analysis, and cloud inventory upload. Three subcommands today:

- `coverage setup` - resumable state machine that wires sidecar installation, framework-aware coverage recipe writing, optional license activation for continuous monitoring, and automatic handoff into `fallow health --runtime-coverage`.
- `coverage analyze` - focused runtime coverage analysis. Local mode reads `--runtime-coverage <path>`; cloud mode requires explicit `--cloud`, `--runtime-coverage-cloud`, or `FALLOW_RUNTIME_COVERAGE_SOURCE=cloud` and never triggers from `FALLOW_API_KEY` alone.
- `coverage upload-inventory` - push a static function inventory to fallow cloud so the dashboard can surface `untracked` functions (those in the codebase but never called at runtime).

```bash
fallow coverage setup                         # interactive
fallow coverage setup --yes                   # accept all prompts
fallow coverage setup --non-interactive       # print instructions, do not prompt
fallow coverage setup --yes --json            # agent-readable JSON, no prompts/writes/installs/network
fallow coverage setup --yes --json --explain  # add _meta field docs, enums, warnings, docs URL

fallow coverage analyze --runtime-coverage ./coverage --format json
fallow coverage analyze --cloud --repo owner/repo --format json

fallow coverage upload-inventory              # infers project-id, git-sha, API key
fallow coverage upload-inventory --dry-run    # print what would be uploaded, exit 0

fallow coverage upload-source-maps --dir dist           # upload build source maps from CI
fallow coverage upload-source-maps --dry-run            # print maps and fileName values, no network
```

`--json` is the agent-driven entry point: implies `--non-interactive`, never writes files, never installs the sidecar, never makes network calls, and produces a stable JSON payload with these top-level keys: `schema_version` (string `"1"`), `framework_detected`, `package_manager`, `runtime_targets`, `members`, `config_written`, `commands`, `files_to_edit`, `snippets`, `dockerfile_snippet`, `next_steps`, `warnings`. Add `--explain` to inject an opt-in `_meta` block with field definitions, enum values, warning semantics, and the docs URL; `schema_version` stays `"1"`. `framework_detected` uses canonical ids (`nextjs`, `nestjs`, `nuxt`, `sveltekit`, `astro`, `remix`, `vite`, `plain_node`, `unknown`). When both a Node-server framework (Elysia, Hono, Fastify, Express, Koa, `@trpc/server`) and Vite appear in the same `package.json`, the Node-server framework wins. Workspace projects emit one `members[]` entry per runtime-bearing workspace (each with its own `framework_detected`, `package_manager`, `runtime_targets`, `files_to_edit`, `snippets`, `dockerfile_snippet`, `warnings`); top-level fields mirror the first emitted runtime member, and `runtime_targets` at top level is the union (`[]`, `["node"]`, `["browser"]`, or `["node", "browser"]`) across all members. Single-app projects emit a `members[]` array of length 1 (path `"."`) so consumers can treat it uniformly. Library-only workspaces (no `start`/`preview`/`dev` script and no Node-server dependency) are skipped, as are aggregator roots whose only `dev` / `preview` script delegates to a tool other than vite (e.g., `turbo dev`, `nx run-many`); when no runtime members are found, the payload reports `framework_detected: "unknown"`, `runtime_targets: []`, `members: []`, and a `warnings` entry of `"No runtime workspace members were detected; emitted install commands only."`. A Vite browser app is recognized when `vite` is a dependency AND either a `dev`/`preview` script invokes `vite` (or `vite-preview` / `vite-plus` / `vp`) OR the workspace contains an `index.html` or `src/main.{ts,tsx,js,jsx,mts,mjs}` entry.

### `setup` flow

1. **License check** - if missing or hard-fail, offers to start a trial.
2. **Sidecar discovery** - resolves `FALLOW_COV_BIN`, `FALLOW_COV_BINARY_PATH`, platform-package binaries in npm/bun/pnpm layouts, project-local `node_modules/.bin/fallow-cov`, package-manager bin, `~/.fallow/bin/fallow-cov`, and `PATH`. When an explicit env path is set but points to a non-existent file, setup errors fast instead of falling through.
3. **Coverage recipe** - detects framework (Next.js, Nuxt, Astro, SvelteKit, Remix, NestJS, Vite browser apps, plain Node) and package manager (npm, pnpm, yarn, bun), then writes `docs/collect-coverage.md` with the correct commands.
4. **Handoff** - if `./coverage/coverage-final.json` or a V8 coverage directory already exists, setup runs `fallow health --runtime-coverage <path>` directly.

### `analyze` flags

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--runtime-coverage <PATH>` | path | none | Local V8 directory, V8 JSON file, or Istanbul coverage map. Mutually exclusive with cloud mode. |
| `--cloud`, `--runtime-coverage-cloud` | bool | false | Explicitly fetch cloud runtime facts from `/v1/coverage/:repo/runtime-context`. |
| `--api-key <KEY>` | string | `$FALLOW_API_KEY` | Fallow cloud bearer token, used only after explicit cloud opt-in. |
| `--api-endpoint <URL>` | string | `$FALLOW_API_URL` or `https://api.fallow.cloud` | Override for staging / on-prem. |
| `--repo <OWNER/REPO>` | string | `$FALLOW_REPO`, then parsed git origin | Repository whose latest cloud runtime facts should be pulled. Slashes are percent-encoded as one route segment. |
| `--coverage-period <DAYS>` | integer | 30 | Cloud observation window, 1 through 90 days. |
| `--project-id <ID>` | string | none | Optional project discriminator for monorepos. |
| `--environment <NAME>` | string | none | Optional environment filter. |
| `--commit-sha <SHA>` | string | none | Optional advanced filter for a specific observed commit. |
| `--top <N>` | integer | unset | Show only the top N runtime findings, hot paths, blast-radius entries, and importance entries. Truncation happens before rendering, so it propagates to JSON, human, and cloud-merge output equally. |
| `--blast-radius` | bool | false | Show the first-class blast-radius section in human output. JSON always includes `runtime_coverage.blast_radius` whenever runtime coverage analysis runs. |
| `--importance` | bool | false | Show the first-class importance section in human output. JSON always includes `runtime_coverage.importance` whenever runtime coverage analysis runs. |
| `--production` | bool | false | Run analyze in production mode, matching `fallow health --production`. Filters out test files and dev-only code paths before merging runtime data. |
| `--min-invocations-hot <N>` | integer | 100 | Hot-path classification threshold. Functions invoked at least N times during the captured window are classified as hot. Mirrors the same flag on `fallow health --runtime-coverage`. |
| `--min-observation-volume <N>` | integer | 5000 | Minimum total trace volume before the sidecar emits high-confidence `safe_to_delete` / `review_required` verdicts. Below this, confidence is capped at `medium`. |
| `--low-traffic-threshold <RATIO>` | decimal | 0.001 | Fraction of total trace count below which an invoked function is classified `low_traffic` rather than `active`. `0.001` = 0.1%. |
| `--explain` | bool | false | With `--format json`, attach a top-level `_meta` block with field definitions, enum values (`data_source`, `test_coverage`, `v8_tracking`, `action_type`, etc.), warning-code documentation, and the docs URL. |

Cloud analysis emits the same `runtime_coverage` JSON block as local mode. Its summary includes `data_source: "cloud"`, `last_received_at`, and `capture_quality` derived from the pulled runtime window. Cloud functions that cannot be matched to the local AST/static index are omitted from findings and reported through a `cloud_functions_unmatched` warning.

Each finding's `actions[].type` uses the canonical kebab-case vocabulary: `delete-cold-code` is emitted on `verdict=safe_to_delete`, `review-runtime` on `verdict=review_required`. The sidecar may emit additional protocol-specific identifiers, so consumers should treat unknown values as forward-compat extensions rather than schema violations.

### `upload-inventory` flags

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--api-key <KEY>` | string | `$FALLOW_API_KEY` | Fallow cloud bearer token. Generate at `https://fallow.cloud/settings#api-keys`. **Prefer `$FALLOW_API_KEY` on shared CI runners**: `--api-key` on the command line may be visible to other processes via `ps`. |
| `--api-endpoint <URL>` | string | `$FALLOW_API_URL` or `https://api.fallow.cloud` | Override for staging / on-prem. |
| `--project-id <OWNER/REPO>` | string | `$GITHUB_REPOSITORY` → `$CI_PROJECT_PATH` → `git remote get-url origin` | Project identifier. |
| `--git-sha <SHA>` | string | `git rev-parse HEAD` | Commit SHA this inventory is keyed to. Max 64 chars; `[A-Za-z0-9._-]` only. |
| `--allow-dirty` | bool | `false` | Silence the warning when the working tree has uncommitted changes. |
| `--exclude-paths <GLOB>` | glob | none | Additional globs to skip (repeatable), applied after the configured fallow ignore rules. |
| `--path-prefix <PREFIX>` | string | none | Prefix prepended to every emitted `filePath` so inventory matches runtime paths. Required for containerized deployments (runtime reports `/app/src/*` while the walker emits `src/*`). Common values: `/app`, `/workspace`, `/usr/src/app`, `/var/task`, `/home/runner/work/<repo>/<repo>`. Must start with `/`. |
| `--dry-run` | bool | `false` | Print what would be uploaded and exit. No network call. |
| `--ignore-upload-errors` | bool | `false` | Treat upload failures as warnings (exit 0). Validation errors still fail hard. |

Only plain JS/TS/JSX/TSX sources are walked. Declaration files (`*.d.ts`, `*.d.mts`, `*.d.cts`, `*.d.tsx`) and bodyless function signatures (TS overloads, `abstract` methods, `declare function`) are intentionally skipped; they have no runtime footprint. Function names match `oxc-coverage-instrument` byte-for-byte so the join with runtime coverage succeeds.

### Environment

- `FALLOW_COV_BIN` - explicit override for the sidecar binary (for `setup`). Wins over all other discovery paths. Must point to an existing file.
- `FALLOW_API_KEY` - fallow cloud bearer token (for `upload-inventory` and `upload-source-maps`). Overridden by `--api-key` for `upload-inventory`; `upload-source-maps` reads only the env var so secrets stay out of argv.
- `FALLOW_API_URL` - base URL for cloud calls. Overridden by `--api-endpoint`.
- `FALLOW_CA_BUNDLE` - PEM certificate bundle for cloud calls. Relative paths resolve from the process cwd. The bundle replaces default WebPKI roots, so private-CA runners should pass a complete bundle that includes public roots plus the private CA.

### `coverage upload-source-maps` flags

Coverage CI helper for bundled/minified runtime coverage. It scans a build directory for `.map` files and uploads them to `/v1/coverage/:repo/source-maps` keyed by the commit SHA the beacon reports.

Uploads retry network failures, HTTP 429, and HTTP 502/503/504 up to three attempts. HTTP 429 honors `Retry-After` delta seconds and HTTP-date values, capped at 60 seconds. Setup or transport failures that prevent every map from uploading exit 7; mixed per-map failures still exit 1.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--dir <PATH>` | path | `dist` | Directory scanned recursively. |
| `--include <GLOB>` | glob | `**/*.map` | Include glob relative to `--dir`. |
| `--exclude <GLOB>` | glob | `**/node_modules/**` | Exclude glob, repeatable. |
| `--repo <NAME>` | string | `package.json` `repository.url`, then `git remote get-url origin` parsed to `owner/repo` | Repo identifier used in the source-map API path. Must match the beacon's `projectId` (and `upload-inventory`'s `--project-id`); pass `--repo <bare-name>` explicitly if the beacon reports a bare name. |
| `--git-sha <SHA>` | string | `$GITHUB_SHA` -> `$CI_COMMIT_SHA` -> `$COMMIT_SHA` -> `git rev-parse HEAD` | Commit SHA, 7-40 hex chars. |
| `--endpoint <URL>` | string | `$FALLOW_API_URL` or `https://api.fallow.cloud` | Override for staging / on-prem. |
| `--strip-path <BOOL>` | bool | `true` | Upload basename-only `fileName` values. Use `--strip-path=false` when runtime coverage reports paths like `assets/app.js`. |
| `--dry-run` | bool | `false` | Print what would upload; no API key or network call. |
| `--concurrency <N>` | integer | `4` | Parallel upload fanout. |
| `--fail-fast` | bool | `false` | Stop on the first upload failure. |

### Exit Codes

| Code | Meaning |
|------|---------|
| `0`  | Setup complete / upload succeeded / dry-run printed |
| `2`  | Bad invocation, unable to resolve sidecar via env override (`setup`) |
| `4`  | Sidecar install failed (`setup`) |
| `5`  | Coverage input could not be pre-processed (`setup`) |
| `7`  | Network failure (trial activation for `setup`; upload DNS/TLS/connect for `upload-inventory`) |
| `10` | Validation error: missing API key, unresolvable project-id, zero functions (`upload-inventory`) |
| `11` | Payload too large: inventory exceeds the 200,000-function server cap (`upload-inventory`) |
| `12` | Auth rejected: 401 / 403 from the server (`upload-inventory`) |
| `13` | Server error: 5xx or other non-2xx status (`upload-inventory`) |

---

## `config`: Show Resolved Config

Prints the loaded config file path and the resolved config (with `extends` merged) as JSON. Useful for verifying which config fallow picked up, especially in monorepos.

```bash
fallow config            # path on first line, JSON below
fallow config --path     # only the path (scriptable)
```

### Flags

<!-- generated:flags:config:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--path` | `bool` | `false` | Print only the config file path, no JSON |

Common global flags for this command: [`--format`](#global-flags), [`--quiet`](#global-flags), [`--config`](#global-flags), [`--root`](#global-flags).
<!-- generated:flags:config:end -->
### Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Config file found and loaded |
| `2` | Error (parse failure, explicit `--config` path missing) |
| `3` | No config file found; defaults are in effect |

Honors the global `--config <path>` flag: if passed, that path is loaded directly instead of walking the directory tree.

The `loaded config: <path>` line is also emitted to stderr automatically at the start of every human-format CLI run (suppressed by `--quiet` and non-human formats).

---

## Global Flags

Available on all commands:

<!-- generated:flags:global:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `-r, --root` | `string` | - | Project root directory |
| `-c, --config` | `string` | - | Config file path |
| `--allow-remote-extends` | `bool` | `false` | Allow trusted config files to extend HTTPS URLs |
| `-f, --format` | `human\|json\|sarif\|compact\|markdown\|codeclimate\|pr-comment-github\|pr-comment-gitlab\|review-github\|review-gitlab\|badge\|github-annotations\|github-summary` | `human` | Output format (alias: --output) |
| `-q, --quiet` | `bool` | `false` | Suppress progress output |
| `--no-cache` | `bool` | `false` | Disable incremental caching |
| `--threads` | `string` | - | Number of parser threads |
| `--changed-since` | `string` | - | Only report issues in files changed since this git ref (e.g., main, HEAD~5) |
| `--diff-file` | `string` | - | Unified diff for line-level scoping. Use `-` to read from stdin. Project-level findings still bypass this filter. When both this and `--changed-since` are set, the diff filter wins for finding scope while `--changed-since` still drives file discovery |
| `--diff-stdin` | `bool` | `false` | Read the unified diff from stdin. Equivalent to `--diff-file -` |
| `--churn-file` | `string` | - | Import change history from a `fallow-churn/v1` JSON file instead of `git log`, powering hotspots, ownership, and bus-factor on projects with no git repository (Yandex Arc, Mercurial, Perforce). A small wrapper translates your VCS log into the contract. Resolved relative to `--root`. Affects `health --hotspots` / `--ownership` / `--targets` only; `audit`, `impact`, and `--changed-since` still require git |
| `--max-file-size` | `string` | - | Skip source files larger than this many megabytes (default 5) instead of parsing them, guarding against the out-of-memory blowup a single multi-MB generated/vendored/bundled file causes on large repos. Use `0` for no limit. Declaration files (`.d.ts`) are always analyzed. Skipped files are reported and excluded from every analysis. Also settable via `FALLOW_MAX_FILE_SIZE` |
| `--baseline` | `string` | - | Compare to baseline |
| `--parent-run` | `string` | - | Correlate this run with a previous telemetry analysis run |
| `--save-baseline` | `string` | - | Save results as baseline |
| `--production` | `bool` | `false` | Exclude test/dev files, only start/build scripts (applies to every analysis) |
| `--no-production` | `bool` | `false` | Force production mode OFF for every analysis, overriding a project config's `production: true` (and `FALLOW_PRODUCTION`). Conflicts with `--production` |
| `--production-dead-code` | `bool` | `false` | Run dead-code analysis in production mode when using bare combined mode |
| `--production-health` | `bool` | `false` | Run health analysis in production mode when using bare combined mode |
| `--production-dead-code` / `--production-health` / `--production-dupes` | `bool` | `false` | Per-analysis production mode for bare combined runs and `fallow audit`. Per-analysis env vars `FALLOW_PRODUCTION_DEAD_CODE`/`HEALTH`/`DUPES` mirror these flags. Per-analysis env beats global `FALLOW_PRODUCTION`. |
| `-w, --workspace` | `string` | - | Scope to one or more workspaces (comma-separated, globs, `!` negation) |
| `--changed-workspaces` | `string` | - | Git-derived monorepo CI scoping: scope to workspaces containing any file changed since `REF`. Mutually exclusive with `--workspace`. Missing ref is a hard error. |
| `--group-by` | `owner\|directory\|package\|section` | - | Group output by CODEOWNERS ownership (`owner`), first path component (`directory`), workspace package (`package`, aliases: `workspace`, `pkg`), or GitLab CODEOWNERS `[Section]` headers (`section`, alias: `gl-section`). All output formats partition issues into labeled groups. `section` mode attaches an `owners` array to each group in JSON output |
| `--performance` | `bool` | `false` | Show pipeline timing breakdown |
| `--explain` | `bool` | `false` | JSON: include metric definitions in `_meta`. Human: print a `Description:` line under each section header. Always on for MCP. |
| `--explain-skipped` | `bool` | `false` | Show a per-pattern breakdown for default duplicate ignores |
| `--summary` | `bool` | `false` | Show only category counts without individual items. Useful for dashboards and quick overviews |
| `--ci` | `bool` | `false` | CI mode: `--format sarif --fail-on-issues --quiet` |
| `--fail-on-issues` | `bool` | `false` | Exit 1 if any issues found (promotes `warn` to `error`) |
| `--sarif-file` | `string` | - | Write SARIF output to a file instead of stdout |
| `-o, --output-file` | `string` | - | Write the report to a file instead of stdout, for any --format (no ANSI codes). Useful on large projects where the terminal scrollback truncates the top. Progress and the confirmation stay on stderr |
| `--report-path-prefix` | `string` | - | Prefix prepended to every path in the CI-facing formats (`github-annotations`, `github-summary`, `codeclimate`, `review-github`, `review-gitlab`). CI platforms address files by repository-root-relative path, so when the analyzed project lives in a subdirectory (e.g. `packages/app/`), paths need that offset. fallow detects the offset via the git toplevel automatically; this flag overrides the detection. Pass an empty string to disable rebasing and emit paths relative to `--root` |
| `--fail-on-regression` | `bool` | `false` | Fail if issue count increased beyond tolerance vs a regression baseline |
| `--tolerance` | `string` | `0` | Allowed increase: `"2%"` (percentage) or `"5"` (absolute). Default: `"0"` |
| `--regression-baseline` | `string` | - | Path to regression baseline file (default: `.fallow/regression-baseline.json`) |
| `--save-regression-baseline` | `string` | - | Save current issue counts as a regression baseline |
| `--only` | `dead-code\|dupes\|health` | - | Run only specific analyses (e.g., `--only dead-code,dupes`). Values: `dead-code` (alias: `check`), `dupes`, `health` |
| `--skip` | `dead-code\|dupes\|health` | - | Skip specific analyses (e.g., `--skip health`). Values: `dead-code` (alias: `check`), `dupes`, `health` |
| `--dupes-mode` | `strict\|mild\|weak\|semantic` | - | Override duplication detection mode in combined mode |
| `--dupes-threshold` | `string` | - | Override duplication threshold in combined mode |
| `--dupes-min-tokens` | `string` | - | Override the minimum token count for clones in combined mode |
| `--dupes-min-lines` | `string` | - | Override the minimum line count for clones in combined mode |
| `--dupes-min-occurrences` | `string` | - | Override the minimum clone occurrences in combined mode (must be >= 2) |
| `--dupes-skip-local` | `bool` | `false` | Only report cross-directory duplicates in combined mode |
| `--dupes-cross-language` | `bool` | `false` | Enable cross-language duplicate detection in combined mode |
| `--dupes-ignore-imports` | `bool` | `false` | Exclude module wiring from duplicate detection in combined mode |
| `--dupes-no-ignore-imports` | `bool` | `false` | Count module wiring as clone candidates in combined mode (opt out of the default exclusion) |
| `--score` | `bool` | `false` | Compute health score (0-100 with letter grade) in combined mode. Enables the health delta header in PR comments. JSON includes `health_score` object with `score`, `grade`, and `penalties` breakdown |
| `--trend` | `bool` | `false` | Compare current health metrics against saved snapshot. Implies `--score`. Shows per-metric deltas with directional indicators. Requires at least one saved snapshot in `.fallow/snapshots/` |
| `--save-snapshot` | `string` | - | Save vital signs snapshot for trend tracking. Default path: `.fallow/snapshots/<timestamp>.json`. Forces file-scores + hotspot computation |
| `--coverage` | `string` | - | Path to Istanbul coverage data for exact CRAP scores in combined mode. Also settable via `FALLOW_COVERAGE` or `health.coverage` |
| `--coverage-root` | `string` | - | Absolute prefix to strip from Istanbul file paths in combined mode. Also settable via `FALLOW_COVERAGE_ROOT` or `health.coverageRoot` |
| `--include-entry-exports` | `bool` | `false` | Report unused exports in entry files instead of auto-marking them as used |
<!-- generated:flags:global:end -->

### Combined Mode Flags

<!-- generated:flags:fallow-combined:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--only` | `dead-code\|dupes\|health` | - | Run only specific analyses when no subcommand is given |
| `--skip` | `dead-code\|dupes\|health` | - | Skip specific analyses when no subcommand is given |
| `--production` | `bool` | `false` | Production mode: exclude test/story/dev files, only start/build scripts, report type-only dependencies |
| `--no-production` | `bool` | `false` | Force production mode OFF for every analysis, overriding a project config's `production: true` (and `FALLOW_PRODUCTION`). Conflicts with `--production` |
| `--production-dead-code` | `bool` | `false` | Run dead-code analysis in production mode when using bare combined mode |
| `--production-health` | `bool` | `false` | Run health analysis in production mode when using bare combined mode |
| `--production-dupes` | `bool` | `false` | Run duplication analysis in production mode when using bare combined mode |
| `--dupes-mode` | `strict\|mild\|weak\|semantic` | - | Override duplication detection mode in combined mode |
| `--dupes-threshold` | `string` | - | Override duplication threshold in combined mode |
| `--dupes-min-tokens` | `string` | - | Override the minimum token count for clones in combined mode |
| `--dupes-min-lines` | `string` | - | Override the minimum line count for clones in combined mode |
| `--dupes-min-occurrences` | `string` | - | Override the minimum clone occurrences in combined mode (must be >= 2) |
| `--dupes-skip-local` | `bool` | `false` | Only report cross-directory duplicates in combined mode |
| `--dupes-cross-language` | `bool` | `false` | Enable cross-language duplicate detection in combined mode |
| `--dupes-ignore-imports` | `bool` | `false` | Exclude module wiring from duplicate detection in combined mode |
| `--score` | `bool` | `false` | Compute health score in combined mode |
| `--trend` | `bool` | `false` | Compare current health metrics against the most recent saved snapshot |
| `--save-snapshot` | `string` | - | Save a vital signs snapshot for trend tracking in combined mode. Provide a path or omit for the default `.fallow/snapshots/` location |
| `--coverage` | `string` | - | Path to Istanbul coverage data for exact CRAP scores in combined mode. Also settable via `FALLOW_COVERAGE` or `health.coverage` |
| `--coverage-root` | `string` | - | Absolute prefix to strip from Istanbul file paths in combined mode. Also settable via `FALLOW_COVERAGE_ROOT` or `health.coverageRoot` |

These are global flags with behavior specific to bare `fallow` combined mode.
<!-- generated:flags:fallow-combined:end -->
---

## Environment Variables

| Variable | Description |
|----------|-------------|
| `FALLOW_FORMAT` | Default output format. CLI `--format` overrides. |
| `FALLOW_QUIET` | Set to `1` to suppress progress. CLI `--quiet` overrides. |
| `FALLOW_BIN` | Path to fallow binary (used by the MCP server). |
| `FALLOW_TIMEOUT_SECS` | MCP server subprocess timeout in seconds (default: `120`). Increase for very large codebases. |
| `FALLOW_EXTENDS_TIMEOUT_SECS` | Timeout for fetching remote config inheritance in seconds (default: `5`). Do not raise this for untrusted sources. |
| `FALLOW_CACHE_DIR` | Override the persistent extraction cache directory. Wins over `cache.dir`. Useful for read-only checkouts or CI cache volumes. `--no-cache` disables this knob. |
| `FALLOW_CACHE_MAX_SIZE` | Maximum on-disk extraction cache (`.fallow/cache.bin`) size in megabytes (default: `256`). Triggers LRU eviction when crossed. Wins over `cache.maxSizeMb` config field. Intended for CI runners with disk quotas. `--no-cache` short-circuits this knob. |
| `FALLOW_COVERAGE` | Path to Istanbul coverage data for exact CRAP scoring in `health`, `audit`, and bare `fallow`. |
| `FALLOW_COVERAGE_ROOT` | Absolute coverage-data prefix to strip before matching Istanbul paths in `health`, `audit`, and bare `fallow`. |
| `FALLOW_AUDIT_BASE` | Pin the `fallow audit` comparison base when `--base` / `--changed-since` is unset (precedence: flag > env > auto-detect). Escape hatch for the agent gate and forks, e.g. `FALLOW_AUDIT_BASE=upstream/main`. When unset, audit auto-detects the `git merge-base` against the branch's upstream or the remote default. A malformed value exits 2. |
| `FALLOW_AUDIT_CACHE_MAX_AGE_DAYS` | Max age (in days since last reuse or fresh create) of a persistent reusable `fallow audit` base-snapshot worktree cache. Older entries are reclaimed at the top of the next `fallow audit` invocation (default: `30`). Wins over `audit.cacheMaxAgeDays` config field. `0` disables the GC; invalid values silently fall back to config / default. |
| `FALLOW_UPDATE_CHECK` | Set to `off`, `0`, `false`, `disabled`, or `no` to disable the human-TTY upgrade nudge and its background latest-version check. `DO_NOT_TRACK`, `FALLOW_TELEMETRY_DISABLED`, and CI also suppress it. |
| `FALLOW_SUGGESTIONS` | Set to `off`, `0`, `false`, `no`, or `disabled` to suppress the top-level `next_steps[]` array of read-only follow-up commands in JSON output (and the human `Next:` line on bare `fallow`). Default on. Inherited by the MCP-spawned CLI, so it disables `next_steps` on MCP responses too. Useful for CI consumers that snapshot-diff raw `--format json`. |
| `FALLOW_COMMAND` | GitLab CI: command to run (default: `dead-code`). |
| `FALLOW_FAIL_ON_ISSUES` | GitLab CI: set to `true` to exit 1 if issues found. |
| `FALLOW_CHANGED_SINCE` | GitLab CI: git ref for incremental analysis. Auto-detected in MR pipelines. |
| `FALLOW_COMMENT` | GitLab CI: set to `true` to post MR summary comments. |
| `FALLOW_REVIEW` | GitLab CI: set to `true` to post inline code review comments on MR diffs. |
| `FALLOW_REVIEW_GUIDANCE` | Add collapsed "What to do" guidance blocks to `review-github` / `review-gitlab` inline comments. |
| `FALLOW_SUMMARY_SCOPE` | Sticky PR/MR summary scope for `pr-comment-github` / `pr-comment-gitlab`: `all` (default) keeps project-level findings outside the diff; `diff` applies the diff filter to those findings too. Inline review comments are unaffected. |
| `FALLOW_PR_COMMENT_LAYOUT` | Sticky PR/MR summary layout for `pr-comment-github` / `pr-comment-gitlab`: `default`, `compact`, `gate-only`, or `details`. |
| `FALLOW_SCORE` | GitLab CI: set to `true` to compute health score in combined mode. Enables health delta header in MR comments. |
| `FALLOW_TREND` | GitLab CI: set to `true` to compare current health metrics against saved snapshot. Implies `FALLOW_SCORE`. |
| `FALLOW_EXTRA_ARGS` | GitLab CI: additional CLI flags passed through to fallow. |
| `FALLOW_VERSION` | GitLab CI: fallow version to install. Empty (default) reads the project's `package.json` `fallow` dependency, then falls back to `latest`; set explicitly to override the local pin. |
| `FALLOW_SKIP_BINARY_VERIFY` | Skip Ed25519 + SHA-256 verification of platform binaries on first invocation of `fallow`, `fallow-lsp`, or `fallow-mcp` (and during the GitHub Action installer). Set to `1`, `true`, or `yes` ONLY when deliberately replacing the published binary (source builds, airgapped mirrors, signed-repack registries). The skip is recorded in `fallow --version` output as `verified: skipped (FALLOW_SKIP_BINARY_VERIFY is set)` so it stays visible in CI logs and vendor audits. Never set in regular CI; use the published binary or the documented out-of-band verification recipe in [`SECURITY.md`](https://github.com/fallow-rs/fallow/blob/main/SECURITY.md) instead. |
| `FALLOW_VERIFY_CACHE_DIR` | Override where the lazy-verify sentinel file is written. Cascade is platform-pkg-dir, then this override, then `$XDG_CACHE_HOME/fallow/sentinels/` (Linux/macOS) or `%LOCALAPPDATA%\fallow\sentinels\` (Windows). Useful when the platform pkg dir is read-only (yarn PnP, Docker layered images, pnpm verify-store). |
| `FALLOW_VERIFY_LOG` | Set to `1`, `true`, or `yes` to emit one structured stderr line per verify outcome (`fallow-verify outcome=ok cache=hit sentinel=...`). Off by default so MCP stdout/stderr stay clean; enable for CI diagnostic logs. |
| `FALLOW_TELEMETRY` | Opt-in product telemetry mode, off by default: `off`/`on`/`inspect` (plus `0`/`1`/`true`/`false`/`disabled`/`enabled`/`debug`/`log`). `inspect` prints the exact payload to stderr without sending. Wins over the user telemetry config. |
| `FALLOW_TELEMETRY_DISABLED` | Admin/fleet telemetry kill switch (top precedence, with `DO_NOT_TRACK`). Truthy (`1`/`true`/`yes`/`on`) hard-disables telemetry and refuses `fallow telemetry enable`. |
| `FALLOW_TELEMETRY_DEBUG` | Forces inspect mode; outranks `FALLOW_TELEMETRY`. |
| `DO_NOT_TRACK` | Honored as a top-precedence telemetry kill switch (consoledonottrack.com convention). |
| `FALLOW_AGENT_SOURCE` | Declare the calling agent for telemetry classification (only used when telemetry is enabled; never enables it): `codex`, `claude_code`, `cursor`, `copilot`, `opencode`, `aider`, `roo`, `windsurf`, `gemini` (aliases `gemini_cli`/`antigravity`), `cline`, `continue`, `zed`, `goose`, `other_known`, `unknown`, `none`. Unrecognized values are ignored. |
| `GITLAB_TOKEN` | GitLab CI: project access token with `api` scope (for MR comments/reviews; `CI_JOB_TOKEN` is read-only for MR notes in the official GitLab API). |

Set `FALLOW_FORMAT=json` and `FALLOW_QUIET=1` in your agent environment to avoid passing flags on every invocation.

---

## Output Formats

| Format | Description | Use Case |
|--------|-------------|----------|
| `human` | Colored terminal output | Interactive use |
| `json` | Machine-readable JSON | Agent integration, CI pipelines |
| `sarif` | Static Analysis Results Interchange Format | GitHub Code Scanning, SARIF-compatible tools |
| `compact` | Grep-friendly: one issue per line. Dupes lines include `code-duplication:path:start-end:fingerprint=dup:<id>,...` | Quick filtering |
| `markdown` | Markdown tables | Documentation, PR comments |
| `codeclimate` / `gitlab-codequality` | CodeClimate JSON array | GitLab Code Quality, CodeClimate-compatible tools |
| `pr-comment-github` / `pr-comment-gitlab` | Sticky PR/MR comment markdown with HTML-comment marker for upsert | Posted by the action / template `comment.sh` scripts |
| `review-github` / `review-gitlab` | JSON envelope for `POST /pulls/.../reviews` (GH) or `POST /merge_requests/.../discussions` (GL) | Posted by the action / template `review.sh` scripts; reconciled by `fallow ci reconcile-review` |

---

## `ci`: Provider-Aware Review Automation

`fallow ci reconcile-review` reads a typed review envelope (`--format review-github` / `review-gitlab`), looks up existing fingerprints on the PR/MR, and resolves stale review threads when their finding is no longer present in the new envelope. Posts an idempotent "Resolved in `<sha>`" follow-up comment per stale finding (skipped if a marker for the same fingerprint at the current SHA already exists).

Provider mutations are fail-fast. If a preflight check, permission error, or provider mutation fails, JSON output keeps `apply_errors` and can add `apply_hint`, `failed_fingerprints`, and `unapplied_fingerprints` so agents and CI wrappers can report what was not fully applied.

### Flags

| Flag | Type | Description |
|------|------|-------------|
| `--provider` | `github\|gitlab` | Required. Selects the provider API. |
| `--pr` | `<number>` | GitHub PR number. Required when `--provider github`. |
| `--mr` | `<iid>` | GitLab MR internal id. Required when `--provider gitlab`. |
| `--repo` | `owner/name` | GitHub repo. Defaults to `$GH_REPO` / `$GITHUB_REPOSITORY`. |
| `--project-id` | `<id>` | GitLab project id (numeric or `group/project`). Defaults to `$CI_PROJECT_ID`. |
| `--api-url` | `<url>` | Override the API base URL (GitHub Enterprise, self-hosted GitLab). |
| `--envelope` | `<path>` | Path to the review envelope JSON written by `--format review-{github,gitlab}`. |
| `--dry-run` | `bool` | Compute the new/stale plan without posting / resolving. |

The HTTP layer mirrors the bash `gh_api_retry` / `curl_retry` helpers: `FALLOW_API_RETRIES` (default 3) caps attempts; `FALLOW_API_RETRY_DELAY` (default 2) sets the floor delay; server-supplied `Retry-After` overrides the floor on 429 responses.

---

## CI Integration

- **GitHub Actions**: `uses: fallow-rs/fallow@v2` - supports SARIF upload to Code Scanning, inline PR annotations (`annotations: true`), PR comments, all commands. Annotations use workflow commands (no Advanced Security required); limit with `max-annotations` (default 50). Set `score: true` to compute health score and enable the health delta header in PR comments
- **GitLab CI**: include `ci/gitlab-ci.yml` template and extend `.fallow` - generates Code Quality reports via `--format codeclimate` / `--format gitlab-codequality` (inline MR annotations), rich MR comments, code review comments, all commands. Use `fallow ci-template gitlab --vendor` when runners cannot reach `raw.githubusercontent.com`; commit the generated `ci/` and `action/` files and use GitLab's local include syntax. Variables use `FALLOW_` prefix (e.g., `FALLOW_COMMAND`, `FALLOW_FAIL_ON_ISSUES`). Set `FALLOW_SCORE: "true"` to compute health score; `FALLOW_TREND: "true"` to compare against saved snapshots
- **Any CI**: `npx fallow --ci` - equivalent to `--format sarif --fail-on-issues --quiet`

### GitLab CI Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `FALLOW_COMMAND` | `dead-code` | Command to run (`dead-code`, `dupes`, `health`, or default combined) |
| `FALLOW_FAIL_ON_ISSUES` | `false` | Exit 1 if issues found |
| `FALLOW_CHANGED_SINCE` | auto | Git ref for incremental analysis. Auto-detected in MR pipelines (`origin/$CI_MERGE_REQUEST_TARGET_BRANCH_NAME`) |
| `FALLOW_COMMENT` | `false` | Post a summary comment on the MR with findings |
| `FALLOW_REVIEW` | `false` | Post inline code review comments on MR diff lines where issues were found |
| `FALLOW_REVIEW_GUIDANCE` | `false` | Add collapsed "What to do" guidance blocks to inline review comments |
| `FALLOW_SUMMARY_SCOPE` | `all` | Sticky summary scope: `all` keeps project-level findings outside the diff; `diff` applies the diff filter to those findings too |
| `FALLOW_PR_COMMENT_LAYOUT` | `default` | Sticky summary layout: `default`, `compact`, `gate-only`, or `details` |
| `FALLOW_SCORE` | `false` | Compute health score (0-100 with letter grade) in combined mode. Enables the health delta header in MR comments |
| `FALLOW_TREND` | `false` | Compare current health metrics against saved snapshot. Implies `FALLOW_SCORE`. Shows per-metric deltas |
| `FALLOW_EXTRA_ARGS` | - | Additional CLI flags passed through to fallow |
| `GITLAB_TOKEN` | - | Project access token with `api` scope (required for `FALLOW_COMMENT` and `FALLOW_REVIEW`). Alternatively, enable job token API access |

**Package manager detection**: The GitLab template auto-detects the project's package manager (npm, pnpm, or yarn) from lockfiles. MR comments and review comments show the correct install/run commands for the detected manager (e.g., `pnpm add -D` vs `npm install --save-dev`).

**Auto `--changed-since` in MR pipelines**: When running in a merge request pipeline, the template automatically sets `--changed-since origin/$CI_MERGE_REQUEST_TARGET_BRANCH_NAME` unless `FALLOW_CHANGED_SINCE` is explicitly set. This scopes analysis to files changed in the MR without manual configuration.

---

## JSON Output Structure

### `dead-code` output

```json
{
  "kind": "dead-code",
  "schema_version": 7,
  "version": "3.3.0",
  "elapsed_ms": 45,
  "total_issues": 12,
  "entry_points": {
    "total": 5,
    "sources": { "package_json_scripts": 2, "next_js": 3 }
  },
  "summary": {
    "total_issues": 12,
    "unused_files": 1,
    "unused_exports": 1,
    "unused_types": 1,
    "unused_dependencies": 1,
    "unused_enum_members": 0,
    "unused_class_members": 0,
    "unresolved_imports": 0,
    "unlisted_dependencies": 0,
    "duplicate_exports": 0,
    "type_only_dependencies": 0,
    "test_only_dependencies": 0,
    "circular_dependencies": 0,
    "re_export_cycles": 0,
    "boundary_violations": 0,
    "stale_suppressions": 0
  },
  "unused_files": [{ "path": "src/old.ts" }],
  "unused_exports": [{ "path": "src/utils.ts", "name": "unusedFn", "line": 42, "actions": [{"type": "remove-export", "auto_fixable": true, "description": "Remove the unused export from the public API"}, {"type": "suppress-line", "auto_fixable": false, "description": "Suppress with an inline comment above the line", "comment": "// fallow-ignore-next-line unused-export"}] }],
  "unused_types": [{ "path": "src/types.ts", "name": "OldType", "line": 10 }],
  "unused_dependencies": [{ "name": "lodash", "line": 5, "used_in_workspaces": ["packages/web"] }],
  "unused_dev_dependencies": [{ "name": "jest", "line": 8 }],
  "unused_enum_members": [{ "path": "src/enums.ts", "enum_name": "Status", "member": "Archived", "line": 5 }],
  "unused_class_members": [{ "path": "src/service.ts", "class_name": "Service", "member": "oldMethod", "line": 20 }],
  "unresolved_imports": [{ "path": "src/index.ts", "specifier": "./missing", "line": 3 }],
  "unlisted_dependencies": [{ "name": "chalk", "imported_from": [{ "path": "src/cli.ts", "line": 1, "col": 0 }] }],
  "duplicate_exports": [{ "name": "Config", "locations": ["src/config.ts:5", "src/types.ts:12"] }],
  "circular_dependencies": [{ "cycle": ["src/a.ts", "src/b.ts", "src/a.ts"], "line": 3, "col": 0, "is_cross_package": false }],
  "re_export_cycles": [{ "files": ["src/api/index.ts", "src/api/internal/index.ts"], "kind": "multi-node", "actions": [{ "type": "fix", "kind": "refactor-re-export-cycle", "auto_fixable": false, "description": "Remove one `export * from` (or `export { ... } from`) statement on any one member to break the cycle" }, { "type": "suppress-file", "kind": "suppress-file", "auto_fixable": false, "comment": "// fallow-ignore-file re-export-cycle" }] }],
  "boundary_violations": [{ "from_path": "src/ui/Button.ts", "to_path": "src/data/db.ts", "from_zone": "ui", "to_zone": "data", "import_specifier": "../data/db", "line": 5, "col": 0 }],
  "unused_optional_dependencies": [{ "name": "fsevents" }],
  "type_only_dependencies": [{ "name": "zod", "used_in": ["src/schema.ts"], "line": 12 }],
  "test_only_dependencies": [{ "name": "msw", "path": "package.json", "line": 15 }],
  "stale_suppressions": [{ "path": "src/utils.ts", "line": 5, "col": 0, "origin": { "type": "inline_comment", "issue_type": "unused-export", "is_file_level": false } }]
}
```

For dependency findings, `used_in_workspaces` means the package is imported by another workspace even though the declaring workspace does not import it. Move the dependency to the consuming workspace instead of auto-removing it.

#### `actions` Array

Every issue in `dead-code` JSON output includes an `actions` array with structured fix suggestions. Each action has:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | yes | Action type in kebab-case (for example `remove-export`, `remove-file`, `remove-dependency`, `move-dependency`, `suppress-line`, `add-to-config`) |
| `auto_fixable` | bool | yes | `true` if `fallow fix` handles this action automatically. Evaluated PER FINDING, not per action type: the same `type` may carry `true` on one finding and `false` on another when per-instance guards in the applier discriminate. Filter on this bool of each individual action, not on `type` alone. |
| `description` | string | yes | Human-readable description of the action |
| `comment` | string | no | Suppression comment text (on `suppress-line` actions) |
| `note` | string | no | Additional context on non-auto-fixable items |
| `config_key` | string | no | Config field to update (on `add-to-config` actions) |
| `value` | string \| array | no | Value to add to the config field (on `add-to-config` actions). Scalar for `ignoreDependencies`-style keys (e.g. `"lodash"`); array of `{ file, exports }` rule objects for `ignoreExports`. |
| `value_schema` | string | no | URL pointing at the JSON Schema fragment that describes `value` (on `add-to-config` actions). Agents that want to validate `value` before writing it into a user's config can fetch and apply the linked schema. |

Example:

```json
{
  "path": "src/utils.ts",
  "name": "helperFn",
  "line": 10,
  "actions": [
    {
      "type": "remove-export",
      "auto_fixable": true,
      "description": "Remove the unused export from the public API"
    },
    {
      "type": "suppress-line",
      "auto_fixable": false,
      "description": "Suppress with an inline comment above the line",
      "comment": "// fallow-ignore-next-line unused-export"
    }
  ]
}
```

Dependency issues use `add-to-config` with `config_key` and `value`:

```json
{
  "name": "autoprefixer",
  "line": 5,
  "actions": [
    {
      "type": "remove-dependency",
      "auto_fixable": true,
      "description": "Remove from package.json dependencies"
    },
    {
      "type": "add-to-config",
      "auto_fixable": false,
      "description": "Add to ignoreDependencies in fallow config",
      "config_key": "ignoreDependencies",
      "value": "autoprefixer",
      "value_schema": "https://raw.githubusercontent.com/fallow-rs/fallow/main/schema.json#/properties/ignoreDependencies/items"
    }
  ]
}
```

When a dependency action is `move-dependency`, `auto_fixable` is `false`; the package is imported from another workspace and needs a package.json ownership move rather than removal.

Per-instance `auto_fixable` flips today (the same action `type` flipping between findings):

- `remove-catalog-entry` (unused-catalog-entries): `true` only when `hardcoded_consumers` is empty; `false` otherwise (the applier skips the entry to avoid breaking `pnpm install`).
- `remove-dependency` vs `move-dependency` (dependency findings): primary action flips between `remove-dependency` (`true`) and `move-dependency` (`false`) on `used_in_workspaces`.
- `add-to-config` for `ignoreExports` (duplicate-exports): `true` when `fallow fix` can safely apply the action, which today means EITHER a fallow config file already exists OR no config exists and the working directory is NOT inside a monorepo subpackage. In the second case the applier creates `.fallowrc.json` using `fallow init`'s framework-aware scaffolding and layers the new rules on top. `false` inside a monorepo subpackage with no workspace-root config (the applier refuses to fragment per-package configs). Pass `--no-create-config` to `fallow fix` from pre-commit hooks, CI bots, and `fallow watch` to opt out of the create-fallback; the action then surfaces with `auto_fixable: false`.
- `update-catalog-reference` (unresolved-catalog-references): always `false` today; non-singleton on the wire so a future applier can promote it without a schema change.
- All `suppress-line` and `suppress-file` actions are uniformly `false`.

#### Health `actions` array (CRAP findings)

Health findings (`fallow health` JSON output) include an `actions` array. Primary action selection is formula-aware: the rule first checks whether full coverage CAN bring CRAP under threshold (CRAP bottoms out at `cyclomatic` at 100% coverage, so `cyclomatic < maxCrap` means coverage is a viable remediation), then uses `coverage_tier` to choose the description.

| Condition | Primary action |
|-----------|----------------|
| `cyclomatic >= maxCrap` (coverage cannot remediate, regardless of tier) | `refactor-function` |
| `cyclomatic < maxCrap` and `coverage_tier=none` | `add-tests` ("start from scratch") |
| `cyclomatic < maxCrap` and `coverage_tier=partial` or `high` | `increase-coverage` ("targeted branch coverage") |
| Cyclomatic/cognitive triggered (no CRAP) | `refactor-function` |

The `coverage_tier` field is `"none"` (file not test-reachable / Istanbul 0%), `"partial"` (Istanbul `(0, 70)` / estimated 40%), or `"high"` (Istanbul `>= 70` / estimated 85%).

Each CRAP finding also carries a `coverage_source` discriminator: `"istanbul"` (direct fnMap match for this function), `"estimated"` (graph-based estimate evaluated against the finding's own file), or `"estimated_component_inherited"` (graph-based estimate inherited from an Angular component `.ts` reached via the inverse `templateUrl` edge). The report summary carries `coverage_source_consistency` (`"uniform"` or `"mixed"`) whenever emitted CRAP findings have source data; grouped health JSON also includes `groups[].coverage_source_consistency`. Synthetic `<template>` findings on Angular `.html` templates use the `estimated_component_inherited` source and include an `inherited_from` field with the project-relative path to the owning `.component.ts`. When the inherit path applies, the primary `increase-coverage` action targets that `.ts` file (description names the component path explicitly and includes a `target_path` field) so AI agents add component tests rather than scaffolding tests against a structurally untestable `.html` path. The human `fallow health` output renders `(inherited from <project-relative-path>.component.ts)` after the CRAP score on those rows (project-relative since fallow 2.78.0; was the bare basename before). This is the JIT-test fallback (Angular's runtime renders templates via `ɵɵconditional` / `ɵɵrepeaterCreate` calls; Istanbul never has `fnMap` entries keyed at `.html` paths). AOT-compiled coverage with source-map back-mapping is planned as a phase 2 follow-up; when it lands, `coverage_source` will gain a `"measured_aot_source_map"` variant.

When CRAP-only with cyclomatic count within `health.crapRefactorBand` of `maxCyclomatic` AND cognitive at or above `maxCognitive / 2`, a secondary `refactor-function` is appended. The default band is `5`; set it to `0` to only add the secondary refactor after cyclomatic reaches `maxCyclomatic`. The cognitive floor suppresses false positives on flat type-tag dispatchers and JSX render maps (high CC, near-zero cog). A single finding can carry multiple action types: e.g. a finding that exceeds both cyclomatic and CRAP at `coverage_tier=partial` gets `increase-coverage` AND `refactor-function`. Treat the first non-`suppress-line` action as primary.

The `suppress-line` action is auto-omitted when `--baseline`/`--save-baseline` is set, OR when `health.suggestInlineSuppression: false` in config. The report root carries an `actions_meta: { suppression_hints_omitted: true, reason: "baseline-active" | "config-disabled" }` breadcrumb in that case.

#### `baseline_deltas` Object

When `--baseline` is used in combined output, the JSON includes a `baseline_deltas` object showing per-category changes since the baseline:

```json
{
  "baseline_deltas": {
    "total_delta": -3,
    "per_category": {
      "unused_files": { "current": 5, "baseline": 7, "delta": -2 },
      "unused_exports": { "current": 10, "baseline": 11, "delta": -1 }
    }
  }
}
```

### `dupes` output

```json
{
  "kind": "dupes",
  "schema_version": 7,
  "version": "3.3.0",
  "elapsed_ms": 82,
  "total_clones": 15,
  "total_lines_duplicated": 230,
  "duplication_percentage": 4.2,
  "clone_groups": [
    {
      "instances": [
        { "path": "src/a.ts", "start_line": 10, "end_line": 25 },
        { "path": "src/b.ts", "start_line": 40, "end_line": 55 }
      ],
      "tokens": 120,
      "lines": 16,
      "family": { "suggestion": "extract_function", "shared_files": ["src/a.ts", "src/b.ts"] }
    }
  ],
  "mirrored_directories": [
    { "dir_a": "src/components", "dir_b": "src/legacy/components", "shared_clones": 4 }
  ]
}
```

The `mirrored_directories` array identifies directory pairs that share many clone groups, suggesting structural duplication (e.g., a copy-pasted module that was never cleaned up).

### `fix` output (dry-run)

```json
{
  "changes": [
    { "path": "src/utils.ts", "action": "remove_export", "name": "unusedFn", "line": 42 },
    { "path": "package.json", "action": "remove_dependency", "name": "lodash" }
  ],
  "total_changes": 2
}
```

### Combined output (`fallow` with no subcommand)

When running `fallow` with no subcommand (all analyses), the JSON output combines results from all enabled analyses:

```json
{
  "kind": "combined",
  "schema_version": 7,
  "version": "3.3.0",
  "elapsed_ms": 159,
  "check": {
    "schema_version": 7,
    "version": "3.3.0",
    "elapsed_ms": 45,
    "total_issues": 12,
    "unused_files": [],
    "unused_exports": [],
    "unused_types": [],
    "unused_dependencies": [],
    "unused_dev_dependencies": [],
    "unused_enum_members": [],
    "unused_class_members": [],
    "unresolved_imports": [],
    "unlisted_dependencies": [],
    "duplicate_exports": [],
    "circular_dependencies": [],
    "re_export_cycles": [],
    "boundary_violations": [],
    "unused_optional_dependencies": [],
    "type_only_dependencies": [],
    "test_only_dependencies": [],
    "stale_suppressions": []
  },
  "dupes": {
    "total_clones": 15,
    "total_lines_duplicated": 230,
    "duplication_percentage": 4.2,
    "clone_groups": []
  },
  "health": {
    "summary": {},
    "findings": [],
    "vital_signs": {}
  }
}
```

Use `--only` or `--skip` to control which analyses are included in the combined output. Use `--coverage` and `--coverage-root` to feed Istanbul coverage data to the embedded health analysis for exact CRAP scoring.

With `--score`, the combined output's `health` section includes a `health_score` object (same schema as `health --score`). With `--trend`, it includes a `health_trend` object comparing against the most recent saved snapshot. With `--save-snapshot`, a vital signs snapshot is persisted for future trend comparisons.

### Error output (exit code 2)

```json
{"error": true, "message": "invalid config: unknown field 'detect'", "exit_code": 2}
```

---

## Configuration File Format

Config files are searched in priority order: `.fallowrc.json` > `.fallowrc.jsonc` > `fallow.toml` > `.fallow.toml`. Both `.fallowrc.json` and `.fallowrc.jsonc` are parsed as JSON-with-comments; the `.jsonc` extension lets editors auto-detect JSONC syntax highlighting.

### JSON Format (`.fallowrc.json` / `.fallowrc.jsonc`)

```jsonc
{
  "$schema": "https://raw.githubusercontent.com/fallow-rs/fallow/main/schema.json",

  // Entry points (glob patterns)
  "entry": ["src/index.ts", "scripts/*.ts"],

  // Files to ignore (glob patterns)
  "ignorePatterns": ["**/*.generated.ts", "**/*.d.ts"],

  // Dependencies to ignore
  "ignoreDependencies": ["autoprefixer"],

  // Suppress unused-export findings when the symbol is referenced inside its
  // declaring file (knip parity). Boolean or { type, interface } object form.
  "ignoreExportsUsedInFile": true,

  // Per-issue-type severity
  "rules": {
    "unused-files": "error",
    "unused-exports": "warn",
    "unused-types": "off",
    "unused-dependencies": "error",
    "unused-dev-dependencies": "warn",
    "unused-enum-members": "error",
    "unused-class-members": "warn",
    "unresolved-imports": "error",
    "unlisted-dependencies": "error",
    "duplicate-exports": "warn",
    "circular-dependencies": "warn",
    "boundary-violation": "error",
    "type-only-dependencies": "error",
    "test-only-dependencies": "warn",
    "stale-suppressions": "warn"
  },

  // Per-path rule overrides
  "overrides": [
    {
      "files": ["*.test.ts", "*.spec.ts"],
      "rules": { "unused-exports": "off" }
    }
  ],

  // Duplication settings
  "duplicates": {
    "mode": "mild",
    "minTokens": 50,
    "minLines": 5,
    "threshold": 0,
    "ignoreDefaults": true,
    "skipLocal": false,
    "ignorePatterns": ["**/*.generated.ts"]
  },

  // Extraction cache settings. FALLOW_CACHE_DIR overrides cache.dir.
  "cache": {
    "dir": "/tmp/fallow-cache",
    "maxSizeMb": 256
  },

  // Architecture boundaries (preset, custom zones/rules, or auto-discovered feature zones)
  // Presets: "layered", "hexagonal", "feature-sliced", "bulletproof"
  // Rules accept an optional `allowTypeOnly: [zones]` list that admits type-only imports
  // (`import type`, inline `{ type Foo }`, namespace type imports, and `export type` re-exports)
  // to the listed zones even when not present in `allow`. Mixed-specifier imports still fire.
  "boundaries": {
    "preset": "bulletproof"
    // Or:
    // "zones": [
    //   { "name": "app", "patterns": ["src/app/**"] },
    //   { "name": "features", "patterns": ["src/features/**"], "autoDiscover": ["src/features"] },
    //   { "name": "shared", "patterns": ["src/shared/**"] }
    // ],
    // "rules": [
    //   { "from": "app", "allow": ["features", "shared"] },
    //   { "from": "features", "allow": ["shared"], "allowTypeOnly": ["features"] }
    // ]
  },

  // Resolve framework convention auto-imports (Nuxt components) as graph edges.
  // Edges for `<Card001 />`-style template tags are always synthesized; setting
  // this to true also drops the Nuxt component entry patterns so an
  // unreferenced component is reported as unused-file. Kept conservative: a
  // `components:` key in nuxt.config keeps the entry patterns. Default false.
  "autoImports": false,

  // Production mode
  "production": false,

  // Workspace packages that are public libraries.
  // Exported API surface from these packages is not flagged as unused.
  "publicPackages": ["@myorg/shared-lib", "@myorg/utils"],

  // Glob patterns for files that are dynamically loaded at runtime.
  // These files are treated as always-used and never flagged as unused.
  "dynamicallyLoaded": ["plugins/**/*.ts", "locales/**/*.json"],

  // Inherit from base config (prefer local paths or trusted npm packages)
  "extends": ["./base-config.json", "npm:@my-org/fallow-config"],

  // Custom external plugins
  "plugins": ["tools/plugins/"],

  // Inline framework definitions
  "framework": [
    {
      "name": "my-framework",
      "enablers": ["my-framework"],
      "entryPoints": ["src/routes/**/*.ts"]
    }
  ]
}
```

### TOML Format (`fallow.toml`)

```toml
entry = ["src/index.ts", "scripts/*.ts"]
ignorePatterns = ["**/*.generated.ts"]
ignoreDependencies = ["autoprefixer"]
ignoreExportsUsedInFile = true
production = false
publicPackages = ["@myorg/shared-lib", "@myorg/utils"]
dynamicallyLoaded = ["plugins/**/*.ts", "locales/**/*.json"]

[rules]
unused-files = "error"
unused-exports = "warn"
unused-types = "off"

[duplicates]
mode = "mild"
minTokens = 50
minLines = 5
ignoreDefaults = true

[[overrides]]
files = ["*.test.ts"]
[overrides.rules]
unused-exports = "off"

[boundaries]
preset = "bulletproof"
```

### Configuration field notes

- `ignoreExportsUsedInFile`: knip-compatible; suppress unused-export findings when the exported symbol is referenced inside the file that declares it. Boolean (`true` covers all kinds) or `{ "type": true, "interface": true }` object form for knip parity. Fallow groups type aliases and interfaces under the same `unused-types` issue, so both type-kind fields behave identically. References inside the export specifier itself (`export { foo }`, `export default foo`) do not count as same-file uses; those exports are still reported when no other in-file expression references the binding
- `publicPackages`: workspace packages that are public libraries; exported API surface from these packages is not flagged as unused
- `dynamicallyLoaded`: glob patterns for files loaded at runtime (plugin dirs, locale files); treated as always-used
- `cache.dir`: override the persistent extraction cache directory. `FALLOW_CACHE_DIR` wins over this config field, and `--no-cache` disables caching entirely
- `cache.maxSizeMb`: cap the serialized extraction cache size in megabytes. `FALLOW_CACHE_MAX_SIZE` wins over this config field
- `usedClassMembers`: class method/property names that extend the built-in Angular/React lifecycle allowlist with framework-invoked names. Each entry is a plain string (global suppression) or a scoped object `{ extends?, implements?, members }` matching only classes with the given heritage. Strings can be exact names (`"agInit"`) or glob patterns (`"*"` matches every member, `"enter*"` prefix, `"*Handler"` suffix, `"on*Event"` combined). Use scoped rules for common names like `refresh` or `execute` to avoid false negatives on unrelated classes; global strings for unique names like `agInit`. Example: `["agInit", { "implements": "ICellRendererAngularComp", "members": ["refresh"] }, { "extends": "BaseCommand", "members": ["execute"] }, { "extends": "GrammarBaseListener", "members": ["enter*", "exit*"] }]`. Glob patterns that match zero members emit a `WARN` so dead allowlist entries surface. An unconstrained scoped rule (no `extends` or `implements`) is rejected at load time. Use plugin-level `usedClassMembers` in a `.fallow/plugins/*.jsonc` file for library-specific allowlists
- `resolve.conditions`: additional package.json `exports` / `imports` condition names to honor during module resolution. Baseline conditions (`development`, `import`, `require`, `default`, `types`, `node`, plus `react-native` / `browser` under RN/Expo) are always included; user entries prepend ahead of them. Use for community conditions like `worker`, `edge-light`, `deno`, or custom bundler conditions. Example: `{ "resolve": { "conditions": ["worker", "edge-light"] } }`
- `unusedComponentProps.ignorePattern`: opt-in regex that exempts a component prop from `unused-component-props` when the prop's LOCAL destructure binding name matches (the leading-underscore "accepted-but-intentionally-unused" convention, mirroring TS `noUnusedParameters` + ESLint `varsIgnorePattern` / `argsIgnorePattern`). Applies to Vue, Svelte, Astro, and React/Preact props. The match is on the local alias (`_stage` in `let { stage: _stage } = $props()`), not the public prop name the finding reports (`stage`); matching is unanchored like ESLint's `RegExp.test`, so anchor with `^_`. An invalid regex fails config load. Example: `{ "unusedComponentProps": { "ignorePattern": "^_" } }`

---

## Inline Suppression Comments

| Comment | Effect |
|---------|--------|
| `// fallow-ignore-next-line` | Suppress any issue on the next line |
| `// fallow-ignore-next-line unused-export` | Suppress specific issue type |
| `// fallow-ignore-file` | Suppress all issues in a file |
| `// fallow-ignore-file unused-export` | Suppress specific issue type file-wide |

### Valid Issue Type Tokens

`unused-file`, `unused-export`, `unused-type`, `unused-dependency`, `unused-dev-dependency`, `unused-enum-member`, `unused-class-member`, `unused-store-member`, `unresolved-import`, `unlisted-dependency`, `duplicate-export`, `circular-dependency`, `re-export-cycle`, `boundary-violation`, `policy-violation`, `unused-optional-dependency`, `type-only-dependency`, `test-only-dependency`, `code-duplication`
