use crate::params::FindDupesParams;

use super::{VALID_DUPES_MODES, push_baseline, push_global, push_str_flag, validation_error_body};

/// Build CLI arguments for the `find_dupes` tool.
/// Returns `Err(message)` if an invalid mode is provided.
pub fn build_find_dupes_args(params: &FindDupesParams) -> Result<Vec<String>, String> {
    let mut args = vec![
        "dupes".to_string(),
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
    push_str_flag(&mut args, "--workspace", params.workspace.as_deref());
    if let Some(ref mode) = params.mode
        && !mode.is_empty()
    {
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
    if params.explain_skipped == Some(true) {
        args.push("--explain-skipped".to_string());
    }
    if let Some(top) = params.top {
        args.extend(["--top".to_string(), top.to_string()]);
    }
    push_baseline(
        &mut args,
        params.baseline.as_deref(),
        params.save_baseline.as_deref(),
    );
    push_str_flag(
        &mut args,
        "--changed-since",
        params.changed_since.as_deref(),
    );
    push_str_flag(&mut args, "--group-by", params.group_by.as_deref());

    Ok(args)
}
