use crate::report::sink::outln;
use std::path::Path;

use fallow_core::duplicates::{CloneFingerprintSet, DuplicationReport};
use fallow_core::results::{AnalysisResults, UnusedExport, UnusedMember};

use super::grouping::ResultGroup;
use super::{normalize_uri, relative_path};

pub(super) fn print_compact(results: &AnalysisResults, root: &Path) {
    for line in build_compact_lines(results, root) {
        outln!("{line}");
    }
}

fn compact_path(path: &Path, root: &Path) -> String {
    normalize_uri(&relative_path(path, root).display().to_string())
}

fn compact_circular_dependency_line(
    cycle: &fallow_core::results::CircularDependencyFinding,
    root: &Path,
) -> String {
    let chain: Vec<String> = cycle
        .cycle
        .files
        .iter()
        .map(|path| compact_path(path, root))
        .collect();
    let mut display_chain = chain.clone();
    if let Some(first) = chain.first() {
        display_chain.push(first.clone());
    }
    let first_file = chain.first().map_or_else(String::new, Clone::clone);
    let cross_pkg_tag = if cycle.cycle.is_cross_package {
        " (cross-package)"
    } else {
        ""
    };
    format!(
        "circular-dependency:{}:{}:{}{}",
        first_file,
        cycle.cycle.line,
        display_chain.join(" \u{2192} "),
        cross_pkg_tag
    )
}

fn compact_re_export_cycle_line(
    cycle: &fallow_core::results::ReExportCycleFinding,
    root: &Path,
) -> String {
    let chain: Vec<String> = cycle
        .cycle
        .files
        .iter()
        .map(|path| compact_path(path, root))
        .collect();
    let first_file = chain.first().map_or_else(String::new, Clone::clone);
    let kind_tag = match cycle.cycle.kind {
        fallow_core::results::ReExportCycleKind::SelfLoop => " (self-loop)",
        fallow_core::results::ReExportCycleKind::MultiNode => "",
    };
    format!(
        "re-export-cycle:{}:{}{}",
        first_file,
        chain.join(" <-> "),
        kind_tag
    )
}

fn compact_boundary_violation_line(
    item: &fallow_core::results::BoundaryViolationFinding,
    root: &Path,
) -> String {
    format!(
        "boundary-violation:{}:{}:{} -> {} ({} -> {})",
        compact_path(&item.violation.from_path, root),
        item.violation.line,
        compact_path(&item.violation.from_path, root),
        compact_path(&item.violation.to_path, root),
        item.violation.from_zone,
        item.violation.to_zone,
    )
}

fn compact_boundary_coverage_line(
    item: &fallow_core::results::BoundaryCoverageViolationFinding,
    root: &Path,
) -> String {
    format!(
        "boundary-coverage:{}:{}:no matching boundary zone",
        compact_path(&item.violation.path, root),
        item.violation.line,
    )
}

fn compact_boundary_call_line(
    item: &fallow_core::results::BoundaryCallViolationFinding,
    root: &Path,
) -> String {
    format!(
        "boundary-call:{}:{}:{} forbidden in zone {} (pattern {})",
        compact_path(&item.violation.path, root),
        item.violation.line,
        item.violation.callee,
        item.violation.zone,
        item.violation.pattern,
    )
}

fn compact_stale_suppression_line(
    item: &fallow_core::results::StaleSuppression,
    root: &Path,
) -> String {
    format!(
        "stale-suppression:{}:{}:{}",
        compact_path(&item.path, root),
        item.line,
        item.display_message(),
    )
}

fn compact_catalog_reference_line(
    item: &fallow_core::results::UnresolvedCatalogReferenceFinding,
    root: &Path,
) -> String {
    format!(
        "unresolved-catalog-reference:{}:{}:{}:{}",
        compact_path(&item.reference.path, root),
        item.reference.line,
        item.reference.catalog_name,
        item.reference.entry_name,
    )
}

fn compact_unused_override_line(
    item: &fallow_core::results::UnusedDependencyOverrideFinding,
    root: &Path,
) -> String {
    format!(
        "unused-dependency-override:{}:{}:{}:{}",
        compact_path(&item.entry.path, root),
        item.entry.line,
        item.entry.source.as_label(),
        item.entry.raw_key,
    )
}

