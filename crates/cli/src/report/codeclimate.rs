use std::path::Path;
use std::process::ExitCode;

use fallow_config::{RulesConfig, Severity};
use fallow_core::duplicates::DuplicationReport;
use fallow_core::results::AnalysisResults;

use super::ci::{fingerprint, severity};
use super::grouping::{self, OwnershipResolver};
use super::{emit_json, normalize_uri, relative_path};
use crate::health_types::{
    ComplexityViolation, CoverageIntelligenceFinding, ExceededThreshold, HealthReport,
    RuntimeCoverageFinding, UntestedExportFinding, UntestedFileFinding,
};
use crate::output_envelope::{
    CodeClimateIssue, CodeClimateIssueKind, CodeClimateLines, CodeClimateLocation,
    CodeClimateSeverity,
};

/// Map fallow severity to CodeClimate severity.
fn severity_to_codeclimate(s: Severity) -> CodeClimateSeverity {
    severity::codeclimate_severity(s)
}

/// Compute a relative path string with forward-slash normalization.
///
/// Uses `normalize_uri` to ensure forward slashes on all platforms
/// and percent-encode brackets for Next.js dynamic routes.
fn cc_path(path: &Path, root: &Path) -> String {
    normalize_uri(&relative_path(path, root).display().to_string())
}

/// Compute a deterministic fingerprint hash from key fields.
///
/// Uses FNV-1a (64-bit) for guaranteed cross-version stability.
/// `DefaultHasher` is explicitly not specified across Rust versions.
fn fingerprint_hash(parts: &[&str]) -> String {
    fingerprint::fingerprint_hash(parts)
}

/// Build a single CodeClimate issue. Wire shape is locked by the
/// [`CodeClimateIssue`] typed envelope (and the schema drift gate);
/// changes to the wire must flow through that struct.
/// The fields of one Code Climate issue, bundled so `cc_issue` takes a single
/// descriptor instead of seven positional parameters.
#[derive(Clone, Copy)]
struct CcIssue<'a> {
    check_name: &'a str,
    description: &'a str,
    severity: CodeClimateSeverity,
    category: &'a str,
    path: &'a str,
    begin_line: Option<u32>,
    fingerprint: &'a str,
}

fn cc_issue(issue: CcIssue<'_>) -> CodeClimateIssue {
    let CcIssue {
        check_name,
        description,
        severity,
        category,
        path,
        begin_line,
        fingerprint,
    } = issue;
    CodeClimateIssue {
        kind: CodeClimateIssueKind::Issue,
        check_name: check_name.to_string(),
        description: description.to_string(),
        categories: vec![category.to_string()],
        severity,
        fingerprint: fingerprint.to_string(),
        location: CodeClimateLocation {
            path: path.to_string(),
            lines: CodeClimateLines {
                begin: begin_line.unwrap_or(1),
            },
        },
    }
}

fn coverage_intelligence_check_name(
    recommendation: crate::health_types::CoverageIntelligenceRecommendation,
) -> &'static str {
    match recommendation {
        crate::health_types::CoverageIntelligenceRecommendation::AddTestOrSplitBeforeMerge => {
            "fallow/coverage-intelligence-risky-change"
        }
        crate::health_types::CoverageIntelligenceRecommendation::DeleteAfterConfirmingOwner => {
            "fallow/coverage-intelligence-delete"
        }
        crate::health_types::CoverageIntelligenceRecommendation::ReviewBeforeChanging => {
            "fallow/coverage-intelligence-review"
        }
        crate::health_types::CoverageIntelligenceRecommendation::RefactorCarefullyKeepBehavior => {
            "fallow/coverage-intelligence-refactor"
        }
    }
}

struct HealthCodeClimateContext<'a> {
    root: &'a Path,
    cyc_t: u16,
    cog_t: u16,
    crap_t: f64,
}

impl HealthCodeClimateContext<'_> {
    fn complexity_issue(&self, finding: &ComplexityViolation) -> CodeClimateIssue {
        let path = cc_path(&finding.path, self.root);
        let check_name = complexity_check_name(finding);
        let line_str = finding.line.to_string();
        let fp = fingerprint_hash(&[check_name, &path, &line_str, &finding.name]);
        cc_issue(CcIssue {
            check_name,
            description: &self.complexity_description(finding),
            severity: health_finding_severity(finding.severity),
            category: "Complexity",
            path: &path,
            begin_line: Some(finding.line),
            fingerprint: &fp,
        })
    }

    fn complexity_description(&self, finding: &ComplexityViolation) -> String {
        match finding.exceeded {
            ExceededThreshold::Both => format!(
                "'{}' has cyclomatic complexity {} (threshold: {}) and cognitive complexity {} (threshold: {})",
                finding.name, finding.cyclomatic, self.cyc_t, finding.cognitive, self.cog_t
            ),
            ExceededThreshold::Cyclomatic => format!(
                "'{}' has cyclomatic complexity {} (threshold: {})",
                finding.name, finding.cyclomatic, self.cyc_t
            ),
            ExceededThreshold::Cognitive => format!(
                "'{}' has cognitive complexity {} (threshold: {})",
                finding.name, finding.cognitive, self.cog_t
            ),
            ExceededThreshold::Crap
            | ExceededThreshold::CyclomaticCrap
            | ExceededThreshold::CognitiveCrap
            | ExceededThreshold::All => {
                let crap = finding.crap.unwrap_or(0.0);
                let coverage = finding
                    .coverage_pct
                    .map(|pct| format!(", coverage {pct:.0}%"))
                    .unwrap_or_default();
                format!(
                    "'{}' has CRAP score {crap:.1} (threshold: {:.1}, cyclomatic {}{coverage})",
                    finding.name, self.crap_t, finding.cyclomatic,
                )
            }
        }
    }

    fn runtime_coverage_issue(&self, finding: &RuntimeCoverageFinding) -> CodeClimateIssue {
        let path = cc_path(&finding.path, self.root);
        let check_name = runtime_coverage_check_name(finding.verdict);
        let invocations_hint = finding.invocations.map_or_else(
            || "untracked".to_owned(),
            |hits| format!("{hits} invocations"),
        );
        let description = format!(
            "'{}' runtime coverage verdict: {} ({})",
            finding.function,
            finding.verdict.human_label(),
            invocations_hint,
        );
        let fp = fingerprint_hash(&[
            check_name,
            &path,
            &finding.line.to_string(),
            &finding.function,
        ]);
        cc_issue(CcIssue {
            check_name,
            description: &description,
            severity: runtime_coverage_severity(finding.verdict),
            category: "Bug Risk",
            path: &path,
            begin_line: Some(finding.line),
            fingerprint: &fp,
        })
    }

    fn coverage_intelligence_issue(
        &self,
        finding: &CoverageIntelligenceFinding,
    ) -> Option<CodeClimateIssue> {
        let severity = coverage_intelligence_severity(finding.verdict)?;
        let path = cc_path(&finding.path, self.root);
        let check_name = coverage_intelligence_check_name(finding.recommendation);
        let identity = finding.identity.as_deref().unwrap_or("code");
        let description = format!(
            "'{}' coverage intelligence verdict: {} ({})",
            identity, finding.verdict, finding.recommendation,
        );
        let fp = fingerprint_hash(&[
            check_name,
            &path,
            &finding.line.to_string(),
            identity,
            &finding.id,
        ]);
        Some(cc_issue(CcIssue {
            check_name,
            description: &description,
            severity,
            category: "Bug Risk",
            path: &path,
            begin_line: Some(finding.line),
            fingerprint: &fp,
        }))
    }

    fn untested_file_issue(&self, item: &UntestedFileFinding) -> CodeClimateIssue {
        let path = cc_path(&item.file.path, self.root);
        let description = format!(
            "File is runtime-reachable but has no test dependency path ({} value export{})",
            item.file.value_export_count,
            if item.file.value_export_count == 1 {
                ""
            } else {
                "s"
            },
        );
        let fp = fingerprint_hash(&["fallow/untested-file", &path]);
        cc_issue(CcIssue {
            check_name: "fallow/untested-file",
            description: &description,
            severity: CodeClimateSeverity::Minor,
            category: "Coverage",
            path: &path,
            begin_line: None,
            fingerprint: &fp,
        })
    }

    fn untested_export_issue(&self, item: &UntestedExportFinding) -> CodeClimateIssue {
        let path = cc_path(&item.export.path, self.root);
        let description = format!(
            "Export '{}' is runtime-reachable but never referenced by test-reachable modules",
            item.export.export_name
        );
        let line_str = item.export.line.to_string();
        let fp = fingerprint_hash(&[
            "fallow/untested-export",
            &path,
            &line_str,
            &item.export.export_name,
        ]);
        cc_issue(CcIssue {
            check_name: "fallow/untested-export",
            description: &description,
            severity: CodeClimateSeverity::Minor,
            category: "Coverage",
            path: &path,
            begin_line: Some(item.export.line),
            fingerprint: &fp,
        })
    }
}

