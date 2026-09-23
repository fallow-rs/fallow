use std::process::ExitCode;

use fallow_api::audit_run::{AuditBaseError, AuditBaseOrigin};

use crate::error::emit_error;

use super::AuditOptions;

/// Resolve the base ref and an optional human-readable provenance for the scope
/// line through `fallow_api::audit_run::resolve_audit_base`, the one resolver
/// that the typed audit also uses. Precedence: explicit `--changed-since` /
/// `--base` flag, then the `FALLOW_AUDIT_BASE` env override, then
/// auto-detection.
pub fn resolve_base_ref(opts: &AuditOptions<'_>) -> Result<(String, Option<String>), ExitCode> {
    match fallow_api::audit_run::resolve_audit_base(opts.root, opts.changed_since) {
        Ok(resolved) => Ok((resolved.git_ref, resolved.description)),
        Err(error) => Err(emit_error(&base_ref_error_message(&error), 2, opts.output)),
    }
}

fn base_ref_error_message(error: &AuditBaseError) -> String {
    match error {
        AuditBaseError::InvalidRef {
            origin: AuditBaseOrigin::Environment,
            value,
            reason,
        } => format!("FALLOW_AUDIT_BASE='{value}' is not a valid git ref: {reason}"),
        AuditBaseError::InvalidRef {
            origin: AuditBaseOrigin::Detected,
            value,
            reason,
        } => format!("auto-detected base ref '{value}' is not a valid git ref: {reason}"),
        AuditBaseError::InvalidRef {
            origin: AuditBaseOrigin::Explicit,
            value,
            reason,
        } => format!("--base '{value}' is not a valid git ref: {reason}"),
        AuditBaseError::NotDetected => "could not detect base branch. Use --base <ref> to specify the comparison target (e.g., --base main)".to_string(),
    }
}
