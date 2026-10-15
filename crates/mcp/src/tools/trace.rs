use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::params::{
    TraceCloneParams, TraceDependencyParams, TraceErrorParams, TraceExportParams, TraceFileParams,
    TraceImportPathParams,
};

use fallow_api::{
    AnalysisOptions, DependencyUsageQuery, DuplicationOptions, SitePageRequest, TraceCloneOptions,
    TraceCloneTarget, TraceDependencyOptions, TraceErrorOptions, TraceExportOptions,
    TraceFileOptions, TraceImportPathOptions, run_trace_clone, run_trace_dependency,
    run_trace_error, run_trace_export, run_trace_file, run_trace_import_path,
    serialize_trace_clone_programmatic_json, serialize_trace_dependency_programmatic_json,
    serialize_trace_error_programmatic_json, serialize_trace_export_programmatic_json,
    serialize_trace_file_programmatic_json, serialize_trace_import_path_programmatic_json,
};
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};

use super::{
    VALID_DUPES_MODES,
    api_runtime::{
        EnvScope, SharedParams, call_cancellation, json_success, programmatic_error_body,
        run_api_blocking, shared_analysis_options, shared_params, typed_api_value,
        typed_tool_result, workspace_patterns_from_param,
    },
    duplication_mode_from_param, min_occurrences_from_param, push_global, push_remote_extends,
    push_scope, require_non_empty, run_tool, validation_error_body,
};

/// Run `trace_export` through the typed API. When type-aware analysis is on,
/// `fallow dead-code --trace` adds semantic evidence that the typed trace
/// cannot, so the tool runs the CLI.
pub async fn run_trace_export_tool(
    binary: &str,
    params: TraceExportParams,
) -> Result<CallToolResult, McpError> {
    let cancellation = call_cancellation();
    let options = match trace_export_options_from_params(&params, Some(Arc::clone(&cancellation))) {
        Ok(options) => options,
        Err(msg) => return Ok(CallToolResult::error(vec![ContentBlock::text(msg)])),
    };
    let result = run_api_blocking("trace_export", cancellation, move || {
        run_trace_export(&options).and_then(serialize_trace_export_programmatic_json)
    })
    .await?;
    if let Some(result) = typed_tool_result(result) {
        return Ok(result);
    }
    match build_trace_export_args(&params) {
        Ok(args) => run_tool(binary, "trace_export", &args).await,
        Err(msg) => Ok(CallToolResult::error(vec![ContentBlock::text(msg)])),
    }
}

/// Run `trace_file` through the typed API.
pub async fn run_trace_file_tool(params: TraceFileParams) -> Result<CallToolResult, McpError> {
    let cancellation = call_cancellation();
    let options = match trace_file_options_from_params(&params, Some(Arc::clone(&cancellation))) {
        Ok(options) => options,
        Err(msg) => return Ok(CallToolResult::error(vec![ContentBlock::text(msg)])),
    };
    let result = run_api_blocking("trace_file", cancellation, move || {
        run_trace_file(&options).and_then(serialize_trace_file_programmatic_json)
    })
    .await?
    .map_or_else(
        |err| CallToolResult::error(vec![ContentBlock::text(programmatic_error_body(&err))]),
        |value| json_success(&value),
    );
    Ok(result)
}

/// Run `trace_import_path` through the typed API.
pub async fn run_trace_import_path_tool(
    params: TraceImportPathParams,
) -> Result<CallToolResult, McpError> {
    let cancellation = call_cancellation();
    let options =
        match trace_import_path_options_from_params(&params, Some(Arc::clone(&cancellation))) {
            Ok(options) => options,
            Err(msg) => return Ok(CallToolResult::error(vec![ContentBlock::text(msg)])),
        };
    let result = run_api_blocking("trace_import_path", cancellation, move || {
        run_trace_import_path(&options).and_then(serialize_trace_import_path_programmatic_json)
    })
    .await?
    .map_or_else(
        |err| CallToolResult::error(vec![ContentBlock::text(programmatic_error_body(&err))]),
        |value| json_success(&value),
    );
    Ok(result)
}

