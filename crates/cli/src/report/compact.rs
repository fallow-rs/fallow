use std::path::Path;

use fallow_core::duplicates::DuplicationReport;
use fallow_core::results::{AnalysisResults, UnusedExport, UnusedMember};

use super::grouping::ResultGroup;
use super::{normalize_uri, relative_path};

pub(super) fn print_compact(results: &AnalysisResults, root: &Path) {
    for line in build_compact_lines(results, root) {
        println!("{line}");
    }
}

/// Build compact output lines for analysis results.
/// Each issue is represented as a single `prefix:details` line.
pub fn build_compact_lines(results: &AnalysisResults, root: &Path) -> Vec<String> {
    let rel = |p: &Path| normalize_uri(&relative_path(p, root).display().to_string());

    let compact_export = |export: &UnusedExport, kind: &str, re_kind: &str| -> String {
        let tag = if export.is_re_export { re_kind } else { kind };
        format!(
            "{}:{}:{}:{}",
            tag,
            rel(&export.path),
            export.line,
            export.export_name
        )
    };

    let compact_member = |member: &UnusedMember, kind: &str| -> String {
        format!(
            "{}:{}:{}:{}.{}",
            kind,
            rel(&member.path),
            member.line,
            member.parent_name,
            member.member_name
        )
    };

    let mut lines = Vec::new();

    for file in &results.unused_files {
        lines.push(format!("unused-file:{}", rel(&file.path)));
    }
    for export in &results.unused_exports {
        lines.push(compact_export(export, "unused-export", "unused-re-export"));
    }
    for export in &results.unused_types {
        lines.push(compact_export(
            export,
            "unused-type",
            "unused-re-export-type",
        ));
    }
    for leak in &results.private_type_leaks {
        lines.push(format!(
            "private-type-leak:{}:{}:{}->{}",
            rel(&leak.path),
            leak.line,
            leak.export_name,
            leak.type_name
        ));
    }
    for dep in &results.unused_dependencies {
        lines.push(format!("unused-dep:{}", dep.package_name));
    }
    for dep in &results.unused_dev_dependencies {
        lines.push(format!("unused-devdep:{}", dep.package_name));
    }
    for dep in &results.unused_optional_dependencies {
        lines.push(format!("unused-optionaldep:{}", dep.package_name));
    }
    for member in &results.unused_enum_members {
        lines.push(compact_member(member, "unused-enum-member"));
    }
    for member in &results.unused_class_members {
        lines.push(compact_member(member, "unused-class-member"));
    }
    for import in &results.unresolved_imports {
        lines.push(format!(
            "unresolved-import:{}:{}:{}",
            rel(&import.path),
            import.line,
            import.specifier
        ));
    }
    for dep in &results.unlisted_dependencies {
        lines.push(format!("unlisted-dep:{}", dep.package_name));
    }
    for dup in &results.duplicate_exports {
        lines.push(format!("duplicate-export:{}", dup.export_name));
    }
    for dep in &results.type_only_dependencies {
        lines.push(format!("type-only-dep:{}", dep.package_name));
    }
    for dep in &results.test_only_dependencies {
        lines.push(format!("test-only-dep:{}", dep.package_name));
    }
    for cycle in &results.circular_dependencies {
        let chain: Vec<String> = cycle.files.iter().map(|p| rel(p)).collect();
        let mut display_chain = chain.clone();
        if let Some(first) = chain.first() {
            display_chain.push(first.clone());
        }
        let first_file = chain.first().map_or_else(String::new, Clone::clone);
        let cross_pkg_tag = if cycle.is_cross_package {
            " (cross-package)"
        } else {
            ""
        };
        lines.push(format!(
            "circular-dependency:{}:{}:{}{}",
            first_file,
            cycle.line,
            display_chain.join(" \u{2192} "),
            cross_pkg_tag
        ));
    }
    for v in &results.boundary_violations {
        lines.push(format!(
            "boundary-violation:{}:{}:{} -> {} ({} -> {})",
            rel(&v.from_path),
            v.line,
            rel(&v.from_path),
            rel(&v.to_path),
            v.from_zone,
            v.to_zone,
        ));
    }
    for s in &results.stale_suppressions {
        lines.push(format!(
            "stale-suppression:{}:{}:{}",
            rel(&s.path),
            s.line,
            s.description(),
        ));
    }
    for entry in &results.unused_catalog_entries {
        lines.push(format!(
            "unused-catalog-entry:{}:{}:{}:{}",
            rel(&entry.path),
            entry.line,
            entry.catalog_name,
            entry.entry_name,
        ));
    }

    lines
}

