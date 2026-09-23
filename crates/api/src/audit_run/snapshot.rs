//! The attribution key snapshot of one audit side, and the rules that make a
//! base snapshot comparable with the head run.

use std::path::Path;

use fallow_engine::changed_files::RenamedFile;
use rustc_hash::{FxHashMap, FxHashSet};

use super::AuditAnalysesView;
use crate::AuditProgrammaticKeySnapshot;
use crate::audit_keys::{
    dead_code_keys, health_keys, relative_key_path, remap_keys_for_renames, styling_keys,
};
use crate::review_deltas::{boundary_edge_keys, cycle_keys};

/// Attribution keys of one audit side (the base commit, or the head run when
/// the base pass is skipped).
///
/// Every key set is root-relative, so a base snapshot taken in a temporary
/// worktree joins the head run by key.
#[derive(Debug, Clone, Default)]
pub struct AuditKeySnapshot {
    /// Semantic identity of a type-aware dead-code pass, `None` for a
    /// syntactic pass.
    pub type_aware_identity: Option<fallow_types::semantic::SemanticAnalysisIdentity>,
    /// Sorted reasons and omissions of the semantic queries that did not
    /// complete. Two sides with different gaps cannot compare semantic keys.
    pub type_aware_gap_signature: Vec<String>,
    /// Dead-code keys captured before type-aware refinement. `None` when the
    /// pass ran without type-aware analysis (then `dead_code` is already
    /// syntactic). The degraded attribution path compares these.
    pub syntactic_dead_code: Option<FxHashSet<String>>,
    /// Dead-code keys.
    pub dead_code: FxHashSet<String>,
    /// Complexity keys.
    pub health: FxHashSet<String>,
    /// Styling keys.
    pub styling: FxHashSet<String>,
    /// Clone-group keys.
    pub dupes: FxHashSet<String>,
    /// Cross-zone boundary edge keys (`<from_zone>->-<to_zone>`), one for each
    /// zone pair.
    pub boundary_edges: FxHashSet<String>,
    /// Canonical circular-dependency keys.
    pub cycles: FxHashSet<String>,
    /// Exports-aware public-export keys (`<rel_path>::<name>`). Empty unless
    /// the surface computed them from the retained module graph.
    pub public_api: FxHashSet<String>,
    /// Branching totals per root-relative path. Threshold-blind and
    /// suppression-blind, so a threshold override or an ignore comment cannot
    /// move the head-versus-base comparison.
    pub branching: FxHashMap<String, fallow_types::extract::FileBranching>,
}

impl AuditKeySnapshot {
    /// Take the attribution keys of one set of analyses.
    #[must_use]
    pub fn from_view(view: &AuditAnalysesView<'_>) -> Self {
        let mut snapshot = Self::default();
        if let Some(dead_code) = view.dead_code.as_ref() {
            snapshot.type_aware_identity =
                dead_code.type_aware.and_then(|meta| meta.identity.clone());
            snapshot.type_aware_gap_signature = dead_code
                .type_aware
                .map_or_else(Vec::new, type_aware_gap_signature);
            snapshot.syntactic_dead_code = dead_code.syntactic_keys.cloned();
            snapshot.dead_code = dead_code_keys(dead_code.results, dead_code.root);
            snapshot.boundary_edges = boundary_edge_keys(&dead_code.results.boundary_violations);
            snapshot.cycles = cycle_keys(&dead_code.results.circular_dependencies, dead_code.root);
            snapshot.public_api = dead_code.public_api.cloned().unwrap_or_default();
        }
        if let Some(health) = view.health.as_ref() {
            snapshot.health = health_keys(health.report, health.root);
            snapshot.styling = styling_keys(health.report, health.root);
            snapshot.branching = health.branching.map_or_else(FxHashMap::default, |by_file| {
                branching_keys(by_file, health.root)
            });
        }
        if let Some(duplication) = view.duplication.as_ref() {
            snapshot.dupes = duplication
                .clone_groups
                .iter()
                .map(|group| crate::audit_keys::dupe_group_key(group, duplication.root))
                .collect();
        }
        snapshot
    }

