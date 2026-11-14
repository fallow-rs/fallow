//! Line-independent identity for security candidates.
//!
//! The location id (`finding_id`) hashes the rule, path, line and column, so
//! an edit above a sink changes it. The stable id hashes the rule, the
//! project-relative path and a line-free name of the finding instead. A
//! stored verdict therefore keeps its candidate after unrelated edits.
//!
//! The name of a finding depends on its kind:
//!
//! - A sink in script code: the normalized source text of the exact sink
//!   expression. The extractor records its byte span, and the caller turns
//!   the span into text.
//! - A hard-coded secret: the binding name and the detector signal (the
//!   provider family, or `entropy`). The secret value is never hashed.
//! - A client-server leak: the project-relative paths of the first and the
//!   last trace hop, and the names of the env secrets that the server module
//!   reads.
//!
//! A finding without such a name gets no stable id. Examples are a sink in
//! template markup and a sink whose span does not parse.
//!
//! The id has the form `sec1:<16 hex digits>`. When several candidates in one
//! file have the same hash input, they are sorted by line and column. The
//! first one keeps the base id. The candidate at sorted position `k` gets the
//! suffix `~k`. This is the same tiebreak as the dead-code finding ids.
//! Candidates that an inline comment suppresses keep their position in this
//! order, so a suppression does not move an id onto another candidate.

use std::path::Path;

use fallow_types::results::{SecurityFinding, SecurityFindingKind};
use fallow_types::security_stable_id::SecurityStableIdInput;
use rustc_hash::FxHashMap;

use crate::HARDCODED_SECRET_CATEGORY_ID;
use crate::identity::security_rule_id;

/// The version prefix of every stable security id. A change to the hash
/// inputs moves this prefix, so an old id never matches a new candidate.
pub const STABLE_SECURITY_ID_SCHEME: &str = "sec1";

/// The project-relative form of `path`, with `/` separators.
fn relative_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The line-free name of `finding`, or `None` when the finding has none.
///
/// `sink_text` gets the absolute anchor path and the byte span of the sink,
/// and returns the normalized sink text.
fn finding_name(
    finding: &SecurityFinding,
    root: &Path,
    sink_text: &mut impl FnMut(&Path, u32, u32) -> Option<String>,
) -> Option<String> {
    if finding.kind == SecurityFindingKind::ClientServerLeak {
        let first = finding.trace.first()?;
        let last = finding.trace.last()?;
        let mut name = format!(
            "{}\0{}",
            relative_path(&first.path, root),
            relative_path(&last.path, root)
        );
        if let Some(SecurityStableIdInput::SecretReads(reads)) = &finding.stable_id_input {
            for read in reads {
                name.push('\0');
                name.push_str(read);
            }
        }
        return Some(name);
    }
    match finding.stable_id_input.as_ref()? {
        SecurityStableIdInput::SecretSignal(signal)
            if finding.category.as_deref() == Some(HARDCODED_SECRET_CATEGORY_ID) =>
        {
            let binding = finding.candidate.sink.callee.as_deref()?;
            Some(format!("{binding}\0{signal}"))
        }
        SecurityStableIdInput::SecretSignal(_) | SecurityStableIdInput::SecretReads(_) => None,
        &SecurityStableIdInput::SinkSpan { start, end } => {
            let absolute = if finding.path.is_absolute() {
                finding.path.clone()
            } else {
                root.join(&finding.path)
            };
            sink_text(&absolute, start, end)
        }
    }
}

/// The base stable id, without a tiebreak suffix.
#[must_use]
pub fn stable_security_base_id(rule_id: &str, relative_path: &str, name: &str) -> String {
    let hash = fallow_types::identity::fnv1a64_parts(&[
        STABLE_SECURITY_ID_SCHEME,
        rule_id,
        relative_path,
        name,
    ]);
    format!("{STABLE_SECURITY_ID_SCHEME}:{hash}")
}

/// One member of a group of candidates with the same base id.
struct GroupMember {
    line: u32,
    col: u32,
    /// The index in `findings`, or `None` for a suppressed candidate.
    index: Option<usize>,
}

