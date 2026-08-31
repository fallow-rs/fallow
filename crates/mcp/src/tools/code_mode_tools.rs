use std::sync::LazyLock;

use crate::params::{
    AnalyzeParams, AuditParams, CheckChangedParams, CheckRuntimeCoverageParams, CombinedParams,
    ExplainParams, FeatureFlagsParams, FindDupesParams, HealthParams, ImpactClosureParams,
    ImpactParams, ListBoundariesParams, ProjectInfoParams, SecurityCandidatesParams,
    TraceCloneParams, TraceDependencyParams, TraceExportParams, TraceFileParams,
};

use fallow_api::{
    AnalysisOptions, CombinedOptions, ComplexityOptions, CoverageInputs, DuplicationMode,
    DuplicationOptions, RootEnvelopeMode, run_combined, serialize_combined_programmatic_json,
    serialize_explain_programmatic_json,
};

use super::super::{
    api_runtime::{
        changed_since_from_param, env_diff_file, non_empty_path, programmatic_error_body,
        resolve_typed_coverage_inputs, workspace_patterns_from_param,
    },
    build_analyze_args, build_audit_args, build_check_changed_args,
    build_check_runtime_coverage_args, build_explain_args, build_feature_flags_args,
    build_find_dupes_args, build_get_blast_radius_args, build_get_cleanup_candidates_args,
    build_get_hot_paths_args, build_get_importance_args, build_health_args, build_impact_args,
    build_impact_closure_args, build_list_boundaries_args, build_project_info_args,
    build_security_candidates_args, build_trace_clone_args, build_trace_dependency_args,
    build_trace_export_args, build_trace_file_args,
    check_changed::run_check_changed_api_value,
    flags::run_feature_flags_api_value,
    list_boundaries::run_list_boundaries_api_value,
    project_info::run_project_info_api_value,
    push_global, push_remote_extends,
    trace::{
        run_trace_clone_api_value, run_trace_dependency_api_value, run_trace_export_api_value,
        run_trace_file_api_value,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CodeModeTool {
    Analyze,
    Combined,
    CheckChanged,
    SecurityCandidates,
    FindDupes,
    ProjectInfo,
    TraceExport,
    TraceFile,
    ImpactClosure,
    TraceDependency,
    TraceClone,
    CheckHealth,
    Audit,
    FallowExplain,
    ListBoundaries,
    FeatureFlags,
    Impact,
    CheckRuntimeCoverage,
    GetHotPaths,
    GetBlastRadius,
    GetImportance,
    GetCleanupCandidates,
}

impl CodeModeTool {
    /// Every variant, in `name()` order. Drift tests bind this list to the
    /// shared manifest in both directions, so a variant missing here (or a
    /// manifest row whose `code_mode_alias` is missing) fails the suite.
    #[cfg(test)]
    pub(super) const ALL: &'static [Self] = &[
        Self::Analyze,
        Self::Combined,
        Self::CheckChanged,
        Self::SecurityCandidates,
        Self::FindDupes,
        Self::ProjectInfo,
        Self::TraceExport,
        Self::TraceFile,
        Self::ImpactClosure,
        Self::TraceDependency,
        Self::TraceClone,
        Self::CheckHealth,
        Self::Audit,
        Self::FallowExplain,
        Self::ListBoundaries,
        Self::FeatureFlags,
        Self::Impact,
        Self::CheckRuntimeCoverage,
        Self::GetHotPaths,
        Self::GetBlastRadius,
        Self::GetImportance,
        Self::GetCleanupCandidates,
    ];

    /// Position of the variant in [`Self::ALL`]. A new variant makes this
    /// match non-exhaustive, which is the compile-time nudge to extend `ALL`
    /// as well; `all_lists_every_variant_in_order` proves the two agree.
    #[cfg(test)]
    const fn ordinal(self) -> usize {
        match self {
            Self::Analyze => 0,
            Self::Combined => 1,
            Self::CheckChanged => 2,
            Self::SecurityCandidates => 3,
            Self::FindDupes => 4,
            Self::ProjectInfo => 5,
            Self::TraceExport => 6,
            Self::TraceFile => 7,
            Self::ImpactClosure => 8,
            Self::TraceDependency => 9,
            Self::TraceClone => 10,
            Self::CheckHealth => 11,
            Self::Audit => 12,
            Self::FallowExplain => 13,
            Self::ListBoundaries => 14,
            Self::FeatureFlags => 15,
            Self::Impact => 16,
            Self::CheckRuntimeCoverage => 17,
            Self::GetHotPaths => 18,
            Self::GetBlastRadius => 19,
            Self::GetImportance => 20,
            Self::GetCleanupCandidates => 21,
        }
    }

    pub(super) fn from_name(name: &str) -> Result<Self, String> {
        match name {
            "analyze" => Ok(Self::Analyze),
            "combined" => Ok(Self::Combined),
            "check_changed" => Ok(Self::CheckChanged),
            "security_candidates" => Ok(Self::SecurityCandidates),
            "find_similar_code" | "inspect_similar_code" => Err(
                "similar-code is not exposed through Code Mode's 30-second window; use the standalone MCP find_similar_code or inspect_similar_code tool, which has a dedicated 15-minute timeout"
                    .to_string(),
            ),
            "find_dupes" => Ok(Self::FindDupes),
            "project_info" => Ok(Self::ProjectInfo),
            "trace_export" => Ok(Self::TraceExport),
            "trace_file" => Ok(Self::TraceFile),
            "impact_closure" => Ok(Self::ImpactClosure),
            "trace_dependency" => Ok(Self::TraceDependency),
            "trace_clone" => Ok(Self::TraceClone),
            "check_health" => Ok(Self::CheckHealth),
            "audit" => Ok(Self::Audit),
            "fallow_explain" => Ok(Self::FallowExplain),
            "list_boundaries" => Ok(Self::ListBoundaries),
            "feature_flags" => Ok(Self::FeatureFlags),
            "impact" => Ok(Self::Impact),
            "check_runtime_coverage" => Ok(Self::CheckRuntimeCoverage),
            "get_hot_paths" => Ok(Self::GetHotPaths),
            "get_blast_radius" => Ok(Self::GetBlastRadius),
            "get_importance" => Ok(Self::GetImportance),
            "get_cleanup_candidates" => Ok(Self::GetCleanupCandidates),
            "fix_preview" | "fix_apply" => Err(
                "code mode does not expose fix tools; use standalone MCP tools for previews"
                    .to_string(),
            ),
            _ => Err(format!("unsupported code mode fallow tool '{name}'")),
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Analyze => "analyze",
            Self::Combined => "combined",
            Self::CheckChanged => "check_changed",
            Self::SecurityCandidates => "security_candidates",
            Self::FindDupes => "find_dupes",
            Self::ProjectInfo => "project_info",
            Self::TraceExport => "trace_export",
            Self::TraceFile => "trace_file",
            Self::ImpactClosure => "impact_closure",
            Self::TraceDependency => "trace_dependency",
            Self::TraceClone => "trace_clone",
            Self::CheckHealth => "check_health",
            Self::Audit => "audit",
            Self::FallowExplain => "fallow_explain",
            Self::ListBoundaries => "list_boundaries",
            Self::FeatureFlags => "feature_flags",
            Self::Impact => "impact",
            Self::CheckRuntimeCoverage => "check_runtime_coverage",
            Self::GetHotPaths => "get_hot_paths",
            Self::GetBlastRadius => "get_blast_radius",
            Self::GetImportance => "get_importance",
            Self::GetCleanupCandidates => "get_cleanup_candidates",
        }
    }

    /// Which of Code Mode's two execution paths this host call takes.
    pub(super) fn backing(self) -> CodeModeBacking {
        if api_route(self).is_some() {
            CodeModeBacking::Api
        } else {
            CodeModeBacking::Subprocess
        }
    }
}

/// How Code Mode executes one host call.
///
/// Standalone MCP tools always prefer `fallow-api`. Code Mode is stricter,
/// because `timeout_ms` has to mean something inside a sandbox capped at 30
/// seconds, and `fallow-api` exposes no cancellation: once an in-process
/// analysis starts, nothing can stop it early.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CodeModeBacking {
    /// Runs in this process through `fallow-api`. `timeout_ms` is a response
    /// deadline rather than a stop signal: the host call returns on time, and
    /// the analysis behind it keeps running until it finishes on its own.
    Api,
    /// Runs as a child `fallow` process that is killed when `timeout_ms`
    /// expires. Either `fallow-api` has no programmatic route for the tool, or
    /// the route exists and Code Mode deliberately trades its speed for a run
    /// it can actually stop.
    Subprocess,
}

