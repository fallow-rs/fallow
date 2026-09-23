//! Diff line filters shared by every surface that accepts a unified diff.
//!
//! The CLI (`--diff-file`, `--diff-stdin` and the CI diff) and the programmatic
//! API (`analysis.diffFile`) narrow a report to the lines a change added. Both
//! call the functions in this module, so a finding cannot pass the filter on
//! one surface and fail it on another.

use std::path::Path;

use fallow_output::DiffIndex;
use fallow_types::duplicates::{CloneInstance, DuplicationReport};
use fallow_types::results::{AnalysisResults, TraceHopRole};

/// Drop the dead-code findings whose source line is not on an added line of
/// the diff.
///
/// Range-shaped findings (clone instances, complexity hotspots) have their own
/// filters. This filter governs the per-file findings on [`AnalysisResults`].
///
/// **Project-level findings bypass the filter.** A change that deletes the
/// last consumer of a package causes the `unused-dependency` finding, even
/// when the diff does not touch `package.json`. The same holds for catalog
/// entries, dependency overrides, type-only dependencies and test-only
/// dependencies. The line filter reduces noise for source-anchored findings.
/// CI must still fail on the project-level findings that the change caused.
///
/// [`DiffIndex::key_for`] gives the diff key of a finding path, relative to the
/// base of the diff and not to `root`. When a path has no key (a different
/// drive, or a path outside the base), the finding stays: a path that the
/// filter cannot judge is better shown than hidden.
pub fn filter_dead_code_by_diff(results: &mut AnalysisResults, diff: &DiffIndex, root: &Path) {
    let touches_file = |path: &Path| -> bool {
        diff.key_for(path, root)
            .is_none_or(|rel| diff.touches_file(&rel))
    };
    let line_in_diff = |path: &Path, line: u32| -> bool {
        diff.key_for(path, root)
            .is_none_or(|rel| diff.line_is_added(&rel, u64::from(line)))
    };

    filter_source_findings(results, &touches_file, &line_in_diff);
    filter_security_findings(results, &touches_file, &line_in_diff);
    filter_dependency_findings(results, &line_in_diff);
    filter_graph_findings(results, &touches_file, &line_in_diff);
    filter_framework_findings(results, &line_in_diff);
}

/// Keep only the clone groups with at least one instance whose line range
/// overlaps an added line of the diff.
///
/// The filter keeps the whole group when one instance overlaps, so a reviewer
/// sees the full clone family in the context of the change. Clone families,
/// statistics and the order are rebuilt from the groups that stay, so the
/// duplication percentage describes the scoped slice.
pub fn filter_duplication_by_diff(report: &mut DuplicationReport, diff: &DiffIndex, root: &Path) {
    let instance_overlaps = |instance: &CloneInstance| -> bool {
        let Some(rel) = diff.key_for(&instance.file, root) else {
            return true;
        };
        let start = u64::try_from(instance.start_line).unwrap_or(u64::MAX);
        let end = u64::try_from(instance.end_line).unwrap_or(u64::MAX);
        diff.range_overlaps_added(&rel, start, end)
    };
    report
        .clone_groups
        .retain(|group| group.instances.iter().any(instance_overlaps));
    crate::duplicates::refresh_scoped_report(report, root);
}

