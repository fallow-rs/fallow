use crate::report::sink::outln;
use std::path::Path;

use fallow_api::ResultGroup;
use fallow_types::duplicates::DuplicationReport;
use fallow_types::results::AnalysisResults;

pub(super) fn print_compact(results: &AnalysisResults, root: &Path) {
    print_lines(fallow_api::build_compact_lines(results, root));
}

/// Print grouped compact output: each line is prefixed with the group key.
///
/// Format: `group-key\tissue-tag:details`
pub(super) fn print_grouped_compact(groups: &[ResultGroup], root: &Path) {
    print_lines(fallow_api::build_grouped_compact_lines(groups, root));
}

pub(super) fn print_health_compact(report: &fallow_output::HealthReport, root: &Path) {
    print_lines(fallow_api::build_health_compact_lines(report, root));
}

pub(super) fn print_duplication_compact(report: &DuplicationReport, root: &Path) {
    print_lines(fallow_api::build_duplication_compact_lines(report, root));
}

fn print_lines(lines: Vec<String>) {
    for line in lines {
        outln!("{line}");
    }
}
