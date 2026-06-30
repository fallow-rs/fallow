use crate::params::{TraceCloneParams, TraceDependencyParams, TraceExportParams, TraceFileParams};

use fallow_api::{
    AnalysisOptions, DuplicationMode, DuplicationOptions, TraceCloneOptions, TraceCloneTarget,
    TraceDependencyOptions, TraceExportOptions, TraceFileOptions, run_trace_clone,
    run_trace_dependency, run_trace_export, run_trace_file,
    serialize_trace_clone_programmatic_json, serialize_trace_dependency_programmatic_json,
    serialize_trace_export_programmatic_json, serialize_trace_file_programmatic_json,
};
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};

use super::{
    VALID_DUPES_MODES,
    api_runtime::{
        env_diff_file, json_success, non_empty_path, non_empty_string, programmatic_error_body,
        run_api_blocking,
    },
    push_global, push_scope, validation_error_body,
};

/// Run `trace_export` through the typed API.
pub async fn run_trace_export_tool(params: TraceExportParams) -> Result<CallToolResult, McpError> {
    let options = match trace_export_options_from_params(&params) {
        Ok(options) => options,
        Err(msg) => return Ok(CallToolResult::error(vec![Content::text(msg)])),
    };
    let result = run_api_blocking("trace_export", move || {
        run_trace_export(&options).and_then(serialize_trace_export_programmatic_json)
    })
    .await?
    .map_or_else(
        |err| CallToolResult::error(vec![Content::text(programmatic_error_body(&err))]),
        |value| json_success(&value),
    );
    Ok(result)
}

/// Run `trace_file` through the typed API.
pub async fn run_trace_file_tool(params: TraceFileParams) -> Result<CallToolResult, McpError> {
    let options = match trace_file_options_from_params(&params) {
        Ok(options) => options,
        Err(msg) => return Ok(CallToolResult::error(vec![Content::text(msg)])),
    };
    let result = run_api_blocking("trace_file", move || {
        run_trace_file(&options).and_then(serialize_trace_file_programmatic_json)
    })
    .await?
    .map_or_else(
        |err| CallToolResult::error(vec![Content::text(programmatic_error_body(&err))]),
        |value| json_success(&value),
    );
    Ok(result)
}

/// Run `trace_dependency` through the typed API.
pub async fn run_trace_dependency_tool(
    params: TraceDependencyParams,
) -> Result<CallToolResult, McpError> {
    let options = match trace_dependency_options_from_params(&params) {
        Ok(options) => options,
        Err(msg) => return Ok(CallToolResult::error(vec![Content::text(msg)])),
    };
    let result = run_api_blocking("trace_dependency", move || {
        run_trace_dependency(&options).and_then(serialize_trace_dependency_programmatic_json)
    })
    .await?
    .map_or_else(
        |err| CallToolResult::error(vec![Content::text(programmatic_error_body(&err))]),
        |value| json_success(&value),
    );
    Ok(result)
}

/// Run `trace_clone` through the typed API.
pub async fn run_trace_clone_tool(params: TraceCloneParams) -> Result<CallToolResult, McpError> {
    let options = match trace_clone_options_from_params(&params) {
        Ok(options) => options,
        Err(msg) => return Ok(CallToolResult::error(vec![Content::text(msg)])),
    };
    let result = run_api_blocking("trace_clone", move || {
        run_trace_clone(&options).and_then(serialize_trace_clone_programmatic_json)
    })
    .await?
    .map_or_else(
        |err| CallToolResult::error(vec![Content::text(programmatic_error_body(&err))]),
        |value| json_success(&value),
    );
    Ok(result)
}

pub fn run_trace_export_api_value(params: &TraceExportParams) -> Result<serde_json::Value, String> {
    let options = trace_export_options_from_params(params)?;
    run_trace_export(&options)
        .and_then(serialize_trace_export_programmatic_json)
        .map_err(|err| programmatic_error_body(&err))
}

pub fn run_trace_file_api_value(params: &TraceFileParams) -> Result<serde_json::Value, String> {
    let options = trace_file_options_from_params(params)?;
    run_trace_file(&options)
        .and_then(serialize_trace_file_programmatic_json)
        .map_err(|err| programmatic_error_body(&err))
}

pub fn run_trace_dependency_api_value(
    params: &TraceDependencyParams,
) -> Result<serde_json::Value, String> {
    let options = trace_dependency_options_from_params(params)?;
    run_trace_dependency(&options)
        .and_then(serialize_trace_dependency_programmatic_json)
        .map_err(|err| programmatic_error_body(&err))
}

pub fn run_trace_clone_api_value(params: &TraceCloneParams) -> Result<serde_json::Value, String> {
    let options = trace_clone_options_from_params(params)?;
    run_trace_clone(&options)
        .and_then(serialize_trace_clone_programmatic_json)
        .map_err(|err| programmatic_error_body(&err))
}

/// Build CLI arguments for the `trace_export` tool.
pub fn build_trace_export_args(params: &TraceExportParams) -> Result<Vec<String>, String> {
    require_non_empty("file", &params.file)?;
    require_non_empty("export_name", &params.export_name)?;

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
    push_scope(&mut args, params.production, params.workspace.as_deref());
    args.extend([
        "--trace".to_string(),
        format!("{}:{}", params.file, params.export_name),
    ]);
    Ok(args)
}

