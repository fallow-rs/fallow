---
paths:
  - "crates/mcp/**"
---

# fallow-mcp crate

MCP server exposing fallow analysis as tools for AI agents. Stdio transport, wraps `fallow` CLI via subprocess.

## Tools (20 total)
- `analyze` - full dead code analysis (`fallow dead-code --format json`), supports `boundary_violations` convenience param
- `check_changed` - incremental analysis (`fallow dead-code --changed-since`)
- `find_dupes` - code duplication (`fallow dupes --format json`), supports `changed_since`
- `check_health` - complexity metrics (`fallow health --format json`), supports `file_scores`, `hotspots`, `targets`, `since`, `min_commits`, `runtime_coverage` (single capture is free, multi-capture is paid; forwards `--runtime-coverage <path>`), `min_invocations_hot` (hot-path threshold), `min_observation_volume` (high-confidence verdict floor), `low_traffic_threshold` (active/low_traffic split), `max_crap` (per-function CRAP threshold, default 30.0; forwards `--max-crap <N>`), `group_by` (`owner`/`directory`/`package`/`section`: each group recomputes its own `vital_signs` and `health_score` from the group's files; SARIF results gain `properties.group` and CodeClimate issues gain a top-level `group` field) params
- `check_runtime_coverage` - focused runtime-coverage entry point (`fallow health --runtime-coverage <path>`). Required `coverage` param (V8 dir, V8 JSON, or Istanbul JSON). A single local capture is free and runs without a license; continuous or multi-capture runtime monitoring (V8 directory with multiple JSON files) requires an active license. Tuning: `min_invocations_hot` (default 100), `min_observation_volume` (default 5000), `low_traffic_threshold` (default 0.001), `max_crap` (default 30.0), `top` (cap returned findings, hot paths, file scores, and refactoring targets), `group_by` (`owner`/`directory`/`package`/`section`). Returns the standard health JSON plus a stable `runtime_coverage.schema_version` ("1") string for agent consumers. Top-level `runtime_coverage.verdict` was renamed `hot-path-changes-needed` -> `hot-path-touched`; agents reading the headline need the new spelling. PR context (when `FALLOW_DIFF_FILE` or `FALLOW_CHANGED_SINCE` is set in the agent's env) promotes `hot-path-touched` over `cold-code-detected`; standalone keeps cold-code primary. The full unprioritized signal list is in `runtime_coverage.signals[]` (kebab-case, severity-descending). Each `runtime_coverage.hot_paths[]` entry carries `start_line` + `end_line` for line-overlap matching against a PR diff. Raise `FALLOW_TIMEOUT_SECS` for multi-megabyte dumps. Pick this over `check_health` when you have a V8 or Istanbul coverage dump and want surfaced dead-in-production verdicts.
- `get_hot_paths` - runtime-context slice over the same `fallow health --runtime-coverage` pipeline. Same params as `check_runtime_coverage`; same free-vs-paid contract (single local capture free, multi-capture paid). Steers agents to read `runtime_coverage.hot_paths`, sorted by percentile and invocation count. Each entry carries `start_line` + `end_line` so agents can match against a PR diff. Use `top` to cap returned hot paths. Environment-driven scoping: if `FALLOW_DIFF_FILE` (path to unified diff) or `FALLOW_CHANGED_SINCE` (git ref) is set in the agent's process env, hot paths are scoped to that change set (line-level for the diff, file-level for changed-since); unset both for project-wide results. Always emits a top-level `warnings` array (empty when none).
- `get_blast_radius` - runtime-context slice. Same params as `check_runtime_coverage`; same free-vs-paid contract. Until `runtime_coverage.blast_radius` ships as a first-class field, agents should combine `file_scores[].fan_in`, `runtime_coverage.hot_paths`, and `runtime_coverage.findings`. Always emits a top-level `warnings` array.
- `get_importance` - runtime-context slice. Same params as `check_runtime_coverage`; same free-vs-paid contract. Until `runtime_coverage.importance` ships as a first-class field, agents should combine `runtime_coverage.hot_paths`, `file_scores`, `hotspots`, and `targets`. Always emits a top-level `warnings` array.
- `get_cleanup_candidates` - runtime-context slice. Same params as `check_runtime_coverage`; same free-vs-paid contract. Steers agents to read `runtime_coverage.findings` for `safe_to_delete`, `review_required`, `low_traffic`, and `coverage_unavailable` verdicts. Always emits a top-level `warnings` array.
- `audit` - combined dead-code + complexity + duplication for changed files, returns verdict (`fallow audit --format json`). Supports `gate` (`new-only` default, `all` to gate every finding in changed files; forwards to `--gate <value>`), `max_crap` (forwards `--max-crap <N>` to the health sub-analysis), and `include_entry_exports` (forwards `--include-entry-exports`; ORs with the `includeEntryExports` config value).
- `fallow_explain` - explain one issue type without running analysis (`fallow explain <issue-type> --format json`). Returns rule name, summary, rationale, example, fix guidance, and docs URL for agent planning.
- `fix_preview` - dry-run auto-fix (`fallow fix --dry-run --format json`)
- `fix_apply` - apply auto-fixes (`fallow fix --yes --format json`), destructive
- `project_info` - project metadata (`fallow list --format json`), supports section params (`entry_points`, `files`, `plugins`, `boundaries`)
- `list_boundaries` - architecture boundary zones and rules (`fallow list --boundaries --format json`)
- `feature_flags` - detect feature flag patterns (`fallow flags --format json`), supports `flag_type`, `confidence`, `dead_code_only` params
- `trace_export` - trace why an export is used/unused (`fallow dead-code --trace FILE:EXPORT_NAME --format json`). Required `file` and `export_name` params. Returns file reachability, entry-point status, direct references, re-export chains, and a reason summary. Use before deleting a supposedly-unused export.
- `trace_file` - trace all graph edges for a file (`fallow dead-code --trace-file PATH --format json`). Required `file` param. Returns reachability, entry-point status, exports, imports-from, imported-by, and re-exports. Use to decide whether a file is isolated, barrel-only, or imported by live entry points.
- `trace_dependency` - trace where a dependency is imported (`fallow dead-code --trace-dependency PACKAGE --format json`). Required `package_name` param. Returns importing files, type-only importers, total import count, `used_in_scripts` (true when invoked from package.json scripts or CI configs like `.github/workflows/*.yml` / `.gitlab-ci.yml`), and `is_used` (combined import + script signal, mirrors the unused-deps detector). Use before removing a dependency or moving between `dependencies` and `devDependencies`.
- `trace_clone` - trace duplicate-code groups at a location (`fallow dupes --trace FILE:LINE --format json`). Required `file` and `line` params. Returns the matched clone instance plus every clone group containing it. Supports `mode`, `min_tokens`, `min_lines`, `threshold`, `skip_local`, `cross_language`, `ignore_imports`. Use to consolidate duplication when you need exact sibling locations.

## Global flags (available on all tools)
- `no_cache` (bool) — disable incremental parse cache
- `threads` (usize) — parser thread count

## Environment-driven scoping
- `FALLOW_DIFF_FILE` (path to unified diff) and `FALLOW_CHANGED_SINCE` (git ref) inherited from the agent's process env scope hot-path matching for the runtime-coverage tools and the audit tool. The `tools/mod.rs::run_fallow` spawn does not strip the env. Unset both for project-wide results.

## Flags on analysis tools (analyze, check_changed, find_dupes, check_health)
- `baseline` (string) — compare against saved baseline
- `save_baseline` (string) — save results as baseline

## Error handling
- Subprocess timeout: 120s default, configurable via `FALLOW_TIMEOUT_SECS` env var
- Exit code 2+ errors: pass through CLI's structured JSON error from stdout when available; fall back to `{"error":true,"message":"...","exit_code":N}` from stderr
- Exit code 1: treated as success (issues found, not an error)
- Pre-spawn validation rejections (empty required field, out-of-range line, invalid mode, unknown issue type) return the same envelope with `exit_code: 0` via `validation_error_body` in `tools/mod.rs`. Clients should branch on `error: true`, not on `exit_code`, since `0` can mean either "never spawned" (validation) or "spawned and succeeded" (normal result).

## Actions injection
All JSON output includes structured `actions` arrays on every finding:
- Dead-code issues: fix action + suppress action (via `inject_actions` in `report/json.rs`)
- Health findings: `refactor-function` + suppress (via `inject_health_actions`)
- Health targets: `apply-refactoring` + suppress when evidence exists
- Dupes families: `extract-shared` + suggestions + suppress (via `inject_dupes_actions`)
- Dupes groups: `extract-shared` + suppress
- Audit: inherits actions from all three sub-analyses

All params structs derive `Default` for ergonomic test construction except those with required non-default fields: `CheckChangedParams` (`since`), `CheckRuntimeCoverageParams` (`coverage`), `TraceExportParams` (`file`, `export_name`), `TraceFileParams` (`file`), `TraceDependencyParams` (`package_name`), and `TraceCloneParams` (`file`, `line`). Trace param tests build struct literals directly; the first two use the helpers `check_changed("main")` and `check_runtime_coverage("./coverage")`.

Built with `rmcp` (official Rust MCP SDK). Thin subprocess wrapper — all analysis logic stays in the CLI.
- `FALLOW_BIN` — binary path (defaults to sibling binary or `fallow` in PATH)
- `FALLOW_TIMEOUT_SECS` — subprocess timeout in seconds (default: 120)
