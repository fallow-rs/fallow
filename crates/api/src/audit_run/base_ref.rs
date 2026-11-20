//! The base ref that an audit compares against.

use std::path::Path;

use fallow_engine::repo_refs::{self, ResolvedAuditBase};
use fallow_engine::resolved_run::AuditBaseDecision;
use fallow_types::sourced::Source;

/// Where an audit base ref came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditBaseOrigin {
    /// The caller named the ref (`--base`, `audit.base`, `changedSince`).
    Explicit,
    /// The `FALLOW_AUDIT_BASE` environment variable.
    Environment,
    /// The `FALLOW_CHANGED_SINCE` environment variable of the MCP server,
    /// read when the audit detects no base.
    ChangedSince,
    /// Auto-detection from the upstream or the remote default branch.
    Detected,
}

/// Why no audit base ref could be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditBaseError {
    /// The ref is not a valid git ref.
    InvalidRef {
        /// Where the ref came from.
        origin: AuditBaseOrigin,
        /// The ref as it was given or detected.
        value: String,
        /// Why the ref is not valid.
        reason: String,
    },
    /// No explicit ref, no environment override, and no base branch to
    /// detect.
    NotDetected,
}

/// Parse a raw `FALLOW_AUDIT_BASE` value: trimmed, and `None` when it is
/// empty or only whitespace.
#[must_use]
pub fn parse_audit_base_override(raw: Option<String>) -> Option<String> {
    let trimmed = raw?.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// The `FALLOW_AUDIT_BASE` override of the process, parsed with
/// [`parse_audit_base_override`].
#[must_use]
pub fn audit_base_override() -> Option<String> {
    parse_audit_base_override(std::env::var("FALLOW_AUDIT_BASE").ok())
}

/// Resolve the base ref of an audit rooted at `root`.
///
/// The order is: the explicit ref, then the `FALLOW_AUDIT_BASE` override
/// (issue #1168: a consumer can pin the base without editing a generated gate
/// script), then auto-detection. Each ref is validated before git sees it.
///
/// # Errors
///
/// Returns [`AuditBaseError::InvalidRef`] for a ref that is not a valid git
/// ref, and [`AuditBaseError::NotDetected`] when no base branch can be found.
pub fn resolve_audit_base(
    root: &Path,
    explicit: Option<&str>,
) -> Result<ResolvedAuditBase, AuditBaseError> {
    resolve_audit_base_with_override(root, explicit, audit_base_override())
}

fn resolve_audit_base_with_override(
    root: &Path,
    explicit: Option<&str>,
    env_override: Option<String>,
) -> Result<ResolvedAuditBase, AuditBaseError> {
    if let Some(explicit) = explicit {
        validate(explicit, AuditBaseOrigin::Explicit)?;
        return Ok(ResolvedAuditBase {
            git_ref: explicit.to_string(),
            description: None,
            inherited_refs: Vec::new(),
        });
    }
    if let Some(env_ref) = env_override {
        validate(&env_ref, AuditBaseOrigin::Environment)?;
        return Ok(ResolvedAuditBase {
            description: Some(format!("FALLOW_AUDIT_BASE={env_ref}")),
            git_ref: env_ref,
            inherited_refs: Vec::new(),
        });
    }
    let detected =
        repo_refs::auto_detect_audit_base_ref(root).ok_or(AuditBaseError::NotDetected)?;
    validate(&detected.git_ref, AuditBaseOrigin::Detected)?;
    Ok(detected)
}

/// Resolve the base ref of an audit rooted at `root` from the candidates
/// that the resolver decided: the named base, then auto-detection, then the
/// fallback.
///
/// # Errors
///
/// Returns [`AuditBaseError::InvalidRef`] for a ref that is not a valid git
/// ref, and [`AuditBaseError::NotDetected`] when no candidate gives a base.
pub fn resolve_audit_base_decided(
    root: &Path,
    decision: &AuditBaseDecision,
) -> Result<ResolvedAuditBase, AuditBaseError> {
    if let Some(named) = &decision.named {
        let value = named.value().clone();
        return if matches!(named.source(), Source::Environment { .. }) {
            resolve_audit_base_with_override(root, None, Some(value))
        } else {
            resolve_audit_base_with_override(root, Some(&value), None)
        };
    }
    match resolve_audit_base_with_override(root, None, None) {
        Err(AuditBaseError::NotDetected) => {
            let Some(fallback) = &decision.fallback else {
                return Err(AuditBaseError::NotDetected);
            };
            validate(fallback.value(), AuditBaseOrigin::ChangedSince)?;
            Ok(ResolvedAuditBase {
                description: Some(format!("FALLOW_CHANGED_SINCE={}", fallback.value())),
                git_ref: fallback.value().clone(),
                inherited_refs: Vec::new(),
            })
        }
        other => other,
    }
}

fn validate(value: &str, origin: AuditBaseOrigin) -> Result<(), AuditBaseError> {
    fallow_engine::validate::validate_git_ref(value)
        .map(|_| ())
        .map_err(|reason| AuditBaseError::InvalidRef {
            origin,
            value: value.to_string(),
            reason,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_override_is_trimmed_and_an_empty_one_is_unset() {
        assert_eq!(parse_audit_base_override(None), None);
        assert_eq!(parse_audit_base_override(Some(String::new())), None);
        assert_eq!(parse_audit_base_override(Some("   ".to_string())), None);
        assert_eq!(
            parse_audit_base_override(Some("  origin/main  ".to_string())),
            Some("origin/main".to_string())
        );
    }

    #[test]
    fn an_explicit_ref_wins_over_the_override() {
        let resolved = resolve_audit_base_with_override(
            Path::new("."),
            Some("main"),
            Some("upstream/main".to_string()),
        )
        .expect("explicit ref resolves");
        assert_eq!(resolved.git_ref, "main");
        assert_eq!(resolved.description, None);
    }

    #[test]
    fn the_override_names_its_source() {
        let resolved = resolve_audit_base_with_override(
            Path::new("."),
            None,
            Some("upstream/main".to_string()),
        )
        .expect("override resolves");
        assert_eq!(resolved.git_ref, "upstream/main");
        assert_eq!(
            resolved.description.as_deref(),
            Some("FALLOW_AUDIT_BASE=upstream/main")
        );
    }

    #[test]
    fn an_invalid_override_names_its_origin() {
        let error = resolve_audit_base_with_override(
            Path::new("."),
            None,
            Some("--upload-pack=evil".to_string()),
        )
        .expect_err("an option-like ref is refused");
        assert!(
            matches!(
                error,
                AuditBaseError::InvalidRef {
                    origin: AuditBaseOrigin::Environment,
                    ..
                }
            ),
            "{error:?}"
        );
    }
}