/// Build CLI arguments for the `trace_file` tool.
pub fn build_trace_file_args(params: &TraceFileParams) -> Result<Vec<String>, String> {
    require_non_empty("file", &params.file)?;

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
    push_scope(&mut args, params.production, params.workspace.as_deref());
    args.extend(["--trace-file".to_string(), params.file.clone()]);
    Ok(args)
}

/// Build CLI arguments for the `trace_dependency` tool.
pub fn build_trace_dependency_args(params: &TraceDependencyParams) -> Result<Vec<String>, String> {
    require_non_empty("package_name", &params.package_name)?;

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
    push_scope(&mut args, params.production, params.workspace.as_deref());
    args.extend([
        "--trace-dependency".to_string(),
        params.package_name.clone(),
    ]);
    Ok(args)
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
    if params.skip_local == Some(true) {
        args.push("--skip-local".to_string());
    }
    if params.cross_language == Some(true) {
        args.push("--cross-language".to_string());
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
            "Invalid mode '{mode}'. Valid values: strict, mild, weak, semantic"
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

fn require_non_empty(field: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(validation_error_body(format!("{field} must not be empty")));
    }
    Ok(())
}

fn trace_export_options_from_params(
    params: &TraceExportParams,
) -> Result<TraceExportOptions, String> {
    require_non_empty("file", &params.file)?;
    require_non_empty("export_name", &params.export_name)?;
    Ok(TraceExportOptions {
        analysis: dead_code_analysis_options(
            params.root.as_deref(),
            params.config.as_deref(),
            params.production,
            params.workspace.as_deref(),
            params.no_cache,
            params.threads,
        ),
        file: params.file.clone(),
        export_name: params.export_name.clone(),
    })
}

fn trace_file_options_from_params(params: &TraceFileParams) -> Result<TraceFileOptions, String> {
    require_non_empty("file", &params.file)?;
    Ok(TraceFileOptions {
        analysis: dead_code_analysis_options(
            params.root.as_deref(),
            params.config.as_deref(),
            params.production,
            params.workspace.as_deref(),
            params.no_cache,
            params.threads,
        ),
        file: params.file.clone(),
    })
}

fn trace_dependency_options_from_params(
    params: &TraceDependencyParams,
) -> Result<TraceDependencyOptions, String> {
    require_non_empty("package_name", &params.package_name)?;
    Ok(TraceDependencyOptions {
        analysis: dead_code_analysis_options(
            params.root.as_deref(),
            params.config.as_deref(),
            params.production,
            params.workspace.as_deref(),
            params.no_cache,
            params.threads,
        ),
        package_name: params.package_name.clone(),
    })
}

fn trace_clone_options_from_params(params: &TraceCloneParams) -> Result<TraceCloneOptions, String> {
    Ok(TraceCloneOptions {
        duplication: DuplicationOptions {
            analysis: AnalysisOptions {
                root: non_empty_path(params.root.as_deref()),
                config_path: non_empty_path(params.config.as_deref()),
                no_cache: params.no_cache.unwrap_or(false),
                threads: params.threads,
                workspace: non_empty_string(params.workspace.as_deref()).map(|value| vec![value]),
                ..AnalysisOptions::default()
            },
            mode: duplication_mode_from_param(params.mode.as_deref())?,
            min_tokens: params.min_tokens.map(|value| value as usize),
            min_lines: params.min_lines.map(|value| value as usize),
            min_occurrences: min_occurrences_from_param(params.min_occurrences)?,
            threshold: params.threshold,
            skip_local: params.skip_local,
            cross_language: params.cross_language,
            ignore_imports: params.ignore_imports,
            top: None,
        },
        target: trace_clone_target(params)?,
    })
}

fn dead_code_analysis_options(
    root: Option<&str>,
    config: Option<&str>,
    production: Option<bool>,
    workspace: Option<&str>,
    no_cache: Option<bool>,
    threads: Option<usize>,
) -> AnalysisOptions {
    AnalysisOptions {
        root: non_empty_path(root),
        config_path: non_empty_path(config),
        no_cache: no_cache.unwrap_or(false),
        threads,
        production: production.unwrap_or(false),
        production_override: production,
        diff_file: env_diff_file(),
        workspace: non_empty_string(workspace).map(|value| vec![value]),
        ..AnalysisOptions::default()
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

fn duplication_mode_from_param(mode: Option<&str>) -> Result<Option<DuplicationMode>, String> {
    match mode {
        None | Some("") => Ok(None),
        Some("strict") => Ok(Some(DuplicationMode::Strict)),
        Some("mild") => Ok(Some(DuplicationMode::Mild)),
        Some("weak") => Ok(Some(DuplicationMode::Weak)),
        Some("semantic") => Ok(Some(DuplicationMode::Semantic)),
        Some(mode) => Err(validation_error_body(format!(
            "Invalid mode '{mode}'. Valid values: strict, mild, weak, semantic"
        ))),
    }
}

fn min_occurrences_from_param(value: Option<u32>) -> Result<Option<usize>, String> {
    match value {
        Some(value) if value < 2 => Err(validation_error_body(format!(
            "min_occurrences must be at least 2 (got {value})"
        ))),
        Some(value) => Ok(Some(value as usize)),
        None => Ok(None),
    }
}