    /// Relocate the keys of renamed files onto their head paths.
    ///
    /// Applies to every path-keyed family (dead code, complexity, styling,
    /// duplication, cycles, public API, branching). Boundary-edge keys are
    /// zone-pair keys without a path, and the type-aware identity has no path,
    /// so they stay as they are. The syntactic dead-code keys of the degraded
    /// type-aware path also stay as they are.
    pub fn remap_for_renames(&mut self, renames: &[RenamedFile], root: &Path) {
        let rename_map: FxHashMap<String, String> = renames
            .iter()
            .filter_map(|rename| {
                let from = relative_key_path(&rename.from, root);
                let to = relative_key_path(&rename.to, root);
                (from != to).then_some((from, to))
            })
            .collect();
        if rename_map.is_empty() {
            return;
        }
        self.dead_code = remap_keys_for_renames(&self.dead_code, &rename_map);
        self.health = remap_keys_for_renames(&self.health, &rename_map);
        self.styling = remap_keys_for_renames(&self.styling, &rename_map);
        self.dupes = remap_keys_for_renames(&self.dupes, &rename_map);
        self.cycles = remap_keys_for_renames(&self.cycles, &rename_map);
        self.public_api = remap_keys_for_renames(&self.public_api, &rename_map);
        // The branching payload is keyed by a bare path, not by an opaque key
        // string, so it needs its own remap.
        self.branching = self
            .branching
            .drain()
            .map(|(path, totals)| match rename_map.get(&path) {
                Some(renamed) => (renamed.clone(), totals),
                None => (path, totals),
            })
            .collect();
    }

    /// The public key snapshot of the programmatic audit output. Its health
    /// set also holds the styling keys.
    #[must_use]
    pub fn to_programmatic(&self) -> AuditProgrammaticKeySnapshot {
        let mut health = self.health.clone();
        health.extend(self.styling.iter().cloned());
        AuditProgrammaticKeySnapshot {
            dead_code: self.dead_code.clone(),
            health,
            dupes: self.dupes.clone(),
        }
    }
}

/// Re-key absolute branching paths into the root-relative key space of the
/// audit, so base and head entries join and the rename remap can move them.
#[must_use]
pub fn branching_keys(
    by_file: &fallow_engine::health::BranchingByFile,
    root: &Path,
) -> FxHashMap<String, fallow_types::extract::FileBranching> {
    by_file
        .iter()
        .map(|(path, totals)| (relative_key_path(path, root), *totals))
        .collect()
}

/// Why type-aware base and head attribution cannot be compared directly, or
/// `None` when the comparison is sound (fully syntactic runs included).
///
/// With a reason, the audit does not fail. It compares the syntactic key sets
/// that each side captured before refinement, so `--gate new-only` keeps
/// working when base and head resolve incompatible semantic identities.
/// Compatibility comes from `SemanticAnalysisIdentity::incompatible_fields`,
/// not raw equality: a deferred project-config hash and an absent identity
/// both mean a side ran no semantic queries, which is compatible with any
/// concrete identity on the other side (#2102).
#[must_use]
pub fn type_aware_attribution_degrade_reason(
    base: Option<&AuditKeySnapshot>,
    head: Option<&fallow_types::envelope::TypeAwareMeta>,
) -> Option<&'static str> {
    let base = base?;
    let base_identity = base.type_aware_identity.as_ref();
    let head_identity = head.and_then(|meta| meta.identity.as_ref());
    if let (Some(base_identity), Some(head_identity)) = (base_identity, head_identity)
        && !base_identity.incompatible_fields(head_identity).is_empty()
    {
        return Some("their semantic analysis identities are incompatible");
    }
    if let Some(head) = head
        && base.type_aware_gap_signature != type_aware_gap_signature(head)
    {
        return Some("their incomplete semantic query reasons or omissions differ");
    }
    None
}

/// The warning that an audit records when it degrades type-aware attribution
/// for `reason`.
#[must_use]
pub fn type_aware_degrade_warning(reason: &str) -> String {
    format!(
        "audit compared base and head with syntactic attribution because {reason} \
(usually a tsconfig or compiler-options change between base and head); \
type-aware refinement still applies to head findings, and \
semantic-only findings stay out of the new-only gate for this run; set \
audit.typeAware: false or pass --no-type-aware to keep the gate syntactic"
    )
}

/// Sorted reasons and omissions of the semantic queries that did not complete.
#[must_use]
pub fn type_aware_gap_signature(meta: &fallow_types::envelope::TypeAwareMeta) -> Vec<String> {
    let mut signature = meta
        .queries
        .iter()
        .filter(|query| query.status != fallow_types::semantic::SemanticCompleteness::Complete)
        .map(|query| {
            let mut omissions = query
                .omissions
                .iter()
                .map(|omission| format!("{:?}:{}", omission.reason_code, omission.count))
                .collect::<Vec<_>>();
            omissions.sort();
            format!(
                "{:?}:{:?}:{}",
                query.capability,
                query.reason_code,
                omissions.join(",")
            )
        })
        .collect::<Vec<_>>();
    signature.sort();
    signature
}
