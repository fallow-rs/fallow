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
/// `rule:path:line:col`.
///
/// This is the single source of truth for both the JSON `finding_id` field and
/// the SARIF `partialFingerprints` value, so an agent can join the two and they
/// never drift. The digest is computed on the project-relative path, so callers
/// must pass the relativized path (issue #900).
#[must_use]
pub fn security_finding_id(finding: &SecurityFinding, relative_path: &Path) -> String {
    let fingerprint = format!(
        "{}:{}:{}:{}",
        security_rule_id(finding),
        relative_path.to_string_lossy().replace('\\', "/"),
        finding.line,
        finding.col,
    );
    fallow_types::identity::fnv1a64_hex(fingerprint.as_bytes())
}

/// Set the `finding_id` of each finding in `findings`.
///
/// The shared analysis pipeline calls this once, directly after detection, so
/// the CLI, MCP and the LSP all read the same id. A path
/// under `root` is made root-relative before the digest. A path outside `root`
/// stays as it is, as in the CLI JSON output.
pub fn stamp_security_finding_ids(findings: &mut [SecurityFinding], root: &Path) {
    for finding in findings {
        let relative = finding.path.strip_prefix(root).unwrap_or(&finding.path);
        finding.finding_id = security_finding_id(finding, relative);
    }
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

    use super::{security_finding_id, security_rule_id, stamp_security_finding_ids};

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

    /// Pins the exact digest. SARIF `fallowSecurity/v2` and the JSON
    /// `finding_id` carry it, so a change breaks every saved id. The expected
    /// value comes from an independent FNV-1a 64 script over
    /// `security/client-server-leak:src/app.tsx:12:0`.
    #[test]
    fn finding_id_golden_value() {
        let finding = finding(SecurityFindingKind::ClientServerLeak, None);

        assert_eq!(
            security_finding_id(&finding, Path::new("src/app.tsx")),
            "ea8fc221d62fda15"
        );
    }

    #[test]
    fn finding_id_distinguishes_same_rule_sinks_on_one_line() {
        let mut first = finding(SecurityFindingKind::TaintedSink, Some("dynamic-regex"));
        first.col = 12;
        let mut second = first.clone();
        second.col = 48;

        assert_ne!(
            security_finding_id(&first, Path::new("src/patterns.ts")),
            security_finding_id(&second, Path::new("src/patterns.ts"))
        );
    }

    #[test]
    fn finding_id_normalizes_windows_separators() {
        let finding = finding(SecurityFindingKind::TaintedSink, Some("dangerous-html"));

        assert_eq!(
            security_finding_id(&finding, Path::new("src\\app.tsx")),
            security_finding_id(&finding, Path::new("src/app.tsx"))
        );
    }

    #[test]
    fn stamp_uses_the_root_relative_path() {
        let mut findings = vec![finding(SecurityFindingKind::ClientServerLeak, None)];
        stamp_security_finding_ids(&mut findings, Path::new("/repo"));

        assert_eq!(
            findings[0].finding_id,
            security_finding_id(&findings[0], Path::new("src/a.ts"))
        );
    }

    #[test]
    fn stamp_keeps_a_path_outside_the_root() {
        let mut findings = vec![finding(SecurityFindingKind::ClientServerLeak, None)];
        stamp_security_finding_ids(&mut findings, Path::new("/elsewhere"));

        assert_eq!(
            findings[0].finding_id,
            security_finding_id(&findings[0], Path::new("/repo/src/a.ts"))
        );
    }
}
