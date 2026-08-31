//! Stable identifiers for security candidates.
//!
//! JSON, SARIF, and the viz Security lens all join on these two strings, so the
//! rule id and the per-finding correlation id live here rather than in any one
//! consumer. A second implementation would let the surfaces drift apart.

use std::path::Path;

use fallow_types::results::{SecurityFinding, SecurityFindingKind};

/// The `category` string distinguishing the server-only-import sink from the
/// secret-leak sink. Both carry the `ClientServerLeak` kind, so the category is
/// the only thing that tells them apart. Matches the constant in
/// `crates/core/src/analyze/security/mod.rs`.
const SERVER_ONLY_CATEGORY: &str = "server-only-import";

/// The stable rule identifier for a finding.
///
/// The secret-leak `ClientServerLeak` keeps its bespoke id; the server-only
/// variant gets `security/server-only-import` so a SARIF consumer tells
/// "reaches server-only code" apart from "reads a secret". Each `TaintedSink`
/// category gets `security/<category>` so candidates group per CWE class.
#[must_use]
pub fn security_rule_id(finding: &SecurityFinding) -> String {
    match finding.kind {
        SecurityFindingKind::ClientServerLeak
            if finding.category.as_deref() == Some(SERVER_ONLY_CATEGORY) =>
        {
            "security/server-only-import".to_owned()
        }
        SecurityFindingKind::ClientServerLeak => "security/client-server-leak".to_owned(),
        SecurityFindingKind::TaintedSink => format!(
            "security/{}",
            finding.category.as_deref().unwrap_or("tainted-sink")
        ),
    }
}

/// The stable per-finding correlation id: an FNV-1a hex digest of
/// `rule:path:line`.
///
/// This is the single source of truth for both the JSON `finding_id` field and
/// the SARIF `partialFingerprints` value, so an agent can join the two and they
/// never drift. The digest is computed on the project-relative path, so callers
/// must pass the relativized path (issue #900).
#[must_use]
pub fn security_finding_id(finding: &SecurityFinding, relative_path: &Path) -> String {
    let fingerprint = format!(
        "{}:{}:{}",
        security_rule_id(finding),
        relative_path.to_string_lossy().replace('\\', "/"),
        finding.line,
    );
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in fingerprint.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use fallow_types::{
        output::IssueAction,
        results::{
            SecurityCandidate, SecurityCandidateBoundary, SecurityCandidateSink, SecurityFinding,
            SecurityFindingKind, SecuritySeverity, TraceHop, TraceHopRole,
        },
    };

    use super::{security_finding_id, security_rule_id};

    fn finding(kind: SecurityFindingKind, category: Option<&str>) -> SecurityFinding {
        let path = PathBuf::from("/repo/src/a.ts");
        SecurityFinding {
            finding_id: String::new(),
            kind,
            category: category.map(str::to_owned),
            cwe: Some(79),
            path: path.clone(),
            line: 12,
            col: 0,
            evidence: "candidate".to_owned(),
            source_backed: false,
            source_read: None,
            severity: SecuritySeverity::Low,
            trace: vec![TraceHop {
                path: path.clone(),
                line: 12,
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
                    line: 12,
                    col: 0,
                    category: category.map(str::to_owned),
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

    #[test]
    fn rule_id_separates_the_two_client_server_leak_variants() {
        assert_eq!(
            security_rule_id(&finding(SecurityFindingKind::ClientServerLeak, None)),
            "security/client-server-leak"
        );
        assert_eq!(
            security_rule_id(&finding(
                SecurityFindingKind::ClientServerLeak,
                Some("server-only-import"),
            )),
            "security/server-only-import"
        );
        assert_eq!(
            security_rule_id(&finding(
                SecurityFindingKind::TaintedSink,
                Some("dangerous-html"),
            )),
            "security/dangerous-html"
        );
        assert_eq!(
            security_rule_id(&finding(SecurityFindingKind::TaintedSink, None)),
            "security/tainted-sink"
        );
    }

    #[test]
    fn finding_id_is_deterministic_and_16_hex_digits() {
        let finding = finding(SecurityFindingKind::ClientServerLeak, None);
        let id = security_finding_id(&finding, Path::new("src/app.tsx"));

        assert_eq!(id, security_finding_id(&finding, Path::new("src/app.tsx")));
        assert_eq!(id.len(), 16);
        assert!(id.chars().all(|character| character.is_ascii_hexdigit()));
        assert_ne!(id, security_finding_id(&finding, Path::new("src/b.tsx")));
    }

    #[test]
    fn finding_id_normalizes_windows_separators() {
        let finding = finding(SecurityFindingKind::TaintedSink, Some("dangerous-html"));

        assert_eq!(
            security_finding_id(&finding, Path::new("src\\app.tsx")),
            security_finding_id(&finding, Path::new("src/app.tsx"))
        );
    }
}
