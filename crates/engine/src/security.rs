//! Security metadata helpers owned by the engine boundary.

use fallow_types::results::{SecurityFinding, SecuritySeverity};

/// Derive the review-priority severity for a security candidate.
#[must_use]
pub fn derive_security_severity(finding: &SecurityFinding) -> SecuritySeverity {
    fallow_core::analyze::derive_security_severity(finding)
}

/// Return the human-readable title for a security catalogue identifier.
#[must_use]
pub fn security_catalogue_title(kind: &str) -> Option<&'static str> {
    fallow_core::analyze::security_catalogue_title(kind)
}