const fn complexity_check_name(finding: &ComplexityViolation) -> &'static str {
    match finding.exceeded {
        ExceededThreshold::Both => "fallow/high-complexity",
        ExceededThreshold::Cyclomatic => "fallow/high-cyclomatic-complexity",
        ExceededThreshold::Cognitive => "fallow/high-cognitive-complexity",
        ExceededThreshold::Crap
        | ExceededThreshold::CyclomaticCrap
        | ExceededThreshold::CognitiveCrap
        | ExceededThreshold::All => "fallow/high-crap-score",
    }
}

const fn health_finding_severity(
    severity: crate::health_types::FindingSeverity,
) -> CodeClimateSeverity {
    match severity {
        crate::health_types::FindingSeverity::Critical => CodeClimateSeverity::Critical,
        crate::health_types::FindingSeverity::High => CodeClimateSeverity::Major,
        crate::health_types::FindingSeverity::Moderate => CodeClimateSeverity::Minor,
    }
}

const fn runtime_coverage_check_name(
    verdict: crate::health_types::RuntimeCoverageVerdict,
) -> &'static str {
    match verdict {
        crate::health_types::RuntimeCoverageVerdict::SafeToDelete => {
            "fallow/runtime-safe-to-delete"
        }
        crate::health_types::RuntimeCoverageVerdict::ReviewRequired => {
            "fallow/runtime-review-required"
        }
        crate::health_types::RuntimeCoverageVerdict::LowTraffic => "fallow/runtime-low-traffic",
        crate::health_types::RuntimeCoverageVerdict::CoverageUnavailable => {
            "fallow/runtime-coverage-unavailable"
        }
        crate::health_types::RuntimeCoverageVerdict::Active
        | crate::health_types::RuntimeCoverageVerdict::Unknown => "fallow/runtime-coverage",
    }
}

const fn runtime_coverage_severity(
    verdict: crate::health_types::RuntimeCoverageVerdict,
) -> CodeClimateSeverity {
    match verdict {
        crate::health_types::RuntimeCoverageVerdict::SafeToDelete => CodeClimateSeverity::Critical,
        crate::health_types::RuntimeCoverageVerdict::ReviewRequired => CodeClimateSeverity::Major,
        _ => CodeClimateSeverity::Minor,
    }
}

const fn coverage_intelligence_severity(
    verdict: crate::health_types::CoverageIntelligenceVerdict,
) -> Option<CodeClimateSeverity> {
    match verdict {
        crate::health_types::CoverageIntelligenceVerdict::RiskyChangeDetected
        | crate::health_types::CoverageIntelligenceVerdict::HighConfidenceDelete => {
            Some(CodeClimateSeverity::Major)
        }
        crate::health_types::CoverageIntelligenceVerdict::ReviewRequired
        | crate::health_types::CoverageIntelligenceVerdict::RefactorCarefully => {
            Some(CodeClimateSeverity::Minor)
        }
        crate::health_types::CoverageIntelligenceVerdict::Clean
        | crate::health_types::CoverageIntelligenceVerdict::Unknown => None,
    }
}

/// Push CodeClimate issues for unused dependencies with a shared structure.
fn push_dep_cc_issues<'a, I>(
    issues: &mut Vec<CodeClimateIssue>,
    deps: I,
    root: &Path,
    rule_id: &str,
    location_label: &str,
    severity: Severity,
) where
    I: IntoIterator<Item = &'a fallow_core::results::UnusedDependency>,
{
    for dep in deps {
        let level = severity_to_codeclimate(severity);
        let path = cc_path(&dep.path, root);
        let line = if dep.line > 0 { Some(dep.line) } else { None };
        let fp = fingerprint_hash(&[rule_id, &dep.package_name]);
        let workspace_context = if dep.used_in_workspaces.is_empty() {
            String::new()
        } else {
            let workspaces = dep
                .used_in_workspaces
                .iter()
                .map(|path| cc_path(path, root))
                .collect::<Vec<_>>()
                .join(", ");
            format!("; imported in other workspaces: {workspaces}")
        };
        issues.push(cc_issue(CcIssue {
            check_name: rule_id,
            description: &format!(
                "Package '{}' is in {location_label} but never imported{workspace_context}",
                dep.package_name
            ),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_unused_file_issues(
    issues: &mut Vec<CodeClimateIssue>,
    files: &[fallow_types::output_dead_code::UnusedFileFinding],
    root: &Path,
    severity: Severity,
) {
    if files.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in files {
        let path = cc_path(&entry.file.path, root);
        let fp = fingerprint_hash(&["fallow/unused-file", &path]);
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/unused-file",
            description: "File is not reachable from any entry point",
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: None,
            fingerprint: &fp,
        }));
    }
}

/// Push CodeClimate issues for unused exports or unused types.
///
/// `direct_label` / `re_export_label` let the same helper produce the right
/// prose for both `unused-export` (Export / Re-export) and `unused-type`
/// (Type export / Type re-export) rule ids.
struct UnusedExportIssuesInput<'a, I> {
    issues: &'a mut Vec<CodeClimateIssue>,
    exports: I,
    root: &'a Path,
    rule_id: &'a str,
    direct_label: &'a str,
    re_export_label: &'a str,
    severity: Severity,
}

fn push_unused_export_issues<'a, I>(input: UnusedExportIssuesInput<'a, I>)
where
    I: IntoIterator<Item = &'a fallow_core::results::UnusedExport>,
{
    for export in input.exports {
        let level = severity_to_codeclimate(input.severity);
        let path = cc_path(&export.path, input.root);
        let kind = if export.is_re_export {
            input.re_export_label
        } else {
            input.direct_label
        };
        let line_str = export.line.to_string();
        let fp = fingerprint_hash(&[input.rule_id, &path, &line_str, &export.export_name]);
        input.issues.push(cc_issue(CcIssue {
            check_name: input.rule_id,
            description: &format!(
                "{kind} '{}' is never imported by other modules",
                export.export_name
            ),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: Some(export.line),
            fingerprint: &fp,
        }));
    }
}

fn push_private_type_leak_issues(
    issues: &mut Vec<CodeClimateIssue>,
    leaks: &[fallow_types::output_dead_code::PrivateTypeLeakFinding],
    root: &Path,
    severity: Severity,
) {
    if leaks.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in leaks {
        let leak = &entry.leak;
        let path = cc_path(&leak.path, root);
        let line_str = leak.line.to_string();
        let fp = fingerprint_hash(&[
            "fallow/private-type-leak",
            &path,
            &line_str,
            &leak.export_name,
            &leak.type_name,
        ]);
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/private-type-leak",
            description: &format!(
                "Export '{}' references private type '{}'",
                leak.export_name, leak.type_name
            ),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: Some(leak.line),
            fingerprint: &fp,
        }));
    }
}