fn filter_source_findings(
    results: &mut AnalysisResults,
    touches_file: &dyn Fn(&Path) -> bool,
    line_in_diff: &dyn Fn(&Path, u32) -> bool,
) {
    results
        .unused_files
        .retain(|finding| touches_file(&finding.file.path));
    results
        .unused_exports
        .retain(|finding| line_in_diff(&finding.export.path, finding.export.line));
    results
        .unused_types
        .retain(|finding| line_in_diff(&finding.export.path, finding.export.line));
    results
        .private_type_leaks
        .retain(|finding| line_in_diff(&finding.leak.path, finding.leak.line));
    results
        .deprecated_exports_in_use
        .retain(|finding| line_in_diff(&finding.export.path, finding.export.line));
    results
        .unused_enum_members
        .retain(|finding| line_in_diff(&finding.member.path, finding.member.line));
    results
        .unused_class_members
        .retain(|finding| line_in_diff(&finding.member.path, finding.member.line));
    results
        .unused_store_members
        .retain(|finding| line_in_diff(&finding.member.path, finding.member.line));
    results
        .unprovided_injects
        .retain(|finding| line_in_diff(&finding.inject.path, finding.inject.line));
    results
        .unrendered_components
        .retain(|finding| line_in_diff(&finding.component.path, finding.component.line));
    results
        .unused_component_props
        .retain(|finding| line_in_diff(&finding.prop.path, finding.prop.line));
    results
        .unused_component_emits
        .retain(|finding| line_in_diff(&finding.emit.path, finding.emit.line));
    results
        .unused_component_inputs
        .retain(|finding| line_in_diff(&finding.input.path, finding.input.line));
    results
        .unused_component_outputs
        .retain(|finding| line_in_diff(&finding.output.path, finding.output.line));
    results
        .unused_svelte_events
        .retain(|finding| line_in_diff(&finding.event.path, finding.event.line));
    results
        .unused_server_actions
        .retain(|finding| line_in_diff(&finding.action.path, finding.action.line));
    results
        .unused_load_data_keys
        .retain(|finding| line_in_diff(&finding.key.path, finding.key.line));
    results
        .unresolved_imports
        .retain(|finding| line_in_diff(&finding.import.path, finding.import.line));
}

fn filter_security_findings(
    results: &mut AnalysisResults,
    touches_file: &dyn Fn(&Path) -> bool,
    line_in_diff: &dyn Fn(&Path, u32) -> bool,
) {
    results.security_findings.retain(|finding| {
        line_in_diff(&finding.path, finding.line)
            || finding.trace.iter().any(|hop| {
                line_in_diff(&hop.path, hop.line)
                    || (matches!(hop.role, TraceHopRole::SecretSource) && touches_file(&hop.path))
            })
            || finding.reachability.as_ref().is_some_and(|reachability| {
                // Any hop on an added line keeps the finding for display, whatever
                // its role. Do not add a role check here: unlike the strict
                // `--gate new` filter of `fallow security`, the display must show a
                // finding whose module-level source sits on an added line.
                reachability
                    .untrusted_source_trace
                    .iter()
                    .any(|hop| line_in_diff(&hop.path, hop.line))
            })
    });
    results
        .security_unresolved_callee_diagnostics
        .retain(|finding| line_in_diff(&finding.path, finding.line));
}

fn filter_dependency_findings(
    results: &mut AnalysisResults,
    line_in_diff: &dyn Fn(&Path, u32) -> bool,
) {
    for finding in &mut results.unlisted_dependencies {
        finding
            .dep
            .imported_from
            .retain(|source| line_in_diff(&source.path, source.line));
    }
    results
        .unlisted_dependencies
        .retain(|finding| !finding.dep.imported_from.is_empty());
}

fn filter_graph_findings(
    results: &mut AnalysisResults,
    touches_file: &dyn Fn(&Path) -> bool,
    line_in_diff: &dyn Fn(&Path, u32) -> bool,
) {
    results.duplicate_exports.retain(|finding| {
        finding
            .export
            .locations
            .iter()
            .any(|location| line_in_diff(&location.path, location.line))
    });
    results
        .circular_dependencies
        .retain(|cycle| cycle.cycle.files.iter().any(|path| touches_file(path)));
    results
        .re_export_cycles
        .retain(|cycle| cycle.cycle.files.iter().any(|path| touches_file(path)));
    results
        .boundary_violations
        .retain(|finding| line_in_diff(&finding.violation.from_path, finding.violation.line));
    results
        .stale_suppressions
        .retain(|finding| line_in_diff(&finding.path, finding.line));
}

fn filter_framework_findings(
    results: &mut AnalysisResults,
    line_in_diff: &dyn Fn(&Path, u32) -> bool,
) {
    results
        .invalid_client_exports
        .retain(|finding| line_in_diff(&finding.export.path, finding.export.line));
    results
        .mixed_client_server_barrels
        .retain(|finding| line_in_diff(&finding.barrel.path, finding.barrel.line));
    results
        .misplaced_directives
        .retain(|finding| line_in_diff(&finding.directive_site.path, finding.directive_site.line));
    results
        .route_collisions
        .retain(|finding| line_in_diff(&finding.collision.path, finding.collision.line));
    results
        .dynamic_segment_name_conflicts
        .retain(|finding| line_in_diff(&finding.conflict.path, finding.conflict.line));
}
