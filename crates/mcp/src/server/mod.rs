use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo};
use rmcp::{ErrorData as McpError, ServerHandler, tool, tool_router};

use crate::params::{
    AnalyzeParams, AuditParams, CheckChangedParams, CheckRuntimeCoverageParams, ExplainParams,
    FeatureFlagsParams, FindDupesParams, FixParams, HealthParams, ListBoundariesParams,
    ProjectInfoParams, TraceCloneParams, TraceDependencyParams, TraceExportParams, TraceFileParams,
};
use crate::tools::{
    build_analyze_args, build_audit_args, build_check_changed_args,
    build_check_runtime_coverage_args, build_explain_args, build_feature_flags_args,
    build_find_dupes_args, build_fix_apply_args, build_fix_preview_args,
    build_get_blast_radius_args, build_get_cleanup_candidates_args, build_get_hot_paths_args,
    build_get_importance_args, build_health_args, build_list_boundaries_args,
    build_project_info_args, build_trace_clone_args, build_trace_dependency_args,
    build_trace_export_args, build_trace_file_args, run_fallow, run_fallow_with_top_level_warnings,
};

#[cfg(test)]
mod tests;

// ── Server ─────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct FallowMcp {
    binary: String,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "read by the rmcp tool_router macro expansion and unit tests"
        )
    )]
    tool_router: ToolRouter<Self>,
}

impl FallowMcp {
    pub fn new() -> Self {
        let binary = resolve_binary();
        Self {
            binary,
            tool_router: Self::tool_router(),
        }
    }
}

/// Resolve the fallow binary path.
/// Priority: `FALLOW_BIN` env var > sibling binary next to fallow-mcp > PATH lookup.
fn resolve_binary() -> String {
    if let Ok(bin) = std::env::var("FALLOW_BIN") {
        return bin;
    }

    // Check for sibling binary next to the current executable (npm install scenario)
    if let Ok(exe) = std::env::current_exe() {
        let sibling = exe.with_file_name("fallow");
        if sibling.is_file()
            && let Some(path) = sibling.to_str()
        {
            return path.to_string();
        }
    }

    "fallow".to_string()
}

// ── Tool implementations ───────────────────────────────────────────

