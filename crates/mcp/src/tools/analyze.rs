use crate::params::AnalyzeParams;

use super::{
    ISSUE_TYPE_FLAGS, push_baseline, push_global, push_regression, push_scope,
    validation_error_body,
};

/// Build CLI arguments for the `analyze` tool.
/// Returns `Err(message)` if an invalid issue type is provided.
pub fn build_analyze_args(params: &AnalyzeParams) -> Result<Vec<String>, String> {
    let mut args = vec![
        "dead-code".to_string(),
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
    push_scope(&mut args, params.production, params.workspace.as_deref());

    // Add boundary_violations convenience param only if issue_types doesn't
    // already include it — clap rejects duplicate boolean flags.
    let types_has_boundaries = params
        .issue_types
        .as_ref()
        .is_some_and(|types| types.iter().any(|t| t == "boundary-violations"));
    if params.boundary_violations == Some(true) && !types_has_boundaries {
        args.push("--boundary-violations".to_string());
    }
    if let Some(ref types) = params.issue_types {
        for t in types {
            if let Some(&(_, flag)) = ISSUE_TYPE_FLAGS.iter().find(|&&(name, _)| name == t) {
                args.push(flag.to_string());
            } else {
                let valid = ISSUE_TYPE_FLAGS
                    .iter()
                    .map(|&(n, _)| n)
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(validation_error_body(format!(
                    "Unknown issue type '{t}'. Valid values: {valid}"
                )));
            }
        }
    }
    push_baseline(
        &mut args,
        params.baseline.as_deref(),
        params.save_baseline.as_deref(),
    );
    push_regression(
        &mut args,
        params.fail_on_regression,
        params.tolerance.as_deref(),
        params.regression_baseline.as_deref(),
        params.save_regression_baseline.as_deref(),
    );
    if let Some(ref gb) = params.group_by {
        args.extend(["--group-by".to_string(), gb.clone()]);
    }
    if let Some(ref files) = params.file {
        for f in files {
            args.extend(["--file".to_string(), f.clone()]);
        }
    }
    if params.include_entry_exports == Some(true) {
        args.push("--include-entry-exports".to_string());
    }

    Ok(args)
}
