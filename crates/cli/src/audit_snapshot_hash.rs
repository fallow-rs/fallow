use xxhash_rust::xxh3::xxh3_64;

use super::cache::sorted_keys;
use super::{AuditKeySnapshot, analyses_view, keys};
use crate::check::CheckResult;
use crate::dupes::DupesResult;
use crate::health::HealthResult;

/// Compute the deterministic graph-snapshot hash from the HEAD-side analysis
/// results plus the resolved base ref + head sha. Reuses [`AuditKeySnapshot::from_view`]
/// for the six key sets (dead_code / health / dupes / boundary_edges / cycles /
/// public_api), each sorted, then folds in the base ref and head sha so the same
/// tree compared against the same base always yields the same hash.
///
/// The verifier is the graph: any structural change (a new finding, a new edge,
/// a new export) shifts a key set and changes this hash, so a stale agent
/// walkthrough whose echoed hash no longer matches is refused on reentry.
pub(super) fn compute_graph_snapshot_hash(
    check: Option<&CheckResult>,
    dupes: Option<&DupesResult>,
    health: Option<&HealthResult>,
    base_ref: &str,
    head_sha: Option<&str>,
    change_anchors: &[crate::audit_walkthrough::ChangeAnchor],
) -> String {
    // The HEAD public-export set was computed on the brief path and retained on
    // the check result (`public_api_keys`); reuse it so the hash is exports-aware
    // without re-walking the graph.
    let snapshot = AuditKeySnapshot::from_view(&analyses_view(check, dupes, health));
    let mut bytes: Vec<u8> = Vec::new();
    // Sorted key sets, each length-prefixed, so the byte stream is unambiguous.
    let health = keys::complexity_baseline_keys(&snapshot.health);
    for set in [
        &snapshot.dead_code,
        &health,
        &snapshot.dupes,
        &snapshot.boundary_edges,
        &snapshot.cycles,
        &snapshot.public_api,
    ] {
        for key in sorted_keys(set) {
            bytes.extend_from_slice(key.as_bytes());
            bytes.push(0);
        }
        bytes.push(1);
    }
    // Seventh key set: the SORTED change-anchor id set, so a moved/added/removed
    // changed region shifts this hash and a cited change_anchor that moved is
    // refused as stale (the finding key sets are line-independent and would not
    // otherwise cover the region-level anchors).
    let mut anchor_ids: Vec<&str> = change_anchors
        .iter()
        .map(|a| a.change_anchor.as_str())
        .collect();
    anchor_ids.sort_unstable();
    for id in anchor_ids {
        bytes.extend_from_slice(id.as_bytes());
        bytes.push(0);
    }
    bytes.push(1);
    bytes.extend_from_slice(base_ref.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(head_sha.unwrap_or("").as_bytes());
    format!("graph:{:016x}", xxh3_64(&bytes))
}
