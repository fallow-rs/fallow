//! Tests of the strict `fallow security --gate new` filter
//! (`retain_gate_new`).

use std::path::{Path, PathBuf};

use fallow_types::results::*;

use super::tests::{build_diff, sample_security_finding, unresolved_callee_diagnostic};
use super::{filter_results_by_diff, retain_gate_new};

#[test]
fn gate_keeps_new_sink_anchor_on_added_line() {
    let diff = build_diff(
        "diff --git a/src/render.ts b/src/render.ts\n\
         --- a/src/render.ts\n\
         +++ b/src/render.ts\n\
         @@ -11,0 +12,1 @@\n\
         +el.innerHTML = req.query.html;\n",
    );
    let root = Path::new("/project");
    let mut results = AnalysisResults::default();
    results.security_findings.push(SecurityFinding {
        kind: SecurityFindingKind::TaintedSink,
        category: Some("dangerous-html".into()),
        cwe: Some(79),
        path: PathBuf::from("/project/src/render.ts"),
        line: 12,
        col: 0,
        ..sample_security_finding()
    });

    retain_gate_new(&mut results, &diff, root);

    assert_eq!(results.security_findings.len(), 1);
}

#[test]
fn gate_new_scopes_unresolved_callee_diagnostics_to_added_lines() {
    let diff = build_diff(
        "diff --git a/src/a.ts b/src/a.ts\n\
         --- a/src/a.ts\n\
         +++ b/src/a.ts\n\
         @@ -2,1 +2,2 @@\n\
          \x20ctx\n\
         +factory()(req.body.name);\n",
    );
    let root = Path::new("/project");
    let mut results = AnalysisResults::default();
    results
        .security_unresolved_callee_diagnostics
        .push(unresolved_callee_diagnostic("/project/src/a.ts", 3));
    results
        .security_unresolved_callee_diagnostics
        .push(unresolved_callee_diagnostic("/project/src/a.ts", 20));

    retain_gate_new(&mut results, &diff, root);

    assert_eq!(results.security_unresolved_callee_diagnostics.len(), 1);
    assert_eq!(results.security_unresolved_callee_diagnostics[0].line, 3);
}

fn secret_leak_with_reads(reads: &[(&str, u32)]) -> SecurityFinding {
    let secret_reads: Vec<TraceSecretRead> = reads
        .iter()
        .map(|&(name, line)| TraceSecretRead {
            name: name.to_owned(),
            line,
            col: 0,
        })
        .collect();
    let first = secret_reads.first().map_or(1, |read| read.line);
    SecurityFinding {
        kind: SecurityFindingKind::ClientServerLeak,
        category: Some("client-server-leak".into()),
        cwe: None,
        path: PathBuf::from("/project/src/client.tsx"),
        line: 2,
        col: 0,
        trace: vec![
            TraceHop::new(
                PathBuf::from("/project/src/client.tsx"),
                2,
                0,
                TraceHopRole::ClientBoundary,
            ),
            TraceHop {
                path: PathBuf::from("/project/src/shared.ts"),
                line: first,
                col: 0,
                role: TraceHopRole::SecretSource,
                secret_reads,
            },
        ],
        ..sample_security_finding()
    }
}

#[test]
fn gate_keeps_secret_read_added_to_existing_module() {
    let diff = build_diff(
        "diff --git a/src/shared.ts b/src/shared.ts\n\
         --- a/src/shared.ts\n\
         +++ b/src/shared.ts\n\
         @@ -1,1 +1,2 @@\n\
          \x20export const name = 'app';\n\
         +export const key = process.env.API_SECRET_KEY;\n",
    );
    let root = Path::new("/project");
    let mut results = AnalysisResults::default();
    results
        .security_findings
        .push(secret_leak_with_reads(&[("process.env.API_SECRET_KEY", 2)]));

    retain_gate_new(&mut results, &diff, root);

    assert_eq!(results.security_findings.len(), 1);
}

#[test]
fn gate_keeps_second_secret_read_added_below_an_old_read() {
    let diff = build_diff(
        "diff --git a/src/shared.ts b/src/shared.ts\n\
         --- a/src/shared.ts\n\
         +++ b/src/shared.ts\n\
         @@ -9,0 +10,1 @@\n\
         +export const b = process.env.SECRET_B;\n",
    );
    let root = Path::new("/project");
    let mut results = AnalysisResults::default();
    results.security_findings.push(secret_leak_with_reads(&[
        ("process.env.SECRET_A", 3),
        ("process.env.SECRET_B", 10),
    ]));

    retain_gate_new(&mut results, &diff, root);

    assert_eq!(results.security_findings.len(), 1);
}

