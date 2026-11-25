//! Report the exported values that only test-side files use (issue #5252).

use std::path::Path;

use fallow_config::Severity;
use fallow_types::output_dead_code::{TestImporters, TestOnlyExportFinding};
use fallow_types::results::TestOnlyExport;

use crate::suppress::IssueKind;

use super::unused_exports::{UnusedExportInput, find_test_only_export_candidates};
use super::{DeadCodeDetectorInput, byte_offset_to_line_col};

/// The directory name of colocated tests below the declaring directory.
const COLOCATED_TEST_DIR: &str = "__tests__";

/// Find the test-only exports without the suppressed ones, sorted by path,
/// line and column. Empty when the rule is off for every path.
pub(super) fn run_test_only_export_detector(
    input: DeadCodeDetectorInput<'_>,
) -> Vec<TestOnlyExportFinding> {
    let config = input.config;
    if config.rules.test_only_exports == Severity::Off
        && !config.overrides.iter().any(|override_config| {
            override_config
                .rules
                .test_only_exports
                .is_some_and(|severity| severity != Severity::Off)
        })
    {
        return Vec::new();
    }
    let graph = input.graph;
    let candidates = find_test_only_export_candidates(UnusedExportInput {
        graph,
        modules: input.modules,
        config,
        plugin_result: input.plugin_result,
        suppressions: input.suppressions,
        line_offsets_by_file: input.line_offsets_by_file,
    });
    let mut findings: Vec<TestOnlyExportFinding> = candidates
        .into_iter()
        .filter_map(|candidate| {
            let module = graph.module(candidate.file)?;
            let export = module.exports.get(candidate.export_index)?;
            let (line, col) = byte_offset_to_line_col(
                input.line_offsets_by_file,
                module.file_id,
                export.span.start,
            );
            if input
                .suppressions
                .is_suppressed(module.file_id, line, IssueKind::TestOnlyExport)
            {
                return None;
            }
            let importer_paths: Vec<&Path> = candidate
                .importers
                .iter()
                .filter_map(|file| graph.module(*file).map(|node| node.path.as_path()))
                .collect();
            let colocated_only = importer_paths
                .iter()
                .all(|importer| is_colocated(&module.path, importer));
            let test_importers = TestImporters::from_files(
                importer_paths
                    .iter()
                    .map(|path| path.to_path_buf())
                    .collect(),
            )?;
            Some(TestOnlyExportFinding::with_actions(TestOnlyExport {
                path: module.path.clone(),
                export_name: export.name.to_string(),
                line,
                col,
                test_importers,
                colocated_only,
            }))
        })
        .collect();
    findings.sort_by(|a, b| {
        (&a.export.path, a.export.line, a.export.col).cmp(&(
            &b.export.path,
            b.export.line,
            b.export.col,
        ))
    });
    findings
}

/// Whether `importer` sits in the directory of `declaring`, or in a
/// `__tests__` directory directly below it.
fn is_colocated(declaring: &Path, importer: &Path) -> bool {
    let Some(dir) = declaring.parent() else {
        return false;
    };
    let Some(importer_dir) = importer.parent() else {
        return false;
    };
    importer_dir == dir
        || (importer_dir.parent() == Some(dir)
            && importer_dir
                .file_name()
                .is_some_and(|name| name == COLOCATED_TEST_DIR))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::is_colocated;

    #[test]
    fn colocated_means_same_directory_or_tests_directory_below() {
        let declaring = Path::new("/p/src/lib.ts");
        assert!(is_colocated(declaring, Path::new("/p/src/lib.test.ts")));
        assert!(is_colocated(
            declaring,
            Path::new("/p/src/__tests__/lib.ts")
        ));
        assert!(!is_colocated(declaring, Path::new("/p/tests/lib.test.ts")));
        assert!(!is_colocated(
            declaring,
            Path::new("/p/src/testing/helpers.ts")
        ));
        assert!(!is_colocated(
            declaring,
            Path::new("/p/src/a/__tests__/lib.ts")
        ));
    }
}
