use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content, RawContent};
use serde::Serialize;
use serde_json::{Value, json};

use crate::params::{
    AnalyzeParams, FindDupesParams, HealthParams, InspectTarget, InspectTargetParams,
    SecurityCandidatesParams, TraceExportParams, TraceFileParams,
};

use super::{
    build_analyze_args, build_find_dupes_args, build_health_args, build_security_candidates_args,
    build_trace_export_args, build_trace_file_args, run_tool, validation_error_body,
};

const TOOL: &str = "inspect_target";

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum SectionStatus {
    Ok,
    Error,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum EvidenceScope {
    Symbol,
    File,
    ProjectFilteredToFile,
}

#[derive(Serialize)]
struct EvidenceSection {
    status: SectionStatus,
    scope: EvidenceScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

impl EvidenceSection {
    fn ok(scope: EvidenceScope, data: Value) -> Self {
        Self {
            status: SectionStatus::Ok,
            scope,
            message: None,
            data: Some(data),
        }
    }

    fn error(scope: EvidenceScope, message: String) -> Self {
        Self {
            status: SectionStatus::Error,
            scope,
            message: Some(message),
            data: None,
        }
    }
}

#[derive(Serialize)]
struct InspectEvidence {
    trace_file: EvidenceSection,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace_export: Option<EvidenceSection>,
    dead_code: EvidenceSection,
    duplication: EvidenceSection,
    complexity: EvidenceSection,
    security: EvidenceSection,
}

#[derive(Serialize)]
struct InspectBundle {
    kind: &'static str,
    target: Value,
    identity: Value,
    evidence: InspectEvidence,
    warnings: Vec<String>,
}

struct NormalizedTarget<'a> {
    file: &'a str,
    export_name: Option<&'a str>,
}

impl<'a> NormalizedTarget<'a> {
    fn from_params(params: &'a InspectTargetParams) -> Result<Self, String> {
        match &params.target {
            InspectTarget::File { file } => {
                require_non_empty("target.file", file)?;
                Ok(Self {
                    file,
                    export_name: None,
                })
            }
            InspectTarget::Symbol { file, export_name } => {
                require_non_empty("target.file", file)?;
                require_non_empty("target.export_name", export_name)?;
                Ok(Self {
                    file,
                    export_name: Some(export_name),
                })
            }
        }
    }

    fn target_json(&self) -> Value {
        match self.export_name {
            Some(export_name) => json!({
                "type": "symbol",
                "file": self.file,
                "export_name": export_name,
            }),
            None => json!({
                "type": "file",
                "file": self.file,
            }),
        }
    }
}

/// Run the composed `inspect_target` MCP tool.
pub async fn inspect_target(
    binary: &str,
    params: &InspectTargetParams,
) -> Result<CallToolResult, McpError> {
    let target = match NormalizedTarget::from_params(params) {
        Ok(target) => target,
        Err(message) => return Ok(error_result(message)),
    };

    let trace_file = match run_inspect_trace_file(binary, params, target.file).await? {
        RequiredJson::Value(value) => value,
        RequiredJson::ToolError(result) => return Ok(result),
    };

    let mut warnings = Vec::new();
    let trace_export = match run_inspect_trace_export(binary, params, &target).await? {
        RequiredJson::Value(value) => value,
        RequiredJson::ToolError(result) => return Ok(result),
    };

    if target.export_name.is_some() {
        warnings.push(
            "dead_code, duplication, complexity, and security evidence is file-scoped in v1; file:line symbol narrowing is a follow-up"
                .to_string(),
        );
    }

    let identity = match trace_export.as_ref() {
        Some(export) => json!({
            "file": target.file,
            "export_name": target.export_name,
            "file_reachable": export.get("file_reachable"),
            "is_entry_point": export.get("is_entry_point"),
            "is_used": export.get("is_used"),
            "reason": export.get("reason"),
        }),
        None => json!({
            "file": target.file,
            "is_reachable": trace_file.get("is_reachable"),
            "is_entry_point": trace_file.get("is_entry_point"),
            "export_count": trace_file.get("exports").and_then(Value::as_array).map(Vec::len),
            "import_count": trace_file.get("imports_from").and_then(Value::as_array).map(Vec::len),
            "imported_by_count": trace_file.get("imported_by").and_then(Value::as_array).map(Vec::len),
        }),
    };

    let evidence =
        collect_inspect_evidence(binary, params, &target, trace_file, trace_export).await;
    push_inspect_warnings(&mut warnings, &evidence);

    let bundle = InspectBundle {
        kind: TOOL,
        target: target.target_json(),
        identity,
        evidence,
        warnings,
    };

    let text = serde_json::to_string(&bundle).map_err(|err| {
        McpError::internal_error(
            format!("failed to serialize inspect_target output: {err}"),
            None,
        )
    })?;
    Ok(CallToolResult::success(vec![Content::text(text)]))
}

async fn run_inspect_trace_file(
    binary: &str,
    params: &InspectTargetParams,
    file: &str,
) -> Result<RequiredJson, McpError> {
    let args = match build_trace_file_args(&TraceFileParams {
        file: file.to_string(),
        root: params.root.clone(),
        config: params.config.clone(),
        production: params.production,
        workspace: params.workspace.clone(),
        no_cache: params.no_cache,
        threads: params.threads,
    }) {
        Ok(args) => args,
        Err(message) => return Ok(RequiredJson::ToolError(error_result(message))),
    };
    run_required_json(binary, args).await
}

async fn run_inspect_trace_export(
    binary: &str,
    params: &InspectTargetParams,
    target: &NormalizedTarget<'_>,
) -> Result<RequiredJson<Option<Value>>, McpError> {
    let Some(export_name) = target.export_name else {
        return Ok(RequiredJson::Value(None));
    };
    let args = match build_trace_export_args(&TraceExportParams {
        file: target.file.to_string(),
        export_name: export_name.to_string(),
        root: params.root.clone(),
        config: params.config.clone(),
        production: params.production,
        workspace: params.workspace.clone(),
        no_cache: params.no_cache,
        threads: params.threads,
    }) {
        Ok(args) => args,
        Err(message) => return Ok(RequiredJson::ToolError(error_result(message))),
    };
    match run_required_json(binary, args).await? {
        RequiredJson::Value(value) => Ok(RequiredJson::Value(Some(value))),
        RequiredJson::ToolError(result) => Ok(RequiredJson::ToolError(result)),
    }
}

async fn collect_inspect_evidence(
    binary: &str,
    params: &InspectTargetParams,
    target: &NormalizedTarget<'_>,
    trace_file: Value,
    trace_export: Option<Value>,
) -> InspectEvidence {
    let dead_code = inspect_dead_code_section(binary, params, target.file).await;
    let duplication = inspect_duplication_section(binary, params, target.file).await;
    let complexity = inspect_complexity_section(binary, params, target.file).await;
    let security = inspect_security_section(binary, params, target.file).await;

    InspectEvidence {
        trace_file: EvidenceSection::ok(EvidenceScope::File, trace_file),
        trace_export: trace_export.map(|value| EvidenceSection::ok(EvidenceScope::Symbol, value)),
        dead_code,
        duplication,
        complexity,
        security,
    }
}

fn push_inspect_warnings(warnings: &mut Vec<String>, evidence: &InspectEvidence) {
    push_warning(warnings, "dead_code", &evidence.dead_code);
    push_warning(warnings, "duplication", &evidence.duplication);
    push_warning(warnings, "complexity", &evidence.complexity);
    push_warning(warnings, "security", &evidence.security);
}

enum RequiredJson<T = Value> {
    Value(T),
    ToolError(CallToolResult),
}

async fn inspect_dead_code_section(
    binary: &str,
    params: &InspectTargetParams,
    file: &str,
) -> EvidenceSection {
    match build_dead_code_args(params, file) {
        Ok(args) => optional_section(binary, args, EvidenceScope::File, |value| value).await,
        Err(message) => EvidenceSection::error(EvidenceScope::File, message),
    }
}

async fn inspect_duplication_section(
    binary: &str,
    params: &InspectTargetParams,
    file: &str,
) -> EvidenceSection {
    match build_dupes_args(params) {
        Ok(args) => {
            optional_section(
                binary,
                args,
                EvidenceScope::ProjectFilteredToFile,
                |value| filter_path_array(&value, file, "clone_groups"),
            )
            .await
        }
        Err(message) => EvidenceSection::error(EvidenceScope::ProjectFilteredToFile, message),
    }
}

async fn inspect_complexity_section(
    binary: &str,
    params: &InspectTargetParams,
    file: &str,
) -> EvidenceSection {
    optional_section(
        binary,
        build_health_args(&HealthParams {
            root: params.root.clone(),
            config: params.config.clone(),
            production: params.production,
            workspace: params.workspace.clone(),
            complexity: Some(true),
            no_cache: params.no_cache,
            threads: params.threads,
            ..Default::default()
        }),
        EvidenceScope::ProjectFilteredToFile,
        |value| filter_path_array(&value, file, "findings"),
    )
    .await
}

async fn inspect_security_section(
    binary: &str,
    params: &InspectTargetParams,
    file: &str,
) -> EvidenceSection {
    match build_security_candidates_args(&SecurityCandidatesParams {
        root: params.root.clone(),
        config: params.config.clone(),
        workspace: params.workspace.clone(),
        paths: Some(vec![file.to_string()]),
        no_cache: params.no_cache,
        threads: params.threads,
        ..Default::default()
    }) {
        Ok(args) => optional_section(binary, args, EvidenceScope::File, |value| value).await,
        Err(message) => EvidenceSection::error(EvidenceScope::File, message),
    }
}

fn build_dead_code_args(params: &InspectTargetParams, file: &str) -> Result<Vec<String>, String> {
    build_analyze_args(&AnalyzeParams {
        root: params.root.clone(),
        config: params.config.clone(),
        production: params.production,
        workspace: params.workspace.clone(),
        file: Some(vec![file.to_string()]),
        no_cache: params.no_cache,
        threads: params.threads,
        ..Default::default()
    })
}

fn build_dupes_args(params: &InspectTargetParams) -> Result<Vec<String>, String> {
    build_find_dupes_args(&FindDupesParams {
        root: params.root.clone(),
        config: params.config.clone(),
        workspace: params.workspace.clone(),
        no_cache: params.no_cache,
        threads: params.threads,
        ..Default::default()
    })
}

async fn run_required_json(binary: &str, args: Vec<String>) -> Result<RequiredJson, McpError> {
    let result = run_tool(binary, TOOL, &args).await?;
    if result.is_error == Some(true) {
        return Ok(RequiredJson::ToolError(result));
    }
    parse_result_json(&result)
        .map(RequiredJson::Value)
        .map_err(|message| McpError::internal_error(message, None))
}

async fn optional_section<F>(
    binary: &str,
    args: Vec<String>,
    scope: EvidenceScope,
    filter: F,
) -> EvidenceSection
where
    F: FnOnce(Value) -> Value,
{
    match run_tool(binary, TOOL, &args).await {
        Ok(result) if result.is_error == Some(true) => EvidenceSection::error(
            scope,
            result_text(&result).unwrap_or("command failed").to_string(),
        ),
        Ok(result) => match parse_result_json(&result) {
            Ok(value) => EvidenceSection::ok(scope, filter(value)),
            Err(message) => EvidenceSection::error(scope, message),
        },
        Err(err) => EvidenceSection::error(scope, err.to_string()),
    }
}

fn parse_result_json(result: &CallToolResult) -> Result<Value, String> {
    let text = result_text(result).ok_or_else(|| "tool returned no text content".to_string())?;
    serde_json::from_str(text).map_err(|err| format!("tool returned invalid JSON: {err}"))
}

fn result_text(result: &CallToolResult) -> Option<&str> {
    let content = result.content.first()?;
    let RawContent::Text(text) = &content.raw else {
        return None;
    };
    Some(&text.text)
}

fn filter_path_array(value: &Value, file: &str, key: &str) -> Value {
    let matched = value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| value_mentions_file(item, file))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let matched_count = matched.len();

    json!({
        key: matched,
        "matched_count": matched_count,
        "summary": value.get("summary").cloned(),
        "stats": value.get("stats").cloned(),
    })
}

fn value_mentions_file(value: &Value, file: &str) -> bool {
    match value {
        Value::String(s) => path_eq(s, file),
        Value::Array(items) => items.iter().any(|item| value_mentions_file(item, file)),
        Value::Object(map) => map.values().any(|item| value_mentions_file(item, file)),
        _ => false,
    }
}

fn path_eq(left: &str, right: &str) -> bool {
    left.replace('\\', "/") == right.replace('\\', "/")
}

fn push_warning(warnings: &mut Vec<String>, section: &str, evidence: &EvidenceSection) {
    if matches!(evidence.status, SectionStatus::Error)
        && let Some(message) = evidence.message.as_ref()
    {
        warnings.push(format!("{section} evidence unavailable: {message}"));
    }
}

fn require_non_empty(field: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    Ok(())
}

fn error_result(message: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![Content::text(validation_error_body(message))])
}
