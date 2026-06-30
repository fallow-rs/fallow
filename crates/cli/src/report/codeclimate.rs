use std::path::Path;
use std::process::ExitCode;

use fallow_config::RulesConfig;
#[cfg(test)]
use fallow_config::Severity;
use fallow_output::{
    CodeClimateAnnotationField, CodeClimateIssue, HealthReport, annotate_codeclimate_issues,
    codeclimate_issues_to_value,
};
#[cfg(test)]
use fallow_output::{CodeClimateSeverity, codeclimate_fingerprint_hash};
use fallow_types::duplicates::DuplicationReport;
use fallow_types::results::AnalysisResults;

use super::grouping::{self, OwnershipResolver};
use super::{emit_json, normalize_uri, relative_path};

/// Map fallow severity to CodeClimate severity.
#[cfg(test)]
fn severity_to_codeclimate(s: Severity) -> CodeClimateSeverity {
    match s {
        Severity::Error => CodeClimateSeverity::Major,
        Severity::Warn => CodeClimateSeverity::Minor,
        Severity::Off => unreachable!(),
    }
}

/// Compute a relative path string with forward-slash normalization.
///
/// Uses `normalize_uri` to ensure forward slashes on all platforms
/// and percent-encode brackets for Next.js dynamic routes.
fn cc_path(path: &Path, root: &Path) -> String {
    normalize_uri(&relative_path(path, root).display().to_string())
}

#[cfg(test)]
fn fingerprint_hash(parts: &[&str]) -> String {
    codeclimate_fingerprint_hash(parts)
}

#[cfg(test)]
fn coverage_intelligence_check_name(
    recommendation: fallow_output::CoverageIntelligenceRecommendation,
) -> &'static str {
    match recommendation {
        fallow_output::CoverageIntelligenceRecommendation::AddTestOrSplitBeforeMerge => {
            "fallow/coverage-intelligence-risky-change"
        }
        fallow_output::CoverageIntelligenceRecommendation::DeleteAfterConfirmingOwner => {
            "fallow/coverage-intelligence-delete"
        }
        fallow_output::CoverageIntelligenceRecommendation::ReviewBeforeChanging => {
            "fallow/coverage-intelligence-review"
        }
        fallow_output::CoverageIntelligenceRecommendation::RefactorCarefullyKeepBehavior => {
            "fallow/coverage-intelligence-refactor"
        }
    }
}

#[cfg(test)]
const fn health_finding_severity(severity: fallow_output::FindingSeverity) -> CodeClimateSeverity {
    match severity {
        fallow_output::FindingSeverity::Critical => CodeClimateSeverity::Critical,
        fallow_output::FindingSeverity::High => CodeClimateSeverity::Major,
        fallow_output::FindingSeverity::Moderate => CodeClimateSeverity::Minor,
    }
}

#[cfg(test)]
const fn runtime_coverage_check_name(
    verdict: fallow_output::RuntimeCoverageVerdict,
) -> &'static str {
    match verdict {
        fallow_output::RuntimeCoverageVerdict::SafeToDelete => "fallow/runtime-safe-to-delete",
        fallow_output::RuntimeCoverageVerdict::ReviewRequired => "fallow/runtime-review-required",
        fallow_output::RuntimeCoverageVerdict::LowTraffic => "fallow/runtime-low-traffic",
        fallow_output::RuntimeCoverageVerdict::CoverageUnavailable => {
            "fallow/runtime-coverage-unavailable"
        }
        fallow_output::RuntimeCoverageVerdict::Active
        | fallow_output::RuntimeCoverageVerdict::Unknown => "fallow/runtime-coverage",
    }
}

#[cfg(test)]
const fn runtime_coverage_severity(
    verdict: fallow_output::RuntimeCoverageVerdict,
) -> CodeClimateSeverity {
    match verdict {
        fallow_output::RuntimeCoverageVerdict::SafeToDelete => CodeClimateSeverity::Critical,
        fallow_output::RuntimeCoverageVerdict::ReviewRequired => CodeClimateSeverity::Major,
        _ => CodeClimateSeverity::Minor,
    }
}

#[cfg(test)]
const fn coverage_intelligence_severity(
    verdict: fallow_output::CoverageIntelligenceVerdict,
) -> Option<CodeClimateSeverity> {
    match verdict {
        fallow_output::CoverageIntelligenceVerdict::RiskyChangeDetected
        | fallow_output::CoverageIntelligenceVerdict::HighConfidenceDelete => {
            Some(CodeClimateSeverity::Major)
        }
        fallow_output::CoverageIntelligenceVerdict::ReviewRequired
        | fallow_output::CoverageIntelligenceVerdict::RefactorCarefully => {
            Some(CodeClimateSeverity::Minor)
        }
        fallow_output::CoverageIntelligenceVerdict::Clean
        | fallow_output::CoverageIntelligenceVerdict::Unknown => None,
    }
}

/// Fetch CodeClimate issues from the API-owned dead-code output builder.
///
/// Returns the typed [`CodeClimateIssue`] vec; callers that emit the wire
/// shape convert via [`fallow_output::codeclimate_issues_to_value`]. The schema
/// drift gate locks the per-issue shape against
/// [`fallow_output::CodeClimateOutput`].
#[must_use]
pub(super) fn api_codeclimate_issues(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
) -> Vec<CodeClimateIssue> {
    fallow_api::build_codeclimate(results, root, rules)
}

/// Print dead-code analysis results in CodeClimate format.
pub(super) fn print_codeclimate(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
) -> ExitCode {
    let issues = api_codeclimate_issues(results, root, rules);
    let value = codeclimate_issues_to_value(&issues);
    emit_json(&value, "CodeClimate")
}

/// Print CodeClimate output with owner properties added to each issue.
///
/// Calls `api_codeclimate_issues` to produce the standard CodeClimate issues, then
/// annotates each entry with `"owner": "@team"` by resolving the
/// issue's location path through the `OwnershipResolver`.
pub(super) fn print_grouped_codeclimate(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    resolver: &OwnershipResolver,
) -> ExitCode {
    let mut issues = api_codeclimate_issues(results, root, rules);
    annotate_codeclimate_issues(&mut issues, CodeClimateAnnotationField::Owner, |path| {
        grouping::resolve_owner(Path::new(path), Path::new(""), resolver)
    });
    let value = codeclimate_issues_to_value(&issues);

    emit_json(&value, "CodeClimate")
}

/// Fetch CodeClimate issues from the API-owned health output builder.
#[must_use]
pub(super) fn api_health_codeclimate_issues(
    report: &HealthReport,
    root: &Path,
) -> Vec<CodeClimateIssue> {
    fallow_api::build_health_codeclimate(report, root)
}

/// Print health analysis results in CodeClimate format.
pub(super) fn print_health_codeclimate(report: &HealthReport, root: &Path) -> ExitCode {
    let issues = api_health_codeclimate_issues(report, root);
    let value = codeclimate_issues_to_value(&issues);
    emit_json(&value, "CodeClimate")
}

/// Print health CodeClimate output with a per-issue `group` field.
///
/// Mirrors the dead-code grouped CodeClimate pattern
/// (`print_grouped_codeclimate`): build the standard payload first, then
/// post-process each issue to attach a `group` key derived from the
/// `OwnershipResolver`. Lets GitLab Code Quality and other CodeClimate
/// consumers partition findings per team / package without re-parsing the
/// project structure.
pub(super) fn print_grouped_health_codeclimate(
    report: &HealthReport,
    root: &Path,
    resolver: &OwnershipResolver,
) -> ExitCode {
    let mut issues = api_health_codeclimate_issues(report, root);
    annotate_codeclimate_issues(&mut issues, CodeClimateAnnotationField::Group, |path| {
        grouping::resolve_owner(Path::new(path), Path::new(""), resolver)
    });
    let value = codeclimate_issues_to_value(&issues);

    emit_json(&value, "CodeClimate")
}

/// Fetch CodeClimate issues from the API-owned duplication output builder.
#[must_use]
pub(super) fn api_duplication_codeclimate_issues(
    report: &DuplicationReport,
    root: &Path,
) -> Vec<CodeClimateIssue> {
    fallow_api::build_duplication_codeclimate(report, root)
}

/// Print duplication analysis results in CodeClimate format.
pub(super) fn print_duplication_codeclimate(report: &DuplicationReport, root: &Path) -> ExitCode {
    let issues = api_duplication_codeclimate_issues(report, root);
    let value = codeclimate_issues_to_value(&issues);
    emit_json(&value, "CodeClimate")
}