#[test]
fn gate_drops_old_secret_read_when_another_line_changes() {
    let diff = build_diff(
        "diff --git a/src/shared.ts b/src/shared.ts\n\
         --- a/src/shared.ts\n\
         +++ b/src/shared.ts\n\
         @@ -9,0 +10,1 @@\n\
         +export const retries = 3;\n",
    );
    let root = Path::new("/project");
    let mut results = AnalysisResults::default();
    results
        .security_findings
        .push(secret_leak_with_reads(&[("process.env.SECRET_A", 3)]));

    retain_gate_new(&mut results, &diff, root);

    assert!(results.security_findings.is_empty());
}

#[test]
fn gate_drops_secret_source_file_touched_when_anchor_unchanged() {
    // The SAME shape the advisory `filter_by_diff` KEEPS (a SecretSource hop
    // whose FILE is touched, anchor line unchanged). The gate MUST DROP it:
    // editing a file that merely reads a secret is not a new sink, and the
    // forbidden "a sink in a changed file fails the gate" false positive.
    // The secret read stays on line 1; the change adds another line.
    let diff = build_diff(
        "diff --git a/src/server.ts b/src/server.ts\n\
         --- a/src/server.ts\n\
         +++ b/src/server.ts\n\
         @@ -8,1 +8,2 @@\n\
          const keep = true;\n\
         +export const retries = 3;\n",
    );
    let root = Path::new("/project");
    let mut results = AnalysisResults::default();
    results.security_findings.push(SecurityFinding {
        kind: SecurityFindingKind::ClientServerLeak,
        category: None,
        cwe: None,
        path: PathBuf::from("/project/src/client.tsx"),
        line: 2,
        col: 0,
        trace: vec![
            TraceHop::new(
                PathBuf::from("/project/src/client.tsx"),
                2,
                0,
                TraceHopRole::ClientBoundary,
            ),
            TraceHop::new(
                PathBuf::from("/project/src/server.ts"),
                1,
                0,
                TraceHopRole::SecretSource,
            ),
        ],
        ..sample_security_finding()
    });

    // Advisory keeps it; the gate drops it.
    let mut advisory = results.clone();
    filter_results_by_diff(&mut advisory, &diff, root);
    assert_eq!(advisory.security_findings.len(), 1);

    retain_gate_new(&mut results, &diff, root);
    assert_eq!(results.security_findings.len(), 0);
}

#[test]
fn gate_keeps_untrusted_source_hop_on_added_line() {
    // A pre-existing sink whose anchor is NOT added, but the PR wires a new
    // UntrustedSource into it on an added line: genuine new exposure, kept.
    let diff = build_diff(
        "diff --git a/src/route.ts b/src/route.ts\n\
         --- a/src/route.ts\n\
         +++ b/src/route.ts\n\
         @@ -3,0 +3,1 @@\n\
         +import { run } from './runner';\n",
    );
    let root = Path::new("/project");
    let mut results = AnalysisResults::default();
    results.security_findings.push(SecurityFinding {
        kind: SecurityFindingKind::TaintedSink,
        category: Some("command-injection".into()),
        cwe: Some(78),
        path: PathBuf::from("/project/src/runner.ts"),
        line: 10,
        col: 0,
        reachability: Some(SecurityReachability {
            reachable_from_entry: false,
            reachable_from_untrusted_source: true,
            taint_confidence: None,
            untrusted_source_hop_count: Some(1),
            untrusted_source_trace: vec![TraceHop::new(
                PathBuf::from("/project/src/route.ts"),
                3,
                0,
                TraceHopRole::UntrustedSource,
            )],
            blast_radius: 0,
            crosses_boundary: false,
        }),
        ..sample_security_finding()
    });

    retain_gate_new(&mut results, &diff, root);

    assert_eq!(results.security_findings.len(), 1);
}

#[test]
fn gate_drops_intermediate_hop_pass_through() {
    // Only an Intermediate (pass-through) hop sits on an added line; the sink
    // anchor and any UntrustedSource/Sink hop are unchanged. Editing a transit
    // module is not new exposure, so the gate drops it.
    let diff = build_diff(
        "diff --git a/src/transit.ts b/src/transit.ts\n\
         --- a/src/transit.ts\n\
         +++ b/src/transit.ts\n\
         @@ -4,0 +4,1 @@\n\
         +const passthrough = next;\n",
    );
    let root = Path::new("/project");
    let mut results = AnalysisResults::default();
    results.security_findings.push(SecurityFinding {
        kind: SecurityFindingKind::TaintedSink,
        category: Some("dangerous-html".into()),
        cwe: Some(79),
        path: PathBuf::from("/project/src/sink.ts"),
        line: 20,
        col: 0,
        trace: vec![TraceHop::new(
            PathBuf::from("/project/src/transit.ts"),
            4,
            0,
            TraceHopRole::Intermediate,
        )],
        ..sample_security_finding()
    });

    retain_gate_new(&mut results, &diff, root);

    assert!(results.security_findings.is_empty());
}
