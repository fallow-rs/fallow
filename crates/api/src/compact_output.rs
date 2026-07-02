use std::path::Path;

use fallow_engine::duplicates::CloneFingerprintSet;
use fallow_output::normalize_uri;
use fallow_types::duplicates::DuplicationReport;
use fallow_types::results::{AnalysisResults, UnusedExport, UnusedMember};

use crate::ResultGroup;

fn relative_path<'a>(path: &'a Path, root: &Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
}

fn compact_path(path: &Path, root: &Path) -> String {
    normalize_uri(&relative_path(path, root).display().to_string())
}

fn compact_circular_dependency_line(
    cycle: &fallow_types::output_dead_code::CircularDependencyFinding,
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
    cycle: &fallow_types::output_dead_code::ReExportCycleFinding,
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
        fallow_types::results::ReExportCycleKind::SelfLoop => " (self-loop)",
        fallow_types::results::ReExportCycleKind::MultiNode => "",
    };
    format!(
        "re-export-cycle:{}:{}{}",
        first_file,
        chain.join(" <-> "),
        kind_tag
    )
}

fn compact_boundary_violation_line(
    item: &fallow_types::output_dead_code::BoundaryViolationFinding,
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
    item: &fallow_types::output_dead_code::BoundaryCoverageViolationFinding,
    root: &Path,
) -> String {
    format!(
        "boundary-coverage:{}:{}:no matching boundary zone",
        compact_path(&item.violation.path, root),
        item.violation.line,
    )
}