/// Print grouped compact output: each line is prefixed with the group key.
///
/// Format: `group-key\tissue-tag:details`
pub(super) fn print_grouped_compact(groups: &[ResultGroup], root: &Path) {
    for group in groups {
        for line in build_compact_lines(&group.results, root) {
            println!("{}\t{line}", group.key);
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "health compact formatter stitches many optional sections into one stream"
)]
pub(super) fn print_health_compact(report: &crate::health_types::HealthReport, root: &Path) {
    if let Some(ref hs) = report.health_score {
        println!("health-score:{:.1}:{}", hs.score, hs.grade);
    }
    if let Some(ref vs) = report.vital_signs {
        let mut parts = Vec::new();
        if vs.total_loc > 0 {
            parts.push(format!("total_loc={}", vs.total_loc));
        }
        parts.push(format!("avg_cyclomatic={:.1}", vs.avg_cyclomatic));
        parts.push(format!("p90_cyclomatic={}", vs.p90_cyclomatic));
        if let Some(v) = vs.dead_file_pct {
            parts.push(format!("dead_file_pct={v:.1}"));
        }
        if let Some(v) = vs.dead_export_pct {
            parts.push(format!("dead_export_pct={v:.1}"));
        }
        if let Some(v) = vs.maintainability_avg {
            parts.push(format!("maintainability_avg={v:.1}"));
        }
        if let Some(v) = vs.hotspot_count {
            parts.push(format!("hotspot_count={v}"));
        }
        if let Some(v) = vs.circular_dep_count {
            parts.push(format!("circular_dep_count={v}"));
        }
        if let Some(v) = vs.unused_dep_count {
            parts.push(format!("unused_dep_count={v}"));
        }
        println!("vital-signs:{}", parts.join(","));
    }
    for finding in &report.findings {
        let relative = normalize_uri(&relative_path(&finding.path, root).display().to_string());
        let severity = match finding.severity {
            crate::health_types::FindingSeverity::Critical => "critical",
            crate::health_types::FindingSeverity::High => "high",
            crate::health_types::FindingSeverity::Moderate => "moderate",
        };
        let crap_suffix = match finding.crap {
            Some(crap) => {
                let coverage = finding
                    .coverage_pct
                    .map(|pct| format!(",coverage_pct={pct:.1}"))
                    .unwrap_or_default();
                format!(",crap={crap:.1}{coverage}")
            }
            None => String::new(),
        };
        println!(
            "high-complexity:{}:{}:{}:cyclomatic={},cognitive={},severity={}{}",
            relative,
            finding.line,
            finding.name,
            finding.cyclomatic,
            finding.cognitive,
            severity,
            crap_suffix,
        );
    }
    for score in &report.file_scores {
        let relative = normalize_uri(&relative_path(&score.path, root).display().to_string());
        println!(
            "file-score:{}:mi={:.1},fan_in={},fan_out={},dead={:.2},density={:.2},crap_max={:.1},crap_above={}",
            relative,
            score.maintainability_index,
            score.fan_in,
            score.fan_out,
            score.dead_code_ratio,
            score.complexity_density,
            score.crap_max,
            score.crap_above_threshold,
        );
    }
    if let Some(ref gaps) = report.coverage_gaps {
        println!(
            "coverage-gap-summary:runtime_files={},covered_files={},file_coverage_pct={:.1},untested_files={},untested_exports={}",
            gaps.summary.runtime_files,
            gaps.summary.covered_files,
            gaps.summary.file_coverage_pct,
            gaps.summary.untested_files,
            gaps.summary.untested_exports,
        );
        for item in &gaps.files {
            let relative = normalize_uri(&relative_path(&item.path, root).display().to_string());
            println!(
                "untested-file:{}:value_exports={}",
                relative, item.value_export_count,
            );
        }
        for item in &gaps.exports {
            let relative = normalize_uri(&relative_path(&item.path, root).display().to_string());
            println!(
                "untested-export:{}:{}:{}",
                relative, item.line, item.export_name,
            );
        }
    }
    if let Some(ref production) = report.runtime_coverage {
        for line in build_runtime_coverage_compact_lines(production, root) {
            println!("{line}");
        }
    }
    for entry in &report.hotspots {
        let relative = normalize_uri(&relative_path(&entry.path, root).display().to_string());
        let ownership_suffix = entry
            .ownership
            .as_ref()
            .map(|o| {
                let mut parts = vec![
                    format!("bus={}", o.bus_factor),
                    format!("contributors={}", o.contributor_count),
                    format!("top={}", o.top_contributor.identifier),
                    format!("top_share={:.3}", o.top_contributor.share),
                ];
                if let Some(owner) = &o.declared_owner {
                    parts.push(format!("owner={owner}"));
                }
                if let Some(unowned) = o.unowned {
                    parts.push(format!("unowned={unowned}"));
                }
                if o.drift {
                    parts.push("drift=true".to_string());
                }
                format!(",{}", parts.join(","))
            })
            .unwrap_or_default();
        println!(
            "hotspot:{}:score={:.1},commits={},churn={},density={:.2},fan_in={},trend={}{}",
            relative,
            entry.score,
            entry.commits,
            entry.lines_added + entry.lines_deleted,
            entry.complexity_density,
            entry.fan_in,
            entry.trend,
            ownership_suffix,
        );
    }
    if let Some(ref trend) = report.health_trend {
        println!(
            "trend:overall:direction={}",
            trend.overall_direction.label()
        );
        for m in &trend.metrics {
            println!(
                "trend:{}:previous={:.1},current={:.1},delta={:+.1},direction={}",
                m.name,
                m.previous,
                m.current,
                m.delta,
                m.direction.label(),
            );
        }
    }
    for target in &report.targets {
        let relative = normalize_uri(&relative_path(&target.path, root).display().to_string());
        let category = target.category.compact_label();
        let effort = target.effort.label();
        let confidence = target.confidence.label();
        println!(
            "refactoring-target:{}:priority={:.1},efficiency={:.1},category={},effort={},confidence={}:{}",
            relative,
            target.priority,
            target.efficiency,
            category,
            effort,
            confidence,
            target.recommendation,
        );
    }
}

fn build_runtime_coverage_compact_lines(
    production: &crate::health_types::RuntimeCoverageReport,
    root: &Path,
) -> Vec<String> {
    let mut lines = vec![format!(
        "runtime-coverage-summary:functions_tracked={},functions_hit={},functions_unhit={},functions_untracked={},coverage_percent={:.1},trace_count={},period_days={},deployments_seen={}",
        production.summary.functions_tracked,
        production.summary.functions_hit,
        production.summary.functions_unhit,
        production.summary.functions_untracked,
        production.summary.coverage_percent,
        production.summary.trace_count,
        production.summary.period_days,
        production.summary.deployments_seen,
    )];
    for finding in &production.findings {
        let relative = normalize_uri(&relative_path(&finding.path, root).display().to_string());
        let invocations = finding
            .invocations
            .map_or_else(|| "null".to_owned(), |hits| hits.to_string());
        lines.push(format!(
            "runtime-coverage:{}:{}:{}:id={},verdict={},invocations={},confidence={}",
            relative,
            finding.line,
            finding.function,
            finding.id,
            finding.verdict,
            invocations,
            finding.confidence,
        ));
    }
    for entry in &production.hot_paths {
        let relative = normalize_uri(&relative_path(&entry.path, root).display().to_string());
        lines.push(format!(
            "production-hot-path:{}:{}:{}:id={},invocations={},percentile={}",
            relative, entry.line, entry.function, entry.id, entry.invocations, entry.percentile,
        ));
    }
    lines
}

pub(super) fn print_duplication_compact(report: &DuplicationReport, root: &Path) {
    for (i, group) in report.clone_groups.iter().enumerate() {
        for instance in &group.instances {
            let relative =
                normalize_uri(&relative_path(&instance.file, root).display().to_string());
            println!(
                "clone-group-{}:{}:{}-{}:{}tokens",
                i + 1,
                relative,
                instance.start_line,
                instance.end_line,
                group.token_count
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health_types::{
        RuntimeCoverageConfidence, RuntimeCoverageDataSource, RuntimeCoverageEvidence,
        RuntimeCoverageFinding, RuntimeCoverageHotPath, RuntimeCoverageReport,
        RuntimeCoverageReportVerdict, RuntimeCoverageSummary, RuntimeCoverageVerdict,
    };
    use crate::report::test_helpers::sample_results;
    use fallow_core::extract::MemberKind;
    use fallow_core::results::*;
    use std::path::PathBuf;

    #[test]
    fn compact_empty_results_no_lines() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let lines = build_compact_lines(&results, &root);
        assert!(lines.is_empty());
    }

    #[test]
    fn compact_unused_file_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: root.join("src/dead.ts"),
        });

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "unused-file:src/dead.ts");
    }

    #[test]
    fn compact_unused_export_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_exports.push(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "helperFn".to_string(),
            is_type_only: false,
            line: 10,
            col: 4,
            span_start: 120,
            is_re_export: false,
        });

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "unused-export:src/utils.ts:10:helperFn");
    }

    #[test]
    fn compact_health_includes_runtime_coverage_lines() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            runtime_coverage: Some(RuntimeCoverageReport {
                verdict: RuntimeCoverageReportVerdict::ColdCodeDetected,
                signals: Vec::new(),
                summary: RuntimeCoverageSummary {
                    data_source: RuntimeCoverageDataSource::Local,
                    last_received_at: None,
                    functions_tracked: 4,
                    functions_hit: 2,
                    functions_unhit: 1,
                    functions_untracked: 1,
                    coverage_percent: 50.0,
                    trace_count: 512,
                    period_days: 7,
                    deployments_seen: 2,
                    capture_quality: None,
                },
                findings: vec![RuntimeCoverageFinding {
                    id: "fallow:prod:deadbeef".to_owned(),
                    path: root.join("src/cold.ts"),
                    function: "coldPath".to_owned(),
                    line: 14,
                    verdict: RuntimeCoverageVerdict::ReviewRequired,
                    invocations: Some(0),
                    confidence: RuntimeCoverageConfidence::Medium,
                    evidence: RuntimeCoverageEvidence {
                        static_status: "used".to_owned(),
                        test_coverage: "not_covered".to_owned(),
                        v8_tracking: "tracked".to_owned(),
                        untracked_reason: None,
                        observation_days: 7,
                        deployments_observed: 2,
                    },
                    actions: vec![],
                }],
                hot_paths: vec![RuntimeCoverageHotPath {
                    id: "fallow:hot:cafebabe".to_owned(),
                    path: root.join("src/hot.ts"),
                    function: "hotPath".to_owned(),
                    line: 3,
                    end_line: 9,
                    invocations: 250,
                    percentile: 99,
                    actions: vec![],
                }],
                blast_radius: vec![],
                importance: vec![],
                watermark: None,
                warnings: vec![],
            }),
            ..Default::default()
        };

        let lines = build_runtime_coverage_compact_lines(
            report
                .runtime_coverage
                .as_ref()
                .expect("runtime coverage should be set"),
            &root,
        );
        assert_eq!(
            lines[0],
            "runtime-coverage-summary:functions_tracked=4,functions_hit=2,functions_unhit=1,functions_untracked=1,coverage_percent=50.0,trace_count=512,period_days=7,deployments_seen=2"
        );
        assert_eq!(
            lines[1],
            "runtime-coverage:src/cold.ts:14:coldPath:id=fallow:prod:deadbeef,verdict=review_required,invocations=0,confidence=medium"
        );
        assert_eq!(
            lines[2],
            "production-hot-path:src/hot.ts:3:hotPath:id=fallow:hot:cafebabe,invocations=250,percentile=99"
        );
    }

    #[test]
    fn compact_unused_type_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_types.push(UnusedExport {
            path: root.join("src/types.ts"),
            export_name: "OldType".to_string(),
            is_type_only: true,
            line: 5,
            col: 0,
            span_start: 60,
            is_re_export: false,
        });

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "unused-type:src/types.ts:5:OldType");
    }

    #[test]
    fn compact_unused_dep_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_dependencies.push(UnusedDependency {
            package_name: "lodash".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "unused-dep:lodash");
    }

    #[test]
    fn compact_unused_devdep_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_dev_dependencies.push(UnusedDependency {
            package_name: "jest".to_string(),
            location: DependencyLocation::DevDependencies,
            path: root.join("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "unused-devdep:jest");
    }

    #[test]
    fn compact_unused_enum_member_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_enum_members.push(UnusedMember {
            path: root.join("src/enums.ts"),
            parent_name: "Status".to_string(),
            member_name: "Deprecated".to_string(),
            kind: MemberKind::EnumMember,
            line: 8,
            col: 2,
        });

        let lines = build_compact_lines(&results, &root);
        assert_eq!(
            lines[0],
            "unused-enum-member:src/enums.ts:8:Status.Deprecated"
        );
    }

    #[test]
    fn compact_unused_class_member_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_class_members.push(UnusedMember {
            path: root.join("src/service.ts"),
            parent_name: "UserService".to_string(),
            member_name: "legacyMethod".to_string(),
            kind: MemberKind::ClassMethod,
            line: 42,
            col: 4,
        });

        let lines = build_compact_lines(&results, &root);
        assert_eq!(
            lines[0],
            "unused-class-member:src/service.ts:42:UserService.legacyMethod"
        );
    }

    #[test]
    fn compact_unresolved_import_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unresolved_imports.push(UnresolvedImport {
            path: root.join("src/app.ts"),
            specifier: "./missing-module".to_string(),
            line: 3,
            col: 0,
            specifier_col: 0,
        });

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "unresolved-import:src/app.ts:3:./missing-module");
    }

    #[test]
    fn compact_unlisted_dep_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unlisted_dependencies.push(UnlistedDependency {
            package_name: "chalk".to_string(),
            imported_from: vec![],
        });

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "unlisted-dep:chalk");
    }

    #[test]
    fn compact_duplicate_export_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.duplicate_exports.push(DuplicateExport {
            export_name: "Config".to_string(),
            locations: vec![
                DuplicateLocation {
                    path: root.join("src/a.ts"),
                    line: 15,
                    col: 0,
                },
                DuplicateLocation {
                    path: root.join("src/b.ts"),
                    line: 30,
                    col: 0,
                },
            ],
        });

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "duplicate-export:Config");
    }

    #[test]
    fn compact_all_issue_types_produce_lines() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let lines = build_compact_lines(&results, &root);

        // 16 issue types, one of each
        assert_eq!(lines.len(), 16);

        // Verify ordering matches output order
        assert!(lines[0].starts_with("unused-file:"));
        assert!(lines[1].starts_with("unused-export:"));
        assert!(lines[2].starts_with("unused-type:"));
        assert!(lines[3].starts_with("unused-dep:"));
        assert!(lines[4].starts_with("unused-devdep:"));
        assert!(lines[5].starts_with("unused-optionaldep:"));
        assert!(lines[6].starts_with("unused-enum-member:"));
        assert!(lines[7].starts_with("unused-class-member:"));
        assert!(lines[8].starts_with("unresolved-import:"));
        assert!(lines[9].starts_with("unlisted-dep:"));
        assert!(lines[10].starts_with("duplicate-export:"));
        assert!(lines[11].starts_with("type-only-dep:"));
        assert!(lines[12].starts_with("test-only-dep:"));
        assert!(lines[13].starts_with("circular-dependency:"));
        assert!(lines[14].starts_with("boundary-violation:"));
    }

    #[test]
    fn compact_strips_root_prefix_from_paths() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: PathBuf::from("/project/src/deep/nested/file.ts"),
        });

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "unused-file:src/deep/nested/file.ts");
    }

    // ── Re-export variants ──

    #[test]
    fn compact_re_export_tagged_correctly() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_exports.push(UnusedExport {
            path: root.join("src/index.ts"),
            export_name: "reExported".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: true,
        });

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "unused-re-export:src/index.ts:1:reExported");
    }

    #[test]
    fn compact_type_re_export_tagged_correctly() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_types.push(UnusedExport {
            path: root.join("src/index.ts"),
            export_name: "ReExportedType".to_string(),
            is_type_only: true,
            line: 3,
            col: 0,
            span_start: 0,
            is_re_export: true,
        });

        let lines = build_compact_lines(&results, &root);
        assert_eq!(
            lines[0],
            "unused-re-export-type:src/index.ts:3:ReExportedType"
        );
    }

    // ── Unused optional dependency ──

    #[test]
    fn compact_unused_optional_dep_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_optional_dependencies.push(UnusedDependency {
            package_name: "fsevents".to_string(),
            location: DependencyLocation::OptionalDependencies,
            path: root.join("package.json"),
            line: 12,
            used_in_workspaces: Vec::new(),
        });

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "unused-optionaldep:fsevents");
    }

    // ── Circular dependency ──

    #[test]
    fn compact_circular_dependency_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.circular_dependencies.push(CircularDependency {
            files: vec![root.join("src/a.ts"), root.join("src/b.ts")],
            length: 2,
            line: 3,
            col: 0,
            is_cross_package: false,
        });

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("circular-dependency:src/a.ts:3:"));
        assert!(lines[0].contains("src/a.ts"));
        assert!(lines[0].contains("src/b.ts"));
        // Chain should close the cycle: a -> b -> a
        assert!(lines[0].contains("\u{2192}"));
    }

    #[test]
    fn compact_circular_dependency_closes_cycle() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.circular_dependencies.push(CircularDependency {
            files: vec![
                root.join("src/a.ts"),
                root.join("src/b.ts"),
                root.join("src/c.ts"),
            ],
            length: 3,
            line: 1,
            col: 0,
            is_cross_package: false,
        });

        let lines = build_compact_lines(&results, &root);
        // Chain: a -> b -> c -> a
        let chain_part = lines[0].split(':').next_back().unwrap();
        let parts: Vec<&str> = chain_part.split(" \u{2192} ").collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], parts[3]); // first == last (cycle closes)
    }

    // ── Type-only dependency ──

    #[test]
    fn compact_type_only_dep_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.type_only_dependencies.push(TypeOnlyDependency {
            package_name: "zod".to_string(),
            path: root.join("package.json"),
            line: 8,
        });

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "type-only-dep:zod");
    }

    // ── Multiple items of same type ──

    #[test]
    fn compact_multiple_unused_files() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: root.join("src/a.ts"),
        });
        results.unused_files.push(UnusedFile {
            path: root.join("src/b.ts"),
        });

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "unused-file:src/a.ts");
        assert_eq!(lines[1], "unused-file:src/b.ts");
    }

    // ── Output ordering matches issue types ──

    #[test]
    fn compact_ordering_optional_dep_between_devdep_and_enum() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_dev_dependencies.push(UnusedDependency {
            package_name: "jest".to_string(),
            location: DependencyLocation::DevDependencies,
            path: root.join("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });
        results.unused_optional_dependencies.push(UnusedDependency {
            package_name: "fsevents".to_string(),
            location: DependencyLocation::OptionalDependencies,
            path: root.join("package.json"),
            line: 12,
            used_in_workspaces: Vec::new(),
        });
        results.unused_enum_members.push(UnusedMember {
            path: root.join("src/enums.ts"),
            parent_name: "Status".to_string(),
            member_name: "Deprecated".to_string(),
            kind: MemberKind::EnumMember,
            line: 8,
            col: 2,
        });

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("unused-devdep:"));
        assert!(lines[1].starts_with("unused-optionaldep:"));
        assert!(lines[2].starts_with("unused-enum-member:"));
    }

    // ── Path outside root ──

    #[test]
    fn compact_path_outside_root_preserved() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: PathBuf::from("/other/place/file.ts"),
        });

        let lines = build_compact_lines(&results, &root);
        assert!(lines[0].contains("/other/place/file.ts"));
    }
}
