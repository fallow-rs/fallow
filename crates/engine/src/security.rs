//! Security metadata helpers owned by the engine boundary.

use fallow_types::results::{SecurityFinding, SecurityRuntimeState, SecuritySeverity};

/// Derive the review-priority severity for a security candidate.
#[must_use]
pub fn derive_security_severity(finding: &SecurityFinding) -> SecuritySeverity {
    if finding
        .runtime
        .as_ref()
        .is_some_and(|runtime| runtime.state == SecurityRuntimeState::RuntimeHot)
        || finding.candidate.boundary.client_server
        || finding
            .candidate
            .boundary
            .architecture_zone
            .as_ref()
            .is_some()
        || finding
            .reachability
            .as_ref()
            .is_some_and(|reach| reach.crosses_boundary)
        || finding
            .reachability
            .as_ref()
            .is_some_and(|reach| reach.reachable_from_entry && finding.source_backed)
    {
        return SecuritySeverity::High;
    }

    if finding.source_backed
        || finding
            .reachability
            .as_ref()
            .is_some_and(|reach| reach.reachable_from_untrusted_source)
    {
        return SecuritySeverity::Medium;
    }

    SecuritySeverity::Low
}

/// Return the human-readable title for a security catalogue identifier.
#[must_use]
pub fn security_catalogue_title(kind: &str) -> Option<&'static str> {
    if kind == fallow_security::HARDCODED_SECRET_CATEGORY_ID {
        Some(fallow_security::HARDCODED_SECRET_CATEGORY_TITLE)
    } else {
        fallow_security::catalogue_title(kind)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_types::{
        output::IssueAction,
        results::{
            SecurityCandidate, SecurityCandidateBoundary, SecurityCandidateSink, SecurityFinding,
            SecurityFindingKind, SecurityReachability, SecurityRuntimeContext,
            SecurityRuntimeState, SecuritySeverity, SecurityZoneCrossing, TraceHop, TraceHopRole,
        },
    };

    use super::derive_security_severity;

    fn finding(name: &str) -> SecurityFinding {
        let path = PathBuf::from("/repo").join(name);
        SecurityFinding {
            finding_id: String::new(),
            kind: SecurityFindingKind::TaintedSink,
            category: Some("dangerous-html".to_string()),
            cwe: Some(79),
            path: path.clone(),
            line: 1,
            col: 0,
            evidence: "candidate".to_string(),
            source_backed: false,
            source_read: None,
            severity: SecuritySeverity::Low,
            trace: vec![TraceHop {
                path: path.clone(),
                line: 1,
                col: 0,
                role: TraceHopRole::Sink,
            }],
            actions: Vec::<IssueAction>::new(),
            dead_code: None,
            reachability: None,
            candidate: SecurityCandidate {
                source_kind: None,
                sink: SecurityCandidateSink {
                    path,
                    line: 1,
                    col: 0,
                    category: Some("dangerous-html".to_string()),
                    cwe: Some(79),
                    callee: None,
                    url_shape: None,
                },
                boundary: SecurityCandidateBoundary::default(),
                network: None,
            },
            taint_flow: None,
            runtime: None,
            attack_surface: None,
        }
    }

    fn reachability(
        reachable_from_entry: bool,
        reachable_from_untrusted_source: bool,
        crosses_boundary: bool,
    ) -> SecurityReachability {
        SecurityReachability {
            reachable_from_entry,
            reachable_from_untrusted_source,
            taint_confidence: None,
            untrusted_source_hop_count: None,
            untrusted_source_trace: vec![],
            blast_radius: 1,
            crosses_boundary,
        }
    }

    #[test]
    fn derives_security_severity_from_typed_signals() {
        assert_eq!(
            derive_security_severity(&finding("baseline.ts")),
            SecuritySeverity::Low
        );

        let mut source_backed = finding("source-backed.ts");
        source_backed.source_backed = true;
        assert_eq!(
            derive_security_severity(&source_backed),
            SecuritySeverity::Medium
        );

        let mut source_reachable = finding("source-reachable.ts");
        source_reachable.reachability = Some(reachability(false, true, false));
        assert_eq!(
            derive_security_severity(&source_reachable),
            SecuritySeverity::Medium
        );

        let mut client_boundary = finding("client-boundary.ts");
        client_boundary.candidate.boundary.client_server = true;

        let mut architecture_boundary = finding("architecture-boundary.ts");
        architecture_boundary.candidate.boundary.architecture_zone = Some(SecurityZoneCrossing {
            from: "web".to_string(),
            to: "server".to_string(),
        });

        let mut crossed_boundary = finding("crossed-boundary.ts");
        crossed_boundary.reachability = Some(reachability(false, false, true));

        let mut source_backed_entry = finding("source-backed-entry.ts");
        source_backed_entry.source_backed = true;
        source_backed_entry.reachability = Some(reachability(true, false, false));

        let mut runtime_hot = finding("runtime-hot.ts");
        runtime_hot.runtime = Some(SecurityRuntimeContext {
            state: SecurityRuntimeState::RuntimeHot,
            function: "handler".to_string(),
            line: 1,
            invocations: Some(500),
            stable_id: Some("fallow:fn:test".to_string()),
            evidence: Some("runtime hot path".to_string()),
        });

        for finding in [
            client_boundary,
            architecture_boundary,
            crossed_boundary,
            source_backed_entry,
            runtime_hot,
        ] {
            assert_eq!(derive_security_severity(&finding), SecuritySeverity::High);
        }
    }
}
