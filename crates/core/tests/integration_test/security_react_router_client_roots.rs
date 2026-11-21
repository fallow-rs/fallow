//! Integration tests for the React Router Framework Mode client roots of the
//! `client-server-leak` rule.
//!
//! Fixture `tests/fixtures/security-react-router-client-roots/` has no
//! `"use client"` file. The route modules, the `.client` modules and the
//! `.server` modules give the rule its roots and sinks.

use fallow_config::{Severity, WorkspaceDiagnostic};
use fallow_core::results::{AnalysisResults, SecurityFinding, TraceHopRole};

use super::common::{create_config_with_rules, fixture_path};

const SECRET_LEAK: &str = "client-server-leak";
const SERVER_ONLY: &str = "server-only-import";

fn analyze_fixture() -> (AnalysisResults, Vec<String>) {
    let root = fixture_path("security-react-router-client-roots");
    let config = create_config_with_rules(root, |rules| {
        rules.security_client_server_leak = Severity::Warn;
    });
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let ids = fallow_config::workspace_diagnostics_for(&config.root)
        .iter()
        .map(|diagnostic: &WorkspaceDiagnostic| diagnostic.kind.id().to_string())
        .collect();
    (results, ids)
}

fn normalized(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn findings_on<'a>(
    results: &'a AnalysisResults,
    suffix: &str,
    category: &str,
) -> Vec<&'a SecurityFinding> {
    results
        .security_findings
        .iter()
        .filter(|f| {
            normalized(&f.path).ends_with(suffix) && f.category.as_deref() == Some(category)
        })
        .collect()
}

fn sink_path(finding: &SecurityFinding) -> String {
    let last = finding.trace.last().expect("trace must have hops");
    normalized(&last.path)
}

#[test]
fn a_route_component_that_imports_a_secret_reader_is_reported() {
    let (results, _) = analyze_fixture();
    let findings = findings_on(&results, "app/routes/home.tsx", SECRET_LEAK);
    assert_eq!(findings.len(), 1, "{:#?}", results.security_findings);
    assert!(sink_path(findings[0]).ends_with("app/lib/billing.ts"));
    assert!(matches!(
        findings[0].trace.last().map(|hop| hop.role),
        Some(TraceHopRole::SecretSource)
    ));
    assert!(findings[0].evidence.contains("STRIPE_SECRET_KEY"));
}

#[test]
fn a_route_component_that_imports_node_fs_is_reported_as_server_only() {
    let (results, _) = analyze_fixture();
    let findings = findings_on(&results, "app/routes/home.tsx", SERVER_ONLY);
    assert_eq!(findings.len(), 1, "{:#?}", results.security_findings);
    assert!(sink_path(findings[0]).ends_with("app/lib/files.ts"));
}

#[test]
fn an_import_that_only_the_loader_uses_is_not_followed() {
    let (results, _) = analyze_fixture();
    assert!(
        !results
            .security_findings
            .iter()
            .any(|f| normalized(&f.path).ends_with("app/routes/admin.tsx")),
        "React Router removes the loader and its imports from the client build: {:#?}",
        results.security_findings
    );
}

#[test]
fn the_root_route_follows_only_its_client_exports() {
    let (results, _) = analyze_fixture();
    let findings = findings_on(&results, "app/root.tsx", SECRET_LEAK);
    assert_eq!(findings.len(), 1, "{:#?}", results.security_findings);
    assert!(sink_path(findings[0]).ends_with("app/lib/analytics.ts"));
    assert!(
        !results
            .security_findings
            .iter()
            .any(|f| f.evidence.contains("SESSION_SECRET")),
        "only the root loader reads the session secret: {:#?}",
        results.security_findings
    );
    assert!(
        findings_on(&results, "app/root.tsx", SERVER_ONLY).is_empty(),
        "only the root loader reads files: {:#?}",
        results.security_findings
    );
}

#[test]
fn a_server_module_that_a_client_export_uses_is_a_server_only_sink() {
    let (results, _) = analyze_fixture();
    let findings = findings_on(&results, "app/routes/session.tsx", SERVER_ONLY);
    assert_eq!(findings.len(), 1, "{:#?}", results.security_findings);
    assert!(sink_path(findings[0]).ends_with("app/lib/auth.server.ts"));
    assert!(
        findings[0]
            .evidence
            .starts_with("The client code of this React Router route module"),
        "{}",
        findings[0].evidence
    );
    assert!(
        findings[0]
            .evidence
            .contains("imports a React Router .server module"),
        "{}",
        findings[0].evidence
    );
    assert!(
        findings[0]
            .evidence
            .contains("React Router fails the client build"),
        "{}",
        findings[0].evidence
    );
    for next_js_advice in ["next/dynamic", "Server Action", "use server"] {
        assert!(
            !findings[0].evidence.contains(next_js_advice),
            "{}",
            findings[0].evidence
        );
    }
    let step = fallow_types::security_next_step::client_server_leak_next_step(findings[0]);
    assert!(
        step.contains("loader, action, middleware or headers"),
        "{step}"
    );
}

#[test]
fn a_node_fs_sink_of_a_route_module_keeps_the_server_only_advice() {
    let (results, _) = analyze_fixture();
    let findings = findings_on(&results, "app/routes/home.tsx", SERVER_ONLY);
    assert_eq!(findings.len(), 1, "{:#?}", results.security_findings);
    assert!(
        !findings[0]
            .evidence
            .contains(fallow_types::security_next_step::REACT_ROUTER_SERVER_SINK),
        "{}",
        findings[0].evidence
    );
    let step = fallow_types::security_next_step::client_server_leak_next_step(findings[0]);
    assert!(step.contains("next/dynamic"), "{step}");
}