fn compact_misconfigured_override_line(
    item: &fallow_core::results::MisconfiguredDependencyOverrideFinding,
    root: &Path,
) -> String {
    format!(
        "misconfigured-dependency-override:{}:{}:{}:{}",
        compact_path(&item.entry.path, root),
        item.entry.line,
        item.entry.source.as_label(),
        item.entry.raw_key,
    )
}

/// Build compact output lines for analysis results.
/// Each issue is represented as a single `prefix:details` line.
pub fn build_compact_lines(results: &AnalysisResults, root: &Path) -> Vec<String> {
    CompactLineBuilder::new(results, root).build()
}

struct CompactLineBuilder<'a> {
    lines: Vec<String>,
    results: &'a AnalysisResults,
    root: &'a Path,
}

impl<'a> CompactLineBuilder<'a> {
    fn new(results: &'a AnalysisResults, root: &'a Path) -> Self {
        Self {
            lines: Vec::new(),
            results,
            root,
        }
    }

    fn build(mut self) -> Vec<String> {
        self.push_core_lines();
        self.push_unused_dependency_lines();
        self.push_member_lines();
        self.push_secondary_dependency_lines();
        self.push_graph_lines();
        self.push_workspace_lines();
        self.lines
    }

    fn rel(&self, path: &Path) -> String {
        compact_path(path, self.root)
    }

    fn unused_export_line(&self, export: &UnusedExport) -> String {
        let tag = if export.is_re_export {
            "unused-re-export"
        } else {
            "unused-export"
        };
        format!(
            "{}:{}:{}:{}",
            tag,
            self.rel(&export.path),
            export.line,
            export.export_name
        )
    }

    fn unused_type_line(&self, export: &UnusedExport) -> String {
        let tag = if export.is_re_export {
            "unused-re-export-type"
        } else {
            "unused-type"
        };
        format!(
            "{}:{}:{}:{}",
            tag,
            self.rel(&export.path),
            export.line,
            export.export_name
        )
    }

    fn compact_member(&self, member: &UnusedMember, kind: &str) -> String {
        format!(
            "{}:{}:{}:{}.{}",
            kind,
            self.rel(&member.path),
            member.line,
            member.parent_name,
            member.member_name
        )
    }

    fn push_core_lines(&mut self) {
        for file in &self.results.unused_files {
            self.lines
                .push(format!("unused-file:{}", self.rel(&file.file.path)));
        }
        for export in &self.results.unused_exports {
            self.lines.push(self.unused_export_line(&export.export));
        }
        for export in &self.results.unused_types {
            self.lines.push(self.unused_type_line(&export.export));
        }
        for leak in &self.results.private_type_leaks {
            self.lines.push(format!(
                "private-type-leak:{}:{}:{}->{}",
                self.rel(&leak.leak.path),
                leak.leak.line,
                leak.leak.export_name,
                leak.leak.type_name
            ));
        }
    }

    fn push_unused_dependency_lines(&mut self) {
        for dep in &self.results.unused_dependencies {
            self.lines
                .push(format!("unused-dep:{}", dep.dep.package_name));
        }
        for dep in &self.results.unused_dev_dependencies {
            self.lines
                .push(format!("unused-devdep:{}", dep.dep.package_name));
        }
        for dep in &self.results.unused_optional_dependencies {
            self.lines
                .push(format!("unused-optionaldep:{}", dep.dep.package_name));
        }
    }

    fn push_member_lines(&mut self) {
        for member in &self.results.unused_enum_members {
            self.lines
                .push(self.compact_member(&member.member, "unused-enum-member"));
        }
        for member in &self.results.unused_class_members {
            self.lines
                .push(self.compact_member(&member.member, "unused-class-member"));
        }
        for member in &self.results.unused_store_members {
            self.lines
                .push(self.compact_member(&member.member, "unused-store-member"));
        }
        for import in &self.results.unresolved_imports {
            self.lines.push(format!(
                "unresolved-import:{}:{}:{}",
                self.rel(&import.import.path),
                import.import.line,
                import.import.specifier
            ));
        }
    }

    fn push_secondary_dependency_lines(&mut self) {
        for dep in &self.results.unlisted_dependencies {
            self.lines
                .push(format!("unlisted-dep:{}", dep.dep.package_name));
        }
        for dup in &self.results.duplicate_exports {
            self.lines
                .push(format!("duplicate-export:{}", dup.export.export_name));
        }
        for dep in &self.results.type_only_dependencies {
            self.lines
                .push(format!("type-only-dep:{}", dep.dep.package_name));
        }
        for dep in &self.results.test_only_dependencies {
            self.lines
                .push(format!("test-only-dep:{}", dep.dep.package_name));
        }
    }