#[tool_router]
impl FallowMcp {
    #[tool(
        description = "Analyze a TypeScript/JavaScript project for unused code, circular dependencies, and re-export cycles (barrel files that form a structural loop, silently breaking re-exports). Detects unused files, exports, types, dependencies, enum/class members, unresolved imports, unlisted dependencies, duplicate exports, circular dependencies, re-export cycles, boundary violations, stale suppression comments, unused pnpm catalog entries (entries in pnpm-workspace.yaml `catalog:` / `catalogs:` not referenced by any workspace package), empty pnpm catalog groups (named `catalogs.<name>:` groups with no entries), unresolved catalog references (workspace package.json declares `catalog:` but the catalog has no entry), unused pnpm dependency overrides (`pnpm-workspace.yaml#overrides` or `package.json#pnpm.overrides` targets a package no workspace package declares and pnpm-lock.yaml does not resolve), and misconfigured pnpm dependency overrides (unparsable key or empty value; pnpm install will reject). Private type leaks are an opt-in API hygiene check via issue_types: [\"private-type-leaks\"]. Returns structured JSON with all issues found, grouped by issue type. For code duplication use find_dupes, for complexity hotspots use check_health. Supports baseline comparisons (baseline/save_baseline), regression detection (fail_on_regression, tolerance, regression_baseline, save_regression_baseline), and performance tuning (no_cache, threads). Set boundary_violations=true to check only architecture boundary violations (convenience alias for issue_types: [\"boundary-violations\"]). Set group_by to \"owner\" (CODEOWNERS), \"directory\", \"package\" (workspace), or \"section\" to group results. The `section` mode reads GitLab CODEOWNERS `[Section]` headers and emits `owners` metadata per group.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn analyze(&self, params: Parameters<AnalyzeParams>) -> Result<CallToolResult, McpError> {
        let params = params.0;
        match build_analyze_args(&params) {
            Ok(args) => run_fallow(&self.binary, &args).await,
            Err(msg) => Ok(CallToolResult::error(vec![Content::text(msg)])),
        }
    }

    #[tool(
        description = "Analyze only files changed since a git ref. Useful for incremental CI checks on pull requests. Returns the same structured JSON as analyze, but filtered to only include issues in changed files. Supports baseline comparisons (baseline/save_baseline), regression detection (fail_on_regression, tolerance, regression_baseline, save_regression_baseline), and performance tuning (no_cache, threads).",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn check_changed(
        &self,
        params: Parameters<CheckChangedParams>,
    ) -> Result<CallToolResult, McpError> {
        let args = build_check_changed_args(params.0);
        run_fallow(&self.binary, &args).await
    }

    #[tool(
        description = "Find code duplication across the project. Detects clone groups (identical or similar code blocks) with configurable detection modes and thresholds. Returns clone families with refactoring suggestions. Each clone_groups[] entry carries a stable `fingerprint` (dup:<id>); pass it to the trace_clone tool to deep-dive that group (locations, an extract-function suggestion with estimated savings, and a best-effort suggested name). Set top=N to show only the N largest clone groups. Set group_by to \"owner\" (CODEOWNERS), \"directory\", \"package\" (workspace), or \"section\" (GitLab CODEOWNERS `[Section]` headers, with `owners` metadata per group) to partition results. explain_skipped only changes the human-format skipped-default-ignores note (human/markdown CLI output); MCP JSON responses stay clean. Supports config, workspace scoping, baseline comparisons, and performance tuning (no_cache, threads).",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn find_dupes(
        &self,
        params: Parameters<FindDupesParams>,
    ) -> Result<CallToolResult, McpError> {
        let params = params.0;
        match build_find_dupes_args(&params) {
            Ok(args) => run_fallow(&self.binary, &args).await,
            Err(msg) => Ok(CallToolResult::error(vec![Content::text(msg)])),
        }
    }

    #[tool(
        description = "Preview auto-fixes without modifying any files. Shows what would be changed: which unused exports would be removed, which unused dependencies would be deleted from package.json, which unused enum members would be removed, which unused pnpm catalog entries would be cleaned up, and which duplicate-export `ignoreExports` rules would be added to the fallow config file. For `add-to-config` actions, each fix entry carries a `proposed_diff` field with the unified-diff preview of the proposed config write. When no fallow config exists outside a monorepo subpackage, fix_apply would CREATE `.fallowrc.json` using `fallow init`'s framework-aware scaffolding (`$schema`, `entry`, `ignorePatterns`, etc.) and layer the `ignoreExports` rules on top; preview entries on that path carry `created_files: [\".fallowrc.json\"]`. Set `no_create_config: true` to skip the config-creation path with `skip_reason: \"no_create_config\"`; source-file previews are unaffected. When a source file's xxh3 content hash at preview time differs from the hash captured during the in-process analysis (parallel editor save or external mutation between the analysis read and the preview entry), the per-file entry is emitted with `skip_reason: \"content_changed\"` and the envelope's top-level `skipped_content_changed` count is incremented; re-run after refreshing analysis to pick up the new on-disk shape. When a source file mixes CRLF and bare-LF line endings (common after cross-platform edits without `core.autocrlf`), the per-file entry is emitted with `skip_reason: \"mixed_line_endings\"` and the envelope's top-level `skipped_mixed_line_endings` count is incremented; this skip is NOT self-healing because re-running fallow alone does not normalize the file. The agent must run `dos2unix <path>` (or instruct the user to set `git config core.autocrlf input` and re-checkout) before re-running. When an unused export lives in a test, mock, or fixture directory (`__mocks__`, `e2e`, `e2e-tests`, `cypress`, `playwright`, `examples`, `fixtures`, `__fixtures__`, `evals`, `golden`), or in a file that itself has an unresolved import, its removal is withheld as low confidence because the consumer may be invisible to static analysis (Vitest mock aliases, off-workspace e2e trees, fixture build steps): the per-file entry carries `skip_reason: \"low_confidence_off_graph\"` or `\"low_confidence_unresolved_imports\"` and the envelope's top-level `skipped_low_confidence_exports` count is incremented. Unlike the two skips above this is INTENTIONAL and does NOT change the exit code; the export stays reported by `fallow check` so it can be confirmed and removed by hand. Supports workspace scoping and performance tuning (no_cache, threads).",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn fix_preview(&self, params: Parameters<FixParams>) -> Result<CallToolResult, McpError> {
        let args = build_fix_preview_args(&params.0);
        run_fallow(&self.binary, &args).await
    }

    #[tool(
        description = "Apply auto-fixes to the project. Removes unused exports from the public API, may delete dead exported enum declarations, deletes unused dependencies from package.json, removes unused enum members, removes unused pnpm catalog entries (rewriting empty groups to `{}` so pnpm install does not reject null), and appends duplicate-export `ignoreExports` rules to the fallow config file. When no fallow config exists outside a monorepo subpackage, this CREATES `.fallowrc.json` using `fallow init`'s framework-aware scaffolding (TypeScript / Storybook / Vitest / Jest / Playwright / React / Vue / Angular / Svelte detection, `$schema`, `entry`, `ignorePatterns`, sensible defaults) and layers the `ignoreExports` rules on top. Inside a monorepo subpackage (workspace marker `pnpm-workspace.yaml` / `package.json#workspaces` / `turbo.json` / `lerna.json` / `rush.json` above the invocation directory) the create-fallback refuses and emits `skip_reason: \"monorepo_subpackage\"` with a relative `workspace_root` path; the agent should re-run `fallow init` at the workspace root or invoke from there. Set `no_create_config: true` to opt out of the create-fallback entirely (recommended for unsupervised agent flows or CI bots where silently materialising a new top-level config file would surprise the user); the duplicate-export path is skipped with `skip_reason: \"no_create_config\"` and source-file edits proceed normally. When a source file's xxh3 content hash at fix time differs from the hash captured during the in-process analysis (parallel editor save, CI rebase, or other tool mutated the file between analysis and write), the per-file fix is skipped with `skip_reason: \"content_changed\"`; the envelope's top-level `skipped_content_changed` count is incremented and the run exits with code 2 so CI surfaces the mismatch instead of treating the partial run as a clean no-op. When a source file mixes CRLF and bare-LF line endings, the per-file fix is skipped with `skip_reason: \"mixed_line_endings\"`; the envelope's top-level `skipped_mixed_line_endings` count is incremented and the run exits with code 2. This skip is NOT self-healing: re-running fallow alone does not normalize the file and will loop. The agent must run `dos2unix <path>` (or instruct the user to set `git config core.autocrlf input` and re-checkout) before re-running. When an unused export lives in a test, mock, or fixture directory (`__mocks__`, `e2e`, `e2e-tests`, `cypress`, `playwright`, `examples`, `fixtures`, `__fixtures__`, `evals`, `golden`), or in a file that itself has an unresolved import, its removal is withheld as low confidence because the consumer may be invisible to static analysis (Vitest mock aliases, off-workspace e2e trees, fixture build steps): the per-file fix carries `skip_reason: \"low_confidence_off_graph\"` or `\"low_confidence_unresolved_imports\"` and the envelope's top-level `skipped_low_confidence_exports` count is incremented. Unlike the two skips above this is INTENTIONAL: it does NOT change the exit code (the run can still exit 0), and the export stays reported by `fallow check` so it can be confirmed and removed by hand. Writes are batched: each per-file rewrite is staged to a sibling temp file, and the orchestrator promotes the batch only after every stage succeeds, so a single stage failure leaves the project untouched. Files with a UTF-8 BOM are read with the BOM stripped (so line offsets align with the parser) and written with the BOM re-prepended (so Windows-authored files round-trip unchanged); fallow does not add or remove a BOM. This modifies files on disk. Use fix_preview first to review planned changes including any `proposed_diff` and `created_files` fields. Supports workspace scoping and performance tuning (no_cache, threads).",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn fix_apply(&self, params: Parameters<FixParams>) -> Result<CallToolResult, McpError> {
        let args = build_fix_apply_args(&params.0);
        run_fallow(&self.binary, &args).await
    }

    #[tool(
        description = "Get project metadata: active framework plugins, discovered source files, and detected entry points. Useful for understanding how fallow sees the project before running analysis. Supports performance tuning (no_cache, threads).",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn project_info(
        &self,
        params: Parameters<ProjectInfoParams>,
    ) -> Result<CallToolResult, McpError> {
        let args = build_project_info_args(&params.0);
        run_fallow(&self.binary, &args).await
    }

    #[tool(
        description = "Trace why an export is considered used or unused. Returns file reachability, entry-point status, direct references, re-export chains, and a concise reason string. Use this when an agent needs evidence before deleting or rewriting a supposedly unused export.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn trace_export(
        &self,
        params: Parameters<TraceExportParams>,
    ) -> Result<CallToolResult, McpError> {
        match build_trace_export_args(&params.0) {
            Ok(args) => run_fallow(&self.binary, &args).await,
            Err(msg) => Ok(CallToolResult::error(vec![Content::text(msg)])),
        }
    }

    #[tool(
        description = "Trace a file's graph context. Returns whether the file is reachable or an entry point, what it exports, what it imports, what imports it, and which re-exports it declares. Use this to understand whether a file is isolated, barrel-only, or imported by live entry points.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn trace_file(
        &self,
        params: Parameters<TraceFileParams>,
    ) -> Result<CallToolResult, McpError> {
        match build_trace_file_args(&params.0) {
            Ok(args) => run_fallow(&self.binary, &args).await,
            Err(msg) => Ok(CallToolResult::error(vec![Content::text(msg)])),
        }
    }

    #[tool(
        description = "Trace where a dependency is used. Returns which files import the package, which imports are type-only, whether the package is referenced from package.json scripts or CI configs (`used_in_scripts`), and whether the dependency is used at all (`is_used` accounts for both imports and script usage, matching the unused-deps detector). Useful before removing a dependency or moving it between dependencies and devDependencies.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn trace_dependency(
        &self,
        params: Parameters<TraceDependencyParams>,
    ) -> Result<CallToolResult, McpError> {
        match build_trace_dependency_args(&params.0) {
            Ok(args) => run_fallow(&self.binary, &args).await,
            Err(msg) => Ok(CallToolResult::error(vec![Content::text(msg)])),
        }
    }

    #[tool(
        description = "Deep-dive a duplicate-code clone group. Address it either by a source location (file + line) or by a stable fingerprint (fingerprint=\"dup:<id>\" from a find_dupes clone_groups[].fingerprint, usually dup:<8hex> and widened only on rare report collisions). Returns the matched clone instance, every sibling clone group / location, plus per group an extract-function suggestion with estimated line savings and a best-effort suggested_name (the field is omitted, not null, when there is no confident name, so branch on key presence; advisory, verify before applying, never auto-apply). Provide exactly one addressing form. Use after find_dupes when consolidating duplication and you need exact sibling locations and a refactor target.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn trace_clone(
        &self,
        params: Parameters<TraceCloneParams>,
    ) -> Result<CallToolResult, McpError> {
        match build_trace_clone_args(&params.0) {
            Ok(args) => run_fallow(&self.binary, &args).await,
            Err(msg) => Ok(CallToolResult::error(vec![Content::text(msg)])),
        }
    }

    #[tool(
        description = "Check code health metrics (cyclomatic and cognitive complexity) for functions in the project. Returns structured JSON with complexity scores per function, sorted by severity. Set score=true for a single 0-100 health score with letter grade (A/B/C/D/F); runs duplication analysis automatically, but the churn-backed hotspot penalty requires hotspots=true (or targets=true). Set min_score=N to fail if score drops below a threshold (CI quality gate). Set file_scores=true for per-file health scores (maintainability index, fan-in, fan-out, dead code ratio, complexity density, CRAP risk), sorted in risk-aware triage order so lower MI and higher CRAP risk appear first. Set coverage_gaps=true to explicitly include static test coverage gaps: runtime files and exports with no test dependency path (not line-level coverage). A provided config file may also enable coverage gaps via rules.coverage-gaps when no health sections are explicitly selected. Set hotspots=true to identify files that are both complex and frequently changing (combines git churn with complexity). Set ownership=true (implies hotspots) to attach per-file ownership signals: bus factor, contributor count, declared CODEOWNERS owner, ownership_state, drift, and unowned-hotspot flag. Use ownership_email_mode=raw|handle|anonymized|hash for author email privacy (default handle; hash is the legacy spelling for anonymized output). Set targets=true for ranked refactoring recommendations sorted by efficiency (quick wins first), with confidence scores and adaptive percentile-based thresholds. Set trend=true to compare current metrics against the most recent saved snapshot and show per-metric deltas with directional indicators (improving/declining/stable). Implies --score. Requires prior snapshots saved with save_snapshot. Set effort to control analysis depth: 'low' (fast, surface-level), 'medium' (balanced, default), or 'high' (thorough, all heuristics). Set summary=true to include a natural-language summary of findings alongside the structured JSON. Set coverage to a path to Istanbul-format coverage data (coverage-final.json from Jest, Vitest, c8, nyc) for accurate per-function CRAP scores instead of the default static binary model. CRAP findings carry a `coverage_source` discriminator (`istanbul`, `estimated`, or `estimated_component_inherited`); synthetic `<template>` findings on Angular `.html` files use `estimated_component_inherited` and include an `inherited_from` path to the owning `.component.ts` so agents target the component file for coverage remediation rather than the template. Angular components whose class AND template both contribute to complexity also emit a synthetic `<component>` rollup finding anchored at the worst class method's `(line, col)`. The rollup's `cyclomatic` is `worst_class_method.cyclomatic + template.cyclomatic` (the same worst-by-cyclomatic method drives both metrics; cognitive is `worst.cognitive + template.cognitive`). The `component_rollup` payload carries the pre-summation breakdown: `class_worst_function` (method name), `class_cyclomatic` / `class_cognitive` (per-method numbers), `template_path` / `template_cyclomatic` / `template_cognitive`, plus a `component` identifier derived from the .ts owner's file stem. The rollup's `suppress-line` action uses `placement: \"above-component-worst-method\"`: a `// fallow-ignore-next-line complexity` placed above the worst class method hides BOTH the per-function finding AND the rollup, so agents do not need to emit two suppression edits. Per-function and per-`<template>` entries stay alongside the rollup; ranking and `--targets` use the rollup so a template-heavy component surfaces as one unit rather than scattered medium findings. Set runtime_coverage to a path (V8 coverage directory, V8 JSON file, or Istanbul JSON file) for merged runtime runtime-coverage findings (paid feature; requires an active license via `fallow license activate`). Runtime-coverage tuning: set min_invocations_hot=N to tune the hot-path threshold (default 100), min_observation_volume=N to tune the high-confidence verdict floor (default 5000), and low_traffic_threshold=RATIO to tune the active/low-traffic split (default 0.001). Set group_by to \"owner\" (CODEOWNERS), \"directory\", \"package\" (workspace), or \"section\" (GitLab CODEOWNERS `[Section]` headers, with `owners` metadata per group) to partition results. Each group gets its own `vital_signs`, `health_score`, `findings`, `file_scores`, `hotspots`, `large_functions`, and `targets` recomputed against the group's files (top-level metrics stay project-wide). Use this to answer per-team or per-package quality questions like \"which workspace has the worst maintainability?\" without running fallow once per package. Supports config, baseline comparisons, and performance tuning (no_cache, threads). Useful for identifying hard-to-maintain code and prioritizing refactoring.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn check_health(
        &self,
        params: Parameters<HealthParams>,
    ) -> Result<CallToolResult, McpError> {
        let args = build_health_args(&params.0);
        run_fallow(&self.binary, &args).await
    }

    #[tool(
        description = "Audit changed files for dead code, complexity, and duplication. Purpose-built for reviewing AI-generated code. Combines dead-code + complexity + duplication scoped to changed files and returns a verdict (pass/warn/fail). Auto-detects the base branch if not specified. By default, audit runs the base ref too and gates only findings introduced by the changeset; inherited findings are annotated with introduced=false and counted under attribution. Set gate=\"all\" or audit.gate=\"all\" in config to gate every finding. Returns JSON with verdict, summary counts per category, attribution counts, and full issue details with actions array for auto-correction. Set coverage to an Istanbul coverage-final.json path and coverage_root to an absolute coverage-data path prefix when paths need rebasing for accurate CRAP scoring in the health sub-analysis. Set group_by to \"owner\" (CODEOWNERS), \"directory\", \"package\" (workspace), or \"section\" (GitLab CODEOWNERS `[Section]` headers, with `owners` metadata per group) to partition results. Set dead_code_baseline, health_baseline, and/or dupes_baseline to per-analysis baseline file paths (as saved by `fallow dead-code|health|dupes --save-baseline`) so pre-existing issues on touched files do not dominate the verdict; only new issues not present in the respective baseline contribute. explain_skipped only changes the human-format skipped-default-ignores note (human/markdown CLI output); MCP JSON responses stay clean. Set include_entry_exports=true to also report unused exports in entry files (catches typos in framework exports like `meatdata` vs `metadata`); the CLI flag ORs with the `includeEntryExports` config value. Set runtime_coverage to a V8 coverage directory, V8 JSON file, or Istanbul coverage-final.json to fold runtime-coverage findings into the same audit invocation: agents get the `hot-path-touched` verdict alongside dead-code and complexity in one call (License-gated; informational verdict, no exit-code change). Set min_invocations_hot=N to tune the runtime-coverage hot-path threshold used by audit (default 100). Health findings include Angular `<component>` rollups (synthetic per-component finding folding `worst_class_method + template` complexity); ranking and `--targets` use the rollup over per-function entries so a template-heavy component is surfaced as one unit. See `check_health` for the `component_rollup` payload shape. When `FALLOW_DIFF_FILE` (path to a unified diff) is set in the agent's process environment, EVERY finding (dead-code, complexity, duplication, boundary, runtime-coverage hot paths) is narrowed to source lines inside an added hunk; project-level findings (unused deps, catalog entries, dependency overrides) bypass the filter because they anchor at fixed `package.json` / `pnpm-workspace.yaml` lines a PR rarely touches. When both `FALLOW_DIFF_FILE` and `FALLOW_CHANGED_SINCE` are set, the diff wins for line-level filtering and `--changed-since` still scopes file discovery. The `runtime_coverage.verdict` also promotes `hot-path-touched` over `cold-code-detected` for PR-review contexts; the full unprioritized signal set is in `runtime_coverage.signals[]`. Use this after generating code to verify quality before committing.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn audit(&self, params: Parameters<AuditParams>) -> Result<CallToolResult, McpError> {
        match build_audit_args(&params.0) {
            Ok(args) => run_fallow(&self.binary, &args).await,
            Err(msg) => Ok(CallToolResult::error(vec![Content::text(msg)])),
        }
    }

    #[tool(
        description = "Explain one fallow issue type without running analysis. Returns the rule id, name, rationale, worked example, fix guidance, and docs URL as JSON. Use this before applying fixes when an agent or reviewer needs to understand what a finding means.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn fallow_explain(
        &self,
        params: Parameters<ExplainParams>,
    ) -> Result<CallToolResult, McpError> {
        let args = build_explain_args(&params.0);
        run_fallow(&self.binary, &args).await
    }

    #[tool(
        description = "List architecture boundary zones and access rules configured for the project. Returns zone definitions (name, glob patterns, matched file count), access rules (which zones may import from which), and `logical_groups[]` (one entry per pre-expansion `autoDiscover` zone, surfacing the user-authored parent name, verbatim `auto_discover` paths, discovered `children`, `status` (`ok` / `empty` / `invalid_path`), `source_zone_index`, summed `file_count`, optional `authored_rule`, optional `fallback_zone` cross-reference for the Bulletproof case, optional `merged_from` when the parent name was declared multiple times, optional `original_zone_root` echo for monorepo subtree scopes, and optional `child_source_indices` attribution when multiple paths were authored). If boundaries are not configured, returns {\"configured\": false}; in that case, boundary violation checks will find no issues and can be skipped. Use this to understand the project's architecture constraints before running analysis.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn list_boundaries(
        &self,
        params: Parameters<ListBoundariesParams>,
    ) -> Result<CallToolResult, McpError> {
        let args = build_list_boundaries_args(&params.0);
        run_fallow(&self.binary, &args).await
    }

    #[tool(
        description = "Detect feature flag patterns in a TypeScript/JavaScript project. Identifies environment variable flags (process.env.FEATURE_*), SDK calls from common providers, and config object patterns. Returns flag locations, detection confidence, and cross-reference with dead code findings.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn feature_flags(
        &self,
        params: Parameters<FeatureFlagsParams>,
    ) -> Result<CallToolResult, McpError> {
        let args = build_feature_flags_args(&params.0);
        run_fallow(&self.binary, &args).await
    }

    #[tool(
        description = "Merge runtime-coverage data into the health report. Focused entry point for the runtime-coverage pipeline: pass a V8 coverage directory (`NODE_V8_COVERAGE=<dir>`), a single V8 coverage JSON file, or an Istanbul `coverage-final.json` via the required `coverage` field. A single local capture is free and runs without a license; continuous or multi-capture runtime monitoring (multiple JSON files in a V8 directory) requires an active license JWT (start a 30-day trial with `fallow license activate --trial --email <addr>`; check state with `fallow license status`). Returns structured JSON with a `runtime_coverage` block containing surfaced `findings` verdicts (`safe_to_delete` / `review_required` / `low_traffic` / `coverage_unavailable`), stable content-hash IDs (`fallow:prod:<hash>`), evidence, percentile-ranked hot paths (each with `start_line` and `end_line` so consumers can match against a PR diff), and on protocol-0.3+ sidecars a `summary.capture_quality` block that flags short-window captures. The sidecar may still classify other functions as `active`, but the CLI omits those from `runtime_coverage.findings` to keep the surfaced list actionable. Tunable via `min_invocations_hot` (hot-path threshold, default 100), `min_observation_volume` (high-confidence verdict floor, default 5000), and `low_traffic_threshold` (active/low_traffic split, default 0.001). `group_by` partitions results by CODEOWNERS / directory / package / section. PR-context behavior: when `FALLOW_DIFF_FILE` (path to a unified diff) is set in the agent's process environment, the top-level `runtime_coverage.verdict` promotes `hot-path-touched` over `cold-code-detected` so reviewers see the diff-tied signal first, AND every hot path is narrowed to functions whose `[start_line, end_line]` overlaps an added hunk. `FALLOW_CHANGED_SINCE` (git ref) also scopes (file-level). Without a change scope the verdict stays cold-code-primary and all hot paths are returned. The full unprioritized list is always in `runtime_coverage.signals[]` (kebab-case strings, severity-descending). Runtime coverage can exceed the default 120s MCP subprocess timeout on multi-megabyte dumps; raise `FALLOW_TIMEOUT_SECS` accordingly. For general complexity / hotspot / CRAP analysis without a production dump, use `check_health` instead.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn check_runtime_coverage(
        &self,
        params: Parameters<CheckRuntimeCoverageParams>,
    ) -> Result<CallToolResult, McpError> {
        let args = build_check_runtime_coverage_args(&params.0);
        run_fallow(&self.binary, &args).await
    }

    #[tool(
        description = "Return production hot paths from a local V8 or Istanbul runtime coverage dump. Pass `coverage` as a V8 coverage directory, single V8 JSON file, or Istanbul `coverage-final.json`. A single local capture is free and runs without a license; continuous or multi-capture runtime monitoring requires an active license. Returns the standard health JSON; agents should read `runtime_coverage.hot_paths`, which is sorted by percentile and invocation count. Each entry carries `start_line` and `end_line` so agents can match the function's full body against a PR diff. Use `top` to cap the returned hot paths. Environment-driven scoping: if `FALLOW_DIFF_FILE` (path to a unified diff) is set in the agent's process env, hot paths are narrowed to functions whose `[start_line, end_line]` overlaps an added hunk; a file the diff touched but with no added lines (deletion-only / pure-rename) drops its hot paths rather than falling through to changed-since file-level matching. `FALLOW_CHANGED_SINCE` (git ref) covers the fallback case (file-level match for files NOT in the diff at all). Unset both for project-wide hot paths.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn get_hot_paths(
        &self,
        params: Parameters<CheckRuntimeCoverageParams>,
    ) -> Result<CallToolResult, McpError> {
        let args = build_get_hot_paths_args(&params.0);
        run_fallow_with_top_level_warnings(&self.binary, &args).await
    }

    #[tool(
        description = "Return first-class blast-radius context alongside local runtime coverage. Pass `coverage` as a V8 coverage directory, single V8 JSON file, or Istanbul `coverage-final.json`. A single local capture is free and runs without a license; continuous or multi-capture runtime monitoring requires an active license. Returns the standard health JSON; agents should read `runtime_coverage.blast_radius`, which contains stable `fallow:blast:<hash>` IDs, caller counts, traffic-weighted caller reach, and low/medium/high risk bands.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn get_blast_radius(
        &self,
        params: Parameters<CheckRuntimeCoverageParams>,
    ) -> Result<CallToolResult, McpError> {
        let args = build_get_blast_radius_args(&params.0);
        run_fallow_with_top_level_warnings(&self.binary, &args).await
    }

    #[tool(
        description = "Return first-class production-importance context from local runtime coverage plus static health signals. Pass `coverage` as a V8 coverage directory, single V8 JSON file, or Istanbul `coverage-final.json`. A single local capture is free and runs without a license; continuous or multi-capture runtime monitoring requires an active license. Returns the standard health JSON; agents should read `runtime_coverage.importance`, which contains stable `fallow:importance:<hash>` IDs, invocations, cyclomatic complexity, owner count, a 0-100 score, and a templated reason.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn get_importance(
        &self,
        params: Parameters<CheckRuntimeCoverageParams>,
    ) -> Result<CallToolResult, McpError> {
        let args = build_get_importance_args(&params.0);
        run_fallow_with_top_level_warnings(&self.binary, &args).await
    }

    #[tool(
        description = "Return cleanup candidates grounded in local runtime coverage. Pass `coverage` as a V8 coverage directory, single V8 JSON file, or Istanbul `coverage-final.json`. A single local capture is free and runs without a license; continuous or multi-capture runtime monitoring requires an active license. Returns the standard health JSON; agents should read `runtime_coverage.findings` for `safe_to_delete`, `review_required`, `low_traffic`, and `coverage_unavailable` verdicts.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn get_cleanup_candidates(
        &self,
        params: Parameters<CheckRuntimeCoverageParams>,
    ) -> Result<CallToolResult, McpError> {
        let args = build_get_cleanup_candidates_args(&params.0);
        run_fallow_with_top_level_warnings(&self.binary, &args).await
    }
}