/// The in-process route for a host call, or `None` when the call belongs to a
/// child process.
///
/// This match is the only place a tool's backing is decided:
/// [`CodeModeTool::backing`] reads it and [`run_api_tool`] runs it, so neither
/// can claim a route the other does not have and no dispatch arm can go
/// unreachable.
fn api_route(tool: CodeModeTool) -> Option<ApiRoute> {
    match tool {
        CodeModeTool::Combined => Some(|params| {
            let params: CombinedParams = parse_params(params)?;
            run_combined_api_value(&params)
        }),
        CodeModeTool::CheckChanged => Some(|params| {
            let params: CheckChangedParams = parse_params(params)?;
            run_check_changed_api_value(&params)
        }),
        CodeModeTool::ProjectInfo => Some(|params| {
            let params: ProjectInfoParams = parse_params(params)?;
            run_project_info_api_value(&params)
        }),
        CodeModeTool::TraceExport => Some(|params| {
            let params: TraceExportParams = parse_params(params)?;
            run_trace_export_api_value(&params).map(Some)
        }),
        CodeModeTool::TraceFile => Some(|params| {
            let params: TraceFileParams = parse_params(params)?;
            run_trace_file_api_value(&params).map(Some)
        }),
        CodeModeTool::TraceDependency => Some(|params| {
            let params: TraceDependencyParams = parse_params(params)?;
            run_trace_dependency_api_value(&params).map(Some)
        }),
        CodeModeTool::TraceClone => Some(|params| {
            let params: TraceCloneParams = parse_params(params)?;
            run_trace_clone_api_value(&params).map(Some)
        }),
        CodeModeTool::FallowExplain => Some(|params| {
            let params: ExplainParams = parse_params(params)?;
            serialize_explain_programmatic_json(&params.issue_type, RootEnvelopeMode::Tagged, None)
                .map(Some)
                .map_err(|error| error.message)
        }),
        CodeModeTool::FeatureFlags => Some(|params| {
            let params: FeatureFlagsParams = parse_params(params)?;
            run_feature_flags_api_value(&params)
        }),
        CodeModeTool::ListBoundaries => Some(|params| {
            let params: ListBoundariesParams = parse_params(params)?;
            run_list_boundaries_api_value(&params)
        }),
        // `fallow-api` can run the first four (their standalone MCP tools do),
        // but a whole-project analysis cannot be cancelled once it starts, so
        // Code Mode gives up the in-process speed to keep `timeout_ms` able to
        // stop the work. The rest have no `fallow-api` route at all.
        CodeModeTool::Analyze
        | CodeModeTool::FindDupes
        | CodeModeTool::CheckHealth
        | CodeModeTool::Audit
        | CodeModeTool::SecurityCandidates
        | CodeModeTool::ImpactClosure
        | CodeModeTool::Impact
        | CodeModeTool::CheckRuntimeCoverage
        | CodeModeTool::GetHotPaths
        | CodeModeTool::GetBlastRadius
        | CodeModeTool::GetImportance
        | CodeModeTool::GetCleanupCandidates => None,
    }
}