fn push_type_only_dep_issues(
    issues: &mut Vec<CodeClimateIssue>,
    deps: &[fallow_core::results::TypeOnlyDependencyFinding],
    root: &Path,
    severity: Severity,
) {
    if deps.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in deps {
        let dep = &entry.dep;
        let path = cc_path(&dep.path, root);
        let line = if dep.line > 0 { Some(dep.line) } else { None };
        let fp = fingerprint_hash(&["fallow/type-only-dependency", &dep.package_name]);
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/type-only-dependency",
            description: &format!(
                "Package '{}' is only imported via type-only imports (consider moving to devDependencies)",
                dep.package_name
            ),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_test_only_dep_issues(
    issues: &mut Vec<CodeClimateIssue>,
    deps: &[fallow_core::results::TestOnlyDependencyFinding],
    root: &Path,
    severity: Severity,
) {
    if deps.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in deps {
        let dep = &entry.dep;
        let path = cc_path(&dep.path, root);
        let line = if dep.line > 0 { Some(dep.line) } else { None };
        let fp = fingerprint_hash(&["fallow/test-only-dependency", &dep.package_name]);
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/test-only-dependency",
            description: &format!(
                "Package '{}' is only imported by test files (consider moving to devDependencies)",
                dep.package_name
            ),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

/// Push CodeClimate issues for unused enum or class members.
///
/// `entity_label` is `"Enum"` or `"Class"` so the rendered description reads
/// "Enum member ..." or "Class member ..." accordingly.
fn push_unused_member_issues<'a, I>(
    issues: &mut Vec<CodeClimateIssue>,
    members: I,
    root: &Path,
    rule_id: &str,
    entity_label: &str,
    severity: Severity,
) where
    I: IntoIterator<Item = &'a fallow_core::results::UnusedMember>,
{
    for member in members {
        let level = severity_to_codeclimate(severity);
        let path = cc_path(&member.path, root);
        let line_str = member.line.to_string();
        let fp = fingerprint_hash(&[
            rule_id,
            &path,
            &line_str,
            &member.parent_name,
            &member.member_name,
        ]);
        issues.push(cc_issue(CcIssue {
            check_name: rule_id,
            description: &format!(
                "{entity_label} member '{}.{}' is never referenced",
                member.parent_name, member.member_name
            ),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: Some(member.line),
            fingerprint: &fp,
        }));
    }
}

fn push_unresolved_import_issues(
    issues: &mut Vec<CodeClimateIssue>,
    imports: &[fallow_types::output_dead_code::UnresolvedImportFinding],
    root: &Path,
    severity: Severity,
) {
    if imports.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in imports {
        let import = &entry.import;
        let path = cc_path(&import.path, root);
        let line_str = import.line.to_string();
        let fp = fingerprint_hash(&[
            "fallow/unresolved-import",
            &path,
            &line_str,
            &import.specifier,
        ]);
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/unresolved-import",
            description: &format!("Import '{}' could not be resolved", import.specifier),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: Some(import.line),
            fingerprint: &fp,
        }));
    }
}

fn push_unlisted_dep_issues(
    issues: &mut Vec<CodeClimateIssue>,
    deps: &[fallow_core::results::UnlistedDependencyFinding],
    root: &Path,
    severity: Severity,
) {
    if deps.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in deps {
        let dep = &entry.dep;
        for site in &dep.imported_from {
            let path = cc_path(&site.path, root);
            let line_str = site.line.to_string();
            let fp = fingerprint_hash(&[
                "fallow/unlisted-dependency",
                &path,
                &line_str,
                &dep.package_name,
            ]);
            issues.push(cc_issue(CcIssue {
                check_name: "fallow/unlisted-dependency",
                description: &format!(
                    "Package '{}' is imported but not listed in package.json",
                    dep.package_name
                ),
                severity: level,
                category: "Bug Risk",
                path: &path,
                begin_line: Some(site.line),
                fingerprint: &fp,
            }));
        }
    }
}

fn push_duplicate_export_issues(
    issues: &mut Vec<CodeClimateIssue>,
    dups: &[fallow_core::results::DuplicateExportFinding],
    root: &Path,
    severity: Severity,
) {
    if dups.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for dup in dups {
        let dup = &dup.export;
        for loc in &dup.locations {
            let path = cc_path(&loc.path, root);
            let line_str = loc.line.to_string();
            let fp = fingerprint_hash(&[
                "fallow/duplicate-export",
                &path,
                &line_str,
                &dup.export_name,
            ]);
            issues.push(cc_issue(CcIssue {
                check_name: "fallow/duplicate-export",
                description: &format!("Export '{}' appears in multiple modules", dup.export_name),
                severity: level,
                category: "Bug Risk",
                path: &path,
                begin_line: Some(loc.line),
                fingerprint: &fp,
            }));
        }
    }
}

fn push_circular_dep_issues(
    issues: &mut Vec<CodeClimateIssue>,
    cycles: &[fallow_types::output_dead_code::CircularDependencyFinding],
    root: &Path,
    severity: Severity,
) {
    if cycles.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in cycles {
        let cycle = &entry.cycle;
        let Some(first) = cycle.files.first() else {
            continue;
        };
        let path = cc_path(first, root);
        let chain: Vec<String> = cycle.files.iter().map(|f| cc_path(f, root)).collect();
        let chain_str = chain.join(":");
        let fp = fingerprint_hash(&["fallow/circular-dependency", &chain_str]);
        let line = if cycle.line > 0 {
            Some(cycle.line)
        } else {
            None
        };
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/circular-dependency",
            description: &format!(
                "Circular dependency{}: {}",
                if cycle.is_cross_package {
                    " (cross-package)"
                } else {
                    ""
                },
                chain.join(" \u{2192} ")
            ),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_re_export_cycle_issues(
    issues: &mut Vec<CodeClimateIssue>,
    cycles: &[fallow_types::output_dead_code::ReExportCycleFinding],
    root: &Path,
    severity: Severity,
) {
    if cycles.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in cycles {
        let cycle = &entry.cycle;
        let Some(first) = cycle.files.first() else {
            continue;
        };
        let path = cc_path(first, root);
        let chain: Vec<String> = cycle.files.iter().map(|f| cc_path(f, root)).collect();
        let chain_str = chain.join(":");
        let kind_token = match cycle.kind {
            fallow_core::results::ReExportCycleKind::SelfLoop => "self-loop",
            fallow_core::results::ReExportCycleKind::MultiNode => "multi-node",
        };
        let kind_tag = match cycle.kind {
            fallow_core::results::ReExportCycleKind::SelfLoop => " (self-loop)",
            fallow_core::results::ReExportCycleKind::MultiNode => "",
        };
        let fp = fingerprint_hash(&["fallow/re-export-cycle", kind_token, &chain_str]);
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/re-export-cycle",
            description: &format!("Re-export cycle{}: {}", kind_tag, chain.join(" <-> ")),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: None,
            fingerprint: &fp,
        }));
    }
}