/// Print duplication CodeClimate output with a per-issue `group` field.
///
/// Mirrors [`print_grouped_health_codeclimate`]: each clone group is attributed
/// to its largest owner ([`super::dupes_grouping::largest_owner`]) and every
/// CodeClimate issue emitted for that clone group's instances carries the same
/// top-level `group` key. Lets GitLab Code Quality and other CodeClimate
/// consumers partition findings per team / package / directory without
/// re-parsing the project structure.
pub(super) fn print_grouped_duplication_codeclimate(
    report: &DuplicationReport,
    root: &Path,
    resolver: &OwnershipResolver,
) -> ExitCode {
    let mut issues = api_duplication_codeclimate_issues(report, root);

    use rustc_hash::FxHashMap;
    let mut path_to_owner: FxHashMap<String, String> = FxHashMap::default();
    for group in &report.clone_groups {
        let owner = super::dupes_grouping::largest_owner(group, root, resolver);
        for instance in &group.instances {
            let path = cc_path(&instance.file, root);
            path_to_owner.insert(path, owner.clone());
        }
    }

    annotate_codeclimate_issues(&mut issues, CodeClimateAnnotationField::Group, |path| {
        path_to_owner
            .get(path)
            .cloned()
            .unwrap_or_else(|| crate::codeowners::UNOWNED_LABEL.to_string())
    });
    let value = codeclimate_issues_to_value(&issues);

    emit_json(&value, "CodeClimate")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::test_helpers::sample_results;
    use fallow_config::RulesConfig;
    use fallow_types::output_dead_code::*;
    use fallow_types::results::*;
    use std::path::PathBuf;

    /// Compute graduated severity for health findings based on threshold ratio.
    /// Kept for unit test coverage of the original CodeClimate severity model.
    fn health_severity(value: u16, threshold: u16) -> &'static str {
        if threshold == 0 {
            return "minor";
        }
        let ratio = f64::from(value) / f64::from(threshold);
        if ratio > 2.5 {
            "critical"
        } else if ratio > 1.5 {
            "major"
        } else {
            "minor"
        }
    }

    #[test]
    fn codeclimate_empty_results_produces_empty_array() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        let arr = output.as_array().unwrap();
        assert!(arr.is_empty());
    }

    #[test]
    fn codeclimate_produces_array_of_issues() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert!(output.is_array());
        let arr = output.as_array().unwrap();
        assert!(!arr.is_empty());
    }

    #[test]
    fn codeclimate_missing_suppression_reason_uses_reason_rule_severity() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.stale_suppressions.push(StaleSuppression {
            path: root.join("src/file.ts"),
            line: 1,
            col: 0,
            origin: SuppressionOrigin::Comment {
                issue_kind: Some("unused-exports".to_string()),
                reason: None,
                is_file_level: false,
                kind_known: true,
            },
            missing_reason: true,
            actions: StaleSuppression::actions_for(true),
        });
        let rules = RulesConfig {
            stale_suppressions: Severity::Off,
            require_suppression_reason: Severity::Error,
            ..Default::default()
        };

        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));

        assert_eq!(output[0]["check_name"], "fallow/missing-suppression-reason");
        assert_eq!(output[0]["severity"], "major");
    }

    #[test]
    fn codeclimate_stale_and_missing_suppression_have_distinct_identities() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        let origin = SuppressionOrigin::Comment {
            issue_kind: Some("unused-exports".to_string()),
            reason: None,
            is_file_level: false,
            kind_known: true,
        };
        results.stale_suppressions.push(StaleSuppression {
            path: root.join("src/file.ts"),
            line: 1,
            col: 0,
            origin: origin.clone(),
            missing_reason: false,
            actions: StaleSuppression::actions_for(false),
        });
        results.stale_suppressions.push(StaleSuppression {
            path: root.join("src/file.ts"),
            line: 1,
            col: 0,
            origin,
            missing_reason: true,
            actions: StaleSuppression::actions_for(true),
        });
        let rules = RulesConfig {
            stale_suppressions: Severity::Warn,
            require_suppression_reason: Severity::Error,
            ..Default::default()
        };

        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));

        assert_eq!(output[0]["check_name"], "fallow/stale-suppression");
        assert_eq!(output[1]["check_name"], "fallow/missing-suppression-reason");
        assert_ne!(output[0]["fingerprint"], output[1]["fingerprint"]);
    }

    #[test]
    fn codeclimate_issue_has_required_fields() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/dead.ts"),
            }));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        let issue = &output.as_array().unwrap()[0];

        assert_eq!(issue["type"], "issue");
        assert_eq!(issue["check_name"], "fallow/unused-file");
        assert!(issue["description"].is_string());
        assert!(issue["categories"].is_array());
        assert!(issue["severity"].is_string());
        assert!(issue["fingerprint"].is_string());
        assert!(issue["location"].is_object());
        assert!(issue["location"]["path"].is_string());
        assert!(issue["location"]["lines"].is_object());
    }

    #[test]
    fn codeclimate_unused_file_severity_follows_rules() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/dead.ts"),
            }));

        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["severity"], "major");

        let rules = RulesConfig {
            unused_files: Severity::Warn,
            ..RulesConfig::default()
        };
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["severity"], "minor");
    }

    #[test]
    fn codeclimate_unused_export_has_line_number() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/utils.ts"),
                export_name: "helperFn".to_string(),
                is_type_only: false,
                line: 10,
                col: 4,
                span_start: 120,
                is_re_export: false,
            }));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        let issue = &output[0];
        assert_eq!(issue["location"]["lines"]["begin"], 10);
    }

    #[test]
    fn codeclimate_unused_file_line_defaults_to_1() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/dead.ts"),
            }));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        let issue = &output[0];
        assert_eq!(issue["location"]["lines"]["begin"], 1);
    }

    #[test]
    fn codeclimate_paths_are_relative() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/deep/nested/file.ts"),
            }));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        let path = output[0]["location"]["path"].as_str().unwrap();
        assert_eq!(path, "src/deep/nested/file.ts");
        assert!(!path.starts_with("/project"));
    }

    #[test]
    fn codeclimate_re_export_label_in_description() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/index.ts"),
                export_name: "reExported".to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: true,
            }));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("Re-export"));
    }

    #[test]
    fn codeclimate_unlisted_dep_one_issue_per_import_site() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "chalk".to_string(),
                    imported_from: vec![
                        ImportSite {
                            path: root.join("src/a.ts"),
                            line: 1,
                            col: 0,
                        },
                        ImportSite {
                            path: root.join("src/b.ts"),
                            line: 5,
                            col: 0,
                        },
                    ],
                },
            ));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        let arr = output.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["check_name"], "fallow/unlisted-dependency");
        assert_eq!(arr[1]["check_name"], "fallow/unlisted-dependency");
    }

    #[test]
    fn codeclimate_duplicate_export_one_issue_per_location() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "Config".to_string(),
                locations: vec![
                    DuplicateLocation {
                        path: root.join("src/a.ts"),
                        line: 10,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: root.join("src/b.ts"),
                        line: 20,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: root.join("src/c.ts"),
                        line: 30,
                        col: 0,
                    },
                ],
            }));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        let arr = output.as_array().unwrap();
        assert_eq!(arr.len(), 3);
    }

    #[test]
    fn codeclimate_circular_dep_emits_chain_in_description() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![root.join("src/a.ts"), root.join("src/b.ts")],
                    length: 2,
                    line: 3,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("Circular dependency"));
        assert!(desc.contains("src/a.ts"));
        assert!(desc.contains("src/b.ts"));
    }

    #[test]
    fn codeclimate_fingerprints_are_deterministic() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let rules = RulesConfig::default();
        let output1 = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        let output2 = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));

        let fps1: Vec<&str> = output1
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["fingerprint"].as_str().unwrap())
            .collect();
        let fps2: Vec<&str> = output2
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["fingerprint"].as_str().unwrap())
            .collect();
        assert_eq!(fps1, fps2);
    }

    #[test]
    fn codeclimate_fingerprints_are_unique() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));

        let mut fps: Vec<&str> = output
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["fingerprint"].as_str().unwrap())
            .collect();
        let original_len = fps.len();
        fps.sort_unstable();
        fps.dedup();
        assert_eq!(fps.len(), original_len, "fingerprints should be unique");
    }

    #[test]
    fn codeclimate_type_only_dep_has_correct_check_name() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .type_only_dependencies
            .push(TypeOnlyDependencyFinding::with_actions(
                TypeOnlyDependency {
                    package_name: "zod".to_string(),
                    path: root.join("package.json"),
                    line: 8,
                },
            ));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/type-only-dependency");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("zod"));
        assert!(desc.contains("type-only"));
    }

    #[test]
    fn codeclimate_dep_with_zero_line_omits_line_number() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".to_string(),
                location: DependencyLocation::Dependencies,
                path: root.join("package.json"),
                line: 0,
                used_in_workspaces: Vec::new(),
            }));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["location"]["lines"]["begin"], 1);
    }

    #[test]
    fn fingerprint_hash_different_inputs_differ() {
        let h1 = fingerprint_hash(&["a", "b"]);
        let h2 = fingerprint_hash(&["a", "c"]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn fingerprint_hash_order_matters() {
        let h1 = fingerprint_hash(&["a", "b"]);
        let h2 = fingerprint_hash(&["b", "a"]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn fingerprint_hash_separator_prevents_collision() {
        let h1 = fingerprint_hash(&["ab", "c"]);
        let h2 = fingerprint_hash(&["a", "bc"]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn fingerprint_hash_is_16_hex_chars() {
        let h = fingerprint_hash(&["test"]);
        assert_eq!(h.len(), 16);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn severity_error_maps_to_major() {
        assert_eq!(
            severity_to_codeclimate(Severity::Error),
            CodeClimateSeverity::Major
        );
    }

    #[test]
    fn severity_warn_maps_to_minor() {
        assert_eq!(
            severity_to_codeclimate(Severity::Warn),
            CodeClimateSeverity::Minor
        );
    }

    #[test]
    #[should_panic(expected = "internal error: entered unreachable code")]
    fn severity_off_is_unreachable() {
        let _ = severity_to_codeclimate(Severity::Off);
    }

    /// Production-mode regression: rules can flip to `Severity::Off` while
    /// the matching findings slice arrives empty (the analyzer's own off-
    /// rule short-circuit clears the vec, but the generic-iterator helpers
    /// in `codeclimate.rs` previously called `severity_to_codeclimate`
    /// before checking emptiness and panicked at `Severity::Off`).
    /// `fallow dead-code --format codeclimate --production` on any project
    /// with a `--production`-suppressed dep / export / member rule used to
    /// exit 101 with `entered unreachable code` at `ci/severity.rs:28`.
    /// This test exercises all three previously-vulnerable helpers
    /// (`push_dep_cc_issues`, `push_unused_export_issues`,
    /// `push_unused_member_issues`) through `api_codeclimate_issues`.
    #[test]
    fn api_codeclimate_issues_with_off_severity_and_empty_findings_does_not_panic() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let rules = RulesConfig {
            unused_dependencies: Severity::Off,
            unused_dev_dependencies: Severity::Off,
            unused_optional_dependencies: Severity::Off,
            unused_exports: Severity::Off,
            unused_types: Severity::Off,
            unused_enum_members: Severity::Off,
            unused_class_members: Severity::Off,
            ..RulesConfig::default()
        };
        let issues = api_codeclimate_issues(&results, &root, &rules);
        assert!(issues.is_empty());
    }

    #[test]
    fn health_severity_zero_threshold_returns_minor() {
        assert_eq!(health_severity(100, 0), "minor");
    }

    #[test]
    fn health_severity_at_threshold_returns_minor() {
        assert_eq!(health_severity(10, 10), "minor");
    }

    #[test]
    fn health_severity_1_5x_threshold_returns_minor() {
        assert_eq!(health_severity(15, 10), "minor");
    }

    #[test]
    fn health_severity_above_1_5x_returns_major() {
        assert_eq!(health_severity(16, 10), "major");
    }

    #[test]
    fn health_severity_at_2_5x_returns_major() {
        assert_eq!(health_severity(25, 10), "major");
    }

    #[test]
    fn health_severity_above_2_5x_returns_critical() {
        assert_eq!(health_severity(26, 10), "critical");
    }

    #[test]
    fn health_codeclimate_includes_coverage_gaps() {
        use fallow_output::*;

        let root = PathBuf::from("/project");
        let report = HealthReport {
            summary: HealthSummary {
                files_analyzed: 10,
                functions_analyzed: 50,
                ..Default::default()
            },
            coverage_gaps: Some(CoverageGaps {
                summary: CoverageGapSummary {
                    runtime_files: 2,
                    covered_files: 0,
                    file_coverage_pct: 0.0,
                    untested_files: 1,
                    untested_exports: 1,
                },
                files: vec![UntestedFileFinding::with_actions(
                    UntestedFile {
                        path: root.join("src/app.ts"),
                        value_export_count: 2,
                    },
                    &root,
                )],
                exports: vec![UntestedExportFinding::with_actions(
                    UntestedExport {
                        path: root.join("src/app.ts"),
                        export_name: "loader".into(),
                        line: 12,
                        col: 4,
                    },
                    &root,
                )],
            }),
            ..Default::default()
        };

        let output = codeclimate_issues_to_value(&api_health_codeclimate_issues(&report, &root));
        let issues = output.as_array().unwrap();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0]["check_name"], "fallow/untested-file");
        assert_eq!(issues[0]["categories"][0], "Coverage");
        assert_eq!(issues[0]["location"]["path"], "src/app.ts");
        assert_eq!(issues[1]["check_name"], "fallow/untested-export");
        assert_eq!(issues[1]["location"]["lines"]["begin"], 12);
        assert!(
            issues[1]["description"]
                .as_str()
                .unwrap()
                .contains("loader")
        );
    }

    #[test]
    fn health_codeclimate_includes_coverage_intelligence_issue() {
        use fallow_output::{
            CoverageIntelligenceAction, CoverageIntelligenceConfidence,
            CoverageIntelligenceEvidence, CoverageIntelligenceFinding,
            CoverageIntelligenceMatchConfidence, CoverageIntelligenceRecommendation,
            CoverageIntelligenceReport, CoverageIntelligenceSchemaVersion,
            CoverageIntelligenceSignal, CoverageIntelligenceSummary, CoverageIntelligenceVerdict,
            HealthReport, HealthSummary,
        };

        let root = PathBuf::from("/project");
        let report = HealthReport {
            summary: HealthSummary {
                files_analyzed: 10,
                functions_analyzed: 50,
                ..Default::default()
            },
            coverage_intelligence: Some(CoverageIntelligenceReport {
                schema_version: CoverageIntelligenceSchemaVersion::V1,
                verdict: CoverageIntelligenceVerdict::HighConfidenceDelete,
                summary: CoverageIntelligenceSummary {
                    findings: 1,
                    high_confidence_deletes: 1,
                    ..Default::default()
                },
                findings: vec![CoverageIntelligenceFinding {
                    id: "fallow:coverage-intel:abc123".to_owned(),
                    path: root.join("src/dead.ts"),
                    identity: Some("deadPath".to_owned()),
                    line: 9,
                    verdict: CoverageIntelligenceVerdict::HighConfidenceDelete,
                    signals: vec![CoverageIntelligenceSignal::RuntimeCold],
                    recommendation: CoverageIntelligenceRecommendation::DeleteAfterConfirmingOwner,
                    confidence: CoverageIntelligenceConfidence::High,
                    related_ids: vec!["fallow:prod:deadbeef".to_owned()],
                    evidence: CoverageIntelligenceEvidence {
                        match_confidence: CoverageIntelligenceMatchConfidence::Direct,
                        ..Default::default()
                    },
                    actions: vec![CoverageIntelligenceAction {
                        kind: "delete-after-confirming-owner".to_owned(),
                        description: "Confirm ownership".to_owned(),
                        auto_fixable: false,
                    }],
                }],
            }),
            ..Default::default()
        };

        let output = codeclimate_issues_to_value(&api_health_codeclimate_issues(&report, &root));
        let issues = output.as_array().unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(
            issues[0]["check_name"],
            "fallow/coverage-intelligence-delete"
        );
        assert!(!issues[0]["fingerprint"].as_str().unwrap().is_empty());
        assert_eq!(issues[0]["location"]["path"], "src/dead.ts");
        assert!(
            issues[0]["description"]
                .as_str()
                .unwrap()
                .contains("deadPath")
        );
    }

    #[test]
    fn health_codeclimate_skips_summary_only_coverage_intelligence() {
        use fallow_output::{
            CoverageIntelligenceReport, CoverageIntelligenceSchemaVersion,
            CoverageIntelligenceSummary, CoverageIntelligenceVerdict, HealthReport,
        };

        let root = PathBuf::from("/project");
        let report = HealthReport {
            coverage_intelligence: Some(CoverageIntelligenceReport {
                schema_version: CoverageIntelligenceSchemaVersion::V1,
                verdict: CoverageIntelligenceVerdict::Clean,
                summary: CoverageIntelligenceSummary {
                    skipped_ambiguous_matches: 2,
                    ..Default::default()
                },
                findings: vec![],
            }),
            ..Default::default()
        };

        let issues = api_health_codeclimate_issues(&report, &root);
        assert!(issues.is_empty());
    }

    #[test]
    fn health_codeclimate_crap_only_uses_crap_check_name() {
        use fallow_output::{ComplexityViolation, FindingSeverity, HealthReport, HealthSummary};
        let root = PathBuf::from("/project");
        let report = HealthReport {
            findings: vec![
                ComplexityViolation {
                    path: root.join("src/untested.ts"),
                    name: "risky".to_string(),
                    line: 7,
                    col: 0,
                    cyclomatic: 10,
                    cognitive: 10,
                    line_count: 20,
                    param_count: 1,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
                    exceeded: fallow_output::ExceededThreshold::Crap,
                    severity: FindingSeverity::High,
                    crap: Some(60.0),
                    coverage_pct: Some(25.0),
                    coverage_tier: None,
                    coverage_source: None,
                    inherited_from: None,
                    component_rollup: None,
                    contributions: Vec::new(),
                    effective_thresholds: None,
                    threshold_source: None,
                }
                .into(),
            ],
            summary: HealthSummary {
                functions_analyzed: 10,
                functions_above_threshold: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let json = codeclimate_issues_to_value(&api_health_codeclimate_issues(&report, &root));
        let issues = json.as_array().unwrap();
        assert_eq!(issues[0]["check_name"], "fallow/high-crap-score");
        assert_eq!(issues[0]["severity"], "major");
        let description = issues[0]["description"].as_str().unwrap();
        assert!(description.contains("CRAP score"), "desc: {description}");
        assert!(description.contains("coverage 25%"), "desc: {description}");
    }

    // ---------------------------------------------------------------------------
    // coverage_intelligence_check_name: all four recommendation arms
    // ---------------------------------------------------------------------------

    #[test]
    fn coverage_intelligence_check_name_review_before_changing() {
        use fallow_output::CoverageIntelligenceRecommendation;
        assert_eq!(
            coverage_intelligence_check_name(
                CoverageIntelligenceRecommendation::ReviewBeforeChanging
            ),
            "fallow/coverage-intelligence-review"
        );
    }

    #[test]
    fn coverage_intelligence_check_name_refactor_carefully() {
        use fallow_output::CoverageIntelligenceRecommendation;
        assert_eq!(
            coverage_intelligence_check_name(
                CoverageIntelligenceRecommendation::RefactorCarefullyKeepBehavior
            ),
            "fallow/coverage-intelligence-refactor"
        );
    }

    // ---------------------------------------------------------------------------
    // complexity description: Both / Cyclomatic / Cognitive / All thresholds
    // ---------------------------------------------------------------------------

    #[test]
    fn complexity_description_both_threshold_exceeded() {
        use fallow_output::{ComplexityViolation, ExceededThreshold, FindingSeverity};
        let root = PathBuf::from("/project");
        let report = fallow_output::HealthReport {
            findings: vec![
                ComplexityViolation {
                    path: root.join("src/fn.ts"),
                    name: "bothFn".to_string(),
                    line: 1,
                    col: 0,
                    cyclomatic: 25,
                    cognitive: 20,
                    line_count: 50,
                    param_count: 2,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
                    exceeded: ExceededThreshold::Both,
                    severity: FindingSeverity::High,
                    crap: None,
                    coverage_pct: None,
                    coverage_tier: None,
                    coverage_source: None,
                    inherited_from: None,
                    component_rollup: None,
                    contributions: vec![],
                    effective_thresholds: None,
                    threshold_source: None,
                }
                .into(),
            ],
            ..Default::default()
        };
        let json = codeclimate_issues_to_value(&api_health_codeclimate_issues(&report, &root));
        let issues = json.as_array().unwrap();
        assert_eq!(issues[0]["check_name"], "fallow/high-complexity");
        let desc = issues[0]["description"].as_str().unwrap();
        assert!(desc.contains("cyclomatic complexity"), "desc: {desc}");
        assert!(desc.contains("cognitive complexity"), "desc: {desc}");
    }

    #[test]
    fn complexity_description_cyclomatic_only() {
        use fallow_output::{ComplexityViolation, ExceededThreshold, FindingSeverity};
        let root = PathBuf::from("/project");
        let report = fallow_output::HealthReport {
            findings: vec![
                ComplexityViolation {
                    path: root.join("src/fn.ts"),
                    name: "cycFn".to_string(),
                    line: 1,
                    col: 0,
                    cyclomatic: 25,
                    cognitive: 5,
                    line_count: 30,
                    param_count: 1,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
                    exceeded: ExceededThreshold::Cyclomatic,
                    severity: FindingSeverity::High,
                    crap: None,
                    coverage_pct: None,
                    coverage_tier: None,
                    coverage_source: None,
                    inherited_from: None,
                    component_rollup: None,
                    contributions: vec![],
                    effective_thresholds: None,
                    threshold_source: None,
                }
                .into(),
            ],
            ..Default::default()
        };
        let json = codeclimate_issues_to_value(&api_health_codeclimate_issues(&report, &root));
        let issues = json.as_array().unwrap();
        assert_eq!(issues[0]["check_name"], "fallow/high-cyclomatic-complexity");
        let desc = issues[0]["description"].as_str().unwrap();
        assert!(desc.contains("cyclomatic complexity"), "desc: {desc}");
        assert!(
            !desc.contains("cognitive"),
            "desc should not mention cognitive: {desc}"
        );
    }

    #[test]
    fn complexity_description_cognitive_only() {
        use fallow_output::{ComplexityViolation, ExceededThreshold, FindingSeverity};
        let root = PathBuf::from("/project");
        let report = fallow_output::HealthReport {
            findings: vec![
                ComplexityViolation {
                    path: root.join("src/fn.ts"),
                    name: "cogFn".to_string(),
                    line: 1,
                    col: 0,
                    cyclomatic: 5,
                    cognitive: 30,
                    line_count: 30,
                    param_count: 1,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
                    exceeded: ExceededThreshold::Cognitive,
                    severity: FindingSeverity::High,
                    crap: None,
                    coverage_pct: None,
                    coverage_tier: None,
                    coverage_source: None,
                    inherited_from: None,
                    component_rollup: None,
                    contributions: vec![],
                    effective_thresholds: None,
                    threshold_source: None,
                }
                .into(),
            ],
            ..Default::default()
        };
        let json = codeclimate_issues_to_value(&api_health_codeclimate_issues(&report, &root));
        let issues = json.as_array().unwrap();
        assert_eq!(issues[0]["check_name"], "fallow/high-cognitive-complexity");
        let desc = issues[0]["description"].as_str().unwrap();
        assert!(desc.contains("cognitive complexity"), "desc: {desc}");
        assert!(!desc.contains("cyclomatic"), "desc: {desc}");
    }

    #[test]
    fn complexity_description_all_threshold_crap_no_coverage() {
        use fallow_output::{ComplexityViolation, ExceededThreshold, FindingSeverity};
        let root = PathBuf::from("/project");
        let report = fallow_output::HealthReport {
            findings: vec![
                ComplexityViolation {
                    path: root.join("src/fn.ts"),
                    name: "allFn".to_string(),
                    line: 1,
                    col: 0,
                    cyclomatic: 30,
                    cognitive: 20,
                    line_count: 80,
                    param_count: 2,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
                    exceeded: ExceededThreshold::All,
                    severity: FindingSeverity::Critical,
                    crap: Some(110.0),
                    coverage_pct: None,
                    coverage_tier: None,
                    coverage_source: None,
                    inherited_from: None,
                    component_rollup: None,
                    contributions: vec![],
                    effective_thresholds: None,
                    threshold_source: None,
                }
                .into(),
            ],
            ..Default::default()
        };
        let json = codeclimate_issues_to_value(&api_health_codeclimate_issues(&report, &root));
        let issues = json.as_array().unwrap();
        assert_eq!(issues[0]["check_name"], "fallow/high-crap-score");
        let desc = issues[0]["description"].as_str().unwrap();
        assert!(desc.contains("CRAP score"), "desc: {desc}");
        assert!(
            !desc.contains("coverage"),
            "no coverage pct should appear: {desc}"
        );
    }

    // ---------------------------------------------------------------------------
    // coverage_intelligence_issue: None identity falls back to "code"
    // ---------------------------------------------------------------------------

    #[test]
    fn coverage_intelligence_issue_none_identity_uses_code_label() {
        use fallow_output::{
            CoverageIntelligenceAction, CoverageIntelligenceConfidence,
            CoverageIntelligenceEvidence, CoverageIntelligenceFinding,
            CoverageIntelligenceMatchConfidence, CoverageIntelligenceRecommendation,
            CoverageIntelligenceReport, CoverageIntelligenceSchemaVersion,
            CoverageIntelligenceSummary, CoverageIntelligenceVerdict,
        };
        let root = PathBuf::from("/project");
        let report = fallow_output::HealthReport {
            coverage_intelligence: Some(CoverageIntelligenceReport {
                schema_version: CoverageIntelligenceSchemaVersion::V1,
                verdict: CoverageIntelligenceVerdict::ReviewRequired,
                summary: CoverageIntelligenceSummary {
                    findings: 1,
                    ..Default::default()
                },
                findings: vec![CoverageIntelligenceFinding {
                    id: "fallow:coverage-intel:xyz".to_owned(),
                    path: root.join("src/util.ts"),
                    identity: None,
                    line: 3,
                    verdict: CoverageIntelligenceVerdict::ReviewRequired,
                    signals: vec![],
                    recommendation: CoverageIntelligenceRecommendation::ReviewBeforeChanging,
                    confidence: CoverageIntelligenceConfidence::Low,
                    related_ids: vec![],
                    evidence: CoverageIntelligenceEvidence {
                        match_confidence: CoverageIntelligenceMatchConfidence::Direct,
                        ..Default::default()
                    },
                    actions: vec![CoverageIntelligenceAction {
                        kind: "review".to_owned(),
                        description: "Review before changing".to_owned(),
                        auto_fixable: false,
                    }],
                }],
            }),
            ..Default::default()
        };
        let json = codeclimate_issues_to_value(&api_health_codeclimate_issues(&report, &root));
        let issues = json.as_array().unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(
            issues[0]["check_name"],
            "fallow/coverage-intelligence-review"
        );
        let desc = issues[0]["description"].as_str().unwrap();
        assert!(
            desc.contains("'code'"),
            "description should say 'code': {desc}"
        );
    }

    // ---------------------------------------------------------------------------
    // coverage_intelligence_issue: Clean/Unknown verdicts are filtered out
    // ---------------------------------------------------------------------------

    #[test]
    fn coverage_intelligence_clean_verdict_emits_no_issue() {
        use fallow_output::{
            CoverageIntelligenceAction, CoverageIntelligenceConfidence,
            CoverageIntelligenceEvidence, CoverageIntelligenceFinding,
            CoverageIntelligenceMatchConfidence, CoverageIntelligenceRecommendation,
            CoverageIntelligenceReport, CoverageIntelligenceSchemaVersion,
            CoverageIntelligenceSummary, CoverageIntelligenceVerdict,
        };
        let root = PathBuf::from("/project");
        let report = fallow_output::HealthReport {
            coverage_intelligence: Some(CoverageIntelligenceReport {
                schema_version: CoverageIntelligenceSchemaVersion::V1,
                verdict: CoverageIntelligenceVerdict::Clean,
                summary: CoverageIntelligenceSummary {
                    findings: 1,
                    ..Default::default()
                },
                findings: vec![CoverageIntelligenceFinding {
                    id: "fallow:coverage-intel:clean".to_owned(),
                    path: root.join("src/util.ts"),
                    identity: Some("cleanFn".to_owned()),
                    line: 3,
                    verdict: CoverageIntelligenceVerdict::Clean,
                    signals: vec![],
                    recommendation: CoverageIntelligenceRecommendation::ReviewBeforeChanging,
                    confidence: CoverageIntelligenceConfidence::Low,
                    related_ids: vec![],
                    evidence: CoverageIntelligenceEvidence {
                        match_confidence: CoverageIntelligenceMatchConfidence::Direct,
                        ..Default::default()
                    },
                    actions: vec![CoverageIntelligenceAction {
                        kind: "review".to_owned(),
                        description: "Review before changing".to_owned(),
                        auto_fixable: false,
                    }],
                }],
            }),
            ..Default::default()
        };
        let issues = api_health_codeclimate_issues(&report, &root);
        assert!(
            issues.is_empty(),
            "Clean verdict should not produce an issue"
        );
    }

    // ---------------------------------------------------------------------------
    // untested_file_issue: singular vs plural export count in description
    // ---------------------------------------------------------------------------

    #[test]
    fn untested_file_singular_export_count() {
        use fallow_output::{CoverageGapSummary, CoverageGaps, UntestedFile, UntestedFileFinding};
        let root = PathBuf::from("/project");
        let report = fallow_output::HealthReport {
            coverage_gaps: Some(CoverageGaps {
                summary: CoverageGapSummary {
                    runtime_files: 1,
                    covered_files: 0,
                    file_coverage_pct: 0.0,
                    untested_files: 1,
                    untested_exports: 0,
                },
                files: vec![UntestedFileFinding::with_actions(
                    UntestedFile {
                        path: root.join("src/single.ts"),
                        value_export_count: 1,
                    },
                    &root,
                )],
                exports: vec![],
            }),
            ..Default::default()
        };
        let json = codeclimate_issues_to_value(&api_health_codeclimate_issues(&report, &root));
        let issues = json.as_array().unwrap();
        assert_eq!(issues.len(), 1);
        let desc = issues[0]["description"].as_str().unwrap();
        assert!(desc.contains("1 value export)"), "singular: {desc}");
        assert!(
            !desc.contains("exports"),
            "singular should not say 'exports': {desc}"
        );
    }

    #[test]
    fn untested_file_plural_export_count() {
        use fallow_output::{CoverageGapSummary, CoverageGaps, UntestedFile, UntestedFileFinding};
        let root = PathBuf::from("/project");
        let report = fallow_output::HealthReport {
            coverage_gaps: Some(CoverageGaps {
                summary: CoverageGapSummary {
                    runtime_files: 1,
                    covered_files: 0,
                    file_coverage_pct: 0.0,
                    untested_files: 1,
                    untested_exports: 0,
                },
                files: vec![UntestedFileFinding::with_actions(
                    UntestedFile {
                        path: root.join("src/multi.ts"),
                        value_export_count: 3,
                    },
                    &root,
                )],
                exports: vec![],
            }),
            ..Default::default()
        };
        let json = codeclimate_issues_to_value(&api_health_codeclimate_issues(&report, &root));
        let issues = json.as_array().unwrap();
        let desc = issues[0]["description"].as_str().unwrap();
        assert!(desc.contains("3 value exports)"), "plural: {desc}");
    }

    // ---------------------------------------------------------------------------
    // health_finding_severity: Critical -> Critical, Moderate -> Minor
    // ---------------------------------------------------------------------------

    #[test]
    fn health_finding_severity_critical_maps_to_critical() {
        use fallow_output::FindingSeverity;
        assert_eq!(
            health_finding_severity(FindingSeverity::Critical),
            CodeClimateSeverity::Critical
        );
    }

    #[test]
    fn health_finding_severity_moderate_maps_to_minor() {
        use fallow_output::FindingSeverity;
        assert_eq!(
            health_finding_severity(FindingSeverity::Moderate),
            CodeClimateSeverity::Minor
        );
    }

    // ---------------------------------------------------------------------------
    // runtime_coverage_check_name: all verdict arms
    // ---------------------------------------------------------------------------

    #[test]
    fn runtime_coverage_check_name_safe_to_delete() {
        use fallow_output::RuntimeCoverageVerdict;
        assert_eq!(
            runtime_coverage_check_name(RuntimeCoverageVerdict::SafeToDelete),
            "fallow/runtime-safe-to-delete"
        );
    }

    #[test]
    fn runtime_coverage_check_name_low_traffic() {
        use fallow_output::RuntimeCoverageVerdict;
        assert_eq!(
            runtime_coverage_check_name(RuntimeCoverageVerdict::LowTraffic),
            "fallow/runtime-low-traffic"
        );
    }

    #[test]
    fn runtime_coverage_check_name_coverage_unavailable() {
        use fallow_output::RuntimeCoverageVerdict;
        assert_eq!(
            runtime_coverage_check_name(RuntimeCoverageVerdict::CoverageUnavailable),
            "fallow/runtime-coverage-unavailable"
        );
    }

    #[test]
    fn runtime_coverage_check_name_active_and_unknown_use_generic() {
        use fallow_output::RuntimeCoverageVerdict;
        assert_eq!(
            runtime_coverage_check_name(RuntimeCoverageVerdict::Active),
            "fallow/runtime-coverage"
        );
        assert_eq!(
            runtime_coverage_check_name(RuntimeCoverageVerdict::Unknown),
            "fallow/runtime-coverage"
        );
    }

    // ---------------------------------------------------------------------------
    // runtime_coverage_severity: verdict arms
    // ---------------------------------------------------------------------------

    #[test]
    fn runtime_coverage_severity_safe_to_delete_is_critical() {
        use fallow_output::RuntimeCoverageVerdict;
        assert_eq!(
            runtime_coverage_severity(RuntimeCoverageVerdict::SafeToDelete),
            CodeClimateSeverity::Critical
        );
    }

    #[test]
    fn runtime_coverage_severity_review_required_is_major() {
        use fallow_output::RuntimeCoverageVerdict;
        assert_eq!(
            runtime_coverage_severity(RuntimeCoverageVerdict::ReviewRequired),
            CodeClimateSeverity::Major
        );
    }

    #[test]
    fn runtime_coverage_severity_other_verdicts_are_minor() {
        use fallow_output::RuntimeCoverageVerdict;
        assert_eq!(
            runtime_coverage_severity(RuntimeCoverageVerdict::LowTraffic),
            CodeClimateSeverity::Minor
        );
        assert_eq!(
            runtime_coverage_severity(RuntimeCoverageVerdict::Active),
            CodeClimateSeverity::Minor
        );
    }

    // ---------------------------------------------------------------------------
    // coverage_intelligence_severity: all verdict arms
    // ---------------------------------------------------------------------------

    #[test]
    fn coverage_intelligence_severity_review_required_is_minor() {
        use fallow_output::CoverageIntelligenceVerdict;
        assert_eq!(
            coverage_intelligence_severity(CoverageIntelligenceVerdict::ReviewRequired),
            Some(CodeClimateSeverity::Minor)
        );
    }

    #[test]
    fn coverage_intelligence_severity_refactor_carefully_is_minor() {
        use fallow_output::CoverageIntelligenceVerdict;
        assert_eq!(
            coverage_intelligence_severity(CoverageIntelligenceVerdict::RefactorCarefully),
            Some(CodeClimateSeverity::Minor)
        );
    }

    #[test]
    fn coverage_intelligence_severity_clean_is_none() {
        use fallow_output::CoverageIntelligenceVerdict;
        assert_eq!(
            coverage_intelligence_severity(CoverageIntelligenceVerdict::Clean),
            None
        );
    }

    #[test]
    fn coverage_intelligence_severity_unknown_is_none() {
        use fallow_output::CoverageIntelligenceVerdict;
        assert_eq!(
            coverage_intelligence_severity(CoverageIntelligenceVerdict::Unknown),
            None
        );
    }

    // ---------------------------------------------------------------------------
    // push_dep_cc_issues: workspace context included in description
    // ---------------------------------------------------------------------------

    #[test]
    fn unused_dep_with_workspace_context_includes_workspace_in_description() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "shared-lib".to_string(),
                location: DependencyLocation::Dependencies,
                path: root.join("package.json"),
                line: 10,
                used_in_workspaces: vec![root.join("packages/app")],
            }));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        let desc = output[0]["description"].as_str().unwrap();
        assert!(
            desc.contains("imported in other workspaces"),
            "desc: {desc}"
        );
        assert!(desc.contains("packages/app"), "desc: {desc}");
    }

    // ---------------------------------------------------------------------------
    // push_private_type_leak_issues
    // ---------------------------------------------------------------------------

    #[test]
    fn private_type_leak_emits_correct_check_name_and_description() {
        use fallow_config::Severity;
        use fallow_types::output_dead_code::PrivateTypeLeakFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .private_type_leaks
            .push(PrivateTypeLeakFinding::with_actions(PrivateTypeLeak {
                path: root.join("src/api.ts"),
                export_name: "ApiResponse".to_string(),
                type_name: "InternalState".to_string(),
                line: 7,
                col: 0,
                span_start: 0,
            }));
        // private_type_leaks defaults to Off; enable it so the issue is emitted.
        let rules = RulesConfig {
            private_type_leaks: Severity::Error,
            ..RulesConfig::default()
        };
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        let arr = output.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["check_name"], "fallow/private-type-leak");
        let desc = arr[0]["description"].as_str().unwrap();
        assert!(desc.contains("ApiResponse"), "desc: {desc}");
        assert!(desc.contains("InternalState"), "desc: {desc}");
        assert_eq!(arr[0]["location"]["lines"]["begin"], 7);
    }

    // ---------------------------------------------------------------------------
    // push_type_only_dep_issues / push_test_only_dep_issues: zero line -> line 1
    // ---------------------------------------------------------------------------

    #[test]
    fn type_only_dep_zero_line_defaults_to_1() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .type_only_dependencies
            .push(TypeOnlyDependencyFinding::with_actions(
                TypeOnlyDependency {
                    package_name: "ts-types".to_string(),
                    path: root.join("package.json"),
                    line: 0,
                },
            ));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["location"]["lines"]["begin"], 1);
    }

    #[test]
    fn test_only_dep_zero_line_defaults_to_1() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .test_only_dependencies
            .push(TestOnlyDependencyFinding::with_actions(
                TestOnlyDependency {
                    package_name: "vitest".to_string(),
                    path: root.join("package.json"),
                    line: 0,
                },
            ));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/test-only-dependency");
        assert_eq!(output[0]["location"]["lines"]["begin"], 1);
    }

    // ---------------------------------------------------------------------------
    // push_re_export_cycle_issues: self-loop and multi-node arms
    // ---------------------------------------------------------------------------

    #[test]
    fn re_export_cycle_self_loop_description_contains_self_loop_tag() {
        use fallow_types::output_dead_code::ReExportCycleFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .re_export_cycles
            .push(ReExportCycleFinding::with_actions(ReExportCycle {
                files: vec![root.join("src/index.ts")],
                kind: ReExportCycleKind::SelfLoop,
            }));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/re-export-cycle");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("(self-loop)"), "desc: {desc}");
    }

    #[test]
    fn re_export_cycle_multi_node_description_has_no_kind_tag() {
        use fallow_types::output_dead_code::ReExportCycleFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .re_export_cycles
            .push(ReExportCycleFinding::with_actions(ReExportCycle {
                files: vec![root.join("src/a.ts"), root.join("src/b.ts")],
                kind: ReExportCycleKind::MultiNode,
            }));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.starts_with("Re-export cycle: "), "desc: {desc}");
        assert!(!desc.contains("(self-loop)"), "desc: {desc}");
        assert!(!desc.contains("(multi-node)"), "desc: {desc}");
        assert!(desc.contains("src/a.ts"), "desc: {desc}");
    }

    // ---------------------------------------------------------------------------
    // push_boundary_coverage_issues and push_boundary_call_issues
    // ---------------------------------------------------------------------------

    #[test]
    fn boundary_coverage_violation_emits_correct_check_name() {
        use fallow_types::output_dead_code::BoundaryCoverageViolationFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .boundary_coverage_violations
            .push(BoundaryCoverageViolationFinding::with_actions(
                BoundaryCoverageViolation {
                    path: root.join("src/orphan.ts"),
                    line: 0,
                    col: 0,
                },
            ));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/boundary-coverage");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("no configured zone"), "desc: {desc}");
    }

    #[test]
    fn boundary_call_violation_emits_pattern_in_description() {
        use fallow_types::output_dead_code::BoundaryCallViolationFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .boundary_call_violations
            .push(BoundaryCallViolationFinding::with_actions(
                BoundaryCallViolation {
                    path: root.join("src/ui/App.ts"),
                    line: 5,
                    col: 0,
                    zone: "ui".to_string(),
                    callee: "fs.readFile".to_string(),
                    pattern: "fs.*".to_string(),
                },
            ));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/boundary-call-violation");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("fs.readFile"), "desc: {desc}");
        assert!(desc.contains("fs.*"), "desc: {desc}");
        assert!(desc.contains("'ui'"), "desc: {desc}");
    }

    // ---------------------------------------------------------------------------
    // push_policy_violation_issues: error severity and optional message
    // ---------------------------------------------------------------------------

    #[test]
    fn policy_violation_error_severity_maps_to_major() {
        use fallow_types::output_dead_code::PolicyViolationFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .policy_violations
            .push(PolicyViolationFinding::with_actions(PolicyViolation {
                path: root.join("src/service.ts"),
                line: 2,
                col: 0,
                pack: "security".to_string(),
                rule_id: "no-eval".to_string(),
                kind: PolicyRuleKind::BannedCall,
                matched: "eval".to_string(),
                severity: PolicyViolationSeverity::Error,
                message: None,
            }));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/policy-violation");
        assert_eq!(output[0]["severity"], "major");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("eval"), "desc: {desc}");
        assert!(desc.contains("security/no-eval"), "desc: {desc}");
    }

    #[test]
    fn policy_violation_warn_severity_maps_to_minor() {
        use fallow_types::output_dead_code::PolicyViolationFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .policy_violations
            .push(PolicyViolationFinding::with_actions(PolicyViolation {
                path: root.join("src/service.ts"),
                line: 2,
                col: 0,
                pack: "style".to_string(),
                rule_id: "no-console".to_string(),
                kind: PolicyRuleKind::BannedCall,
                matched: "console.log".to_string(),
                severity: PolicyViolationSeverity::Warn,
                message: Some("Use the project logger instead".to_string()),
            }));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["severity"], "minor");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(
            desc.contains("Use the project logger instead"),
            "desc: {desc}"
        );
    }

    // ---------------------------------------------------------------------------
    // push_invalid_client_export_issues and push_mixed_client_server_barrel_issues
    // ---------------------------------------------------------------------------

    #[test]
    fn invalid_client_export_emits_correct_check_name_and_directive() {
        use fallow_types::output_dead_code::InvalidClientExportFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .invalid_client_exports
            .push(InvalidClientExportFinding::with_actions(
                InvalidClientExport {
                    path: root.join("src/page.ts"),
                    export_name: "metadata".to_string(),
                    directive: "use client".to_string(),
                    line: 3,
                    col: 0,
                },
            ));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/invalid-client-export");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("metadata"), "desc: {desc}");
        assert!(desc.contains("use client"), "desc: {desc}");
    }

    #[test]
    fn mixed_client_server_barrel_emits_correct_check_name_and_origins() {
        use fallow_types::output_dead_code::MixedClientServerBarrelFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .mixed_client_server_barrels
            .push(MixedClientServerBarrelFinding::with_actions(
                MixedClientServerBarrel {
                    path: root.join("src/index.ts"),
                    client_origin: "./client-comp".to_string(),
                    server_origin: "./server-only".to_string(),
                    line: 1,
                    col: 0,
                },
            ));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/mixed-client-server-barrel");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("./client-comp"), "desc: {desc}");
        assert!(desc.contains("./server-only"), "desc: {desc}");
    }

    // ---------------------------------------------------------------------------
    // push_misplaced_directive_issues
    // ---------------------------------------------------------------------------

    #[test]
    fn misplaced_directive_emits_correct_check_name_and_guidance() {
        use fallow_types::output_dead_code::MisplacedDirectiveFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .misplaced_directives
            .push(MisplacedDirectiveFinding::with_actions(
                MisplacedDirective {
                    path: root.join("src/page.ts"),
                    directive: "use client".to_string(),
                    line: 5,
                    col: 0,
                },
            ));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/misplaced-directive");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("use client"), "desc: {desc}");
        assert!(desc.contains("leading position"), "desc: {desc}");
    }

    // ---------------------------------------------------------------------------
    // push_unrendered_component_issues
    // ---------------------------------------------------------------------------

    #[test]
    fn unrendered_component_emits_correct_check_name_and_description() {
        use fallow_types::output_dead_code::UnrenderedComponentFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unrendered_components
            .push(UnrenderedComponentFinding::with_actions(
                UnrenderedComponent {
                    path: root.join("src/Dead.vue"),
                    component_name: "Dead".to_string(),
                    framework: "vue".to_string(),
                    reachable_via: None,
                    line: 1,
                    col: 0,
                },
            ));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/unrendered-component");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("`Dead`"), "desc: {desc}");
        assert!(desc.contains("rendered nowhere"), "desc: {desc}");
    }

    // ---------------------------------------------------------------------------
    // push_unused_component_prop_issues
    // ---------------------------------------------------------------------------

    #[test]
    fn unused_component_prop_emits_correct_check_name_and_description() {
        use fallow_types::output_dead_code::UnusedComponentPropFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_component_props
            .push(UnusedComponentPropFinding::with_actions(
                UnusedComponentProp {
                    path: root.join("src/Card.vue"),
                    component_name: "Card".to_string(),
                    prop_name: "color".to_string(),
                    line: 3,
                    col: 0,
                },
            ));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/unused-component-prop");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("`color`"), "desc: {desc}");
        assert!(desc.contains("Card"), "desc: {desc}");
    }

    // ---------------------------------------------------------------------------
    // push_unused_component_emit_issues
    // ---------------------------------------------------------------------------

    #[test]
    fn unused_component_emit_emits_correct_check_name_and_description() {
        use fallow_types::output_dead_code::UnusedComponentEmitFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_component_emits
            .push(UnusedComponentEmitFinding::with_actions(
                UnusedComponentEmit {
                    path: root.join("src/Button.vue"),
                    component_name: "Button".to_string(),
                    emit_name: "close".to_string(),
                    line: 4,
                    col: 0,
                },
            ));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/unused-component-emit");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("`close`"), "desc: {desc}");
        assert!(desc.contains("Button"), "desc: {desc}");
    }

    // ---------------------------------------------------------------------------
    // push_unused_svelte_event_issues
    // ---------------------------------------------------------------------------

    #[test]
    fn unused_svelte_event_emits_correct_check_name_and_description() {
        use fallow_types::output_dead_code::UnusedSvelteEventFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_svelte_events
            .push(UnusedSvelteEventFinding::with_actions(UnusedSvelteEvent {
                path: root.join("src/Child.svelte"),
                component_name: "Child".to_string(),
                event_name: "submit".to_string(),
                line: 2,
                col: 0,
            }));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/unused-svelte-event");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("`submit`"), "desc: {desc}");
        assert!(desc.contains("listened to nowhere"), "desc: {desc}");
    }

    // ---------------------------------------------------------------------------
    // push_unused_component_input_issues and push_unused_component_output_issues
    // ---------------------------------------------------------------------------

    #[test]
    fn unused_component_input_emits_correct_check_name_and_description() {
        use fallow_types::output_dead_code::UnusedComponentInputFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_component_inputs
            .push(UnusedComponentInputFinding::with_actions(
                UnusedComponentInput {
                    path: root.join("src/card.component.ts"),
                    component_name: "CardComponent".to_string(),
                    input_name: "label".to_string(),
                    line: 5,
                    col: 0,
                },
            ));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/unused-component-input");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("`label`"), "desc: {desc}");
        assert!(desc.contains("CardComponent"), "desc: {desc}");
    }

    #[test]
    fn unused_component_output_emits_correct_check_name_and_description() {
        use fallow_types::output_dead_code::UnusedComponentOutputFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_component_outputs
            .push(UnusedComponentOutputFinding::with_actions(
                UnusedComponentOutput {
                    path: root.join("src/toggle.component.ts"),
                    component_name: "ToggleComponent".to_string(),
                    output_name: "changed".to_string(),
                    line: 6,
                    col: 0,
                },
            ));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/unused-component-output");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("`changed`"), "desc: {desc}");
        assert!(desc.contains("ToggleComponent"), "desc: {desc}");
    }

    // ---------------------------------------------------------------------------
    // push_unused_server_action_issues
    // ---------------------------------------------------------------------------

    #[test]
    fn unused_server_action_emits_correct_check_name_and_description() {
        use fallow_types::output_dead_code::UnusedServerActionFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_server_actions
            .push(UnusedServerActionFinding::with_actions(
                UnusedServerAction {
                    path: root.join("src/app/actions.ts"),
                    action_name: "deleteUser".to_string(),
                    line: 8,
                    col: 0,
                },
            ));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/unused-server-action");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("`deleteUser`"), "desc: {desc}");
        assert!(desc.contains("\"use server\""), "desc: {desc}");
    }

    // ---------------------------------------------------------------------------
    // push_unused_load_data_key_issues
    // ---------------------------------------------------------------------------

    #[test]
    fn unused_load_data_key_emits_correct_check_name_and_description() {
        use fallow_types::output_dead_code::UnusedLoadDataKeyFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_load_data_keys
            .push(UnusedLoadDataKeyFinding::with_actions(UnusedLoadDataKey {
                path: root.join("src/routes/+page.ts"),
                key_name: "postCount".to_string(),
                line: 7,
                col: 0,
                route_dir: None,
            }));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/unused-load-data-key");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("`postCount`"), "desc: {desc}");
        assert!(desc.contains("no consumer"), "desc: {desc}");
    }

    // ---------------------------------------------------------------------------
    // push_route_collision_issues and push_dynamic_segment_name_conflict_issues
    // ---------------------------------------------------------------------------

    #[test]
    fn route_collision_emits_correct_check_name_and_url_in_description() {
        use fallow_types::output_dead_code::RouteCollisionFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .route_collisions
            .push(RouteCollisionFinding::with_actions(RouteCollision {
                path: root.join("src/app/about/page.tsx"),
                url: "/about".to_string(),
                conflicting_paths: vec![root.join("src/app/(marketing)/about/page.tsx")],
                line: 1,
                col: 0,
            }));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/route-collision");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("`/about`"), "desc: {desc}");
        assert!(desc.contains("1 other file"), "desc: {desc}");
    }

    #[test]
    fn dynamic_segment_name_conflict_emits_correct_check_name_and_position() {
        use fallow_types::output_dead_code::DynamicSegmentNameConflictFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.dynamic_segment_name_conflicts.push(
            DynamicSegmentNameConflictFinding::with_actions(DynamicSegmentNameConflict {
                path: root.join("src/app/shop/[id]/page.tsx"),
                position: "/shop".to_string(),
                conflicting_segments: vec!["[id]".to_string(), "[slug]".to_string()],
                conflicting_paths: vec![root.join("src/app/shop/[slug]/page.tsx")],
                line: 1,
                col: 0,
            }),
        );
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(
            output[0]["check_name"],
            "fallow/dynamic-segment-name-conflict"
        );
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("`/shop`"), "desc: {desc}");
        assert!(desc.contains("[id]"), "desc: {desc}");
        assert!(desc.contains("[slug]"), "desc: {desc}");
    }

    // ---------------------------------------------------------------------------
    // push_unresolved_catalog_reference_issues: default and named catalog
    // ---------------------------------------------------------------------------

    #[test]
    fn unresolved_catalog_reference_default_catalog_description() {
        use fallow_types::output_dead_code::UnresolvedCatalogReferenceFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unresolved_catalog_references.push(
            UnresolvedCatalogReferenceFinding::with_actions(UnresolvedCatalogReference {
                entry_name: "react".to_string(),
                catalog_name: "default".to_string(),
                path: root.join("packages/app/package.json"),
                line: 5,
                available_in_catalogs: vec![],
            }),
        );
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(
            output[0]["check_name"],
            "fallow/unresolved-catalog-reference"
        );
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("the default catalog"), "desc: {desc}");
        assert!(desc.contains("react"), "desc: {desc}");
        assert!(desc.contains("catalog:"), "desc: {desc}");
    }

    #[test]
    fn unresolved_catalog_reference_named_catalog_description() {
        use fallow_types::output_dead_code::UnresolvedCatalogReferenceFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unresolved_catalog_references.push(
            UnresolvedCatalogReferenceFinding::with_actions(UnresolvedCatalogReference {
                entry_name: "lodash".to_string(),
                catalog_name: "react17".to_string(),
                path: root.join("packages/legacy/package.json"),
                line: 3,
                available_in_catalogs: vec!["default".to_string()],
            }),
        );
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("catalog 'react17'"), "desc: {desc}");
        assert!(desc.contains("available in"), "desc: {desc}");
        assert!(desc.contains("default"), "desc: {desc}");
    }

    // ---------------------------------------------------------------------------
    // push_unused_dependency_override_issues: hint included in description
    // ---------------------------------------------------------------------------

    #[test]
    fn unused_dependency_override_with_hint_includes_hint_in_description() {
        use fallow_types::output_dead_code::UnusedDependencyOverrideFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_dependency_overrides
            .push(UnusedDependencyOverrideFinding::with_actions(
                UnusedDependencyOverride {
                    raw_key: "react".to_string(),
                    target_package: "react".to_string(),
                    parent_package: None,
                    version_constraint: None,
                    version_range: "^18.0.0".to_string(),
                    source: DependencyOverrideSource::PnpmPackageJson,
                    path: root.join("package.json"),
                    line: 12,
                    hint: Some("verify lockfile before removing".to_string()),
                },
            ));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/unused-dependency-override");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("react"), "desc: {desc}");
        assert!(desc.contains("verify lockfile"), "desc: {desc}");
    }

    // ---------------------------------------------------------------------------
    // api_health_codeclimate_issues: runtime coverage findings
    // ---------------------------------------------------------------------------

    #[test]
    fn health_codeclimate_runtime_safe_to_delete_maps_to_critical_severity() {
        use fallow_output::{
            RuntimeCoverageConfidence, RuntimeCoverageDataSource, RuntimeCoverageEvidence,
            RuntimeCoverageFinding, RuntimeCoverageReport, RuntimeCoverageReportVerdict,
            RuntimeCoverageSchemaVersion, RuntimeCoverageSummary, RuntimeCoverageVerdict,
        };
        let root = PathBuf::from("/project");
        let report = fallow_output::HealthReport {
            runtime_coverage: Some(RuntimeCoverageReport {
                schema_version: RuntimeCoverageSchemaVersion::V1,
                verdict: RuntimeCoverageReportVerdict::ColdCodeDetected,
                signals: vec![],
                summary: RuntimeCoverageSummary {
                    data_source: RuntimeCoverageDataSource::Local,
                    last_received_at: None,
                    functions_tracked: 10,
                    functions_hit: 8,
                    functions_unhit: 2,
                    functions_untracked: 0,
                    coverage_percent: 80.0,
                    trace_count: 1000,
                    period_days: 7,
                    deployments_seen: 1,
                    capture_quality: None,
                },
                findings: vec![RuntimeCoverageFinding {
                    id: "fallow:prod:aabbccdd".to_string(),
                    stable_id: None,
                    source_hash: None,
                    path: root.join("src/legacy.ts"),
                    function: "legacyHelper".to_string(),
                    line: 12,
                    verdict: RuntimeCoverageVerdict::SafeToDelete,
                    invocations: Some(0),
                    confidence: RuntimeCoverageConfidence::High,
                    evidence: RuntimeCoverageEvidence {
                        static_status: "unused".to_string(),
                        test_coverage: "not_covered".to_string(),
                        v8_tracking: "tracked".to_string(),
                        untracked_reason: None,
                        observation_days: 30,
                        deployments_observed: 3,
                    },
                    actions: vec![],
                    discriminators: None,
                }],
                hot_paths: vec![],
                blast_radius: vec![],
                importance: vec![],
                watermark: None,
                warnings: vec![],
                actionable: true,
                actionability_reason: None,
                actionability_verdict: None,
                provenance: fallow_output::RuntimeCoverageProvenance::default(),
            }),
            ..Default::default()
        };
        let json = codeclimate_issues_to_value(&api_health_codeclimate_issues(&report, &root));
        let issues = json.as_array().unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0]["check_name"], "fallow/runtime-safe-to-delete");
        assert_eq!(issues[0]["severity"], "critical");
        let desc = issues[0]["description"].as_str().unwrap();
        assert!(desc.contains("legacyHelper"), "desc: {desc}");
        assert!(desc.contains("safe to delete"), "desc: {desc}");
        assert!(desc.contains("0 invocations"), "desc: {desc}");
        assert_eq!(issues[0]["location"]["path"], "src/legacy.ts");
        assert_eq!(issues[0]["location"]["lines"]["begin"], 12);
    }

    #[test]
    fn health_codeclimate_runtime_finding_with_none_invocations_shows_untracked() {
        use fallow_output::{
            RuntimeCoverageConfidence, RuntimeCoverageDataSource, RuntimeCoverageEvidence,
            RuntimeCoverageFinding, RuntimeCoverageReport, RuntimeCoverageReportVerdict,
            RuntimeCoverageSchemaVersion, RuntimeCoverageSummary, RuntimeCoverageVerdict,
        };
        let root = PathBuf::from("/project");
        let report = fallow_output::HealthReport {
            runtime_coverage: Some(RuntimeCoverageReport {
                schema_version: RuntimeCoverageSchemaVersion::V1,
                verdict: RuntimeCoverageReportVerdict::ColdCodeDetected,
                signals: vec![],
                summary: RuntimeCoverageSummary {
                    data_source: RuntimeCoverageDataSource::Local,
                    last_received_at: None,
                    functions_tracked: 5,
                    functions_hit: 4,
                    functions_unhit: 0,
                    functions_untracked: 1,
                    coverage_percent: 80.0,
                    trace_count: 500,
                    period_days: 7,
                    deployments_seen: 1,
                    capture_quality: None,
                },
                findings: vec![RuntimeCoverageFinding {
                    id: "fallow:prod:11223344".to_string(),
                    stable_id: None,
                    source_hash: None,
                    path: root.join("src/worker.ts"),
                    function: "workerFn".to_string(),
                    line: 5,
                    verdict: RuntimeCoverageVerdict::CoverageUnavailable,
                    invocations: None,
                    confidence: RuntimeCoverageConfidence::Low,
                    evidence: RuntimeCoverageEvidence {
                        static_status: "used".to_string(),
                        test_coverage: "not_covered".to_string(),
                        v8_tracking: "untracked".to_string(),
                        untracked_reason: Some("worker_thread".to_string()),
                        observation_days: 7,
                        deployments_observed: 1,
                    },
                    actions: vec![],
                    discriminators: None,
                }],
                hot_paths: vec![],
                blast_radius: vec![],
                importance: vec![],
                watermark: None,
                warnings: vec![],
                actionable: true,
                actionability_reason: None,
                actionability_verdict: None,
                provenance: fallow_output::RuntimeCoverageProvenance::default(),
            }),
            ..Default::default()
        };
        let json = codeclimate_issues_to_value(&api_health_codeclimate_issues(&report, &root));
        let issues = json.as_array().unwrap();
        let desc = issues[0]["description"].as_str().unwrap();
        assert!(desc.contains("untracked"), "desc: {desc}");
    }

    // ---------------------------------------------------------------------------
    // api_duplication_codeclimate_issues
    // ---------------------------------------------------------------------------

    #[test]
    fn duplication_codeclimate_one_issue_per_instance() {
        use fallow_types::duplicates::{
            CloneGroup, CloneInstance, DuplicationReport, DuplicationStats,
        };
        let root = PathBuf::from("/project");
        let report = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![
                    CloneInstance {
                        file: root.join("src/a.ts"),
                        start_line: 10,
                        end_line: 20,
                        start_col: 0,
                        end_col: 0,
                        fragment: "const x = 1;\nconst y = 2;".to_string(),
                    },
                    CloneInstance {
                        file: root.join("src/b.ts"),
                        start_line: 30,
                        end_line: 40,
                        start_col: 0,
                        end_col: 0,
                        fragment: "const x = 1;\nconst y = 2;".to_string(),
                    },
                ],
                token_count: 20,
                line_count: 10,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats::default(),
        };
        let output =
            codeclimate_issues_to_value(&api_duplication_codeclimate_issues(&report, &root));
        let issues = output.as_array().unwrap();
        assert_eq!(issues.len(), 2, "one issue per clone instance");
        assert_eq!(issues[0]["check_name"], "fallow/code-duplication");
        assert_eq!(issues[1]["check_name"], "fallow/code-duplication");
        assert_eq!(issues[0]["location"]["path"].as_str().unwrap(), "src/a.ts");
        assert_eq!(issues[1]["location"]["path"].as_str().unwrap(), "src/b.ts");
        assert_eq!(issues[0]["location"]["lines"]["begin"], 10);
        assert_eq!(issues[1]["location"]["lines"]["begin"], 30);
        assert_eq!(issues[0]["categories"][0], "Duplication");
    }

    #[test]
    fn duplication_codeclimate_description_includes_group_number_and_line_count() {
        use fallow_types::duplicates::{
            CloneGroup, CloneInstance, DuplicationReport, DuplicationStats,
        };
        let root = PathBuf::from("/project");
        let report = DuplicationReport {
            clone_groups: vec![
                CloneGroup {
                    instances: vec![CloneInstance {
                        file: root.join("src/a.ts"),
                        start_line: 1,
                        end_line: 10,
                        start_col: 0,
                        end_col: 0,
                        fragment: "x".to_string(),
                    }],
                    token_count: 5,
                    line_count: 8,
                },
                CloneGroup {
                    instances: vec![CloneInstance {
                        file: root.join("src/b.ts"),
                        start_line: 5,
                        end_line: 12,
                        start_col: 0,
                        end_col: 0,
                        fragment: "y".to_string(),
                    }],
                    token_count: 3,
                    line_count: 6,
                },
            ],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats::default(),
        };
        let output =
            codeclimate_issues_to_value(&api_duplication_codeclimate_issues(&report, &root));
        let issues = output.as_array().unwrap();
        assert_eq!(issues.len(), 2);
        let desc0 = issues[0]["description"].as_str().unwrap();
        assert!(desc0.contains("group 1"), "first group: {desc0}");
        assert!(desc0.contains("8 lines"), "first group line count: {desc0}");
        let desc1 = issues[1]["description"].as_str().unwrap();
        assert!(desc1.contains("group 2"), "second group: {desc1}");
        assert!(
            desc1.contains("6 lines"),
            "second group line count: {desc1}"
        );
    }

    #[test]
    fn duplication_codeclimate_empty_report_produces_empty_array() {
        use fallow_types::duplicates::{DuplicationReport, DuplicationStats};
        let root = PathBuf::from("/project");
        let report = DuplicationReport {
            clone_groups: vec![],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats::default(),
        };
        let output =
            codeclimate_issues_to_value(&api_duplication_codeclimate_issues(&report, &root));
        assert!(output.as_array().unwrap().is_empty());
    }

    #[test]
    fn duplication_codeclimate_fingerprints_are_unique_across_instances() {
        use fallow_types::duplicates::{
            CloneGroup, CloneInstance, DuplicationReport, DuplicationStats,
        };
        let root = PathBuf::from("/project");
        let report = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![
                    CloneInstance {
                        file: root.join("src/x.ts"),
                        start_line: 1,
                        end_line: 5,
                        start_col: 0,
                        end_col: 0,
                        fragment: "hello".to_string(),
                    },
                    CloneInstance {
                        file: root.join("src/y.ts"),
                        start_line: 10,
                        end_line: 14,
                        start_col: 0,
                        end_col: 0,
                        fragment: "hello".to_string(),
                    },
                ],
                token_count: 5,
                line_count: 4,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats::default(),
        };
        let output =
            codeclimate_issues_to_value(&api_duplication_codeclimate_issues(&report, &root));
        let issues = output.as_array().unwrap();
        let fp0 = issues[0]["fingerprint"].as_str().unwrap();
        let fp1 = issues[1]["fingerprint"].as_str().unwrap();
        assert_ne!(
            fp0, fp1,
            "different file+line instances must have distinct fingerprints"
        );
    }

    // ---------------------------------------------------------------------------
    // circular_dep: cross-package variant includes cross-package label
    // ---------------------------------------------------------------------------

    #[test]
    fn circular_dep_cross_package_description_contains_label() {
        use fallow_types::output_dead_code::CircularDependencyFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![
                        root.join("packages/a/src/index.ts"),
                        root.join("packages/b/src/index.ts"),
                    ],
                    length: 2,
                    line: 0,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: true,
                },
            ));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("(cross-package)"), "desc: {desc}");
    }

    // ---------------------------------------------------------------------------
    // is_re_export: unused_types re-export label
    // ---------------------------------------------------------------------------

    #[test]
    fn unused_type_re_export_uses_type_re_export_label() {
        use fallow_types::output_dead_code::UnusedTypeFinding;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: root.join("src/index.ts"),
                export_name: "ReExportedType".to_string(),
                is_type_only: true,
                line: 2,
                col: 0,
                span_start: 0,
                is_re_export: true,
            }));
        let rules = RulesConfig::default();
        let output = codeclimate_issues_to_value(&api_codeclimate_issues(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/unused-type");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("Type re-export"), "desc: {desc}");
    }

    #[test]
    fn codeclimate_result_contract_exclusions_are_explicit() {
        use std::collections::BTreeSet;

        let missing: BTreeSet<&str> = fallow_output::issue_output_contracts()
            .filter(|contract| contract.codeclimate_check_names.is_empty())
            .map(|contract| contract.code)
            .collect();

        assert_eq!(
            BTreeSet::from(["duplicate-prop-shape", "prop-drilling", "thin-wrapper"]),
            missing
        );

        let check_names: BTreeSet<String> = fallow_output::issue_output_contracts()
            .flat_map(|contract| contract.codeclimate_check_names)
            .collect();
        assert!(check_names.contains("fallow/stale-suppression"));
        assert!(check_names.contains("fallow/missing-suppression-reason"));
    }
}