/// An in-process host call: params in, serialized fallow JSON out.
type ApiRoute = fn(serde_json::Value) -> Result<Option<serde_json::Value>, String>;

/// The sandbox host API, as `(camelCase alias, wire tool name)` pairs,
/// projected from `fallow_types::mcp_manifest` so the allowlist has exactly
/// one source of truth. Adding a tool to Code Mode means setting
/// `code_mode_alias` on its manifest row, never editing a list here.
pub(super) static CODE_MODE_ALIASES: LazyLock<Vec<(&'static str, &'static str)>> =
    LazyLock::new(fallow_types::mcp_manifest::code_mode_allowlist);

/// Host-call aliases whose backing is [`CodeModeBacking::Subprocess`], so
/// `timeout_ms` kills them instead of only abandoning them. The `code_execute`
/// description names exactly this set, and a server test binds that prose to
/// this projection.
#[cfg(test)]
pub fn code_mode_subprocess_aliases() -> Vec<&'static str> {
    CODE_MODE_ALIASES
        .iter()
        .filter(|(_, name)| {
            CodeModeTool::from_name(name)
                .is_ok_and(|tool| tool.backing() == CodeModeBacking::Subprocess)
        })
        .map(|(alias, _)| *alias)
        .collect()
}

pub(super) fn merge_default_root(
    params_json: &str,
    default_root: Option<&str>,
) -> Result<serde_json::Value, String> {
    let mut params: serde_json::Value =
        serde_json::from_str(params_json).map_err(|err| format!("invalid params JSON: {err}"))?;
    if !params.is_object() {
        return Err("fallow host call params must be an object".to_string());
    }
    if let Some(root) = default_root
        && params.get("root").is_none()
        && let Some(object) = params.as_object_mut()
    {
        object.insert(
            "root".to_string(),
            serde_json::Value::String(root.to_string()),
        );
    }
    Ok(params)
}

