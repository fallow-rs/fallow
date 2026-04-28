# Changelog

All notable changes to fallow are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **GitLab CI now respects package.json fallow pins to match GitHub Action behavior.** When `FALLOW_VERSION` is empty, the GitLab template reads the project's `package.json` `fallow` dependency, validates the spec with the same injection-safe whitelist as the GitHub Action, supports semver ranges, falls back to `latest` for unsupported package specs, and warns when an exact package pin drifts from the installed CLI version. The default value of `FALLOW_VERSION` changed from `"latest"` to `""`; pipelines that omit `FALLOW_VERSION` and have a `fallow` entry in `package.json` will now install that pinned version on the next run instead of `latest`. Set `FALLOW_VERSION: "latest"` explicitly to keep the previous behaviour.

## [2.53.0] - 2026-04-28

### Added

- **Cross-workspace dependency leak detection.** `fallow dead-code` now flags packages declared in one workspace's `package.json` but only consumed from another workspace's source files, surfaced as a new `cross-workspace-dependency` issue with a `move-dependency` action carrying the suggested target workspace path. Helps monorepo maintainers find dependencies that drifted out of their original workspace as code moved around. Available across every output format (human, JSON, SARIF, CodeClimate, markdown, compact); the JSON action `type` enum gains `move-dependency`.

### Fixed

- **Vue script-instance member access in templates is now credited.** `<script setup> const counter = new Counter(); ...` followed by `<button @click="counter.bump()">{{ counter.value }}</button>` no longer reports `Counter.bump` and `Counter.value` as unused class members. The Vue template scanner threads the script block's instance-binding map through expression and statement evaluation so `counter.bump()` remaps to `Counter.bump`. v-for / v-slot locals continue to shadow the script binding, avoiding false positives. Mirrors the same fix shipped earlier in this cycle for Svelte. Cache version bumped to 53 so existing caches re-extract the new member-access shape.
- **Svelte 5 `{@attach}` directives and arrow-bound class member calls are now credited as used.** `<div {@attach myAttach}>` no longer reports `myAttach` as an unused export, and `onclick={() => counter.bump()}` plus `{counter.value}` correctly map to the script-local `Counter` instance's `bump` and `value` members through the script's binding-target table. Template locals (e.g., `{#each rows as counter}`) shadow the script binding so genuinely unused names in shadowed scopes still get reported. A small follow-up skips synthetic `this.*` keys when seeding the template scanner's bound targets, trimming dead weight from the reference set. Closes [#200](https://github.com/fallow-rs/fallow/issues/200). Thanks [@kevmodrome](https://github.com/kevmodrome) for the report and [@imwyvern](https://github.com/imwyvern) for [#201](https://github.com/fallow-rs/fallow/pull/201).
- **Peer dependencies of used packages are no longer reported as unused.** `fallow dead-code` now reads installed packages' required `peerDependencies` and credits them when the package itself is used, recursively. Peers marked optional via `peerDependenciesMeta.<name>.optional: true` are still reported when otherwise unused. Package lookup follows ancestor `node_modules` directories so workspace-local, hoisted monorepo, and scoped-package installs are covered. Closes [#199](https://github.com/fallow-rs/fallow/issues/199).
- **Linux GNU release binaries now run on glibc 2.31+ (Debian Bullseye, Ubuntu 20.04, RHEL 9, Amazon Linux 2023).** Previous releases inherited the GitHub `ubuntu-latest` runner's glibc 2.39, which broke fallow on most CI images and older distros. The release workflow now compiles `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu` inside the `rust:1.95-bullseye` container, with a post-build `readelf` gate that fails the release if any shipped binary requires a glibc version newer than 2.31. Forward-only fix: applies to artifacts produced from this tag onward. Closes [#191](https://github.com/fallow-rs/fallow/issues/191). Thanks [@OmerGronich](https://github.com/OmerGronich).
- **`docs/output-schema.json` now lists `move-dependency` in the action `type` enum.** New emit sites in `crates/cli/src/report/json.rs` introduced a value the schema didn't know about, which would have rejected every cross-workspace finding for consumers validating against the published schema.

## [2.52.2] - 2026-04-28

### Fixed

- **Workspace-aware unused-dependency baselines.** When the same package appeared as unused in multiple workspace `package.json` files, `fallow dead-code --save-baseline` collapsed them into a single bare-package-name key. Suppressing the entry in one workspace then silenced every other workspace's report of the same package, hiding genuinely unused deps in the rest of the monorepo. Baseline keys for `unused_dependencies`, `unused_dev_dependencies`, `unused_optional_dependencies`, `type_only_dependencies`, and `test_only_dependencies` are now `package.json:package_name` so per-workspace instances stay distinct. Existing baselines saved with bare-package-name keys still match on load, so upgrading does not require regenerating baselines.

## [2.52.1] - 2026-04-27

### Fixed

