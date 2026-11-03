//! Protocol hot-path, blast-radius and importance rows mapped to the output
//! rows of the local runtime coverage report, in the output order.

use std::path::PathBuf;

use fallow_cov_protocol::{
    BlastRadiusEntry as ProtocolBlastRadiusEntry, HotPath as ProtocolHotPath,
    ImportanceEntry as ProtocolImportanceEntry, RiskBand,
};
use fallow_output::{RuntimeCoverageHotPath, RuntimeCoverageRiskBand};

pub(super) fn map_runtime_hot_paths(entries: Vec<ProtocolHotPath>) -> Vec<RuntimeCoverageHotPath> {
    let mut hot_paths = entries
        .into_iter()
        .map(|entry| RuntimeCoverageHotPath {
            cloud_context: None,
            id: entry.id,
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
            stable_id: entry.identity.map(|identity| identity.stable_id),
            file: PathBuf::from(entry.file),
            function: entry.function,
            line: entry.line,
            caller_count: entry.caller_count,
            caller_count_weighted_by_traffic: entry.caller_count_weighted_by_traffic,
            deploys_touched: entry.deploys_touched,
            risk_band: map_risk_band(entry.risk_band),
        })
        .collect::<Vec<_>>();
    blast_radius.sort_by(|left, right| {
        risk_band_rank(right.risk_band)
            .cmp(&risk_band_rank(left.risk_band))
            .then_with(|| {
                right
                    .caller_count_weighted_by_traffic
                    .cmp(&left.caller_count_weighted_by_traffic)
            })
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

const fn map_risk_band(risk_band: RiskBand) -> RuntimeCoverageRiskBand {
    match risk_band {
        RiskBand::Low => RuntimeCoverageRiskBand::Low,
        RiskBand::High => RuntimeCoverageRiskBand::High,
        RiskBand::Medium | RiskBand::Unknown => RuntimeCoverageRiskBand::Medium,
    }
}

const fn risk_band_rank(risk_band: RuntimeCoverageRiskBand) -> u8 {
    match risk_band {
        RuntimeCoverageRiskBand::Low => 0,
        RuntimeCoverageRiskBand::Medium => 1,
        RuntimeCoverageRiskBand::High => 2,
    }
}