fn push_boundary_violation_issues(
    issues: &mut Vec<CodeClimateIssue>,
    violations: &[fallow_types::output_dead_code::BoundaryViolationFinding],
    root: &Path,
    severity: Severity,
) {
    if violations.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in violations {
        let v = &entry.violation;
        let path = cc_path(&v.from_path, root);
        let to = cc_path(&v.to_path, root);
        let fp = fingerprint_hash(&["fallow/boundary-violation", &path, &to]);
        let line = if v.line > 0 { Some(v.line) } else { None };
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/boundary-violation",
            description: &format!(
                "Boundary violation: {} -> {} ({} -> {})",
                path, to, v.from_zone, v.to_zone
            ),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_boundary_coverage_issues(
    issues: &mut Vec<CodeClimateIssue>,
    violations: &[fallow_types::output_dead_code::BoundaryCoverageViolationFinding],
    root: &Path,
    severity: Severity,
) {
    if violations.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in violations {
        let v = &entry.violation;
        let path = cc_path(&v.path, root);
        let fp = fingerprint_hash(&["fallow/boundary-coverage", &path]);
        let line = if v.line > 0 { Some(v.line) } else { None };
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/boundary-coverage",
            description: &format!("Boundary coverage: {path} matches no configured zone"),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_boundary_call_issues(
    issues: &mut Vec<CodeClimateIssue>,
    violations: &[fallow_types::output_dead_code::BoundaryCallViolationFinding],
    root: &Path,
    severity: Severity,
) {
    if violations.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in violations {
        let v = &entry.violation;
        let path = cc_path(&v.path, root);
        let fp = fingerprint_hash(&["fallow/boundary-call-violation", &path, &v.callee]);
        let line = if v.line > 0 { Some(v.line) } else { None };
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/boundary-call-violation",
            description: &format!(
                "Boundary call: `{}` matches forbidden pattern `{}` in zone '{}'",
                v.callee, v.pattern, v.zone
            ),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_policy_violation_issues(
    issues: &mut Vec<CodeClimateIssue>,
    violations: &[fallow_types::output_dead_code::PolicyViolationFinding],
    root: &Path,
) {
    use fallow_core::results::PolicyViolationSeverity;

    for entry in violations {
        let v = &entry.violation;
        let path = cc_path(&v.path, root);
        let rule = format!("{}/{}", v.pack, v.rule_id);
        let fp = fingerprint_hash(&["fallow/policy-violation", &path, &rule, &v.matched]);
        let line = if v.line > 0 { Some(v.line) } else { None };
        // Severity comes from the EFFECTIVE per-finding value, not the
        // policy-violation master, so a severity: "error" rule under a warn
        // master maps to blocker-level just like the exit-code gate.
        let level = severity_to_codeclimate(match v.severity {
            PolicyViolationSeverity::Error => Severity::Error,
            PolicyViolationSeverity::Warn => Severity::Warn,
        });
        let message = match &v.message {
            Some(message) => format!(
                "Policy violation: `{}` is banned by `{rule}`. {message}",
                v.matched
            ),
            None => format!("Policy violation: `{}` is banned by `{rule}`", v.matched),
        };
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/policy-violation",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_invalid_client_export_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::InvalidClientExportFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let e = &entry.export;
        let path = cc_path(&e.path, root);
        let fp = fingerprint_hash(&["fallow/invalid-client-export", &path, &e.export_name]);
        let line = if e.line > 0 { Some(e.line) } else { None };
        let message = format!(
            "Export `{}` is not allowed in a \"{}\" file (Next.js server-only / route-config name)",
            e.export_name, e.directive
        );
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/invalid-client-export",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_mixed_client_server_barrel_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::MixedClientServerBarrelFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let b = &entry.barrel;
        let path = cc_path(&b.path, root);
        let fp = fingerprint_hash(&[
            "fallow/mixed-client-server-barrel",
            &path,
            &b.client_origin,
            &b.server_origin,
        ]);
        let line = if b.line > 0 { Some(b.line) } else { None };
        let message = format!(
            "Barrel re-exports both a \"use client\" module (`{}`) and a server-only module (`{}`); one import drags the other's directive across the boundary",
            b.client_origin, b.server_origin
        );
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/mixed-client-server-barrel",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_misplaced_directive_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::MisplacedDirectiveFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let d = &entry.directive_site;
        let path = cc_path(&d.path, root);
        let fp = fingerprint_hash(&[
            "fallow/misplaced-directive",
            &path,
            &d.line.to_string(),
            &d.directive,
        ]);
        let line = if d.line > 0 { Some(d.line) } else { None };
        let message = format!(
            "Directive `\"{}\"` is not in the leading position, so the RSC bundler ignores it; move it to the top of the file",
            d.directive
        );
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/misplaced-directive",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_unprovided_inject_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnprovidedInjectFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let i = &entry.inject;
        let path = cc_path(&i.path, root);
        let fp = fingerprint_hash(&[
            "fallow/unprovided-inject",
            &path,
            &i.line.to_string(),
            &i.key_name,
        ]);
        let line = if i.line > 0 { Some(i.line) } else { None };
        let message = format!(
            "inject(`{}`) has no matching provide(`{}`) in this project; at runtime it returns undefined (provide the key or remove this inject)",
            i.key_name, i.key_name
        );
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/unprovided-inject",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_unrendered_component_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnrenderedComponentFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let c = &entry.component;
        let path = cc_path(&c.path, root);
        let fp = fingerprint_hash(&[
            "fallow/unrendered-component",
            &path,
            &c.line.to_string(),
            &c.component_name,
        ]);
        let line = if c.line > 0 { Some(c.line) } else { None };
        let message = format!(
            "component `{}` is reachable but rendered nowhere in this project (render it somewhere or remove it)",
            c.component_name
        );
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/unrendered-component",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_unused_component_prop_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnusedComponentPropFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let p = &entry.prop;
        let path = cc_path(&p.path, root);
        let fp = fingerprint_hash(&[
            "fallow/unused-component-prop",
            &path,
            &p.line.to_string(),
            &p.prop_name,
        ]);
        let line = if p.line > 0 { Some(p.line) } else { None };
        let message = format!(
            "prop `{}` is declared but referenced nowhere in component `{}` (remove it or use it)",
            p.prop_name, p.component_name
        );
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/unused-component-prop",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_unused_component_emit_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnusedComponentEmitFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let e = &entry.emit;
        let path = cc_path(&e.path, root);
        let fp = fingerprint_hash(&[
            "fallow/unused-component-emit",
            &path,
            &e.line.to_string(),
            &e.emit_name,
        ]);
        let line = if e.line > 0 { Some(e.line) } else { None };
        let message = format!(
            "emit `{}` is declared but emitted nowhere in component `{}` (remove it or emit it)",
            e.emit_name, e.component_name
        );
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/unused-component-emit",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_unused_svelte_event_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnusedSvelteEventFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let e = &entry.event;
        let path = cc_path(&e.path, root);
        let fp = fingerprint_hash(&[
            "fallow/unused-svelte-event",
            &path,
            &e.line.to_string(),
            &e.event_name,
        ]);
        let line = if e.line > 0 { Some(e.line) } else { None };
        let message = format!(
            "event `{}` is dispatched by component `{}` but listened to nowhere in the project (remove it or listen for it)",
            e.event_name, e.component_name
        );
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/unused-svelte-event",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_unused_component_input_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnusedComponentInputFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let i = &entry.input;
        let path = cc_path(&i.path, root);
        let fp = fingerprint_hash(&[
            "fallow/unused-component-input",
            &path,
            &i.line.to_string(),
            &i.input_name,
        ]);
        let line = if i.line > 0 { Some(i.line) } else { None };
        let message = format!(
            "input `{}` is declared but referenced nowhere in component `{}` (remove it or use it)",
            i.input_name, i.component_name
        );
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/unused-component-input",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_unused_component_output_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnusedComponentOutputFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let o = &entry.output;
        let path = cc_path(&o.path, root);
        let fp = fingerprint_hash(&[
            "fallow/unused-component-output",
            &path,
            &o.line.to_string(),
            &o.output_name,
        ]);
        let line = if o.line > 0 { Some(o.line) } else { None };
        let message = format!(
            "output `{}` is declared but emitted nowhere in component `{}` (remove it or emit it)",
            o.output_name, o.component_name
        );
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/unused-component-output",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_unused_server_action_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnusedServerActionFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let a = &entry.action;
        let path = cc_path(&a.path, root);
        let fp = fingerprint_hash(&[
            "fallow/unused-server-action",
            &path,
            &a.line.to_string(),
            &a.action_name,
        ]);
        let line = if a.line > 0 { Some(a.line) } else { None };
        let message = format!(
            "server action `{}` is exported from a \"use server\" file but no code in this project references it (wire it to a consumer or remove it)",
            a.action_name
        );
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/unused-server-action",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_unused_load_data_key_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnusedLoadDataKeyFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let k = &entry.key;
        let path = cc_path(&k.path, root);
        let fp = fingerprint_hash(&[
            "fallow/unused-load-data-key",
            &path,
            &k.line.to_string(),
            &k.key_name,
        ]);
        let line = if k.line > 0 { Some(k.line) } else { None };
        let message = format!(
            "load() return key `{}` is read by no consumer (sibling +page.svelte data.<key> or project-wide page.data.<key>); delete the key or wire a consumer",
            k.key_name
        );
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/unused-load-data-key",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_route_collision_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::RouteCollisionFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let c = &entry.collision;
        let path = cc_path(&c.path, root);
        let fp = fingerprint_hash(&["fallow/route-collision", &path, &c.url]);
        let line = if c.line > 0 { Some(c.line) } else { None };
        let message = format!(
            "Route file resolves to `{}`, also owned by {} other file(s); Next.js fails the build because a URL can have only one owner",
            c.url,
            c.conflicting_paths.len()
        );
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/route-collision",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_dynamic_segment_name_conflict_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::DynamicSegmentNameConflictFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in findings {
        let c = &entry.conflict;
        let path = cc_path(&c.path, root);
        let fp = fingerprint_hash(&["fallow/dynamic-segment-name-conflict", &path, &c.position]);
        let line = if c.line > 0 { Some(c.line) } else { None };
        let message = format!(
            "Dynamic segments at `{}` use different slug names ({}); Next.js requires one consistent name per dynamic path",
            c.position,
            c.conflicting_segments.join(", ")
        );
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/dynamic-segment-name-conflict",
            description: &message,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: line,
            fingerprint: &fp,
        }));
    }
}

fn push_stale_suppression_issues(
    issues: &mut Vec<CodeClimateIssue>,
    suppressions: &[fallow_core::results::StaleSuppression],
    root: &Path,
    rules: &RulesConfig,
) {
    if suppressions.is_empty() {
        return;
    }
    for s in suppressions {
        let severity = if s.missing_reason {
            rules.require_suppression_reason
        } else {
            rules.stale_suppressions
        };
        let level = severity_to_codeclimate(severity);
        let path = cc_path(&s.path, root);
        let line_str = s.line.to_string();
        let check_name = if s.missing_reason {
            "fallow/missing-suppression-reason"
        } else {
            "fallow/stale-suppression"
        };
        let fp = fingerprint_hash(&[check_name, &path, &line_str]);
        issues.push(cc_issue(CcIssue {
            check_name,
            description: &s.display_message(),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: Some(s.line),
            fingerprint: &fp,
        }));
    }
}

fn push_unused_catalog_entry_issues(
    issues: &mut Vec<CodeClimateIssue>,
    entries: &[fallow_core::results::UnusedCatalogEntryFinding],
    root: &Path,
    severity: Severity,
) {
    if entries.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in entries {
        let entry = &entry.entry;
        let path = cc_path(&entry.path, root);
        let line_str = entry.line.to_string();
        let fp = fingerprint_hash(&[
            "fallow/unused-catalog-entry",
            &path,
            &line_str,
            &entry.catalog_name,
            &entry.entry_name,
        ]);
        let description = if entry.catalog_name == "default" {
            format!(
                "Catalog entry '{}' is not referenced by any workspace package",
                entry.entry_name
            )
        } else {
            format!(
                "Catalog entry '{}' (catalog '{}') is not referenced by any workspace package",
                entry.entry_name, entry.catalog_name
            )
        };
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/unused-catalog-entry",
            description: &description,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: Some(entry.line),
            fingerprint: &fp,
        }));
    }
}

