//! The base ref of a typed audit and of the decision surface.

use std::path::PathBuf;

use fallow_engine::repo_refs::ResolvedAuditBase;

use super::ProgrammaticResult;
use crate::audit_run::{AuditBaseError, AuditBaseOrigin};
use crate::run::{NamedAuditBase, ProgrammaticEntry, RouteSettings};
use crate::{AuditOptions, ProgrammaticError};

/// Resolve the base ref of `options`: the base that the caller names, then
/// `FALLOW_AUDIT_BASE`, then the detected base, then `FALLOW_CHANGED_SINCE`
/// on the MCP server. The variables come from the environment snapshot of
/// `options.analysis.host`.
pub(super) fn resolve_audit_base_ref(
    options: &AuditOptions,
) -> ProgrammaticResult<ResolvedAuditBase> {
    let inputs = crate::run::run_inputs(
        &options.analysis,
        &ProgrammaticEntry::Audit {
            dead_code: options.dead_code,
            duplication: options.duplication,
            health: options.health,
            production: options.production,
            production_dead_code: options.production_dead_code,
            production_health: options.production_health,
            production_dupes: options.production_dupes,
        },
        RouteSettings {
            include_entry_exports: options.include_entry_exports,
            audit_base: NamedAuditBase::of(options),
        },
    );
    let decision = fallow_engine::resolved_run::decide_audit_base(&inputs);
    let root = options
        .analysis
        .root
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    crate::audit_run::resolve_audit_base_decided(&root, &decision).map_err(|error| match error {
        AuditBaseError::InvalidRef {
            origin,
            value,
            reason,
        } => ProgrammaticError::new(format!("invalid git ref `{value}`: {reason}"), 2)
            .with_code("FALLOW_INVALID_GIT_REF")
            .with_context(match origin {
                AuditBaseOrigin::Environment => "FALLOW_AUDIT_BASE",
                AuditBaseOrigin::ChangedSince => "FALLOW_CHANGED_SINCE",
                AuditBaseOrigin::Explicit | AuditBaseOrigin::Detected => "audit.base",
            }),
        AuditBaseError::NotDetected => ProgrammaticError::new(
            "could not detect base branch. Set audit.base to specify the comparison target",
            2,
        )
        .with_code("FALLOW_AUDIT_BASE_NOT_FOUND")
        .with_context("audit.base"),
    })
}