    fn push_graph_lines(&mut self) {
        self.push_structure_lines();
        self.push_framework_lines();
        self.push_component_lines();
        self.push_route_lines();
        self.push_suppression_lines();
    }

    fn push_structure_lines(&mut self) {
        for cycle in &self.results.circular_dependencies {
            self.lines
                .push(compact_circular_dependency_line(cycle, self.root));
        }
        for cycle in &self.results.re_export_cycles {
            self.lines
                .push(compact_re_export_cycle_line(cycle, self.root));
        }
        for violation in &self.results.boundary_violations {
            self.lines
                .push(compact_boundary_violation_line(violation, self.root));
        }
        for violation in &self.results.boundary_coverage_violations {
            self.lines
                .push(compact_boundary_coverage_line(violation, self.root));
        }
        for violation in &self.results.boundary_call_violations {
            self.lines
                .push(compact_boundary_call_line(violation, self.root));
        }
        for violation in &self.results.policy_violations {
            self.lines.push(format!(
                "policy-violation:{}:{}:{} banned by {}/{}",
                self.rel(&violation.violation.path),
                violation.violation.line,
                violation.violation.matched,
                violation.violation.pack,
                violation.violation.rule_id,
            ));
        }
    }

    fn push_framework_lines(&mut self) {
        for finding in &self.results.invalid_client_exports {
            self.lines.push(format!(
                "invalid-client-export:{}:{}:{} (from \"{}\")",
                self.rel(&finding.export.path),
                finding.export.line,
                finding.export.export_name,
                finding.export.directive,
            ));
        }
        for finding in &self.results.mixed_client_server_barrels {
            self.lines.push(format!(
                "mixed-client-server-barrel:{}:{}:{} (server-only \"{}\")",
                self.rel(&finding.barrel.path),
                finding.barrel.line,
                finding.barrel.client_origin,
                finding.barrel.server_origin,
            ));
        }
        for finding in &self.results.misplaced_directives {
            self.lines.push(format!(
                "misplaced-directive:{}:{}:{}",
                self.rel(&finding.directive_site.path),
                finding.directive_site.line,
                finding.directive_site.directive,
            ));
        }
        for finding in &self.results.unprovided_injects {
            self.lines.push(format!(
                "unprovided-inject:{}:{}:{}",
                self.rel(&finding.inject.path),
                finding.inject.line,
                finding.inject.key_name,
            ));
        }
    }

    fn push_component_lines(&mut self) {
        self.push_component_member_lines();
        self.push_component_framework_lines();
    }

    /// Push compact lines for unrendered components, props, emits, inputs, and outputs.
    fn push_component_member_lines(&mut self) {
        for finding in &self.results.unrendered_components {
            self.lines.push(format!(
                "unrendered-component:{}:{}:{}",
                self.rel(&finding.component.path),
                finding.component.line,
                finding.component.component_name,
            ));
        }
        for finding in &self.results.unused_component_props {
            self.lines.push(format!(
                "unused-component-prop:{}:{}:{}",
                self.rel(&finding.prop.path),
                finding.prop.line,
                finding.prop.prop_name,
            ));
        }
        for finding in &self.results.unused_component_emits {
            self.lines.push(format!(
                "unused-component-emit:{}:{}:{}",
                self.rel(&finding.emit.path),
                finding.emit.line,
                finding.emit.emit_name,
            ));
        }
        for finding in &self.results.unused_component_inputs {
            self.lines.push(format!(
                "unused-component-input:{}:{}:{}",
                self.rel(&finding.input.path),
                finding.input.line,
                finding.input.input_name,
            ));
        }
        for finding in &self.results.unused_component_outputs {
            self.lines.push(format!(
                "unused-component-output:{}:{}:{}",
                self.rel(&finding.output.path),
                finding.output.line,
                finding.output.output_name,
            ));
        }
    }