pub(super) fn run_api_tool(
    tool: CodeModeTool,
    params: serde_json::Value,
) -> Result<Option<serde_json::Value>, String> {
    api_route(tool).map_or(Ok(None), |route| route(params))
}

pub(super) fn build_tool_args(
    tool: CodeModeTool,
    params: serde_json::Value,
) -> Result<Vec<String>, String> {
    match tool {
        CodeModeTool::Analyze
        | CodeModeTool::Combined
        | CodeModeTool::CheckChanged
        | CodeModeTool::SecurityCandidates
        | CodeModeTool::FindDupes
        | CodeModeTool::ProjectInfo => build_project_tool_args(tool, params),
        CodeModeTool::TraceExport
        | CodeModeTool::TraceFile
        | CodeModeTool::ImpactClosure
        | CodeModeTool::TraceDependency
        | CodeModeTool::TraceClone => build_trace_tool_args(tool, params),
        CodeModeTool::CheckHealth
        | CodeModeTool::Audit
        | CodeModeTool::FallowExplain
        | CodeModeTool::ListBoundaries
        | CodeModeTool::FeatureFlags
        | CodeModeTool::Impact => build_health_and_config_tool_args(tool, params),
        CodeModeTool::CheckRuntimeCoverage
        | CodeModeTool::GetHotPaths
        | CodeModeTool::GetBlastRadius
        | CodeModeTool::GetImportance
        | CodeModeTool::GetCleanupCandidates => build_runtime_coverage_tool_args(tool, params),
    }
}

fn build_project_tool_args(
    tool: CodeModeTool,
    params: serde_json::Value,
) -> Result<Vec<String>, String> {
    match tool {
        CodeModeTool::Analyze => {
            let params: AnalyzeParams = parse_params(params)?;
            build_analyze_args(&params)
        }
        CodeModeTool::Combined => {
            let params: CombinedParams = parse_params(params)?;
            Ok(build_combined_args(&params))
        }
        CodeModeTool::CheckChanged => {
            let params: CheckChangedParams = parse_params(params)?;
            Ok(build_check_changed_args(params))
        }
        CodeModeTool::SecurityCandidates => {
            let params: SecurityCandidatesParams = parse_params(params)?;
            build_security_candidates_args(&params)
        }
        CodeModeTool::FindDupes => {
            let params: FindDupesParams = parse_params(params)?;
            build_find_dupes_args(&params)
        }
        CodeModeTool::ProjectInfo => {
            let params: ProjectInfoParams = parse_params(params)?;
            Ok(build_project_info_args(&params))
        }
        _ => unreachable!("project tool helper called with non-project tool"),
    }
}

fn build_trace_tool_args(
    tool: CodeModeTool,
    params: serde_json::Value,
) -> Result<Vec<String>, String> {
    match tool {
        CodeModeTool::TraceExport => {
            let params: TraceExportParams = parse_params(params)?;
            build_trace_export_args(&params)
        }
        CodeModeTool::TraceFile => {
            let params: TraceFileParams = parse_params(params)?;
            build_trace_file_args(&params)
        }
        CodeModeTool::ImpactClosure => {
            let params: ImpactClosureParams = parse_params(params)?;
            build_impact_closure_args(&params)
        }
        CodeModeTool::TraceDependency => {
            let params: TraceDependencyParams = parse_params(params)?;
            build_trace_dependency_args(&params)
        }
        CodeModeTool::TraceClone => {
            let params: TraceCloneParams = parse_params(params)?;
            build_trace_clone_args(&params)
        }
        _ => unreachable!("trace tool helper called with non-trace tool"),
    }
}

