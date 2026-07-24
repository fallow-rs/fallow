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

pub(super) fn print_type_aware_markdown(
    type_aware: Option<&fallow_types::envelope::TypeAwareMeta>,
    scope: Option<&str>,
) {
    let Some(meta) = type_aware else {
        return;
    };
    let completeness = meta
        .identity
        .as_ref()
        .and_then(|identity| serde_json::to_value(identity.completeness).ok())
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_string());
    let capabilities = meta.identity.as_ref().map_or_else(
        || "none".to_string(),
        |identity| {
            identity
                .capabilities
                .iter()
                .filter_map(|capability| serde_json::to_value(capability).ok())
                .filter_map(|value| value.as_str().map(|label| format!("`{label}`")))
                .collect::<Vec<_>>()
                .join(", ")
        },
    );
    let heading = type_aware_heading(scope);
    outln!(
        "\n## {heading}\n\n- Executed: `{}`\n- Backend: `{}@{}`\n- Completeness: `{}`\n- Capabilities: {}\n- Confirmed used: {}\n- Contract preserved: {}\n- No static references: {}\n- Guarded fixes available: {}\n- Unresolved: {}\n- Abstained: {}\n- Warnings: {}",
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
    );
}

fn type_aware_heading(scope: Option<&str>) -> String {
    scope.map_or_else(
        || "Type-aware evidence".to_string(),
        |scope| format!("Type-aware {scope} evidence"),
    )
}

#[cfg(test)]
mod tests {
    use super::type_aware_heading;

    #[test]
    fn type_aware_heading_labels_combined_scope() {
        assert_eq!(
            type_aware_heading(Some("dead-code")),
            "Type-aware dead-code evidence"
        );
        assert_eq!(
            type_aware_heading(Some("health")),
            "Type-aware health evidence"
        );
    }
}
