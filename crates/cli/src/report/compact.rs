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

pub(super) fn print_type_aware_compact(type_aware: Option<&fallow_types::envelope::TypeAwareMeta>) {
    let Some(meta) = type_aware else {
        return;
    };
    let completeness = meta
        .identity
        .as_ref()
        .and_then(|identity| serde_json::to_value(identity.completeness).ok())
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_string());
    let capabilities = meta
        .identity
        .as_ref()
        .map(|identity| {
            identity
                .capabilities
                .iter()
                .filter_map(|capability| serde_json::to_value(capability).ok())
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    outln!(
        "type-aware:backend={}@{} completeness={} capabilities={} confirmed-used={} abstained={} warnings={}",
        meta.backend,
        meta.backend_version,
        completeness,
        capabilities,
        meta.confirmed_used_count,
        meta.abstained_count,
        meta.warning_count
    );
}

fn print_lines(lines: Vec<String>) {
    for line in lines {
        outln!("{line}");
    }
}