fn push_unresolved_catalog_reference_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_core::results::UnresolvedCatalogReferenceFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for finding in findings {
        let finding = &finding.reference;
        let path = cc_path(&finding.path, root);
        let line_str = finding.line.to_string();
        let fp = fingerprint_hash(&[
            "fallow/unresolved-catalog-reference",
            &path,
            &line_str,
            &finding.catalog_name,
            &finding.entry_name,
        ]);
        let catalog_phrase = if finding.catalog_name == "default" {
            "the default catalog".to_string()
        } else {
            format!("catalog '{}'", finding.catalog_name)
        };
        let mut description = format!(
            "Package '{}' is referenced via `catalog:{}` but {} does not declare it; `pnpm install` will fail",
            finding.entry_name,
            if finding.catalog_name == "default" {
                ""
            } else {
                finding.catalog_name.as_str()
            },
            catalog_phrase,
        );
        if !finding.available_in_catalogs.is_empty() {
            use std::fmt::Write as _;
            let _ = write!(
                description,
                " (available in: {})",
                finding.available_in_catalogs.join(", ")
            );
        }
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/unresolved-catalog-reference",
            description: &description,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: Some(finding.line),
            fingerprint: &fp,
        }));
    }
}

fn push_empty_catalog_group_issues(
    issues: &mut Vec<CodeClimateIssue>,
    groups: &[fallow_core::results::EmptyCatalogGroupFinding],
    root: &Path,
    severity: Severity,
) {
    if groups.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for group in groups {
        let group = &group.group;
        let path = cc_path(&group.path, root);
        let line_str = group.line.to_string();
        let fp = fingerprint_hash(&[
            "fallow/empty-catalog-group",
            &path,
            &line_str,
            &group.catalog_name,
        ]);
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/empty-catalog-group",
            description: &format!("Catalog group '{}' has no entries", group.catalog_name),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: Some(group.line),
            fingerprint: &fp,
        }));
    }
}

fn push_unused_dependency_override_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_core::results::UnusedDependencyOverrideFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for finding in findings {
        let finding = &finding.entry;
        let path = cc_path(&finding.path, root);
        let line_str = finding.line.to_string();
        let fp = fingerprint_hash(&[
            "fallow/unused-dependency-override",
            &path,
            &line_str,
            finding.source.as_label(),
            &finding.raw_key,
        ]);
        let mut description = format!(
            "Override `{}` forces version `{}` but `{}` is not declared by any workspace package or resolved in pnpm-lock.yaml",
            finding.raw_key, finding.version_range, finding.target_package,
        );
        if let Some(hint) = &finding.hint {
            use std::fmt::Write as _;
            let _ = write!(description, " ({hint})");
        }
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/unused-dependency-override",
            description: &description,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: Some(finding.line),
            fingerprint: &fp,
        }));
    }
}

fn push_misconfigured_dependency_override_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_core::results::MisconfiguredDependencyOverrideFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for finding in findings {
        let finding = &finding.entry;
        let path = cc_path(&finding.path, root);
        let line_str = finding.line.to_string();
        let fp = fingerprint_hash(&[
            "fallow/misconfigured-dependency-override",
            &path,
            &line_str,
            finding.source.as_label(),
            &finding.raw_key,
        ]);
        let description = format!(
            "Override `{}` -> `{}` is malformed: {}",
            finding.raw_key,
            finding.raw_value,
            finding.reason.describe(),
        );
        issues.push(cc_issue(CcIssue {
            check_name: "fallow/misconfigured-dependency-override",
            description: &description,
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: Some(finding.line),
            fingerprint: &fp,
        }));
    }
}

/// Serialize a typed CodeClimate issue list to the wire-shape JSON array.
/// Centralizes the `serde_json::to_value(&issues)` conversion used by every
/// callsite that needs a `serde_json::Value` (PR comment, review envelope,
/// CodeClimate format dispatch, combined / audit aggregation).
///
/// Infallible: `CodeClimateIssue` only contains `String`, `u32`, and enum
/// variants serialized as kebab-case strings; serde_json cannot fail on
/// these shapes.
#[must_use]
#[expect(
    clippy::expect_used,
    reason = "CodeClimateIssue contains only infallibly serializable fields"
)]
pub fn issues_to_value(issues: &[CodeClimateIssue]) -> serde_json::Value {
    serde_json::to_value(issues).expect("CodeClimateIssue serializes infallibly")
}

/// Build CodeClimate issues from dead-code analysis results.
///
/// Returns the typed [`CodeClimateIssue`] vec; callers that emit the wire
/// shape convert via [`issues_to_value`]. The schema drift gate locks the
/// per-issue shape against [`CodeClimateOutput`](
/// crate::output_envelope::CodeClimateOutput).
#[must_use]
pub fn build_codeclimate(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
) -> Vec<CodeClimateIssue> {
    CodeClimateBuilder {
        issues: Vec::new(),
        results,
        root,
        rules,
    }
    .build()
}

struct CodeClimateBuilder<'a> {
    issues: Vec<CodeClimateIssue>,
    results: &'a AnalysisResults,
    root: &'a Path,
    rules: &'a RulesConfig,
}

