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
fn cc_issue(
    check_name: &str,
    description: &str,
    severity: CodeClimateSeverity,
    category: &str,
    path: &str,
    begin_line: Option<u32>,
    fingerprint: &str,
) -> CodeClimateIssue {
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
        cc_issue(
            check_name,
            &self.complexity_description(finding),
            health_finding_severity(finding.severity),
            "Complexity",
            &path,
            Some(finding.line),
            &fp,
        )
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
        cc_issue(
            check_name,
            &description,
            runtime_coverage_severity(finding.verdict),
            "Bug Risk",
            &path,
            Some(finding.line),
            &fp,
        )
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
        Some(cc_issue(
            check_name,
            &description,
            severity,
            "Bug Risk",
            &path,
            Some(finding.line),
            &fp,
        ))
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
        cc_issue(
            "fallow/untested-file",
            &description,
            CodeClimateSeverity::Minor,
            "Coverage",
            &path,
            None,
            &fp,
        )
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
        cc_issue(
            "fallow/untested-export",
            &description,
            CodeClimateSeverity::Minor,
            "Coverage",
            &path,
            Some(item.export.line),
            &fp,
        )
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
        issues.push(cc_issue(
            rule_id,
            &format!(
                "Package '{}' is in {location_label} but never imported{workspace_context}",
                dep.package_name
            ),
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/unused-file",
            "File is not reachable from any entry point",
            level,
            "Bug Risk",
            &path,
            None,
            &fp,
        ));
    }
}

/// Push CodeClimate issues for unused exports or unused types.
///
/// `direct_label` / `re_export_label` let the same helper produce the right
/// prose for both `unused-export` (Export / Re-export) and `unused-type`
/// (Type export / Type re-export) rule ids.
fn push_unused_export_issues<'a, I>(
    issues: &mut Vec<CodeClimateIssue>,
    exports: I,
    root: &Path,
    rule_id: &str,
    direct_label: &str,
    re_export_label: &str,
    severity: Severity,
) where
    I: IntoIterator<Item = &'a fallow_core::results::UnusedExport>,
{
    for export in exports {
        let level = severity_to_codeclimate(severity);
        let path = cc_path(&export.path, root);
        let kind = if export.is_re_export {
            re_export_label
        } else {
            direct_label
        };
        let line_str = export.line.to_string();
        let fp = fingerprint_hash(&[rule_id, &path, &line_str, &export.export_name]);
        issues.push(cc_issue(
            rule_id,
            &format!(
                "{kind} '{}' is never imported by other modules",
                export.export_name
            ),
            level,
            "Bug Risk",
            &path,
            Some(export.line),
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/private-type-leak",
            &format!(
                "Export '{}' references private type '{}'",
                leak.export_name, leak.type_name
            ),
            level,
            "Bug Risk",
            &path,
            Some(leak.line),
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/type-only-dependency",
            &format!(
                "Package '{}' is only imported via type-only imports (consider moving to devDependencies)",
                dep.package_name
            ),
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/test-only-dependency",
            &format!(
                "Package '{}' is only imported by test files (consider moving to devDependencies)",
                dep.package_name
            ),
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
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
        issues.push(cc_issue(
            rule_id,
            &format!(
                "{entity_label} member '{}.{}' is never referenced",
                member.parent_name, member.member_name
            ),
            level,
            "Bug Risk",
            &path,
            Some(member.line),
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/unresolved-import",
            &format!("Import '{}' could not be resolved", import.specifier),
            level,
            "Bug Risk",
            &path,
            Some(import.line),
            &fp,
        ));
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
            issues.push(cc_issue(
                "fallow/unlisted-dependency",
                &format!(
                    "Package '{}' is imported but not listed in package.json",
                    dep.package_name
                ),
                level,
                "Bug Risk",
                &path,
                Some(site.line),
                &fp,
            ));
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
            issues.push(cc_issue(
                "fallow/duplicate-export",
                &format!("Export '{}' appears in multiple modules", dup.export_name),
                level,
                "Bug Risk",
                &path,
                Some(loc.line),
                &fp,
            ));
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
        issues.push(cc_issue(
            "fallow/circular-dependency",
            &format!(
                "Circular dependency{}: {}",
                if cycle.is_cross_package {
                    " (cross-package)"
                } else {
                    ""
                },
                chain.join(" \u{2192} ")
            ),
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/re-export-cycle",
            &format!("Re-export cycle{}: {}", kind_tag, chain.join(" <-> ")),
            level,
            "Bug Risk",
            &path,
            None,
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/boundary-violation",
            &format!(
                "Boundary violation: {} -> {} ({} -> {})",
                path, to, v.from_zone, v.to_zone
            ),
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/boundary-coverage",
            &format!("Boundary coverage: {path} matches no configured zone"),
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/boundary-call-violation",
            &format!(
                "Boundary call: `{}` matches forbidden pattern `{}` in zone '{}'",
                v.callee, v.pattern, v.zone
            ),
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/policy-violation",
            &message,
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/invalid-client-export",
            &message,
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/mixed-client-server-barrel",
            &message,
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/misplaced-directive",
            &message,
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/unprovided-inject",
            &message,
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/unrendered-component",
            &message,
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/unused-component-prop",
            &message,
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/unused-component-emit",
            &message,
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/unused-server-action",
            &message,
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/unused-load-data-key",
            &message,
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/route-collision",
            &message,
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/dynamic-segment-name-conflict",
            &message,
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
    }
}

fn push_stale_suppression_issues(
    issues: &mut Vec<CodeClimateIssue>,
    suppressions: &[fallow_core::results::StaleSuppression],
    root: &Path,
    severity: Severity,
) {
    if suppressions.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for s in suppressions {
        let path = cc_path(&s.path, root);
        let line_str = s.line.to_string();
        let fp = fingerprint_hash(&["fallow/stale-suppression", &path, &line_str]);
        issues.push(cc_issue(
            "fallow/stale-suppression",
            &s.display_message(),
            level,
            "Bug Risk",
            &path,
            Some(s.line),
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/unused-catalog-entry",
            &description,
            level,
            "Bug Risk",
            &path,
            Some(entry.line),
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/unresolved-catalog-reference",
            &description,
            level,
            "Bug Risk",
            &path,
            Some(finding.line),
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/empty-catalog-group",
            &format!("Catalog group '{}' has no entries", group.catalog_name),
            level,
            "Bug Risk",
            &path,
            Some(group.line),
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/unused-dependency-override",
            &description,
            level,
            "Bug Risk",
            &path,
            Some(finding.line),
            &fp,
        ));
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
        issues.push(cc_issue(
            "fallow/misconfigured-dependency-override",
            &description,
            level,
            "Bug Risk",
            &path,
            Some(finding.line),
            &fp,
        ));
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
        push_unused_export_issues(
            &mut self.issues,
            self.results.unused_exports.iter().map(|e| &e.export),
            self.root,
            "fallow/unused-export",
            "Export",
            "Re-export",
            self.rules.unused_exports,
        );
        push_unused_export_issues(
            &mut self.issues,
            self.results.unused_types.iter().map(|e| &e.export),
            self.root,
            "fallow/unused-type",
            "Type export",
            "Type re-export",
            self.rules.unused_types,
        );
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
            self.rules.stale_suppressions,
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
            issues.push(cc_issue(
                "fallow/code-duplication",
                &format!(
                    "Code clone group {} ({} lines, {} instances)",
                    i + 1,
                    group.line_count,
                    group.instances.len()
                ),
                CodeClimateSeverity::Minor,
                "Duplication",
                &path,
                Some(instance.start_line as u32),
                &fp,
            ));
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
}
