//! The `Next:` step of a `client-server-leak` candidate.
//!
//! The human CLI output and the LSP hover print the same step, so the two
//! texts cannot drift apart.
//!
//! A `server-only-import` sink comes from one of two origins. A React Router
//! `.server` module is a sink by its file name. Other sinks import server-only
//! code, such as the `server-only` package, `next/headers` or `node:fs`. The
//! advice for the two origins is different, so the step reads the origin from
//! the evidence that the analysis wrote.

use crate::results::{SecurityFinding, TraceHopRole};

/// The `category` of a `client-server-leak` candidate that reaches
/// server-only code.
const SERVER_ONLY_CATEGORY: &str = "server-only-import";

/// The phrase that the evidence of a `server-only-import` candidate contains
/// when the sink is a React Router `.server` module. The analysis writes it,
/// and [`client_server_leak_next_step`] reads it, so the two cannot drift.
pub const REACT_ROUTER_SERVER_SINK: &str = "imports a React Router .server module";

const REACT_ROUTER_SERVER_STEP: &str = "Next: move the call into loader, action, middleware \
    or headers, which React Router keeps out of the client build. Or move the code that the \
    client needs out of the .server module. If the import is type-only, mark it as a false \
    positive.";

const SERVER_ONLY_STEP: &str = "Next: check whether this server-only code is meant to run on \
    the client. If it is pulled in only through next/dynamic(..., { ssr: false }), type-only, \
    or removed at build time, mark it as a false positive. fallow already excludes a \"use \
    server\" module whose value exports are all async functions. If the sink is a \"use \
    server\" module, move its non-action export (a top-level const, a re-export, or a default \
    value) to a server-only file.";

const DIRECT_READ_STEP: &str = "Next: check whether the env read on this line runs in code \
    that ships to the browser. If only server code runs it, or a build step removes it, mark \
    it as a false positive.";

const IMPORT_STEP: &str = "Next: check whether this import can ship a secret to the browser. \
    If it is type-only, server-only, or removed at build time, mark it as a false positive.";

/// The `Next:` step of a `client-server-leak` candidate.
///
/// A `server-only-import` candidate gets React Router advice when its sink is a
/// React Router `.server` module, and Next.js advice otherwise.
///
/// A candidate whose trace is one `secret-source` hop reads the secret in the
/// client file itself, so the step talks about the env read. A longer trace
/// reaches the secret through an import, so the step talks about the import.
#[must_use]
pub fn client_server_leak_next_step(finding: &SecurityFinding) -> &'static str {
    if finding.category.as_deref() == Some(SERVER_ONLY_CATEGORY) {
        if finding.evidence.contains(REACT_ROUTER_SERVER_SINK) {
            return REACT_ROUTER_SERVER_STEP;
        }
        return SERVER_ONLY_STEP;
    }
    let direct_read = matches!(
        finding.trace.as_slice(),
        [hop] if hop.role == TraceHopRole::SecretSource
    );
    if direct_read {
        DIRECT_READ_STEP
    } else {
        IMPORT_STEP
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::results::{
        SecurityCandidate, SecurityCandidateBoundary, SecurityCandidateSink, SecurityFindingKind,
        SecuritySeverity, TraceHop,
    };

    fn server_only_finding(evidence: &str) -> SecurityFinding {
        let path = PathBuf::from("app/routes/home.tsx");
        SecurityFinding {
            finding_id: "id".to_string(),
            stable_finding_id: None,
            stable_id_input: None,
            kind: SecurityFindingKind::ClientServerLeak,
            category: Some(SERVER_ONLY_CATEGORY.to_string()),
            cwe: None,
            path: path.clone(),
            line: 1,
            col: 0,
            evidence: evidence.to_string(),
            source_backed: false,
            source_read: None,
            severity: SecuritySeverity::High,
            trace: vec![TraceHop::new(path.clone(), 1, 0, TraceHopRole::Sink)],
            actions: vec![],
            dead_code: None,
            reachability: None,
            candidate: SecurityCandidate {
                source_kind: None,
                sink: SecurityCandidateSink {
                    path,
                    line: 1,
                    col: 0,
                    category: Some(SERVER_ONLY_CATEGORY.to_string()),
                    cwe: None,
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
    fn a_react_router_server_sink_gets_react_router_advice() {
        let evidence = format!(
            "The client code of this React Router route module {REACT_ROUTER_SERVER_SINK}."
        );
        let step = client_server_leak_next_step(&server_only_finding(&evidence));
        assert!(
            step.contains("loader, action, middleware or headers"),
            "{step}"
        );
        assert!(!step.contains("next/dynamic"), "{step}");
        assert!(!step.contains("use server"), "{step}");
    }

    #[test]
    fn a_server_only_package_sink_keeps_the_next_js_advice() {
        let finding = server_only_finding(
            "This \"use client\" file transitively imports a SERVER-ONLY module (it imports \
             server-only code such as server-only, next/headers, next/server, or node:fs / \
             node:child_process; see the sink hop in the trace).",
        );
        let step = client_server_leak_next_step(&finding);
        assert!(step.contains("next/dynamic"), "{step}");
        assert!(!step.contains("loader"), "{step}");
    }
}
