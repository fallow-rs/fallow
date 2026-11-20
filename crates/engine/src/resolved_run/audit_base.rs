//! The base ref of an audit, before git sees it.
//!
//! The order is the base ref that the caller names, then
//! `FALLOW_AUDIT_BASE`, then the base that the audit detects from the
//! upstream or the remote default branch. Only when the audit detects no
//! base does the MCP server use `FALLOW_CHANGED_SINCE`. The detection reads
//! git, so the caller of [`decide_audit_base`] runs it.

use fallow_config::{CommandKind, EnvVar, RunInputs};
use fallow_types::sourced::{Scope, Source, Sourced};

use super::env_policy::applies;

/// The base ref candidates of one audit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditBaseDecision {
    /// The base ref that the caller or `FALLOW_AUDIT_BASE` names. `None`
    /// means the audit detects the base.
    pub named: Option<Sourced<String>>,
    /// The base ref when the audit detects no base: `FALLOW_CHANGED_SINCE`
    /// on the MCP server.
    pub fallback: Option<Sourced<String>>,
}

/// Decide the base ref candidates of an audit from the run inputs. A run of
/// another command has none.
///
/// A `FALLOW_AUDIT_BASE` value is trimmed. An empty value, a value with only
/// spaces and a value that is not valid UTF-8 count as unset, as an empty
/// `FALLOW_CHANGED_SINCE` does.
#[must_use]
pub fn decide_audit_base(inputs: &RunInputs) -> AuditBaseDecision {
    if inputs.command != CommandKind::Audit {
        return AuditBaseDecision::default();
    }
    let read = |var: EnvVar| {
        if !applies(var, inputs.surface(), inputs.command) {
            return None;
        }
        inputs
            .env()
            .get(var)
            .and_then(|value| value.to_str())
            .map(str::to_owned)
    };
    let environment = |value: String, var: EnvVar| {
        Sourced::new(
            value,
            Source::Environment { name: var.name() },
            Scope::Global,
        )
    };
    let named = inputs
        .audit_base
        .as_ref()
        .map(|given| {
            Sourced::new(
                given.value.clone(),
                Source::Given {
                    origin: given.origin,
                },
                Scope::Global,
            )
        })
        .or_else(|| {
            read(EnvVar::AuditBase)
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .map(|value| environment(value, EnvVar::AuditBase))
        });
    let fallback = read(EnvVar::ChangedSince)
        .filter(|value| !value.is_empty())
        .map(|value| environment(value, EnvVar::ChangedSince));
    AuditBaseDecision { named, fallback }
}

#[cfg(test)]
#[path = "audit_base_tests.rs"]
mod tests;