    /// Push compact lines for Svelte events, server actions, and load-data keys.
    fn push_component_framework_lines(&mut self) {
        for finding in &self.results.unused_svelte_events {
            self.lines.push(format!(
                "unused-svelte-event:{}:{}:{}",
                self.rel(&finding.event.path),
                finding.event.line,
                finding.event.event_name,
            ));
        }
        for finding in &self.results.unused_server_actions {
            self.lines.push(format!(
                "unused-server-action:{}:{}:{}",
                self.rel(&finding.action.path),
                finding.action.line,
                finding.action.action_name,
            ));
        }
        for finding in &self.results.unused_load_data_keys {
            self.lines.push(format!(
                "unused-load-data-key:{}:{}:{}",
                self.rel(&finding.key.path),
                finding.key.line,
                finding.key.key_name,
            ));
        }
    }

    fn push_route_lines(&mut self) {
        for finding in &self.results.route_collisions {
            self.lines.push(format!(
                "route-collision:{}:{} (url {})",
                self.rel(&finding.collision.path),
                finding.collision.line,
                finding.collision.url,
            ));
        }
        for finding in &self.results.dynamic_segment_name_conflicts {
            self.lines.push(format!(
                "dynamic-segment-name-conflict:{}:{} ({} at {})",
                self.rel(&finding.conflict.path),
                finding.conflict.line,
                finding.conflict.conflicting_segments.join(" vs "),
                finding.conflict.position,
            ));
        }
    }

    fn push_suppression_lines(&mut self) {
        for suppression in &self.results.stale_suppressions {
            self.lines
                .push(compact_stale_suppression_line(suppression, self.root));
        }
    }

    fn push_workspace_lines(&mut self) {
        for entry in &self.results.unused_catalog_entries {
            self.lines.push(format!(
                "unused-catalog-entry:{}:{}:{}:{}",
                self.rel(&entry.entry.path),
                entry.entry.line,
                entry.entry.catalog_name,
                entry.entry.entry_name,
            ));
        }
        for group in &self.results.empty_catalog_groups {
            self.lines.push(format!(
                "empty-catalog-group:{}:{}:{}",
                self.rel(&group.group.path),
                group.group.line,
                group.group.catalog_name,
            ));
        }
        for finding in &self.results.unresolved_catalog_references {
            self.lines
                .push(compact_catalog_reference_line(finding, self.root));
        }
        for finding in &self.results.unused_dependency_overrides {
            self.lines
                .push(compact_unused_override_line(finding, self.root));
        }
        for finding in &self.results.misconfigured_dependency_overrides {
            self.lines
                .push(compact_misconfigured_override_line(finding, self.root));
        }
    }
}

/// Print grouped compact output: each line is prefixed with the group key.
///
/// Format: `group-key\tissue-tag:details`
pub(super) fn print_grouped_compact(groups: &[ResultGroup], root: &Path) {
    for group in groups {
        for line in build_compact_lines(&group.results, root) {
            outln!("{}\t{line}", group.key);
        }
    }
}

pub(super) fn print_health_compact(report: &crate::health_types::HealthReport, root: &Path) {
    print_health_score_compact(report);
    print_vital_signs_compact(report);
    print_health_findings_compact(&report.findings, root);
    print_threshold_overrides_compact(&report.threshold_overrides, root);
    print_file_scores_compact(&report.file_scores, root);
    print_coverage_gaps_compact(report, root);
    print_runtime_sections_compact(report, root);
    print_hotspots_compact(&report.hotspots, root);
    print_health_trend_compact(report);
    print_refactoring_targets_compact(&report.targets, root);
}

fn print_threshold_overrides_compact(
    entries: &[crate::health_types::ThresholdOverrideState],
    root: &Path,
) {
    for entry in entries {
        let status = match entry.status {
            crate::health_types::ThresholdOverrideStatus::Active => "active",
            crate::health_types::ThresholdOverrideStatus::Stale => "stale",
            crate::health_types::ThresholdOverrideStatus::NoMatch => "no_match",
        };
        let target = entry.path.as_ref().map_or_else(
            || "no-match".to_string(),
            |path| {
                let display = health_compact_path(path, root);
                entry
                    .function
                    .as_ref()
                    .map_or_else(|| display.clone(), |name| format!("{display}:{name}"))
            },
        );
        let metrics = entry.metrics.map_or(String::new(), |metrics| {
            let crap = metrics
                .crap
                .map_or(String::new(), |value| format!(",crap={value:.1}"));
            format!(
                ",cyclomatic={},cognitive={}{}",
                metrics.cyclomatic, metrics.cognitive, crap
            )
        });
        outln!(
            "threshold-override:{}:{}:{}{}",
            entry.override_index,
            status,
            target,
            metrics
        );
    }
}

