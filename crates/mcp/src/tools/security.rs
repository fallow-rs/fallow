use crate::params::SecurityCandidatesParams;

use super::{push_global, push_str_flag, validation_error_body};

fn has_value(value: Option<&str>) -> bool {
    value.is_some_and(|s| !s.is_empty())
}

/// Build CLI arguments for the `security_candidates` tool.
pub fn build_security_candidates_args(
    params: &SecurityCandidatesParams,
) -> Result<Vec<String>, String> {
    if has_value(params.workspace.as_deref()) && has_value(params.changed_workspaces.as_deref()) {
        return Err(validation_error_body(
            "workspace and changed_workspaces are mutually exclusive for security_candidates",
        ));
    }

    let mut args = vec![
        "security".to_string(),
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
    push_str_flag(&mut args, "--workspace", params.workspace.as_deref());
    push_str_flag(
        &mut args,
        "--changed-since",
        params.changed_since.as_deref(),
    );
    if let Some(paths) = params.paths.as_ref() {
        for path in paths {
            if path.trim().is_empty() {
                return Err(validation_error_body("paths entries must not be empty"));
            }
            args.extend(["--file".to_string(), path.clone()]);
        }
    }
    push_str_flag(
        &mut args,
        "--changed-workspaces",
        params.changed_workspaces.as_deref(),
    );

    Ok(args)
}
