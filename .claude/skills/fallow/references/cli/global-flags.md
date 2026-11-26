# Global Flags

Available on all commands:

<!-- generated:flags:global:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `path` | `string` | - | Scope reported findings to this file or directory (default: whole project). The full project graph is still built; only reported items are narrowed |
| `-r, --root` | `string` | - | Project root directory |
| `-c, --config` | `string` | - | Config file path |
| `--allow-remote-extends` | `bool` | `false` | Allow trusted config files to extend HTTPS URLs |
| `-f, --format` | `human\|json\|sarif\|compact\|markdown\|codeclimate\|pr-comment-github\|pr-comment-gitlab\|review-github\|review-gitlab\|badge\|github-annotations\|github-summary` | `human` | Output format (alias: --output) |
| `--pretty` | `bool` | `false` | Indent JSON output for manual inspection. Requires the final output format to be JSON |
| `--lean` | `bool` | `false` | Trim the JSON of dead-code, health and the combined report for agent context: leave out empty arrays and zero dead-code summary counts, group workspace diagnostics, cap health tables, and move repeated action text to `_meta.actions`. Other commands and other output formats exit with code 2. Run without `--lean` for the full diagnostic list and the complete tables |
| `--full-tables` | `bool` | `false` | With `--lean`, keep every row of the health tables (`hotspots`, `file_scores`, `large_functions`) |
| `-q, --quiet` | `bool` | `false` | Suppress progress output; `audit` also prints only actionable findings |
| `--changed-since` | `string` | - | Only report issues in files changed since this git ref (e.g., main, HEAD~5) |
| `--no-package-baselines` | `bool` | `false` | Ignore the per-package refs of `workspaces.changedSince` for this run |
| `--diff-file` | `string` | - | Unified diff for line-level scoping. Use `-` to read from stdin. Project-level findings still bypass this filter. When both this and `--changed-since` are set, the diff filter wins for finding scope while `--changed-since` still drives file discovery |
| `--diff-stdin` | `bool` | `false` | Read the unified diff from stdin. Equivalent to `--diff-file -` |
| `--churn-file` | `string` | - | Import change history from a `fallow-churn/v1` JSON file instead of `git log`, powering hotspots, ownership, and bus-factor on projects with no git repository (Yandex Arc, Mercurial, Perforce). A small wrapper translates your VCS log into the contract. Resolved relative to `--root`. Affects `health --hotspots` / `--ownership` / `--targets` only; `audit`, `impact`, and `--changed-since` still require git |
| `--max-file-size` | `string` | - | Skip source files larger than this many megabytes (default 5) instead of parsing them, guarding against the out-of-memory blowup a single multi-MB generated/vendored/bundled file causes on large repos. Use `0` for no limit. Declaration files (`.d.ts`) are always analyzed. Skipped files are reported and excluded from every analysis. Also settable via `FALLOW_MAX_FILE_SIZE` |
| `--baseline` | `string` | - | Compare to baseline. On bare `fallow` it holds the dead-code baseline only; use `--health-baseline` and `--dupes-baseline` for the others |
| `--baseline-mode` | `count\|identity` | - | How `--baseline` matches health findings: per file and category (`count`, the default) or per function identity (`identity`, strict, and only against a baseline saved with `--baseline-mode identity`; such a baseline still reads in count mode). Identity is file path plus function name, so renaming or moving a function that is still in the baseline reports it as new; re-save after that kind of refactor. |
| `--parent-run` | `string` | - | Correlate this run with a previous telemetry analysis run |
| `--save-baseline` | `string` | - | Save results as baseline. On bare `fallow` it saves the dead-code baseline only |
| `--production` | `bool` | `false` | Exclude test/dev files, only start/build scripts (applies to every analysis) |
| `--no-production` | `bool` | `false` | Force production mode OFF for every analysis, overriding a project config's `production: true` (and `FALLOW_PRODUCTION`). Conflicts with `--production` |
| `--production-dead-code` | `bool` | `false` | Run dead-code analysis in production mode when using bare combined mode |
| `--production-health` | `bool` | `false` | Run health analysis in production mode when using bare combined mode |
| `--production-dead-code` / `--production-health` / `--production-dupes` | `bool` | `false` | Per-analysis production mode for bare combined runs and `fallow audit`. Per-analysis env vars `FALLOW_PRODUCTION_DEAD_CODE`/`HEALTH`/`DUPES` mirror these flags. Per-analysis env beats global `FALLOW_PRODUCTION`. |
| `-w, --workspace` | `string` | - | Scope to one or more workspaces (comma-separated, globs, `!` negation) |
| `--changed-workspaces` | `string` | - | Git-derived monorepo CI scoping: scope to workspaces containing any file changed since `REF`. Mutually exclusive with `--workspace`. Missing ref is a hard error. |
| `--group-by` | `owner\|directory\|package\|section` | - | Group output by CODEOWNERS ownership (`owner`), first path component (`directory`), workspace package (`package`, aliases: `workspace`, `pkg`), or GitLab CODEOWNERS `[Section]` headers (`section`, alias: `gl-section`). All output formats partition issues into labeled groups. `section` mode attaches an `owners` array to each group in JSON output |
| `--group` | `string` | - | Keep only the matching groups of a `--group-by` health run. Accepts exact group keys, glob patterns, and `!`-prefixed negations. Values can be comma-separated or repeated. Project-level sections are not filtered. Supported by `fallow health` only |
| `--explain` | `bool` | `false` | JSON: include metric definitions in `_meta`. Human: print a `Description:` line under each section header. Always on for MCP. |
| `--explain-skipped` | `bool` | `false` | Human/markdown only: show per-pattern counts for files skipped by the default duplicates ignores. `dupes` prints only that breakdown; on `check`, `dead-code`, `audit` and the default run the same flag reports source files the built-in discovery ignores removed |
| `--summary` | `bool` | `false` | Show only category counts without individual items. Useful for dashboards and quick overviews |
| `--ci` | `bool` | `false` | CI mode: `--format sarif --fail-on-issues --quiet` |
| `--fail-on-issues` | `bool` | `false` | Exit 1 if any issues found (promotes `warn` to `error`) |
| `--sarif-file` | `string` | - | Write SARIF output to a file (in addition to the primary --format output). Used by bare `fallow`, `dead-code`, `architecture` and `security`. The path must resolve inside the project root, its Git work tree, the CI workspace or a temp directory |
| `-o, --output-file` | `string` | - | Write the report to a file instead of stdout, for any --format (no ANSI codes). Useful on large projects where the terminal scrollback truncates the top. Progress and the confirmation stay on stderr |
| `--report-path-prefix` | `string` | - | Prefix prepended to every path in the CI-facing formats (`github-annotations`, `github-summary`, `codeclimate`, `review-github`, `review-gitlab`). CI platforms address files by repository-root-relative path, so when the analyzed project lives in a subdirectory (e.g. `packages/app/`), paths need that offset. fallow detects the offset via the git toplevel automatically; this flag overrides the detection. Pass an empty string to disable rebasing and emit paths relative to `--root` |
| `--fail-on-regression` | `bool` | `false` | Fail if issue count increased beyond tolerance vs a regression baseline |
| `--fail-on-stale-baseline` | `bool` | `false` | Exit with code 1 if a loaded --baseline has entries that match nothing in this run. Judges a dead-code baseline in the mode that the file records. Inert when the scope of the run differs from the file, and on `health` and `dupes` in production mode |
| `--fail-on-baseline-growth` | `bool` | `false` | Exit with code 1 if a loaded baseline has a key that the same file at the base ref does not have |
| `--baseline-base` | `string` | - | The git ref that --fail-on-baseline-growth compares the baseline with |
| `--fail-on-parse-error` | `bool` | `false` | Exit with code 1 if fallow could not parse a source file cleanly |
| `--tolerance` | `string` | `0` | Allowed increase: `"2%"` (percentage) or `"5"` (absolute). Default: `"0"` |
| `--regression-baseline` | `string` | - | Path to a standalone regression baseline file. Without it, fallow uses `regression.baseline` from the config |
| `--save-regression-baseline` | `string` | - | Save current issue counts. With no path, update `regression.baseline` in the discovered fallow config or create `.fallowrc.json`; with a path, write a standalone baseline file |
| `--only` | `dead-code\|dupes\|health\|architecture` | - | Run only specific analyses (e.g., `--only dead-code,dupes`). Values: `dead-code` (alias: `check`), `dupes`, `health`, `architecture`. `architecture` alone runs the dead-code analysis and reports only import cycles, boundary violations and policy violations |
| `--skip` | `dead-code\|dupes\|health\|architecture` | - | Skip specific analyses (e.g., `--skip health`). Values: `dead-code` (alias: `check`), `dupes`, `health`, `architecture`. `architecture` alone removes import cycles, boundary violations and policy violations from the dead-code section |
| `--dupes-mode` | `strict\|mild\|weak\|semantic` | - | Override duplication detection mode in combined mode |
| `--dupes-near` | `bool` | `false` | Enable function-scoped near-miss clone detection in combined mode |
| `--dupes-threshold` | `string` | - | Override duplication threshold in combined mode |
| `--dupes-min-tokens` | `string` | - | Override the minimum token count for clones in combined mode |
| `--dupes-min-lines` | `string` | - | Override the minimum line count for clones in combined mode |
| `--dupes-min-occurrences` | `string` | - | Override the minimum clone occurrences in combined mode (must be >= 2) |
| `--dupes-skip-local` | `bool` | `false` | Only report cross-directory duplicates in combined mode |
| `--dupes-cross-language` | `bool` | `false` | Enable cross-language duplicate detection in combined mode |
| `--dupes-ignore-imports` | `bool` | `false` | Exclude module wiring from duplicate detection in combined mode |
| `--dupes-no-ignore-imports` | `bool` | `false` | Count module wiring as clone candidates in combined mode (opt out of the default exclusion) |
| `--dupes-ignore-symlinks` | `bool` | `false` | Omit clone instances whose path is a symlink, or lies under a symlinked directory, in combined mode |
| `--dupes-no-ignore-symlinks` | `bool` | `false` | Report symlinked clone instances in combined mode (opt out of a config `duplicates.ignoreSymlinks: true`) |
| `--score` | `bool` | `false` | Compute health score (0-100 with letter grade) in combined mode. Enables the health delta header in PR comments. JSON includes `health_score` object with `score`, `grade`, and `penalties` breakdown |
| `--trend` | `bool` | `false` | Compare current health metrics against saved snapshot. Implies `--score`. Shows per-metric deltas with directional indicators. Requires at least one saved snapshot in `.fallow/snapshots/` |
| `--trend-from` | `string` | - | Compare current health metrics against this snapshot file in combined mode. Implies --trend and --score |
| `--save-snapshot` | `string` | - | Save vital signs snapshot for trend tracking. Default path: `.fallow/snapshots/<timestamp>.json`. Forces file-scores + hotspot computation |
| `--coverage` | `string` | - | Path to Istanbul or raw V8 coverage data for measured CRAP scores in combined mode. Also settable via `FALLOW_COVERAGE` or `health.coverage` |
| `--coverage-root` | `string` | - | Absolute prefix to strip from Istanbul file paths in combined mode. Also settable via `FALLOW_COVERAGE_ROOT` or `health.coverageRoot` |
| `--dupes-baseline` | `string` | - | Compare duplication clone groups against a saved baseline in combined mode (produced by `fallow dupes --save-baseline`) |
| `--health-baseline` | `string` | - | Compare health findings against a saved baseline in combined mode (produced by `fallow health --save-baseline`) |
| `--include-entry-exports` | `bool` | `false` | Report unused exports in entry files instead of auto-marking them as used |
| `--show-cascade` | `bool` | `false` | Also report the unused exports, types, class members and enum members of unused files. By default the report hides them, because deleting the file removes them, and counts them in `cascade_hidden` |
| `--type-aware` | `bool` | `false` | Opt in to TypeScript semantic analysis for project-wide symbol evidence. This does not emit compiler diagnostics or typed lint findings |
| `--no-type-aware` | `bool` | `false` | Disable TypeScript semantic analysis even when `typeAware.enabled` or `FALLOW_TYPE_AWARE` opts in, keeping this run fully syntactic |
| `--type-aware-project` | `string` | - | TypeScript project config to use for type-aware analysis (repeatable) |
| `--type-aware-require` | `best-effort\|complete` | - | Decide whether incomplete type-aware analysis is advisory or gating |
| `--no-cache` | `bool` | `false` | Disable incremental caching |
| `--threads` | `string` | - | Number of parser threads |
| `--performance` | `bool` | `false` | Show pipeline timing breakdown |
<!-- generated:flags:global:end -->

Type-aware candidate decisions are `confirmed-used`, `contract-preserved`,
`confirmed-no-static-references`, `retained-abstained`, or
`retained-unresolved`. The first two remove a syntactic false positive.
Complete negative evidence keeps the finding and only makes a class member
automatically fixable when every owning project is complete, no contract or
dynamic gap exists, and the exact declaration hash still matches.
`fallow fix --type-aware --dry-run --format json --quiet` previews these
guarded edits.

## Combined Mode Flags

<!-- generated:flags:fallow-combined:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--only` | `dead-code\|dupes\|health\|architecture` | - | Run only specific analyses when no subcommand is given |
| `--skip` | `dead-code\|dupes\|health\|architecture` | - | Skip specific analyses when no subcommand is given |
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
| `--coverage` | `string` | - | Path to Istanbul or raw V8 coverage data for measured CRAP scores in combined mode. Also settable via `FALLOW_COVERAGE` or `health.coverage` |
| `--coverage-root` | `string` | - | Absolute prefix to strip from Istanbul file paths in combined mode. Also settable via `FALLOW_COVERAGE_ROOT` or `health.coverageRoot` |

These are global flags with behavior specific to bare `fallow` combined mode.
<!-- generated:flags:fallow-combined:end -->
