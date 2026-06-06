//! Integration tests for the `client-server-leak` security candidate rule.
//!
//! Fixture `tests/fixtures/security-client-server-leak/` models a Next.js App
//! Router shape: `"use client"` boundary files, server modules reading
//! `process.env` and `import.meta.env`, public-prefix env reads, a multi-hop
//! barrel chain, a no-directive control, package self-reference exports, and a
//! dynamic-import blind-spot file.

use fallow_config::Severity;
use fallow_core::results::{AnalysisResults, SecurityFindingKind, TraceHopRole};

use super::common::{create_config, create_config_with_rules, fixture_path};

fn analyze_with_security() -> AnalysisResults {
    let root = fixture_path("security-client-server-leak");
    let config = create_config_with_rules(root, |rules| {
        rules.security_client_server_leak = Severity::Warn;
    });
    fallow_core::analyze(&config).expect("analysis should succeed")
}

/// Returns true when any finding is anchored on a file whose path ends with `suffix`.
fn anchored_on(results: &AnalysisResults, suffix: &str) -> bool {
    results.security_findings.iter().any(|f| {
        f.path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with(suffix)
    })
}

#[test]
fn single_hop_leak_is_reported_with_named_secret() {
    // Criterion 1: a "use client" file importing a process.env-reading module
    // reports a client-server-leak whose evidence names the secret.
    let results = analyze_with_security();
    let finding = results
        .security_findings
        .iter()
        .find(|f| {
            f.path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with("src/client.tsx")
        })
        .expect("client.tsx should be flagged");
    assert!(matches!(
        finding.kind,
        SecurityFindingKind::ClientServerLeak
    ));
    assert!(
        finding.evidence.contains("DATABASE_URL"),
        "evidence should name the secret var: {}",
        finding.evidence
    );
    // Trace ends at the secret source.
    let last = finding.trace.last().expect("trace must have hops");
    assert!(matches!(last.role, TraceHopRole::SecretSource));
    assert!(
        last.path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("src/server.ts")
    );
}

#[test]
fn no_use_client_directive_is_not_scanned() {
    // Criterion 2: plain.tsx imports the same server module but has no
    // "use client" directive, so it is never flagged.
    let results = analyze_with_security();
    assert!(
        !anchored_on(&results, "src/plain.tsx"),
        "a file without \"use client\" must not be flagged"
    );
}

#[test]
fn public_prefix_env_read_is_not_a_secret() {
    // Criterion 3: public-prefix reads are public-by-convention and must not
    // mark a module as a secret source, so public clients are not flagged.
    let results = analyze_with_security();
    assert!(
        !anchored_on(&results, "src/public-client.tsx"),
        "a client file reaching only a NEXT_PUBLIC_ read must not be flagged"
    );
    assert!(
        !anchored_on(&results, "src/vite-public-client.tsx"),
        "a client file reaching only a VITE_ read must not be flagged"
    );
}

#[test]
fn multi_hop_leak_through_barrel_lists_every_hop() {
    // Criterion 4: client2 -> barrel -> secret2 is detected and the trace lists
    // every hop in order.
    let results = analyze_with_security();
    let finding = results
        .security_findings
        .iter()
        .find(|f| {
            f.path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with("src/client2.tsx")
        })
        .expect("client2.tsx should be flagged");
    assert!(
        finding.evidence.contains("SESSION_SECRET"),
        "evidence should name the secret: {}",
        finding.evidence
    );
    let hops: Vec<String> = finding
        .trace
        .iter()
        .map(|h| h.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        hops.len() >= 3,
        "multi-hop trace should list every hop: {hops:?}"
    );
    assert!(hops[0].ends_with("src/client2.tsx"));
    assert!(hops.iter().any(|h| h.ends_with("src/barrel.ts")));
    assert!(hops.last().unwrap().ends_with("src/secret2.ts"));
    assert!(matches!(
        finding.trace[0].role,
        TraceHopRole::ClientBoundary
    ));
}

#[test]
fn dynamic_import_blind_spot_is_counted_in_band() {
    // Criterion 10: a client file with a dynamic import the BFS cannot follow
    // bumps the in-band unresolved-edge counter rather than being silently
    // treated as clean.
    let results = analyze_with_security();
    assert!(
        results.security_unresolved_edge_files >= 1,
        "dyn-client.tsx's dynamic import should count as an unresolved edge"
    );
}