// ── ServerHandler ──────────────────────────────────────────────────

#[rmcp::tool_handler]
impl ServerHandler for FallowMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("fallow-mcp", env!("CARGO_PKG_VERSION"))
                    .with_description("Codebase analysis for TypeScript/JavaScript projects"),
            )
            .with_instructions(
                "Fallow MCP server, codebase analysis for TypeScript/JavaScript projects. \
                 Tools: analyze (full analysis), check_changed (incremental/PR analysis), \
                 find_dupes (code duplication), fix_preview/fix_apply (auto-fix), \
                 project_info (plugins, files, entry points, boundary zones), \
                 trace_export / trace_file / trace_dependency / trace_clone (graph and clone evidence), \
                 check_health (code complexity metrics), \
                 check_runtime_coverage (paid; merges a V8 or Istanbul runtime coverage dump into the health report), \
                 get_hot_paths / get_blast_radius / get_importance / get_cleanup_candidates (paid runtime context slices), \
                 audit (combined dead-code + complexity + duplication for changed files, returns verdict), \
                 fallow_explain (rule rationale and fix guidance without running analysis), \
                 list_boundaries (architecture boundary zones and access rules), \
                 feature_flags (detect feature flag patterns). \
                 Picking check_health vs check_runtime_coverage: use check_runtime_coverage when you have a V8 or Istanbul coverage dump and want surfaced dead-in-production verdicts; use check_health for general complexity / hotspot / CRAP analysis without a coverage dump.",
            )
    }
}
