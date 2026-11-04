use std::path::PathBuf;

use super::*;
use fallow_config::{WorkspaceDiagnostic, WorkspaceDiagnosticKind};
use fallow_types::output_dead_code::{
    BoundaryViolationFinding, CircularDependencyFinding, DuplicateExportFinding,
    PrivateTypeLeakFinding, UnlistedDependencyFinding, UnusedClassMemberFinding,
    UnusedEnumMemberFinding, UnusedExportFinding, UnusedFileFinding, UnusedTypeFinding,
};
use fallow_types::results::{
    BoundaryViolation, CircularDependency, DuplicateExport, DuplicateLocation, ImportSite,
    PrivateTypeLeak, UnlistedDependency, UnusedExport, UnusedFile, UnusedMember,
};

#[test]
fn finding_id_filter_keeps_request_order_and_drops_duplicates() {
    let a = "dc1:unused-export:0123456789abcdef";
    let b = "dc1:unused-file:0123456789abcdef~1";

    let filter = FindingIdFilter::parse(&[b, a, b])
        .expect("valid ids")
        .expect("a filter");

    assert_eq!(filter.requested, vec![b.to_owned(), a.to_owned()]);
    assert_eq!(FindingIdFilter::parse::<&str>(&[]), Ok(None));
}

#[test]
fn normalized_text_ignores_line_endings_and_trailing_newlines() {
    assert_eq!(normalized_text(b"dist\r\nbuild\r\n"), "dist\nbuild");
    assert_eq!(normalized_text(b"dist\nbuild\n\n"), "dist\nbuild");
    assert_eq!(normalized_text(b"dist\nbuild"), "dist\nbuild");
    assert_ne!(
        normalized_text(b"dist\nbuild"),
        normalized_text(b"dist\nbuilt")
    );
}

#[test]
fn a_crlf_checkout_has_the_same_fingerprint_as_an_lf_checkout() {
    let write_project = |line_end: &str| {
        let dir = tempfile::tempdir().expect("project");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(
            root.join(".gitignore"),
            format!("dist{line_end}coverage{line_end}"),
        )
        .expect("gitignore");
        std::fs::write(
            root.join("package.json"),
            format!("{{{line_end}  \"name\": \"crlf\"{line_end}}}{line_end}"),
        )
        .expect("package.json");
        dir
    };
    let config_at = |root: &std::path::Path| {
        fallow_config::FallowConfig::default().resolve(
            root.to_path_buf(),
            fallow_config::OutputFormat::Json,
            1,
            true,
            true,
            None,
        )
    };
    let lf = write_project("\n");
    let crlf = write_project("\r\n");

    assert_eq!(
        analysis_fingerprint_for_version(&config_at(lf.path()), "1.0.0"),
        analysis_fingerprint_for_version(&config_at(crlf.path()), "1.0.0")
    );
}