fn print_health_score_compact(report: &crate::health_types::HealthReport) {
    if let Some(ref hs) = report.health_score {
        outln!("health-score:{:.1}:{}", hs.score, hs.grade);
    }
}

fn print_vital_signs_compact(report: &crate::health_types::HealthReport) {
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
        outln!("vital-signs:{}", parts.join(","));
    }
}

fn health_compact_path(path: &Path, root: &Path) -> String {
    normalize_uri(&relative_path(path, root).display().to_string())
}

fn print_health_findings_compact(findings: &[crate::health_types::HealthFinding], root: &Path) {
    for finding in findings {
        let relative = health_compact_path(&finding.path, root);
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
        outln!(
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
}

fn print_file_scores_compact(scores: &[crate::health_types::FileHealthScore], root: &Path) {
    for score in scores {
        let relative = health_compact_path(&score.path, root);
        outln!(
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
}

fn print_coverage_gaps_compact(report: &crate::health_types::HealthReport, root: &Path) {
    if let Some(ref gaps) = report.coverage_gaps {
        outln!(
            "coverage-gap-summary:runtime_files={},covered_files={},file_coverage_pct={:.1},untested_files={},untested_exports={}",
            gaps.summary.runtime_files,
            gaps.summary.covered_files,
            gaps.summary.file_coverage_pct,
            gaps.summary.untested_files,
            gaps.summary.untested_exports,
        );
        for item in &gaps.files {
            let relative = health_compact_path(&item.file.path, root);
            outln!(
                "untested-file:{}:value_exports={}",
                relative,
                item.file.value_export_count,
            );
        }
        for item in &gaps.exports {
            let relative = health_compact_path(&item.export.path, root);
            outln!(
                "untested-export:{}:{}:{}",
                relative,
                item.export.line,
                item.export.export_name,
            );
        }
    }
}

fn print_runtime_sections_compact(report: &crate::health_types::HealthReport, root: &Path) {
    if let Some(ref production) = report.runtime_coverage {
        for line in build_runtime_coverage_compact_lines(production, root) {
            outln!("{line}");
        }
    }
    if let Some(ref intelligence) = report.coverage_intelligence {
        for line in build_coverage_intelligence_compact_lines(intelligence, root) {
            outln!("{line}");
        }
    }
}

fn compact_ownership_suffix(ownership: Option<&crate::health_types::OwnershipMetrics>) -> String {
    ownership.map_or_else(String::new, |o| {
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
        let state = match o.ownership_state {
            crate::health_types::OwnershipState::Active => "active",
            crate::health_types::OwnershipState::Unowned => "unowned",
            crate::health_types::OwnershipState::DeclaredInactive => "declared_inactive",
            crate::health_types::OwnershipState::Drifting => "drifting",
        };
        parts.push(format!("ownership_state={state}"));
        if o.drift {
            parts.push("drift=true".to_string());
        }
        format!(",{}", parts.join(","))
    })
}

fn print_hotspots_compact(hotspots: &[crate::health_types::HotspotFinding], root: &Path) {
    for entry in hotspots {
        let relative = health_compact_path(&entry.path, root);
        let ownership_suffix = compact_ownership_suffix(entry.ownership.as_ref());
        outln!(
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
}

fn print_health_trend_compact(report: &crate::health_types::HealthReport) {
    if let Some(ref trend) = report.health_trend {
        outln!(
            "trend:overall:direction={}",
            trend.overall_direction.label()
        );
        for m in &trend.metrics {
            outln!(
                "trend:{}:previous={:.1},current={:.1},delta={:+.1},direction={}",
                m.name,
                m.previous,
                m.current,
                m.delta,
                m.direction.label(),
            );
        }
    }
}

fn print_refactoring_targets_compact(
    targets: &[crate::health_types::RefactoringTargetFinding],
    root: &Path,
) {
    for target in targets {
        let relative = health_compact_path(&target.path, root);
        let category = target.category.compact_label();
        let effort = target.effort.label();
        let confidence = target.confidence.label();
        outln!(
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

fn build_coverage_intelligence_compact_lines(
    intelligence: &crate::health_types::CoverageIntelligenceReport,
    root: &Path,
) -> Vec<String> {
    let mut lines = vec![format!(
        "coverage-intelligence-summary:verdict={},findings={},risky_changes={},high_confidence_deletes={},review_required={},refactor_carefully={},skipped_ambiguous_matches={}",
        intelligence.verdict,
        intelligence.summary.findings,
        intelligence.summary.risky_changes,
        intelligence.summary.high_confidence_deletes,
        intelligence.summary.review_required,
        intelligence.summary.refactor_carefully,
        intelligence.summary.skipped_ambiguous_matches,
    )];
    for finding in &intelligence.findings {
        let relative = normalize_uri(&relative_path(&finding.path, root).display().to_string());
        let identity = finding.identity.as_deref().unwrap_or("-");
        let signals = finding
            .signals
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("+");
        lines.push(format!(
            "coverage-intelligence:{}:{}:{}:id={},verdict={},recommendation={},confidence={},signals={}",
            relative,
            finding.line,
            identity,
            finding.id,
            finding.verdict,
            finding.recommendation,
            finding.confidence,
            signals,
        ));
    }
    lines
}

pub(super) fn print_duplication_compact(report: &DuplicationReport, root: &Path) {
    let fingerprints = CloneFingerprintSet::from_groups(&report.clone_groups);
    for (index, group) in report.clone_groups.iter().enumerate() {
        let fingerprint = fingerprints.fingerprint_for_group(group);
        for instance in &group.instances {
            outln!(
                "code-duplication:{}:{}-{}:fingerprint={},group={},tokens={},lines={},instances={}",
                compact_path(&instance.file, root),
                instance.start_line,
                instance.end_line,
                fingerprint,
                index + 1,
                group.token_count,
                group.line_count,
                group.instances.len(),
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
        RuntimeCoverageReportVerdict, RuntimeCoverageSchemaVersion, RuntimeCoverageSummary,
        RuntimeCoverageVerdict,
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
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/dead.ts"),
            }));

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "unused-file:src/dead.ts");
    }

    #[test]
    fn compact_unused_export_format() {
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

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "unused-export:src/utils.ts:10:helperFn");
    }

    #[test]
    fn compact_health_includes_runtime_coverage_lines() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            runtime_coverage: Some(RuntimeCoverageReport {
                schema_version: RuntimeCoverageSchemaVersion::V1,
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
                    stable_id: None,
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
                    source_hash: None,
                    discriminators: None,
                }],
                hot_paths: vec![RuntimeCoverageHotPath {
                    id: "fallow:hot:cafebabe".to_owned(),
                    stable_id: None,
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
                actionable: true,
                actionability_reason: None,
                actionability_verdict: None,
                provenance: crate::health_types::RuntimeCoverageProvenance::default(),
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
    fn compact_health_includes_coverage_intelligence_lines() {
        use crate::health_types::{
            CoverageIntelligenceAction, CoverageIntelligenceConfidence,
            CoverageIntelligenceEvidence, CoverageIntelligenceFinding,
            CoverageIntelligenceMatchConfidence, CoverageIntelligenceRecommendation,
            CoverageIntelligenceReport, CoverageIntelligenceSchemaVersion,
            CoverageIntelligenceSignal, CoverageIntelligenceSummary, CoverageIntelligenceVerdict,
        };

        let root = PathBuf::from("/project");
        let report = CoverageIntelligenceReport {
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
                signals: vec![
                    CoverageIntelligenceSignal::StaticUnused,
                    CoverageIntelligenceSignal::RuntimeCold,
                ],
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
        };

        let lines = build_coverage_intelligence_compact_lines(&report, &root);
        assert_eq!(
            lines[0],
            "coverage-intelligence-summary:verdict=high-confidence-delete,findings=1,risky_changes=0,high_confidence_deletes=1,review_required=0,refactor_carefully=0,skipped_ambiguous_matches=0"
        );
        assert_eq!(
            lines[1],
            "coverage-intelligence:src/dead.ts:9:deadPath:id=fallow:coverage-intel:abc123,verdict=high-confidence-delete,recommendation=delete-after-confirming-owner,confidence=high,signals=static_unused+runtime_cold"
        );
    }

    #[test]
    fn compact_unused_type_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: root.join("src/types.ts"),
                export_name: "OldType".to_string(),
                is_type_only: true,
                line: 5,
                col: 0,
                span_start: 60,
                is_re_export: false,
            }));

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "unused-type:src/types.ts:5:OldType");
    }

    #[test]
    fn compact_unused_dep_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".to_string(),
                location: DependencyLocation::Dependencies,
                path: root.join("package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "unused-dep:lodash");
    }

    #[test]
    fn compact_unused_devdep_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_dev_dependencies
            .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
                package_name: "jest".to_string(),
                location: DependencyLocation::DevDependencies,
                path: root.join("package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "unused-devdep:jest");
    }

    #[test]
    fn compact_unused_enum_member_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: root.join("src/enums.ts"),
                parent_name: "Status".to_string(),
                member_name: "Deprecated".to_string(),
                kind: MemberKind::EnumMember,
                line: 8,
                col: 2,
            }));

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
        results
            .unused_class_members
            .push(UnusedClassMemberFinding::with_actions(UnusedMember {
                path: root.join("src/service.ts"),
                parent_name: "UserService".to_string(),
                member_name: "legacyMethod".to_string(),
                kind: MemberKind::ClassMethod,
                line: 42,
                col: 4,
            }));

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
        results
            .unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: root.join("src/app.ts"),
                specifier: "./missing-module".to_string(),
                line: 3,
                col: 0,
                specifier_col: 0,
            }));

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "unresolved-import:src/app.ts:3:./missing-module");
    }

    #[test]
    fn compact_unlisted_dep_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "chalk".to_string(),
                    imported_from: vec![],
                },
            ));

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "unlisted-dep:chalk");
    }

    #[test]
    fn compact_duplicate_export_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
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
            }));

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "duplicate-export:Config");
    }

    #[test]
    fn compact_all_issue_types_produce_lines() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let lines = build_compact_lines(&results, &root);

        assert_eq!(lines.len(), 26);

        assert!(lines[0].starts_with("unused-file:"));
        assert!(lines[1].starts_with("unused-export:"));
        assert!(lines[2].starts_with("unused-type:"));
        assert!(lines[3].starts_with("unused-dep:"));
        assert!(lines[4].starts_with("unused-devdep:"));
        assert!(lines[5].starts_with("unused-optionaldep:"));
        assert!(lines[6].starts_with("unused-enum-member:"));
        assert!(lines[7].starts_with("unused-class-member:"));
        assert!(lines[8].starts_with("unused-store-member:"));
        assert!(lines[9].starts_with("unresolved-import:"));
        assert!(lines[10].starts_with("unlisted-dep:"));
        assert!(lines[11].starts_with("duplicate-export:"));
        assert!(lines[12].starts_with("type-only-dep:"));
        assert!(lines[13].starts_with("test-only-dep:"));
        assert!(lines[14].starts_with("circular-dependency:"));
        assert!(lines[15].starts_with("boundary-violation:"));
        assert!(lines.iter().any(|l| l.starts_with("unprovided-inject:")));
        assert!(lines.iter().any(|l| l.starts_with("unrendered-component:")));
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("unused-component-prop:"))
        );
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("unused-component-emit:"))
        );
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("unused-component-input:"))
        );
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("unused-component-output:"))
        );
        assert!(lines.iter().any(|l| l.starts_with("unused-svelte-event:")));
        assert!(lines.iter().any(|l| l.starts_with("unused-server-action:")));
        assert!(lines.iter().any(|l| l.starts_with("unused-load-data-key:")));
    }

    #[test]
    fn compact_covers_api_and_boundary_variants() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .private_type_leaks
            .push(PrivateTypeLeakFinding::with_actions(PrivateTypeLeak {
                path: root.join("src/api.ts"),
                export_name: "createApi".to_owned(),
                type_name: "InternalShape".to_owned(),
                line: 12,
                col: 4,
                span_start: 100,
            }));
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![
                        root.join("packages/a/index.ts"),
                        root.join("packages/b/index.ts"),
                    ],
                    length: 2,
                    line: 3,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: true,
                },
            ));
        results
            .boundary_coverage_violations
            .push(BoundaryCoverageViolationFinding::with_actions(
                BoundaryCoverageViolation {
                    path: root.join("src/unmatched.ts"),
                    line: 1,
                    col: 0,
                },
            ));
        results
            .boundary_call_violations
            .push(BoundaryCallViolationFinding::with_actions(
                BoundaryCallViolation {
                    path: root.join("src/ui/button.ts"),
                    line: 20,
                    col: 6,
                    zone: "ui".to_owned(),
                    callee: "child_process.exec".to_owned(),
                    pattern: "child_process.*".to_owned(),
                },
            ));

        let lines = build_compact_lines(&results, &root);

        assert_eq!(
            lines[0],
            "private-type-leak:src/api.ts:12:createApi->InternalShape"
        );
        assert!(lines[1].contains(" (cross-package)"));
        assert_eq!(
            lines[2],
            "boundary-coverage:src/unmatched.ts:1:no matching boundary zone"
        );
        assert_eq!(
            lines[3],
            "boundary-call:src/ui/button.ts:20:child_process.exec forbidden in zone ui (pattern child_process.*)"
        );
    }

    #[test]
    fn compact_strips_root_prefix_from_paths() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/project/src/deep/nested/file.ts"),
            }));

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "unused-file:src/deep/nested/file.ts");
    }

    #[test]
    fn compact_re_export_tagged_correctly() {
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

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "unused-re-export:src/index.ts:1:reExported");
    }

    #[test]
    fn compact_type_re_export_tagged_correctly() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: root.join("src/index.ts"),
                export_name: "ReExportedType".to_string(),
                is_type_only: true,
                line: 3,
                col: 0,
                span_start: 0,
                is_re_export: true,
            }));

        let lines = build_compact_lines(&results, &root);
        assert_eq!(
            lines[0],
            "unused-re-export-type:src/index.ts:3:ReExportedType"
        );
    }

    #[test]
    fn compact_unused_optional_dep_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_optional_dependencies
            .push(UnusedOptionalDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "fsevents".to_string(),
                    location: DependencyLocation::OptionalDependencies,
                    path: root.join("package.json"),
                    line: 12,
                    used_in_workspaces: Vec::new(),
                },
            ));

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "unused-optionaldep:fsevents");
    }

    #[test]
    fn compact_circular_dependency_format() {
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

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("circular-dependency:src/a.ts:3:"));
        assert!(lines[0].contains("src/a.ts"));
        assert!(lines[0].contains("src/b.ts"));
        assert!(lines[0].contains("\u{2192}"));
    }

    #[test]
    fn compact_circular_dependency_closes_cycle() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![
                        root.join("src/a.ts"),
                        root.join("src/b.ts"),
                        root.join("src/c.ts"),
                    ],
                    length: 3,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));

        let lines = build_compact_lines(&results, &root);
        let chain_part = lines[0].split(':').next_back().unwrap();
        let parts: Vec<&str> = chain_part.split(" \u{2192} ").collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], parts[3]); // first == last (cycle closes)
    }

    #[test]
    fn compact_type_only_dep_format() {
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

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines[0], "type-only-dep:zod");
    }

    #[test]
    fn compact_multiple_unused_files() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/a.ts"),
            }));
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/b.ts"),
            }));

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "unused-file:src/a.ts");
        assert_eq!(lines[1], "unused-file:src/b.ts");
    }

    #[test]
    fn compact_ordering_optional_dep_between_devdep_and_enum() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_dev_dependencies
            .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
                package_name: "jest".to_string(),
                location: DependencyLocation::DevDependencies,
                path: root.join("package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        results
            .unused_optional_dependencies
            .push(UnusedOptionalDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "fsevents".to_string(),
                    location: DependencyLocation::OptionalDependencies,
                    path: root.join("package.json"),
                    line: 12,
                    used_in_workspaces: Vec::new(),
                },
            ));
        results
            .unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: root.join("src/enums.ts"),
                parent_name: "Status".to_string(),
                member_name: "Deprecated".to_string(),
                kind: MemberKind::EnumMember,
                line: 8,
                col: 2,
            }));

        let lines = build_compact_lines(&results, &root);
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("unused-devdep:"));
        assert!(lines[1].starts_with("unused-optionaldep:"));
        assert!(lines[2].starts_with("unused-enum-member:"));
    }

    #[test]
    fn compact_path_outside_root_preserved() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/other/place/file.ts"),
            }));

        let lines = build_compact_lines(&results, &root);
        assert!(lines[0].contains("/other/place/file.ts"));
    }
}
