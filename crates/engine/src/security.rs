//! Security metadata helpers owned by the engine boundary.
//!
//! The severity policy, the catalogue-title lookup, the rule and finding
//! identifiers, and the rule-enabling policy are shared with the core analyze
//! ranking pass via fallow-security so the CLI, SARIF, JSON, and viz Security
//! surfaces can never diverge.

pub use fallow_security::{
    derive_security_severity, enable_security_rules, resolve_security_finding_severity,
    security_catalogue_title, security_finding_id, security_rule_id, security_rules_can_error,
    stamp_security_finding_ids,
};