fn compact_boundary_call_line(
    item: &fallow_types::output_dead_code::BoundaryCallViolationFinding,
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
    item: &fallow_types::results::StaleSuppression,
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
    item: &fallow_types::output_dead_code::UnresolvedCatalogReferenceFinding,
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
    item: &fallow_types::output_dead_code::UnusedDependencyOverrideFinding,
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
    item: &fallow_types::output_dead_code::MisconfiguredDependencyOverrideFinding,
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

/// Build grouped compact output lines, each prefixed with the group key.
///
/// Format: `group-key\tissue-tag:details`
#[must_use]
pub fn build_grouped_compact_lines(groups: &[ResultGroup], root: &Path) -> Vec<String> {
    groups
        .iter()
        .flat_map(|group| {
            build_compact_lines(&group.results, root)
                .into_iter()
                .map(|line| format!("{}\t{line}", group.key))
        })
        .collect()
}

/// Build compact output lines for health results.
#[must_use]
pub fn build_health_compact_lines(
    report: &fallow_output::HealthReport,
    root: &Path,
) -> Vec<String> {
    let mut lines = Vec::new();
    push_health_score_compact(&mut lines, report);
    push_vital_signs_compact(&mut lines, report);
    push_health_findings_compact(&mut lines, &report.findings, root);
    push_styling_findings_compact(&mut lines, &report.styling_findings, root);
    push_threshold_overrides_compact(&mut lines, &report.threshold_overrides, root);
    push_file_scores_compact(&mut lines, &report.file_scores, root);
    push_coverage_gaps_compact(&mut lines, report, root);
    push_runtime_sections_compact(&mut lines, report, root);
    push_hotspots_compact(&mut lines, &report.hotspots, root);
    push_health_trend_compact(&mut lines, report);
    push_refactoring_targets_compact(&mut lines, &report.targets, root);
    lines
}

fn push_styling_findings_compact(
    lines: &mut Vec<String>,
    findings: &[fallow_output::StylingFinding],
    root: &Path,
) {
    for finding in findings {
        let relative = health_compact_path(Path::new(&finding.path), root);
        let severity = match finding.effective_severity {
            fallow_output::StylingFindingSeverity::Error => "error",
            fallow_output::StylingFindingSeverity::Warn => "warn",
        };
        let value = compact_field_value(&finding.value);
        lines.push(format!(
            "{}:{}:{}:{}:severity={},value={}",
            finding.code, relative, finding.line, finding.sub_kind, severity, value
        ));
    }
}

fn compact_field_value(value: &str) -> String {
    value
        .replace([':', ',', '\n', '\r'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn push_threshold_overrides_compact(
    lines: &mut Vec<String>,
    entries: &[fallow_output::ThresholdOverrideState],
    root: &Path,
) {
    for entry in entries {
        let status = match entry.status {
            fallow_output::ThresholdOverrideStatus::Active => "active",
            fallow_output::ThresholdOverrideStatus::Stale => "stale",
            fallow_output::ThresholdOverrideStatus::NoMatch => "no_match",
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
        lines.push(format!(
            "threshold-override:{}:{}:{}{}",
            entry.override_index, status, target, metrics
        ));
    }
}

fn push_health_score_compact(lines: &mut Vec<String>, report: &fallow_output::HealthReport) {
    if let Some(ref hs) = report.health_score {
        lines.push(format!("health-score:{:.1}:{}", hs.score, hs.grade));
    }
}

fn push_vital_signs_compact(lines: &mut Vec<String>, report: &fallow_output::HealthReport) {
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
        lines.push(format!("vital-signs:{}", parts.join(",")));
    }
}

fn health_compact_path(path: &Path, root: &Path) -> String {
    normalize_uri(&relative_path(path, root).display().to_string())
}

fn push_health_findings_compact(
    lines: &mut Vec<String>,
    findings: &[fallow_output::HealthFinding],
    root: &Path,
) {
    for finding in findings {
        let relative = health_compact_path(&finding.path, root);
        let severity = match finding.severity {
            fallow_output::FindingSeverity::Critical => "critical",
            fallow_output::FindingSeverity::High => "high",
            fallow_output::FindingSeverity::Moderate => "moderate",
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
        lines.push(format!(
            "high-complexity:{}:{}:{}:cyclomatic={},cognitive={},severity={}{}",
            relative,
            finding.line,
            finding.name,
            finding.cyclomatic,
            finding.cognitive,
            severity,
            crap_suffix,
        ));
    }
}

fn push_file_scores_compact(
    lines: &mut Vec<String>,
    scores: &[fallow_output::FileHealthScore],
    root: &Path,
) {
    for score in scores {
        let relative = health_compact_path(&score.path, root);
        lines.push(format!(
            "file-score:{}:mi={:.1},fan_in={},fan_out={},dead={:.2},density={:.2},crap_max={:.1},crap_above={}",
            relative,
            score.maintainability_index,
            score.fan_in,
            score.fan_out,
            score.dead_code_ratio,
            score.complexity_density,
            score.crap_max,
            score.crap_above_threshold,
        ));
    }
}

fn push_coverage_gaps_compact(
    lines: &mut Vec<String>,
    report: &fallow_output::HealthReport,
    root: &Path,
) {
    if let Some(ref gaps) = report.coverage_gaps {
        lines.push(format!(
            "coverage-gap-summary:runtime_files={},covered_files={},file_coverage_pct={:.1},untested_files={},untested_exports={}",
            gaps.summary.runtime_files,
            gaps.summary.covered_files,
            gaps.summary.file_coverage_pct,
            gaps.summary.untested_files,
            gaps.summary.untested_exports,
        ));
        for item in &gaps.files {
            let relative = health_compact_path(&item.file.path, root);
            lines.push(format!(
                "untested-file:{}:value_exports={}",
                relative, item.file.value_export_count,
            ));
        }
        for item in &gaps.exports {
            let relative = health_compact_path(&item.export.path, root);
            lines.push(format!(
                "untested-export:{}:{}:{}",
                relative, item.export.line, item.export.export_name,
            ));
        }
    }
}

fn push_runtime_sections_compact(
    lines: &mut Vec<String>,
    report: &fallow_output::HealthReport,
    root: &Path,
) {
    if let Some(ref production) = report.runtime_coverage {
        lines.extend(build_runtime_coverage_compact_lines(production, root));
    }
    if let Some(ref intelligence) = report.coverage_intelligence {
        lines.extend(build_coverage_intelligence_compact_lines(
            intelligence,
            root,
        ));
    }
}

fn compact_ownership_suffix(ownership: Option<&fallow_output::OwnershipMetrics>) -> String {
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
            fallow_output::OwnershipState::Active => "active",
            fallow_output::OwnershipState::Unowned => "unowned",
            fallow_output::OwnershipState::DeclaredInactive => "declared_inactive",
            fallow_output::OwnershipState::Drifting => "drifting",
        };
        parts.push(format!("ownership_state={state}"));
        if o.drift {
            parts.push("drift=true".to_string());
        }
        format!(",{}", parts.join(","))
    })
}

fn push_hotspots_compact(
    lines: &mut Vec<String>,
    hotspots: &[fallow_output::HotspotFinding],
    root: &Path,
) {
    for entry in hotspots {
        let relative = health_compact_path(&entry.path, root);
        let ownership_suffix = compact_ownership_suffix(entry.ownership.as_ref());
        lines.push(format!(
            "hotspot:{}:score={:.1},commits={},churn={},density={:.2},fan_in={},trend={}{}",
            relative,
            entry.score,
            entry.commits,
            entry.lines_added + entry.lines_deleted,
            entry.complexity_density,
            entry.fan_in,
            entry.trend,
            ownership_suffix,
        ));
    }
}

fn push_health_trend_compact(lines: &mut Vec<String>, report: &fallow_output::HealthReport) {
    if let Some(ref trend) = report.health_trend {
        lines.push(format!(
            "trend:overall:direction={}",
            trend.overall_direction.label()
        ));
        for m in &trend.metrics {
            lines.push(format!(
                "trend:{}:previous={:.1},current={:.1},delta={:+.1},direction={}",
                m.name,
                m.previous,
                m.current,
                m.delta,
                m.direction.label(),
            ));
        }
    }
}

fn push_refactoring_targets_compact(
    lines: &mut Vec<String>,
    targets: &[fallow_output::RefactoringTargetFinding],
    root: &Path,
) {
    for target in targets {
        let relative = health_compact_path(&target.path, root);
        let category = target.category.compact_label();
        let effort = target.effort.label();
        let confidence = target.confidence.label();
        lines.push(format!(
            "refactoring-target:{}:priority={:.1},efficiency={:.1},category={},effort={},confidence={}:{}",
            relative,
            target.priority,
            target.efficiency,
            category,
            effort,
            confidence,
            target.recommendation,
        ));
    }
}

fn build_runtime_coverage_compact_lines(
    production: &fallow_output::RuntimeCoverageReport,
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
    intelligence: &fallow_output::CoverageIntelligenceReport,
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

/// Build compact output lines for duplication results.
#[must_use]
pub fn build_duplication_compact_lines(report: &DuplicationReport, root: &Path) -> Vec<String> {
    let fingerprints = CloneFingerprintSet::from_groups(&report.clone_groups);
    let mut lines = Vec::new();
    for (index, group) in report.clone_groups.iter().enumerate() {
        let fingerprint = fingerprints.fingerprint_for_group(group);
        for instance in &group.instances {
            lines.push(format!(
                "code-duplication:{}:{}-{}:fingerprint={},group={},tokens={},lines={},instances={}",
                compact_path(&instance.file, root),
                instance.start_line,
                instance.end_line,
                fingerprint,
                index + 1,
                group.token_count,
                group.line_count,
                group.instances.len(),
            ));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_types::duplicates::{CloneGroup, CloneInstance, DuplicationStats};
    use fallow_types::output_dead_code::UnusedFileFinding;
    use fallow_types::results::{AnalysisResults, UnusedFile};

    use super::*;

    #[test]
    fn compact_unused_file_format_uses_relative_paths() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/dead.ts"),
            }));

        let lines = build_compact_lines(&results, &root);

        assert_eq!(lines, vec!["unused-file:src/dead.ts"]);
    }

    #[test]
    fn grouped_compact_prefixes_each_issue_with_group_key() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/dead.ts"),
            }));
        let groups = vec![ResultGroup {
            key: "team-a".to_owned(),
            owners: Some(vec!["@team-a".to_owned()]),
            results,
        }];

        let lines = build_grouped_compact_lines(&groups, &root);

        assert_eq!(lines, vec!["team-a\tunused-file:src/dead.ts"]);
    }

    #[test]
    fn duplication_compact_lines_include_stable_group_context() {
        let root = PathBuf::from("/project");
        let report = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![CloneInstance {
                    file: root.join("src/a.ts"),
                    start_line: 2,
                    end_line: 6,
                    start_col: 0,
                    end_col: 10,
                    fragment: "const duplicated = true;".to_owned(),
                }],
                token_count: 12,
                line_count: 5,
            }],
            clone_families: Vec::new(),
            mirrored_directories: Vec::new(),
            stats: DuplicationStats::default(),
        };

        let lines = build_duplication_compact_lines(&report, &root);

        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("code-duplication:src/a.ts:2-6:fingerprint="));
        assert!(lines[0].contains(",group=1,tokens=12,lines=5,instances=1"));
    }

    #[test]
    fn health_compact_lines_include_score_and_vital_signs() {
        let root = PathBuf::from("/project");
        let report = fallow_output::HealthReport {
            health_score: Some(fallow_output::HealthScore {
                formula_version: 1,
                score: 91.2,
                grade: "A",
                penalties: fallow_output::HealthScorePenalties {
                    dead_files: None,
                    dead_exports: None,
                    complexity: 0.0,
                    p90_complexity: 0.0,
                    maintainability: None,
                    hotspots: None,
                    unused_deps: None,
                    circular_deps: None,
                    unit_size: None,
                    coupling: None,
                    duplication: None,
                    prop_drilling: None,
                },
            }),
            vital_signs: Some(fallow_output::VitalSigns {
                total_loc: 120,
                avg_cyclomatic: 3.4,
                p90_cyclomatic: 8,
                ..Default::default()
            }),
            ..Default::default()
        };

        let lines = build_health_compact_lines(&report, &root);

        assert_eq!(lines[0], "health-score:91.2:A");
        assert_eq!(
            lines[1],
            "vital-signs:total_loc=120,avg_cyclomatic=3.4,p90_cyclomatic=8"
        );
    }

    #[test]
    fn health_compact_lines_include_styling_findings() {
        let root = PathBuf::from("/project");
        let report = fallow_output::HealthReport {
            styling_findings: vec![fallow_output::StylingFinding {
                code: "css-token-drift".to_string(),
                sub_kind: "tailwind-arbitrary-value".to_string(),
                path: "/project/src/app.css".to_string(),
                line: 6,
                value: "--color-brand: rgb(240, 90, 41)".to_string(),
                effective_severity: fallow_output::StylingFindingSeverity::Warn,
                blast_radius: None,
                confidence: None,
                agent_disposition: None,
                nearest_token: None,
                fix_hint: None,
                actions: Vec::new(),
            }],
            ..Default::default()
        };

        let lines = build_health_compact_lines(&report, &root);

        assert_eq!(
            lines,
            vec![
                "css-token-drift:src/app.css:6:tailwind-arbitrary-value:severity=warn,value=--color-brand rgb(240 90 41)"
            ]
        );
    }
}