- **Stack overflow on Svelte snippet params with typed tuple bindings.** `extract_pattern_binding_names` and `collect_pattern_usage` only stripped trailing TS type annotations when the pattern started with `{` or `[`. A snippet param like `{#snippet foo(x: [number, number])}` reached the comma-split branch where `split_top_level` refused to split (commas at depth=1 inside the tuple) and recursed on the unchanged input until the stack overflowed. Now top-level commas split first, then a top-level `:` scan strips trailing type annotations for both destructured and plain identifier bindings. Closes [#172](https://github.com/fallow-rs/fallow/issues/172).
- **ESLint config: dependencies inside `overrides[*]` and relative `extends` chains are now detected.** Two parser gaps produced false-positive unused devDependency reports: `parser`/`plugins`/`extends` inside `overrides[*]` were ignored (only top-level fields were extracted), and `extends` entries pointing at relative paths (`./config/base.js`, `../shared/eslintrc.json`) were passed verbatim to the package-name resolver, producing nonsense like `eslint-config-./config/base.js`. The walker now recurses into each override entry as a sub-config and resolves path-like extends relative to the parent dir, probing `.js`/`.cjs`/`.mjs`/`.json` like ESLint's resolver. Cycle protection via `FxHashSet<PathBuf>`; depth bounded at 8. Closes [#198](https://github.com/fallow-rs/fallow/issues/198).
- **`ignorePatterns` is now honored when warning about undeclared workspaces.** Directories whose project-root-relative path matched the user's `ignorePatterns` config still triggered the `package.json directory not declared as a workspace` warning. The compiled `GlobSet` from `ResolvedConfig` is now plumbed into `find_undeclared_workspaces` and matched against both the relative directory path and its `package.json` suffix, so paths like `references/vitest` are silenced consistently with the rest of the analysis. Closes [#193](https://github.com/fallow-rs/fallow/issues/193).
- **Five false-positive cases for files referenced from non-source artifacts.** Vite `css.preprocessorOptions.{scss,sass,less,stylus}.additionalData` bodies are now scanned for `@use`/`@import`/`@forward`; local paths seed entry patterns and bare specifiers credit referenced dependencies. Vue/Svelte SFC `<style lang="scss">` body imports and `<style src="...">` references are extracted symmetrically with `<script src>` (imports tagged `from_style: true` so the resolver routes through CSS-restricted SCSS fallbacks before the standard resolver, avoiding `./Foo` resolving to the `.vue` self-file). `package.json` scripts positional file arguments now use the workspace prefix when joined into entry patterns with `..` segment normalization. CI YAML scanner returns `CiAnalysis { used_packages, entry_files }` so positional file paths in `.gitlab-ci.yml` and `.github/workflows/*.yml` are seeded as entries. Cypress plugin extracts `e2e.specPattern`, `component.specPattern`, `e2e.supportFile`, `component.supportFile` from `cypress.config.{ts,js,mjs,cjs}` so Cypress 10+ projects whose specs live outside `cypress/**` stay reachable. A follow-up commit covered additional Vite preprocessor and Cypress edge cases. Cache version bumped to 51 (new `from_style` field on `CachedImport`). Closes [#195](https://github.com/fallow-rs/fallow/issues/195).
- **LSP `changedSince` filter now works when the workspace is a subdirectory of the git repo.** Turborepo, Next.js `apps/`, and Nx `packages/` layouts silently produced an empty filter because `try_get_changed_files` joined `git diff` output (toplevel-relative) against the workspace root (subdirectory). The resolver now resolves the canonical git toplevel via `git rev-parse --show-toplevel`, joins git output against it, and passes `--full-name` to `git ls-files` for parity with `git diff`. The LSP canonicalizes the workspace root in `initialize` so absolute paths agree across symlink boundaries on macOS and Windows. A one-time WARN log fires when workspace differs from toplevel so monorepo subdirectory layouts are self-diagnosable. Same release also dedupes cross-root duplicates that `merge_results` accumulates when multiple project roots walk overlapping files (with `UnlistedDependency` getting a real merge of `imported_from` sites across roots), stamps `Diagnostic.data` with `{ "changedSince": "<ref>" }` when the filter is active so AI agents reading via `vscode.languages.getDiagnostics()` can verify the filter, and promotes the `(since <ref>)` indicator to all four status bar states via a shared `renderStatusBarText` helper. Closes [#190](https://github.com/fallow-rs/fallow/issues/190).
- **LSP advertises `diagnostic_provider` (LSP 3.17 pull-model) so strict clients call `textDocument/diagnostic`.** The pull handler and `cached_diagnostics` map were already wired up, but `ServerCapabilities.diagnostic_provider` was unset. Strict 3.17 clients (Helix, Zed) gate the pull request on the advertised capability, so the handler was dead code for those editors; VS Code's LSP client tolerated the gap by falling back to push-mode. The advertisement uses `identifier: "fallow"`, `inter_file_dependencies: true` (cross-file findings such as unused exports / unused dependencies), and `workspace_diagnostics: false` (no `workspace/diagnostic` handler).
- **`cargo doc --document-private-items` no longer fails on a public-doc-links-private warning in `crates/extract/src/css.rs`.** The doc comment on `extract_css_imports` (public) referenced `[\`normalize_css_import_path\`]` (private), which `-D rustdoc::private-intra-doc-links` rejects. Reverted to plain backtick formatting; CI `cargo doc --workspace --no-deps --document-private-items` is green again. Pre-existing problem; no behavior change.
- **`zizmor` Actions Security scan no longer audits fallow's own test fixtures.** The CI step ran `uvx zizmor ... .` from the repo root, which swept in `tests/fixtures/issue-195-ci-file-args/.github/workflows/deploy.yml` (a deliberately lax fixture used by fallow's CI YAML detection, not a real workflow) and failed on `unpinned-uses` plus `excessive-permissions`. Scoped the invocation to `.github/` and `action.yml` so the audit covers the workflows we actually ship.

## [2.52.0] - 2026-04-26

### Added

- **`fallow dupes --group-by` partitions clone groups by owner / directory / package / section.** Previously the flag was validated (so global flag errors stayed consistent across `check`, `health`, and `dupes`) but the resolver was built and discarded; the output stayed ungrouped. Now grouped output mirrors the existing `health --group-by` shape across every format. Multi-owner clone groups are attributed to the **largest-owner** (most instances in the group) with **alphabetical tiebreak** on equal counts, matching jscpd's majority-instance attribution; per-instance owners are surfaced inline so consumers can see split ownership. JSON output gains `grouped_by`, `total_issues`, and a `groups` array carrying per-bucket `stats`, `clone_groups` (each with `primary_owner`), and `clone_families`. SARIF gains `properties.group` per result; CodeClimate gains a top-level `group` per issue. Human output mirrors health's grouped section style: cyan-bullet `Per-{mode} duplication` header, per-bucket cyan-bullet sub-headers with stats, and a project-totals footer. The largest-owner attribution preamble renders only for `--group-by owner` (where attribution is genuinely ambiguous); directory / package / section modes hide it as cognitive noise. `--top` is skipped when `--group-by` is active so per-bucket stats reflect the full bucket; the renderer applies its own per-bucket cap. The MCP `find_dupes` tool already plumbed `--group-by`; no MCP-side change needed.
- **`BoundaryZone.root` for monorepo per-package boundary configurations.** `[[boundaries.zones]] root = "packages/app/"` resolves the zone's `patterns` against paths relative to the subtree instead of the project root. At classification time, `classify_zone` checks the path starts with the root prefix; if yes, strips it and glob-matches the patterns against the remainder; if no, skips the zone. Filter approach over prefix-rewriting: zero-cost for zones without `root`, keeps stored pattern strings un-corrupted, debug output stays clean. Closes the `FALLOW-BOUNDARY-ROOT-RESERVED` warning ramp from v2.51.0; the warning emission and `BoundaryConfig::reserved_root_zones()` method are removed. A new `FALLOW-BOUNDARY-ROOT-REDUNDANT-PREFIX` validation flags patterns that double-prefix the root (e.g., `root: "packages/app/"` + `patterns: ["packages/app/src/**"]`) so users who manually pre-prefixed their patterns during the warning ramp see the migration cue. Validation is emitted via `tracing::error!` consistent with `validate_zone_references`. Root matching is case-sensitive, matching globset's pattern conventions; locked down with a regression test to prevent silent platform-divergent classification on case-insensitive filesystems. New ADR: `decisions/007-boundary-zone-root.md`.
- **`<template>` complexity findings now cover inline Angular `@Component({ template: \`...\` })` decorators.** v2.51.0 emitted synthetic `<template>` cyclomatic/cognitive findings only for standalone `.html` files referenced via `templateUrl`, leaving codebases that put their templates inline (the modern Angular default) with zero coverage for the same control-flow density. The visitor now records each captured inline `template:` literal and the parser runs the existing template-complexity scanner over it, anchoring the resulting finding at the host file's `@Component`/`@Directive` decorator line. Jump-to-source lands on the decorator and `// fallow-ignore-next-line complexity` placed directly above the decorator suppresses through the existing health-side check, no extra suppression plumbing required. Template literals containing `${...}` expressions and `template:` properties bound to a variable are skipped (out of scope for the first cut). Cache version bumped to 49 so existing caches re-extract the new finding shape. Closes [#187](https://github.com/fallow-rs/fallow/issues/187).

### Changed

- **Schema version policy documented explicitly in `docs/output-schema.json`.** The `SchemaVersion` definition now states the bump policy: additive changes (new optional top-level fields, new optional struct fields, new array entries, new MCP tools, new CLI flags that map to new optional fields) do NOT bump the version; breaking changes (renamed fields, removed fields, type changes, enum-variant removals, semantic changes to existing fields) DO bump. Consumers should feature-detect new fields via JSON-key existence rather than gating on the version number. Resolves the ambiguity around `dupes --group-by` adding `grouped_by` / `total_issues` / `groups` without a version bump.

### Fixed

- **`cargo doc --document-private-items` no longer fails on `<template>` and unresolved intra-doc links.** Three rustdoc errors landed in earlier v2.51.x work: a bare `<template>` HTML tag in a doc comment on `InlineTemplateFinding`, an unresolved `[`VitalSigns`]` / `[`HealthScore`]` intra-doc link in `crates/cli/src/health/grouping.rs`, and a public-doc-links-private warning on `[`crate::health::HealthResult`]` in `crates/cli/src/health_types/grouped.rs`. CI `cargo doc --workspace --no-deps --document-private-items` is green again. Pre-existing problem; no behavior change.

## [2.51.0] - 2026-04-26

### Added

- **Synthetic `<template>` complexity findings for Angular HTML templates.** `fallow health --complexity` now scans standalone `.html` files for Angular template syntax and emits a synthetic finding named `<template>` whenever the template uses control-flow blocks (`@if`/`@else`/`@for`/`@switch`/`@case`/`@defer (when ...)`/`@let`), legacy structural directives (`*ngIf`, `*ngFor`, `[ngFor]`, `[ngForOf]`), expression-bound attributes (`[x]`, `(x)`, `bind-x`, `on-x`), or `{{ }}` interpolations. Cyclomatic increments per branch; cognitive accumulates with nesting and resets logical-operator runs across ternary branches. The finding flows through every output format (human, JSON, SARIF, CodeClimate, markdown, compact) and feeds the existing health score, hotspot, and refactoring-target pipelines. Suppress with `<!-- fallow-ignore-file complexity -->` at the top of the template; the JSON `actions` array carries a `suppress-file` entry with the exact comment so AI agents can apply it. Closes [#183](https://github.com/fallow-rs/fallow/issues/183). The CRAP-coverage story for `<template>` findings (JIT inherit-from-`.ts`, AOT source-map back-mapping) is tracked separately in [#186](https://github.com/fallow-rs/fallow/issues/186).
- **Tier-aware CRAP refactoring actions and baseline-aware suppression.** `fallow health --complexity` now emits coverage-leaning actions tailored to the function's coverage tier when CRAP triggers a finding: `add-tests` for `coverage_tier: none`, `increase-coverage` for `partial`/`high`, and `refactor-function` when even 100% coverage cannot bring CRAP below the threshold. CRAP-only findings within five of the cyclomatic threshold also append a secondary `refactor-function` action when cognitive complexity passes the cognitive floor (suppresses false positives on flat type-tag dispatchers and JSX render maps). Baseline-active runs and projects with `health.suggestInlineSuppression: false` no longer emit `suppress-line` actions; the report root carries an `actions_meta: { suppression_hints_omitted: true, reason: "baseline-active" }` breadcrumb so consumers can detect the omission.

### Changed

- **`--production-coverage` renamed to `--runtime-coverage` (no backwards-compatibility alias).** V8 and Istanbul measure invocations at runtime regardless of environment, so "production coverage" implied a restriction that did not exist; the beacon ships from any environment the operator points it at (staging, dev, load test, production). Surface changes: CLI flag `--production-coverage` → `--runtime-coverage`; MCP tool `check_production_coverage` → `check_runtime_coverage`; JWT feature claim `production_coverage` → `runtime_coverage`; SARIF rule IDs `fallow/production-*` → `fallow/runtime-*` (subtypes `safe-to-delete`/`review-required`/`low-traffic`); Rust types `ProductionCoverage*` → `RuntimeCoverage*`; source files `health_types/production_coverage.rs` → `runtime_coverage.rs`; tests `production_coverage_tests.rs` → `runtime_coverage_tests.rs`; snapshots `*_with_production_coverage.snap` → `*_with_runtime_coverage.snap`. The unrelated `--production` static-analysis flag (excludes test/dev files) is unchanged; only the paid V8/Istanbul coverage feature is renamed. SARIF / CodeClimate consumers that filter by `ruleId` need to update their selectors. Counterpart changes ship in fallow-cloud (sidecar + dashboard) and the companion repos (fallow-docs, fallow-skills) so the protocol, server, docs, and skill references stay in sync.

## [2.50.0] - 2026-04-26

### Added

- **Per-workspace health metrics with `--group-by package`.** `fallow health --group-by package|owner|directory|section` finally produces a real grouped envelope instead of silently discarding the flag. JSON output keeps `vital_signs`, `health_score`, `summary`, `findings`, `file_scores`, `hotspots`, `large_functions`, and `targets` at the project level so consumers that ignore grouping still see the headline, then adds `grouped_by` plus a `groups` array where each bucket carries its own `vital_signs` and `health_score` recomputed from the group's files. SARIF results gain `properties.group` and CodeClimate issues gain a top-level `group` field, so GitHub Code Scanning and GitLab Code Quality can partition findings per team or package without dropping out of the SARIF/CodeClimate pipeline. Human output adds a per-group score / files / hot / p90 summary block sorted worst-first when `--score` is set, with a color-coded grade column and a `(root)` legend for files outside any workspace package. Compact, markdown, and badge fall back to ungrouped output with a stderr note pointing at `--format json`. The `check_health` MCP tool description now spells out the per-group output so AI agents can use it for per-team or per-package quality questions in a single invocation. Closes [#184](https://github.com/fallow-rs/fallow/issues/184).
- **VS Code extension: `fallow.changedSince` setting to scope LSP diagnostics to changed files.** New extension setting accepts a git ref (e.g. `main`, `HEAD~1`, `origin/main`); the LSP only emits diagnostics for files changed since that ref. Useful for legacy codebases adopting fallow incrementally without surfacing the entire pre-existing dead-code backlog at once. The status bar tooltip surfaces the active scope so users can see when diagnostics are filtered.

### Fixed

- **`fallow health --workspace X` now scopes `vital_signs`, `health_score`, and `summary.files_analyzed` to the workspace's files.** Previously the workspace flag scoped only `findings`, `file_scores`, and `hotspots`, leaving the project-level metrics at monorepo-wide values; consumers asking "what is the health score of workspace X" got the monorepo answer back. Now every per-module aggregate (cyclomatic distribution, total LOC, unit profiles), every `analysis_counts` denominator (dead files, dead exports, unused deps, circular deps), and the summary file count is recomputed against the workspace subset. Internal change: introduces `SubsetFilter` (`Full` / `Workspaces` / `Paths`) and `AnalysisCountsSnapshot` retained on `FileScoreOutput` so per-subset counts can be derived without re-running the analyser.
- **`fallow health --group-by package` on a non-monorepo emits exactly one structured JSON error instead of two.** The pre-existing flow validated `--group-by package` *after* the hotspot pipeline, so a non-monorepo run produced a `hotspot analysis requires a git repository` error followed by a `--group-by package requires a monorepo` error in the same stdout, breaking any pipeline that did `jq .` on the output. Validation now runs upfront so misconfigured invocations short-circuit before any expensive work and emit a single error object. The non-monorepo error message also suggests `--group-by directory` as a fallback for single-package projects.
- **VS Code status bar: `changedSince` value is HTML-escaped in the trusted-content tooltip and truncated to 40 characters in the status bar.** Avoids breaking tooltip rendering when the ref string contains markdown-active characters and prevents long refs (commit SHAs, full origin/branch names) from squeezing the rest of the bar.

## [2.49.0] - 2026-04-25

### Added

- **Per-analysis production mode for the programmatic API.** `analyze_project` now accepts an optional `production` configuration that overrides the global `FallowConfig.production` flag for that single call, so an embedder can run a check pass with `production: false` (developer-mode entry points, full graph) and a follow-up health pass with `production: true` (start/build entries only, type-only deps detected) against the same project state without rebuilding config. CLI users get the same toggle via the existing `--production` flag, but the new path is the embedder API surface (Vite plugin, GitHub Action library mode, IDE plugins). Programmatic callers see `production: bool` on the analysis options struct; CLI callers see no behavior change.

### Fixed

- **Cross-package enum and class members are no longer flagged as unused when consumed through a barrel re-export.** When `enum Foo { A, B, C }` is defined in `lib/types.ts`, re-exported by `lib/index.ts` (`export { Foo } from './types'`), and consumed by another workspace package via `import { Foo } from '@scope/lib'; Foo.A`, every member of `Foo` was reported unused. Phase 4 chain resolution synthesizes a stub `ExportSymbol` on barrel files for reference tracking, which is indistinguishable from a real local declaration by name alone, so the access map keyed at the barrel while the detection loop looked up accesses at the origin file (where `members` are populated). `find_unused_members` now walks each access key through `ReExportEdge` chains (named, renamed, and `export *` fan-out) to every defining-site export and copies the access set. Same fix applies to `whole_object_used_exports` for `Object.values(Foo)` patterns. Covers both `unused_enum_members` and `unused_class_members`. Real-world impact on vue-core: 17 false positives eliminated (12 enum + 5 class). Closes [#178](https://github.com/fallow-rs/fallow/issues/178).
- **Embedder API now respects per-analysis production config end-to-end.** `analyze_project` was reading the global `FallowConfig.production` flag for downstream filtering even when the per-call options overrode it, so the new per-analysis production mode silently fell back to the project default in the trace and reachability passes. The flatten step now threads the resolved per-analysis flag through every detector consistently.
- **GitLab CI template defaults FALLOW_PRODUCTION* variables to empty strings.** The `.fallow:base` template previously set `FALLOW_PRODUCTION_CHECK: "false"` and similar; pipelines that overrode these via `extends:` could not unset them because GitLab merges variable values rather than replacing. Switching the defaults to `""` makes the template idempotent under user overrides.
- **Health Istanbul coverage now keys by `(name, line, col)` instead of `(name, line)`.** Curried arrow functions that share a start line on the same column would merge their coverage records and double-count one branch. Adding the column disambiguates the rare collision without affecting any other coverage shape.

## [2.48.5] - 2026-04-25

### Added

- **MCP trace tools for chasing why a finding exists.** Four new tools land in the MCP server: `trace_export` (why is this export reachable, or why isn't it), `trace_file` (which entry points pull this file in), `trace_dependency` (which scripts and config files keep this package alive), and `trace_clone` (where else does this clone group appear). Each returns the full propagation chain, not just the verdict, so an agent can walk from "fallow says X" back to the entry point or script that justifies it without re-running analysis. `trace_dependency` correctly credits packages referenced only from `package.json` `scripts` blocks (e.g., husky, lint-staged) so they stop showing as unused in the trace; `trace_clone` strips absolute paths from instance locations so the output stays portable across machines, and validation errors share the same error envelope as the rest of the MCP surface. Closes [#176](https://github.com/fallow-rs/fallow/pull/176). Thanks [@M-Hassan-Raza](https://github.com/M-Hassan-Raza).

### Fixed

- **VS Code status bar tooltip now renders codicons instead of literal `$(error)` / `$(warning)` / `$(check)` text.** The popup markdown is built as a `vscode.MarkdownString` and the missing `supportThemeIcons = true` flag meant every codicon shorthand fell through as raw text. Independently, the status bar text and tooltip could disagree (e.g., `0.8% duplication` in the bar, `0.6% duplication` in the popup) because two code paths drove the surface: the LSP `analysisComplete` notification updated both, while CLI completion only updated the text. Both paths now feed through one `buildParamsFromCli` + `applyTooltipAndSeverity` pipeline so text and tooltip always derive from the same data. Crashed analysis runs now also surface as `setStatusBarError()` instead of a misleading "$(check) No issues found" tooltip. Closes [#179](https://github.com/fallow-rs/fallow/issues/179).
- **Health CRAP score no longer mismerges curried arrow functions that share a start line.** When two named arrow functions in the same file had identical line numbers (typical for one-line curried definitions like `const f = (a) => (b) => a + b`), the per-function CRAP merge collapsed them into a single record and double-counted coverage. The merge key now includes function name alongside `(file, line)` so curried arrows produce distinct CRAP entries.

## [2.48.4] - 2026-04-24

### Fixed

- **npm release pipeline recovery, second attempt.** v2.48.3 shipped on crates.io and GitHub Releases but the npm-publish job failed again: the v2.48.3 release commit had `crates/napi/package.json` at 2.48.3 while the lockfile's `packages.node_modules/@fallow-cli/fallow-node-*` entries pointed at 2.48.1, so CI's `npm ci --omit=optional` rejected the repo as out-of-sync. The working v2.48.1 pattern leaves `crates/napi/*` entirely unchanged in the release commit and lets CI's own "Update NAPI package version" step bump them at publish time. v2.48.4 reverts `crates/napi/package.json`, `crates/napi/package-lock.json`, and `crates/napi/index.js` to that in-sync 2.48.1 state; CI then bumps them from the tag after `npm ci` passes. No user-visible behavior change relative to v2.48.2 / v2.48.3; this ships the same PandaCSS plugin, Angular template fixes, and MCP rename in a form that finally makes it through the npm publish pipeline.

## [2.48.3] - 2026-04-24

### Fixed

- **npm release pipeline recovery.** v2.48.2 shipped on crates.io and GitHub Releases but the `npm-publish` job failed at the pre-publish `npm ci` in `crates/napi/` because the canonical sync step's `npm install --package-lock-only` dropped the `packages.node_modules/@fallow-cli/fallow-node-*` entries (npm could not resolve the new platform-package versions pre-publish). v2.48.3 restores the lockfile's nested package entries pointing at the last-published platform version (2.48.1), leaving `optionalDependencies` at 2.48.3; npm ci is happy because the platform packages are optional. No user-visible behavior change relative to v2.48.2. See [`48a5947d`](https://github.com/fallow-rs/fallow/commit/48a5947d) for the prior manifestation of the same drift.

## [2.48.2] - 2026-04-24

### Added

- **PandaCSS framework plugin.** `panda.config.{ts,js,mjs,cjs}` is no longer flagged as an unused file when `@pandacss/dev` is a dependency. The Panda CLI (`panda codegen`, `panda`) discovers the config by filesystem convention with no import edge, so static analysis alone cannot see it as used. The new built-in plugin mirrors the UnoCSS / Tailwind pattern: enabler `@pandacss/dev`, config patterns and always-used on `panda.config.{ts,js,mjs,cjs}`, tooling dependencies for `@pandacss/dev` / `@pandacss/studio` / `@pandacss/eslint-plugin`, and `resolve_config` that captures both ES imports and shallow-string `presets` entries so preset packages like `@pandacss/preset-panda` and `@park-ui/panda-preset` are not flagged as unused. Closes [#175](https://github.com/fallow-rs/fallow/issues/175).

### Changed

- **MCP tool `health_production_coverage` renamed to `check_production_coverage`** for picker parallelism with `check_health` and `check_changed`. The tool description now leads with `(paid)`, and the server instructions gain a disambiguation line so agents pick the right tool between `check_health` (static, free) and `check_production_coverage` (V8-runtime-backed, paid). `check_health` also gains `min_observation_volume` and `low_traffic_threshold` params to reach parity with the underlying CLI flags, and `check_production_coverage` gains `group_by` for CODEOWNERS / directory / package / section grouping. Breaking change on the MCP wire protocol (tool name); accepted since this is a paid preview tool with no production users yet.

### Fixed

- **Angular inherited and DI-injected members referenced in external templates are no longer reported as unused.** Two independent root causes: (1) child-component template references bridged into `self_accessed_members` were not propagated up the `extends` chain to the base class's file (e.g., a base component's lifecycle hooks or helpers used in a child's `templateUrl`), and (2) the template scanner captured only top-level identifiers (losing `getTotal` in `{{ dataService.getTotal() }}`), and constructor-param types from DI were not exposed on `ModuleInfo`, so standalone-parsed HTML could not resolve chains to injected services. Scanner now returns member-access chains alongside identifiers; constructor params with accessibility modifiers and typed property declarations are captured into `ClassHeritageInfo.instance_bindings`; the analysis bridge resolves HTML chains through the importing component's bindings. Cache version bumped to 47. Closes [#174](https://github.com/fallow-rs/fallow/issues/174).

## [2.48.1] - 2026-04-24

### Fixed

- **`fallow setup-hooks` now ships a gate script that refuses to run with a stale fallow on `PATH`.** A fallow older than v2.46.0 (before the uncommitted-changes inclusion fix, commit `aabb8e1b`) silently passed audits that newer fallow would fail, defeating the purpose of the Claude Code `git commit` / `git push` gate. The generated `.claude/hooks/fallow-gate.sh` now enforces a `FALLOW_GATE_MIN_VERSION` floor (default `2.46.0`) and blocks with a copy-pasteable upgrade hint when the binary resolved from `PATH` is below it. Override with `FALLOW_GATE_MIN_VERSION=<semver>` or disable with an empty string. Every verdict=fail block now also prepends `fallow-gate: blocked by fallow <version> at <binary>` to stderr so the responsible binary is identifiable without re-diagnosis, and the installed script stamps `# Installer version: <semver>` in its header for forensics (substituted at install time from `fallow setup-hooks`'s own version). See <https://docs.fallow.tools/integrations/claude-hooks#version-floor>.

### Changed

- **Internal unit-size refactors with no behavioral change.** `find_unused_members` class inheritance propagation split into `build_parent_to_children` + `propagate_class_inheritance` helpers; `build_human_lines` / `build_sarif` / `build_codeclimate` each split into per-section helpers; package-path dep sections in CodeClimate collapsed behind a `NamedPkgDep` trait. Output byte-identical across all formats; same fingerprints, same severities, same issue ordering.

## [2.48.0] - 2026-04-23

### Added

- **Agent skill now ships inside the `fallow` npm package.** The `skills/fallow/` content (SKILL.md plus CLI reference, gotchas, and patterns references) is now packaged alongside the CLI wrapper, so `npm install fallow` places the skill on disk next to the binary the agent drives. This removes version drift between the installed binary and the skill an agent loads from a separately-cloned `fallow-skills` checkout, and removes the manual clone step from the onboarding path. Claude Code, Codex, and other agents that discover skills on disk pick it up without any extra configuration. The same content remains published from the `fallow-skills` repo for users who prefer plugin-style installs. A CI lane now verifies the bundled skill stays in sync with the source of truth on every PR. Closes [#173](https://github.com/fallow-rs/fallow/issues/173). Thanks [@OmerGronich](https://github.com/OmerGronich) for the proposal.

## [2.47.1] - 2026-04-23

### Fixed

- **Windows ARM64 (`aarch64-pc-windows-msvc`) is now a first-class release target.** `npx fallow` previously hard-failed on Windows ARM devices because no native binary was published. The release pipeline now builds and publishes `@fallow-cli/win32-arm64-msvc` for the CLI / LSP / MCP binaries and `@fallow-cli/fallow-node-win32-arm64-msvc` for the Node bindings, and the npm wrapper plus VS Code extension downloader both resolve the new target. A PR-time CI lane on native Windows ARM64 runners compiles the CLI and the NAPI addon so the target is covered before release instead of only at tag time. The npm platform resolution was also factored into a shared helper with tests so future targets stay consistent. Closes [#165](https://github.com/fallow-rs/fallow/issues/165). Thanks [@M-Hassan-Raza](https://github.com/M-Hassan-Raza) ([#171](https://github.com/fallow-rs/fallow/pull/171)).

## [2.47.0] - 2026-04-23

### Added

- **Native Node.js bindings package (`@fallow-cli/fallow-node`).** New async NAPI-RS bindings expose the main one-shot analyses directly inside Node without spawning the CLI: `detectDeadCode`, `detectCircularDependencies`, `detectBoundaryViolations`, `detectDuplication`, `computeComplexity`, and `computeHealth`. The bindings reuse the CLI orchestration layer and return the same JSON report envelopes the CLI emits, including `schema_version`, `summary`, relative paths, and injected `actions`. The Rust-side programmatic facade lives in `fallow-cli::programmatic`, and the repo now includes a temp-project Node smoke test plus CI/release wiring for the addon package. The JS API accepts lowercase CLI-style enum literals (`"mild"`, `"cyclomatic"`, `"low"`, `"handle"`) and rejected promises now expose structured fallow fields like `exitCode`, `help`, and `context`. See <https://docs.fallow.tools/integrations/node-bindings>.

### Fixed

- **Benchmark JSON parsing hardened against partial runs.** The benchmark harness used a best-effort parser that silently treated truncated or malformed JSON as "zero findings", so a crashed run scored as a clean run. Parsing now validates the envelope and rejects invalid payloads up front, so partial results no longer poison cross-tool comparisons.
- **Benchmark validity checks tightened; Vite overhead reduced.** The benchmark validity gate used to accept runs whose `summary.total_issues` silently disagreed with the sum of its per-category counts. It now rejects those runs, matching the stricter behavior the public comparison tables rely on. Adjacent Vite bench setup no longer installs dev dependencies it never invokes, cutting per-run overhead.
- **Recursive plugin config scans avoided.** Some framework plugins re-scanned the project config object every time a file was processed, producing redundant work on large monorepos. The scans now memoize the resolved config per analysis run, removing the redundant traversals.
- **`unused-listed-deps` no longer false-positives on broken `tsconfig.json` path aliases.** A `tsconfig.json` whose `compilerOptions.paths` pointed at a non-existent prefix would cause the resolver to fall through to "unlisted" on imports that were actually listed in `package.json`, flagging legitimate dependencies as unused. The resolver now treats unresolved alias prefixes as transient rather than definitive, so the listed-dep check only fires on genuinely missing entries.

### Changed

- **`cargo shear` no longer flags `tokio` in `crates/napi`.** The Node bindings crate no longer declares a direct `tokio` dependency; the napi `tokio_rt` feature already pulls it transitively.
- **NAPI option parsing no longer silently truncates `maxCyclomatic` / `maxCognitive`.** Both fields are now validated against `u16::MAX`; out-of-range values produce a typed `InvalidArg` napi error instead of wrapping to a different threshold.
- **`sync-npm-versions.sh` now rewrites the version strings NAPI-RS bakes into `crates/napi/index.js`.** Bumping a release used to leave the hardcoded `!== 'X.Y.Z'` comparisons at the previous tag, so strict `NAPI_RS_ENFORCE_VERSION_CHECK` consumers saw spurious mismatches on every release. The sync script now rewrites those literals in lockstep with `package.json`.

## [2.46.0] - 2026-04-23

### Added

- **`fallow setup-hooks` generates a Claude Code `PreToolUse` gate** that intercepts Claude's `git commit` / `git push` Bash tool calls, runs `fallow audit --format json --quiet --explain`, and blocks with exit 2 + JSON-to-stderr on `verdict: "fail"`. The agent reads the full audit envelope (including `_meta.docs` links and `actions` arrays on every finding), fixes the code, and retries. Runtime errors fail open with a single stderr notice so skips stay visible rather than silently trusting a missing gate. Writes project-level `.claude/settings.json` + `.claude/hooks/fallow-gate.sh` (chmod 755 on Unix), and optionally maintains an idempotent managed block in `AGENTS.md` between `<!-- fallow:setup-hooks:start/end -->` markers as a Codex fallback while Codex hooks stabilise. Flags: `--agent claude|codex`, `--dry-run`, `--force`, `--user` (installs globally under `~/.claude/` with a `$HOME`-based handler command), `--gitignore-claude`, `--uninstall`. The uninstall path removes the fallow handler while preserving any non-fallow handlers in the same matcher group, deletes the hook script if it still carries the generator marker, and strips the managed AGENTS.md block; idempotent on repeat runs. Re-running the installer upgrades handlers written by earlier manual setups (absolute paths, `~/`-based paths, Windows drive paths) to the canonical `"$CLAUDE_PROJECT_DIR"` form without leaving duplicates. See <https://docs.fallow.tools/integrations/claude-hooks>.
- **Zed editor extension.** Thanks [@M-Hassan-Raza](https://github.com/M-Hassan-Raza) ([#164](https://github.com/fallow-rs/fallow/pull/164)).

### Fixed

- **Health baselines survive line-number churn.** The health baseline keyed on `file + function + line_start`, so any refactor that shifted lines invalidated every entry and reported the same findings as "new." The key now uses `file + function + qualified_name` (line-stable), mirroring the dead-code and duplication baselines. Closes [#169](https://github.com/fallow-rs/fallow/issues/169). Thanks [@M-Hassan-Raza](https://github.com/M-Hassan-Raza).
- **`fallow --save-baseline <path>` auto-creates the parent directory.** Copy-pasting the `fallow-baselines/dead-code.json` example into a fresh repo used to error with "No such file or directory" because the baseline writer never ran `mkdir -p`. Applies to `dead-code`, `health`, and `dupes` save-baseline paths.
- **Shallow nested package scripts are discovered again.** The package-script walker stopped descending into `workspaces/*/package.json` entries that lived one directory below the root, silently dropping nested script declarations from the entry-point graph. The walker now follows shallow nesting the same way it follows deep nesting.

## [2.45.1] - 2026-04-22

### Fixed

- **Istanbul `--coverage` now matches functions produced by standard Istanbul tooling (Jest, nyc, c8, babel-plugin-istanbul).** Standard Istanbul producers omit the non-standard `FnEntry.line` field that `oxc-coverage-instrument` writes, so fallow's `load_istanbul_coverage` silently defaulted the line to `0` and every lookup failed — `istanbul_matched: 0` out of N, all CRAP scores fell back to binary estimation, and `--max-crap` ran against estimates instead of real coverage. The loader now falls back to `FnEntry.decl.start.line` when `FnEntry.line` is missing, restoring the anonymous-by-line and fuzzy lookup paths for Jest/nyc/c8-produced coverage while preserving the fast path for `oxc-coverage-instrument` output. Closes [#166](https://github.com/fallow-rs/fallow/issues/166).
- **Repeated "entry point outside project root" warnings collapse into a single diagnostic.** Monorepos with shared entry points (e.g. workspace `package.json` pointing at a sibling build output) used to emit the same warning once per analysis target, spamming `stderr` with dozens of identical lines. The warnings are now deduplicated with a counted summary so the signal stays visible without drowning out other diagnostics.
- **`fallow dupes` tuning flags (`--min-lines`, `--min-tokens`, `--mode`, `--cross-language`) are forwarded correctly in combined GitLab CI mode.** The wrapper's combined-mode `ARGS=()` block built the `dupes` invocation without reading the `FALLOW_DUPES_*` env vars, so user overrides set in `.gitlab-ci.yml` silently became defaults. Fix pairs with the same GitHub Action fix from v2.44.x so both CI integrations honor the same tuning surface.
- **Entry point discovery and VS Code extension downloads are hardened.** The entry-point walker now tolerates package manifests with missing / non-object `bin` or `exports` fields instead of aborting discovery, and the VS Code extension's binary-download path validates content-length + checksum against the expected artifact before unpacking (no more silent half-downloads that fail the first `fallow` invocation with an opaque error).
- **CLI `stderr` noise reduced on normal runs.** Incidental `tracing` / `println!` lines that leaked into `stderr` during analysis (progress prints, one-off debug lines) are gone; `FALLOW_QUIET=1` runs are byte-clean on stdout/stderr for scripting integrations.
- **`--changed-since` now includes untracked files that were added in the working tree but not yet committed.** Changed-scope filtering based on `git diff` missed brand-new files until they were staged, so a PR-style run (`--changed-since origin/main`) could falsely report zero issues on new code. Untracked-and-added paths are now folded into the changed set.
- **Production coverage signal alignment in CI reporting.** `fallow health --production-coverage` output now consistently reports the same matched / unmatched / stale counts across human, JSON, markdown, and the GitHub Action / GitLab CI summary + annotation pipelines. Prior releases could show different subtotals depending on whether combined mode or the standalone command ran.

## [2.45.0] - 2026-04-22

### Added

- **Static inventory extractor (`fallow-extract`).** New extraction phase that produces a per-file list of exported functions keyed by `(path, name, line)`. Feeds the line-aware production-coverage matcher so runtime samples can be joined against the AST view of "every function that exists" without collisions on overloaded names.
- **`fallow coverage upload-inventory`.** Uploads the static function inventory to fallow cloud so the Production Coverage dashboard can filter `untracked` functions (the AST side of the join: functions that exist in the code but were never observed at runtime). Requires a valid license. See <https://docs.fallow.tools/analysis/production-coverage>.

### Fixed

- **External CSS packages imported from Vue SFC `<style>` blocks are credited as used.** CSS-only design-system packages imported only through `<style src="pkg/dist/style.css">` or an `@import "pkg/..."` inside a Vue SFC `<style>` block used to be reported as `unused-dependencies` because the Vue plugin only walked script imports. A new external-style scanner now recognizes style-side package imports declared in `.vue` SFCs and credits the npm package, eliminating the false positive for Vue + Vite design-system codebases.
- **`compilerOptions.paths` aliases in `tsconfig.json` are honored.** Path aliases like `"@/*": ["src/*"]` declared in `compilerOptions.paths` are now extracted by the TypeScript plugin and registered as resolver path aliases, so `@/foo` imports resolve to `src/foo` in projects that rely on alias-only paths without a matching `vite.config` / `webpack.config` rewrite.
- **`tsconfig.references` entries pointing at a `.json` file directly are followed.** References of the form `{ "path": "./tsconfig.app.json" }` are now resolved to the literal file rather than getting `tsconfig.json` concatenated onto them, fixing reference-chain walking for the common "one tsconfig per build" pattern.
- **Coverage inventory upload aligned with line-aware function identity.** `fallow coverage upload-inventory` now emits records keyed by `(file, name, line)` to match the line-aware sidecar protocol introduced alongside v2.44.x, replacing the older line-less identity that could collide on overloaded / re-declared names. Upload integration hardened against CI-only path normalization regressions that surfaced on Linux runners.

## [2.44.2] - 2026-04-21

### Fixed

- **Class members used only through interface-typed bindings are no longer flagged unused.** When a class method was called only via an interface-typed variable or parameter (e.g. `const strategy: VirtualScrollStrategy = ...; strategy.attach();` where `FixedSizeScrollStrategy implements VirtualScrollStrategy`), the method appeared as an unused class member because the access resolved to the interface name, not the implementer. The extractor now tracks type-annotated bindings (locals, parameters, class fields, parameter properties) alongside `new ClassName()` bindings, and the member-usage analysis propagates interface member accesses to every class that `implements` the interface. Same-named interface exports in separate files stay isolated by `ExportKey`, so unrelated implementers do not silently credit each other. Cache version bumped to 44 so warm caches pick up the new extraction records on upgrade. Closes [#132](https://github.com/fallow-rs/fallow/issues/132). Thanks [@M-Hassan-Raza](https://github.com/M-Hassan-Raza).
- **`react-router.config.ts` route modules with a `routesFn` are honored.** Config-driven route modules (`import { flatRoutes } from "@react-router/fs-routes"; export default { routes: flatRoutes() }`) are now extracted the same way as static `routes: [...]` arrays, so route files referenced only through the filesystem convention are no longer reported as unused files in React Router 7 apps.
- **`rand 0.8` re-pinned in `fallow-license` dev-dependencies.** A Dependabot bump to `rand 0.9` broke the `OsRng` + `SignatureEncoding` wiring used by test key generation; the release build never exercised that path so CI missed the regression. Dev-deps now stay on 0.8 until the signing path is ported to 0.9's new `TryRngCore` trait set.

## [2.44.1] - 2026-04-21

### Fixed

- **Istanbul `--coverage` now matches arrow-function exports against `(anonymous_N)` entries.** Istanbul records arrow functions bound to a `const` (e.g. `export const handler = () => {...}`) as `(anonymous_0)`, while fallow extracts the binding identifier (`handler`). Neither the exact `(name, line)` match nor the name-only fuzzy match succeeded, so arrow-heavy codebases silently fell through to the estimated coverage model with `istanbul_matched: 0`, and `--max-crap` ran against estimates instead of real coverage. `IstanbulFileCoverage::lookup` now has a third fallback: when no name-based match exists and exactly one `(anonymous_N)` entry starts within ±2 lines of the requested line, use it. The single-candidate guard keeps the match unambiguous (multiple candidates return `None` rather than risk misattribution). Closes [#155](https://github.com/fallow-rs/fallow/issues/155).

## [2.44.0] - 2026-04-21

### Added

- **`--max-crap <N>`: per-function CRAP threshold on `fallow health` and `fallow audit`.** New threshold flag (and `health.maxCrap` config key, default 30.0) that fails the command when any function's CRAP (Change Risk Anti-Patterns) score meets or exceeds the threshold. Mirrors the existing `--max-cyclomatic` / `--max-cognitive` gates but uses the Savoia and Evans (2007) canonical formula `CC^2 * (1 - cov/100)^3 + CC`, which combines complexity with coverage so an AI-assisted workflow can force refactors or test coverage on exactly the functions that combine both risk dimensions. Pair with `--coverage ./coverage/coverage-final.json` for Istanbul-backed per-function coverage; without Istanbul data fallow estimates coverage from the module graph (directly test-referenced = 85%, indirectly reachable = 40%, untested = 0%). `HealthFinding` gains optional `crap` and `coverage_pct` fields and the `exceeded` enum gains four CRAP-containing variants (`crap`, `cyclomatic_crap`, `cognitive_crap`, `all`); the CRAP-only finding carries an `add-tests` action in JSON output because coverage is the leverage point, not refactoring. Findings are filterable by `--baseline` / `--save-baseline` the same way complexity findings are. SARIF adds a new `fallow/high-crap-score` rule (12 health rules total), CodeClimate maps CRAP findings to a matching check name, markdown renders a dedicated CRAP column, and human output appends a third metrics line with `(N% tested)` when Istanbul matched. MCP surfaces `max_crap` on `check_health`, `audit`, and `health_production_coverage`. GitHub Action and GitLab CI expose the input with floating-point validation and thread it through the jq summary, annotation, and review-comment templates. Closes [#141](https://github.com/fallow-rs/fallow/issues/141).
- **`development` export condition honored by the resolver.** The Oxc resolver is now configured with `"development"` in the baseline condition set alongside `import`, `require`, `default`, `types`, and `node`. Packages that publish a `development` branch in `exports` / `imports` (common pattern: source files for dev, compiled output for prod; used by Vite, Vitest, esbuild, Rollup) now resolve deterministically to source when fallow walks them. The `resolve.conditions` config key lets users prepend custom condition names (e.g. `["test"]`, `["bun"]`) ahead of the baseline so codebases with custom conditional-export setups can opt into the variants that match their toolchain. When the React Native or Expo plugin is active, `"react-native"` and `"browser"` prepend automatically as before. Closes [#135](https://github.com/fallow-rs/fallow/issues/135).

### Changed

- **README summary block matches actual `--summary` output.** The hero block on the README used to show a hand-written sketch of what `fallow --summary` produced; it now mirrors the real output byte-for-byte so new users scanning the front page see the same column alignment and labels they will encounter when they run the command.

## [2.43.0] - 2026-04-21

### Added

- **`fallow audit` per-analysis baselines.** Three new flags on the `audit` command let each sub-analysis (dead code, health, duplication) filter against its own saved baseline: `--dead-code-baseline <PATH>`, `--health-baseline <PATH>`, `--dupes-baseline <PATH>`. Baselines reuse the formats produced by `fallow dead-code --save-baseline`, `fallow health --save-baseline`, and `fallow dupes --save-baseline` respectively (no new format). Issues present in the respective baseline are excluded from the audit verdict, so touching a legacy file with pre-existing findings no longer makes `fallow audit` fail on a PR that introduces no new issues. Defaults can be set in `.fallowrc.json` under `audit.deadCodeBaseline` / `audit.healthBaseline` / `audit.dupesBaseline`, with CLI flags overriding. Relative paths from either source resolve against `--root` so CI scripts using `--root $REPO_ROOT` behave consistently with interactive use. The MCP `audit` tool exposes the same three params (`dead_code_baseline`, `health_baseline`, `dupes_baseline`). Closes [#139](https://github.com/fallow-rs/fallow/issues/139).
- **`group_by` on `find_dupes`, `check_health`, and `audit` MCP tools.** The three sibling tools now accept the same `group_by` param (`"owner"` / `"directory"` / `"package"` / `"section"`) that `analyze` already accepts, so agents can partition duplication, complexity, and audit output the same way they partition dead-code output. Fixes an oversight from the `--group-by section` shipping in v2.42.0 where the flag reached every CLI surface but only one of four MCP tools.

### Changed

- **Global `--baseline` / `--save-baseline` are now rejected on `fallow audit`** (exit 2) with an error pointing users at `--dead-code-baseline`, `--health-baseline`, and `--dupes-baseline`. Audit runs three sub-analyses with different baseline formats, so a single baseline path was ambiguous. Previously the global flag was silently ignored on `audit`. If you were passing it on audit before, it was already a no-op; switch to the per-analysis flags.

### Fixed

- **Nuxt shared auto-imports are no longer flagged as unused.** Composables and utilities exported by auto-import-aware Nuxt packages (`#imports`, framework-scoped auto-imports) were surfacing as unused-export false positives in projects that relied on the auto-import dance instead of explicit `import` statements. The Nuxt plugin now recognizes these shared auto-imports and treats them as always-reachable.

## [2.42.0] - 2026-04-20

### Added

- **`--group-by section`: GitLab CODEOWNERS section grouping.** New `--group-by` mode that partitions output by GitLab CODEOWNERS `[Section]` header name instead of by primary owner. Distinct sections stay in distinct groups even when they share a default reviewer, so repos where one common approval group is listed first on every section no longer collapse every finding into a single bucket. In JSON output each group carries an `owners: string[]` array populated from the section's default owners. Rules declared before any section header fall into the `(no section)` bucket; unmatched paths continue to use `(unowned)`. Human and markdown formats surface the owners under each group header. `--group-by section` against a CODEOWNERS file that contains no `[Section]` headers errors out with a pointer to `--group-by owner`, rather than silently falling back. MCP's `analyze` tool forwards `section` as a valid `group_by` value with `owners: string[]` metadata documented on the param. Closes [#133](https://github.com/fallow-rs/fallow/issues/133).
- **`health_production_coverage` MCP tool: focused paid production-coverage entry point.** New MCP tool that wraps `fallow health --production-coverage <path>` with a narrow parameter surface so agents can discover the right tool by name instead of passing special fields to the general `check_health` tool. Required `coverage` param (V8 directory, V8 JSON, or Istanbul JSON file). Optional scope passthroughs: `root`, `config`, `production`, `workspace`, `no_cache`, `threads`. Tuning knobs: `min_invocations_hot`, `min_observation_volume`, `low_traffic_threshold`. Tool description warns about raising `FALLOW_TIMEOUT_SECS` for multi-megabyte dumps that exceed the 120s default MCP subprocess timeout.
- **Production-coverage `capture_quality` summary and trial CTA.** Bumps `fallow-cov-protocol` from 0.2 to 0.3 and wires the new `Summary.capture_quality` block (`window_seconds`, `instances_observed`, `lazy_parse_warning`, `untracked_ratio_percent`) through the health pipeline. When the sidecar flags a short-window or lazy-parse capture, the human renderer shows a yellow `note:` caveat plus a quantified trial CTA for `fallow license activate --trial`; long / clean captures stay quiet. Markdown surfaces the warning for PR comments (no CTA). JSON passes the block through for agent consumers. `schema_version` stays at 4 per the additive-fields policy.
- **`--changed-workspaces <REF>`: git-derived monorepo CI scoping.** New global flag that auto-detects which workspaces contain any file changed since the given git ref (e.g. `origin/main`, `HEAD~1`) and scopes output to that set. Removes the need for CI jobs to hand-maintain a `--workspace a,b,c` list as the package graph evolves. Reuses the existing `--workspace` filter path end-to-end, so dead-code, health (complexity + hotspots + coverage), flags, audit, and the combined pipeline all respect the derived scope identically. Mutually exclusive with `--workspace`: validated at the CLI layer with a targeted error. Unlike `--changed-since`, a missing ref or non-git directory is a hard error (exit 2) rather than a silent full-scope fallback. The flag's entire purpose is to narrow CI scope, so quietly widening back to the whole monorepo would defeat the optimization. When git succeeds but no tracked workspace file changed (e.g. a root-only lockfile bump), the scope is empty and the pipeline exits 0 with no issues reported. GitHub Action and GitLab CI both expose the new input and wire it through the analyze pipeline. Closes knip PR #1455 parity gap.

### Fixed

- **`dupes` now respects `--workspace` and `--changed-workspaces` scoping.** Previously `fallow dupes --workspace X` returned every clone group in the repo, as did combined mode (`fallow --workspace X` / `fallow --changed-workspaces REF`). The workspace flag was wired into `check` and `health` but not `dupes`, so in a monorepo every duplication report spanned the whole project regardless of scope. Surfaced during the post-ship end-to-end test of `--changed-workspaces` on next.js, where combined mode reported 4780 clone groups despite a narrow scope. Fixed by adding `workspace` / `changed_workspaces` to `DupesOptions`, a new `filter_by_workspaces` pass that retains clone groups with at least one instance under any selected workspace root (rebuilding clone families, mirrored-directory detection, and duplication stats from the survivors), and wiring it through the standalone `fallow dupes`, combined, and `fallow audit` entry points. No schema changes; reported `duplication_percentage` now reflects the scoped slice instead of the whole repo.
- **Nuxt CSS aliases resolve through `normalize_nuxt_path`.** Local CSS entries (`~/`, `~~/`, `@/`, `@@/`, `./`, `/`) now route through the same alias resolver as plugins, components, and aliases, so they match how the rest of the Nuxt plugin already resolves paths. Bare specifiers stay treated as npm package CSS. Adds an integration fixture (`nuxt-css-alias`) plus unit coverage for nested-config and `srcDir: 'app'` combinations.
- **`FALLOW_LICENSE_PATH=""` is now treated as unset.** A shell exporting an empty-string `FALLOW_LICENSE_PATH` previously produced a cryptic `license I/O error: No such file or directory (os error 2)` on any paid-feature command, because `std::env::var` returns `Ok("")` and `load_raw_jwt` handed that to `Path::new("")`. Empty and whitespace-only values now fall through to default discovery, matching the user mental model and mirroring the neighboring `FALLOW_LICENSE` normalize-and-check-empty pattern.

## [2.41.0] - 2026-04-20

### Added

- **`--workspace` accepts multiple patterns with globs and `!` negation.** The global `-w` / `--workspace` flag is no longer limited to a single exact package name; it now accepts comma-separated values (`-w web,admin`), repeated flags (`-w web -w admin`), glob patterns matched against BOTH the `package.json` name AND the workspace path relative to the repo root (`-w 'apps/*'`, `-w '@scope/*'`), and `!`-prefixed negations (`-w '!apps/legacy'`). Combinations follow gitignore-style rules: positive-only includes matches, negative-only starts from all workspaces and removes matches, mixed applies positives then subtracts negatives. Literal package names that happen to contain glob metacharacters (e.g. `web-[staging]`) are handled by an exact-name short-circuit so they still match without any quoting dance. Single-value usage (`-w my-package`) stays fully back-compatible. Propagated through `dead-code`, `health` (including hotspots, coverage sidecar, large-function analysis), `flags`, `audit`, and the combined pipeline. MCP `workspace` param keeps its `Option<String>` shape so existing agent integrations continue to work; docstrings document the new syntax. Unmatched positive patterns are collected into a single "no workspaces matched patterns: 'X', 'Y'. Available: a, b, ..." error (exit 2) rather than one error per miss, and the available-workspaces list caps at 10 names for large monorepos. The "all workspaces excluded" error splits Included vs Excluded so a typo in a negation pattern is immediately visible. Closes knip issue #1441.

### Fixed

- **Sidecar discovery now prefers the real platform binary over the Node wrapper.** After `npm install @fallow-cli/fallow-cov`, `node_modules/.bin/fallow-cov` is a Node wrapper script that re-execs the real binary in the platform subpackage (e.g. `@fallow-cli/fallow-cov-darwin-arm64/fallow-cov`). The wrapper path has no adjacent `.sig` file, so the Phase 2.5 signature verification gate refused to spawn with `Sidecar binary at ... is missing its signature file` (exit 4) even though the signed binary and its `.sig` were sitting right next to each other in the platform subdirectory. Discovery now walks up ancestors looking for `node_modules/@fallow-cli/fallow-cov-*/fallow-cov` first, then falls back to the `.bin` wrapper for non-npm-optionalDependency layouts. Every v2.40.0 / v2.40.1 / v2.40.2 user running `fallow health --production-coverage` for the first time hit this; `FALLOW_COV_BINARY_PATH=...` worked around it but the happy path should not require an env var. Found via Phase 2.5 end-to-end smoke test against sidecar-v0.1.5.

## [2.40.2] - 2026-04-17

### Fixed

- **`super.method()` calls now credit the parent class's method as used.** Base-class methods only invoked via `super.method()` from subclasses were silently dropped by the AST visitor and surfaced as false-positive `unused-class-member` findings. A `class_super_stack` now tracks the nearest enclosing class's `extends` identifier while walking the class body; `super.<member>` expressions emit a member access against that identifier, and the existing `local_to_imported` resolver in `find_unused_members` maps it back to the parent's export name. Object-literal `super` (no enclosing class on the stack) is correctly dropped, and nested class expressions respect their own frame. Verified against svelte (13 false positives eliminated, each traced to a real `super.<member>` call in source) and vite (-2 false positives); zero new findings introduced on any benchmark fixture. Closes [#130](https://github.com/fallow-rs/fallow/issues/130).

## [2.40.1] - 2026-04-17

### Fixed

- **`BINARY_SIGNING_VERIFY_KEY` rotated to match the key pair now deployed in the sidecar release pipeline.** The first two `fallow-rs/fallow-cloud` sidecar release tags (`sidecar-v0.1.0`, `sidecar-v0.1.1`) failed in CI because the `ED25519_BINARY_SIGNING_PRIVATE_KEY` GitHub secret had been set with a malformed `gh secret set --body -` invocation that stored the literal `-` rather than reading the seed from stdin. Recovering the original seed was not possible (write-only secrets, local env scrubbed after write), so a fresh key pair was provisioned across Fly (staged), GitHub Actions secret (validated via `workflow_dispatch`), and GitHub Actions variable. This commit updates the compiled-in verify key so v2.40.1+ CLI binaries accept signatures produced by the new signing key. v2.40.0 remains safe -- no sidecar binaries were ever published under the broken configuration (`sidecar-v0.1.0` / `sidecar-v0.1.1` reached neither npm nor the GitHub release assets), so no user-visible regression to undo. First real signed sidecar will ship against v2.40.1 + `sidecar-v0.1.2`.

## [2.40.0] - 2026-04-17

### Added

- **Scoped `usedClassMembers` rules** -- `usedClassMembers` entries may now be either a plain member name (global suppression, as before) OR a scoped object with `extends` / `implements` constraints. Framework-invoked method names that are too common to globally suppress (e.g. TypeORM `MigrationInterface.up` / `down`, ag-Grid `agInit`, Web Components `connectedCallback`) can be scoped so they only suppress on classes matching the specified heritage clause. Plain string entries keep their previous semantics; schema is a backward-compatible superset. New `UsedClassMemberRule` / `ScopedUsedClassMemberRule` types exposed in `schema.json` and `plugin-schema.json`. Thanks @M-Hassan-Raza ([#129](https://github.com/fallow-rs/fallow/pull/129)).
- **`FALLOW_COV_BINARY_PATH` env var** -- second explicit-override slot for the production-coverage sidecar binary, ordered between `FALLOW_COV_BIN` and project-local lookup. Required for air-gapped enterprise installs, Linux distro-packaged sidecars, and Docker multi-user setups where `~/.fallow/bin` is not writable. Missing-file returns a clear error instead of silently falling through to auto-discovery, matching `FALLOW_COV_BIN`'s explicit-beats-implicit semantics.
- **Ed25519 signature verification on every sidecar spawn** -- the `fallow-cov` production-coverage sidecar binary must ship with an adjacent `<binary>.sig` file. `fallow health --production-coverage` verifies the signature against a compiled-in public key (`BINARY_SIGNING_VERIFY_KEY`) before executing the sidecar and fails hard (exit code 4) on any of: missing `.sig` file, wrong signature length, or invalid signature. Strict from day one -- no warn-and-run, no opt-out env var. Hardens the sidecar supply chain across npm mirrors, air-gapped installs, and ad-hoc binary placement. Error messages direct users to `npm install @fallow-cli/fallow-cov`.

### Fixed

- **`fallow license` non-Unix stub** no longer fails clippy under rust 1.95 -- the `#[cfg(not(unix))]` permission-set stub returns `Result<(), String>` to mirror the Unix variant's signature, which rust 1.95's `unnecessary_wraps` flags. Added an explicit `#[allow]` with a reason.

## [2.39.0] - 2026-04-17

### Added

- **Production Coverage Intelligence (Phase 2)** -- `fallow health --production-coverage <path>` merges V8 or Istanbul runtime coverage into the existing health report. Spawns the closed-source `fallow-cov` sidecar (distributed separately; binary distribution deferred to Phase 2.5) and surfaces a typed report with stable content-hash IDs, per-finding verdicts, a supporting evidence block, hot-path percentiles, and a `license-expired-grace` watermark track. Paid feature — gated on a valid Ed25519-signed license JWT with the `production_coverage` feature.
- **`fallow license` subcommand** -- `activate` (with `--trial --email <addr>` for zero-credit-card onboarding or `--from-file <path>` / stdin for existing licenses), `status`, `refresh`, `deactivate`. JWT verification is fully offline against a compiled-in Ed25519 public key; only `activate --trial` and `refresh` make network calls (5s connect timeout, 10s total). License is stored at `~/.fallow/license.jwt` (or `$FALLOW_LICENSE_PATH`) with owner-only permissions on Unix; the Windows fallback honours `%USERPROFILE%`. Grace ladder matches Docker Desktop / JetBrains: 0–7 day warning, 7–30 day watermark, 30+ day hard-fail.
- **`fallow coverage setup`** -- single-entry-point resumable first-run flow that walks a user through: license check, `fallow-cov` sidecar discovery/install, framework-specific coverage recipe (Next.js / Nest / SvelteKit / Remix / Astro / plain Node), and automatic handoff into `fallow health --production-coverage`. Sidecar discovery resolves `$FALLOW_COV_BIN` → `./node_modules/.bin/fallow-cov` → package-manager bin → `~/.fallow/bin/fallow-cov` → `$PATH`.
- **`--min-observation-volume <N>`** on `fallow health` -- minimum total trace volume before the sidecar is allowed to emit high-confidence `safe_to_delete` / `review_required` verdicts. Below this threshold the sidecar caps confidence at `medium`. Defaults to the spec value (5000) when omitted.
- **`--low-traffic-threshold <RATIO>`** on `fallow health` -- fraction of total trace count below which an invoked function is classified as `low_traffic` rather than `active` (e.g. `0.001` for 0.1%). Defaults to the spec value when omitted.
- **`fallow-cov-protocol 0.2.0`** -- new published crate (`crates.io`) holding the versioned envelope types shared between the public CLI and the closed-source sidecar. Stable content-hash IDs (`fallow:prod:<hash>` / `fallow:hot:<hash>`), per-finding `Verdict` / `Confidence` / `Evidence`, `ReportVerdict` renaming, `StaticFunction::static_used` + `test_covered` as required fields, new `Options` knobs, and forward-compat `Unknown` sentinels on every enum.
- **SARIF rules for production coverage** -- `fallow/production-safe-to-delete`, `fallow/production-review-required`, `fallow/production-low-traffic`, `fallow/production-coverage-unavailable`, `fallow/production-coverage`. Each rule has `fullDescription` and `helpUri` from the shared explain module.
- **CodeClimate severity mapping for production coverage** -- `safe_to_delete` → `critical`, `review_required` → `major`, everything else → `minor` (GitLab Code Quality only renders inline MR annotations for blocker/critical/major/minor).

### Changed

- **`schema_version` bumped 3 → 4** on `fallow health --format json` -- the `production_coverage` object is reshaped to the 0.2 contract. Renames: `functions_total` → `functions_tracked`, `functions_called` → `functions_hit`, `functions_never_called` → `functions_unhit`, `functions_coverage_unavailable` → `functions_untracked`, `percent_dead_in_production` → `coverage_percent` (inverted semantics). Findings now carry `id`, required `line`, `verdict` (per-finding: `safe_to_delete` / `review_required` / `coverage_unavailable` / `low_traffic` / `active`), `invocations` (nullable for untracked functions), and a full `evidence` block. Hot paths carry `id`, required `line`, and `percentile`. Summary adds `trace_count`, `period_days`, `deployments_seen`.
- **Baseline dedup for production-coverage findings** keys on the sidecar's stable content-hash ID (`fallow:prod:<hash>`) rather than a derived `path:function:line:state` string. Moving a function now correctly shows as a fresh finding instead of a stable baseline match.

### Fixed

- **GitLab CODEOWNERS section headers (`[Docs]`, `^[Frontend]`) now parsed correctly** -- previously treated as regular rules, which produced nonsensical "reviewer: @Frontend" output. Section headers are now recognised and their nested rules attributed to the section. Section-level exclusions (`^[Frontend] @excluded`) are also respected.
- **`~` home-directory resolution on Windows** for `$FALLOW_LICENSE_PATH` and `~/.fallow/license.jwt` -- previously failed with "user home directory not found" on Windows where `$HOME` is unset by default. Now falls back to `%USERPROFILE%`.
- **`fallow health` exit code triggers on `SafeToDelete` + `ReviewRequired` verdicts** (replacing the 0.1 `NeverCalled` state) so CI continues to fail on definitive dead-code signals.
- **Numerous production-coverage hardening fixes** from the Phase 2 beta cycle: license-gate parity between CLI and sidecar, runtime license-key override removal, local license verification key support, coverage input detection polish, CI stabilization for Rust 1.95, sidecar setup recipe improvements, and protocol-version enforcement keyed on `0.<minor>` while the contract stays on 0.x.

### Breaking

- **`fallow health --production-coverage` JSON output** has moved from the 0.1 shape to the 0.2 shape described above (`schema_version 3 → 4`). Consumers of the `production_coverage` object must update field names and switch from the per-finding `state` enum (`called` / `never-called` / `coverage-unavailable`) to the `verdict` enum (`safe_to_delete` / `review_required` / `coverage_unavailable` / `low_traffic` / `active`). The top-level report `verdict` is unchanged (`clean` / `hot-path-changes-needed` / `cold-code-detected` / `license-expired-grace`).
- **`fallow-cov-protocol` requires `StaticFunction::static_used` + `test_covered`** -- any existing non-Fallow consumer of the protocol crate must populate both fields on every `StaticFunction`. The sidecar rejects 0.1-shape requests at deserialization (exit code 2) rather than silently defaulting to "used + covered", which would hide every `safe_to_delete` finding.

## [2.38.0] - 2026-04-15

### Added

- **Ownership risk analysis on hotspots** -- `fallow health --hotspots --ownership` surfaces per-file ownership signals derived from git author history and CODEOWNERS. For each hotspot: Avelino truck factor (`bus_factor`), contributor count, top contributor with stale-days and commits, up to three additional recent contributors, `suggested_reviewers` (recent contributors with `stale_days < 90` — first-class field for AI agent routing), declared CODEOWNERS owner, tristate `unowned` flag (`true`=no rule matches / `false`=rule matches / `null`=no CODEOWNERS file), drift detection with human-readable reason. Human output prepends a project-level summary line (`9/10 hotspots depend on a single recent contributor · top authors: @alice (6), @bob (4)`) so tech leads see the organizational pattern before scanning per-file rows. JSON gains three new action types: `low-bus-factor` (with file-specific candidate reviewers from `suggested_reviewers`), `unowned-hotspot` (with synthesized CODEOWNERS pattern + `heuristic: "directory-deepest"` discriminator for future evolution), and `ownership-drift`. Test-path hotspots tagged `[test]` in human output and `is_test_path: true` in JSON. Only the free tool combining dead code + complexity + ownership risk. Research-backed (Avelino et al., Thongtanunam et al. ICSE 2016). ([#116](https://github.com/fallow-rs/fallow/discussions/116))
- **`--ownership-emails={raw|handle|hash}` privacy control** -- chooses how author emails are rendered in output. `handle` (default) shows the local-part only, unwrapping GitHub-style noreply prefixes (`12345+alice@users.noreply.github.com` → `alice`); `hash` emits stable non-cryptographic `xxh3:<16hex>` pseudonyms for regulated environments where author identities are sensitive in CI artifacts (SARIF, code-scanning uploads); `raw` shows full email addresses for public OSS repos. `ContributorEntry` gains a `format` discriminator so typed JSON consumers can branch without re-parsing the identifier. Configure the repo-wide default via `health.ownership.emailMode` in config.
- **`health.ownership` config section** -- `botPatterns` (glob patterns matched against raw author emails; defaults cover `*\[bot\]*`, `dependabot*`, `renovate*`, `github-actions*`, `svc-*`, `*-service-account*` — `*noreply*` is deliberately NOT a default because it would filter real human contributors using GitHub's privacy-default email format), `emailMode` (raw/handle/hash).
- **Ownership signals in MCP `check_health` tool** -- new `ownership: bool` and `ownership_email_mode: "raw"|"handle"|"hash"` params (typed enum so JSON Schema constrains input at the agent layer). Tool description updated.

### Changed

- **Churn layer tracks per-author contributions** -- `git log` now runs with `--use-mailmap` and the `%at|%ae` format to record commit authors. Per-file `FileChurn.authors` map (`u32` → `AuthorContribution { commits, weighted_commits, first_commit_ts, last_commit_ts }`) references interned author emails in a shared pool on `ChurnResult` to keep the bitcode cache compact. Cache schema version bumped to 2; older caches are automatically rejected on load.

## [2.37.0] - 2026-04-15

### Added

- **Nx plugin now extracts `styles`, `scripts`, and `browser` entry points from `project.json`** -- Nx projects using the `@angular/build:application` executor declare global stylesheets (`styles: ["src/styles.scss"]`), polyfills/analytics scripts (`scripts: [...]`), and the Angular 17+ `browser` entry (replaces `main`) in `project.json`. Previously only `main` and `tsConfig` were picked up, so `src/styles.scss` and similar files were falsely reported as unused. Mirrors the existing Angular plugin's extraction of the equivalent `angular.json` fields. `{projectRoot}` / `{workspaceRoot}` tokens are expanded for these fields too. (Follow-up on [#125](https://github.com/fallow-rs/fallow/issues/125))

### Fixed

- **`ignorePatterns` now excludes workspace `package.json` discovery** -- `.fallowrc.json` with `"ignorePatterns": ["**/dist/**"]` correctly excludes source files from analysis but previously still scanned `dist/package.json` (build artifacts from ng-packagr, tsc, Rollup) for unused-/unlisted-dependency detection, producing a false positive for every dependency listed there. Workspace `package.json` discovery now consults the same glob filter used by the source walker. ([#124](https://github.com/fallow-rs/fallow/issues/124))
- **SCSS `@import` / `@use` bare specifiers now resolve from `node_modules/`** -- `@import 'bootstrap/scss/functions'` previously failed with `./bootstrap/scss/functions` unresolved because the resolver searched only the importing file's directory and framework-supplied `includePaths`. Matches Sass's own resolution: walks up from the importer to the filesystem root, probing each ancestor's `node_modules/` for `<pkg>/<path>.{scss,sass,css}`, partial convention (`_<name>.{scss,sass}`), and directory index (`<path>/{_index,index}.{scss,sass}`). Handles both SCSS partials (`bootstrap/scss/functions` → `_functions.scss`) and CSS fallback (`animate.css/animate.min` → `animate.min.css`). Matched packages are tracked as used so dependency analysis stays accurate. ([#125](https://github.com/fallow-rs/fallow/issues/125))
- **Angular CLI object-form `styles`/`scripts`/`polyfills` entries now extracted** -- the Angular workspace schema allows two forms per entry: the shorthand `"src/styles.scss"` and the object form `{ "input": "src/theme.scss", "bundleName": "theme", "inject": false }` (used for vendor stylesheets that opt out of auto-injection). The shared `expression_to_string_or_array` config-parser helper previously silently dropped object-form array elements, causing false-positive "unused file" reports for any file referenced that way. Both the Angular (`angular.json`) and Nx (`project.json`) plugins benefit. ([#126](https://github.com/fallow-rs/fallow/issues/126))

## [2.36.0] - 2026-04-14

### Added

- **`fallow config` subcommand** -- prints the loaded config path on the first line followed by the JSON-serialized config (with `extends` resolved). Use `--path` to print only the path (scriptable). Exit code 0 if a config was found, 3 if only defaults are in effect. Mirrors `eslint --print-config` and `dprint output-resolved-config`.
- **`loaded config: <path>` line on stderr** -- the CLI now prints which config file was picked up at the start of every human-format run (suppressed by `--quiet` and non-human formats). Closes the silent-config-loss UX gap where users could not tell whether their `.fallowrc.json` was being applied. The LSP emits the same information as a `window/logMessage` per project root, visible in the editor's Output panel.
- **`sealed: true` config field** -- marks a config as self-contained so `extends` paths must be file-relative and resolve within the config's own directory. `npm:` and `https:` extends are rejected. Useful for library publishers and monorepo sub-packages that want to guarantee their config cannot be modified by ancestor configs being injected via `extends`.

### Fixed

- **Config discovery now walks past `package.json` in monorepos** -- config search previously stopped at the first `package.json`, silently preventing monorepo sub-packages (pnpm/npm/yarn workspaces, Nx) from inheriting the root `.fallowrc.json`. Discovery now stops only at VCS boundaries (`.git`, `.hg`, `.svn`), matching Prettier/ESLint/Biome behavior. This fixes silent config loss in the VS Code extension, LSP, and CLI when invoked from a workspace sub-package. A sub-package with its own `.fallowrc.json` still wins (first-match-wins). ([#113](https://github.com/fallow-rs/fallow/issues/113))

## [2.35.0] - 2026-04-14

### Added

- **Severity tiers on health findings** -- each complexity finding now has a `severity` field (`moderate`, `high`, `critical`) based on how far above thresholds. Default thresholds: cognitive 25/40, cyclomatic 30/50 (asymmetric because cognitive maps more directly to comprehension difficulty). Summary includes `severity_critical_count`, `severity_high_count`, `severity_moderate_count`. Human output shows `CRITICAL` and `HIGH` badges on findings. All 6 output formats updated. ([#119](https://github.com/fallow-rs/fallow/issues/119))
- **`--min-severity` CLI flag** -- only exit with error for findings at or above the specified severity level (`moderate`, `high`, `critical`). Use `--min-severity critical` to ignore moderate/high findings in CI.

### Fixed

- **Maintainability Index formula distorted for small files** -- the MI formula used `complexity_density = cyclomatic / LOC` as its dominant term, causing a 5-line type-guard function to score worse than a 192-line function with 57 cyclomatic complexity. Added a LOC dampening factor `min(lines/50, 1.0)` that proportionally reduces the density weight for files under 50 lines. `SNAPSHOT_SCHEMA_VERSION` bumped to 7. ([#118](https://github.com/fallow-rs/fallow/issues/118))
- **Integration test fixture for bin-script dependency resolution** tracked in git.

## [2.34.0] - 2026-04-14

### Added

- **Per-file LOC in health output** -- surfaces lines of code per file in health analysis, enabling size-aware codebase navigation.
- **Total LOC vital sign** -- adds total lines of code as a health vital sign with penalty breakdown visibility.

### Changed

- **Significant performance improvements** -- parallel file walker via `ignore` crate, O(1) export name dedup, O(n^2) elimination in analyze/cross-reference/families, shared parsed modules and analysis graph between check and health in combined mode, borrowed line offsets in complexity computation.

### Fixed

- **CLI binary names in scripts resolved via dependency `bin` fields** -- packages whose binary name diverges from the package name (e.g., `@arethetypeswrong/cli` provides `attw`) are no longer falsely flagged as unused dependencies when used in npm scripts. Builds a reverse map from dependency `package.json` `bin` entries, probing `node_modules/` at project root and workspace roots. ([#107](https://github.com/fallow-rs/fallow/discussions/107))
- **Dynamic import `.then()` member access tracked** -- `import('./Foo').then(m => m.Bar)` now correctly credits `Bar` as a used export, preventing false unused-export reports for lazy-loaded named exports. ([#115](https://github.com/fallow-rs/fallow/issues/115))
- **Nx workspace tokens expanded in all project.json fields** -- `{projectRoot}` and `{workspaceRoot}` tokens are now expanded in `main` and `tsConfig` fields, not just `includePaths`. ([#114](https://github.com/fallow-rs/fallow/issues/114))
- **Deterministic clone family sort** -- added tiebreaker to clone family ordering to prevent non-deterministic output.

## [2.33.0] - 2026-04-13

### Added

- **`ignoreImports` config option** -- excludes ES `import` declarations from clone detection, reducing noise from sorted import blocks that naturally look similar across files. Available as `ignoreImports: true` in the `duplicates` config section and `--ignore-imports` CLI flag on the `dupes` command. Only affects ES import statements; CommonJS `require()` calls and re-exports are unaffected. Default: `false`. ([#113](https://github.com/fallow-rs/fallow/issues/113))

### Fixed

- **`fallow audit` now respects project duplication config** -- the `audit` command previously hardcoded default values for duplication settings (`mode`, `minTokens`, `minLines`, `skipLocal`, `crossLanguage`), ignoring user configuration. Now reads from the project config file, matching the behavior of `fallow` (combined mode) and `fallow dupes`.
- **Hardened CI workflow permissions** -- tightened GitHub Actions permissions for OpenSSF Scorecard compliance.

## [2.32.1] - 2026-04-13

### Fixed

- **Test-only dependency detection now recognizes workspace-level config files** -- after #111 narrowed config file exclusion patterns to root-only, `find_test_only_dependencies` no longer matched nested config files like `packages/foo/vitest.config.ts`. Now uses the curated `is_config_file()` predicate alongside glob patterns, correctly identifying workspace-level tooling configs without false-matching application configs like `app.config.ts`. ([#112](https://github.com/fallow-rs/fallow/issues/112))

## [2.32.0] - 2026-04-13

### Added

- **Stale suppression detection** -- detects orphaned `// fallow-ignore-next-line` and `// fallow-ignore-file` comments that no longer match any active issue. Implemented via a `SuppressionContext` that tracks which suppressions are consumed by detectors during analysis. Default severity: `warn` (reports without failing CI). Non-core suppression kinds (complexity, feature flags, code duplication, dependency-level) are excluded from stale detection to avoid false positives. New `--stale-suppressions` CLI flag to filter output to stale findings only. Full support across all 6 output formats, LSP diagnostics (`HINT` severity with `UNNECESSARY` tag), MCP server, GitHub Action annotations, and GitLab CI summaries.
- **`@expected-unused` JSDoc tag** -- marks an export as intentionally unused. Unlike `@public` (permanent exemption), `@expected-unused` reports a stale suppression when the export becomes used (gains references from reachable modules). Useful for API completeness patterns where exports exist for symmetry but have no internal consumers yet.
- **Structured suppression origin in JSON output** -- stale suppression findings include a discriminated `origin` field (`"type": "comment"` with `issue_kind` and `is_file_level`, or `"type": "jsdoc_tag"` with `export_name`) for machine consumers to distinguish inline comments from JSDoc tags.

### Fixed

- **Health test no longer fails with global git signing config** -- isolated temp repo git operations from global config (`GIT_CONFIG_GLOBAL=/dev/null`) to prevent commit signing requirements from breaking the `--changed-since` integration test.

## [2.31.0] - 2026-04-13

### Added

- **5 new framework plugins** -- Hardhat, UnoCSS, Qwik, Convex, and pnpm are now detected automatically (85 to 90 total plugins).
- **LSP binary distributed via npm** -- `fallow-lsp` is now included in the npm package alongside `fallow` and `fallow-mcp`. The VS Code extension resolves the LSP binary from `node_modules/.bin/fallow-lsp` without a separate install step.

### Fixed

- **`production: true` no longer excludes Angular `app.config.ts`** -- the `**/*.config.*` production exclude pattern was too broad, matching Angular's `src/app/app.config.ts` (a runtime application file) and breaking the entire import chain. Narrowed to `*.config.*` (root-anchored) with `literal_separator(true)` so nested config files in `src/` are preserved. Also added `app.config.ts` and `app.config.server.ts` to the Angular plugin's `always_used` list as defense-in-depth. ([#111](https://github.com/fallow-rs/fallow/issues/111))
- **Health test no longer fails with global git signing config** -- isolated temp repo git operations from global config (`GIT_CONFIG_GLOBAL=/dev/null`) to prevent commit signing requirements from breaking the `--changed-since` integration test.

## [2.30.0] - 2026-04-12

### Added

- **Arrow-wrapped dynamic import detection** -- `React.lazy(() => import('./Foo'))`, `loadable(() => import('./Component'))`, `defineAsyncComponent(() => import('./View'))`, and any function call with an arrow or function expression argument whose body is a sole `import()` expression now credits the target module's default export as used. Handles expression body, block body with return, and function expressions. Covers all lazy loading wrappers generically without maintaining a list of known functions.
- **JSDoc visibility tags** (`@internal`, `@beta`, `@alpha`) -- extends the existing `@public` tag support with three additional TSDoc lifecycle tags. All four tags suppress unused-export detection while preserving semantic distinction via a `VisibilityTag` enum (`Public`, `Internal`, `Alpha`, `Beta`). Priority when multiple tags present: Public > Internal > Alpha > Beta. Stored across the full pipeline (extraction, cache, graph, analysis). `#[repr(u8)]` with explicit discriminants for stable cache encoding.
- **`--include-entry-exports` flag** -- makes entry file exports (package.json main/module/exports, framework convention files) subject to unused-export detection instead of automatically marking them as used. Catches typos in framework exports (e.g., `meatdata` instead of `metadata` in Next.js pages). Still respects plugin `used_exports` rules for partial checking.
- **`--file <PATH>` flag** -- scopes check output to specified file(s) for lint-staged pre-commit integration. Accepts multiple paths. Full graph still builds (correct results require cross-file analysis), but only issues in matching files are reported. Dependency-level issues are suppressed in file mode. Warns when a path is not found (showing the resolved path).
- **Script multiplexer support** -- `concurrently`, `npm-run-all`, `npm-run-all2`, `run-s`, `run-p`, `run-s2`, `run-p2` are recognized as script multiplexers. Their positional arguments are treated as script names (not binary dependencies), preventing false "unused dependency" reports for packages referenced only via `npm:` prefixed script delegation.

### Fixed

- **Arrow-wrapped dynamic imports now credit default export** -- `ImportedName::Named("default")` from the arrow-wrapped import extractor is now correctly resolved to `ImportedName::Default`, so lazy-loaded modules have their default export credited in the module graph. Previously, the default export appeared unused despite the file being reachable.

## [2.29.1] - 2026-04-12

### Fixed

- **Feature flag inline suppression now works** -- `// fallow-ignore-next-line feature-flag` and `// fallow-ignore-file feature-flag` now suppress individual or all feature flag findings in the `fallow flags` command. Previously, the JSON output suggested this comment but the command never checked the suppression system, so the comment was silently ignored. Both the built-in detection loop and the custom SDK/env-prefix detection loop now call `is_suppressed()` / `is_file_suppressed()` with `IssueKind::FeatureFlag`.

## [2.29.0] - 2026-04-12

### Added

- **Inline suppression for health complexity findings** -- `// fallow-ignore-next-line complexity` and `// fallow-ignore-file complexity` now suppress individual functions or entire files from the health command's complexity findings. The blanket `// fallow-ignore-next-line` (without a kind) also works. Useful for intentionally complex code like generated parsers or configuration dispatch functions that you don't want to refactor but don't want cluttering health output. File scores, vital signs, and hotspot rankings are unaffected by suppressions since they reflect actual complexity, not alerting. The `IssueKind::Complexity` variant (discriminant 18) integrates with the existing suppression system used by all other issue types. Human output now shows a suppress hint in the findings footer when 3+ functions exceed thresholds. ([#108](https://github.com/fallow-rs/fallow/issues/108))
- **Standalone plugin schema** -- `fallow config-schema --plugins` emits a JSON schema describing all built-in plugin names, useful for editor autocompletion in config files. ([#109](https://github.com/fallow-rs/fallow/pull/109))

## [2.28.2] - 2026-04-11

### Fixed

- **`` html`...` `` tagged template literal asset references** -- `<script src="...">` and `<link rel="stylesheet|modulepreload" href="...">` inside the `html` tagged template literal (used by [Hono](https://hono.dev) via `hono/html`, [lit-html](https://lit.dev), and [htm](https://github.com/developit/htm)) now emit `SideEffect` imports via a new `visit_tagged_template_expression` override, mirroring the JSX `<script src>` / `<link href>` fix shipped in 2.28.1. The bare identifier tag `html` is matched only — `css`, `sql`, `gql`, `styled.div`, and member or call expressions are deliberately ignored so unrelated tagged templates in the same file are never misread as HTML. Each template quasi is scanned independently with the same regex pipeline used by the HTML file parser (extracted into a shared `collect_asset_refs` helper), so an asset reference split across an interpolation boundary (`` html`<script src="${base}/app.js">` ``) is skipped rather than producing a garbled, unresolvable specifier. Addresses till's follow-up comment on [#105](https://github.com/fallow-rs/fallow/issues/105) where `.ts` Hono layouts using the `html` tagged template (rather than JSX) still flagged sibling `static/*.js` files as unused.

## [2.28.1] - 2026-04-11

### Fixed

- **JSDoc `import()` type references now tracked as type-only imports** -- types referenced only from JSDoc type annotations (e.g., `@param foo {import('./types.js').Foo}`, `@returns`, `@type`, `@typedef`, `@callback`) are no longer flagged as unused exports. A new comment scanner runs alongside the existing `@public` tag extractor in `parse_source_to_module`, pushing synthesized `ImportInfo` entries with `is_type_only: true` and empty `local_name` so the module graph treats the referenced exports as used and the referenced files as reachable. Covers all JSDoc tag contexts, union annotations (`{import('./a').A | import('./b').B}`), nested member access, bare specifiers, parent-relative paths, and side-effect-only `import('./types')` without a `.Member`. Only `/** */` JSDoc blocks are scanned; single-star `/* */` comments are ignored. Addresses the first gap raised by till on [#105](https://github.com/fallow-rs/fallow/issues/105).
- **JSX `<script src>` / `<link href>` asset references inside JSX/TSX files** -- lowercase intrinsic `<script src="...">` and `<link rel="stylesheet|modulepreload" href="...">` inside JSX/TSX now emit `SideEffect` imports via a new `visit_jsx_opening_element` override, mirroring the HTML parser at the JSX level. Needed for SSR frameworks like [Hono](https://hono.dev) where layout components emit HTML via JSX and assets are referenced as plain string literals. Only `StringLiteral` attribute values are captured; `{expr}` containers and capitalized React-style components (`<Script>`, `<Link>`) are intentionally skipped. The web-root-relative resolver branch is also broadened from HTML-only to cover `jsx|tsx|js|ts|mjs|cjs|mts|cts` sources so `<link href="/static/style.css" />` resolves against the source file's parent directory first and the project root second. Addresses the second gap raised by till on [#105](https://github.com/fallow-rs/fallow/issues/105).

## [2.28.0] - 2026-04-11

### Added

- **Configurable class member allowlist** -- new top-level `usedClassMembers` config field and per-plugin `usedClassMembers` field on external plugins extend the built-in Angular/React lifecycle allowlist with framework-invoked method names. Useful for libraries that call consumer methods reflectively through an interface contract: ag-Grid's `agInit`/`refresh`, TypeORM's `MigrationInterface.up`/`down`, Web Components' `connectedCallback`/`disconnectedCallback`/`attributeChangedCallback`, and any strategy/plugin pattern. The allowlist is merged with plugin contributions during analysis and scoped to class methods and properties, so enum members with matching names are still flagged. ([#98](https://github.com/fallow-rs/fallow/issues/98))
- **Angular `stylePreprocessorOptions.includePaths`** -- the Angular plugin now reads `projects.*.architect.build.options.stylePreprocessorOptions.includePaths` from `angular.json`, and the Nx plugin reads the same key from `project.json`. Bare SCSS `@import 'variables'` / `@use 'mixins'` specifiers that fail file-local resolution retry against each configured include directory using the SCSS partial and directory-index conventions. Eliminates false unresolved-imports and false unused-files for Angular and Nx projects that share SCSS partials through `includePaths`. Parent-relative specifiers (`../shared/vars`) are left untouched so they cannot be silently redirected. ([#103](https://github.com/fallow-rs/fallow/issues/103))

### Fixed

- **Self-referencing and cross-workspace package imports in monorepo libraries** -- when a workspace library uses its own name in import specifiers (a [Node.js v12+ feature](https://nodejs.org/api/packages.html#self-referencing-a-package-using-its-name) commonly used by Angular libraries built with `ng-packagr`), secondary entry point files are no longer reported as unused. A new workspace package fallback strips the matching package name prefix from any bare specifier whose package name is in the workspace registry and resolves the remainder against the library's source tree via `oxc_resolver::resolve_file`, bypassing the `package.json` `exports` map's pointers at compiled output. The fallback also covers cross-workspace imports in monorepos that haven't run `npm install`, where bare `@org/pkg/sub` specifiers would previously fall through to `NpmPackage` classification and create false-positive unused-file and unlisted-dependency reports. ([#106](https://github.com/fallow-rs/fallow/issues/106))
- **SCSS `sass:*` built-in modules no longer reported as unlisted npm dependencies** -- the Sass compiler's [built-in modules](https://sass-lang.com/documentation/modules/) imported via `@use 'sass:string'`, `@use 'sass:math'`, etc. are now recognized alongside the existing `node:`, `bun:`, `cloudflare:`, and Deno `std` platform builtins. These are language-level Sass features, not npm packages, and cannot appear in `package.json`. Previously, every unique `sass:*` module used across a codebase produced a separate false-positive unlisted dependency report (up to 7 per Sass codebase for the full module set: `math`, `string`, `color`, `list`, `map`, `meta`, `selector`). The prefix guard is strict: `sass-loader`, `@types/sass`, and the `sass` compiler package itself remain normal npm packages. ([#104](https://github.com/fallow-rs/fallow/issues/104))
- **Entry points inside ignored output directories now fall back to source index** -- when a `package.json` `main`/`module`/`exports` field resolves to a file inside an ignored output directory (`dist`, `build`, `out`, `esm`, `cjs`) and no direct source mirror exists in `src/`, fallow now probes `src/index.*`, `src/main.*`, `index.*`, `main.*` in the package root. Prevents TypeScript libraries whose compiled entry has no one-to-one source mapping (e.g. `dist/esm2022/index.js` with only `src/index.ts`) from having their entire source tree marked unreachable. Emits a single `tracing::info!` per fallback so the heuristic is visible in logs. ([#102](https://github.com/fallow-rs/fallow/issues/102))

## [2.27.6] - 2026-04-11

### Fixed

- **Bare HTML / Vue / Svelte asset references without `./` prefix** -- HTML `<script src="app.js">`, `<link rel="stylesheet" href="styles.css">`, `<link rel="modulepreload" href="vendor.js">`, and Vue/Svelte `<script src="logic.ts">` now normalize bare filenames to `./`-prefixed relative paths before downstream resolution. Previously these were reported as unlisted npm dependencies because the resolver's specifier classifier treats anything without `./`, `/`, or `://` as a bare package. Browsers, Vite, Parcel, and the Vue/Svelte compilers all resolve these references relative to the document or component file whether or not the `./` prefix is present, so normalization matches real-world semantics. Scoped package paths (`@scope/header.html`), root-absolute paths (`/src/main.ts`), URLs, and `data:` URIs remain untouched. Same bug shape as the Angular `templateUrl` fix shipped in 2.27.5. ([#101](https://github.com/fallow-rs/fallow/pull/101))

## [2.27.5] - 2026-04-11

### Fixed

- **Import-then-reexport pattern recognized as re-export** -- the pattern `import { X } from './a'; export { X };` is now detected as a re-export (semantically equivalent to `export { X } from './a';`) instead of a local export. The visitor scans previously-seen imports when processing an export without a `from` clause and emits a `ReExportInfo` pointing back to the original source. Eliminates false duplicate-export findings (where the bottom export collided with the original) and false unused-export reports (where the re-export chain propagation never reached the misclassified entry). Handles renames on either side, default imports, and mixed local + re-export statements. Order-sensitive: relies on imports preceding exports in source order.
- **Accurate line numbers for unused re-exports** -- re-exports synthesized into `ExportSymbol` entries from `ReExportInfo` were reported with line `:1` because the synthesis used `Span::new(0, 0)` as a sentinel. The visitor's real source span is now plumbed through `ReExportInfo` and `ReExportEdge` into the synthesized `ExportSymbol`, so unused re-exports now report the actual line and column of the `export {...}` declaration. The `(re-export)` marker in human/JSON/SARIF/CodeClimate output is preserved via a semantic lookup against the module's `re_exports` list. On vue-core, 86 unused re-export entries went from `:1` to their real source positions.
- **Angular `templateUrl` / `styleUrl` without `./` prefix** -- bare filenames in Angular `@Component` decorators (e.g., `templateUrl: 'app.component.html'`) are now treated as relative paths instead of being misclassified as npm package specifiers. Angular resolves both `'app.component.html'` and `'./app.component.html'` relative to the component file, so the `./` prefix is optional. Without normalization, bare filenames were reported as unlisted dependencies. Already-relative, absolute, URL, and scoped-package paths are left untouched. ([#99](https://github.com/fallow-rs/fallow/issues/99), [#100](https://github.com/fallow-rs/fallow/pull/100))

### Changed

- **O(N) re-export detection** -- the new `is_re_export` lookup pre-builds an `FxHashSet` of re-exported names per module, replacing a per-export `iter().any()` scan. Avoids O(N²) work on barrel files with many re-exports.

## [2.27.4] - 2026-04-10

### Fixed

- **Broken tsconfig chain no longer poisons sibling resolution** -- a solution-style `tsconfig.json` with `references` to a sibling tsconfig that has a broken `extends` chain (e.g., extending a non-existent file, as commonly scaffolded by Angular CLI / Nx workspaces) no longer fails import resolution for files covered by a healthy sibling. Previously, a broken `tsconfig.spec.json` (covering only `*.spec.ts`) caused every relative import in the workspace to be flagged as unresolved, even in files covered by a valid `tsconfig.app.json`. Fallow now retries resolution without tsconfig context when it detects a tsconfig-loading error (`TsconfigNotFound`, `TsconfigCircularExtend`, `TsconfigSelfReference`, malformed JSON, IO error), and emits a single `tracing::warn!` per unique broken chain so the configuration issue is visible instead of silent. Path aliases in the broken workspace remain unresolvable (they were already broken in `tsc`). ([#97](https://github.com/fallow-rs/fallow/issues/97))
- **VS Code extension packaging** -- the root `LICENSE` file is now copied into `editors/vscode/` at package time so it is bundled in the VSIX. `vsce` only reads the extension's working directory, so the repo-root `LICENSE` was previously not included in the published extension.

### Changed

- **VS Code extension size reduced 48%** -- `.vscodeignore` now excludes `pnpm-lock.yaml`, `.fallow/`, `.test-dist/`, `test/`, `vitest.config.mts`, `tsconfig.test.json`, and `.fallowrc.json`. The packaged VSIX is 100 KB (down from 192 KB).
- **CSS path alias regression tests** -- added coverage for `@/components/Button.css` sharing the `@`-prefix with scoped packages, ensuring path aliases stay as bare specifiers for the resolver's alias fallback to handle.

## [2.27.3] - 2026-04-10

### Fixed

- **CSS scoped package imports** -- `@import` statements referencing scoped npm packages with CSS extensions (e.g., `@fontsource/monaspace-neon/400.css`, `@fallow/design-system/styles.css`) are now kept as bare specifiers for node_modules resolution. Previously, the extension-based heuristic incorrectly prepended `./`, causing all such imports to be reported as unresolved.
- **HTML root-relative paths in workspace members** -- `<script src="/src/main.tsx">` in workspace member HTML files (e.g., `site/index.html`) now resolves relative to the workspace member root, not the monorepo root. Previously, `site/index.html` referencing `/src/main.tsx` looked for `<monorepo>/src/main.tsx` instead of `site/src/main.tsx`, leaving the entry point file and all its transitively reachable modules flagged as unused. Resolution now tries the HTML file's parent directory first, with fallback to the project root for the `public/index.html` pattern.
- **SCSS directory index convention** -- `@use 'components'` now resolves to `components/_index.scss` or `components/index.scss` per SCSS directory-import convention, in addition to the existing `_components.scss` partial fallback.
- **`export default class extends Foo` inheritance** -- default-exported classes with an `extends` clause now participate in the inheritance-aware class member detection introduced in 2.27.2, preventing false unused-member reports on child class overrides.

## [2.27.2] - 2026-04-10

### Fixed

- **SCSS partial resolution** -- `@use 'variables'` and `@import '../styles/variables'` now resolve to `_variables.scss` per SCSS convention. Extensionless bare specifiers in `.scss`/`.sass` files are normalized to relative paths, and the resolver tries the underscore-prefix fallback before npm package classification. Eliminates cascading false positives across unresolved imports, unlisted dependencies, and unused files. ([#95](https://github.com/fallow-rs/fallow/issues/95))
- **Inheritance-aware class member detection** -- child class members accessed via `this.*` in parent class methods are no longer flagged as unused. Builds an inheritance map from `extends` clauses and propagates member accesses bidirectionally through the hierarchy. Reduces false positives from 7 to 1 in typical polymorphic dispatch patterns (the remaining case requires type inference). Also handles `export default class extends Foo` patterns. ([#96](https://github.com/fallow-rs/fallow/issues/96))

## [2.27.1] - 2026-04-10

### Fixed

- **Angular `@defer` block scanning** -- `@defer (when condition)` expressions are now parsed for member references, including `prefetch when` and `hydrate when` variants. Previously, members referenced only in `@defer` conditions were flagged as unused. ([#94](https://github.com/fallow-rs/fallow/issues/94))
- **Angular `@let` template variables** -- `@let name = expr;` template-local variable declarations are now parsed, extracting member references from the expression and tracking the variable as a template local. Previously, members referenced only in `@let` expressions were flagged as unused. ([#93](https://github.com/fallow-rs/fallow/issues/93))
- **Angular `@else if` condition scanning** -- `@else if (condition)` expressions are now parsed for member references. Previously, the condition was skipped and members referenced only in `@else if` were flagged as unused.
- **Angular `outputFromObservable()` recognition** -- class properties initialized with `outputFromObservable()` (from `@angular/core/rxjs-interop`) are now recognized as framework-managed, matching the existing `output()` handling. ([#92](https://github.com/fallow-rs/fallow/issues/92))

## [2.27.0] - 2026-04-10

### Added

- **Feature flag detection** -- new `fallow flags` command detects static feature flags and environment gates in JavaScript/TypeScript code. Three detection categories: boolean flags (`if (FLAG)`), environment checks (`process.env.NODE_ENV`), and config-key guards (`config.featureX`). Cross-references with dead code analysis to surface flags guarding unused code. All 6 output formats supported (human, JSON, compact, SARIF, Markdown, CodeClimate). Available as MCP tool for AI agent integration.
- **Deep Docusaurus convention coverage** -- the Docusaurus plugin now parses `docusaurus.config.*` to discover content plugin instances (docs, blog, pages), classic preset expansion, theme swizzle components, i18n/versioned docs, static directories, client modules, scripts, and stylesheets. Sidebar files are only marked as used when explicitly referenced via `sidebarPath`. Contributed by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza). ([#91](https://github.com/fallow-rs/fallow/pull/91))

### Changed

- **Plugin infrastructure** -- 9 plugins migrated from manual `impl Plugin` to the `define_plugin!` macro, reducing boilerplate. Shared config parser helpers extracted for reuse across plugins (function body extraction for async config creators, property accessors, disabled-section detection).
- **MCP server** -- shared CLI arg-building helpers extracted, reducing duplication across tool implementations.
- **Internal visibility** -- core crate modules restricted to `pub(crate)` where cross-crate access was unnecessary.
- **SFC template parser** -- deduplicated template tag parsing logic shared between Vue and Svelte extractors.

### Fixed

- **VS Code extension** -- version marker round-trip handling improved with additional test coverage.

## [2.26.1] - 2026-04-09

### Changed

- **Health: churn disk cache and parallel git log** -- churn analysis results are now cached to disk and git log calls run in parallel, significantly reducing health command runtime on repos with long histories. New `--performance` flag prints timing breakdown per analysis phase.

### Fixed

- **VS Code extension: stale LSP binary after extension update** -- the version check that detects outdated auto-downloaded binaries silently skipped re-downloading when `--version` output could not be parsed, leaving a stale binary in place. Now writes a `.fallow-version` marker file alongside downloaded binaries and treats unknown-version binaries as stale. Also warns when a PATH-resolved binary is outdated relative to the extension. ([#90](https://github.com/fallow-rs/fallow/issues/90))
- **GitHub Action: multiple JSON objects in bare invocation** -- `analyze.sh` now handles bare invocations that produce multiple JSON objects instead of failing to parse.

## [2.26.0] - 2026-04-09

### Added

- **Expo Router plugin** -- new `expo-router` plugin (85th built-in) detects Expo Router projects, reads the configured route root from `app.json` / `app.config.ts`, and marks route files plus special-file exports as framework-used. Covers `+api` (HTTP methods), `+middleware`, `+html`, `+not-found`, and `+native-intent` conventions. Contributed by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza). ([#88](https://github.com/fallow-rs/fallow/pull/88))
- **TanStack Router config parsing** -- the `tanstack-router` plugin now reads `tsr.config.json` to support custom route directories (`routesDirectory`), route file prefixes (`routeFilePrefix`), ignore patterns (`routeFileIgnorePrefix`, `routeFileIgnorePattern`), generated route tree location (`generatedRouteTree`), and lazy route files (`.lazy.tsx`). Config-driven routes replace static defaults instead of layering. Contributed by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza). ([#88](https://github.com/fallow-rs/fallow/pull/88))
- **Path rule exclusions** -- plugin entry-point and used-export rules now support glob, regex, and segment-regex exclusions for strict file matching. Prevents false negatives from overly broad patterns.

### Fixed

- **GitHub Action: fork PR CI failures** -- the `analyze.sh` script inherited `set -e` (errexit) from the composite action runner shell, causing silent failures on fork PRs where intermediate commands (git diff, grep) return non-zero as part of normal flow.
- **LSP: workspace diagnostic merging** -- `test_only_dependencies`, `boundary_violations`, and `export_usages` are now correctly merged across workspace roots instead of being dropped.

## [2.25.1] - 2026-04-09

### Fixed

- **VS Code extension: duplicate diagnostics in multi-root workspaces** -- diagnostics were shown N times (once per workspace root) when hovering over issues. Fixed by building diagnostics once from the merged results instead of per-root. ([#90](https://github.com/fallow-rs/fallow/issues/90))

## [2.25.0] - 2026-04-09

### Added

- **Health: large function drill-down and duplication penalty** -- `fallow health` now shows the largest functions contributing to unit size risk and applies a score penalty when code duplication is detected. Progressive disclosure hints guide users to relevant flags (`--top`, duplication timing).
- **SIG-aligned clippy lints and module coupling CI** -- added `too_many_lines` (150 LOC), `too_many_arguments` (7 params), and `cognitive_complexity` (25) thresholds aligned with SIG maintainability properties. New CI workflow tracks inter-module coupling metrics.

### Fixed

- **VS Code extension: raw ANSI escape codes in Output panel** -- the LSP server's tracing subscriber now disables ANSI color codes, matching the pattern already used by the MCP server. VS Code's Output panel does not render ANSI sequences. ([#89](https://github.com/fallow-rs/fallow/issues/89))

## [2.24.0] - 2026-04-09

### Added

- **Function size and parameter risk profiles** -- `fallow health` now computes risk distribution histograms for function size (1-15 / 16-30 / 31-60 / >60 LOC) and parameter count (0-2 / 3-4 / 5-6 / >=7 params). Profiles appear in JSON as `unit_size_profile` and `unit_interfacing_profile` on `vital_signs`, and in human output when approaching penalty thresholds.
- **Parameter counting on functions** -- `FunctionComplexity` now includes `param_count` (excluding TypeScript's `this` parameter, including rest parameters). Available in health findings JSON output.
- **Coupling concentration metrics** -- `p95_fan_in` and `coupling_high_pct` added to vital signs, tracking the 95th percentile of incoming dependencies and the percentage of highly-coupled files.
- **Two new health score penalties** -- `unit_size` (max 10 points, activates when >5% of functions exceed 60 LOC) and `coupling` (max 5 points, activates when p95 fan-in exceeds 30). Conservatively calibrated: zero penalty on well-maintained projects.
- **Trend schema version migration warning** -- when comparing against a snapshot from an older schema version, the trend output warns that score deltas may reflect formula changes rather than code changes.

### Changed

- **Conditional risk profile rendering** -- function size profiles only appear in human output when the very-high-risk bin reaches 3% (approaching the 5% penalty threshold). Parameter profiles only appear when high or very-high bins carry signal. Healthy projects see clean, noise-free output.
- **Risk profiles visible during trend view** -- removed suppression of risk profiles when comparing against snapshots, so users can see absolute distributions alongside trend deltas.
- **Snapshot schema v4** -- added risk profiles and coupling fields. Old v3 snapshots load seamlessly via serde defaults.

### Fixed

- **Baseline key portability** -- baseline keys now use relative paths for cross-machine consistency.
- **Zero-match baseline warning** -- improved messaging when baselines match no current issues.
- **Baseline stats in combined JSON** -- unified baseline key helpers across check/health/dupes.

## [2.23.1] - 2026-04-09

### Fixed

- **VS Code extension: explicit `lspPath` setting now takes priority over local `node_modules`** -- the `fallow.lspPath` setting represents explicit user intent and should not be silently overridden by an implicitly discovered devDependency binary. Resolution order is now: `lspPath` > `node_modules/.bin` > PATH > global storage > auto-download.

## [2.23.0] - 2026-04-09

### Added

- **VS Code extension: local devDependency binary resolution** -- the extension now checks `node_modules/.bin/` in the workspace root for locally installed binaries. Teams can pin fallow as a devDependency for consistent versions across developers. ([#86](https://github.com/fallow-rs/fallow/discussions/86))

### Fixed

- **Infinite recursion on TypeScript type annotations in destructuring patterns** -- `collect_pattern_usage` now strips type annotations before traversing, preventing stack overflow on patterns like `({ href, content }: Props)` in Vue and Svelte templates.

## [2.22.4] - 2026-04-08

### Added

- **Nuxt module authoring support** -- projects using `@nuxt/kit` with `src/runtime/` directory now have components, composables, plugins, utils, locale, types, and vue subdirectories recognized as entry points. Tested on Nuxt UI v3: false positive unused files dropped from 521 to 226.

### Fixed

- **Stack overflow on Svelte 5 typed snippet parameters** -- `{#snippet Link({ href, content }: Props)}` caused infinite recursion in `extract_pattern_binding_names` because the TypeScript type annotation prevented destructuring pattern matching. Projects like shadcn-svelte now analyze without crashing.

### Changed

- **Duplicate detection in re-export chain resolution** -- switched from linear scan to `FxHashSet` for O(1) duplicate detection during star re-export propagation.
- **Reduced allocations in plugin system and graph construction** -- reuses scratch buffers and avoids unnecessary `String`/`Vec` allocations.

## [2.22.3] - 2026-04-08

### Fixed

- **aarch64-linux-musl release build failure** -- switched from vendored OpenSSL (`native-tls`) to `rustls` for TLS, eliminating all C dependencies for cross-compilation. Vendored OpenSSL linked against glibc symbols (`__memcpy_chk`) unavailable on musl targets.

## [2.22.2] - 2026-04-08

### Added

- **Meta-framework prepare step warning** -- warns when Nuxt, Next.js, SvelteKit, or Astro need a prepare/build step before analysis can see generated types and routes.

### Fixed

- **CI dupes comment showing "No code duplication found" with `--changed-since` active** -- duplication JSON output had absolute file paths instead of project-relative paths, causing the GitHub Action's changed-file filter to silently drop all clone groups. The same issue existed in combined and audit mode for both dupes and health sections. ([#85](https://github.com/fallow-rs/fallow/issues/85))
- **Star re-exports not propagated through entry point chains** -- `export * from './module'` in barrel files referenced by entry points now correctly marks the re-exported module's exports as used.
- **Wildcard subpath exports missing CSS/asset entry points** -- package.json `exports` maps with wildcards (`"./*": "./dist/*.js"`) now expand to discover CSS, SCSS, and asset entry points.
- **Astro `getStaticPaths` not recognized for TypeScript page files** -- `.ts` files in Astro `src/pages/` now have `getStaticPaths` marked as framework-used.
- **`.js` resolved before `.d.ts` when both exist side-by-side** -- prevents false unresolved imports when a `.js` implementation file coexists with a `.d.ts` declaration.
- **Test framework `include`/`testMatch`/`testRegex` patterns not used as entry point overrides** -- Vitest `test.include` and Jest `testMatch`/`testRegex` config patterns now correctly override the default test file discovery.
- **Case-insensitive HTML/SFC regex matching on non-ASCII** -- added `unicode-case` feature to regex crate for correct case-insensitive matching in HTML and Vue/Svelte template parsing.
- **TLS provider not configured for URL extends resolution** -- explicitly configured native-tls provider for ureq HTTP client, fixing `extends: "https://..."` config resolution.
- **OpenSSL missing on Linux musl/cross-compilation** -- added vendored OpenSSL for static linking on musl targets.

### Changed

- **Binary size reduced by 16.6%** (-1.38MB) via release profile optimizations.
- **Oxc parser upgraded from 0.123 to 0.124.**
- **Sub-packages discovered from package.json exports map** -- nested packages referenced via `exports` field subpath patterns are now resolved.

## [2.22.1] - 2026-04-08

### Fixed

- **Astro frontmatter extraction on Windows** -- the regex for extracting code between `---` delimiters required LF line endings, silently failing on Windows where git checks out CRLF. All `.astro` file exports were invisible on Windows. Now handles both LF and CRLF.

## [2.22.0] - 2026-04-08

### Added

- **Expanded Astro convention coverage** -- actions (`src/actions/index`), content config (`src/content/config`, `src/content.config`), nested middleware (`src/middleware/index`), page exports (`getStaticPaths`, `prerender`, `partial`), and endpoint HTTP method handlers (`GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD`, `OPTIONS`, `ALL`) are now recognized as framework-used. ([#84](https://github.com/fallow-rs/fallow/pull/84))
- **React Router v7 route config and root module exports** -- `app/routes.ts` default export, root `Layout`, `middleware`, `clientMiddleware`, and `shouldRevalidate` are now kept alive. ([#84](https://github.com/fallow-rs/fallow/pull/84))
- **Remix root and client data exports** -- `clientLoader`, `clientAction`, `shouldRevalidate`, `Layout`, and `HydrateFallback` on root and route modules are now recognized. ([#84](https://github.com/fallow-rs/fallow/pull/84))
- **Gatsby `config` page export and `src/api` function conventions** -- page-level `config` export (Gatsby 5 Slice API) and `src/api/**` serverless function exports (`default`, `config`) are now covered. ([#84](https://github.com/fallow-rs/fallow/pull/84))
- **VitePress `docs/` scaffold discovery** -- `.vitepress` added to hidden directory allowlist, and `docs/.vitepress/config` and `docs/.vitepress/theme/index` patterns added for the common docs-subdirectory layout. Theme entry `default` export is kept alive. ([#84](https://github.com/fallow-rs/fallow/pull/84))

## [2.21.0] - 2026-04-08

### Added

- **Istanbul match-rate stats** -- `istanbul_matched` and `istanbul_total` fields in JSON `summary` when using Istanbul coverage model. Human output footer shows function match count. Helps detect path mismatches or naming gaps in coverage data.
- **Coverage model in snapshots** -- `VitalSignsSnapshot` now persists `coverage_model` (schema v3). Trend output warns with a yellow note when comparing snapshots with different CRAP models, preventing misattribution of score changes.
- **CI auto-detection note** -- when `CI=true` is set and coverage data is auto-detected (not explicit `--coverage`), emits a note on stderr suggesting explicit `--coverage` for deterministic scores.
- **`--coverage-root` flag** -- rebases file paths in Istanbul coverage data by stripping a prefix and prepending the project root. Enables cross-environment coverage matching (CI runner paths vs local checkout). Also available as `coverage_root` MCP parameter.

## [2.20.0] - 2026-04-08

### Added

- **Tiered CRAP scoring with graph-based estimation** -- CRAP scores now use a per-function spectrum instead of per-file binary. Functions whose exports are directly referenced by tests get 85% estimated coverage, indirectly test-reachable functions get 40%, untested files get 0%. The real CRAP formula (CC^2 * (1-cov/100)^3 + CC) is applied with these estimates. Zero config, zero extra latency.
- **Istanbul coverage integration** -- new `--coverage <PATH>` flag (or `FALLOW_COVERAGE` env var) reads `coverage-final.json` from Jest, Vitest, c8, nyc for exact per-function CRAP scores. Auto-detects coverage data from `coverage/coverage-final.json` and `.nyc_output/coverage-final.json`.
- **`oxc_coverage_instrument` dependency** -- uses the published fallow-rs/oxc-coverage-instrument crate for parsing Istanbul-format coverage data.

### Changed

- **Default CRAP model is now `static_estimated`** -- replaces the binary `static_binary` model. The `coverage_model` field in JSON output reflects which model was used: `"static_estimated"` (default) or `"istanbul"` (when coverage data is available).
- **Updated `--explain` CRAP description** -- now documents the tiered estimation model and upgrade path to Istanbul coverage.
- **Updated human output footer** -- file scores footer now describes the estimation model and mentions `--coverage` for exact scores.

## [2.19.3] - 2026-04-08

### Fixed

- **`ignorePatterns` now correctly filters files during discovery** -- user-configured relative glob patterns (e.g. `src/api/generated/**`) were matched against absolute walker paths, so only patterns with `**/` prefix worked. Now strips the project root before matching, consistent with all other glob filters. ([#83](https://github.com/fallow-rs/fallow/issues/83))

## [2.19.2] - 2026-04-08

### Fixed

- **Angular implicit member reference detection** -- `@Component`/`@Directive` metadata extraction now covers inline `template:` strings, `host:` binding expressions, `inputs:`/`outputs:` arrays, and `queries:` metadata. Angular 17+ signal APIs (`input()`, `input.required()`, `output()`, `model()`, `viewChild()`, `viewChildren()`, `contentChild()`, `contentChildren()`) on Angular-decorated classes are treated as framework-managed, preventing false `unused-class-members` reports. ([#82](https://github.com/fallow-rs/fallow/issues/82))

### Changed

- **Module splits for maintainability** -- split `graph/build.rs` narrowing helpers into `graph/narrowing.rs` and `analyze/predicates.rs` into `predicates/{file,import,lifecycle}.rs` for better cohesion. No API changes.
- **Test coverage improvements** -- added 258 tests across 12 files, improving overall line coverage from 90.2% to 90.9%.

## [2.19.1] - 2026-04-07

### Fixed

- **Value/type declaration merging no longer flagged as `duplicate-export`** -- TypeScript allows a const and a type to share the same name (`export const Status = z.enum([...]); export type Status = z.infer<typeof Status>`). This idiomatic pattern (common with Zod, Prisma, and class+interface merging) was falsely reported as a duplicate export. Detection now tracks the type-only flag and skips groups that span both value and type namespaces. ([#81](https://github.com/fallow-rs/fallow/discussions/81))

## [2.19.0] - 2026-04-07

### Added

- **Angular external template tracking** -- `@Component({ templateUrl, styleUrl })` decorators now create graph edges to referenced template and style files, preventing false `unused-files` reports. HTML templates are scanned for Angular syntax (`{{ }}`, `[prop]`, `(event)`, `[(ngModel)]`, `*ngIf`, `*ngFor`, `@if`, `@for`, `@switch`, pipes) and referenced identifiers are bridged to the component's class members, preventing false `unused-class-members` reports. Covers both legacy structural directives and Angular 17+ control flow. ([#73](https://github.com/fallow-rs/fallow/issues/73))

### Fixed

- **Factory initializer instance tracking** -- `const [svc] = useState(() => new MyService())` and similar React hook patterns now correctly resolve the first array-destructured element as a class instance for member access tracking. Prevents false `unused-class-members` when the class is instantiated through a wrapper function. Handles arrow expression bodies, arrow block bodies, and function expressions. ([#79](https://github.com/fallow-rs/fallow/pull/79))
- **VS Code extension binary version skew** -- the auto-downloaded `fallow-lsp` binary in the extension's global storage was never refreshed after initial download, causing LSP diagnostics to diverge from the CLI version. Now checks the installed binary's version on activation and re-downloads when it doesn't match the extension version. ([#80](https://github.com/fallow-rs/fallow/discussions/80))

## [2.18.3] - 2026-04-07

### Fixed

- **`fallow fix` export specifier lists** -- `export { A, B, C }` lines are now fixed correctly by removing individual unused specifiers instead of stripping the `export` keyword (which left invalid `{ A, B, C }` syntax). Handles `export { ... }`, `export { ... } from "..."`, `export type { ... }`, and aliased specifiers. ([#74](https://github.com/fallow-rs/fallow/pull/74))
- **npm publish CI** -- restored two-step npm bootstrap (`npm@10.9.8` then `npm@latest`) for reliable OIDC trusted publishing on Node 22 runners.

## [2.18.2] - 2026-04-07

### Fixed

- **npm publish CI** -- pinned `npm@10` in the release workflow to avoid `promise-retry` module error on Node 22 runners that broke npm package publishing.

## [2.18.1] - 2026-04-07

### Changed

- **Cache serialization switched to bitcode** -- replaced bincode with bitcode for incremental cache serialization, yielding smaller cache files and faster round-trips.
- **Supply chain hardening** -- added OpenSSF Scorecard workflow, zizmor static analysis for GitHub Actions, Dependabot auto-merge for patch/minor dependency bumps, and pinned all CI action SHAs.
- **Miri moved into CI workflow** -- consolidated the standalone Miri workflow into the main CI pipeline for simpler maintenance.

### Fixed

- **Chained member access through class field assignments** -- `this.field.method()` chains where `this.field` is assigned a class instance are now correctly resolved, preventing false positive unused-export reports on the assigned class's methods. ([#63](https://github.com/fallow-rs/fallow/pull/63))
- **Workspace symlink setup idempotent** -- test workspace symlink creation no longer fails when the symlink already exists.

## [2.18.0] - 2026-04-07

### Added

- **`--score`, `--trend`, `--save-snapshot` on combined command** -- the bare `fallow` command (all analyses) now supports health score computation, snapshot saving, and trend comparison. Previously these flags were only available on the `health` subcommand. Enables the health delta header in CI PR/MR comments for combined runs.
- **Health delta header uses metric labels** -- the trend delta line now uses the metric's own label (e.g., "avg complexity" from the trend data) instead of hardcoded text, making the output more robust and descriptive.
- **First-run progressive disclosure** -- when `--score` is enabled but no snapshot exists for trend comparison, the PR comment shows a hint: "Enable `save-snapshot: true` to track score trends over time."
- **Fixture accuracy** -- test fixtures now include `crap_max` and `crap_above_threshold` fields on all `file_scores` entries, matching the actual CLI output schema.

### Fixed

- **GitLab CI `FALLOW_SCORE` and `FALLOW_TREND` wiring** -- both env vars now work for the combined (empty command) case, not just the `health` subcommand.
- **`signed(0)` formatting consistency** -- zero deltas now display as "0.0" instead of "0", consistent with non-zero values like "7.2".
- **GitLab test parity** -- dead export delta assertion (`(-3.8%)`) added to GitLab CI tests, matching the GitHub Action test coverage.
- **CI input descriptions** -- `action.yml` descriptions for `score`, `trend`, and `save-snapshot` now mention bare command support and snapshot storage location.

## [2.17.1] - 2026-04-07

### Added

- **GitHub Action `score` input** -- new `score: true` input enables the `--score` flag on the health command, adding a health delta header to PR summaries showing grade, score change, dead export drift, and complexity trends.
- **Health delta header in CI summaries** -- when `--score` is used, both GitHub Action and GitLab CI summaries display a one-line health score comparison against the previous snapshot.

### Fixed

- **Summary section links** -- all section headers (Code issues, Duplication, Complexity, Codebase health) and table metrics now link to their respective docs pages. Fixes broken markdown link in the Duplication `<summary>` tag.

## [2.17.0] - 2026-04-07

### Added

- **Nuxt runtime convention hardening** -- `plugins/`, `middleware/`, `server/middleware/`, and `components/` directories now have their default exports and named exports (`defineNuxtPlugin`, `defineNuxtRouteMiddleware`, `defineEventHandler`, `defineNuxtComponent`) treated as framework-used. Nested directory scanning for plugins, composables, and utils. Nuxt config `plugins`, `css`, and `modules` entries discovered as runtime entry points with path normalization. ([#60](https://github.com/fallow-rs/fallow/pull/60))
- **Vue SFC edge case hardening** -- `<script src="...">` external script references in Vue SFCs now generate graph edges. `<script>` blocks with `generic="T extends Foo<Bar>"` containing `>` in type parameters are parsed correctly. HTML comments in Vue templates are stripped before import extraction. ([#60](https://github.com/fallow-rs/fallow/pull/60))

### Fixed

- **HTML root-relative path resolution** -- root-relative paths in HTML files (e.g., `<script src="/src/main.tsx">`) are now resolved against the project root instead of being treated as absolute filesystem paths. This fixes false unresolved-import reports in Vite, Parcel, and similar dev server setups. ([#61](https://github.com/fallow-rs/fallow/pull/61))

## [2.16.0] - 2026-04-07

### Added

- **Untested complexity risk (CRAP) scores** -- per-function risk scoring combining cyclomatic complexity with static test reachability from the module graph. Binary model: test-reachable files score CC, untested files score CC^2+CC. Threshold at >=30 (CC>=5 untested), based on the CRAP metric (Savoia & Evans, 2007). Adds `crap_max` and `crap_above_threshold` fields to `FileHealthScore`, `coverage_model: "static_binary"` to `HealthSummary` and `TrendPoint`. Gated behind `--file-scores`. Extreme values (>999) capped in human output; JSON stays uncapped.
- **AddTestCoverage refactoring target** -- new recommendation category (Rule 7) fires when a file has 2+ functions above the CRAP threshold and complexity density > 0.3. Contributing factor `crap_max` appears on targets with high untested risk. Evidence links to the top complex functions.
- **SvelteKit `$types` route group parentheses coverage** -- route groups with parentheses in path segments are now handled correctly for `$types` imports. ([#54](https://github.com/fallow-rs/fallow/pull/54))

### Changed

- **Lazy complexity computation** -- cyclomatic/cognitive complexity is now computed on-demand rather than eagerly, reducing parse time for pipelines that don't need it.
- **Packed ModuleNode flags** -- boolean flags on `ModuleNode` packed into a bitfield for tighter memory layout.
- **Smarter BFS reachability** -- reachability analysis uses optimized graph traversal.
- **Duplicate detection improvements** -- faster clone detection with reduced memory allocation.

### Fixed

- **Windows path separator normalization** -- health coverage-gaps tests now normalize backslashes for Windows CI compatibility.
- **GitHub Action fork PR handling** -- PR comment tests are skipped on fork PRs where write permissions are unavailable.

## [2.15.0] - 2026-04-06

### Added

- **Next.js convention export tracking** -- App Router special files (`loading`, `error`, `not-found`, `template`, `default`, `global-error`, `forbidden`, `unauthorized`, `global-not-found`) and route segment config exports (`revalidate`, `dynamic`, `runtime`, `fetchCache`, `preferredRegion`, `maxDuration`, `viewport`, `generateViewport`) are now treated as framework-used. Pages Router `_app` (`reportWebVitals`), API routes (`config`), `middleware`, `proxy`, `instrumentation` (`register`, `onRequestError`), `instrumentation-client` (`onRouterTransitionStart`), and `mdx-components` (`useMDXComponents`) are also covered. Entry-point files still report genuinely unused helper exports alongside framework-used ones. ([#59](https://github.com/fallow-rs/fallow/pull/59) by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza))
- **Next.js `transpilePackages` support** -- packages listed in `next.config.{ts,js}` `transpilePackages` are treated as referenced dependencies, preventing false unused-dependency reports. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Config-defined alias resolution for Vite, Nuxt, and SvelteKit** -- aliases defined in `vite.config.ts` (`resolve.alias`), `nuxt.config.ts` (`alias`), and `svelte.config.ts` (`kit.alias`) are extracted at analysis time and fed into the resolver as fallbacks. Supports object form, array form, `fileURLToPath(new URL(...))`, `path.resolve(__dirname, ...)`, and `path.join(...)` patterns. Eliminates false unresolved-import, unlisted-dependency, and unused-file reports for config-aliased paths. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Nuxt `srcDir` support** -- when `srcDir` is set in `nuxt.config.ts`, fallow remaps `~/` and `@/` aliases to the configured source directory, adds entry patterns and always-used files under that directory, and resolves `imports.dirs` and `components` paths relative to srcDir. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Nuxt custom directories** -- `imports.dirs`, `components` (string arrays and `{ path: "..." }` objects), and `components.dirs` from `nuxt.config.ts` are now discovered as auto-import roots and component directories. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Nuxt `@/` and `@@/` alias recognition** -- `@/` (srcDir synonym for `~/`) and `@@/` (rootDir synonym for `~~/`) are now recognized as path aliases in both static analysis and resolver fallback. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **SvelteKit param matcher support** -- files in `src/params/**/*.{ts,js}` have their `match` export treated as framework-used. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Vue component tag tracking** -- imported components used as template tags (`<FancyCard />`, `<fancy-card />`, `<Form.Input />`) are now credited as used. Supports PascalCase, kebab-case-to-PascalCase conversion, and namespace member access. Built-in components (`Transition`, `KeepAlive`, `Teleport`, `Suspense`, `Slot`) are excluded. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Vue custom directive tracking** -- custom directives like `v-focus-trap` are mapped to their imported binding name (`vFocusTrap`) and credited as used. Built-in directives (`v-if`, `v-for`, `v-model`, etc.) are excluded. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Vue `v-on` and `v-bind` object syntax** -- `v-on="handlers"` and `v-bind="attrs"` now credit the bound expression as used. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Svelte component tag tracking** -- imported components used as Svelte markup tags (`<FancyButton />`, `<Icons.Alert />`) are credited as used, including namespace member access. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Svelte directive and attribute tracking** -- `use:tooltip`, `transition:fade`, `animate:flip`, `in:fly`, `out:slide` directives credit their imported action/transition binding as used. Attribute value expressions (`class:active={isActive}`) and shorthand attributes (`{page}`) are also tracked. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Svelte `$store` subscription tracking** -- `$page.url.pathname` in Svelte templates now credits the `page` import as used, with `url` tracked as a member access. ([#59](https://github.com/fallow-rs/fallow/pull/59))

### Fixed

- **Svelte template scanner index advancement** -- the HTML tag scanner was advancing by only 1 byte instead of jumping to the end of the scanned tag, causing every HTML tag to be re-parsed from its second byte. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Panic on malformed Svelte brace attributes** -- `parse_markup_tag` used `.expect()` on `scan_curly_section` which could panic on unterminated brace expressions in Svelte templates. Replaced with graceful fallback. ([#59](https://github.com/fallow-rs/fallow/pull/59))

## [2.14.2] - 2026-04-06

### Added

- **Svelte/Vue template-visible import tracking** -- imports used only in SFC template markup (e.g., `{formatDate(x)}` in Svelte, `{{ utils.format() }}` in Vue) are now credited as used, preventing false unused-export/import reports. Namespace member access in templates (e.g., `utils.formatDate`) is tracked as member usage. Vue credits only `<script setup>` bindings; Svelte excludes `context="module"` scripts. ([#58](https://github.com/fallow-rs/fallow/pull/58) by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza))
- **Type-only circular dependency filtering** -- `import type` edges are excluded from cycle detection since they are erased at compile time and cannot cause runtime cycles. ([#54](https://github.com/fallow-rs/fallow/issues/54))
- **Duplicate export common-importer filter** -- duplicate exports are only reported when the files sharing the same export name also share a common importer. Unrelated leaf files (e.g., SvelteKit route modules in different directories) are no longer flagged. ([#54](https://github.com/fallow-rs/fallow/issues/54))

### Fixed

- **Workspace plugin merge: generated imports and path aliases** -- `generated_import_patterns` (e.g., SvelteKit `$types`) and `path_aliases` (e.g., SvelteKit `$lib/`) from workspace-level plugins were not propagated to the root analysis, causing false-positive unresolved imports and resolution failures in monorepo setups. ([#54](https://github.com/fallow-rs/fallow/issues/54))
- **Windows path prefix** -- replaced `std::fs::canonicalize()` with `dunce::canonicalize()` to avoid `\\?\` extended-length path prefix on Windows, which broke `oxc_resolver` tsconfig discovery and caused all path-aliased imports to be reported as unresolved. ([#55](https://github.com/fallow-rs/fallow/pull/55) by [@KamilDev](https://github.com/KamilDev))

## [2.14.1] - 2026-04-06

### Added

- **HTML entry file parsing** -- when an HTML file is reachable (e.g., Vite's `index.html` entry point), fallow now parses `<script src>`, `<link rel="stylesheet" href>`, and `<link rel="modulepreload" href>` to create graph edges to referenced local assets. This prevents false-positive dead-code reports for JS/CSS files and their transitive imports in Vite/Parcel-style apps. HTML files are exempt from unused-file detection. ([#57](https://github.com/fallow-rs/fallow/issues/57))
- **Parcel `index.html` entry pattern** -- the Parcel plugin now auto-detects `index.html` as a runtime entry point, matching the Vite plugin's behavior.

### Fixed

- **Coverage gaps inline suppression** -- `coverage-gaps` issue kind can now be suppressed with `// fallow-ignore-next-line coverage-gaps` comments.

## [2.14.0] - 2026-04-06

### Added

- **Static test coverage gaps** (`fallow health --coverage-gaps`) -- reports runtime files and exports that no test dependency path reaches through the module graph. Splits entry points into runtime, test, and support roles across 84+ framework plugins. All 6 output formats supported. ([#53](https://github.com/fallow-rs/fallow/pull/53) by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza))
- **Coverage gaps severity control** -- `coverage-gaps` rule in config (`error`/`warn`/`off`, default `off`). When set to `error`, non-zero exit on any gap. When `warn`, reports gaps without failing CI.
- **Coverage gap actions** -- JSON output includes `add-tests` and `add-test-import` actions for AI agents on untested files and exports.
- **Entry point roles for external plugins** -- `entryPointRole` field (`runtime`/`test`/`support`) on external plugin definitions, allowing custom frameworks to declare how their entry points affect coverage reachability.

### Fixed

- **Workspace entry point fallback scoping** -- `discover_workspace_entry_points` fallback now correctly scopes to the workspace root instead of searching the entire project.
- **External plugin default role** -- external plugin `entryPointRole` now defaults to `support` (matching unknown builtins) instead of `runtime`.

## [2.13.4] - 2026-04-06

### Fixed

- **False positive unused exports from namespace exports** -- `export namespace Foo { export function bar() {} }` no longer reports inner declarations (`bar`) as unused top-level exports. Inner exports are now tracked as namespace members. Runtime namespaces (no `declare`) are correctly classified as non-type-only. ([#52](https://github.com/fallow-rs/fallow/issues/52))

## [2.13.3] - 2026-04-05

### Changed

- **Human output readability** -- all abbreviations spelled out in user-facing output: "deps" → "dependencies", "MI" → "maintainability", "dep" → "dependency". Affects health vital signs, dead-code summary footer, combined orientation header, markdown tables, and score deductions.
- **Section headers in dead-code output** -- human format now groups findings under `── Unused Code ──`, `── Dependencies ──`, and `── Structure ──` headers for faster scanning.
- **Labeled metrics and deductions** -- health score deductions line now prefixed with "Deductions:", metrics lines prefixed with "Metrics:" across health, combined, and audit commands.

### Fixed

- **Boundaries excluded from default `fallow list`** -- `fallow list` no longer shows "Boundaries: not configured" noise; use `--boundaries` to inspect zones and rules. ([#49](https://github.com/fallow-rs/fallow/pull/49) by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza))
- **Ecosystem runner error handling** -- `install_deps()` no longer swallows failures via `|| true`, and stderr is separated from JSON output files. Also uses canonical `dead-code` command with `--quiet`. ([#48](https://github.com/fallow-rs/fallow/pull/48) by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza))
- **Audit vital signs labeled** -- the audit command's vital signs line now includes a "Metrics:" prefix, consistent with health and combined commands.
- **Stale `fallow check` in scripts** -- replaced legacy command name in `bench-ci.sh` and `conformance/run.sh`.

## [2.13.2] - 2026-04-05

### Fixed

- **Plugin entry points included in `--entry-points` mode** -- `fallow list --entry-points` now includes plugin-discovered entry points, matching the behavior of the default `fallow list` output. Previously, plugin detection was skipped when `--entry-points` was used without `--plugins`. ([#45](https://github.com/fallow-rs/fallow/pull/45) by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza))
- **`Failed` summary line uses canonical command name** -- the summary line printed on analysis failure now says `fallow dead-code` instead of the legacy `fallow check`.
- **Documentation `cargo run` examples** -- all `cargo run` examples in CONTRIBUTING.md and CLAUDE.md now include `--bin fallow` (required for multi-binary workspaces) and use `dead-code` instead of the legacy `check` command. ([#44](https://github.com/fallow-rs/fallow/pull/44) by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza))

## [2.13.1] - 2026-04-05

### Fixed

- **Init hook uses canonical command name** -- `fallow init --hooks` now generates hooks with `fallow dead-code` instead of the legacy `fallow check` alias, and the internal `--base` field is renamed to `--branch` to match the CLI flag. ([#43](https://github.com/fallow-rs/fallow/pull/43) by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza))
- **Legacy command name cleanup** -- replaced `fallow check` with `fallow dead-code` in 4 user-facing messages: combined output suggestion, GitHub Action PR review body, VS Code extension diagram, and conformance test script.
- **Documentation consistency** -- fixed stale `--base` references to `--branch` in `AGENTS.md`, `docs/backwards-compatibility.md`, and companion repos.

## [2.13.0] - 2026-04-04

### Added

- **Bun built-in module support** -- `bun:sqlite`, `bun:test`, `bun:ffi`, and other `bun:` prefixed imports are now recognized as platform builtins and never flagged as unlisted dependencies. ([#40](https://github.com/fallow-rs/fallow/issues/40))
- **`ignoreDependencies` suppresses unlisted warnings** -- dependencies listed in `ignoreDependencies` are now excluded from both unused dependency AND unlisted dependency detection. Useful for runtime-provided packages like `bun:sqlite` or globally available dependencies. ([#40](https://github.com/fallow-rs/fallow/issues/40))
- **MCP server distributed via npm** -- `fallow-mcp` binary is now included in the npm package. After `npm install fallow`, the `fallow-mcp` command is available for MCP server integration with Claude, OpenCode, and other AI agents. ([#42](https://github.com/fallow-rs/fallow/issues/42))

### Fixed

- **`$schema` accepted in `.fallowrc.json`** -- the JSON schema now includes `$schema` as a valid property, so JSON editors no longer show "unknown key" warnings when using the schema reference. ([#39](https://github.com/fallow-rs/fallow/issues/39))
- **VS Code extension LSP download on Windows** -- the release workflow now names LSP binaries with the correct platform identifier (e.g., `fallow-lsp-win32-x64-msvc.exe`), matching what the VS Code extension expects. Previously, Windows users saw "no LSP binary found" errors. ([#38](https://github.com/fallow-rs/fallow/issues/38))

## [2.12.1] - 2026-04-04

### Fixed

- **Tab indentation preserved in export auto-fix** -- `fallow fix` no longer silently converts tab indentation to spaces when removing the `export` keyword. The original whitespace prefix is now preserved exactly. ([#36](https://github.com/fallow-rs/fallow/issues/36), [#37](https://github.com/fallow-rs/fallow/pull/37) by [@swalha1999](https://github.com/swalha1999))

## [2.12.0] - 2026-04-03

### Added

- **Vital signs percentage referents** -- the orientation header now shows denominators: `dead files 0.6% (1 of 173)` instead of just `0.6%`. Raw counts are also exposed in the health JSON `vital_signs.counts` object for CI dashboards.
- **Entry-point detection inline** -- combined and standalone check modes display `130 entry points detected (124 plugin, 6 package.json)` on stderr, with a yellow warning when zero entry points are found. Entry-point summary is also in the check JSON `entry_points` object.
- **Baseline-aware deltas** -- when `--baseline` is active, the `Failed:` summary line shows `+N since baseline` or `-N since baseline`. A `baseline_deltas` object with per-category deltas is added to the check JSON output.
- **`--summary` flag** -- global flag that shows only category counts without individual items. Works across check (severity-colored counts), dupes (families, groups, lines, rate), and health (functions analyzed, MI, score). JSON output always includes the full `summary` counts object regardless of this flag.
- **`--effort` filter** -- `fallow health --effort low|medium|high` filters refactoring targets by estimated effort level.
- **`--group-by package`** -- groups dead-code findings by workspace package in monorepos. Discovers workspaces automatically from `package.json`/`pnpm-workspace.yaml`.
- **`publicPackages` config** -- workspace package name patterns (exact or glob) whose exports are treated as public API and excluded from unused-export detection.
- **`dynamicallyLoaded` config** -- glob patterns for files loaded at runtime (plugin directories, locale files) that are treated as always-used entry points.
- **`fixture_glob_patterns()` Plugin trait method** -- plugins can now declare test fixture patterns that are implicitly used. Added for Jest, Vitest, and Playwright.
- **Cross-package circular dependency flag** -- `CircularDependency.is_cross_package` indicates cycles crossing workspace boundaries. Shown as `(cross-package)` in human output, present in JSON/SARIF/compact/markdown/CodeClimate.
- **Mirrored directories in JSON** -- `DuplicationReport.mirrored_directories` array with `dir_a`, `dir_b`, `shared_files`, and `total_lines` for CI consumption.
- **Smarter `fallow init`** -- detects project structure (TypeScript, monorepo tool, test framework, UI framework, Storybook) and generates a tailored `.fallowrc.json` with workspace patterns, ignore rules, and entry points.
- **Undeclared workspace diagnostic** -- warns when directories with `package.json` exist but aren't declared in workspace patterns.
- **Second-level directory rollup** -- when one directory holds >80% of unused files, the rollup automatically shows per-subdirectory breakdown (e.g., `packages/svelte/ 4463 files` instead of just `packages/ 4463`).
- **Test-only dependencies in CI summaries** -- `test_only_dependencies` category now appears in GitHub Action and GitLab CI PR summary comments.

### Changed

- **MI legend gated** -- the `MI scale: good ≥85, moderate ≥65, low <65 (0–100)` legend only appears when the average MI is below 85 (moderate or low). Projects with good health scores no longer see it.
- **Config quality note threshold** -- raised from 50% to 80%, reducing noise for projects with moderate test directory concentration.
- **Plugin discovery hint** -- unresolved imports footer now mentions framework plugins: "Framework-specific imports may need a plugin".
- **Scale-aware start-here nudge** -- when total issues exceed 500, the nudge suggests `--workspace <name>` instead of pointing to a specific file.
- **Summary footer filtered counts** -- the `✗ N files · N exports` summary line now reflects visible counts after export suppression, not raw totals.
- **Advisory note stream consistency** -- all advisory notes now consistently use stderr.
- **Rollup suppress hint** -- directory rollup sections suggest `ignorePatterns` in config instead of inline `fallow-ignore-next-line` comments.

### Fixed

- **Unlisted deps false positives** -- shell variables (`$DIR`), pure numbers (`1`), and bundler-internal specifiers (`__barrel_optimize__?...`) are no longer classified as npm package names. Reduces next.js false positives from 761 to 753.
- **Start-here noise filter** -- the "start with X" recommendation no longer points to test fixtures, playground files, or generated files (e.g., `a0.js`). When all targets are noise, the nudge is omitted entirely.
- **Nested node_modules exclusion** -- workspace glob expansion now skips directories inside `node_modules`, preventing third-party package.json files from being analyzed as workspace packages.
- **Init config serialization** -- changed `unwrap_or_else` with silent `{}` fallback to `expect()` for infallible JSON serialization.

## [2.11.0] - 2026-04-03

### Added

- **`--group-by owner|directory`** -- new global flag that groups all dead-code analysis output by team ownership (CODEOWNERS) or by first directory component. CODEOWNERS parser with auto-probe (`.github/CODEOWNERS`, `.gitlab/CODEOWNERS`, `docs/CODEOWNERS`), last-match-wins pattern matching, first-owner-on-line selection. All 6 output formats supported: human (colored group headers with summary line, per-type breakdown, matching rule annotations), JSON (grouped envelope with `groups` array), compact (group prefix per line), markdown (section headers per group), SARIF (`properties.owner`), CodeClimate (`owner` field).
- **`codeowners` config field** -- optional path to a non-standard CODEOWNERS file in `.fallowrc.json` / `fallow.toml`.
- **MCP `group_by` parameter** -- the `analyze` tool now accepts `group_by: "owner" | "directory"` to produce grouped JSON output.

### Changed

- **GitHub Action review comments** -- now filtered to PR diff hunks using `--slurpfile` for large PRs, preventing `ARG_MAX` crashes on PRs with 50+ changed files.
- **GitHub Action review UX** -- improved hunk filtering to tolerate whitespace-only and context lines, with better fallback behavior when no hunk matches.

## [2.10.1] - 2026-04-03

### Changed

- **PR comment vital signs clarity** -- the metrics table in GitHub Action and GitLab CI PR comments now has a "Codebase health" section header, making it clear that metrics are project-wide rather than PR-scoped. The `dead_export_pct` metric is removed from PR comments (still available in CLI JSON output) since it's a graph-level property not actionable from a single PR.
- **Scoped maintainability row** -- when `--changed-since` is active, PR comments now show a "Maintainability (changed files)" row alongside the codebase-wide score, so developers can see how their changes compare to the project baseline.
- **Clickable commit hash** -- the "Scoped to files changed since" footer now links the commit hash to the commit on GitHub/GitLab instead of showing plain text.
- **Disambiguated scoping footer** -- the footer now reads "Issue counts scoped to files changed since ... · health metrics reflect the full codebase" to eliminate ambiguity about what is PR-scoped vs codebase-wide.

## [2.10.0] - 2026-04-03

### Added

- **HTTPS URL extends** -- the `extends` field in `.fallowrc.json` now supports `https://` URLs alongside relative paths and `npm:` packages. Fetch remote shared configs without publishing an npm package. HTTPS-only, 5s default timeout (configurable via `FALLOW_EXTENDS_TIMEOUT_SECS`), 1 MB body limit, URL normalization for cycle detection. URL-sourced configs may extend other URLs or `npm:` packages but not relative paths.
- **GitHub Action shallow clone resilience** -- PR comment, review, annotation, and summary scripts now detect shallow clones and fall back gracefully instead of failing.

## [2.9.3] - 2026-04-03

### Fixed

- **GitHub Action Marketplace** -- shortened action description to meet the 125-character limit required for Marketplace publication.

## [2.9.2] - 2026-04-02

### Added

- **npm package resolution for `extends`** -- config files can now extend shared configs from npm packages using the `npm:` prefix (e.g., `"extends": "npm:@company/fallow-config"`). Resolution walks up `node_modules/`, checks `package.json` `exports`/`main`, and falls back to standard config file names. Subpaths supported (e.g., `npm:@co/config/strict.json`).
- **MCP server hardening** -- improved parameter validation, tool descriptions, and error messages for better AI agent integration.

### Internal

- Path confinement (`resolve_confined`) prevents traversal attacks via npm subpaths or malicious `package.json` exports/main fields.
- Package name validation rejects `..`/`.` components and bare `@scope` without name.

## [2.9.1] - 2026-04-02

### Fixed

- **Deterministic output ordering** -- all result arrays are now sorted after parallel collection, ensuring identical JSON/SARIF/CodeClimate output across runs. Previously, rayon parallelism caused item ordering to vary between invocations.
- **Symlink canonical index warning** -- downgraded noisy symlink resolution warning to debug level.
- **GitHub Action scoping** -- PR comments and annotations now correctly scope to changed files when `--changed-since` is active.

### Changed

- **Performance** -- `OutputFormat`, `MemberKind`, and `DupesMode` enums now derive `Copy`, eliminating unnecessary clones and reference indirection across 20 CLI files.

### Internal

- Split 5 large modules into focused submodules: `regression.rs` (5 modules), `diagnostics.rs` (4 modules), `health_types.rs` (5 modules), plus 3 monolith test files into 30 modules.
- Extracted helper functions from 4 long CLI command functions (1,001 → 365 lines).
- Added 103 unit tests to the config crate (287 → 390 tests).
- Added module-level documentation to all `graph/resolve/` submodules.

## [2.9.0] - 2026-04-01

### Added

- **Architecture boundary violations** -- define zones (glob patterns mapping directories to architecture layers) and rules (which zones may import from which). Violations are detected at the import site using the resolved target's zone. Reported in all 6 output formats, with inline and file-level suppression via `// fallow-ignore-next-line boundary-violation`. LSP diagnostics, code actions, GitHub Action annotations, GitLab CI review comments, and MCP server integration all included.
- **Built-in boundary presets** -- `"preset": "layered"` (4 zones: presentation/application/domain/infrastructure), `"preset": "hexagonal"` (3 zones: adapters/ports/domain), `"preset": "feature-sliced"` (6 zones: app/pages/widgets/features/entities/shared), `"preset": "bulletproof"` (4 zones: app/features/shared/server). Presets auto-detect `rootDir` from `tsconfig.json` for pattern prefixes. User zones and rules merge on top of presets.
- **`fallow list --boundaries`** -- inspect expanded boundary zones, rules, and per-zone file counts. Supports human and JSON output formats.
- **`--boundary-violations` filter** -- show only boundary violations in `fallow dead-code` output.

### Fixed

- **Rest patterns in destructured exports** -- `export const { a, ...rest } = obj` no longer causes a parser error.

### Changed

- **oxc dependency upgrade** -- bumped all 7 oxc crates to latest versions.

## [2.8.1] - 2026-04-01

### Fixed

- **`fallow init` no longer panics** -- the global `--base` alias for `--changed-since` collided with Init's own `--base` flag, causing a runtime panic on every `fallow init` invocation. Init's flag is now `--branch`.

### Added

- **CLI integration test infrastructure** -- shared test harness for all CLI commands with 58 new tests covering `check`, `health`, `dupes`, `init`, exit codes, baselines, and MCP end-to-end. 7 new test fixtures (astro, mdx, complexity, config-file, config-toml, hidden-dir-allowlist, error-no-package-json). Human output snapshots for check, health, and dupes commands.

## [2.8.0] - 2026-04-01

### Added

- **`fallow audit` command** -- combined dead-code + complexity + duplication analysis scoped to changed files, returning a verdict (pass/warn/fail). Purpose-built for reviewing AI-generated code and PR quality gates. Auto-detects the base branch if `--base` is not specified. JSON output includes `verdict`, per-category `summary`, and full sub-results with `actions` arrays. All 6 output formats supported. MCP `audit` tool wraps the CLI.
- **`--base` global alias** -- `--base` is now a global alias for `--changed-since` on all commands. More intuitive for PR review workflows.
- **`.fallow/` added to `.gitignore` during `fallow init`** -- prevents snapshot and cache directories from being committed.

### Changed

- **Release binary uses `panic=abort`** -- smaller binary size by removing unwind tables.
- **Rust 2024 formatting style** -- `.rustfmt.toml` with `style_edition = "2024"`.
- **Stricter lint discipline** -- 6 restriction lints added, `#[expect]` required for all lint suppressions, `tail_expr_drop_order` compiler lint enabled.

## [2.7.3] - 2026-03-31

### Fixed

- **`--format badge` now auto-enables `--score`** -- previously `fallow health --complexity --format badge` would error because score computation wasn't triggered when explicit section flags were passed. Badge format now implies `--score`, matching the behavior of `--min-score`, `--trend`, and `--save-snapshot`.

## [2.7.2] - 2026-03-31

### Added

- **Badge generation** (`--format badge`) -- generates a self-contained shields.io-compatible flat SVG badge showing the health score and letter grade. Grade-first message format (`B (76)`) matches quality dashboard conventions. Color-coded by grade: A=brightgreen, B=green, C=yellow, D=orange, F=red. Unique SVG element IDs prevent collisions when multiple badges are inlined on one page. Also available via `FALLOW_FORMAT=badge` environment variable.

## [2.7.1] - 2026-03-30

### Fixed

- **SvelteKit `$app` and `$env` no longer reported as unlisted dependencies** -- virtual module prefix matching failed when the extracted package name (e.g., `$app`) was compared against a prefix with a trailing slash (`$app/`). Also fixes the same latent bug for Docusaurus virtual prefixes (`@theme/`, `@docusaurus/`, etc.)
- **SvelteKit `./$types` imports no longer reported as unresolved** -- added `generated_import_patterns()` to the Plugin trait so frameworks can declare build-time generated import suffixes. SvelteKit uses this to suppress `./$types`, `./$types.js`, and `./$types.ts` route type imports that are generated at build time and don't exist on disk during static analysis.

## [2.7.0] - 2026-03-30

### Added

- **Structured fix suggestions in JSON output** -- every issue in `--format json` output now includes an `actions` array with machine-actionable fix and suppress hints. 14 fix action types (kebab-case), `auto_fixable` flag on every action, optional `note` for non-auto-fixable items. Dependency issues use `add-to-config` suppress with concrete package name. Re-export findings include a warning note about public API surface. `duplicate_exports` suppress includes `scope: "per-location"`. No schema version bump (additive change).
- **Pre-commit hook setup** (`fallow init --hooks`) -- scaffolds a git pre-commit hook that runs `fallow check --changed-since` on changed files. Auto-detects base branch via git. Supports `--base <ref>` override. Detects husky, lefthook, and bare `.git/hooks`. Includes binary guard and helpful success message with `--no-verify` bypass. Input validated to prevent shell injection.

## [2.6.0] - 2026-03-30

### Added

- **Trend reporting** (`fallow health --trend`) -- compare current health metrics against the most recent saved snapshot and show per-metric deltas with directional indicators (improving/declining/stable). Tracks 8 metrics: health score, dead files, dead exports, avg cyclomatic complexity, maintainability, unused deps, circular deps, and hotspots. Implies `--score`. Reads from `.fallow/snapshots/` (saved via `--save-snapshot`). Output in all formats: human (colored arrows with all-stable collapse), JSON (structured `health_trend` object with raw counts), compact (grep-friendly `trend:overall:direction=` + per-metric lines), and markdown (table for PR comments). Warns when combined with `--changed-since`. Wired through MCP server and GitHub Action.

## [2.5.5] - 2026-03-28

### Fixed

- **Stale cache caused type-level enum detection to not take effect** -- bumped cache version to invalidate entries from v2.5.3/v2.5.4 that were missing the new type-level `whole_object_uses` extraction data

## [2.5.4] - 2026-03-28

### Fixed

- **`Record<Enum, T>` now marks all enum members as used** -- `Record<Status, string>` and nested variants like `Partial<Record<Status, number>>` are now recognized as whole-object enum usage, preventing false unused member reports
- **`keyof typeof Enum` in mapped types now marks all members as used** -- `{ [K in keyof typeof Direction]: string }` is now detected as a whole-object use pattern
- **Relaxed `@types/` unlisted dependency check** -- the type-only import check for `@types/<package>` now works regardless of the import's `type` modifier style

## [2.5.3] - 2026-03-28

### Fixed

- **Type-only imports with `@types/` no longer flagged as unlisted** -- `import type { Feature } from 'geojson'` is no longer reported as an unlisted dependency when `@types/geojson` is in devDependencies. TypeScript erases type-only imports at compile time, so the bare package doesn't need to be installed. Scoped packages are handled via the DefinitelyTyped convention (`@scope/pkg` → `@types/scope__pkg`). Value imports still require the real package.
- **Unused enum member false positives in type-level usage** -- enum members used via TypeScript qualified names in type position (e.g., `type X = Status.Active`) are now detected as used. Enums used as mapped type constraints (e.g., `{ [K in Direction]: string }`) mark all members as used.

## [2.5.2] - 2026-03-28

### Fixed

- **Vitest reporter false positives** -- packages referenced as strings in `test.reporters` (e.g., `vitest-sonar-reporter`) are now detected as used dependencies
- **Vitest coverage/typecheck/browser false positives** -- `test.coverage.provider`, `test.typecheck.checker`, and `test.browser.provider` values are now resolved to their corresponding packages
- **ESLint import resolver false positives** -- packages referenced via `settings["import/resolver"]` (e.g., `eslint-import-resolver-typescript`) are now detected in string, array, and object key formats
- **CI pipeline dependency false positives** -- binaries invoked via `npx` in `.gitlab-ci.yml` and `.github/workflows/*.yml` are now detected as used dependencies

## [2.5.1] - 2026-03-28

### Fixed

- **Absolute paths in duplication suggestions** -- refactoring suggestions (e.g. "Extract ... into src/hooks") were printing absolute filesystem paths instead of project-relative paths

## [2.5.0] - 2026-03-28

### Added

- **Project health score** (`--score`, `--min-score`) -- a single 0-100 number with letter grade (A/B/C/D/F) aggregated from dead code, complexity, maintainability, hotspots, unused dependencies, and circular dependencies. Penalty breakdown in JSON is reproducible: `100 - sum(penalties) == score`. The score is shown by default in `fallow health` output and automatically included in vital signs snapshots (schema v2).
- **CI quality gate** (`--min-score N`) -- exit code 1 when the health score drops below a threshold. Pairs with `--score` for a simple "is this codebase healthy enough?" CI check.
- **Regression detection** (`--fail-on-regression`, `--tolerance`) -- CI gate that compares current issue counts against a previously saved baseline. Supports both absolute (`"5"`) and percentage (`"2%"`) tolerance. Baselines can be saved to a file (`--save-regression-baseline`) or embedded in config.

### Changed

- Health score is included by default when running `fallow health` (no section flags). Use `--score` as a section filter to show only the score.
- `--save-snapshot` now automatically includes the health score and grade in the snapshot (snapshot schema v2). Old v1 snapshots remain readable.

## [2.4.0] - 2026-03-27

### Added

- **Test-only production dependency detection** -- new issue type (14th) flags production dependencies that are only imported by test files, suggesting they be moved to `devDependencies`. Full pipeline: all 6 output formats, LSP diagnostics, baseline support, inline suppression, severity rules (default: `warn`).
- **`tap` test runner support** -- `tap` and `@tapjs/*` packages recognized as tooling dependencies
- **`type-only-dependency` inline suppression** -- `// fallow-ignore-next-line type-only-dependency` now works (was a pre-existing gap where the suppression comment was silently ignored)

### Fixed

- **Tooling over-exemption** -- removed blanket `@babel/`, `babel-`, and `@rollup/` prefix exemptions from unused dependency detection. These packages are now verified against actual config files via plugin parsing, catching ~8 previously missed unused dev dependencies.
- **ESLint shared config following** -- ESLint plugin now reads imported config packages one level deep to discover peer dependencies referenced by shared configs
- **Prettier config parsing** -- new PrettierPlugin extracts plugin references from `.prettierrc` and `prettier.config.*` files
- **Storybook addon object form** -- Storybook plugin now handles `{ name: '@storybook/addon-essentials' }` object entries alongside string entries in the `addons` array

## [2.3.1] - 2026-03-27

### Added

- **Comprehensive test coverage** — 1,200+ new tests across all crates bringing total to 4,700+ unit/integration tests, 101 snapshot tests, 30 property-based tests, 14 doc tests, and 7 conformance fixtures, achieving 91% line coverage
- **CJS `module.exports.foo` detection** — individual property assignments like `module.exports.foo = fn` are now extracted as named exports, closing ~60 missed findings on CJS-heavy projects
- **Conformance test harness** — `tests/conformance/verify-fixtures.sh` and `verify-expected.py` provide automated expected-output verification for 7 analysis scenarios (barrel resolution, circular deps, suppression, type-only imports, and more)

### Fixed

- **Unreachable module export blindspot** — modules not reachable from entry points but containing a mix of used/unused exports were previously skipped entirely; now each export is evaluated individually
- **Dead test file removed** — `unused_exports_tests.rs` (924 lines) was never compiled due to a missing module directive and contained a type mismatch; inline tests already covered all cases

## [2.3.0] - 2026-03-27

### Added

- **GitLab CI rich MR comments** — new `FALLOW_COMMENT` and `FALLOW_REVIEW` variables enable rich MR summary comments with collapsible sections and inline review discussions with suggestion blocks, matching the GitHub Action's review quality
- **GitLab inline review discussions** — posts findings as positioned `DiffNote` discussions on MR diffs with "Why this matters" sections, actionable fix steps, docs links, and one-click suppress instructions
- **GitLab suggestion blocks** — unused export findings include `suggestion:-0+0` blocks for one-click `export` keyword removal directly in the MR diff
- **Comment merging pipeline** — groups unused exports per file into single comments, deduplicates clone group findings, drops redundant refactoring targets, and merges same-line findings with numbered headers
- **Auto `--changed-since` in GitLab MR context** — automatically scopes analysis to changed files using `CI_MERGE_REQUEST_DIFF_BASE_SHA` when running in merge request pipelines
- **Package manager detection** — review comments and annotations now show correct install/uninstall commands (`npm uninstall`, `pnpm remove`, or `yarn remove`) based on lock file detection
- **GitLab comment cleanup** — automatically removes previous fallow comments and discussions on re-runs to prevent comment spam

### Changed

- **GitLab CI template modularized** — inline jq scripts extracted to separate files in `ci/jq/` and `ci/scripts/`, downloaded at runtime for maintainability
- **`diff_refs` from MR API** — GitLab inline discussions now fetch `base_sha`, `start_sha`, `head_sha` from the MR API instead of CI environment variables, matching the ictu-mcp pattern and improving positioning accuracy after rebases
- **Suggestion block ANSI-C quoting** — GitHub Action suggestion blocks fixed to use `$'...'` quoting for correct newline rendering

### Fixed

- **Subshell variable loss** — review comment counters (`POSTED`/`SKIPPED`) now use process substitution instead of pipe subshell to correctly track posting results
- **Comment body assignment** — `comment.sh` separates jq execution from string concatenation to correctly detect jq failures
- **Combined mode null arrays** — `jq -s 'add'` replaced with explicit `jq -n --argjson` to prevent null output when all comment arrays are empty

## [2.2.3] - 2026-03-27

### Added

- **Auto-changed-since for PRs** — GitHub Action automatically scopes analysis to changed files in pull requests using `--changed-since` with the merge base, eliminating the need for manual configuration
- **Enriched PR annotations** — inline annotations now include actionable context (export names, dependency names, file paths) with improved formatting for duplication annotations
- **VS Code status bar and tree views** — expanded extension UX with project health status bar, issue tree view, and pnpm workspace support
- **`analyze_with_parse_result` API** — new public function in `fallow-core` that accepts pre-parsed modules, enabling callers to skip the parsing stage when modules are already available

### Changed

- **Health pipeline optimization** — `fallow health --file-scores` no longer runs the analysis pipeline twice; pre-parsed modules are reused via the new `analyze_with_parse_result` API
- **O(1) tooling dependency lookups** — `GENERAL_TOOLING_EXACT` (76 entries) converted from linear slice scan to `OnceLock<FxHashSet>` for constant-time lookups
- **O(1) unused import binding lookups** — `ResolvedModule.unused_import_bindings` converted from `Vec<String>` to `FxHashSet<String>` in hot-path reference population
- **Optimized member export referencing** — `mark_member_exports_referenced` now uses `FxHashSet<&str>` and avoids per-export `to_string()` allocation
- **Report dispatcher unified** — new `ReportContext` struct replaces individual parameters across all 3 report dispatch functions for consistent signatures
- **`define_plugin!` macro extended** — supports `resolve_config: imports_only` variant; Cypress, Commitlint, Remark plugins migrated
- **Comprehensive code deduplication** — `emit_json()`, `plural()`, `build_json_envelope()`, shared `sample_results` test helper, fix module helpers, config parser shared traversal

### Fixed

- **Watch mode reload stability** — hardened debounce behavior and related cleanup
- **Windows CI path normalization** — discovery tests now normalize path separators for cross-platform compatibility
- **GitHub Action `pull_request_target` handling** — correctly detects and handles `pull_request_target` events in auto-changed-since logic

### Removed

- **1,986 lines of dead code** — removed orphaned `crates/graph/src/graph/build/` directory that was never compiled (abandoned refactoring artifact)

## [2.2.2] - 2026-03-27

### Added

- **CodeClimate output format** — `--format codeclimate` for GitLab Code Quality integration, with deterministic FNV-1a fingerprints and proper severity mapping
- **GitHub Actions inline annotations** — `--format annotations` emits `::warning` / `::error` workflow commands for inline PR annotations without any Action dependency
- **Real-world conformance benchmarks** — CI now validates against 8 real-world projects (zod, preact, vite, next.js, angular, nuxt, svelte, vue-core)
- **~283 new tests** — comprehensive coverage for complexity metrics, JSDoc @public tags, config extends/merge, re-export chain propagation, dynamic import patterns, declaration extraction, visitor helpers, analysis predicates, cycle detection, and file discovery

### Fixed

- **CodeClimate fingerprint stability** — use FNV-1a instead of `DefaultHasher` for deterministic cross-run fingerprints; include group index in duplication fingerprints
- **Circular dependency annotations** — sanitize chain strings and guard against empty files in annotation output
- **npm/pnpm install stdout leak** — suppress package manager install stdout that leaked into JSON report output
- **Duplicate exports comparison** — handle dict locations correctly in `duplicate_exports` comparison

## [2.2.1] - 2026-03-26

### Changed

- **Parallel workspace processing** — workspace entry point discovery and plugin runs now execute in parallel using rayon, with sequential merge for deterministic results. Up to 21% faster on monorepos (vite: 507ms → 399ms, next.js: 1532ms → 1371ms)
- **Lazy canonicalize** — skips upfront bulk `canonicalize()` of all source files when the project root is already canonical (common case). A `OnceLock`-based fallback handles the rare intra-project symlink edge case on demand. Saves up to 148ms on 20k-file projects
- **O(1) plugin dedup** — workspace plugin name and virtual module prefix deduplication uses `FxHashSet` instead of `Vec::contains` (O(n²) → O(n))

### Fixed

- **Benchmark accuracy** — benchmark script now correctly excludes knip runs that crash (exit code 2) instead of counting crash timings as valid results. Also guards against null status from timeouts
- **Updated benchmark numbers** — rebenchmarked all projects with honest error handling. Speed claims updated: 5-41x vs knip v5 (was 3-36x), 2-18x vs knip v6 (was 2-14x), 8-26x vs jscpd (was 20-33x)

## [2.2.0] - 2026-03-26

### Added

- **Efficiency score** — refactoring targets now include an `efficiency` field (`priority / effort`) and are sorted by efficiency descending, surfacing quick wins first
- **Confidence levels** — each target includes a `confidence` field (`high`/`medium`/`low`) based on data source reliability: `high` for graph/AST analysis, `medium` for heuristic thresholds, `low` for git-dependent recommendations
- **Adaptive thresholds** — fan-in/fan-out normalization uses percentile-based thresholds (p95/p90/p75/p25) from the project's distribution instead of hardcoded constants, with floors to prevent degenerate values in small projects
- **Target thresholds in JSON** — `target_thresholds` object in health JSON output exposes the computed adaptive thresholds for programmatic consumers
- **Effort summary** — human output shows effort breakdown after the targets header (e.g., `16 low effort · 34 medium · 43 high`)
- **Machine-parseable compact categories** — compact output uses underscore-delimited category labels (`circular_dep`, `dead_code`) instead of space-separated labels

### Changed

- **Human output: efficiency as primary score** — the hero number is now efficiency (sort key), with priority shown as a dimmed secondary value
- **Human output: labeled metadata** — effort and confidence on line 2 are now prefixed (`effort:low · confidence:high`) for self-documenting output
- **Markdown table: 5 columns** — reduced from 7 to 5 columns by merging effort/confidence and dropping the separate priority column
- **SARIF messages** — now include priority, efficiency, and confidence values

### Fixed

- **Cycle path deduplication** — `evidence.cycle_path` no longer contains duplicate entries when a file participates in multiple cycles
- **GitLab CI template** — uses Alpine image and detects package manager correctly
- **Benchmark alert threshold** — corrected for `customBiggerIsBetter` benchmarks
- **SARIF version redaction** in test fixtures
- **MCP analyze tool description** — corrected to match `dead-code` command

## [2.1.0] - 2026-03-25

### Added

- **GitLab CI template** (`ci/gitlab-ci.yml`) — includable template with full feature parity to the GitHub Action: Code Quality reports (CodeClimate format) for inline MR annotations, MR comment summaries, incremental caching, and all fallow commands/options via `FALLOW_*` variables
- **GitHub Action: test workflow** — CI validation for SARIF, JSON, dupes, fix, zero-issues, and PR comment scenarios

### Fixed

- **`list --no-cache`** — the `--no-cache` flag now works correctly with the `list` command
- **GitHub Action: `check` → `dead-code` rename** — completed the rename across all case statements, SARIF fallback, and job summary dispatch
- **`dead-code` subcommand** in backwards-compatibility stable interface list

## [2.0.1] - 2026-03-25

### Added

- **MCP server: all global CLI flags exposed** — `--baseline`, `--save-baseline`, `--no-cache`, `--threads` now available on all MCP tools; `--config` gap-filled on `find_dupes`/`check_health`; `--workspace` gap-filled on `find_dupes`/`fix_preview`/`fix_apply`
- **GitHub Action: 13 new inputs** — `no-cache`, `threads`, `only`, `skip`, `cross-language`, `file-scores`, `hotspots`, `targets`, `complexity`, `since`, `min-commits`, `save-snapshot`, `issue-types`
- **GitHub Action: `dead-code` alias support** — all case statements now handle both `dead-code` and legacy `check` command names
- **GitHub Action: bare invocation support** — combined issue count extraction, job summary, and PR comments work when no command is specified

### Fixed

- **GitHub Action: `fix` without `--dry-run` now adds `--yes`** — previously would hang in CI waiting for TTY input

## [2.0.0] - 2026-03-25

### Breaking

- **Bare `fallow` now runs all analyses** (check + dupes + health). Previously it ran only dead code analysis. Use `fallow dead-code` or `fallow --only check` for the old behavior.
- **`check` renamed to `dead-code`**: `fallow dead-code` is now the canonical name for dead code analysis. `fallow check` remains as a hidden alias for backwards compatibility.
- **`--ci`, `--fail-on-issues`, `--sarif-file` are now global flags**: work with all commands (check, dupes, health, bare). Previously check-only.
- **Combined JSON output**: bare `fallow --format json` produces `{ "check": {...}, "dupes": {...}, "health": {...} }` instead of a flat check-only object.
- **Combined SARIF output**: bare `fallow --format sarif` produces a multi-run SARIF document with one run per analysis.
- **MCP server tools** now use `dead-code` subcommand internally.

### Added

- **Combined analysis command**: bare `fallow` runs dead code + duplication + complexity in a single invocation with combined output across all 5 formats
- **`--only` / `--skip` flags**: select which analyses to run when using bare `fallow` (e.g., `fallow --only check,dupes`, `fallow --skip health`)
- **`dead-code` command alias**: `fallow dead-code` as the canonical name for dead code analysis (replaces `check`)
- **`--ci` on all commands**: `fallow dupes --ci` and `fallow health --ci` now work (SARIF + quiet + fail-on-issues)
- **Vital signs snapshots** (`fallow health --save-snapshot`): save codebase health metrics for trend tracking
- **Execute/run split**: internal refactor enabling combined mode — `execute_check`, `execute_dupes`, `execute_health` return results without printing

### Changed

- Codebase refactored: 10 large files split into focused submodules for maintainability

## [1.9.0] - 2026-03-25

### Added
- **Refactoring targets** (`fallow health --targets`): ranked, actionable recommendations that synthesize complexity, coupling, churn, and dead code signals into a prioritized list of files to refactor
  - Seven recommendation rules evaluated in priority order: urgent churn+complexity, break circular dependency, split high-impact file, remove dead code, extract complex functions, reduce coupling
  - Priority formula: `min(density,1)×30 + hotspot×25 + dead_code×20 + fan_in_norm×15 + fan_out_norm×10`
  - Contributing factors with raw `value` and `threshold` for programmatic use
  - **Effort estimation** (`low`/`medium`/`high`) based on file size, function count, and fan-in — shown in all output formats
  - **Evidence linking**: structured data for AI agents — unused export names, complex function names with line numbers, cycle member paths
  - **Baseline support**: `--save-baseline` / `--baseline` now includes refactoring targets for tracking progress over time
  - All five output formats: human (category · effort labels), JSON (with evidence), compact, markdown (Effort column), SARIF (warning-level findings)
  - MCP server: `targets` parameter on `check_health` tool
- VS Code extension overhaul: LSP notifications, walkthrough, diagnostics improvements, tree view enhancements
- Clickable documentation links in all diagnostics + JSON schema for config
- ~100 unit tests across 6 crates

### Fixed
- 11 bug fixes across parser, analysis, baseline, LSP, and performance
- Empty tree views now show "no issues found" after analysis
- `hasAnalyzed` context set from CLI analysis to clear welcome views
- Removed buggy extract duplicate code action
- Qualified `Span` in benchmark to resolve ambiguous import

## [1.8.1] - 2026-03-25

### Added
- LSP pull-model diagnostics (`textDocument/diagnostic`) for Cursor/VSCodium compatibility
- Multi-root workspace support in LSP: discovers monorepo workspaces via `package.json` workspaces, pnpm-workspace.yaml, and tsconfig references
- Precise diagnostic ranges for unresolved imports: squiggly underline now covers only the module specifier string literal, not the entire import line
- `source_span` field on `ImportInfo` for precise source string literal positions

### Fixed
- LSP `textDocument/diagnostic` errors in Cursor (Method not found -32601)
- Unused dependency diagnostics now appear on the correct line in package.json (was always line 1)
- Circular dependency diagnostics now appear on the import statement that starts the cycle
- File-level diagnostics (unused files, unlisted deps) highlight the entire first line instead of a zero-width marker
- Stale cached diagnostics cleared when issues are resolved between analysis runs
- Extract cache version bumped to 16 for new `ImportInfo.source_span` field

### Changed
- Internal refactoring: split large modules (human.rs, visitor.rs, resolve.rs, registry.rs, etc.) into focused submodules
- Improved test coverage across graph, extract, core, and LSP crates

## [1.8.0] - 2026-03-24

### Added
- **Human output polish**: completely redesigned terminal output across all three commands for readability at scale
  - Per-section footers with one-line explanations and docs links (always shown, replaces `--explain` for human output)
  - Mirrored directory detection in dupes: collapses `src/ ↔ deno/lib/` patterns into a single group
  - Circular dependency hub grouping: cycles sharing the same file are grouped with path elision
  - Duplicate exports stacked vertically with path elision for long monorepo paths
  - Global truncation: all sections default to 10 items with `... and N more` overflow hints
  - Thousands separators for large numbers (e.g., `5,433` lines)
- **`--complexity` flag** for `fallow health`: select only the complexity findings section
- **Health defaults to all sections**: `fallow health` now shows complexity + file-scores + hotspots by default. Use `--complexity`, `--file-scores`, or `--hotspots` to select specific sections.
- **`--explain` flag**: adds `_meta` object with metric definitions and docs URLs to JSON output. Human output always includes explanations.
- **`--top` flag** for `fallow dupes`: limit the number of clone groups shown
- `elide_common_prefix` utility for shorter paths in dependency chains and duplicate exports

### Changed
- Summary footer uses shortened labels (`27 files · 89 exports` instead of `27 unused files · 89 unused exports`)
- Health footer compacted to single line (`✗ 22 above threshold · 3111 analyzed · MI 90.5`)
- Dupes footer uses `✗` prefix consistent with check and health
- Health complexity findings use two-line format (function name on line 1, metrics on line 2)
- Single-group clone families suppressed from default output
- Tree connectors (├─ └─) replaced with simple indentation in dupes
- All docs URLs updated to `/explanations/{dead-code,health,duplication}` with section anchors
- Config warnings use `tracing::warn!` instead of `eprintln!`
- CI workflow permissions scoped to per-job level

## [1.7.0] - 2026-03-24

### Added
- Hotspot analysis (`fallow health --hotspots`): combines git churn history with complexity data to surface the riskiest files in a codebase. Score formula: `normalized_churn × normalized_complexity × 100` (0-100 scale). Recency-weighted commit count with exponential decay (half-life 90 days). Trend detection labels files as accelerating, stable, or cooling.
  - `--since` accepts durations (`6m`, `90d`, `1y`, `2w`) and ISO dates (`2025-06-01`), default 6 months
  - `--min-commits` threshold (default 3) excludes low-activity files from ranking
  - Fan-in shown as separate "blast radius" column
  - Shallow clone detection with warning
  - Available in all output formats (human, JSON, compact, markdown)
  - MCP `check_health` tool supports `hotspots`, `since`, and `min_commits` parameters

### Changed
- Health human output uses two-line format for hotspots and file scores: score/MI on first line with path (dimmed directory, bold filename), metrics on indented second line
- Renamed "lines" to "churn" in hotspot output across all formats to avoid ambiguity with file length

### Fixed
- MCP tests on Windows: skip `/bin/sh`-dependent tests on non-Unix platforms
- Typo checker false positive and Windows path separator in list tests

## [1.6.1] - 2026-03-24

### Added
- Per-file health scores (`fallow health --file-scores`): maintainability index combining complexity density, dead code ratio, fan-in, and fan-out. Available in all output formats (human, JSON, compact, markdown, SARIF). MCP `check_health` tool supports `file_scores: true` parameter.
- Markdown and SARIF output formats for health command
- Workspace scoping (`--workspace`) and baseline support (`--baseline`/`--save-baseline`) for health command

### Fixed
- Re-export chain propagation for entry-point-only exports: exports consumed solely via re-export from an entry point barrel (e.g., `export { render } from './render'` in `src/index.js`) are no longer falsely reported as unused. Fixes false positives for the common library barrel pattern.
- Entry point star re-exports (`export * from './source'`) now correctly exclude the default export per ES spec
- Deterministic bare specifier resolution: removed the `BareSpecifierCache` that caused non-deterministic results in multi-threaded mode when per-file tsconfig path aliases resolved the same specifier to different targets. Replaced with a deterministic post-resolution pass that upgrades `NpmPackage` to `InternalModule`. Analysis results are now identical between `--threads 1` and default multi-threaded mode.
- Hard error on health baseline I/O failures instead of silent fallback
- MCP server test and review findings addressed

## [1.6.0] - 2026-03-24

### Added
- Precise line/column locations in SARIF output for all issue types that previously defaulted to line 1
  - Unlisted dependencies: SARIF now points to the actual import statement in the source file
  - Unused dependencies (all variants): SARIF now points to the dependency entry line in package.json
  - Type-only dependencies: SARIF now points to the dependency entry line in package.json
  - Circular dependencies: SARIF now points to the import statement that starts the cycle
- `ImportSite` type in JSON output for unlisted dependencies (replaces plain path strings with `{path, line, col}` objects)
- `find_import_span_start()` method on `ModuleGraph` for looking up import statement locations between modules

### Changed
- JSON output `schema_version` bumped to 3 (new fields on `UnusedDependency`, `TypeOnlyDependency`, `CircularDependency`)
- `UnlistedDependency.imported_from` changed from `string[]` to `ImportSite[]` in JSON output
- `UnusedDependency` now includes `line` field (1-based line in package.json)
- `TypeOnlyDependency` now includes `line` field (1-based line in package.json)
- `CircularDependency` now includes `line` and `col` fields (location of the cycle-starting import)

### Fixed
- GitHub Code Scanning alerts for unlisted/unused dependencies no longer point to `package.json:1`
- GitHub Code Scanning alerts for circular dependencies no longer point to line 1 of the first file
- `output-schema.json` `DuplicateExport.locations` corrected from `string[]` to proper `DuplicateLocation[]` objects
- `output-schema.json` now includes missing `unused_optional_dependencies` field

## [1.5.0] - 2026-03-24

### Added
- `fallow health` command: per-function cyclomatic complexity (McCabe classic) and cognitive complexity (SonarSource algorithm) analysis with configurable thresholds (default: cyclomatic 20, cognitive 15), `--top N`, `--sort`, `--changed-since`, and human/JSON/compact output formats
- `[health]` config section with `maxCyclomatic`, `maxCognitive`, and `ignore` glob patterns
- `check_health` MCP tool for AI agent integration
- Function name capture from class methods, object method shorthand, variable declarators, property definitions, and `export default function()`

### Changed
- Duplicate export locations now include line and column numbers (JSON output `schema_version` bumped to 2)
- GitHub Action job summary redesigned with tables, icons, and collapsible sections
- Cache version bumped to 15 (old caches auto-invalidate)

### Fixed
- SARIF URIs now percent-encode brackets for Next.js dynamic routes (`[slug]` → `%5Bslug%5D`)
- JSON output uses relative paths for readable CI summaries
- GHAS availability check before SARIF upload in GitHub Action

## [1.4.0] - 2026-03-23

### Added
- Class instance member tracking: `const svc = new MyService(); svc.greet()` correctly tracks `greet` as a used class member, reducing false positives for class-based code (NestJS services, Angular components, etc.)
- JSDoc `@public` tag support: exports annotated with `/** @public */` (or `/** @api public */`) are never reported as unused, designed for library authors whose exports are consumed by external projects
- Type-only dependencies rule: production dependencies only imported via `import type` are reported as `type-only-dependency` (they should be devDependencies since types are erased at runtime)
- Progress spinners showing analysis pipeline stages (discovery, parsing, resolution, analysis) for better UX on larger projects
- VS Code extension published on Open VSX for Cursor and VSCodium support
- Historical performance metric tracking with GitHub Pages dashboard
- Package.json `#subpath` imports integration test

### Changed
- VS Code extension bundler migrated from esbuild to Rolldown
- Large modules split into focused submodules for maintainability: `discover.rs`, `config.rs`, `resolve.rs`, `detect.rs`, `cache.rs`, `visitor.rs`, `check.rs`, and integration tests
- Watch mode now shows changed file paths and clears screen between runs

### Fixed
- SARIF output now includes `tool.driver.version` and `$schema` fields
- Conformance workflow JSON output handling

## [1.3.1] - 2026-03-23

### Added
- `dupes --changed-since`: duplication detection now supports `--changed-since` to only report clone groups involving changed files
- Angular plugin `resolve_config()`: parses `angular.json` to extract `styles`, `scripts`, `main`, `browser`, and `polyfills` from build targets as entry points; adds Angular peer dependencies (`rxjs`, `@angular/common`, `@angular/platform-browser`, `@angular/build`) to tooling deps; widens entry patterns for Nx monorepo layouts
- Nx plugin `resolve_config()`: parses `project.json` to extract executor references as dependencies and `options.main` as entry points; adds `@nx/angular`, `@nx/storybook`, `@nx/webpack`, and other `@nx/*` packages to tooling deps
- JSON config file filesystem fallback: plugins with JSON config patterns (e.g., `angular.json`, `**/project.json`) are now discovered via filesystem scan when they're not in the JS/TS discovered file set; workspace-level plugins also check the project root for config files
- File-based plugin activation for ESLint and Vitest: plugins now activate when their config files exist in a workspace, not just when the package is in `dependencies`. Fixes false positives in monorepos where `eslint`/`vitest` are only in the root `package.json`
- Vitest plugin marks `setupTests.{ts,tsx,js,jsx}` and `test-setup.{ts,tsx,js,jsx}` as always-used when active, fixing false positives for test setup files referenced via imported/spread base configs
- Nested package entry discovery now searches `services/`, `tools/`, and `utils/` directories in addition to `packages/`, `apps/`, `libs/`, `modules/`, `plugins/`
- Infrastructure entry point detection: Dockerfiles, Procfiles, and `fly.toml` are scanned for source file references

### Fixed
- Bare specifier cache poisoning: the resolver cache for bare specifiers (e.g., `@scope/pkg`) now only caches results from successful `oxc_resolver` resolution; previously, when resolution failed for a tsconfig path alias that looked like an npm package, the `NpmPackage` fallback was cached and prevented correct resolution for all subsequent files
- Production dependency false positives: `plugin_tooling` dependencies (e.g., `zone.js`, `@angular/compiler`) are now excluded from unused production dependency detection, matching the existing dev dependency behavior
- Workspace entry pattern double-prefixing: entry patterns from `resolve_config()` that are already project-root-relative (e.g., `apps/client/src/styles.css` from `angular.json`) are no longer double-prefixed with the workspace path
- `check --changed-since` now filters all issue types: unlisted dependencies, duplicate exports, and circular dependencies were previously unfiltered
- Duplication stats recomputation: `recompute_stats` now deduplicates overlapping line ranges per file (matching the original `compute_stats` logic), fixing inflated `duplicated_lines` counts after baseline or `--changed-since` filtering
- Plugin entry point attribution: entry points discovered by plugins now show the correct plugin name (e.g., `Plugin { name: "nextjs" }`) instead of the generic `Plugin { name: "plugin" }`

## [1.3.0] - 2026-03-22

### Added
- Unused optionalDependencies detection: packages in `optionalDependencies` never imported or used as script binaries are now reported
- Function overload deduplication: multiple overload signatures for the same function name are deduplicated to avoid false positive unused-export reports

### Changed
- Workspace discovery performance: canonicalize optimization reduces syscall overhead during workspace resolution
- Re-export chain performance: `matches_str` optimization for faster re-export chain propagation

### Fixed
- Ecosystem test script: `--root` flag and install commands corrected for cross-project validation

## [1.2.0] - 2026-03-22

### Added
- Unused import binding detection via `oxc_semantic`: imports where the bound name is never read in the importing file no longer count as references to the exported symbol, improving unused-export detection precision
- Namespace destructuring detection: `const { foo, bar } = ns` after `import * as ns` (and dynamic import / require namespaces) now correctly tracks accessed members for narrowing

### Fixed
- Namespace imports with whole-object consumption patterns (`Object.values(ns)`, `{...ns}`, `for..in ns`, `const { a, ...rest } = ns`) now correctly mark all exports as referenced instead of being skipped by the entry-point guard

## [1.1.0] - 2026-03-21

### Added
- Markdown output format (`--format markdown`)
- Oxc-inspired code quality: workspace-level clippy configuration with `all`, `pedantic`, `nursery`, `cargo` lint groups and 13 restriction lints
- `#[expect]` over `#[allow]` for all clippy suppressions (warns when suppression becomes unnecessary)
- Const size assertions on hot-path types (`ModuleNode`, `ModuleInfo`, `ExportInfo`, `ImportInfo`, `Edge`) to prevent accidental struct bloat
- VS Code extension icon and marketplace metadata

### Changed
- Module graph split into focused submodules (`types.rs`, `build.rs`, `reachability.rs`, `re_exports.rs`, `cycles.rs`)
- Dev profile optimized with `debug = false` and selective `opt-level` for proc-macro crates

### Fixed
- Windows build: restrict `ModuleNode` size assertion to Unix (`PathBuf` layout differs on Windows MSVC)

## [1.0.4] - 2026-03-21

### Fixed
- CI failures across typos, cargo-deny, docs, MSRV, and clippy
- Windows symlink support for workspace integration test

## [1.0.3] - 2026-03-20

### Fixed
- npm publish: switch to OIDC trusted publishing (no NPM_TOKEN secret needed)

## [1.0.2] - 2026-03-20

### Fixed
- npm publish: add registry authentication to release workflow

## [1.0.1] - 2026-03-20

### Fixed
- Windows build: restrict `DiscoveredFile` size assertion to unix (different `PathBuf` layout on Windows MSVC)

## [1.0.0] - 2026-03-20

### Added
- TypeScript project references: workspace discovery from `tsconfig.json` `references` field (additive with npm/pnpm workspaces, deduplicated by canonical path)
- Elementary cycle enumeration for circular dependencies (individual cycles within SCCs, max 20 per SCC, deterministic output)
- CSS Modules support (`.module.css`/`.module.scss`) with class name export tracking
- `fallow migrate` command to convert knip and jscpd configs
- CSS/SCSS file discovery with `@import`, `@use`, `@forward`, and `@apply` extraction
- Cross-workspace `exports` field subpath resolution with output→source fallback
- Pnpm content-addressable store detection for injected dependencies
- Cache-aware incremental parsing with `--performance` cache hit/miss stats
- Code Lens with export reference counts and click-to-navigate in LSP
- Duplication diagnostics and "Extract duplicate into function" code action in LSP
- VS Code extension CI, LSP binary builds, and marketplace publishing
- Nuxt `resolve_config()` for deep config parsing (modules, css, plugins, extends, postcss, path aliases)
- Circular dependency benchmarks vs madge and dpdm
- Inline suppression for circular dependencies (`fallow-ignore-file circular-dependency`)
- Backwards compatibility policy (`docs/backwards-compatibility.md`)
- JSON output schema documentation (`docs/output-schema.json`)

### Fixed
- UTF-8 boundary handling in duplication detection (multi-byte character safety)
- Exports map resolution robustness for cross-workspace imports
- Nested output subdirectory mapping (e.g., `dist/esm/utils.mjs` → `src/utils.ts`)
- Trace path matching for monorepo compatibility (canonicalized vs user-provided paths)

## [0.3.0] - 2026-03-18

### Added
- Production mode (`--production`) — excludes test/dev files, limits to production scripts, reports type-only imports
- Clone families with refactoring suggestions (extract function/module)
- Config schema generation (`fallow config-schema`) with `$schema` support for IDE autocomplete
- Duplication baselines (`--save-baseline` / `--baseline`) for incremental CI adoption
- Deep `resolve_config()` for all top 10 framework plugins (ESLint, Vite, Jest, Storybook, Tailwind, Webpack, TypeScript, Babel, Rollup, PostCSS)
- JSONC as default config format (with JSON and TOML still supported)
- Pre-commit hooks for fmt and clippy checks

### Changed
- Migrated repository to fallow-rs organization

### Fixed
- Detection accuracy: star re-export tracking, workspace plugins, JSX fallback
- GitHub Action YAML parse error and benchmark workflow

### Performance
- Optimized resolve, discovery, and duplication detection hotpaths

## [0.2.0] - 2026-03-18

### Added
- Code duplication detection (`fallow dupes`) with suffix array + LCP algorithm
- 4 detection modes: strict, mild, weak, semantic
- Duplication benchmarks comparing fallow dupes vs jscpd (4-75x faster)
- Plugin system with 40 built-in framework plugins as single source of truth
- Dynamic import resolution (template literals, `import.meta.glob`, `require.context`)
- Vue/Svelte SFC parsing with `<script>` block extraction
- Dynamic member heuristics (Object.values/keys/entries, for..in, spread)
- GitHub Action with job summaries, PR comments, and `--sarif-file` flag
- LSP diagnostics for all 10 issue types

### Fixed
- CJS require analysis and namespace narrowing for detection accuracy

### Performance
- Clone detection pipeline optimized up to 177x faster vs initial implementation

## [0.1.7] - 2026-03-17

### Added
- Roadmap and project documentation

### Changed
- Improved human report formatting

## [0.1.6] - 2026-03-17

### Fixed
- Reduced false positives based on knip comparison across 6 public projects

## [0.1.5] - 2026-03-17

### Added
- Plugin system, baseline comparison, and `fallow fix` command
- Comprehensive test coverage for high-priority gaps
- CI regression tracking benchmarks

### Fixed
- Reduced false positives based on knip comparison across 31 projects
- Byte column counting, strip_prefix, and HashSet reuse
- Deduplicated imported_from paths in unlisted dependency reports

## [0.1.4] - 2026-03-17

### Fixed
- Reduced false positives and improved re-export chain detection
- Repository URL casing for GitHub provenance

## [0.1.3] - 2026-03-17

### Fixed
- npm publish path resolution

### Changed
- Updated GitHub Actions to Node.js 24, added npm READMEs

## [0.1.2] - 2026-03-17

### Fixed
- Repository URLs and npm publish error surfacing

## [0.1.1] - 2026-03-17

### Added
- Initial public release
- Dead code analysis with 10 issue types
- npm publishing pipeline with platform-specific binaries (macOS, Linux, Windows)
- LSP server with diagnostics and code actions
- CLI commands: check, watch, fix, init, list, schema
- 4 output formats: human, JSON, SARIF, compact
- Rules system with per-issue-type severity
- Inline suppression comments
- `--changed-since` and `--fail-on-issues` for CI
- Cross-workspace resolution for npm/yarn/pnpm workspaces

[Unreleased]: https://github.com/fallow-rs/fallow/compare/v2.53.0...HEAD
[2.53.0]: https://github.com/fallow-rs/fallow/compare/v2.52.2...v2.53.0
[2.52.2]: https://github.com/fallow-rs/fallow/compare/v2.52.1...v2.52.2
[2.52.1]: https://github.com/fallow-rs/fallow/compare/v2.52.0...v2.52.1
[2.52.0]: https://github.com/fallow-rs/fallow/compare/v2.51.0...v2.52.0
[2.51.0]: https://github.com/fallow-rs/fallow/compare/v2.50.0...v2.51.0
[2.50.0]: https://github.com/fallow-rs/fallow/compare/v2.49.0...v2.50.0
[2.49.0]: https://github.com/fallow-rs/fallow/compare/v2.48.5...v2.49.0
[2.48.5]: https://github.com/fallow-rs/fallow/compare/v2.48.4...v2.48.5
[2.48.4]: https://github.com/fallow-rs/fallow/compare/v2.48.3...v2.48.4
[2.48.3]: https://github.com/fallow-rs/fallow/compare/v2.48.2...v2.48.3
[2.48.2]: https://github.com/fallow-rs/fallow/compare/v2.48.1...v2.48.2
[2.48.1]: https://github.com/fallow-rs/fallow/compare/v2.48.0...v2.48.1
[2.48.0]: https://github.com/fallow-rs/fallow/compare/v2.47.1...v2.48.0
[2.47.1]: https://github.com/fallow-rs/fallow/compare/v2.47.0...v2.47.1
[2.47.0]: https://github.com/fallow-rs/fallow/compare/v2.46.0...v2.47.0
[2.46.0]: https://github.com/fallow-rs/fallow/compare/v2.45.1...v2.46.0
[2.45.1]: https://github.com/fallow-rs/fallow/compare/v2.45.0...v2.45.1
[2.45.0]: https://github.com/fallow-rs/fallow/compare/v2.44.2...v2.45.0
[2.44.2]: https://github.com/fallow-rs/fallow/compare/v2.44.1...v2.44.2
[2.44.1]: https://github.com/fallow-rs/fallow/compare/v2.44.0...v2.44.1
[2.44.0]: https://github.com/fallow-rs/fallow/compare/v2.43.0...v2.44.0
[2.43.0]: https://github.com/fallow-rs/fallow/compare/v2.42.0...v2.43.0
[2.42.0]: https://github.com/fallow-rs/fallow/compare/v2.41.0...v2.42.0
[2.41.0]: https://github.com/fallow-rs/fallow/compare/v2.40.3...v2.41.0
[2.40.3]: https://github.com/fallow-rs/fallow/compare/v2.40.2...v2.40.3
[2.40.2]: https://github.com/fallow-rs/fallow/compare/v2.40.1...v2.40.2
[2.40.1]: https://github.com/fallow-rs/fallow/compare/v2.40.0...v2.40.1
[2.40.0]: https://github.com/fallow-rs/fallow/compare/v2.39.0...v2.40.0
[2.39.0]: https://github.com/fallow-rs/fallow/compare/v2.38.0...v2.39.0
[2.38.0]: https://github.com/fallow-rs/fallow/compare/v2.37.0...v2.38.0
[2.37.0]: https://github.com/fallow-rs/fallow/compare/v2.36.0...v2.37.0
[2.36.0]: https://github.com/fallow-rs/fallow/compare/v2.35.0...v2.36.0
[2.35.0]: https://github.com/fallow-rs/fallow/compare/v2.34.0...v2.35.0
[2.34.0]: https://github.com/fallow-rs/fallow/compare/v2.33.0...v2.34.0
[2.33.0]: https://github.com/fallow-rs/fallow/compare/v2.32.1...v2.33.0
[2.32.1]: https://github.com/fallow-rs/fallow/compare/v2.32.0...v2.32.1
[2.32.0]: https://github.com/fallow-rs/fallow/compare/v2.31.0...v2.32.0
[2.31.0]: https://github.com/fallow-rs/fallow/compare/v2.30.0...v2.31.0
[2.30.0]: https://github.com/fallow-rs/fallow/compare/v2.29.1...v2.30.0
[2.29.1]: https://github.com/fallow-rs/fallow/compare/v2.29.0...v2.29.1
[2.29.0]: https://github.com/fallow-rs/fallow/compare/v2.28.2...v2.29.0
[2.28.2]: https://github.com/fallow-rs/fallow/compare/v2.28.1...v2.28.2
[2.28.1]: https://github.com/fallow-rs/fallow/compare/v2.28.0...v2.28.1
[2.28.0]: https://github.com/fallow-rs/fallow/compare/v2.27.6...v2.28.0
[2.27.6]: https://github.com/fallow-rs/fallow/compare/v2.27.5...v2.27.6
[2.27.5]: https://github.com/fallow-rs/fallow/compare/v2.27.4...v2.27.5
[2.27.4]: https://github.com/fallow-rs/fallow/compare/v2.27.3...v2.27.4
[2.27.3]: https://github.com/fallow-rs/fallow/compare/v2.27.2...v2.27.3
[2.27.2]: https://github.com/fallow-rs/fallow/compare/v2.27.1...v2.27.2
[2.27.1]: https://github.com/fallow-rs/fallow/compare/v2.27.0...v2.27.1
[2.27.0]: https://github.com/fallow-rs/fallow/compare/v2.26.1...v2.27.0
[2.26.1]: https://github.com/fallow-rs/fallow/compare/v2.26.0...v2.26.1
[2.26.0]: https://github.com/fallow-rs/fallow/compare/v2.25.1...v2.26.0
[2.25.1]: https://github.com/fallow-rs/fallow/compare/v2.25.0...v2.25.1
[2.25.0]: https://github.com/fallow-rs/fallow/compare/v2.24.0...v2.25.0
[2.24.0]: https://github.com/fallow-rs/fallow/compare/v2.23.1...v2.24.0
[2.23.1]: https://github.com/fallow-rs/fallow/compare/v2.23.0...v2.23.1
[2.23.0]: https://github.com/fallow-rs/fallow/compare/v2.22.4...v2.23.0
[2.22.4]: https://github.com/fallow-rs/fallow/compare/v2.22.3...v2.22.4
[2.22.3]: https://github.com/fallow-rs/fallow/compare/v2.22.2...v2.22.3
[2.22.2]: https://github.com/fallow-rs/fallow/compare/v2.22.1...v2.22.2
[2.22.1]: https://github.com/fallow-rs/fallow/compare/v2.22.0...v2.22.1
[2.22.0]: https://github.com/fallow-rs/fallow/compare/v2.21.0...v2.22.0
[2.21.0]: https://github.com/fallow-rs/fallow/compare/v2.20.0...v2.21.0
[2.20.0]: https://github.com/fallow-rs/fallow/compare/v2.19.3...v2.20.0
[2.19.3]: https://github.com/fallow-rs/fallow/compare/v2.19.2...v2.19.3
[2.19.2]: https://github.com/fallow-rs/fallow/compare/v2.19.1...v2.19.2
[2.19.1]: https://github.com/fallow-rs/fallow/compare/v2.19.0...v2.19.1
[2.19.0]: https://github.com/fallow-rs/fallow/compare/v2.18.3...v2.19.0
[2.18.3]: https://github.com/fallow-rs/fallow/compare/v2.18.2...v2.18.3
[2.18.2]: https://github.com/fallow-rs/fallow/compare/v2.18.1...v2.18.2
[2.18.1]: https://github.com/fallow-rs/fallow/compare/v2.18.0...v2.18.1
[2.18.0]: https://github.com/fallow-rs/fallow/compare/v2.17.1...v2.18.0
[2.17.1]: https://github.com/fallow-rs/fallow/compare/v2.17.0...v2.17.1
[2.17.0]: https://github.com/fallow-rs/fallow/compare/v2.16.0...v2.17.0
[2.16.0]: https://github.com/fallow-rs/fallow/compare/v2.15.0...v2.16.0
[2.15.0]: https://github.com/fallow-rs/fallow/compare/v2.14.2...v2.15.0
[2.14.2]: https://github.com/fallow-rs/fallow/compare/v2.14.1...v2.14.2
[2.14.1]: https://github.com/fallow-rs/fallow/compare/v2.14.0...v2.14.1
[2.14.0]: https://github.com/fallow-rs/fallow/compare/v2.13.4...v2.14.0
[2.13.4]: https://github.com/fallow-rs/fallow/compare/v2.13.3...v2.13.4
[2.13.3]: https://github.com/fallow-rs/fallow/compare/v2.13.2...v2.13.3
[2.13.2]: https://github.com/fallow-rs/fallow/compare/v2.13.1...v2.13.2
[2.13.1]: https://github.com/fallow-rs/fallow/compare/v2.13.0...v2.13.1
[2.13.0]: https://github.com/fallow-rs/fallow/compare/v2.12.1...v2.13.0
[2.12.1]: https://github.com/fallow-rs/fallow/compare/v2.12.0...v2.12.1
[2.12.0]: https://github.com/fallow-rs/fallow/compare/v2.11.0...v2.12.0
[2.11.0]: https://github.com/fallow-rs/fallow/compare/v2.10.1...v2.11.0
[2.10.1]: https://github.com/fallow-rs/fallow/compare/v2.10.0...v2.10.1
[2.10.0]: https://github.com/fallow-rs/fallow/compare/v2.9.3...v2.10.0
[2.9.3]: https://github.com/fallow-rs/fallow/compare/v2.9.2...v2.9.3
[2.9.2]: https://github.com/fallow-rs/fallow/compare/v2.9.1...v2.9.2
[2.9.1]: https://github.com/fallow-rs/fallow/compare/v2.9.0...v2.9.1
[2.9.0]: https://github.com/fallow-rs/fallow/compare/v2.8.1...v2.9.0
[2.8.1]: https://github.com/fallow-rs/fallow/compare/v2.8.0...v2.8.1
[2.8.0]: https://github.com/fallow-rs/fallow/compare/v2.7.3...v2.8.0
[2.7.3]: https://github.com/fallow-rs/fallow/compare/v2.7.2...v2.7.3
[2.7.2]: https://github.com/fallow-rs/fallow/compare/v2.7.1...v2.7.2
[2.7.1]: https://github.com/fallow-rs/fallow/compare/v2.7.0...v2.7.1
[2.7.0]: https://github.com/fallow-rs/fallow/compare/v2.6.0...v2.7.0
[2.6.0]: https://github.com/fallow-rs/fallow/compare/v2.5.5...v2.6.0
[2.5.5]: https://github.com/fallow-rs/fallow/compare/v2.5.4...v2.5.5
[2.5.4]: https://github.com/fallow-rs/fallow/compare/v2.5.3...v2.5.4
[2.5.3]: https://github.com/fallow-rs/fallow/compare/v2.5.2...v2.5.3
[2.5.2]: https://github.com/fallow-rs/fallow/compare/v2.5.1...v2.5.2
[2.5.1]: https://github.com/fallow-rs/fallow/compare/v2.5.0...v2.5.1
[2.5.0]: https://github.com/fallow-rs/fallow/compare/v2.4.0...v2.5.0
[2.4.0]: https://github.com/fallow-rs/fallow/compare/v2.3.1...v2.4.0
[2.3.1]: https://github.com/fallow-rs/fallow/compare/v2.3.0...v2.3.1
[2.3.0]: https://github.com/fallow-rs/fallow/compare/v2.2.3...v2.3.0
[2.2.3]: https://github.com/fallow-rs/fallow/compare/v2.2.1...v2.2.3
[2.2.1]: https://github.com/fallow-rs/fallow/compare/v2.2.0...v2.2.1
[2.2.0]: https://github.com/fallow-rs/fallow/compare/v2.1.0...v2.2.0
[2.1.0]: https://github.com/fallow-rs/fallow/compare/v2.0.1...v2.1.0
[2.0.1]: https://github.com/fallow-rs/fallow/compare/v2.0.0...v2.0.1
[2.0.0]: https://github.com/fallow-rs/fallow/compare/v1.9.0...v2.0.0
[1.9.0]: https://github.com/fallow-rs/fallow/compare/v1.8.1...v1.9.0
[1.8.1]: https://github.com/fallow-rs/fallow/compare/v1.8.0...v1.8.1
[1.8.0]: https://github.com/fallow-rs/fallow/compare/v1.7.0...v1.8.0
[1.7.0]: https://github.com/fallow-rs/fallow/compare/v1.6.1...v1.7.0
[1.6.1]: https://github.com/fallow-rs/fallow/compare/v1.6.0...v1.6.1
[1.6.0]: https://github.com/fallow-rs/fallow/compare/v1.5.0...v1.6.0
[2.1.0]: https://github.com/fallow-rs/fallow/compare/v2.0.1...v2.1.0
[1.5.0]: https://github.com/fallow-rs/fallow/compare/v1.4.0...v1.5.0
[1.4.0]: https://github.com/fallow-rs/fallow/compare/v1.3.1...v1.4.0
[1.3.1]: https://github.com/fallow-rs/fallow/compare/v1.3.0...v1.3.1
[1.3.0]: https://github.com/fallow-rs/fallow/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/fallow-rs/fallow/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/fallow-rs/fallow/compare/v1.0.4...v1.1.0
[1.0.4]: https://github.com/fallow-rs/fallow/compare/v1.0.3...v1.0.4
[1.0.3]: https://github.com/fallow-rs/fallow/compare/v1.0.2...v1.0.3
[1.0.2]: https://github.com/fallow-rs/fallow/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/fallow-rs/fallow/compare/v1.0.0...v1.0.1
[2.3.0]: https://github.com/fallow-rs/fallow/compare/v2.2.3...v2.3.0
[2.2.3]: https://github.com/fallow-rs/fallow/compare/v2.2.2...v2.2.3
[2.2.2]: https://github.com/fallow-rs/fallow/compare/v2.2.1...v2.2.2
[1.0.0]: https://github.com/fallow-rs/fallow/compare/v0.3.0...v1.0.0
[0.3.0]: https://github.com/fallow-rs/fallow/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/fallow-rs/fallow/compare/v0.1.7...v0.2.0
[0.1.7]: https://github.com/fallow-rs/fallow/compare/v0.1.6...v0.1.7
[0.1.6]: https://github.com/fallow-rs/fallow/compare/v0.1.5...v0.1.6
[0.1.5]: https://github.com/fallow-rs/fallow/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/fallow-rs/fallow/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/fallow-rs/fallow/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/fallow-rs/fallow/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/fallow-rs/fallow/releases/tag/v0.1.1
