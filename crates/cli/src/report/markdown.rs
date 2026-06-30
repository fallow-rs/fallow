use crate::report::sink::outln;
use std::path::Path;

use fallow_api::ResultGroup;
use fallow_types::duplicates::DuplicationReport;
use fallow_types::results::AnalysisResults;

pub(super) fn print_markdown(results: &AnalysisResults, root: &Path) {
    outln!("{}", fallow_api::build_markdown(results, root));
}

pub(super) fn print_grouped_markdown(groups: &[ResultGroup], root: &Path) {
    outln!("{}", fallow_api::build_grouped_markdown(groups, root));
}

pub(super) fn print_duplication_markdown(report: &DuplicationReport, root: &Path) {
    outln!("{}", fallow_api::build_duplication_markdown(report, root));
}

pub(super) fn print_health_markdown(report: &fallow_output::HealthReport, root: &Path) {
    outln!("{}", fallow_api::build_health_markdown(report, root));
}