fn build_health_and_config_tool_args(
    tool: CodeModeTool,
    params: serde_json::Value,
) -> Result<Vec<String>, String> {
    match tool {
        CodeModeTool::CheckHealth => {
            let params: HealthParams = parse_params(params)?;
            Ok(build_health_args(&params))
        }
        CodeModeTool::Audit => {
            let params: AuditParams = parse_params(params)?;
            build_audit_args(&params)
        }
        CodeModeTool::FallowExplain => {
            let params: ExplainParams = parse_params(params)?;
            Ok(build_explain_args(&params))
        }
        CodeModeTool::ListBoundaries => {
            let params: ListBoundariesParams = parse_params(params)?;
            Ok(build_list_boundaries_args(&params))
        }
        CodeModeTool::FeatureFlags => {
            let params: FeatureFlagsParams = parse_params(params)?;
            Ok(build_feature_flags_args(&params))
        }
        CodeModeTool::Impact => {
            let params: ImpactParams = parse_params(params)?;
            Ok(build_impact_args(&params))
        }
        _ => unreachable!("health/config helper called with unrelated tool"),
    }
}

fn build_runtime_coverage_tool_args(
    tool: CodeModeTool,
    params: serde_json::Value,
) -> Result<Vec<String>, String> {
    match tool {
        CodeModeTool::CheckRuntimeCoverage => {
            let params: CheckRuntimeCoverageParams = parse_params(params)?;
            Ok(build_check_runtime_coverage_args(&params))
        }
        CodeModeTool::GetHotPaths => {
            let params: CheckRuntimeCoverageParams = parse_params(params)?;
            Ok(build_get_hot_paths_args(&params))
        }
        CodeModeTool::GetBlastRadius => {
            let params: CheckRuntimeCoverageParams = parse_params(params)?;
            Ok(build_get_blast_radius_args(&params))
        }
        CodeModeTool::GetImportance => {
            let params: CheckRuntimeCoverageParams = parse_params(params)?;
            Ok(build_get_importance_args(&params))
        }
        CodeModeTool::GetCleanupCandidates => {
            let params: CheckRuntimeCoverageParams = parse_params(params)?;
            Ok(build_get_cleanup_candidates_args(&params))
        }
        _ => unreachable!("runtime coverage helper called with unrelated tool"),
    }
}

fn run_combined_api_value(params: &CombinedParams) -> Result<Option<serde_json::Value>, String> {
    let options = combined_options_from_params(params)?;
    let value = run_combined(&options)
        .and_then(serialize_combined_programmatic_json)
        .map_err(|err| programmatic_error_body(&err))?;

    Ok(Some(value))
}

fn combined_options_from_params(params: &CombinedParams) -> Result<CombinedOptions, String> {
    let analysis = AnalysisOptions {
        root: non_empty_path(params.root.as_deref()),
        config_path: non_empty_path(params.config.as_deref()),
        allow_remote_extends: params.allow_remote_extends.unwrap_or(false),
        no_cache: params.no_cache.unwrap_or(false),
        threads: params.threads,
        production: params.production.unwrap_or(false),
        production_override: params.production,
        changed_since: changed_since_from_param(params.changed_since.as_deref()),
        diff_file: env_diff_file(),
        workspace: workspace_patterns_from_param(params.workspace.as_deref()),
        explain: true,
        ..AnalysisOptions::default()
    };
    let coverage = resolve_typed_coverage_inputs(
        &analysis,
        CoverageInputs {
            coverage: non_empty_path(params.coverage.as_deref()),
            coverage_root: non_empty_path(params.coverage_root.as_deref()),
        },
        None,
        "combined.coverage_root",
    )
    .map_err(|error| programmatic_error_body(&error))?;
    Ok(CombinedOptions {
        analysis,
        include_entry_exports: params.include_entry_exports.unwrap_or(false),
        duplication_options: DuplicationOptions {
            mode: combined_duplication_mode(params.dupes_mode.as_deref())?,
            near: params.dupes_near,
            min_tokens: params.dupes_min_tokens.map(|value| value as usize),
            min_lines: params.dupes_min_lines.map(|value| value as usize),
            min_occurrences: params.dupes_min_occurrences.map(|value| value as usize),
            threshold: params.dupes_threshold,
            skip_local: params.dupes_skip_local,
            cross_language: params.dupes_cross_language,
            ignore_imports: params.dupes_ignore_imports,
            ..DuplicationOptions::default()
        },
        health_options: ComplexityOptions {
            max_cyclomatic: params.max_cyclomatic,
            max_cognitive: params.max_cognitive,
            max_crap: params.max_crap,
            coverage: coverage.coverage,
            coverage_root: coverage.coverage_root,
            complexity: params.complexity.unwrap_or(true),
            file_scores: params.file_scores.unwrap_or(true),
            hotspots: params.hotspots.unwrap_or(true),
            targets: params.targets.unwrap_or(true),
            score: params.score.unwrap_or(false),
            ..ComplexityOptions::default()
        },
        ..CombinedOptions::default()
    })
}