#[test]
fn a_dot_client_file_is_a_client_root() {
    let (results, _) = analyze_fixture();
    let findings = findings_on(&results, "app/lib/tracker.client.ts", SECRET_LEAK);
    assert_eq!(findings.len(), 1, "{:#?}", results.security_findings);
    assert!(sink_path(findings[0]).ends_with("app/lib/signing.ts"));
}

#[test]
fn a_file_under_a_client_directory_that_imports_a_server_directory_is_reported() {
    let (results, _) = analyze_fixture();
    let findings = findings_on(&results, "app/.client/storage.ts", SERVER_ONLY);
    assert_eq!(findings.len(), 1, "{:#?}", results.security_findings);
    assert!(sink_path(findings[0]).ends_with("app/.server/mailer.ts"));
}

#[test]
fn plugin_client_roots_suppress_the_no_client_roots_diagnostic() {
    let (_, ids) = analyze_fixture();
    assert!(
        !ids.iter()
            .any(|id| id == "client-server-leak-no-client-roots"),
        "the React Router plugin supplies client roots, got {ids:?}"
    );
}

fn has_finding_on(results: &AnalysisResults, suffix: &str) -> bool {
    results
        .security_findings
        .iter()
        .any(|f| normalized(&f.path).ends_with(suffix))
}

#[test]
fn a_re_export_of_a_server_only_export_is_not_followed() {
    let (results, _) = analyze_fixture();
    assert!(
        !has_finding_on(&results, "app/routes/notes.tsx"),
        "React Router removes `export {{ x as action }} from`: {:#?}",
        results.security_findings
    );
}

#[test]
fn a_route_module_that_the_cone_reaches_drops_its_server_only_exports() {
    let (results, _) = analyze_fixture();
    assert!(
        !has_finding_on(&results, "app/routes/settings.tsx"),
        "only the action of theme.tsx uses theme.server.ts: {:#?}",
        results.security_findings
    );
}

#[test]
fn a_test_file_under_the_route_directory_is_not_a_root() {
    let (results, _) = analyze_fixture();
    assert!(
        !has_finding_on(&results, "app/routes/home.test.tsx"),
        "{:#?}",
        results.security_findings
    );
}

#[test]
fn a_reached_route_module_counts_only_its_client_code_as_a_sink() {
    let (results, _) = analyze_fixture();
    for route in ["app/routes/settings.tsx", "app/routes/theme.tsx"] {
        assert!(
            !has_finding_on(&results, route),
            "only the action of theme.tsx reads node:fs and THEME_SECRET: {:#?}",
            results.security_findings
        );
    }
}

#[test]
fn an_env_read_that_only_the_loader_makes_is_not_a_finding() {
    let (results, _) = analyze_fixture();
    assert!(
        !results
            .security_findings
            .iter()
            .any(|f| f.evidence.contains("ADMIN_SECRET")
                || f.evidence.contains("BANNER_LOADER_SECRET")),
        "{:#?}",
        results.security_findings
    );
}

#[test]
fn an_env_read_in_the_route_component_is_a_direct_finding() {
    let (results, _) = analyze_fixture();
    let findings = findings_on(&results, "app/routes/banner.tsx", SECRET_LEAK);
    assert_eq!(findings.len(), 1, "{:#?}", results.security_findings);
    assert!(findings[0].evidence.contains("process.env.BANNER_SECRET"));
    assert_eq!(findings[0].trace.len(), 1);
}

#[test]
fn a_helper_next_to_a_folder_route_is_not_a_root() {
    let (results, _) = analyze_fixture();
    for path in [
        "app/routes/api.users/db.ts",
        "app/routes/api.users/route.tsx",
    ] {
        assert!(
            !has_finding_on(&results, path),
            "only the loader of route.tsx uses db.ts: {:#?}",
            results.security_findings
        );
    }
}

#[test]
fn a_nested_route_file_with_a_route_export_is_a_root() {
    let (results, _) = analyze_fixture();
    let findings = findings_on(&results, "app/routes/_auth/login.tsx", SECRET_LEAK);
    assert_eq!(findings.len(), 1, "{:#?}", results.security_findings);
    assert!(sink_path(findings[0]).ends_with("app/lib/billing.ts"));
}

fn analyze_workspace_fixture() -> AnalysisResults {
    let root = fixture_path("security-react-router-workspace-markers");
    let config = create_config_with_rules(root, |rules| {
        rules.security_client_server_leak = Severity::Warn;
    });
    fallow_core::analyze(&config).expect("analysis should succeed")
}

#[test]
fn client_markers_apply_in_the_react_router_package() {
    let results = analyze_workspace_fixture();
    let findings = findings_on(&results, "packages/web/app/tracker.client.ts", SECRET_LEAK);
    assert_eq!(findings.len(), 1, "{:#?}", results.security_findings);
}

#[test]
fn markers_do_not_apply_in_a_package_without_react_router() {
    let results = analyze_workspace_fixture();
    assert!(
        !has_finding_on(&results, "packages/other/src/store.client.ts"),
        "{:#?}",
        results.security_findings
    );
    assert!(
        findings_on(&results, "packages/other/src/panel.ts", SERVER_ONLY).is_empty(),
        "data.server.ts is not a sink outside the React Router package: {:#?}",
        results.security_findings
    );
}
