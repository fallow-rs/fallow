# Backwards Compatibility Policy

Starting with v1.0, fallow follows [semantic versioning](https://semver.org/).

## What is stable

These interfaces are covered by semver — breaking changes only happen in major version bumps:

### Configuration format

- **Config file names**: `.fallowrc.json`, `.fallowrc.jsonc`, `fallow.toml`, `.fallow.toml`
- **All documented config fields**: `extends`, `ignorePatterns`, `rules`, `overrides`, `entry`, `ignoreDependencies`, `ignoreExports`, `ignoreExportsUsedInFile`, `includeEntryExports`, `duplicates`, `audit`, `production` (boolean form `production: true` or per-analysis form `production: { deadCode, health, dupes }`), `framework`, `workspaces`, `plugins`, `boundaries` (including `boundaries.preset`)
- **Rule names and severity values**: `unused-files`, `unused-exports`, etc. with `error`/`warn`/`off`
- **Extends and overrides semantics**: merge behavior, glob matching, override precedence, `npm:` prefix resolution, `https://` URL resolution
- **Inline suppression comment syntax**: `fallow-ignore-next-line`, `fallow-ignore-file`

### JSON output schema

- **Top-level structure**: `schema_version`, `version`, `elapsed_ms`, `total_issues`, and all issue arrays
- **Issue type arrays**: `unused_files`, `unused_exports`, `unused_types`, `private_type_leaks`, `unused_dependencies`, `unused_dev_dependencies`, `unused_enum_members`, `unused_class_members`, `unresolved_imports`, `unlisted_dependencies`, `duplicate_exports`, `type_only_dependencies`, `circular_dependencies`, `boundary_violations`
- **Issue object fields**: all fields documented in `docs/output-schema.json`
- **Schema version**: the `schema_version` field follows its own versioning (independent of the tool version). The schema version is bumped when the JSON output structure changes.

### CLI interface

- **Subcommands**: `dead-code` (legacy alias: `check`), `dupes`, `health`, `audit`, `explain`, `fix`, `watch`, `init`, `hooks`, `setup-hooks`, `migrate`, `list`, `schema`, `config-schema`, `plugin-schema`, `config`, `coverage`, `license`, `ci`
- **`coverage` subcommands**: `setup`, `analyze`, `upload-source-maps`, `upload-inventory`. `analyze` accepts `--runtime-coverage <path>` for local mode and `--cloud` / `--runtime-coverage-cloud` (or `FALLOW_RUNTIME_COVERAGE_SOURCE=cloud`) for explicit cloud-pull; `FALLOW_API_KEY` alone never selects cloud mode.
- **`license` subcommands**: `activate`, `status`, `refresh`, `deactivate`, `trial`. JWT verification is offline-only; `activate` and `refresh` are the only network-touching operations.
- **Default behavior**: bare `fallow` (no subcommand) runs dead-code + dupes + health combined
- **Exit codes**: 0 (success/no errors), 1 (issues with error severity found), 2 (runtime error). `fallow audit` defaults to `--gate new-only`, so inherited error-severity findings in changed files can be reported with exit 0; use `--gate all` to fail on every finding in changed files.
- **Global flags**: `--format`, `--config`, `--workspace`, `--production`, `--baseline`, `--save-baseline`, `--no-cache`, `--threads`, `--changed-since` (alias: `--base`), `--performance`, `--explain`, `--ci`, `--fail-on-issues`, `--sarif-file`, `--fail-on-regression`, `--tolerance`, `--regression-baseline`, `--save-regression-baseline`, `--summary`, `--group-by` (owner, directory, package, section), `--include-entry-exports`
- **Per-analysis production flags**: `--production-dead-code`, `--production-health`, `--production-dupes` (bare combined mode and `fallow audit`)
- **Bare command flags**: `--only`, `--skip` (select which analyses to run), `--score` (health score in combined mode), `--trend` (compare against snapshot), `--save-snapshot` (save vital signs for trend tracking)
- **Health flags**: `--score` (project health score 0-100 with letter grade), `--min-score` (CI quality gate), `--max-cyclomatic` / `--max-cognitive` / `--max-crap` (per-function complexity thresholds; CRAP combines complexity with coverage), `--targets` (refactoring recommendations), `--effort` (filter targets by effort level: low/medium/high), `--coverage-gaps` (static test coverage gaps), `--coverage` (Istanbul coverage data for accurate CRAP scores), `--coverage-root` (absolute coverage-data prefix for CI rebasing), `--save-snapshot` (saves vital signs snapshot for trend tracking), `--trend` (compare against most recent snapshot)
- **Audit flags**: `--gate <new-only|all>` (controls whether only introduced findings or all findings affect the verdict), `--max-crap` (forwarded to the health sub-analysis; mirrors `health.maxCrap` in config), `--coverage` (Istanbul coverage data for accurate CRAP scores), `--coverage-root` (absolute coverage-data prefix for CI rebasing)
- **Init flags**: `--toml`, `--hooks` (scaffold pre-commit git hook), `--branch` (fallback base branch/ref for the hook when no upstream is set)
- **Hooks command**: `hooks install|uninstall --target <git|agent>` manages Git pre-commit hooks and agent gates. `setup-hooks` remains supported as the legacy agent-hook command.
- **Environment variables**: `FALLOW_FORMAT`, `FALLOW_QUIET`, `FALLOW_BIN`, `FALLOW_TIMEOUT_SECS`, `FALLOW_EXTENDS_TIMEOUT_SECS`, `FALLOW_COVERAGE`, `FALLOW_PRODUCTION`, `FALLOW_PRODUCTION_DEAD_CODE`, `FALLOW_PRODUCTION_HEALTH`, `FALLOW_PRODUCTION_DUPES`
- **Generated hook-script env vars**: `FALLOW_GATE_MIN_VERSION` (consumed by `.claude/hooks/fallow-gate.sh` written by `fallow hooks install --target agent` / `fallow setup-hooks`; controls the minimum fallow version the gate accepts, default `2.46.0`, empty string disables)

### External plugin format

- **Plugin file structure**: as documented in `docs/plugin-authoring.md`
- **Detection types**: `dependency`, `fileExists`, `all`, `any`

## What may change in minor/patch versions

These are explicitly **not** covered by the stability guarantee:

- **New fields** may be added to config, JSON output, or plugin format (additive changes)
- **New issue types** may be added
- **New plugins** may be added to the built-in set
- **Detection accuracy**: false positive/negative rates may improve
- **Human-readable output**: terminal formatting, colors, wording
- **Performance characteristics**: timing, memory usage, parallelism
- **SARIF output details**: beyond what the SARIF spec requires
- **LSP protocol details**: diagnostics, code actions, Code Lens behavior
- **Internal crate APIs**: `fallow-core`, `fallow-config`, etc. are not public API

## Deprecation process

When a stable interface needs to change:

1. The old behavior is deprecated with a warning in the current major version
2. The new behavior is available alongside the old one
3. The old behavior is removed in the next major version

## Notable behavior changes within v2

These are documented for the rare CI script that depended on the old behavior. None require a config migration.

- **`fallow health --hotspots --format json` outside a git repository now exits 0** (was exit 2). Missing git history is treated as unavailable hotspot data: the `hotspots` array is omitted (empty) and `hotspot_summary` is omitted, with a non-fatal `note: hotspot analysis skipped: no git repository found at project root` on stderr (suppressed by `--quiet`). Combined-mode `--format json` always emits exactly one JSON document on stdout regardless of git state. CI scripts that asserted exit 2 to detect "no git repo" should inspect `hotspot_summary` (absent when not analyzed, present otherwise) instead. Fixed in [#297](https://github.com/fallow-rs/fallow/pull/297).
- **`--coverage` paths now resolve relative to `--root`; `--coverage-root` must be absolute**. `fallow health --coverage relative/path.json --root sub-project/` (and the same flags on `fallow audit`) used to look for `cwd/relative/path.json`, breaking monorepo CI runs that invoke fallow from the workspace root with a sub-project `--root`. Relative `--coverage` paths now resolve under `--root` like every other project input, so the same invocation finds `sub-project/relative/path.json`. `--coverage-root` is different: it strips a prefix from paths inside the coverage data, so relative values such as `src` are rejected. Pass the absolute source prefix from the machine that generated coverage, for example `/home/runner/work/myapp`.

## Config format migration

The `fallow migrate` command helps migrate between config formats. When breaking config changes happen in a major version, `migrate` will be updated to handle the transition.
