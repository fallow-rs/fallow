//! Worktree-scoped gate and resolution history in the shared Impact store.

use super::{
    AuditVerdict, ContainmentEvent, Deserialize, FxHashMap, ID_SEP, ImpactCounts, ImpactStore,
    MAX_CONTAINMENT, PendingContainment, ResolutionEvent, Serialize,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct StoredResolution {
    #[serde(flatten)]
    pub(super) event: ResolutionEvent,
    /// Shared stores before version 8 did not record the worktree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) worktree: Option<String>,
}

impl ImpactStore {
    pub(super) fn containment_count(&self) -> usize {
        self.archived_containment_count
            .saturating_add(self.containment.len())
    }

    pub(super) fn record_gate(
        &mut self,
        worktree: &str,
        verdict: AuditVerdict,
        git_sha: Option<&str>,
        timestamp: &str,
        counts: &ImpactCounts,
    ) {
        if verdict == AuditVerdict::Fail {
            self.pending_by_worktree
                .entry(worktree.to_owned())
                .or_insert_with(|| PendingContainment {
                    blocked_at: timestamp.to_owned(),
                    git_sha: git_sha.map(ToOwned::to_owned),
                    blocked_counts: counts.clone(),
                });
            self.bound_pending_gates(worktree);
            return;
        }
        let Some(pending) = self.pending_by_worktree.remove(worktree) else {
            return;
        };
        self.containment.push(ContainmentEvent {
            blocked_at: pending.blocked_at,
            cleared_at: timestamp.to_owned(),
            git_sha: pending.git_sha,
            blocked_counts: pending.blocked_counts,
        });
        let overflow = self.containment.len().saturating_sub(MAX_CONTAINMENT);
        self.containment.drain(..overflow);
        self.archived_containment_count = self.archived_containment_count.saturating_add(overflow);
    }
    fn bound_pending_gates(&mut self, active: &str) {
        let overflow = self
            .pending_by_worktree
            .len()
            .saturating_sub(MAX_CONTAINMENT);
        if overflow == 0 {
            return;
        }
        // A removed worktree never sends a passing gate. Keep the active
        // failure and the newest other failures, breaking timestamp ties by key.
        let mut by_age: Vec<_> = self
            .pending_by_worktree
            .iter()
            .filter(|(key, _)| key.as_str() != active)
            .map(|(key, pending)| (&pending.blocked_at, key))
            .collect();
        by_age.sort_unstable();
        let expired: Vec<_> = by_age
            .into_iter()
            .take(overflow)
            .map(|(_, key)| key.clone())
            .collect();
        for key in expired {
            self.pending_by_worktree.remove(&key);
        }
    }
}

pub(super) fn consume_appearance(counts: &mut FxHashMap<String, usize>, key: &str) -> bool {
    let Some(count) = counts.get_mut(key) else {
        return false;
    };
    *count -= 1;
    if *count == 0 {
        counts.remove(key);
    }
    true
}

pub(super) fn uncredit_cross_run_moves(
    store: &mut ImpactStore,
    worktree: &str,
    appearances: &mut FxHashMap<String, usize>,
    prior_history_len: usize,
) {
    // Same-run moves get first claim. Match the remaining appearances to the
    // most recent earlier resolutions in this tree.
    for index in (0..prior_history_len).rev() {
        if appearances.is_empty() {
            break;
        }
        let stored = &store.recent_resolved[index];
        if stored.worktree.as_deref() == Some(worktree)
            && let Some(symbol) = &stored.event.symbol
            && consume_appearance(
                appearances,
                &format!("{}{ID_SEP}{symbol}", stored.event.kind),
            )
        {
            store.recent_resolved.remove(index);
            store.resolved_total = store.resolved_total.saturating_sub(1);
        }
    }
}
