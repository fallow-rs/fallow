use crate::params::{TraceCloneParams, TraceDependencyParams, TraceExportParams, TraceFileParams};

use super::{VALID_DUPES_MODES, push_global, push_scope, validation_error_body};

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
    require_non_empty("file", &params.file)?;
    if params.line == 0 {
        return Err(validation_error_body("line must be greater than 0"));
    }

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
    if let Some(ref mode) = params.mode {
        if !VALID_DUPES_MODES.contains(&mode.as_str()) {
            return Err(validation_error_body(format!(
                "Invalid mode '{mode}'. Valid values: strict, mild, weak, semantic"
            )));
        }
        args.extend(["--mode".to_string(), mode.clone()]);
    }
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
    if params.skip_local == Some(true) {
        args.push("--skip-local".to_string());
    }
    if params.cross_language == Some(true) {
        args.push("--cross-language".to_string());
    }
    if params.ignore_imports == Some(true) {
        args.push("--ignore-imports".to_string());
    }
    args.extend([
        "--trace".to_string(),
        format!("{}:{}", params.file, params.line),
    ]);

    Ok(args)
}

fn require_non_empty(field: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(validation_error_body(format!("{field} must not be empty")));
    }
    Ok(())
}
