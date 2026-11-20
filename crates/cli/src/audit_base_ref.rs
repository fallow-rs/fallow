use std::path::PathBuf;
use std::process::ExitCode;

use fallow_api::audit_run::{AuditBaseError, AuditBaseOrigin, audit_base_override};

use rustc_hash::FxHashSet;

use crate::base_worktree::git_rev_parse;
use crate::error::emit_input_error;

use super::AuditOptions;

/// Resolve the base ref and an optional human-readable provenance for the scope
/// line through `fallow_api::audit_run::resolve_audit_base`, the one resolver
/// that the typed audit also uses. Precedence: explicit `--changed-since` /
/// `--base` flag, then the `FALLOW_AUDIT_BASE` env override, then
/// auto-detection.
pub fn resolve_base_ref(
    opts: &AuditOptions<'_>,
) -> Result<fallow_engine::repo_refs::ResolvedAuditBase, ExitCode> {
    match fallow_api::audit_run::resolve_audit_base(opts.root, opts.changed_since) {
        Ok(resolved) => Ok(resolved),
        Err(error) => Err(emit_input_error(
            &base_ref_error_message(&error),
            opts.output,
        )),
    }
}

/// The files an audit compares, or the exit-2 document naming why git could
/// not say.
///
/// Resolved without the shared `--changed-since` warning on purpose. That
/// sentence says the report covers the whole project instead of the changed
/// files, which is what every command that WIDENS does; audit widens nothing,
/// it stops here. It also names `--changed-since`, and audit's flag is
/// `--base`. The cause travels into the error document instead, folded onto one
/// line so a CI log keeps one fact per record.
pub(super) fn audit_changed_files(
    opts: &AuditOptions<'_>,
    base_ref: &str,
) -> Result<FxHashSet<PathBuf>, ExitCode> {
    crate::check::try_get_changed_files(opts.root, base_ref).map_err(|err| {
        let cause = err
            .describe()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        emit_input_error(
            &format!(
                "could not determine changed files for base ref '{base_ref}': {cause}. Verify the ref exists in this git repository"
            ),
            opts.output,
        )
    })
}

/// Whether an audit with no changed file compared HEAD with itself because a
/// shallow clone gave it no base. True when the base was auto-detected (no
/// `--base` and no `FALLOW_AUDIT_BASE`), the repository is a shallow clone,
/// and `base_ref` names the HEAD commit. A CI checkout with `fetch-depth: 1`
/// has this shape: the only commit that the clone has is HEAD, so every
/// detected base is HEAD.
pub(super) fn shallow_clone_base_is_head(opts: &AuditOptions<'_>, base_ref: &str) -> bool {
    if opts.changed_since.is_some() || audit_base_override().is_some() {
        return false;
    }
    if git_rev_parse(opts.root, "--is-shallow-repository").as_deref() != Some("true") {
        return false;
    }
    let base_commit = git_rev_parse(opts.root, &format!("{base_ref}^{{commit}}"));
    base_commit.is_some() && base_commit == git_rev_parse(opts.root, "HEAD")
}

fn base_ref_error_message(error: &AuditBaseError) -> String {
    match error {
        AuditBaseError::InvalidRef {
            origin: AuditBaseOrigin::Environment,
            value,
            reason,
        } => format!("FALLOW_AUDIT_BASE='{value}' is not a valid git ref: {reason}"),
        AuditBaseError::InvalidRef {
            origin: AuditBaseOrigin::ChangedSince,
            value,
            reason,
        } => format!("FALLOW_CHANGED_SINCE='{value}' is not a valid git ref: {reason}"),
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
