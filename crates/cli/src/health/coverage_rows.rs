//! Protocol hot-path, blast-radius and importance rows mapped to the output
//! rows of the local runtime coverage report, in the output order.

use std::path::PathBuf;

use fallow_cov_protocol::{
    BlastRadiusEntry as ProtocolBlastRadiusEntry, HotPath as ProtocolHotPath,
    ImportanceEntry as ProtocolImportanceEntry,
};
use fallow_output::{RuntimeCoverageHotPath, RuntimeCoverageRiskBand};

use crate::coverage::risk_band::risk_band;

pub(super) fn map_runtime_hot_paths(entries: Vec<ProtocolHotPath>) -> Vec<RuntimeCoverageHotPath> {
    let mut hot_paths = entries
        .into_iter()
        .map(|entry| RuntimeCoverageHotPath {
            cloud_context: None,
            id: entry.id,
            legacy_id: None,
            stable_id: entry.identity.map(|identity| identity.stable_id),
            path: PathBuf::from(entry.file),
            function: entry.function,
            line: entry.line,
            end_line: entry.end_line,
            invocations: entry.invocations,
            percentile: entry.percentile,
            actions: Vec::new(),
            optimization_target: None,
        })
        .collect::<Vec<_>>();
    hot_paths.sort_by(|left, right| {
        right
            .invocations
            .cmp(&left.invocations)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.function.cmp(&right.function))
    });
    hot_paths
}

pub(super) fn map_runtime_blast_radius(
    entries: Vec<ProtocolBlastRadiusEntry>,
) -> Vec<fallow_output::RuntimeCoverageBlastRadiusEntry> {
    let mut blast_radius = entries
        .into_iter()
        .map(|entry| fallow_output::RuntimeCoverageBlastRadiusEntry {
            cloud_context: None,
            id: entry.id,
            legacy_id: None,
            stable_id: entry.identity.map(|identity| identity.stable_id),
            file: PathBuf::from(entry.file),
            function: entry.function,
            line: entry.line,
            caller_count: entry.caller_count,
            // The sidecar multiplies the own invocations by the callers, which
            // is not the importer traffic that this field holds, and it bands
            // on traffic too. The band follows the importer count only, as in
            // cloud mode (fallow-cloud#1642).
            caller_count_weighted_by_traffic: None,
            deploys_touched: entry.deploys_touched,
            risk_band: risk_band(entry.caller_count),
        })
        .collect::<Vec<_>>();
    blast_radius.sort_by(|left, right| {
        risk_band_rank(right.risk_band)
            .cmp(&risk_band_rank(left.risk_band))
            .then_with(|| right.caller_count.cmp(&left.caller_count))
            .then_with(|| left.file.cmp(&right.file))
            .then_with(|| left.function.cmp(&right.function))
    });
    blast_radius
}

pub(super) fn map_runtime_importance(
    entries: Vec<ProtocolImportanceEntry>,
) -> Vec<fallow_output::RuntimeCoverageImportanceEntry> {
    let mut importance = entries
        .into_iter()
        .map(|entry| fallow_output::RuntimeCoverageImportanceEntry {
            id: entry.id,
            legacy_id: None,
            stable_id: entry.identity.map(|identity| identity.stable_id),
            file: PathBuf::from(entry.file),
            function: entry.function,
            line: entry.line,
            invocations: entry.invocations,
            cyclomatic: entry.cyclomatic,
            owner_count: entry.owner_count,
            importance_score: entry.importance_score,
            reason: entry.reason,
        })
        .collect::<Vec<_>>();
    importance.sort_by(|left, right| {
        right
            .importance_score
            .total_cmp(&left.importance_score)
            .then_with(|| right.invocations.cmp(&left.invocations))
            .then_with(|| left.file.cmp(&right.file))
            .then_with(|| left.function.cmp(&right.function))
    });
    importance
}

const fn risk_band_rank(risk_band: RuntimeCoverageRiskBand) -> u8 {
    match risk_band {
        RuntimeCoverageRiskBand::Unknown => 0,
        RuntimeCoverageRiskBand::Low => 1,
        RuntimeCoverageRiskBand::Medium => 2,
        RuntimeCoverageRiskBand::High => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sidecar_row(function: &str, caller_count: u32, band: &str) -> ProtocolBlastRadiusEntry {
        serde_json::from_value(serde_json::json!({
            "id": format!("fallow:blast:{function}"),
            "file": "src/a.ts",
            "function": function,
            "line": 1,
            "caller_count": caller_count,
            "caller_count_weighted_by_traffic": 2_000_000,
            "risk_band": band,
        }))
        .expect("protocol row")
    }

    /// The sidecar bands on traffic too and multiplies the own invocations
    /// by the callers. The report bands on the importer count alone and has
    /// no importer traffic in a local run (fallow-cloud#1642).
    #[test]
    fn local_rows_band_on_callers_and_carry_no_traffic() {
        let rows = map_runtime_blast_radius(vec![
            sidecar_row("rare", 1, "high"),
            sidecar_row("wide", 7, "high"),
            sidecar_row("wider", 9, "medium"),
        ]);

        let bands: Vec<(&str, RuntimeCoverageRiskBand)> = rows
            .iter()
            .map(|row| (row.function.as_str(), row.risk_band))
            .collect();
        assert_eq!(
            bands,
            vec![
                ("wider", RuntimeCoverageRiskBand::Medium),
                ("wide", RuntimeCoverageRiskBand::Medium),
                ("rare", RuntimeCoverageRiskBand::Low),
            ]
        );
        let json = serde_json::to_value(&rows[0]).expect("serialize");
        assert!(json["caller_count_weighted_by_traffic"].is_null());
    }
}
