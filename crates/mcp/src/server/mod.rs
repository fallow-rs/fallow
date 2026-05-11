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
        description = "Analyze a TypeScript/JavaScript project for unused code and circular dependencies. Detects unused files, exports, types, dependencies, enum/class members, unresolved imports, unlisted dependencies, duplicate exports, circular dependencies, boundary violations, stale suppression comments, and unused pnpm catalog entries (entries in pnpm-workspace.yaml `catalog:` / `catalogs:` not referenced by any workspace package). Private type leaks are an opt-in API hygiene check via issue_types: [\"private-type-leaks\"]. Returns structured JSON with all issues found, grouped by issue type. For code duplication use find_dupes, for complexity hotspots use check_health. Supports baseline comparisons (baseline/save_baseline), regression detection (fail_on_regression, tolerance, regression_baseline, save_regression_baseline), and performance tuning (no_cache, threads). Set boundary_violations=true to check only architecture boundary violations (convenience alias for issue_types: [\"boundary-violations\"]). Set group_by to \"owner\" (CODEOWNERS), \"directory\", \"package\" (workspace), or \"section\" to group results. The `section` mode reads GitLab CODEOWNERS `[Section]` headers and emits `owners` metadata per group.",
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
        description = "Find code duplication across the project. Detects clone groups (identical or similar code blocks) with configurable detection modes and thresholds. Returns clone families with refactoring suggestions. Set top=N to show only the N largest clone groups. Set group_by to \"owner\" (CODEOWNERS), \"directory\", \"package\" (workspace), or \"section\" (GitLab CODEOWNERS `[Section]` headers, with `owners` metadata per group) to partition results. explain_skipped only changes the human-format skipped-default-ignores note (human/markdown CLI output); MCP JSON responses stay clean. Supports config, workspace scoping, baseline comparisons, and performance tuning (no_cache, threads).",
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
        description = "Preview auto-fixes without modifying any files. Shows what would be changed: which unused exports would be removed and which unused dependencies would be deleted from package.json. Returns a JSON list of planned fixes. Supports workspace scoping and performance tuning (no_cache, threads).",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn fix_preview(&self, params: Parameters<FixParams>) -> Result<CallToolResult, McpError> {
        let args = build_fix_preview_args(&params.0);
        run_fallow(&self.binary, &args).await
    }

    #[tool(
        description = "Apply auto-fixes to the project. Removes unused exports from the public API, may delete dead exported enum declarations, and deletes unused dependencies from package.json. This modifies files on disk. Use fix_preview first to review planned changes. Supports workspace scoping and performance tuning (no_cache, threads).",
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
        description = "Trace duplicate-code groups containing a given file and line. Returns the matched clone instance plus every clone group that contains it. Useful when an agent wants to consolidate duplication but needs the exact sibling locations first.",
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
        description = "Check code health metrics (cyclomatic and cognitive complexity) for functions in the project. Returns structured JSON with complexity scores per function, sorted by severity. Set score=true for a single 0-100 health score with letter grade (A/B/C/D/F); runs duplication analysis automatically, but the churn-backed hotspot penalty requires hotspots=true (or targets=true). Set min_score=N to fail if score drops below a threshold (CI quality gate). Set file_scores=true for per-file maintainability index (fan-in, fan-out, dead code ratio, complexity density). Set coverage_gaps=true to explicitly include static test coverage gaps: runtime files and exports with no test dependency path (not line-level coverage). A provided config file may also enable coverage gaps via rules.coverage-gaps when no health sections are explicitly selected. Set hotspots=true to identify files that are both complex and frequently changing (combines git churn with complexity). Set ownership=true (implies hotspots) to attach per-file ownership signals: bus factor, contributor count, declared CODEOWNERS owner, drift, and unowned-hotspot flag. Use ownership_email_mode=raw|handle|hash for author email privacy (default handle). Set targets=true for ranked refactoring recommendations sorted by efficiency (quick wins first), with confidence scores and adaptive percentile-based thresholds. Set trend=true to compare current metrics against the most recent saved snapshot and show per-metric deltas with directional indicators (improving/declining/stable). Implies --score. Requires prior snapshots saved with save_snapshot. Set effort to control analysis depth: 'low' (fast, surface-level), 'medium' (balanced, default), or 'high' (thorough, all heuristics). Set summary=true to include a natural-language summary of findings alongside the structured JSON. Set coverage to a path to Istanbul-format coverage data (coverage-final.json from Jest, Vitest, c8, nyc) for accurate per-function CRAP scores instead of the default static binary model. Set runtime_coverage to a path (V8 coverage directory, V8 JSON file, or Istanbul JSON file) for merged runtime runtime-coverage findings (paid feature; requires an active license via `fallow license activate`). Set min_invocations_hot=N to tune the hot-path threshold used by runtime-coverage output (default 100). Set group_by to \"owner\" (CODEOWNERS), \"directory\", \"package\" (workspace), or \"section\" (GitLab CODEOWNERS `[Section]` headers, with `owners` metadata per group) to partition results. Each group gets its own `vital_signs`, `health_score`, `findings`, `file_scores`, `hotspots`, `large_functions`, and `targets` recomputed against the group's files (top-level metrics stay project-wide). Use this to answer per-team or per-package quality questions like \"which workspace has the worst maintainability?\" without running fallow once per package. Supports config, baseline comparisons, and performance tuning (no_cache, threads). Useful for identifying hard-to-maintain code and prioritizing refactoring.",
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
        description = "Audit changed files for dead code, complexity, and duplication. Purpose-built for reviewing AI-generated code. Combines dead-code + complexity + duplication scoped to changed files and returns a verdict (pass/warn/fail). Auto-detects the base branch if not specified. By default, audit runs the base ref too and gates only findings introduced by the changeset; inherited findings are annotated with introduced=false and counted under attribution. Set gate=\"all\" or audit.gate=\"all\" in config to gate every finding. Returns JSON with verdict, summary counts per category, attribution counts, and full issue details with actions array for auto-correction. Set coverage to an Istanbul coverage-final.json path and coverage_root to an absolute coverage-data path prefix when paths need rebasing for accurate CRAP scoring in the health sub-analysis. Set group_by to \"owner\" (CODEOWNERS), \"directory\", \"package\" (workspace), or \"section\" (GitLab CODEOWNERS `[Section]` headers, with `owners` metadata per group) to partition results. Set dead_code_baseline, health_baseline, and/or dupes_baseline to per-analysis baseline file paths (as saved by `fallow dead-code|health|dupes --save-baseline`) so pre-existing issues on touched files do not dominate the verdict; only new issues not present in the respective baseline contribute. explain_skipped only changes the human-format skipped-default-ignores note (human/markdown CLI output); MCP JSON responses stay clean. Set include_entry_exports=true to also report unused exports in entry files (catches typos in framework exports like `meatdata` vs `metadata`); the CLI flag ORs with the `includeEntryExports` config value. Set runtime_coverage to a V8 coverage directory, V8 JSON file, or Istanbul coverage-final.json to fold runtime-coverage findings into the same audit invocation: agents get the `hot-path-touched` verdict alongside dead-code and complexity in one call (License-gated; informational verdict, no exit-code change). When `FALLOW_DIFF_FILE` or `--changed-since` supplies a change scope, `runtime_coverage.verdict` promotes `hot-path-touched` over `cold-code-detected` for PR-review contexts; the full unprioritized signal set is in `runtime_coverage.signals[]`. Use this after generating code to verify quality before committing.",
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
        description = "List architecture boundary zones and access rules configured for the project. Returns zone definitions (name, glob patterns, matched file count) and access rules (which zones may import from which). If boundaries are not configured, returns {\"configured\": false}; in that case, boundary violation checks will find no issues and can be skipped. Use this to understand the project's architecture constraints before running analysis.",
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
        description = "Detect feature flag patterns in a TypeScript/JavaScript project. Identifies environment variable flags (process.env.FEATURE_*), SDK calls (LaunchDarkly, Statsig, Unleash, GrowthBook), and config object patterns. Returns flag locations, detection confidence, and cross-reference with dead code findings.",
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
        description = "Merge runtime-coverage data into the health report. Focused entry point for the runtime-coverage pipeline: pass a V8 coverage directory (`NODE_V8_COVERAGE=<dir>`), a single V8 coverage JSON file, or an Istanbul `coverage-final.json` via the required `coverage` field. A single local capture is free and runs without a license; continuous or multi-capture runtime monitoring (multiple JSON files in a V8 directory) requires an active license JWT (start a 30-day trial with `fallow license activate --trial --email <addr>`; check state with `fallow license status`). Returns structured JSON with a `runtime_coverage` block containing surfaced `findings` verdicts (`safe_to_delete` / `review_required` / `low_traffic` / `coverage_unavailable`), stable content-hash IDs (`fallow:prod:<hash>`), evidence, percentile-ranked hot paths (each with `start_line` and `end_line` so consumers can match against a PR diff), and on protocol-0.3+ sidecars a `summary.capture_quality` block that flags short-window captures. The sidecar may still classify other functions as `active`, but the CLI omits those from `runtime_coverage.findings` to keep the surfaced list actionable. Tunable via `min_invocations_hot` (hot-path threshold, default 100), `min_observation_volume` (high-confidence verdict floor, default 5000), and `low_traffic_threshold` (active/low_traffic split, default 0.001). `group_by` partitions results by CODEOWNERS / directory / package / section. PR-context behavior: when `FALLOW_DIFF_FILE` (path to a unified diff) or `FALLOW_CHANGED_SINCE` (git ref) is set in the agent's process environment, the top-level `runtime_coverage.verdict` promotes `hot-path-touched` over `cold-code-detected` so reviewers see the diff-tied signal first; without a change scope the verdict stays cold-code-primary. The full unprioritized list is always in `runtime_coverage.signals[]` (kebab-case strings, severity-descending). Runtime coverage can exceed the default 120s MCP subprocess timeout on multi-megabyte dumps; raise `FALLOW_TIMEOUT_SECS` accordingly. For general complexity / hotspot / CRAP analysis without a production dump, use `check_health` instead.",
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
        description = "Return production hot paths from a local V8 or Istanbul runtime coverage dump. Pass `coverage` as a V8 coverage directory, single V8 JSON file, or Istanbul `coverage-final.json`. A single local capture is free and runs without a license; continuous or multi-capture runtime monitoring requires an active license. Returns the standard health JSON; agents should read `runtime_coverage.hot_paths`, which is sorted by percentile and invocation count. Each entry carries `start_line` and `end_line` so agents can match the function's full body against a PR diff. Use `top` to cap the returned hot paths. Environment-driven scoping: if `FALLOW_DIFF_FILE` (path to a unified diff) or `FALLOW_CHANGED_SINCE` (git ref) is set in the agent's process env, hot paths are scoped to that change set (line-level for the diff, file-level for changed-since); unset both for project-wide hot paths.",
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