/// Run `trace_error` through the typed API.
pub async fn run_trace_error_tool(params: TraceErrorParams) -> Result<CallToolResult, McpError> {
    let cancellation = call_cancellation();
    let options = trace_error_options_from_params(&params, Some(Arc::clone(&cancellation)));
    let result = run_api_blocking("trace_error", cancellation, move || {
        run_trace_error(&options).and_then(serialize_trace_error_programmatic_json)
    })
    .await?
    .map_or_else(
        |err| CallToolResult::error(vec![ContentBlock::text(programmatic_error_body(&err))]),
        |value| json_success(&value),
    );
    Ok(result)
}

/// Run `trace_dependency` through the typed API.
pub async fn run_trace_dependency_tool(
    params: TraceDependencyParams,
) -> Result<CallToolResult, McpError> {
    let cancellation = call_cancellation();
    let options =
        match trace_dependency_options_from_params(&params, Some(Arc::clone(&cancellation))) {
            Ok(options) => options,
            Err(msg) => return Ok(CallToolResult::error(vec![ContentBlock::text(msg)])),
        };
    let result = run_api_blocking("trace_dependency", cancellation, move || {
        run_trace_dependency(&options).and_then(serialize_trace_dependency_programmatic_json)
    })
    .await?
    .map_or_else(
        |err| CallToolResult::error(vec![ContentBlock::text(programmatic_error_body(&err))]),
        |value| json_success(&value),
    );
    Ok(result)
}

/// Run `trace_clone` through the typed API.
pub async fn run_trace_clone_tool(params: TraceCloneParams) -> Result<CallToolResult, McpError> {
    let cancellation = call_cancellation();
    let options = match trace_clone_options_from_params(&params, Some(Arc::clone(&cancellation))) {
        Ok(options) => options,
        Err(msg) => return Ok(CallToolResult::error(vec![ContentBlock::text(msg)])),
    };
    let result = run_api_blocking("trace_clone", cancellation, move || {
        run_trace_clone(&options).and_then(serialize_trace_clone_programmatic_json)
    })
    .await?
    .map_or_else(
        |err| CallToolResult::error(vec![ContentBlock::text(programmatic_error_body(&err))]),
        |value| json_success(&value),
    );
    Ok(result)
}