impl CodeClimateBuilder<'_> {
    fn build(mut self) -> Vec<CodeClimateIssue> {
        self.push_file_and_export_issues();
        self.push_private_type_leak_issues();
        self.push_package_dependency_issues();
        self.push_type_test_dependency_issues();
        self.push_member_issues();
        self.push_import_and_duplicate_issues();
        self.push_graph_issues();
        self.push_boundary_issues();
        self.push_suppression_and_catalog_issues();
        self.push_override_issues();
        self.issues
    }

    fn push_file_and_export_issues(&mut self) {
        push_unused_file_issues(
            &mut self.issues,
            &self.results.unused_files,
            self.root,
            self.rules.unused_files,
        );
        push_unused_export_issues(UnusedExportIssuesInput {
            issues: &mut self.issues,
            exports: self.results.unused_exports.iter().map(|e| &e.export),
            root: self.root,
            rule_id: "fallow/unused-export",
            direct_label: "Export",
            re_export_label: "Re-export",
            severity: self.rules.unused_exports,
        });
        push_unused_export_issues(UnusedExportIssuesInput {
            issues: &mut self.issues,
            exports: self.results.unused_types.iter().map(|e| &e.export),
            root: self.root,
            rule_id: "fallow/unused-type",
            direct_label: "Type export",
            re_export_label: "Type re-export",
            severity: self.rules.unused_types,
        });
    }

    fn push_private_type_leak_issues(&mut self) {
        push_private_type_leak_issues(
            &mut self.issues,
            &self.results.private_type_leaks,
            self.root,
            self.rules.private_type_leaks,
        );
    }

    fn push_package_dependency_issues(&mut self) {
        push_dep_cc_issues(
            &mut self.issues,
            self.results.unused_dependencies.iter().map(|f| &f.dep),
            self.root,
            "fallow/unused-dependency",
            "dependencies",
            self.rules.unused_dependencies,
        );
        push_dep_cc_issues(
            &mut self.issues,
            self.results.unused_dev_dependencies.iter().map(|f| &f.dep),
            self.root,
            "fallow/unused-dev-dependency",
            "devDependencies",
            self.rules.unused_dev_dependencies,
        );
        push_dep_cc_issues(
            &mut self.issues,
            self.results
                .unused_optional_dependencies
                .iter()
                .map(|f| &f.dep),
            self.root,
            "fallow/unused-optional-dependency",
            "optionalDependencies",
            self.rules.unused_optional_dependencies,
        );
    }

    fn push_type_test_dependency_issues(&mut self) {
        push_type_only_dep_issues(
            &mut self.issues,
            &self.results.type_only_dependencies,
            self.root,
            self.rules.type_only_dependencies,
        );
        push_test_only_dep_issues(
            &mut self.issues,
            &self.results.test_only_dependencies,
            self.root,
            self.rules.test_only_dependencies,
        );
    }

    fn push_member_issues(&mut self) {
        push_unused_member_issues(
            &mut self.issues,
            self.results.unused_enum_members.iter().map(|m| &m.member),
            self.root,
            "fallow/unused-enum-member",
            "Enum",
            self.rules.unused_enum_members,
        );
        push_unused_member_issues(
            &mut self.issues,
            self.results.unused_class_members.iter().map(|m| &m.member),
            self.root,
            "fallow/unused-class-member",
            "Class",
            self.rules.unused_class_members,
        );
        push_unused_member_issues(
            &mut self.issues,
            self.results.unused_store_members.iter().map(|m| &m.member),
            self.root,
            "fallow/unused-store-member",
            "Store",
            self.rules.unused_store_members,
        );
    }

    fn push_import_and_duplicate_issues(&mut self) {
        push_unresolved_import_issues(
            &mut self.issues,
            &self.results.unresolved_imports,
            self.root,
            self.rules.unresolved_imports,
        );
        push_unlisted_dep_issues(
            &mut self.issues,
            &self.results.unlisted_dependencies,
            self.root,
            self.rules.unlisted_dependencies,
        );
        push_duplicate_export_issues(
            &mut self.issues,
            &self.results.duplicate_exports,
            self.root,
            self.rules.duplicate_exports,
        );
    }

    fn push_graph_issues(&mut self) {
        push_circular_dep_issues(
            &mut self.issues,
            &self.results.circular_dependencies,
            self.root,
            self.rules.circular_dependencies,
        );
        push_re_export_cycle_issues(
            &mut self.issues,
            &self.results.re_export_cycles,
            self.root,
            self.rules.re_export_cycle,
        );
    }

    fn push_boundary_issues(&mut self) {
        self.push_architecture_boundary_issues();
        self.push_client_server_boundary_issues();
        self.push_component_boundary_issues();
        self.push_framework_route_issues();
    }

    fn push_architecture_boundary_issues(&mut self) {
        push_boundary_violation_issues(
            &mut self.issues,
            &self.results.boundary_violations,
            self.root,
            self.rules.boundary_violation,
        );
        push_boundary_coverage_issues(
            &mut self.issues,
            &self.results.boundary_coverage_violations,
            self.root,
            self.rules.boundary_violation,
        );
        push_boundary_call_issues(
            &mut self.issues,
            &self.results.boundary_call_violations,
            self.root,
            self.rules.boundary_violation,
        );
        push_policy_violation_issues(&mut self.issues, &self.results.policy_violations, self.root);
    }

    fn push_client_server_boundary_issues(&mut self) {
        push_invalid_client_export_issues(
            &mut self.issues,
            &self.results.invalid_client_exports,
            self.root,
            self.rules.invalid_client_export,
        );
        push_mixed_client_server_barrel_issues(
            &mut self.issues,
            &self.results.mixed_client_server_barrels,
            self.root,
            self.rules.mixed_client_server_barrel,
        );
        push_misplaced_directive_issues(
            &mut self.issues,
            &self.results.misplaced_directives,
            self.root,
            self.rules.misplaced_directive,
        );
    }

    fn push_component_boundary_issues(&mut self) {
        push_unprovided_inject_issues(
            &mut self.issues,
            &self.results.unprovided_injects,
            self.root,
            self.rules.unprovided_injects,
        );
        push_unrendered_component_issues(
            &mut self.issues,
            &self.results.unrendered_components,
            self.root,
            self.rules.unrendered_components,
        );
        push_unused_component_prop_issues(
            &mut self.issues,
            &self.results.unused_component_props,
            self.root,
            self.rules.unused_component_props,
        );
        push_unused_component_emit_issues(
            &mut self.issues,
            &self.results.unused_component_emits,
            self.root,
            self.rules.unused_component_emits,
        );
        push_unused_component_input_issues(
            &mut self.issues,
            &self.results.unused_component_inputs,
            self.root,
            self.rules.unused_component_inputs,
        );
        push_unused_component_output_issues(
            &mut self.issues,
            &self.results.unused_component_outputs,
            self.root,
            self.rules.unused_component_outputs,
        );
        push_unused_svelte_event_issues(
            &mut self.issues,
            &self.results.unused_svelte_events,
            self.root,
            self.rules.unused_svelte_events,
        );
    }

    fn push_framework_route_issues(&mut self) {
        push_unused_server_action_issues(
            &mut self.issues,
            &self.results.unused_server_actions,
            self.root,
            self.rules.unused_server_actions,
        );
        push_unused_load_data_key_issues(
            &mut self.issues,
            &self.results.unused_load_data_keys,
            self.root,
            self.rules.unused_load_data_keys,
        );
        push_route_collision_issues(
            &mut self.issues,
            &self.results.route_collisions,
            self.root,
            self.rules.route_collision,
        );
        push_dynamic_segment_name_conflict_issues(
            &mut self.issues,
            &self.results.dynamic_segment_name_conflicts,
            self.root,
            self.rules.dynamic_segment_name_conflict,
        );
    }

    fn push_suppression_and_catalog_issues(&mut self) {
        push_stale_suppression_issues(
            &mut self.issues,
            &self.results.stale_suppressions,
            self.root,
            self.rules,
        );
        push_unused_catalog_entry_issues(
            &mut self.issues,
            &self.results.unused_catalog_entries,
            self.root,
            self.rules.unused_catalog_entries,
        );
        push_empty_catalog_group_issues(
            &mut self.issues,
            &self.results.empty_catalog_groups,
            self.root,
            self.rules.empty_catalog_groups,
        );
        push_unresolved_catalog_reference_issues(
            &mut self.issues,
            &self.results.unresolved_catalog_references,
            self.root,
            self.rules.unresolved_catalog_references,
        );
    }

    fn push_override_issues(&mut self) {
        push_unused_dependency_override_issues(
            &mut self.issues,
            &self.results.unused_dependency_overrides,
            self.root,
            self.rules.unused_dependency_overrides,
        );
        push_misconfigured_dependency_override_issues(
            &mut self.issues,
            &self.results.misconfigured_dependency_overrides,
            self.root,
            self.rules.misconfigured_dependency_overrides,
        );
    }
}

/// Print dead-code analysis results in CodeClimate format.
pub(super) fn print_codeclimate(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
) -> ExitCode {
    let issues = build_codeclimate(results, root, rules);
    let value = issues_to_value(&issues);
    emit_json(&value, "CodeClimate")
}

