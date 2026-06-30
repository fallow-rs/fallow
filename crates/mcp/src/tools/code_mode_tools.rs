use crate::params::{
    AnalyzeParams, AuditParams, CheckChangedParams, CheckRuntimeCoverageParams, ExplainParams,
    FeatureFlagsParams, FindDupesParams, HealthParams, ImpactParams, ListBoundariesParams,
    ProjectInfoParams, SecurityCandidatesParams, TraceCloneParams, TraceDependencyParams,
    TraceExportParams, TraceFileParams,
};

use fallow_api::{RootEnvelopeMode, serialize_explain_programmatic_json};

use super::super::{
    analyze::run_analyze_api_value,
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
    trace::{
        run_trace_clone_api_value, run_trace_dependency_api_value, run_trace_export_api_value,
        run_trace_file_api_value,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CodeModeTool {
    Analyze,
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
}

pub(super) const CODE_MODE_ALIASES: &[(&str, &str)] = &[
    ("analyze", "analyze"),
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
    CodeModeTool::CheckChanged,
    CodeModeTool::FindDupes,
    CodeModeTool::ProjectInfo,
    CodeModeTool::TraceExport,
    CodeModeTool::TraceFile,
    CodeModeTool::TraceDependency,
    CodeModeTool::TraceClone,
    CodeModeTool::CheckHealth,
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
        | CodeModeTool::Audit
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
                "check_changed",
                "find_dupes",
                "project_info",
                "trace_export",
                "trace_file",
                "trace_dependency",
                "trace_clone",
                "check_health",
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
    fn cli_only_code_mode_tools_are_not_api_backed() {
        for tool in [
            CodeModeTool::SecurityCandidates,
            CodeModeTool::Audit,
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