#[test]
fn a_tsconfig_extends_target_outside_the_walk_is_an_input() {
    let dir = tempfile::tempdir().expect("project");
    let root = dir.path();
    std::fs::create_dir_all(root.join(".config")).expect("hidden dir");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{ "extends": "./.config/tsconfig.base.json" }"#,
    )
    .expect("tsconfig");
    let base = root.join(".config/tsconfig.base.json");
    std::fs::write(&base, r#"{ "compilerOptions": { "baseUrl": "." } }"#).expect("base");
    let config = fallow_config::FallowConfig::default().resolve(
        root.to_path_buf(),
        fallow_config::OutputFormat::Json,
        1,
        true,
        true,
        None,
    );
    let before = analysis_fingerprint_for_version(&config, "1.0.0");

    std::fs::write(&base, r#"{ "compilerOptions": { "baseUrl": "src" } }"#).expect("edit");

    assert_ne!(analysis_fingerprint_for_version(&config, "1.0.0"), before);
}

#[test]
fn analysis_fingerprint_depends_on_the_version_and_not_on_the_root() {
    let config_at = |root: &std::path::Path| {
        fallow_config::FallowConfig::default().resolve(
            root.to_path_buf(),
            fallow_config::OutputFormat::Json,
            1,
            true,
            true,
            None,
        )
    };
    let a = tempfile::tempdir().expect("project a");
    let b = tempfile::tempdir().expect("project b");
    let config_a = config_at(a.path());
    let config_b = config_at(b.path());

    let base = analysis_fingerprint_for_version(&config_a, "1.0.0");
    assert!(base.starts_with("af1:"), "{base}");
    assert_eq!(base, analysis_fingerprint_for_version(&config_a, "1.0.0"));
    assert_eq!(base, analysis_fingerprint_for_version(&config_b, "1.0.0"));
    assert_ne!(base, analysis_fingerprint_for_version(&config_a, "1.0.1"));
}

#[test]
fn finding_id_filter_refuses_a_malformed_id() {
    let error =
        FindingIdFilter::parse(&["dc1:unused-export:helper"]).expect_err("malformed id refused");

    assert!(error.contains("dc1:unused-export:helper"), "{error}");
}

#[test]
fn finding_id_rule_is_off_reads_rules_and_overrides() {
    let id = "dc1:unused-export:0123456789abcdef";
    let mut config = fallow_config::FallowConfig::default().resolve(
        PathBuf::from("/repo"),
        fallow_config::OutputFormat::Json,
        1,
        true,
        true,
        None,
    );
    assert!(!finding_id_rule_is_off(id, &config));

    config.rules.unused_exports = Severity::Off;
    assert!(finding_id_rule_is_off(id, &config));
    assert!(!finding_id_rule_is_off(
        "dc1:unused-file:0123456789abcdef",
        &config
    ));

    let with_override = serde_json::from_str::<fallow_config::FallowConfig>(
        r#"{"overrides":[{"files":["src/a.ts"],"rules":{"unused-exports":"off"}}]}"#,
    )
    .expect("config parses")
    .resolve(
        PathBuf::from("/repo"),
        fallow_config::OutputFormat::Json,
        1,
        true,
        true,
        None,
    );
    assert!(finding_id_rule_is_off(id, &with_override));
}

#[test]
fn workspace_filter_keeps_findings_under_workspace_root() {
    let root = PathBuf::from("/repo/packages/app");
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src/unused.ts"),
        }));
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: PathBuf::from("/repo/packages/docs/src/unused.ts"),
        }));

    filter_to_workspaces(&mut results, std::slice::from_ref(&root));

    assert_eq!(results.unused_files.len(), 1);
    assert_eq!(
        results.unused_files[0].file.path,
        root.join("src/unused.ts")
    );
}