/// The Code Mode form of [`run_trace_export_tool`]: `Ok(None)` asks the
/// caller to run the CLI.
pub fn run_trace_export_api_value(
    params: &TraceExportParams,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<Option<serde_json::Value>, String> {
    let options = trace_export_options_from_params(params, cancellation)?;
    typed_api_value(run_trace_export(&options).and_then(serialize_trace_export_programmatic_json))
}

pub fn run_trace_file_api_value(
    params: &TraceFileParams,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<serde_json::Value, String> {
    let options = trace_file_options_from_params(params, cancellation)?;
    run_trace_file(&options)
        .and_then(serialize_trace_file_programmatic_json)
        .map_err(|err| programmatic_error_body(&err))
}

pub fn run_trace_dependency_api_value(
    params: &TraceDependencyParams,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<serde_json::Value, String> {
    let options = trace_dependency_options_from_params(params, cancellation)?;
    run_trace_dependency(&options)
        .and_then(serialize_trace_dependency_programmatic_json)
        .map_err(|err| programmatic_error_body(&err))
}

pub fn run_trace_clone_api_value(
    params: &TraceCloneParams,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<serde_json::Value, String> {
    let options = trace_clone_options_from_params(params, cancellation)?;
    run_trace_clone(&options)
        .and_then(serialize_trace_clone_programmatic_json)
        .map_err(|err| programmatic_error_body(&err))
}

/// Build CLI arguments for the `trace_export` tool.
pub fn build_trace_export_args(params: &TraceExportParams) -> Result<Vec<String>, String> {
    require_non_empty("file", &params.file).map_err(validation_error_body)?;
    require_non_empty("export_name", &params.export_name).map_err(validation_error_body)?;

    let mut args = vec![
        "dead-code".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ];

    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        params.no_cache,
        params.threads,
    );
    push_remote_extends(&mut args, params.allow_remote_extends);
    push_scope(&mut args, params.production, params.workspace.as_deref());
    args.extend([
        "--trace".to_string(),
        format!("{}:{}", params.file, params.export_name),
    ]);
    Ok(args)
}

/// Build CLI arguments for the `trace_file` tool.
pub fn build_trace_file_args(params: &TraceFileParams) -> Result<Vec<String>, String> {
    require_non_empty("file", &params.file).map_err(validation_error_body)?;

    let mut args = vec![
        "dead-code".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ];

    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        params.no_cache,
        params.threads,
    );
    push_remote_extends(&mut args, params.allow_remote_extends);
    push_scope(&mut args, params.production, params.workspace.as_deref());
    args.extend(["--trace-file".to_string(), params.file.clone()]);
    Ok(args)
}

/// Build CLI arguments for the `trace_dependency` tool.
pub fn build_trace_dependency_args(params: &TraceDependencyParams) -> Result<Vec<String>, String> {
    require_non_empty("package_name", &params.package_name).map_err(validation_error_body)?;
    let usage = dependency_usage_query(params)?;
    let command = if usage.is_some() {
        "trace"
    } else {
        "dead-code"
    };

    let mut args = vec![
        command.to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ];

    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        params.no_cache,
        params.threads,
    );
    push_remote_extends(&mut args, params.allow_remote_extends);
    push_scope(&mut args, params.production, params.workspace.as_deref());
    match usage {
        Some(query) => push_dependency_usage_args(&mut args, &params.package_name, &query),
        None => args.extend([
            "--trace-dependency".to_string(),
            params.package_name.clone(),
        ]),
    }
    Ok(args)
}

/// Push the `fallow trace --dependency` target and its usage flags.
fn push_dependency_usage_args(
    args: &mut Vec<String>,
    package_name: &str,
    query: &DependencyUsageQuery,
) {
    args.extend(["--dependency".to_string(), package_name.to_string()]);
    for name in &query.specifiers {
        args.extend(["--specifier".to_string(), name.clone()]);
    }
    if let Some(page) = &query.sites {
        args.extend([
            "--sites".to_string(),
            "--limit".to_string(),
            page.limit.to_string(),
        ]);
        if let Some(cursor) = &page.cursor {
            args.extend(["--cursor".to_string(), cursor.clone()]);
        }
    }
    if let Some(depth) = query.closure_depth {
        args.extend([
            "--callers".to_string(),
            "--depth".to_string(),
            depth.to_string(),
        ]);
    }
}

/// Build CLI arguments for the `trace_clone` tool.
pub fn build_trace_clone_args(params: &TraceCloneParams) -> Result<Vec<String>, String> {
    let trace_spec = trace_clone_spec(params)?;

    let mut args = vec![
        "dupes".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ];

    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        params.no_cache,
        params.threads,
    );
    push_remote_extends(&mut args, params.allow_remote_extends);
    if let Some(ref workspace) = params.workspace {
        args.extend(["--workspace".to_string(), workspace.clone()]);
    }
    push_trace_clone_options(&mut args, params)?;
    args.extend(["--trace".to_string(), trace_spec]);

    Ok(args)
}

fn trace_clone_spec(params: &TraceCloneParams) -> Result<String, String> {
    let has_location = params.file.is_some() || params.line.is_some();
    let has_fingerprint = params
        .fingerprint
        .as_deref()
        .is_some_and(|fp| !fp.trim().is_empty());

    match (has_location, has_fingerprint) {
        (true, true) => Err(validation_error_body(
            "provide either file + line OR fingerprint, not both",
        )),
        (false, false) => Err(validation_error_body(
            "provide file + line (a clone location) or fingerprint (a dup:<id> from find_dupes)",
        )),
        (true, false) => trace_clone_location(params),
        (false, true) => Ok(params
            .fingerprint
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_string()),
    }
}

fn trace_clone_location(params: &TraceCloneParams) -> Result<String, String> {
    let file = params.file.as_deref().unwrap_or_default();
    if file.trim().is_empty() {
        return Err(validation_error_body("file must not be empty"));
    }

    match params.line {
        None => Err(validation_error_body("line is required with file")),
        Some(0) => Err(validation_error_body("line must be greater than 0")),
        Some(line) => Ok(format!("{file}:{line}")),
    }
}

