//! Output rows of `coverage analyze --cloud` that come from a Fallow Cloud
//! runtime-context answer without a local merge: blast radius, importance,
//! and the `cloud_context` copy of the fields that the CLI does not read.

use std::path::PathBuf;

use fallow_output::{RuntimeCoverageCloudContext, RuntimeCoverageRiskBand};

use super::cloud_client::{CloudRuntimeContext, CloudRuntimeRiskBand};

/// The cloud blast-radius rows, else the rows that the CLI built.
pub(super) fn cloud_blast_radius_entries(
    snapshot: &CloudRuntimeContext,
    synthesized: Vec<fallow_output::RuntimeCoverageBlastRadiusEntry>,
) -> Vec<fallow_output::RuntimeCoverageBlastRadiusEntry> {
    if snapshot.blast_radius.is_empty() {
        return synthesized;
    }
    snapshot
        .blast_radius
        .iter()
        .map(|entry| fallow_output::RuntimeCoverageBlastRadiusEntry {
            id: entry.id.clone(),
            stable_id: entry.stable_id.clone(),
            file: PathBuf::from(&entry.file),
            function: entry.function.clone(),
            line: entry.line,
            caller_count: entry.caller_count.unwrap_or(0),
            caller_count_weighted_by_traffic: entry.caller_count_weighted_by_traffic.unwrap_or(0),
            deploys_touched: entry.deploys_touched,
            risk_band: map_cloud_risk_band(entry.risk_band),
            cloud_context: cloud_context(&entry.extra),
        })
        .collect()
}

/// The cloud importance rows, else the `ranked` rows that the CLI built.
pub(super) fn cloud_importance_entries(
    snapshot: &CloudRuntimeContext,
    ranked: Vec<fallow_output::RuntimeCoverageImportanceEntry>,
) -> Vec<fallow_output::RuntimeCoverageImportanceEntry> {
    if snapshot.importance.is_empty() {
        return ranked;
    }
    snapshot
        .importance
        .iter()
        .map(|entry| fallow_output::RuntimeCoverageImportanceEntry {
            id: entry.id.clone(),
            stable_id: entry.stable_id.clone(),
            file: PathBuf::from(&entry.file),
            function: entry.function.clone(),
            line: entry.line,
            invocations: entry.invocations,
            cyclomatic: entry.cyclomatic.unwrap_or(0),
            owner_count: entry.owner_count.unwrap_or(0),
            importance_score: entry.importance_score,
            reason: entry.reason.clone(),
        })
        .collect()
}

/// The cloud fields that the CLI does not read, for the `cloud_context` of
/// an output row. `None` when the cloud sent no such field.
pub(super) fn cloud_context(
    extra: &RuntimeCoverageCloudContext,
) -> Option<RuntimeCoverageCloudContext> {
    (!extra.is_empty()).then(|| extra.clone())
}

const fn map_cloud_risk_band(risk_band: CloudRuntimeRiskBand) -> RuntimeCoverageRiskBand {
    match risk_band {
        CloudRuntimeRiskBand::Low | CloudRuntimeRiskBand::Unknown => RuntimeCoverageRiskBand::Low,
        CloudRuntimeRiskBand::Medium => RuntimeCoverageRiskBand::Medium,
        CloudRuntimeRiskBand::High => RuntimeCoverageRiskBand::High,
    }
}
