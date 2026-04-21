use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo};
use rmcp::{ErrorData as McpError, ServerHandler, tool, tool_router};

use crate::params::{
    AnalyzeParams, AuditParams, CheckChangedParams, FeatureFlagsParams, FindDupesParams, FixParams,
    HealthParams, HealthProductionCoverageParams, ListBoundariesParams, ProjectInfoParams,
};
use crate::tools::{
    build_analyze_args, build_audit_args, build_check_changed_args, build_feature_flags_args,
    build_find_dupes_args, build_fix_apply_args, build_fix_preview_args, build_health_args,
    build_health_production_coverage_args, build_list_boundaries_args, build_project_info_args,
    run_fallow,
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
        description = "Analyze a TypeScript/JavaScript project for unused code and circular dependencies. Detects unused files, exports, types, dependencies, enum/class members, unresolved imports, unlisted dependencies, duplicate exports, circular dependencies, boundary violations, and stale suppression comments. Returns structured JSON with all issues found, grouped by issue type. For code duplication use find_dupes, for complexity hotspots use check_health. Supports baseline comparisons (baseline/save_baseline), regression detection (fail_on_regression, tolerance, regression_baseline, save_regression_baseline), and performance tuning (no_cache, threads). Set boundary_violations=true to check only architecture boundary violations (convenience alias for issue_types: [\"boundary-violations\"]). Set group_by to \"owner\" (CODEOWNERS), \"directory\", \"package\" (workspace), or \"section\" (GitLab CODEOWNERS `[Section]` headers, with `owners` metadata per group) to group results.",
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
        description = "Find code duplication across the project. Detects clone groups (identical or similar code blocks) with configurable detection modes and thresholds. Returns clone families with refactoring suggestions. Set top=N to show only the N largest clone groups. Set group_by to \"owner\" (CODEOWNERS), \"directory\", \"package\" (workspace), or \"section\" (GitLab CODEOWNERS `[Section]` headers, with `owners` metadata per group) to partition results. Supports config, workspace scoping, baseline comparisons, and performance tuning (no_cache, threads).",
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
        description = "Apply auto-fixes to the project. Removes unused export keywords from source files and deletes unused dependencies from package.json. This modifies files on disk. Use fix_preview first to review planned changes. Supports workspace scoping and performance tuning (no_cache, threads).",
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
        description = "Check code health metrics (cyclomatic and cognitive complexity) for functions in the project. Returns structured JSON with complexity scores per function, sorted by severity. Set score=true for a single 0-100 health score with letter grade (A/B/C/D/F); forces full pipeline for accuracy. Set min_score=N to fail if score drops below a threshold (CI quality gate). Set file_scores=true for per-file maintainability index (fan-in, fan-out, dead code ratio, complexity density). Set coverage_gaps=true to explicitly include static test coverage gaps: runtime files and exports with no test dependency path (not line-level coverage). A provided config file may also enable coverage gaps via rules.coverage-gaps when no health sections are explicitly selected. Set hotspots=true to identify files that are both complex and frequently changing (combines git churn with complexity). Set ownership=true (implies hotspots) to attach per-file ownership signals: bus factor, contributor count, declared CODEOWNERS owner, drift, and unowned-hotspot flag. Use ownership_email_mode=raw|handle|hash for author email privacy (default handle). Set targets=true for ranked refactoring recommendations sorted by efficiency (quick wins first), with confidence scores and adaptive percentile-based thresholds. Set trend=true to compare current metrics against the most recent saved snapshot and show per-metric deltas with directional indicators (improving/declining/stable). Implies --score. Requires prior snapshots saved with save_snapshot. Set effort to control analysis depth: 'low' (fast, surface-level), 'medium' (balanced, default), or 'high' (thorough, all heuristics). Set summary=true to include a natural-language summary of findings alongside the structured JSON. Set coverage to a path to Istanbul-format coverage data (coverage-final.json from Jest, Vitest, c8, nyc) for accurate per-function CRAP scores instead of the default static binary model. Set production_coverage to a path (V8 coverage directory, V8 JSON file, or Istanbul JSON file) for merged runtime production-coverage findings (paid feature; requires an active license via `fallow license activate`). Set min_invocations_hot=N to tune the hot-path threshold used by production-coverage output (default 100). Set group_by to \"owner\" (CODEOWNERS), \"directory\", \"package\" (workspace), or \"section\" (GitLab CODEOWNERS `[Section]` headers, with `owners` metadata per group) to partition results. Supports config, baseline comparisons, and performance tuning (no_cache, threads). Useful for identifying hard-to-maintain code and prioritizing refactoring.",
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
        description = "Audit changed files for dead code, complexity, and duplication. Purpose-built for reviewing AI-generated code. Combines dead-code + complexity + duplication scoped to changed files and returns a verdict (pass/warn/fail). Auto-detects the base branch if not specified. Returns JSON with verdict, summary counts per category, and full issue details with actions array for auto-correction. Set group_by to \"owner\" (CODEOWNERS), \"directory\", \"package\" (workspace), or \"section\" (GitLab CODEOWNERS `[Section]` headers, with `owners` metadata per group) to partition results. Set dead_code_baseline, health_baseline, and/or dupes_baseline to per-analysis baseline file paths (as saved by `fallow dead-code|health|dupes --save-baseline`) so pre-existing issues on touched files do not dominate the verdict; only new issues not present in the respective baseline contribute. Use this after generating code to verify quality before committing.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn audit(&self, params: Parameters<AuditParams>) -> Result<CallToolResult, McpError> {
        let args = build_audit_args(&params.0);
        run_fallow(&self.binary, &args).await
    }

    #[tool(
        description = "List architecture boundary zones and access rules configured for the project. Returns zone definitions (name, glob patterns, matched file count) and access rules (which zones may import from which). If boundaries are not configured, returns {\"configured\": false} — in that case, boundary violation checks will find no issues and can be skipped. Use this to understand the project's architecture constraints before running analysis.",
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
        description = "Merge runtime production-coverage data into the health report (paid feature). Focused entry point for the production-coverage pipeline: pass a V8 coverage directory (`NODE_V8_COVERAGE=<dir>`), a single V8 coverage JSON file, or an Istanbul `coverage-final.json` via the required `coverage` field. Requires an active license JWT (start a 30-day trial with `fallow license activate --trial --email <addr>`). Returns structured JSON with a `production_coverage` block containing per-finding `verdict` (`safe_to_delete` / `review_required` / `low_traffic` / `coverage_unavailable` / `active`), stable content-hash IDs (`fallow:prod:<hash>`), evidence, percentile-ranked hot paths, and on protocol-0.3+ sidecars a `summary.capture_quality` block that flags short-window captures. Tunable via `min_invocations_hot` (hot-path threshold), `min_observation_volume` (high-confidence verdict floor), and `low_traffic_threshold` (active/low_traffic split). Production coverage can exceed the default 120s MCP subprocess timeout on multi-megabyte dumps; raise `FALLOW_TIMEOUT_SECS` accordingly. For general complexity / hotspot / CRAP analysis without a production dump, use `check_health` instead.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn health_production_coverage(
        &self,
        params: Parameters<HealthProductionCoverageParams>,
    ) -> Result<CallToolResult, McpError> {
        let args = build_health_production_coverage_args(&params.0);
        run_fallow(&self.binary, &args).await
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
                 check_health (code complexity metrics), \
                 health_production_coverage (paid; merges a V8 or Istanbul production coverage dump into the health report), \
                 audit (combined dead-code + complexity + duplication for changed files, returns verdict), \
                 list_boundaries (architecture boundary zones and access rules), \
                 feature_flags (detect feature flag patterns).",
            )
    }
}
