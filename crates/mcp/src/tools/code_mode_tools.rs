use crate::params::{
    AnalyzeParams, AuditParams, CheckChangedParams, CheckRuntimeCoverageParams, CombinedParams,
    ExplainParams, FeatureFlagsParams, FindDupesParams, HealthParams, ImpactParams,
    ListBoundariesParams, ProjectInfoParams, SecurityCandidatesParams, TraceCloneParams,
    TraceDependencyParams, TraceExportParams, TraceFileParams,
};

use fallow_api::{
    AnalysisOptions, CombinedOptions, ComplexityOptions, DuplicationMode, DuplicationOptions,
    RootEnvelopeMode, run_combined, serialize_combined_programmatic_json,
    serialize_explain_programmatic_json,
};

use super::super::{
    analyze::run_analyze_api_value,
    api_runtime::{changed_since_from_param, env_diff_file, non_empty_path, non_empty_string},
    audit::run_audit_api_value,
    build_analyze_args, build_audit_args, build_check_changed_args,
    build_check_runtime_coverage_args, build_explain_args, build_feature_flags_args,
    build_find_dupes_args, build_get_blast_radius_args, build_get_cleanup_candidates_args,
    build_get_hot_paths_args, build_get_importance_args, build_health_args, build_impact_args,
    build_list_boundaries_args, build_project_info_args, build_security_candidates_args,
    build_trace_clone_args, build_trace_dependency_args, build_trace_export_args,
    build_trace_file_args,
    check_changed::run_check_changed_api_value,
    dupes::run_find_dupes_api_value,
    flags::run_feature_flags_api_value,
    health::run_health_api_value,
    list_boundaries::run_list_boundaries_api_value,
    project_info::run_project_info_api_value,
    push_global,
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
    pub(super) fn from_name(name: &str) -> Result<Self, String> {
        match name {
            "analyze" => Ok(Self::Analyze),
            "combined" => Ok(Self::Combined),
            "check_changed" => Ok(Self::CheckChanged),
            "security_candidates" => Ok(Self::SecurityCandidates),
            "find_dupes" => Ok(Self::FindDupes),
            "project_info" => Ok(Self::ProjectInfo),
            "trace_export" => Ok(Self::TraceExport),
            "trace_file" => Ok(Self::TraceFile),
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

    pub(super) fn is_api_backed(self) -> bool {
        API_BACKED_CODE_MODE_TOOLS.contains(&self)
    }

    pub(super) fn is_code_mode_api_backed(self) -> bool {
        self.is_api_backed()
            && !matches!(
                self,
                Self::Analyze | Self::FindDupes | Self::CheckHealth | Self::Audit
            )
    }
}

pub(super) const CODE_MODE_ALIASES: &[(&str, &str)] = &[
    ("analyze", "analyze"),
    ("combined", "combined"),
    ("checkChanged", "check_changed"),
    ("securityCandidates", "security_candidates"),
    ("findDupes", "find_dupes"),
    ("projectInfo", "project_info"),
    ("traceExport", "trace_export"),
    ("traceFile", "trace_file"),
    ("traceDependency", "trace_dependency"),
    ("traceClone", "trace_clone"),
    ("checkHealth", "check_health"),
    ("audit", "audit"),
    ("explain", "fallow_explain"),
    ("listBoundaries", "list_boundaries"),
    ("featureFlags", "feature_flags"),
    ("impact", "impact"),
    ("checkRuntimeCoverage", "check_runtime_coverage"),
    ("getHotPaths", "get_hot_paths"),
    ("getBlastRadius", "get_blast_radius"),
    ("getImportance", "get_importance"),
    ("getCleanupCandidates", "get_cleanup_candidates"),
];

pub(super) const API_BACKED_CODE_MODE_TOOLS: &[CodeModeTool] = &[
    CodeModeTool::Analyze,
    CodeModeTool::Combined,
    CodeModeTool::CheckChanged,
    CodeModeTool::FindDupes,
    CodeModeTool::ProjectInfo,
    CodeModeTool::TraceExport,
    CodeModeTool::TraceFile,
    CodeModeTool::TraceDependency,
    CodeModeTool::TraceClone,
    CodeModeTool::CheckHealth,
    CodeModeTool::Audit,
    CodeModeTool::FallowExplain,
    CodeModeTool::ListBoundaries,
    CodeModeTool::FeatureFlags,
];

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
    if !tool.is_api_backed() {
        return Ok(None);
    }

    match tool {
        CodeModeTool::Analyze => {
            let params: AnalyzeParams = parse_params(params)?;
            run_analyze_api_value(&params)
        }
        CodeModeTool::Combined => {
            let params: CombinedParams = parse_params(params)?;
            run_combined_api_value(&params)
        }
        CodeModeTool::CheckChanged => {
            let params: CheckChangedParams = parse_params(params)?;
            run_check_changed_api_value(&params)
        }
        CodeModeTool::FindDupes => {
            let params: FindDupesParams = parse_params(params)?;
            run_find_dupes_api_value(&params)
        }
        CodeModeTool::ProjectInfo => {
            let params: ProjectInfoParams = parse_params(params)?;
            run_project_info_api_value(&params)
        }
        CodeModeTool::TraceExport => {
            let params: TraceExportParams = parse_params(params)?;
            run_trace_export_api_value(&params).map(Some)
        }
        CodeModeTool::TraceFile => {
            let params: TraceFileParams = parse_params(params)?;
            run_trace_file_api_value(&params).map(Some)
        }
        CodeModeTool::TraceDependency => {
            let params: TraceDependencyParams = parse_params(params)?;
            run_trace_dependency_api_value(&params).map(Some)
        }
        CodeModeTool::TraceClone => {
            let params: TraceCloneParams = parse_params(params)?;
            run_trace_clone_api_value(&params).map(Some)
        }
        CodeModeTool::CheckHealth => {
            let params: HealthParams = parse_params(params)?;
            run_health_api_value(&params)
        }
        CodeModeTool::Audit => {
            let params: AuditParams = parse_params(params)?;
            run_audit_api_value(&params)
        }
        CodeModeTool::FallowExplain => {
            let params: ExplainParams = parse_params(params)?;
            serialize_explain_programmatic_json(&params.issue_type, RootEnvelopeMode::Tagged, None)
                .map(Some)
                .map_err(|error| error.message)
        }
        CodeModeTool::FeatureFlags => {
            let params: FeatureFlagsParams = parse_params(params)?;
            run_feature_flags_api_value(&params)
        }
        CodeModeTool::ListBoundaries => {
            let params: ListBoundariesParams = parse_params(params)?;
            run_list_boundaries_api_value(&params)
        }
        CodeModeTool::SecurityCandidates
        | CodeModeTool::Impact
        | CodeModeTool::CheckRuntimeCoverage
        | CodeModeTool::GetHotPaths
        | CodeModeTool::GetBlastRadius
        | CodeModeTool::GetImportance
        | CodeModeTool::GetCleanupCandidates => unreachable!(
            "{} is not API-backed and should have returned before dispatch",
            tool.name()
        ),
    }
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
        .map_err(|err| err.to_string())?;

    Ok(Some(value))
}

fn combined_options_from_params(params: &CombinedParams) -> Result<CombinedOptions, String> {
    Ok(CombinedOptions {
        analysis: AnalysisOptions {
            root: non_empty_path(params.root.as_deref()),
            config_path: non_empty_path(params.config.as_deref()),
            no_cache: params.no_cache.unwrap_or(false),
            threads: params.threads,
            production: params.production.unwrap_or(false),
            production_override: params.production,
            changed_since: changed_since_from_param(params.changed_since.as_deref()),
            diff_file: env_diff_file(),
            workspace: non_empty_string(params.workspace.as_deref())
                .map(|workspace| vec![workspace]),
            explain: true,
            ..AnalysisOptions::default()
        },
        include_entry_exports: params.include_entry_exports.unwrap_or(false),
        duplication_options: DuplicationOptions {
            mode: combined_duplication_mode(params.dupes_mode.as_deref())?,
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
    push_opt_arg(&mut args, "--dupes-mode", params.dupes_mode.as_deref());
    push_opt_arg(
        &mut args,
        "--dupes-min-tokens",
        params
            .dupes_min_tokens
            .map(|value| value.to_string())
            .as_deref(),
    );
    push_opt_arg(
        &mut args,
        "--dupes-min-lines",
        params
            .dupes_min_lines
            .map(|value| value.to_string())
            .as_deref(),
    );
    push_opt_arg(
        &mut args,
        "--dupes-min-occurrences",
        params
            .dupes_min_occurrences
            .map(|value| value.to_string())
            .as_deref(),
    );
    push_opt_arg(
        &mut args,
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
    if params.score == Some(true) {
        args.push("--score".to_string());
    }
    args
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
    use super::*;

    #[test]
    fn api_backed_code_mode_tools_are_explicitly_registered() {
        let names = API_BACKED_CODE_MODE_TOOLS
            .iter()
            .map(|tool| tool.name())
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                "analyze",
                "combined",
                "check_changed",
                "find_dupes",
                "project_info",
                "trace_export",
                "trace_file",
                "trace_dependency",
                "trace_clone",
                "check_health",
                "audit",
                "fallow_explain",
                "list_boundaries",
                "feature_flags",
            ]
        );

        for tool in API_BACKED_CODE_MODE_TOOLS {
            assert!(
                tool.is_api_backed(),
                "{} should use fallow-api",
                tool.name()
            );
        }
    }

    #[test]
    fn heavy_code_mode_tools_keep_cancellable_cli_path() {
        for tool in [
            CodeModeTool::Analyze,
            CodeModeTool::FindDupes,
            CodeModeTool::CheckHealth,
            CodeModeTool::Audit,
        ] {
            assert!(
                tool.is_api_backed(),
                "{} should still be API-backed for standalone MCP tools",
                tool.name()
            );
            assert!(
                !tool.is_code_mode_api_backed(),
                "{} should use Code Mode's cancellable subprocess path",
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
    fn cli_only_code_mode_tools_are_not_api_backed() {
        for tool in [
            CodeModeTool::SecurityCandidates,
            CodeModeTool::Impact,
            CodeModeTool::CheckRuntimeCoverage,
            CodeModeTool::GetHotPaths,
            CodeModeTool::GetBlastRadius,
            CodeModeTool::GetImportance,
            CodeModeTool::GetCleanupCandidates,
        ] {
            assert!(
                !tool.is_api_backed(),
                "{} should use CLI fallback",
                tool.name()
            );
            assert_eq!(
                run_api_tool(tool, serde_json::json!({})).expect("fallback decision"),
                None
            );
        }
    }
}
