//! Gates of the re-export cycle and package cycle detectors.

use super::*;

/// Thin wrapper around [`super::re_export_cycles::find_re_export_cycles`] that gates
/// on `Severity::Off`. Extracted alongside [`super::run_circular_dep_detector`].
pub(super) fn run_re_export_cycle_detector(
    input: DeadCodeDetectorInput<'_>,
) -> Vec<ReExportCycleFinding> {
    if input.config.rules.re_export_cycle == Severity::Off {
        return Vec::new();
    }
    find_re_export_cycles(input.graph, input.suppressions, input.line_offsets_by_file)
}

/// Thin wrapper around [`super::package_cycles::find_package_cycles`] that gates on
/// `Severity::Off`. Extracted alongside [`super::run_circular_dep_detector`].
pub(super) fn run_package_cycle_detector(
    input: DeadCodeDetectorInput<'_>,
) -> Vec<PackageCycleFinding> {
    if input.config.rules.package_cycle == Severity::Off {
        return Vec::new();
    }
    package_cycles::find_package_cycles(
        input.graph,
        input.resolved_modules,
        input.workspaces,
        &input.config.root,
        input.line_offsets_by_file,
        input.suppressions,
        &|path| input.config.resolve_rules_for_path(path).package_cycle == Severity::Off,
    )
}
