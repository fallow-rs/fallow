use std::process::ExitCode;

use crate::error::emit_error;

use super::AuditOptions;

/// Parse a raw `FALLOW_AUDIT_BASE` value: trim, treat empty / whitespace-only as
/// unset. Pure helper so the trimming logic is testable without mutating env.
pub fn parse_audit_base_override(raw: Option<String>) -> Option<String> {
    let trimmed = raw?.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// The `FALLOW_AUDIT_BASE` override (trimmed), or `None` when unset / empty.
/// Lets a downstream consumer pin the base without editing the generated agent
/// gate script (issue #1168), e.g. `FALLOW_AUDIT_BASE=upstream/main` on a fork.
fn audit_base_env_override() -> Option<String> {
    parse_audit_base_override(std::env::var("FALLOW_AUDIT_BASE").ok())
}

/// Resolve the base ref and an optional human-readable provenance for the scope
/// line. Precedence: explicit `--changed-since` / `--base` flag, then the
/// `FALLOW_AUDIT_BASE` env override, then auto-detection.
pub fn resolve_base_ref(opts: &AuditOptions<'_>) -> Result<(String, Option<String>), ExitCode> {
    if let Some(ref_str) = opts.changed_since {
        return Ok((ref_str.to_string(), None));
    }
    if let Some(env_ref) = audit_base_env_override() {
        if let Err(e) = crate::validate::validate_git_ref(&env_ref) {
            return Err(emit_error(
                &format!("FALLOW_AUDIT_BASE='{env_ref}' is not a valid git ref: {e}"),
                2,
                opts.output,
            ));
        }
        let description = format!("FALLOW_AUDIT_BASE={env_ref}");
        return Ok((env_ref, Some(description)));
    }
    let Some(detected) = fallow_engine::repo_refs::auto_detect_audit_base_ref(opts.root) else {
        return Err(emit_error(
            "could not detect base branch. Use --base <ref> to specify the comparison target (e.g., --base main)",
            2,
            opts.output,
        ));
    };
    if let Err(e) = crate::validate::validate_git_ref(&detected.git_ref) {
        return Err(emit_error(
            &format!(
                "auto-detected base ref '{}' is not a valid git ref: {e}",
                detected.git_ref
            ),
            2,
            opts.output,
        ));
    }
    Ok((detected.git_ref, detected.description))
}