/// Print CodeClimate output with owner properties added to each issue.
///
/// Calls `build_codeclimate` to produce the standard CodeClimate JSON array,
/// then post-processes each entry to add `"owner": "@team"` by resolving the
/// issue's location path through the `OwnershipResolver`.
#[expect(
    clippy::expect_used,
    reason = "grouped CodeClimate entries are JSON objects created by issues_to_value"
)]
pub(super) fn print_grouped_codeclimate(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    resolver: &OwnershipResolver,
) -> ExitCode {
    let issues = build_codeclimate(results, root, rules);
    let mut value = issues_to_value(&issues);

    if let Some(items) = value.as_array_mut() {
        for issue in items {
            let path = issue
                .pointer("/location/path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let owner = grouping::resolve_owner(Path::new(path), Path::new(""), resolver);
            issue
                .as_object_mut()
                .expect("CodeClimate issue should be an object")
                .insert("owner".to_string(), serde_json::Value::String(owner));
        }
    }

    emit_json(&value, "CodeClimate")
}

/// Build CodeClimate JSON array from health/complexity analysis results.
#[must_use]
pub fn build_health_codeclimate(report: &HealthReport, root: &Path) -> Vec<CodeClimateIssue> {
    let mut issues = Vec::new();
    let ctx = HealthCodeClimateContext {
        root,
        cyc_t: report.summary.max_cyclomatic_threshold,
        cog_t: report.summary.max_cognitive_threshold,
        crap_t: report.summary.max_crap_threshold,
    };

    for finding in &report.findings {
        issues.push(ctx.complexity_issue(finding));
    }

    if let Some(ref production) = report.runtime_coverage {
        for finding in &production.findings {
            issues.push(ctx.runtime_coverage_issue(finding));
        }
    }

    if let Some(ref intelligence) = report.coverage_intelligence {
        for finding in &intelligence.findings {
            if let Some(issue) = ctx.coverage_intelligence_issue(finding) {
                issues.push(issue);
            }
        }
    }

    if let Some(ref gaps) = report.coverage_gaps {
        for item in &gaps.files {
            issues.push(ctx.untested_file_issue(item));
        }

        for item in &gaps.exports {
            issues.push(ctx.untested_export_issue(item));
        }
    }

    issues
}

/// Print health analysis results in CodeClimate format.
pub(super) fn print_health_codeclimate(report: &HealthReport, root: &Path) -> ExitCode {
    let issues = build_health_codeclimate(report, root);
    let value = issues_to_value(&issues);
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
#[expect(
    clippy::expect_used,
    reason = "grouped health CodeClimate entries are JSON objects created by issues_to_value"
)]
pub(super) fn print_grouped_health_codeclimate(
    report: &HealthReport,
    root: &Path,
    resolver: &OwnershipResolver,
) -> ExitCode {
    let issues = build_health_codeclimate(report, root);
    let mut value = issues_to_value(&issues);

    if let Some(items) = value.as_array_mut() {
        for issue in items {
            let path = issue
                .pointer("/location/path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let group = grouping::resolve_owner(Path::new(path), Path::new(""), resolver);
            issue
                .as_object_mut()
                .expect("CodeClimate issue should be an object")
                .insert("group".to_string(), serde_json::Value::String(group));
        }
    }

    emit_json(&value, "CodeClimate")
}

/// Build CodeClimate JSON array from duplication analysis results.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "line numbers are bounded by source size"
)]
pub fn build_duplication_codeclimate(
    report: &DuplicationReport,
    root: &Path,
) -> Vec<CodeClimateIssue> {
    let mut issues = Vec::new();

    for (i, group) in report.clone_groups.iter().enumerate() {
        let token_str = group.token_count.to_string();
        let line_count_str = group.line_count.to_string();
        let fragment_prefix: String = group
            .instances
            .first()
            .map(|inst| inst.fragment.chars().take(64).collect())
            .unwrap_or_default();

        for instance in &group.instances {
            let path = cc_path(&instance.file, root);
            let start_str = instance.start_line.to_string();
            let fp = fingerprint_hash(&[
                "fallow/code-duplication",
                &path,
                &start_str,
                &token_str,
                &line_count_str,
                &fragment_prefix,
            ]);
            issues.push(cc_issue(CcIssue {
                check_name: "fallow/code-duplication",
                description: &format!(
                    "Code clone group {} ({} lines, {} instances)",
                    i + 1,
                    group.line_count,
                    group.instances.len()
                ),
                severity: CodeClimateSeverity::Minor,
                category: "Duplication",
                path: &path,
                begin_line: Some(instance.start_line as u32),
                fingerprint: &fp,
            }));
        }
    }

    issues
}