fn combined_duplication_mode(value: Option<&str>) -> Result<Option<DuplicationMode>, String> {
    match value {
        None | Some("") => Ok(None),
        Some("strict") => Ok(Some(DuplicationMode::Strict)),
        Some("mild") => Ok(Some(DuplicationMode::Mild)),
        Some("weak") => Ok(Some(DuplicationMode::Weak)),
        Some("semantic") => Ok(Some(DuplicationMode::Semantic)),
        Some(value) => Err(format!(
            "Invalid dupes_mode '{value}'. Valid values: strict, mild, weak, semantic"
        )),
    }
}

fn build_combined_args(params: &CombinedParams) -> Vec<String> {
    let mut args = vec![
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
        "--explain".to_string(),
    ];
    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        params.no_cache,
        params.threads,
    );
    push_remote_extends(&mut args, params.allow_remote_extends);
    if params.production == Some(true) {
        args.push("--production".to_string());
    }
    push_opt_arg(&mut args, "--workspace", params.workspace.as_deref());
    push_opt_arg(
        &mut args,
        "--changed-since",
        params.changed_since.as_deref(),
    );
    if params.include_entry_exports == Some(true) {
        args.push("--include-entry-exports".to_string());
    }
    push_combined_duplication_args(&mut args, params);
    if params.score == Some(true) {
        args.push("--score".to_string());
    }
    args
}

fn push_combined_duplication_args(args: &mut Vec<String>, params: &CombinedParams) {
    push_opt_arg(args, "--dupes-mode", params.dupes_mode.as_deref());
    if params.dupes_near == Some(true) {
        args.push("--dupes-near".to_string());
    }
    push_opt_arg(
        args,
        "--dupes-min-tokens",
        params
            .dupes_min_tokens
            .map(|value| value.to_string())
            .as_deref(),
    );
    push_opt_arg(
        args,
        "--dupes-min-lines",
        params
            .dupes_min_lines
            .map(|value| value.to_string())
            .as_deref(),
    );
    push_opt_arg(
        args,
        "--dupes-min-occurrences",
        params
            .dupes_min_occurrences
            .map(|value| value.to_string())
            .as_deref(),
    );
    push_opt_arg(
        args,
        "--dupes-threshold",
        params
            .dupes_threshold
            .map(|value| value.to_string())
            .as_deref(),
    );
    if params.dupes_skip_local == Some(true) {
        args.push("--dupes-skip-local".to_string());
    }
    if params.dupes_cross_language == Some(true) {
        args.push("--dupes-cross-language".to_string());
    }
    match params.dupes_ignore_imports {
        Some(true) => args.push("--dupes-ignore-imports".to_string()),
        Some(false) => args.push("--dupes-no-ignore-imports".to_string()),
        None => {}
    }
}

fn push_opt_arg(args: &mut Vec<String>, flag: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        args.extend([flag.to_string(), value.to_string()]);
    }
}

