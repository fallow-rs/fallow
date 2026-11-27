//! Output rows of `coverage analyze --cloud` that come from a Fallow Cloud
//! runtime-context answer without a local merge: blast radius, importance,
//! and the `cloud_context` copy of the fields that the CLI does not read.

use std::path::PathBuf;

use fallow_output::{RuntimeCoverageCloudContext, RuntimeCoverageRiskBand};
use rustc_hash::FxHashMap;

use super::analyze::blast_radius_risk_band;
use super::cloud_client::{CloudRuntimeContext, CloudRuntimeRiskBand};

/// The local AST structure of one called cloud function. It fills the
/// structure fields that a cloud row leaves null, because the cloud has no
/// caller graph, complexity or owners for that repository.
#[derive(Debug, Clone, Copy)]
pub(super) struct LocalStructure {
    pub caller_count: u32,
    pub cyclomatic: u32,
    pub owner_count: Option<u32>,
    /// Production invocations of the function in the cloud window.
    pub invocations: u64,
}

/// The local structure of each cloud function, under its `stable_id` and
/// under its file, name and line. An older cloud sends `stable_id` on the
/// function rows but not on the blast-radius and importance rows, so a row
/// lookup tries the id first and the position second.
#[derive(Debug, Default)]
pub(super) struct LocalStructureIndex {
    by_stable_id: FxHashMap<String, LocalStructure>,
    by_position: FxHashMap<(String, String, u32), LocalStructure>,
}

impl LocalStructureIndex {
    pub(super) fn insert(
        &mut self,
        stable_id: Option<&str>,
        file: &str,
        function: &str,
        line: u32,
        structure: LocalStructure,
    ) {
        if let Some(id) = stable_id {
            self.by_stable_id.insert(id.to_owned(), structure);
        }
        self.by_position
            .insert((file.to_owned(), function.to_owned(), line), structure);
    }

    fn get(
        &self,
        stable_id: Option<&str>,
        file: &str,
        function: &str,
        line: u32,
    ) -> Option<&LocalStructure> {
        stable_id
            .and_then(|id| self.by_stable_id.get(id))
            .or_else(|| {
                self.by_position
                    .get(&(file.to_owned(), function.to_owned(), line))
            })
    }
}

/// The cloud blast-radius rows, else the rows that the CLI built.
pub(super) fn cloud_blast_radius_entries(
    snapshot: &CloudRuntimeContext,
    synthesized: Vec<fallow_output::RuntimeCoverageBlastRadiusEntry>,
    local: &LocalStructureIndex,
) -> Vec<fallow_output::RuntimeCoverageBlastRadiusEntry> {
    if snapshot.blast_radius.is_empty() {
        return synthesized;
    }
    snapshot
        .blast_radius
        .iter()
        .map(|entry| {
            let structure = local.get(
                entry.stable_id.as_deref(),
                &entry.file,
                &entry.function,
                entry.line,
            );
            // A cloud caller count means the cloud has a caller graph: keep
            // its counts, also a null weighted value (no importer file ran).
            // Without one, the local AST gives the importers and the weighted
            // traffic, and the band follows from them as in a local run.
            let (caller_count, weighted) = match (entry.caller_count, structure) {
                (None, Some(local)) => (
                    Some(local.caller_count),
                    Some(
                        local
                            .invocations
                            .saturating_mul(u64::from(local.caller_count)),
                    ),
                ),
                _ => (entry.caller_count, entry.caller_count_weighted_by_traffic),
            };
            let risk_band = match (entry.risk_band, caller_count, weighted) {
                (CloudRuntimeRiskBand::Unknown, Some(callers), Some(weighted)) => {
                    blast_radius_risk_band(callers, weighted)
                }
                (band, _, _) => map_cloud_risk_band(band),
            };
            fallow_output::RuntimeCoverageBlastRadiusEntry {
                id: entry.id.clone(),
                legacy_id: entry.legacy_id.clone(),
                stable_id: entry.stable_id.clone(),
                file: PathBuf::from(&entry.file),
                function: entry.function.clone(),
                line: entry.line,
                caller_count: caller_count.unwrap_or(0),
                caller_count_weighted_by_traffic: weighted.unwrap_or(0),
                deploys_touched: entry.deploys_touched,
                risk_band,
                cloud_context: cloud_context(&entry.extra),
            }
        })
        .collect()
}

/// The cloud importance rows, else the `ranked` rows that the CLI built.
pub(super) fn cloud_importance_entries(
    snapshot: &CloudRuntimeContext,
    ranked: Vec<fallow_output::RuntimeCoverageImportanceEntry>,
    local: &LocalStructureIndex,
) -> Vec<fallow_output::RuntimeCoverageImportanceEntry> {
    if snapshot.importance.is_empty() {
        return ranked;
    }
    snapshot
        .importance
        .iter()
        .map(|entry| {
            let structure = local.get(
                entry.stable_id.as_deref(),
                &entry.file,
                &entry.function,
                entry.line,
            );
            // F4: the local structure fills only the read-only fields. The
            // cloud `importance_score` and `reason` stay as the cloud sent them.
            fallow_output::RuntimeCoverageImportanceEntry {
                id: entry.id.clone(),
                legacy_id: entry.legacy_id.clone(),
                stable_id: entry.stable_id.clone(),
                file: PathBuf::from(&entry.file),
                function: entry.function.clone(),
                line: entry.line,
                invocations: entry.invocations,
                cyclomatic: entry
                    .cyclomatic
                    .or_else(|| structure.map(|local| local.cyclomatic))
                    .unwrap_or(0),
                owner_count: entry
                    .owner_count
                    .or_else(|| structure.and_then(|local| local.owner_count))
                    .unwrap_or(0),
                importance_score: entry.importance_score,
                reason: entry.reason.clone(),
            }
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