/// Print duplication analysis results in CodeClimate format.
pub(super) fn print_duplication_codeclimate(report: &DuplicationReport, root: &Path) -> ExitCode {
    let issues = build_duplication_codeclimate(report, root);
    let value = issues_to_value(&issues);
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
#[expect(
    clippy::expect_used,
    reason = "grouped duplication CodeClimate entries are JSON objects created by issues_to_value"
)]
pub(super) fn print_grouped_duplication_codeclimate(
    report: &DuplicationReport,
    root: &Path,
    resolver: &OwnershipResolver,
) -> ExitCode {
    let issues = build_duplication_codeclimate(report, root);
    let mut value = issues_to_value(&issues);

    use rustc_hash::FxHashMap;
    let mut path_to_owner: FxHashMap<String, String> = FxHashMap::default();
    for group in &report.clone_groups {
        let owner = super::dupes_grouping::largest_owner(group, root, resolver);
        for instance in &group.instances {
            let path = cc_path(&instance.file, root);
            path_to_owner.insert(path, owner.clone());
        }
    }

    if let Some(items) = value.as_array_mut() {
        for issue in items {
            let path = issue
                .pointer("/location/path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let group = path_to_owner
                .get(&path)
                .cloned()
                .unwrap_or_else(|| crate::codeowners::UNOWNED_LABEL.to_string());
            issue
                .as_object_mut()
                .expect("CodeClimate issue should be an object")
                .insert("group".to_string(), serde_json::Value::String(group));
        }
    }

    emit_json(&value, "CodeClimate")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::test_helpers::sample_results;
    use fallow_config::RulesConfig;
    use fallow_core::results::*;
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
        let arr = output.as_array().unwrap();
        assert!(arr.is_empty());
    }

    #[test]
    fn codeclimate_produces_array_of_issues() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let rules = RulesConfig::default();
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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

        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));

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

        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));

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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
        assert_eq!(output[0]["severity"], "major");

        let rules = RulesConfig {
            unused_files: Severity::Warn,
            ..RulesConfig::default()
        };
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output1 = issues_to_value(&build_codeclimate(&results, &root, &rules));
        let output2 = issues_to_value(&build_codeclimate(&results, &root, &rules));

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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));

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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
    /// `push_unused_member_issues`) through `build_codeclimate`.
    #[test]
    fn build_codeclimate_with_off_severity_and_empty_findings_does_not_panic() {
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
        let issues = build_codeclimate(&results, &root, &rules);
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
        use crate::health_types::*;

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

        let output = issues_to_value(&build_health_codeclimate(&report, &root));
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
        use crate::health_types::{
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

        let output = issues_to_value(&build_health_codeclimate(&report, &root));
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
        use crate::health_types::{
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

        let issues = build_health_codeclimate(&report, &root);
        assert!(issues.is_empty());
    }

    #[test]
    fn health_codeclimate_crap_only_uses_crap_check_name() {
        use crate::health_types::{
            ComplexityViolation, FindingSeverity, HealthReport, HealthSummary,
        };
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
                    exceeded: crate::health_types::ExceededThreshold::Crap,
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
        let json = issues_to_value(&build_health_codeclimate(&report, &root));
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
        use crate::health_types::CoverageIntelligenceRecommendation;
        assert_eq!(
            coverage_intelligence_check_name(
                CoverageIntelligenceRecommendation::ReviewBeforeChanging
            ),
            "fallow/coverage-intelligence-review"
        );
    }

    #[test]
    fn coverage_intelligence_check_name_refactor_carefully() {
        use crate::health_types::CoverageIntelligenceRecommendation;
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
        use crate::health_types::{ComplexityViolation, ExceededThreshold, FindingSeverity};
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
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
        let json = issues_to_value(&build_health_codeclimate(&report, &root));
        let issues = json.as_array().unwrap();
        assert_eq!(issues[0]["check_name"], "fallow/high-complexity");
        let desc = issues[0]["description"].as_str().unwrap();
        assert!(desc.contains("cyclomatic complexity"), "desc: {desc}");
        assert!(desc.contains("cognitive complexity"), "desc: {desc}");
    }

    #[test]
    fn complexity_description_cyclomatic_only() {
        use crate::health_types::{ComplexityViolation, ExceededThreshold, FindingSeverity};
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
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
        let json = issues_to_value(&build_health_codeclimate(&report, &root));
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
        use crate::health_types::{ComplexityViolation, ExceededThreshold, FindingSeverity};
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
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
        let json = issues_to_value(&build_health_codeclimate(&report, &root));
        let issues = json.as_array().unwrap();
        assert_eq!(issues[0]["check_name"], "fallow/high-cognitive-complexity");
        let desc = issues[0]["description"].as_str().unwrap();
        assert!(desc.contains("cognitive complexity"), "desc: {desc}");
        assert!(!desc.contains("cyclomatic"), "desc: {desc}");
    }

    #[test]
    fn complexity_description_all_threshold_crap_no_coverage() {
        use crate::health_types::{ComplexityViolation, ExceededThreshold, FindingSeverity};
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
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
        let json = issues_to_value(&build_health_codeclimate(&report, &root));
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
        use crate::health_types::{
            CoverageIntelligenceAction, CoverageIntelligenceConfidence,
            CoverageIntelligenceEvidence, CoverageIntelligenceFinding,
            CoverageIntelligenceMatchConfidence, CoverageIntelligenceRecommendation,
            CoverageIntelligenceReport, CoverageIntelligenceSchemaVersion,
            CoverageIntelligenceSummary, CoverageIntelligenceVerdict,
        };
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
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
        let json = issues_to_value(&build_health_codeclimate(&report, &root));
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
        use crate::health_types::{
            CoverageIntelligenceAction, CoverageIntelligenceConfidence,
            CoverageIntelligenceEvidence, CoverageIntelligenceFinding,
            CoverageIntelligenceMatchConfidence, CoverageIntelligenceRecommendation,
            CoverageIntelligenceReport, CoverageIntelligenceSchemaVersion,
            CoverageIntelligenceSummary, CoverageIntelligenceVerdict,
        };
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
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
        let issues = build_health_codeclimate(&report, &root);
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
        use crate::health_types::{
            CoverageGapSummary, CoverageGaps, UntestedFile, UntestedFileFinding,
        };
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
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
        let json = issues_to_value(&build_health_codeclimate(&report, &root));
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
        use crate::health_types::{
            CoverageGapSummary, CoverageGaps, UntestedFile, UntestedFileFinding,
        };
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
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
        let json = issues_to_value(&build_health_codeclimate(&report, &root));
        let issues = json.as_array().unwrap();
        let desc = issues[0]["description"].as_str().unwrap();
        assert!(desc.contains("3 value exports)"), "plural: {desc}");
    }

    // ---------------------------------------------------------------------------
    // health_finding_severity: Critical -> Critical, Moderate -> Minor
    // ---------------------------------------------------------------------------

    #[test]
    fn health_finding_severity_critical_maps_to_critical() {
        use crate::health_types::FindingSeverity;
        assert_eq!(
            health_finding_severity(FindingSeverity::Critical),
            CodeClimateSeverity::Critical
        );
    }

    #[test]
    fn health_finding_severity_moderate_maps_to_minor() {
        use crate::health_types::FindingSeverity;
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
        use crate::health_types::RuntimeCoverageVerdict;
        assert_eq!(
            runtime_coverage_check_name(RuntimeCoverageVerdict::SafeToDelete),
            "fallow/runtime-safe-to-delete"
        );
    }

    #[test]
    fn runtime_coverage_check_name_low_traffic() {
        use crate::health_types::RuntimeCoverageVerdict;
        assert_eq!(
            runtime_coverage_check_name(RuntimeCoverageVerdict::LowTraffic),
            "fallow/runtime-low-traffic"
        );
    }

    #[test]
    fn runtime_coverage_check_name_coverage_unavailable() {
        use crate::health_types::RuntimeCoverageVerdict;
        assert_eq!(
            runtime_coverage_check_name(RuntimeCoverageVerdict::CoverageUnavailable),
            "fallow/runtime-coverage-unavailable"
        );
    }

    #[test]
    fn runtime_coverage_check_name_active_and_unknown_use_generic() {
        use crate::health_types::RuntimeCoverageVerdict;
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
        use crate::health_types::RuntimeCoverageVerdict;
        assert_eq!(
            runtime_coverage_severity(RuntimeCoverageVerdict::SafeToDelete),
            CodeClimateSeverity::Critical
        );
    }

    #[test]
    fn runtime_coverage_severity_review_required_is_major() {
        use crate::health_types::RuntimeCoverageVerdict;
        assert_eq!(
            runtime_coverage_severity(RuntimeCoverageVerdict::ReviewRequired),
            CodeClimateSeverity::Major
        );
    }

    #[test]
    fn runtime_coverage_severity_other_verdicts_are_minor() {
        use crate::health_types::RuntimeCoverageVerdict;
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
        use crate::health_types::CoverageIntelligenceVerdict;
        assert_eq!(
            coverage_intelligence_severity(CoverageIntelligenceVerdict::ReviewRequired),
            Some(CodeClimateSeverity::Minor)
        );
    }

    #[test]
    fn coverage_intelligence_severity_refactor_carefully_is_minor() {
        use crate::health_types::CoverageIntelligenceVerdict;
        assert_eq!(
            coverage_intelligence_severity(CoverageIntelligenceVerdict::RefactorCarefully),
            Some(CodeClimateSeverity::Minor)
        );
    }

    #[test]
    fn coverage_intelligence_severity_clean_is_none() {
        use crate::health_types::CoverageIntelligenceVerdict;
        assert_eq!(
            coverage_intelligence_severity(CoverageIntelligenceVerdict::Clean),
            None
        );
    }

    #[test]
    fn coverage_intelligence_severity_unknown_is_none() {
        use crate::health_types::CoverageIntelligenceVerdict;
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/unused-dependency-override");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("react"), "desc: {desc}");
        assert!(desc.contains("verify lockfile"), "desc: {desc}");
    }

    // ---------------------------------------------------------------------------
    // build_health_codeclimate: runtime coverage findings
    // ---------------------------------------------------------------------------

    #[test]
    fn health_codeclimate_runtime_safe_to_delete_maps_to_critical_severity() {
        use crate::health_types::{
            RuntimeCoverageConfidence, RuntimeCoverageDataSource, RuntimeCoverageEvidence,
            RuntimeCoverageFinding, RuntimeCoverageReport, RuntimeCoverageReportVerdict,
            RuntimeCoverageSchemaVersion, RuntimeCoverageSummary, RuntimeCoverageVerdict,
        };
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
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
                provenance: crate::health_types::RuntimeCoverageProvenance::default(),
            }),
            ..Default::default()
        };
        let json = issues_to_value(&build_health_codeclimate(&report, &root));
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
        use crate::health_types::{
            RuntimeCoverageConfidence, RuntimeCoverageDataSource, RuntimeCoverageEvidence,
            RuntimeCoverageFinding, RuntimeCoverageReport, RuntimeCoverageReportVerdict,
            RuntimeCoverageSchemaVersion, RuntimeCoverageSummary, RuntimeCoverageVerdict,
        };
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
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
                provenance: crate::health_types::RuntimeCoverageProvenance::default(),
            }),
            ..Default::default()
        };
        let json = issues_to_value(&build_health_codeclimate(&report, &root));
        let issues = json.as_array().unwrap();
        let desc = issues[0]["description"].as_str().unwrap();
        assert!(desc.contains("untracked"), "desc: {desc}");
    }

    // ---------------------------------------------------------------------------
    // build_duplication_codeclimate
    // ---------------------------------------------------------------------------

    #[test]
    fn duplication_codeclimate_one_issue_per_instance() {
        use fallow_core::duplicates::{
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
        let output = issues_to_value(&build_duplication_codeclimate(&report, &root));
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
        use fallow_core::duplicates::{
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
        let output = issues_to_value(&build_duplication_codeclimate(&report, &root));
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
        use fallow_core::duplicates::{DuplicationReport, DuplicationStats};
        let root = PathBuf::from("/project");
        let report = DuplicationReport {
            clone_groups: vec![],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats::default(),
        };
        let output = issues_to_value(&build_duplication_codeclimate(&report, &root));
        assert!(output.as_array().unwrap().is_empty());
    }

    #[test]
    fn duplication_codeclimate_fingerprints_are_unique_across_instances() {
        use fallow_core::duplicates::{
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
        let output = issues_to_value(&build_duplication_codeclimate(&report, &root));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
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
        let output = issues_to_value(&build_codeclimate(&results, &root, &rules));
        assert_eq!(output[0]["check_name"], "fallow/unused-type");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("Type re-export"), "desc: {desc}");
    }
}