fn push_trace_clone_options(
    args: &mut Vec<String>,
    params: &TraceCloneParams,
) -> Result<(), String> {
    push_trace_clone_mode(args, params)?;
    push_trace_clone_numeric_options(args, params)?;
    if params.near == Some(true) {
        args.push("--near".to_string());
    }
    if params.skip_local == Some(true) {
        args.push("--skip-local".to_string());
    }
    if params.cross_language == Some(true) {
        args.push("--cross-language".to_string());
    }
    match params.ignore_symlinks {
        Some(true) => args.push("--ignore-symlinks".to_string()),
        Some(false) => args.push("--no-ignore-symlinks".to_string()),
        None => {}
    }
    match params.ignore_imports {
        Some(true) => args.push("--ignore-imports".to_string()),
        Some(false) => args.push("--no-ignore-imports".to_string()),
        None => {}
    }
    Ok(())
}

fn push_trace_clone_mode(args: &mut Vec<String>, params: &TraceCloneParams) -> Result<(), String> {
    let Some(ref mode) = params.mode else {
        return Ok(());
    };
    if !VALID_DUPES_MODES.contains(&mode.as_str()) {
        return Err(validation_error_body(format!(
            "Invalid mode '{mode}'. Valid values: {}",
            VALID_DUPES_MODES.join(", ")
        )));
    }
    args.extend(["--mode".to_string(), mode.clone()]);
    Ok(())
}

fn push_trace_clone_numeric_options(
    args: &mut Vec<String>,
    params: &TraceCloneParams,
) -> Result<(), String> {
    if let Some(min_tokens) = params.min_tokens {
        args.extend(["--min-tokens".to_string(), min_tokens.to_string()]);
    }
    if let Some(min_lines) = params.min_lines {
        args.extend(["--min-lines".to_string(), min_lines.to_string()]);
    }
    if let Some(min_occurrences) = params.min_occurrences {
        if min_occurrences < 2 {
            return Err(validation_error_body(format!(
                "min_occurrences must be at least 2 (got {min_occurrences})"
            )));
        }
        args.extend(["--min-occurrences".to_string(), min_occurrences.to_string()]);
    }
    if let Some(threshold) = params.threshold {
        args.extend(["--threshold".to_string(), threshold.to_string()]);
    }
    Ok(())
}