fn parse_params<T>(params: serde_json::Value) -> Result<T, String>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_value(params).map_err(|err| format!("invalid tool params: {err}"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use fallow_types::mcp_manifest::{MCP_TOOLS, code_mode_allowlist};

    use super::*;

    #[test]
    fn all_lists_every_variant_in_order() {
        for (index, tool) in CodeModeTool::ALL.iter().enumerate() {
            assert_eq!(
                tool.ordinal(),
                index,
                "CodeModeTool::ALL disagrees with ordinal() at slot {index}; both must list \
                 every variant in the same order"
            );
        }
    }

    /// The drift gate finding 5 was missing: `MCP_TOOLS` and the sandbox's
    /// dispatch enum must describe the same allowlist, so a tool cannot be
    /// added to (or dropped from) one surface while the other keeps its old
    /// answer.
    #[test]
    fn enum_variants_match_the_manifest_allowlist_both_directions() {
        let manifest: BTreeSet<&str> = code_mode_allowlist()
            .iter()
            .map(|(_, name)| *name)
            .collect();
        let variants: BTreeSet<&str> = CodeModeTool::ALL.iter().map(|tool| tool.name()).collect();
        assert_eq!(
            manifest, variants,
            "the Code Mode allowlist in fallow_types::mcp_manifest (code_mode_alias plus \
             CODE_MODE_ONLY_TOOLS) must equal the CodeModeTool variants exactly"
        );
    }

    #[test]
    fn manifest_tools_dispatch_exactly_when_they_carry_an_alias() {
        for tool in MCP_TOOLS {
            assert_eq!(
                CodeModeTool::from_name(tool.name).is_ok(),
                tool.code_mode_alias.is_some(),
                "Code Mode dispatch for {} disagrees with its manifest code_mode_alias",
                tool.name
            );
        }
    }

    #[test]
    fn aliases_project_the_manifest_and_resolve_to_their_tool() {
        assert_eq!(*CODE_MODE_ALIASES, code_mode_allowlist());
        for &(alias, name) in CODE_MODE_ALIASES.as_slice() {
            let tool = CodeModeTool::from_name(name)
                .unwrap_or_else(|err| panic!("alias {alias} maps to unknown tool {name}: {err}"));
            assert_eq!(tool.name(), name, "alias {alias} does not round-trip");
        }
    }

    /// Code Mode's two rejections are routing hints agents read, not
    /// accidents: the sandbox is read-only, and similar-code's cold-inference
    /// window does not fit the 30-second cap. Deriving the allowlist from the
    /// manifest must not flatten them into the generic unsupported-tool error.
    #[test]
    fn deliberate_rejections_keep_their_routing_hints() {
        for name in ["fix_preview", "fix_apply"] {
            let err = CodeModeTool::from_name(name).expect_err("fix tools stay out of Code Mode");
            assert!(
                err.contains("code mode does not expose fix tools"),
                "error was: {err}"
            );
        }
        for name in ["find_similar_code", "inspect_similar_code"] {
            let err =
                CodeModeTool::from_name(name).expect_err("similar-code stays out of Code Mode");
            assert!(
                err.contains("standalone MCP find_similar_code or inspect_similar_code"),
                "error was: {err}"
            );
            assert!(
                err.contains("dedicated 15-minute timeout"),
                "error was: {err}"
            );
        }
    }

    #[test]
    fn subprocess_aliases_are_every_host_call_a_timeout_can_kill() {
        let mut aliases = code_mode_subprocess_aliases();
        aliases.sort_unstable();
        assert_eq!(
            aliases,
            [
                "analyze",
                "audit",
                "checkHealth",
                "checkRuntimeCoverage",
                "findDupes",
                "getBlastRadius",
                "getCleanupCandidates",
                "getHotPaths",
                "getImportance",
                "impact",
                "impactClosure",
                "securityCandidates",
            ]
        );
    }

    #[test]
    fn in_process_host_calls_are_explicitly_registered() {
        let names = CodeModeTool::ALL
            .iter()
            .filter(|tool| tool.backing() == CodeModeBacking::Api)
            .map(|tool| tool.name())
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                "combined",
                "check_changed",
                "project_info",
                "trace_export",
                "trace_file",
                "trace_dependency",
                "trace_clone",
                "fallow_explain",
                "list_boundaries",
                "feature_flags",
            ]
        );
    }

    /// `fallow-api` can run these four, and their standalone MCP tools do.
    /// Code Mode does not, because it has to be able to stop them.
    #[test]
    fn whole_project_analyses_keep_the_killable_subprocess_path() {
        for tool in [
            CodeModeTool::Analyze,
            CodeModeTool::FindDupes,
            CodeModeTool::CheckHealth,
            CodeModeTool::Audit,
        ] {
            assert_eq!(
                tool.backing(),
                CodeModeBacking::Subprocess,
                "{} must stay killable in Code Mode",
                tool.name()
            );
        }
    }

    #[test]
    fn combined_params_default_to_cli_combined_health_sections() {
        let options =
            combined_options_from_params(&CombinedParams::default()).expect("combined options");

        assert!(options.health_options.complexity);
        assert!(options.health_options.file_scores);
        assert!(options.health_options.hotspots);
        assert!(options.health_options.targets);
        assert!(!options.health_options.score);
    }

    #[test]
    fn combined_params_forward_explicit_coverage_inputs() {
        let options = combined_options_from_params(&CombinedParams {
            coverage: Some("artifacts/coverage-final.json".to_string()),
            coverage_root: Some("/ci/workspace".to_string()),
            ..CombinedParams::default()
        })
        .expect("combined options");

        assert_eq!(
            options.health_options.coverage.as_deref(),
            Some(std::path::Path::new("artifacts/coverage-final.json"))
        );
        assert_eq!(
            options.health_options.coverage_root.as_deref(),
            Some(std::path::Path::new("/ci/workspace"))
        );
    }

    #[test]
    fn combined_forwards_near_detection() {
        let params = CombinedParams {
            dupes_near: Some(true),
            ..CombinedParams::default()
        };
        let options = combined_options_from_params(&params).expect("combined options");
        let args = build_combined_args(&params);

        assert_eq!(options.duplication_options.near, Some(true));
        assert!(args.contains(&"--dupes-near".to_string()));
    }

    #[test]
    fn combined_args_preserve_ignore_imports_override() {
        let args = build_combined_args(&CombinedParams {
            dupes_ignore_imports: Some(true),
            ..CombinedParams::default()
        });

        assert!(args.contains(&"--dupes-ignore-imports".to_string()));
        assert!(!args.contains(&"--dupes-no-ignore-imports".to_string()));

        let args = build_combined_args(&CombinedParams {
            dupes_ignore_imports: Some(false),
            ..CombinedParams::default()
        });

        assert!(args.contains(&"--dupes-no-ignore-imports".to_string()));
        assert!(!args.contains(&"--dupes-ignore-imports".to_string()));
    }

    #[test]
    fn subprocess_tools_forward_remote_extends_only_when_explicitly_enabled() {
        for tool in [
            CodeModeTool::Analyze,
            CodeModeTool::Combined,
            CodeModeTool::FindDupes,
            CodeModeTool::CheckHealth,
            CodeModeTool::Audit,
        ] {
            for (value, expected) in [(Some(true), true), (Some(false), false), (None, false)] {
                let params = value.map_or_else(
                    || serde_json::json!({}),
                    |value| serde_json::json!({ "allow_remote_extends": value }),
                );
                let args = build_tool_args(tool, params).expect("subprocess arguments");

                assert_eq!(
                    args.contains(&"--allow-remote-extends".to_string()),
                    expected,
                    "{} with allow_remote_extends={value:?}",
                    tool.name()
                );
            }
        }
    }

    #[test]
    fn check_changed_builder_forwards_remote_extends_only_when_explicitly_enabled() {
        for (value, expected) in [(Some(true), true), (Some(false), false), (None, false)] {
            let mut params = serde_json::json!({ "since": "main" });
            if let Some(value) = value {
                params["allow_remote_extends"] = serde_json::json!(value);
            }
            let args = build_tool_args(CodeModeTool::CheckChanged, params)
                .expect("check_changed arguments");

            assert_eq!(
                args.contains(&"--allow-remote-extends".to_string()),
                expected,
                "allow_remote_extends={value:?}"
            );
        }
    }

    #[test]
    fn config_listing_builders_forward_remote_extends_only_when_explicitly_enabled() {
        for tool in [
            CodeModeTool::ProjectInfo,
            CodeModeTool::ListBoundaries,
            CodeModeTool::FeatureFlags,
        ] {
            for (value, expected) in [(Some(true), true), (Some(false), false), (None, false)] {
                let params = value.map_or_else(
                    || serde_json::json!({}),
                    |value| serde_json::json!({ "allow_remote_extends": value }),
                );
                let args = build_tool_args(tool, params).expect("config listing arguments");

                assert_eq!(
                    args.contains(&"--allow-remote-extends".to_string()),
                    expected,
                    "{} with allow_remote_extends={value:?}",
                    tool.name()
                );
            }
        }
    }

    /// The modelled backing and the dispatcher cannot disagree: every
    /// subprocess-backed tool must decline in-process execution rather than
    /// reach a route that does not exist.
    #[test]
    fn subprocess_backed_tools_decline_in_process_dispatch() {
        for tool in CodeModeTool::ALL
            .iter()
            .filter(|tool| tool.backing() == CodeModeBacking::Subprocess)
        {
            assert_eq!(
                run_api_tool(*tool, serde_json::json!({})).expect("fallback decision"),
                None,
                "{} must fall back to its subprocess",
                tool.name()
            );
        }
    }
}
