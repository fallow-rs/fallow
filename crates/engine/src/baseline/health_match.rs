//! The match of a saved health baseline against the current findings.
//!
//! A saved slot covers one current finding of the same or a lower severity
//! in the same bucket and dimension. The filter, the staleness counts and
//! the prune read one result of this match.

use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::health_categories::{
    HEALTH_FINDING_DIMENSIONS, HealthFindingCategory, health_finding_categories,
    health_finding_counts, severity_counts_for_dimension, severity_index,
};
use super::identity_counts::{PlaceholderLoan, placeholder_loans};
use super::{
    HealthBaselineCount, HealthBaselineData, HealthBaselineMode, HealthBaselineOverlap,
    HealthFindingCountMap, health_bucket_key, health_finding_key, moved_identity_bucket_remaps,
};

const SEVERITIES: [fallow_output::FindingSeverity; 3] = [
    fallow_output::FindingSeverity::Moderate,
    fallow_output::FindingSeverity::High,
    fallow_output::FindingSeverity::Critical,
];

/// The result of the greedy match for one bucket and one dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SeveritySlotMatch {
    /// Saved slots per severity that a current finding uses.
    pub(super) used: [usize; 3],
    /// `true` for a severity that has more current findings than
    /// compatible slots.
    pub(super) overflow: [bool; 3],
}

/// Match current findings to saved slots, lowest severity first. A finding
/// takes the lowest free slot of its own or a higher severity.
pub(super) fn match_severity_slots(current: [usize; 3], saved: [usize; 3]) -> SeveritySlotMatch {
    let mut available = saved;
    let mut overflow = [false; 3];
    for severity_idx in 0..3 {
        let compatible = available[severity_idx..].iter().sum::<usize>();
        overflow[severity_idx] = compatible < current[severity_idx];
        let mut matched = current[severity_idx].min(compatible);
        for slot in available.iter_mut().skip(severity_idx) {
            let taken = matched.min(*slot);
            *slot -= taken;
            matched -= taken;
            if matched == 0 {
                break;
            }
        }
    }
    SeveritySlotMatch {
        used: [
            saved[0] - available[0],
            saved[1] - available[1],
            saved[2] - available[2],
        ],
        overflow,
    }
}

/// The match of all saved buckets against all current buckets.
pub(super) struct HealthMatch {
    /// Per current bucket: the categories with more findings than slots.
    overflow: FxHashMap<String, FxHashSet<&'static str>>,
    /// Per saved bucket, under the key that the file stores: the slots per
    /// category that a current finding uses. A lent slot counts in the
    /// placeholder bucket that owns it.
    used: HealthFindingCountMap,
    /// Used slots in the buckets that matched through a followed file move.
    moved_entries: usize,
}

impl HealthMatch {
    /// `true` when the baseline does not cover every finding of `category`
    /// in the current bucket `bucket`.
    fn overflows(&self, bucket: &str, category: &str) -> bool {
        self.overflow
            .get(bucket)
            .is_some_and(|categories| categories.contains(category))
    }

    /// The slots of `category` in the saved bucket `bucket` that a current
    /// finding uses.
    pub(super) fn used_slots(&self, bucket: &str, category: &str) -> usize {
        self.used
            .get(bucket)
            .and_then(|counts| counts.get(category))
            .map_or(0, |entry| entry.count)
    }

    /// The staleness counts of this match.
    fn overlap(&self) -> HealthBaselineOverlap {
        HealthBaselineOverlap {
            matched_entries: self.used.values().map(slot_total).sum(),
            moved_entries: self.moved_entries,
        }
    }
}

fn slot_total(counts: &BTreeMap<String, HealthBaselineCount>) -> usize {
    counts.values().map(|entry| entry.count).sum()
}

fn add_slots(counts: &mut HealthFindingCountMap, bucket: &str, category: &str, count: usize) {
    counts
        .entry(bucket.to_owned())
        .or_default()
        .entry(category.to_owned())
        .or_insert(HealthBaselineCount { count: 0 })
        .count += count;
}

/// Match the saved slots of one count map against the current findings.
///
/// In identity mode a saved bucket follows a moved file, and a placeholder
/// bucket lends the slots that it does not use to named functions of its file.
pub(super) fn match_health_baseline(
    saved: &HealthFindingCountMap,
    current: &HealthFindingCountMap,
    mode: HealthBaselineMode,
    root: &Path,
) -> HealthMatch {
    let identity = mode == HealthBaselineMode::Identity;
    let remaps = if identity {
        moved_identity_bucket_remaps(saved, current, root)
    } else {
        Vec::new()
    };
    let mut pool = slots_under_current_keys(saved, &remaps);
    let loans = if identity {
        placeholder_loans(&pool, current)
    } else {
        Vec::new()
    };
    for loan in &loans {
        add_slots(&mut pool, &loan.to, loan.category, loan.count);
    }
    let (overflow, mut used) = match_buckets(&pool, current);
    return_loans(&mut used, &loans);
    let mut moved_entries = 0;
    for (saved_key, current_key) in remaps {
        if let Some(counts) = used.remove(&current_key) {
            moved_entries += slot_total(&counts);
            used.insert(saved_key, counts);
        }
    }
    HealthMatch {
        overflow,
        used,
        moved_entries,
    }
}