fn trace_export_options_from_params(
    params: &TraceExportParams,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<TraceExportOptions, String> {
    require_non_empty("file", &params.file).map_err(validation_error_body)?;
    require_non_empty("export_name", &params.export_name).map_err(validation_error_body)?;
    Ok(TraceExportOptions {
        analysis: dead_code_analysis_options(
            shared_params!(params),
            params.production,
            params.workspace.as_deref(),
            cancellation,
        ),
        file: params.file.clone(),
        export_name: params.export_name.clone(),
    })
}

fn trace_file_options_from_params(
    params: &TraceFileParams,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<TraceFileOptions, String> {
    require_non_empty("file", &params.file).map_err(validation_error_body)?;
    Ok(TraceFileOptions {
        analysis: dead_code_analysis_options(
            shared_params!(params),
            params.production,
            params.workspace.as_deref(),
            cancellation,
        ),
        file: params.file.clone(),
    })
}

fn trace_import_path_options_from_params(
    params: &TraceImportPathParams,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<TraceImportPathOptions, String> {
    require_non_empty("from", &params.from).map_err(validation_error_body)?;
    require_non_empty("to", &params.to).map_err(validation_error_body)?;
    Ok(TraceImportPathOptions {
        analysis: dead_code_analysis_options(
            shared_params!(params),
            params.production,
            params.workspace.as_deref(),
            cancellation,
        ),
        from: params.from.clone(),
        to: params.to.clone(),
        eager_only: false,
    })
}

/// The `source` label reported back when the caller supplied none. An MCP
/// caller pastes the trace itself rather than naming a file, so the honest
/// label is where it arrived from, not a path that does not exist.
const MCP_TRACE_SOURCE: &str = "mcp";

/// Build the typed options without pre-validating `trace`.
///
/// The API layer refuses an empty or oversized trace with a `code`, a `help`
/// and a `context`; short-circuiting the empty case here replaced that with a
/// bare `{error, message, exit_code}` body, so the weakest refusal on the tool
/// was the one an agent hits first.
fn trace_error_options_from_params(
    params: &TraceErrorParams,
    cancellation: Option<Arc<AtomicBool>>,
) -> TraceErrorOptions {
    let source = params
        .source
        .as_deref()
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .unwrap_or(MCP_TRACE_SOURCE);
    TraceErrorOptions {
        analysis: dead_code_analysis_options(
            shared_params!(params),
            params.production,
            params.workspace.as_deref(),
            cancellation,
        ),
        trace: params.trace.clone(),
        source: source.to_string(),
    }
}

fn trace_dependency_options_from_params(
    params: &TraceDependencyParams,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<TraceDependencyOptions, String> {
    require_non_empty("package_name", &params.package_name).map_err(validation_error_body)?;
    Ok(TraceDependencyOptions {
        analysis: dead_code_analysis_options(
            shared_params!(params),
            params.production,
            params.workspace.as_deref(),
            cancellation,
        ),
        package_name: params.package_name.clone(),
        usage: dependency_usage_query(params)?,
    })
}

/// Whether the call asks for a page of usage sites.
fn sites_requested(params: &TraceDependencyParams) -> bool {
    params.specifiers.is_some()
        || params.sites == Some(true)
        || params.limit.is_some()
        || params.cursor.is_some()
}

/// Whether the call asks for the `usage` object.
fn usage_requested(params: &TraceDependencyParams) -> bool {
    params.usage == Some(true) || params.closure_depth.is_some() || sites_requested(params)
}

/// The validated usage query, or `None` when no usage parameter is set. The
/// API constructor checks the ranges that the JSON schema declares.
fn dependency_usage_query(
    params: &TraceDependencyParams,
) -> Result<Option<DependencyUsageQuery>, String> {
    if !usage_requested(params) {
        return Ok(None);
    }
    let sites = sites_requested(params).then(|| SitePageRequest {
        limit: params
            .limit
            .unwrap_or(fallow_types::trace_usage::DEFAULT_USAGE_SITE_LIMIT),
        cursor: params.cursor.clone(),
    });
    DependencyUsageQuery::new(
        params.specifiers.clone().unwrap_or_default(),
        sites,
        params.closure_depth,
    )
    .map(Some)
    .map_err(|err| validation_error_body(err.to_string()))
}

fn trace_clone_options_from_params(
    params: &TraceCloneParams,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<TraceCloneOptions, String> {
    Ok(TraceCloneOptions {
        duplication: DuplicationOptions {
            analysis: AnalysisOptions {
                workspace: workspace_patterns_from_param(params.workspace.as_deref()),
                // This envelope carries no `request_outcomes`, so a ref that
                // stood down would widen the result with nothing to say so:
                // keep the hard error of an explicit ref.
                ..shared_analysis_options(shared_params!(params), EnvScope::HardRef, cancellation)
            },
            mode: duplication_mode_from_param(params.mode.as_deref())?,
            near: params.near,
            min_tokens: params.min_tokens.map(|value| value as usize),
            min_lines: params.min_lines.map(|value| value as usize),
            min_occurrences: min_occurrences_from_param(params.min_occurrences)?,
            threshold: params.threshold,
            skip_local: params.skip_local,
            ignore_symlinks: params.ignore_symlinks,
            cross_language: params.cross_language,
            ignore_imports: params.ignore_imports,
            top: None,
            include_fragments: None,
        },
        target: trace_clone_target(params)?,
    })
}

/// The analysis options of the dead-code trace routes. This envelope carries
/// no `request_outcomes`, so a ref that stood down would widen the result with
/// nothing to say so: keep the hard error of an explicit ref.
fn dead_code_analysis_options(
    shared: SharedParams<'_>,
    production: Option<bool>,
    workspace: Option<&str>,
    cancellation: Option<Arc<AtomicBool>>,
) -> AnalysisOptions {
    AnalysisOptions {
        production: production.unwrap_or(false),
        production_override: production,
        workspace: workspace_patterns_from_param(workspace),
        ..shared_analysis_options(shared, EnvScope::HardRef, cancellation)
    }
}

fn trace_clone_target(params: &TraceCloneParams) -> Result<TraceCloneTarget, String> {
    let has_location = params.file.is_some() || params.line.is_some();
    let has_fingerprint = params
        .fingerprint
        .as_deref()
        .is_some_and(|fp| !fp.trim().is_empty());

    match (has_location, has_fingerprint) {
        (true, true) => Err(validation_error_body(
            "provide either file + line OR fingerprint, not both",
        )),
        (false, false) => Err(validation_error_body(
            "provide file + line (a clone location) or fingerprint (a dup:<id> from find_dupes)",
        )),
        (true, false) => trace_clone_location_target(params),
        (false, true) => Ok(TraceCloneTarget::Fingerprint(
            params
                .fingerprint
                .as_deref()
                .unwrap_or_default()
                .trim()
                .to_string(),
        )),
    }
}

fn trace_clone_location_target(params: &TraceCloneParams) -> Result<TraceCloneTarget, String> {
    let file = params.file.as_deref().unwrap_or_default();
    if file.trim().is_empty() {
        return Err(validation_error_body("file must not be empty"));
    }

    match params.line {
        None => Err(validation_error_body("line is required with file")),
        Some(0) => Err(validation_error_body("line must be greater than 0")),
        Some(line) => Ok(TraceCloneTarget::Location {
            file: file.to_string(),
            line,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty `trace` is a refused input, and every other refusal on this
    /// tool (the oversized trace beside it, the sibling trace tools) carries a
    /// `code`, a `help` and a `context`. It used to be the one that did not,
    /// because MCP validated the field itself and never reached the API's
    /// typed error.
    #[tokio::test]
    async fn empty_trace_is_refused_with_the_typed_error_shape() {
        let params: TraceErrorParams = serde_json::from_value(serde_json::json!({
            "trace": "   ",
        }))
        .expect("trace error params");

        let result = run_trace_error_tool(params)
            .await
            .expect("empty trace stays a tool result");

        assert_eq!(result.is_error, Some(true));
        let ContentBlock::Text(text) = result.content.first().expect("refusal body") else {
            panic!("refusal body must be text");
        };
        let body: serde_json::Value =
            serde_json::from_str(&text.text).expect("refusal body is JSON");
        assert_eq!(body["error"], true);
        assert_eq!(body["exit_code"], 2);
        assert_eq!(body["code"], "FALLOW_INVALID_TRACE_OPTIONS");
        assert_eq!(body["context"], "trace_error");
        assert!(
            body["help"].as_str().is_some_and(|help| !help.is_empty()),
            "a refusal without a help leaves the caller guessing: {body}"
        );
    }

    #[tokio::test]
    async fn oversized_trace_is_refused_with_the_documented_error_code() {
        const OVERSIZED_TRACE_BYTES: usize = 1024 * 1024 + 1;
        let params: TraceErrorParams = serde_json::from_value(serde_json::json!({
            "trace": "x".repeat(OVERSIZED_TRACE_BYTES),
        }))
        .expect("trace error params");

        let result = run_trace_error_tool(params)
            .await
            .expect("oversized trace stays a tool result");

        assert_eq!(result.is_error, Some(true));
        let ContentBlock::Text(text) = result.content.first().expect("refusal body") else {
            panic!("refusal body must be text");
        };
        let body: serde_json::Value =
            serde_json::from_str(&text.text).expect("refusal body is JSON");
        assert_eq!(body["error"], true);
        assert_eq!(body["exit_code"], 2);
        assert_eq!(body["code"], "FALLOW_INVALID_TRACE_OPTIONS");
        assert_eq!(body["context"], "trace_error");
        assert!(
            body["help"].as_str().is_some_and(|help| !help.is_empty()),
            "a refusal without a help leaves the caller guessing: {body}"
        );
    }

    #[test]
    fn trace_clone_forwards_near_detection() {
        let params = TraceCloneParams {
            fingerprint: Some("dup:12345678".to_string()),
            near: Some(true),
            ..TraceCloneParams::default()
        };
        let args = build_trace_clone_args(&params).expect("trace clone args");

        assert!(args.contains(&"--near".to_string()));
        assert_eq!(
            trace_clone_options_from_params(&params, None)
                .expect("trace clone options")
                .duplication
                .near,
            Some(true)
        );
    }

    #[test]
    fn typed_dead_code_traces_forward_remote_extends_opt_in() {
        for (value, expected) in [(Some(true), true), (Some(false), false), (None, false)] {
            let export: TraceExportParams = serde_json::from_value(serde_json::json!({
                "file": "src/main.ts",
                "export_name": "main",
                "allow_remote_extends": value,
            }))
            .expect("trace export params");
            let file: TraceFileParams = serde_json::from_value(serde_json::json!({
                "file": "src/main.ts",
                "allow_remote_extends": value,
            }))
            .expect("trace file params");
            let dependency: TraceDependencyParams = serde_json::from_value(serde_json::json!({
                "package_name": "react",
                "allow_remote_extends": value,
            }))
            .expect("trace dependency params");

            assert_eq!(
                trace_export_options_from_params(&export, None)
                    .expect("trace export options")
                    .analysis
                    .allow_remote_extends,
                expected
            );
            assert_eq!(
                trace_file_options_from_params(&file, None)
                    .expect("trace file options")
                    .analysis
                    .allow_remote_extends,
                expected
            );
            assert_eq!(
                trace_dependency_options_from_params(&dependency, None)
                    .expect("trace dependency options")
                    .analysis
                    .allow_remote_extends,
                expected
            );
        }
    }

    /// `typeAware.enabled` in config makes `fallow dead-code --trace` add
    /// semantic evidence. The typed trace has none, so the tool and the Code
    /// Mode route run the CLI.
    #[tokio::test]
    async fn trace_export_runs_the_cli_when_the_config_turns_type_aware_on() {
        use super::super::api_runtime::{
            MISSING_FALLBACK_BINARY, assert_cli_fallback_ran, write_type_aware_config,
        };
        let project = tempfile::tempdir().expect("project");
        std::fs::write(
            project.path().join("package.json"),
            r#"{"name":"fixture","main":"index.ts"}"#,
        )
        .expect("write package");
        std::fs::write(project.path().join("index.ts"), "export const a = 1;\n")
            .expect("write source");
        let params = || TraceExportParams {
            file: "index.ts".to_string(),
            export_name: "a".to_string(),
            root: Some(project.path().display().to_string()),
            config: None,
            allow_remote_extends: None,
            production: None,
            workspace: None,
            no_cache: Some(true),
            threads: None,
        };
        assert!(
            run_trace_export_api_value(&params(), None)
                .expect("typed trace")
                .is_some(),
            "a syntactic config stays on the typed route"
        );

        write_type_aware_config(project.path(), r#"{"typeAware":{"enabled":true}}"#);
        assert_eq!(run_trace_export_api_value(&params(), None), Ok(None));
        assert_cli_fallback_ran(run_trace_export_tool(MISSING_FALLBACK_BINARY, params()).await);
    }
}

#[cfg(test)]
mod route_parity_tests {
    use super::*;

    /// Every trace tool with both routes expresses the same shared settings on each.
    #[test]
    fn typed_and_cli_routes_agree_on_shared_settings() {
        super::super::route_parity::assert_routes_agree(
            "trace_export",
            &serde_json::json!({ "file": "src/a.ts", "export_name": "used" }),
            |params: TraceExportParams| build_trace_export_args(&params).expect("argv"),
            |params: TraceExportParams| {
                trace_export_options_from_params(&params, None)
                    .expect("options")
                    .analysis
            },
        );
        super::super::route_parity::assert_routes_agree(
            "trace_file",
            &serde_json::json!({ "file": "src/a.ts" }),
            |params: TraceFileParams| build_trace_file_args(&params).expect("argv"),
            |params: TraceFileParams| {
                trace_file_options_from_params(&params, None)
                    .expect("options")
                    .analysis
            },
        );
        super::super::route_parity::assert_routes_agree(
            "trace_dependency",
            &serde_json::json!({ "package_name": "react" }),
            |params: TraceDependencyParams| build_trace_dependency_args(&params).expect("argv"),
            |params: TraceDependencyParams| {
                trace_dependency_options_from_params(&params, None)
                    .expect("options")
                    .analysis
            },
        );
        super::super::route_parity::assert_routes_agree(
            "trace_clone",
            &serde_json::json!({ "fingerprint": "dup:0123abcd" }),
            |params: TraceCloneParams| build_trace_clone_args(&params).expect("argv"),
            |params: TraceCloneParams| {
                trace_clone_options_from_params(&params, None)
                    .expect("options")
                    .duplication
                    .analysis
            },
        );
    }
}
