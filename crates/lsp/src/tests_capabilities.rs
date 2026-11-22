//! Server capability and analysis-complete notification tests.

use super::*;

#[test]
fn server_capabilities_advertise_pull_diagnostics() {
    let caps = build_server_capabilities(true);
    let provider = caps.diagnostic_provider.expect(
        "diagnostic_provider must be advertised for clients that can refresh pulled diagnostics",
    );
    match provider {
        DiagnosticServerCapabilities::Options(opts) => {
            assert_eq!(opts.identifier.as_deref(), Some("fallow"));
            assert!(
                opts.inter_file_dependencies,
                "fallow diagnostics span files; clients must re-pull related files on changes"
            );
            assert!(
                !opts.workspace_diagnostics,
                "no workspace/diagnostic handler is registered"
            );
        }
        DiagnosticServerCapabilities::RegistrationOptions(_) => {
            panic!("dynamic registration not supported");
        }
    }
}

#[test]
fn server_capabilities_omit_pull_diagnostics_when_not_refreshable() {
    let caps = build_server_capabilities(false);
    assert!(caps.diagnostic_provider.is_none());
    assert!(caps.text_document_sync.is_some());
    assert!(caps.code_action_provider.is_some());
    assert!(caps.code_lens_provider.is_some());
    assert!(caps.hover_provider.is_some());
}

#[test]
fn analysis_complete_params_collapses_boundary_subresults() {
    let mut results = AnalysisResults {
        boundary_violations: vec![BoundaryViolationFinding::with_actions(BoundaryViolation {
            from_path: "/ui.ts".into(),
            to_path: "/data.ts".into(),
            from_zone: "ui".to_string(),
            to_zone: "data".to_string(),
            import_specifier: "../data".to_string(),
            line: 1,
            col: 0,
            via_path: None,
            reason: fallow_types::results::BoundaryViolationReason::DisallowedZone,
            public_files: vec![],
        })],
        boundary_coverage_violations: vec![
            fallow_api::editor_results::BoundaryCoverageViolationFinding::with_actions(
                fallow_api::editor_results::BoundaryCoverageViolation {
                    path: "/unzoned.ts".into(),
                    line: 2,
                    col: 0,
                },
            ),
        ],
        boundary_call_violations: vec![
            fallow_api::editor_results::BoundaryCallViolationFinding::with_actions(
                fallow_api::editor_results::BoundaryCallViolation {
                    path: "/domain.ts".into(),
                    line: 3,
                    col: 0,
                    zone: "domain".to_string(),
                    callee: "console.log".to_string(),
                    pattern: "console.*".to_string(),
                },
            ),
        ],
        ..AnalysisResults::default()
    };

    let params = analysis_complete_params_for_test(&results, &DuplicationReport::default());

    assert_eq!(params.boundary_violations, 3);
    assert_eq!(params.total_issues, 3);

    results.boundary_call_violations.clear();
    let params = analysis_complete_params_for_test(&results, &DuplicationReport::default());
    assert_eq!(params.boundary_violations, 2);
}

#[test]
fn analysis_complete_changed_since_scope_is_additive_and_structured() {
    let results = AnalysisResults::default();
    let duplication = DuplicationReport::default();
    let legacy = analysis_complete_params_for_test(&results, &duplication);
    let legacy_json = serde_json::to_value(&legacy).expect("legacy completion serializes");
    assert!(
        legacy_json.get("changedSinceScope").is_none(),
        "omitted scope status must preserve the legacy payload"
    );
    assert!(legacy_json.get("packageBaselines").is_none());
    let _: protocol::AnalysisCompleteParams =
        serde_json::from_value(legacy_json).expect("legacy completion remains accepted");

    let packages = vec![fallow_api::PackageBaselineStatus {
        workspace_root: "packages/web".to_owned(),
        reference: "main".to_owned(),
    }];
    let package_params = protocol::analysis_complete_params(
        protocol::AnalysisCompleteInput::new(&results, &duplication)
            .with_package_baselines(&packages),
    );
    let package_json = serde_json::to_value(package_params).expect("package status serializes");
    assert_eq!(
        package_json["packageBaselines"][0]["workspace_root"],
        "packages/web"
    );
    assert_eq!(package_json["packageBaselines"][0]["reference"], "main");

    let applied_status = protocol::ChangedSinceScopeStatus {
        requested_ref: "origin/main".to_string(),
        state: protocol::ChangedSinceScopeState::Applied,
        reason: None,
    };
    let applied = protocol::analysis_complete_params_with_scope_for_test(
        &results,
        &duplication,
        Some(&applied_status),
    );
    let applied_json = serde_json::to_value(applied).expect("applied completion serializes");
    assert_eq!(
        applied_json["changedSinceScope"]["requestedRef"],
        "origin/main"
    );
    assert_eq!(applied_json["changedSinceScope"]["state"], "applied");
    assert!(applied_json["changedSinceScope"].get("reason").is_none());

    let dropped_status = protocol::ChangedSinceScopeStatus {
        requested_ref: "missing-ref".to_string(),
        state: protocol::ChangedSinceScopeState::Dropped,
        reason: Some("git ref was not found".to_string()),
    };
    let dropped = protocol::analysis_complete_params_with_scope_for_test(
        &results,
        &duplication,
        Some(&dropped_status),
    );
    let dropped_json = serde_json::to_value(dropped).expect("dropped completion serializes");
    assert_eq!(dropped_json["changedSinceScope"]["state"], "dropped");
    assert_eq!(
        dropped_json["changedSinceScope"]["reason"],
        "git ref was not found"
    );
}

#[test]
fn server_capabilities_keep_existing_providers() {
    let caps = build_server_capabilities(true);
    assert!(caps.text_document_sync.is_some());
    assert!(caps.code_action_provider.is_some());
    assert!(caps.code_lens_provider.is_some());
    assert!(caps.hover_provider.is_some());
}

#[test]
fn default_client_capabilities_do_not_support_workspace_diagnostic_refresh() {
    assert!(!client_supports_workspace_diagnostic_refresh(
        &ClientCapabilities::default()
    ));
}

#[test]
fn client_capabilities_support_workspace_diagnostic_refresh() {
    let capabilities: ClientCapabilities = serde_json::from_value(json!({
        "workspace": {
            "diagnostics": {
                "refreshSupport": true
            }
        }
    }))
    .expect("workspace.diagnostics.refreshSupport should deserialize");

    assert!(client_supports_workspace_diagnostic_refresh(&capabilities));
}
