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

pub(super) fn print_type_aware_compact(
    type_aware: Option<&fallow_types::envelope::TypeAwareMeta>,
    scope: Option<&str>,
) {
    let Some(meta) = type_aware else {
        return;
    };
    // Compact stdout is a stable stream of finding records. Semantic run
    // metadata is diagnostic context, so keep it on stderr instead of adding a
    // new record kind that existing compact parsers could mistake for a finding.
    eprintln!("{}", format_type_aware_metadata(meta, scope));
}

fn format_type_aware_metadata(
    meta: &fallow_types::envelope::TypeAwareMeta,
    scope: Option<&str>,
) -> String {
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
    let prefix = scope.map_or_else(
        || "type-aware".to_string(),
        |scope| format!("type-aware[{scope}]"),
    );
    format!(
        "{prefix}:executed={} backend={}@{} completeness={} capabilities={} confirmed-used={} contract-preserved={} no-static-references={} fix-eligible={} unresolved={} abstained={} warnings={}",
        meta.executed,
        meta.backend,
        meta.backend_version.as_deref().unwrap_or("not-run"),
        completeness,
        capabilities,
        meta.confirmed_used_count,
        meta.contract_preserved_count,
        meta.no_static_references_count,
        meta.fix_eligible_count,
        meta.unresolved_count,
        meta.abstained_count,
        meta.warning_count
    )
}

fn print_lines(lines: Vec<String>) {
    for line in lines {
        outln!("{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_aware_metadata_is_diagnostic_context_not_a_compact_record() {
        let meta = fallow_types::envelope::TypeAwareMeta {
            executed: true,
            backend: "typescript-go".to_string(),
            backend_version: Some("1.2.3".to_string()),
            ..Default::default()
        };

        let context = format_type_aware_metadata(&meta, None);

        assert!(context.starts_with("type-aware:executed=true backend=typescript-go@1.2.3"));
    }

    #[test]
    fn type_aware_metadata_labels_combined_scope() {
        let meta = fallow_types::envelope::TypeAwareMeta::default();

        let context = format_type_aware_metadata(&meta, Some("health"));

        assert!(context.starts_with("type-aware[health]:"));
    }
}