/// The saved slots with each moved bucket under its current key.
fn slots_under_current_keys(
    saved: &HealthFindingCountMap,
    remaps: &[(String, String)],
) -> HealthFindingCountMap {
    let current_key: FxHashMap<&str, &str> = remaps
        .iter()
        .map(|(saved_key, current_key)| (saved_key.as_str(), current_key.as_str()))
        .collect();
    saved
        .iter()
        .map(|(key, counts)| {
            let key = current_key
                .get(key.as_str())
                .copied()
                .unwrap_or(key.as_str());
            (key.to_owned(), counts.clone())
        })
        .collect()
}

/// One greedy match per bucket and dimension: the overflow per current
/// bucket and the used slots per pool bucket.
fn match_buckets(
    pool: &HealthFindingCountMap,
    current: &HealthFindingCountMap,
) -> (
    FxHashMap<String, FxHashSet<&'static str>>,
    HealthFindingCountMap,
) {
    let mut overflow: FxHashMap<String, FxHashSet<&'static str>> = FxHashMap::default();
    let mut used = HealthFindingCountMap::new();
    let buckets: BTreeSet<&String> = pool.keys().chain(current.keys()).collect();
    for bucket in buckets {
        for dimension in HEALTH_FINDING_DIMENSIONS {
            let matched = match_severity_slots(
                severity_counts_for_dimension(current.get(bucket), dimension),
                severity_counts_for_dimension(pool.get(bucket), dimension),
            );
            for severity in SEVERITIES {
                let index = severity_index(severity);
                let category = HealthFindingCategory {
                    dimension,
                    severity,
                }
                .key();
                if matched.overflow[index] {
                    overflow.entry(bucket.clone()).or_default().insert(category);
                }
                if matched.used[index] > 0 {
                    add_slots(&mut used, bucket, category, matched.used[index]);
                }
            }
        }
    }
    (overflow, used)
}

/// Count each lent slot in the placeholder bucket that owns it. The match
/// uses every lent slot: a loan never exceeds the current count of its
/// category above the slots that the recipient owns.
fn return_loans(used: &mut HealthFindingCountMap, loans: &[PlaceholderLoan]) {
    for loan in loans {
        if let Some(entry) = used
            .get_mut(&loan.to)
            .and_then(|counts| counts.get_mut(loan.category))
        {
            entry.count = entry.count.saturating_sub(loan.count);
        }
        add_slots(used, &loan.from, loan.category, loan.count);
    }
    used.retain(|_, counts| {
        counts.retain(|_, entry| entry.count > 0);
        !counts.is_empty()
    });
}

/// The findings that a baseline does not cover, and the overlap counts of the
/// same match.
pub struct HealthBaselineComparison {
    /// The findings that the baseline does not cover.
    pub new_findings: Vec<fallow_output::ComplexityViolation>,
    /// The staleness counts of the match.
    pub overlap: HealthBaselineOverlap,
}

impl HealthBaselineData {
    /// Compare current findings with this baseline in one match.
    pub(crate) fn compare(
        &self,
        mut findings: Vec<fallow_output::ComplexityViolation>,
        root: &Path,
        mode: HealthBaselineMode,
    ) -> HealthBaselineComparison {
        let saved = self.counts_for(mode);
        if saved.is_empty() {
            // A file with only the `findings` key list has no slots.
            let keys: FxHashSet<&str> = self.findings.iter().map(String::as_str).collect();
            let before = findings.len();
            findings.retain(|finding| !keys.contains(health_finding_key(finding, root).as_str()));
            return HealthBaselineComparison {
                overlap: HealthBaselineOverlap {
                    matched_entries: before - findings.len(),
                    moved_entries: 0,
                },
                new_findings: findings,
            };
        }
        let counting = self.file_length_counting();
        let current = health_finding_counts(&findings, root, mode, counting);
        let matched = match_health_baseline(saved, &current, mode, root);
        findings.retain(|finding| {
            let bucket = health_bucket_key(finding, root, mode);
            health_finding_categories(finding, counting)
                .into_iter()
                .flatten()
                .any(|category| matched.overflows(&bucket, category.key()))
        });
        HealthBaselineComparison {
            overlap: matched.overlap(),
            new_findings: findings,
        }
    }
}

/// The filter half of [`HealthBaselineData::compare`], for tests.
#[cfg(test)]
pub fn filter_new_health_findings(
    findings: Vec<fallow_output::ComplexityViolation>,
    baseline: &HealthBaselineData,
    root: &Path,
    mode: HealthBaselineMode,
) -> Vec<fallow_output::ComplexityViolation> {
    baseline.compare(findings, root, mode).new_findings
}

#[cfg(test)]
impl HealthBaselineData {
    /// The overlap half of [`HealthBaselineData::compare`], for tests.
    pub(crate) fn overlap_entries(
        &self,
        findings: &[fallow_output::ComplexityViolation],
        root: &Path,
        mode: HealthBaselineMode,
    ) -> HealthBaselineOverlap {
        self.compare(findings.to_vec(), root, mode).overlap
    }
}

#[cfg(test)]
#[path = "health_match_tests.rs"]
mod tests;
