//! The merge of the results of the parallel dead-code detectors.

use super::*;

pub(super) fn collect_parallel_dead_code_detector_results(
    input: DeadCodeDetectorInput<'_>,
) -> ParallelDeadCodeDetectorResults {
    let (
        ((unused_files, suppressed_unused_files), export_results),
        (
            (member_results, dependency_results),
            (
                (unresolved_imports, (duplicate_exports, duplicate_export_aliases)),
                (
                    (
                        boundary_violations,
                        (
                            boundary_coverage_violations,
                            (boundary_call_violations, policy_violations),
                        ),
                    ),
                    (circular_dependencies, ((re_export_cycles, package_cycles), export_usages)),
                ),
            ),
        ),
    ) = rayon::join(
        || run_file_and_export_detectors(input),
        || {
            rayon::join(
                || run_member_and_dependency_detectors(input),
                || {
                    rayon::join(
                        || {
                            import_and_duplicate_detectors::run_import_and_duplicate_detectors(
                                input,
                            )
                        },
                        || run_boundary_cycle_and_usage_detectors(input),
                    )
                },
            )
        },
    );

    ParallelDeadCodeDetectorResults {
        unused_files,
        suppressed_unused_files,
        export_results,
        member_results,
        dependency_results,
        unresolved_imports,
        duplicate_exports,
        duplicate_export_aliases,
        boundary_violations,
        boundary_coverage_violations,
        boundary_call_violations,
        policy_violations,
        circular_dependencies,
        re_export_cycles,
        package_cycles,
        export_usages,
    }
}