#[test]
fn direct_secret_read_in_client_file_is_reported() {
    // A "use client" file that itself reads a non-public secret (no import hop)
    // is the most direct leak and is flagged with a single-hop trace.
    let results = analyze_with_security();
    let finding = results
        .security_findings
        .iter()
        .find(|f| {
            f.path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with("src/direct-client.tsx")
        })
        .expect("direct-client.tsx should be flagged");
    assert!(finding.evidence.contains("STRIPE_SECRET_KEY"));
    assert_eq!(finding.trace.len(), 1);
    assert!(matches!(finding.trace[0].role, TraceHopRole::SecretSource));
}

#[test]
fn direct_import_meta_env_secret_read_in_client_file_is_reported() {
    let results = analyze_with_security();
    let finding = results
        .security_findings
        .iter()
        .find(|f| {
            f.path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with("src/vite-direct-client.tsx")
        })
        .expect("vite-direct-client.tsx should be flagged");
    assert!(
        finding
            .evidence
            .contains("import.meta.env.DIRECT_SECRET_KEY")
    );
    assert_eq!(finding.trace.len(), 1);
    assert!(matches!(finding.trace[0].role, TraceHopRole::SecretSource));
}

#[test]
fn transitive_import_meta_env_secret_read_is_reported() {
    let results = analyze_with_security();
    let finding = results
        .security_findings
        .iter()
        .find(|f| {
            f.path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with("src/vite-client.tsx")
        })
        .expect("vite-client.tsx should be flagged");
    assert!(finding.evidence.contains("import.meta.env.SECRET_KEY"));
    let last = finding.trace.last().expect("trace must have hops");
    assert!(matches!(last.role, TraceHopRole::SecretSource));
    assert!(
        last.path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("src/vite-config.ts")
    );
}

#[test]
fn package_self_reference_import_condition_does_not_visit_server_entry() {
    let results = analyze_with_security();
    assert!(
        !anchored_on(&results, "src/conditional-client.tsx"),
        "client import through the package import condition must not report the node entry"
    );
    for finding in &results.security_findings {
        assert!(
            finding.trace.iter().all(|hop| {
                !hop.path
                    .to_string_lossy()
                    .replace('\\', "/")
                    .ends_with("src/export-server.ts")
            }),
            "server-only export entry must not appear in client trace: {:?}",
            finding.trace
        );
    }
}

#[test]
fn file_level_suppression_opts_out() {
    // suppressed-client.tsx leaks but carries a file-level
    // `// fallow-ignore-file security-client-server-leak`, so it is not flagged.
    let results = analyze_with_security();
    assert!(
        !anchored_on(&results, "src/suppressed-client.tsx"),
        "a file-level-suppressed client file must not be flagged"
    );
}

#[test]
fn every_finding_carries_a_suppress_action() {
    // Machine-contract: each finding has an actions array with a file-level
    // suppress hint (auto_fixable: false).
    let results = analyze_with_security();
    assert!(!results.security_findings.is_empty());
    for f in &results.security_findings {
        assert!(
            !f.actions.is_empty(),
            "finding must carry actions: {:?}",
            f.path
        );
    }
}

#[test]
fn exactly_five_leaks_reported() {
    // Genuine leaks: client.tsx (single-hop), client2.tsx (multi-hop),
    // direct-client.tsx (direct read), vite-client.tsx (transitive
    // import.meta.env), and vite-direct-client.tsx (direct import.meta.env).
    // public-client / vite-public-client / plain / dyn-client /
    // conditional-client / suppressed-client must NOT produce findings.
    let results = analyze_with_security();
    assert_eq!(
        results.security_findings.len(),
        5,
        "expected exactly five client-server-leak findings, got: {:?}",
        results
            .security_findings
            .iter()
            .map(|f| f.path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    );
}

#[test]
fn default_off_emits_no_security_findings() {
    // Criterion 5 (core half): with the rule at its default `off`, bare
    // `fallow_core::analyze` (the engine behind bare `fallow` and `audit`)
    // produces zero security findings. The field is also `#[serde(skip)]`, so
    // it never reaches JSON output regardless.
    let root = fixture_path("security-client-server-leak");
    let config = create_config(root);
    assert_eq!(config.rules.security_client_server_leak, Severity::Off);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        results.security_findings.is_empty(),
        "default-off rule must not populate security_findings"
    );
    assert_eq!(results.security_unresolved_edge_files, 0);
}