/// Set the `stable_finding_id` of each finding in `findings`.
///
/// `suppressed` holds the candidates that an inline comment hides. They get no
/// id, but they keep their tiebreak position, so the id of each other finding
/// does not depend on a suppression.
///
/// `sink_text` gets the absolute anchor path and the byte span of a sink, and
/// returns its normalized source text, or `None` when that text is not known.
/// A finding without a line-free name keeps `stable_finding_id: None`.
pub fn stamp_stable_security_finding_ids(
    findings: &mut [SecurityFinding],
    suppressed: &[SecurityFinding],
    root: &Path,
    mut sink_text: impl FnMut(&Path, u32, u32) -> Option<String>,
) {
    let mut groups: FxHashMap<String, Vec<GroupMember>> = FxHashMap::default();
    let mut base_id = |finding: &SecurityFinding| {
        let name = finding_name(finding, root, &mut sink_text)?;
        Some(stable_security_base_id(
            &security_rule_id(finding),
            &relative_path(&finding.path, root),
            &name,
        ))
    };

    for (index, finding) in findings.iter_mut().enumerate() {
        finding.stable_finding_id = None;
        if let Some(base) = base_id(finding) {
            groups.entry(base).or_default().push(GroupMember {
                line: finding.line,
                col: finding.col,
                index: Some(index),
            });
        }
    }
    for finding in suppressed {
        if let Some(base) = base_id(finding) {
            groups.entry(base).or_default().push(GroupMember {
                line: finding.line,
                col: finding.col,
                index: None,
            });
        }
    }

    for (base, mut members) in groups {
        members.sort_by_key(|member| (member.line, member.col, member.index));
        for (ordinal, member) in members.into_iter().enumerate() {
            let Some(index) = member.index else {
                continue;
            };
            findings[index].stable_finding_id = Some(if ordinal == 0 {
                base.clone()
            } else {
                format!("{base}~{ordinal}")
            });
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "fixture setup asserts its own invariants directly"
)]
mod tests {
    use std::path::{Path, PathBuf};

    use fallow_types::results::{
        SecurityCandidate, SecurityCandidateBoundary, SecurityCandidateSink, SecurityFinding,
        SecurityFindingKind, SecuritySeverity, TraceHop, TraceHopRole,
    };

    use fallow_types::security_stable_id::SecurityStableIdInput;

    use super::stamp_stable_security_finding_ids;

    fn finding(line: u32, input: Option<SecurityStableIdInput>, category: &str) -> SecurityFinding {
        let path = PathBuf::from("/repo/src/a.js");
        SecurityFinding {
            finding_id: String::new(),
            stable_finding_id: None,
            stable_id_input: input,
            kind: SecurityFindingKind::TaintedSink,
            category: Some(category.to_owned()),
            cwe: None,
            path: path.clone(),
            line,
            col: 0,
            evidence: String::new(),
            source_backed: false,
            source_read: None,
            severity: SecuritySeverity::Low,
            trace: Vec::new(),
            actions: Vec::new(),
            dead_code: None,
            reachability: None,
            candidate: SecurityCandidate {
                source_kind: None,
                sink: SecurityCandidateSink {
                    path,
                    line,
                    col: 0,
                    category: Some(category.to_owned()),
                    cwe: None,
                    callee: Some("API_KEY".to_owned()),
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

    const fn span(start: u32, end: u32) -> Option<SecurityStableIdInput> {
        let input = SecurityStableIdInput::SinkSpan { start, end };
        if start < end { Some(input) } else { None }
    }

    /// Stamp `findings` with a sink text that is the span offsets, or the
    /// fixed `text` when it is given.
    fn stamp(findings: &mut [SecurityFinding], text: Option<&str>) {
        stamp_stable_security_finding_ids(findings, &[], Path::new("/repo"), |path, start, end| {
            assert_eq!(path, Path::new("/repo/src/a.js"));
            Some(text.map_or_else(|| format!("{start}..{end}"), str::to_owned))
        });
    }

    fn id(finding: &SecurityFinding) -> String {
        finding.stable_finding_id.clone().expect("stable id")
    }

    #[test]
    fn identical_sinks_get_ordinals_in_source_order() {
        let mut findings = vec![
            finding(2, span(10, 19), "command-injection"),
            finding(1, span(0, 9), "command-injection"),
        ];
        stamp(&mut findings, Some("exec ( cmd )"));
        let first = id(&findings[1]);
        assert!(first.starts_with("sec1:"));
        assert_eq!(first.len(), "sec1:".len() + 16);
        assert_eq!(id(&findings[0]), format!("{first}~1"));
    }

    #[test]
    fn a_suppressed_sink_keeps_its_ordinal() {
        let suppressed = vec![finding(1, span(0, 9), "command-injection")];
        let mut findings = vec![finding(2, span(10, 19), "command-injection")];
        stamp_stable_security_finding_ids(
            &mut findings,
            &suppressed,
            Path::new("/repo"),
            |_, _, _| Some("exec ( cmd )".to_owned()),
        );
        let mut alone = vec![finding(1, span(0, 9), "command-injection")];
        stamp(&mut alone, Some("exec ( cmd )"));
        assert_eq!(id(&findings[0]), format!("{}~1", id(&alone[0])));
    }

    #[test]
    fn the_id_follows_the_sink_text_and_not_the_line() {
        let mut before = vec![finding(1, span(0, 9), "command-injection")];
        stamp(&mut before, Some("exec ( cmd )"));
        let mut moved = vec![finding(7, span(80, 89), "command-injection")];
        stamp(&mut moved, Some("exec ( cmd )"));
        let mut changed = vec![finding(1, span(0, 11), "command-injection")];
        stamp(&mut changed, Some("exec ( other )"));
        assert_eq!(id(&before[0]), id(&moved[0]));
        assert_ne!(id(&before[0]), id(&changed[0]));
    }

    #[test]
    fn a_finding_without_known_sink_text_gets_no_id() {
        let mut findings = vec![
            finding(1, None, "dangerous-html"),
            finding(2, span(0, 9), "dangerous-html"),
        ];
        stamp_stable_security_finding_ids(&mut findings, &[], Path::new("/repo"), |_, _, _| None);
        assert_eq!(findings[0].stable_finding_id, None);
        assert_eq!(findings[1].stable_finding_id, None);
    }

    #[test]
    fn a_hardcoded_secret_hashes_the_binding_and_the_signal() {
        let secret = |signal: &str| {
            let input = Some(SecurityStableIdInput::SecretSignal(signal.to_owned()));
            let mut findings = vec![finding(1, input, "hardcoded-secret")];
            stamp_stable_security_finding_ids(&mut findings, &[], Path::new("/repo"), |_, _, _| {
                panic!("a secret never reads the source")
            });
            id(&findings[0])
        };
        assert_eq!(secret("entropy"), secret("entropy"));
        assert_ne!(secret("entropy"), secret("Stripe key"));
    }

    #[test]
    fn a_client_server_leak_hashes_the_first_and_last_trace_hop() {
        let leak = |middle: &str| {
            let hop = |path: &str| TraceHop {
                path: PathBuf::from(path),
                line: 1,
                col: 0,
                role: TraceHopRole::Intermediate,
            };
            let mut finding = finding(1, None, "client-server-leak");
            finding.kind = SecurityFindingKind::ClientServerLeak;
            finding.trace = vec![
                hop("/repo/src/a.js"),
                hop(middle),
                hop("/repo/src/secret.js"),
            ];
            let mut findings = vec![finding];
            stamp(&mut findings, None);
            id(&findings[0])
        };
        assert_eq!(leak("/repo/src/b.js"), leak("/repo/src/c.js"));
    }

    #[test]
    fn a_client_server_leak_hashes_the_secret_names() {
        let leak = |reads: &[&str]| {
            let hop = |path: &str| TraceHop {
                path: PathBuf::from(path),
                line: 1,
                col: 0,
                role: TraceHopRole::Intermediate,
            };
            let input = SecurityStableIdInput::SecretReads(
                reads.iter().map(|read| (*read).to_owned()).collect(),
            );
            let mut finding = finding(1, Some(input), "client-server-leak");
            finding.kind = SecurityFindingKind::ClientServerLeak;
            finding.trace = vec![hop("/repo/src/a.js"), hop("/repo/src/secret.js")];
            let mut findings = vec![finding];
            stamp(&mut findings, None);
            id(&findings[0])
        };
        let label = leak(&["process.env.BUILD_LABEL"]);
        assert_eq!(label, leak(&["process.env.BUILD_LABEL"]));
        assert_ne!(label, leak(&["process.env.DATABASE_PASSWORD"]));
    }
}