#[test]
fn configured_filter_removes_findings_added_after_engine_analysis() {
    let project = tempfile::tempdir().expect("project");
    let config = serde_json::from_str::<fallow_config::FallowConfig>(
        r#"{"ignoreFindings":["src/hidden.ts"]}"#,
    )
    .expect("config parses")
    .resolve(
        project.path().to_path_buf(),
        fallow_config::OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    let mut results = AnalysisResults::default();
    results
        .private_type_leaks
        .push(PrivateTypeLeakFinding::with_actions(PrivateTypeLeak {
            path: project.path().join("src/hidden.ts"),
            export_name: "publicApi".to_string(),
            type_name: "PrivateShape".to_string(),
            line: 1,
            col: 0,
            span_start: 0,
            semantic: None,
        }));
    results
        .boundary_violations
        .push(BoundaryViolationFinding::with_actions(BoundaryViolation {
            from_path: project.path().join("src/hidden.ts"),
            to_path: project.path().join("src/data.ts"),
            from_zone: "ui".to_string(),
            to_zone: "data".to_string(),
            import_specifier: "./data".to_string(),
            line: 1,
            col: 0,
            via_path: None,
        }));

    filter_configured_ignored_findings(&mut results, &config);

    assert!(results.private_type_leaks.is_empty());
    assert_eq!(results.boundary_violations.len(), 1);
}

fn unmatched_patterns(diagnostics: &[WorkspaceDiagnostic]) -> Vec<(&'static str, String)> {
    diagnostics
        .iter()
        .filter_map(|diagnostic| match &diagnostic.kind {
            WorkspaceDiagnosticKind::IgnoreFindingsPatternUnmatched { pattern } => {
                Some(("ignoreFindings", pattern.clone()))
            }
            WorkspaceDiagnosticKind::IgnoreDependenciesGlobUnmatched { pattern } => {
                Some(("ignoreDependencies", pattern.clone()))
            }
            _ => None,
        })
        .collect()
}

fn write_manifest(root: &Path, dependencies: &str) {
    std::fs::write(
        root.join("package.json"),
        format!(
            r#"{{"name":"app","private":true,"main":"src/index.ts","dependencies":{dependencies}}}"#
        ),
    )
    .expect("write package.json");
}

#[test]
fn config_pattern_diagnostics_describe_the_latest_pass_only() {
    let project = tempfile::tempdir().expect("project");
    let root = project.path();
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(root.join("src/index.ts"), "export const main = 1;\n").expect("write entry");
    std::fs::write(root.join("src/orphan.ts"), "export const orphan = 1;\n").expect("write orphan");
    write_manifest(root, r#"{"@acme/lib":"1.0.0"}"#);
    let config = serde_json::from_str::<fallow_config::FallowConfig>(
        r#"{"ignoreDependencies":["@acme/*","@typo/*"],"ignoreFindings":["src/legcy/**"]}"#,
    )
    .expect("config parses")
    .resolve(
        root.to_path_buf(),
        fallow_config::OutputFormat::Human,
        1,
        true,
        true,
        None,
    );

    crate::session::AnalysisSession::from_resolved_config(config.clone())
        .expect("session")
        .analyze_dead_code()
        .expect("first pass");
    assert_eq!(
        unmatched_patterns(&config_pattern_diagnostics(&config, true)),
        vec![
            ("ignoreFindings", "src/legcy/**".to_owned()),
            ("ignoreDependencies", "@typo/*".to_owned()),
        ]
    );
    assert_eq!(
        unmatched_patterns(&config_pattern_diagnostics(&config, false)),
        vec![("ignoreFindings", "src/legcy/**".to_owned())],
        "a run that reports no dependency findings omits the dependency globs"
    );

    // A long-lived process keeps the config. The second pass must not
    // inherit the `@acme/*` hit of the first pass.
    write_manifest(root, r#"{"react":"1.0.0"}"#);
    crate::session::AnalysisSession::from_resolved_config(config.clone())
        .expect("session")
        .analyze_dead_code()
        .expect("second pass");
    assert_eq!(
        unmatched_patterns(&config_pattern_diagnostics(&config, true)),
        vec![
            ("ignoreFindings", "src/legcy/**".to_owned()),
            ("ignoreDependencies", "@acme/*".to_owned()),
            ("ignoreDependencies", "@typo/*".to_owned()),
        ]
    );
}

fn config_with_override(
    pattern: &str,
    configure: impl FnOnce(&mut fallow_config::PartialRulesConfig),
) -> ResolvedConfig {
    let mut partial = fallow_config::PartialRulesConfig::default();
    configure(&mut partial);
    fallow_config::FallowConfig {
        rules: RulesConfig {
            private_type_leaks: Severity::Warn,
            ..RulesConfig::default()
        },
        overrides: vec![fallow_config::ConfigOverride {
            files: vec![pattern.to_string()],
            rules: partial,
        }],
        ..fallow_config::FallowConfig::default()
    }
    .resolve(
        PathBuf::from("/project"),
        fallow_config::OutputFormat::Human,
        1,
        true,
        true,
        None,
    )
}

fn unused_export(path: &str) -> UnusedExportFinding {
    UnusedExportFinding::with_actions(UnusedExport {
        path: PathBuf::from(path),
        export_name: "Unused".to_string(),
        is_type_only: false,
        line: 1,
        col: 0,
        span_start: 0,
        is_re_export: false,
        deprecated: false,
        deprecated_reason: None,
    })
}

fn private_type_leak(path: &str) -> PrivateTypeLeakFinding {
    PrivateTypeLeakFinding::with_actions(PrivateTypeLeak {
        path: PathBuf::from(path),
        export_name: "Unused".to_string(),
        type_name: "Props".to_string(),
        line: 1,
        col: 0,
        span_start: 0,
        semantic: None,
    })
}

fn overridden_fixture() -> AnalysisResults {
    let mut results = AnalysisResults::default();
    results
        .unused_exports
        .push(unused_export("/project/src/ui/kit.ts"));
    results
        .unused_exports
        .push(unused_export("/project/src/lib/util.ts"));
    results
        .private_type_leaks
        .push(private_type_leak("/project/src/ui/kit.ts"));
    results
        .private_type_leaks
        .push(private_type_leak("/project/src/lib/util.ts"));
    results
}

#[test]
fn rule_severities_drop_findings_only_on_overridden_paths() {
    let config = config_with_override("src/ui/**", |rules| {
        rules.unused_exports = Some(Severity::Off);
        rules.private_type_leaks = Some(Severity::Off);
    });
    let mut results = overridden_fixture();

    apply_rule_severities(&mut results, &config);

    assert_eq!(
        results
            .unused_exports
            .iter()
            .map(|finding| finding.export.path.clone())
            .collect::<Vec<_>>(),
        vec![PathBuf::from("/project/src/lib/util.ts")]
    );
    assert_eq!(
        results
            .private_type_leaks
            .iter()
            .map(|finding| finding.leak.path.clone())
            .collect::<Vec<_>>(),
        vec![PathBuf::from("/project/src/lib/util.ts")]
    );
}

#[test]
fn rule_severities_are_idempotent() {
    // The editor path resolves severities once after analysis and again
    // after type-aware reconciliation, so a second pass must not change
    // the result set.
    let config = config_with_override("src/ui/**", |rules| {
        rules.unused_exports = Some(Severity::Off);
        rules.private_type_leaks = Some(Severity::Off);
    });

    let mut once = overridden_fixture();
    apply_rule_severities(&mut once, &config);
    let mut twice = overridden_fixture();
    apply_rule_severities(&mut twice, &config);
    apply_rule_severities(&mut twice, &config);

    assert_eq!(
        once.unused_exports
            .iter()
            .map(|finding| finding.export.path.clone())
            .collect::<Vec<_>>(),
        twice
            .unused_exports
            .iter()
            .map(|finding| finding.export.path.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        once.private_type_leaks
            .iter()
            .map(|finding| finding.leak.path.clone())
            .collect::<Vec<_>>(),
        twice
            .private_type_leaks
            .iter()
            .map(|finding| finding.leak.path.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn rule_severities_keep_a_cycle_when_any_member_file_stays_enabled() {
    let config = config_with_override("src/ui/**", |rules| {
        rules.circular_dependencies = Some(Severity::Off);
    });
    let mut results = AnalysisResults::default();
    results
        .circular_dependencies
        .push(CircularDependencyFinding::with_actions(
            CircularDependency {
                files: vec![
                    PathBuf::from("/project/src/ui/a.ts"),
                    PathBuf::from("/project/src/lib/b.ts"),
                ],
                length: 2,
                line: 1,
                col: 0,
                edges: Vec::new(),
                is_cross_package: false,
            },
        ));
    results
        .circular_dependencies
        .push(CircularDependencyFinding::with_actions(
            CircularDependency {
                files: vec![
                    PathBuf::from("/project/src/ui/c.ts"),
                    PathBuf::from("/project/src/ui/d.ts"),
                ],
                length: 2,
                line: 1,
                col: 0,
                edges: Vec::new(),
                is_cross_package: false,
            },
        ));

    apply_rule_severities(&mut results, &config);

    assert_eq!(results.circular_dependencies.len(), 1);
    assert_eq!(
        results.circular_dependencies[0].cycle.files[0],
        PathBuf::from("/project/src/ui/a.ts")
    );
}

#[test]
fn rule_severities_clear_base_rules_without_overrides() {
    let config = fallow_config::FallowConfig {
        rules: RulesConfig {
            unused_exports: Severity::Off,
            private_type_leaks: Severity::Warn,
            ..RulesConfig::default()
        },
        ..fallow_config::FallowConfig::default()
    }
    .resolve(
        PathBuf::from("/project"),
        fallow_config::OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    let mut results = overridden_fixture();

    apply_rule_severities(&mut results, &config);

    assert!(results.unused_exports.is_empty());
    assert_eq!(results.private_type_leaks.len(), 2);
}

const DEAD_FILE: &str = "/project/src/dead.ts";
const LIVE_FILE: &str = "/project/src/live.ts";

fn resolve(config: fallow_config::FallowConfig) -> ResolvedConfig {
    config.resolve(
        PathBuf::from("/project"),
        fallow_config::OutputFormat::Human,
        1,
        true,
        true,
        None,
    )
}

fn member(path: &str, kind: fallow_types::extract::MemberKind) -> UnusedMember {
    UnusedMember {
        path: PathBuf::from(path),
        parent_name: "Parent".to_string(),
        member_name: "member".to_string(),
        kind,
        line: 2,
        col: 0,
    }
}

fn unused_type(path: &str) -> UnusedTypeFinding {
    let mut export = unused_export(path).export;
    export.is_type_only = true;
    UnusedTypeFinding::with_actions(export)
}

/// One finding of each cascade kind in `DEAD_FILE` and in `LIVE_FILE`,
/// plus findings of other kinds that touch `DEAD_FILE`.
fn cascade_fixture() -> AnalysisResults {
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: PathBuf::from(DEAD_FILE),
        }));
    for path in [DEAD_FILE, LIVE_FILE] {
        results.unused_exports.push(unused_export(path));
        results.unused_types.push(unused_type(path));
        results
            .unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(member(
                path,
                fallow_types::extract::MemberKind::EnumMember,
            )));
        results
            .unused_class_members
            .push(UnusedClassMemberFinding::with_actions(member(
                path,
                fallow_types::extract::MemberKind::ClassMethod,
            )));
    }
    results
        .duplicate_exports
        .push(DuplicateExportFinding::with_actions(DuplicateExport {
            export_name: "Shared".to_string(),
            locations: [DEAD_FILE, LIVE_FILE]
                .iter()
                .map(|path| DuplicateLocation {
                    path: PathBuf::from(path),
                    line: 1,
                    col: 0,
                })
                .collect(),
        }));
    results
        .circular_dependencies
        .push(CircularDependencyFinding::with_actions(
            CircularDependency {
                files: vec![PathBuf::from(DEAD_FILE), PathBuf::from(LIVE_FILE)],
                length: 2,
                line: 1,
                col: 0,
                edges: Vec::new(),
                is_cross_package: false,
            },
        ));
    results
        .unlisted_dependencies
        .push(UnlistedDependencyFinding::with_actions(
            UnlistedDependency {
                package_name: "left-pad".to_string(),
                imported_from: vec![ImportSite {
                    path: PathBuf::from(DEAD_FILE),
                    line: 1,
                    col: 0,
                }],
            },
        ));
    results
        .boundary_violations
        .push(BoundaryViolationFinding::with_actions(BoundaryViolation {
            from_path: PathBuf::from(DEAD_FILE),
            to_path: PathBuf::from(LIVE_FILE),
            from_zone: "ui".to_string(),
            to_zone: "data".to_string(),
            import_specifier: "./live".to_string(),
            line: 1,
            col: 0,
            via_path: None,
        }));
    results
}

fn cascade_paths(results: &AnalysisResults) -> Vec<Vec<PathBuf>> {
    vec![
        results
            .unused_exports
            .iter()
            .map(|f| f.export.path.clone())
            .collect(),
        results
            .unused_types
            .iter()
            .map(|f| f.export.path.clone())
            .collect(),
        results
            .unused_enum_members
            .iter()
            .map(|f| f.member.path.clone())
            .collect(),
        results
            .unused_class_members
            .iter()
            .map(|f| f.member.path.clone())
            .collect(),
    ]
}

#[test]
fn rule_pass_hides_the_cascade_kinds_of_unused_files() {
    let config = resolve(fallow_config::FallowConfig::default());
    let mut results = cascade_fixture();

    apply_rule_severities(&mut results, &config);

    let live = vec![PathBuf::from(LIVE_FILE)];
    assert_eq!(cascade_paths(&results), vec![live; 4]);
    assert_eq!(results.cascade_hidden, 4);
    assert_eq!(results.cascade.hidden.len(), 4);
    assert_eq!(results.unused_files.len(), 1, "the unused file stays");
}

#[test]
fn rule_pass_keeps_other_kinds_in_unused_files() {
    let config = resolve(fallow_config::FallowConfig::default());
    let mut results = cascade_fixture();

    apply_rule_severities(&mut results, &config);

    assert_eq!(results.duplicate_exports.len(), 1);
    assert_eq!(results.circular_dependencies.len(), 1);
    assert_eq!(results.unlisted_dependencies.len(), 1);
    assert_eq!(results.boundary_violations.len(), 1);
}

#[test]
fn show_cascade_keeps_every_finding() {
    let config = resolve(fallow_config::FallowConfig {
        show_cascade: true,
        ..fallow_config::FallowConfig::default()
    });
    let mut results = cascade_fixture();

    apply_rule_severities(&mut results, &config);

    let both = vec![PathBuf::from(DEAD_FILE), PathBuf::from(LIVE_FILE)];
    assert_eq!(cascade_paths(&results), vec![both; 4]);
    assert_eq!(results.cascade_hidden, 0);
}

#[test]
fn unused_files_rule_off_for_the_file_keeps_its_findings() {
    let config = config_with_override("src/dead.ts", |rules| {
        rules.unused_files = Some(Severity::Off);
    });
    let mut results = cascade_fixture();

    apply_rule_severities(&mut results, &config);

    assert!(results.unused_files.is_empty());
    let both = vec![PathBuf::from(DEAD_FILE), PathBuf::from(LIVE_FILE)];
    assert_eq!(cascade_paths(&results), vec![both; 4]);
    assert_eq!(results.cascade_hidden, 0);
}

#[test]
fn a_suppressed_unused_file_still_hides_its_findings() {
    // An inline `fallow-ignore-file unused-file` comment removes the file
    // from `unused_files`, and the detector records it as suppressed.
    let config = resolve(fallow_config::FallowConfig::default());
    let mut results = cascade_fixture();
    results.unused_files.clear();
    results.cascade.suppressed_unused_files = vec![PathBuf::from(DEAD_FILE)];

    apply_rule_severities(&mut results, &config);

    let live = vec![PathBuf::from(LIVE_FILE)];
    assert_eq!(cascade_paths(&results), vec![live; 4]);
    assert_eq!(results.cascade_hidden, 4);
}

#[test]
fn a_file_missing_from_the_report_hides_nothing() {
    // An issue-type filter without `unused-files`, a scope or a rule can
    // remove the unused file from the report. Its findings must then stay,
    // or the report loses them without a pointer to the file.
    let config = resolve(fallow_config::FallowConfig::default());
    let mut results = cascade_fixture();
    results.unused_files.clear();

    apply_rule_severities(&mut results, &config);

    let both = vec![PathBuf::from(DEAD_FILE), PathBuf::from(LIVE_FILE)];
    assert_eq!(cascade_paths(&results), vec![both; 4]);
    assert_eq!(results.cascade_hidden, 0);
}

#[test]
fn cascade_hiding_is_idempotent() {
    let config = resolve(fallow_config::FallowConfig::default());
    let mut results = cascade_fixture();

    apply_rule_severities(&mut results, &config);
    apply_rule_severities(&mut results, &config);

    assert_eq!(results.cascade_hidden, 4);
    assert_eq!(results.cascade.hidden.len(), 4);
}

#[test]
fn cascade_hidden_serializes_only_when_not_zero() {
    let config = resolve(fallow_config::FallowConfig::default());
    let mut results = cascade_fixture();
    let before = serde_json::to_value(&results).expect("serializes");
    assert!(before.get("cascade_hidden").is_none());

    apply_rule_severities(&mut results, &config);

    let after = serde_json::to_value(&results).expect("serializes");
    assert_eq!(after["cascade_hidden"], 4);
    assert!(after.get("cascade").is_none());
}
