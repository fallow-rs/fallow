//! Per-group attribution for `fallow dupes --group-by`.
//!
//! For each `CloneGroup`, every instance is attributed to a group key (owner,
//! directory, package, or section) via the same [`OwnershipResolver`] used by
//! `check` and `health`. The group itself is then attributed to its
//! **largest owner**: the key with the most instances in that clone group.
//! Ties are broken alphabetically (lexicographic ascending).
//!
//! This mirrors jscpd's majority-owner attribution and avoids the
//! positional non-determinism that a "first-instance-wins" rule would
//! introduce, since `DuplicationReport::sort()` already orders instances
//! deterministically by file path then line.

use std::collections::BTreeMap;
use std::path::Path;

use fallow_core::duplicates::{
    CloneFingerprintSet, CloneGroup, CloneInstance, DuplicationReport, DuplicationStats,
};
use rustc_hash::FxHashSet;
use serde::Serialize;

use super::grouping::OwnershipResolver;
use super::relative_path;
use crate::baseline::recompute_stats;
use crate::codeowners::UNOWNED_LABEL;
use crate::output_dupes::{AttributedCloneGroupFinding, CloneFamilyFinding};

/// Resolve the group key for a single instance file.
fn key_for_instance(instance: &CloneInstance, root: &Path, resolver: &OwnershipResolver) -> String {
    resolver.resolve(relative_path(&instance.file, root))
}

/// Pick the largest owner for a clone group: most instances wins, ties broken
/// alphabetically (smallest key wins).
///
/// Iterates a `BTreeMap` so iteration order is alphabetical. The first key
/// to reach the running maximum wins, which means equal counts resolve to the
/// alphabetically-smallest key.
pub fn largest_owner(group: &CloneGroup, root: &Path, resolver: &OwnershipResolver) -> String {
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    for instance in &group.instances {
        let key = key_for_instance(instance, root, resolver);
        *counts.entry(key).or_insert(0) += 1;
    }
    if counts.is_empty() {
        return UNOWNED_LABEL.to_string();
    }
    let mut best_key: Option<String> = None;
    let mut best_count: u32 = 0;
    for (key, count) in counts {
        if best_key.is_none() || count > best_count {
            best_count = count;
            best_key = Some(key);
        }
    }
    best_key.unwrap_or_else(|| UNOWNED_LABEL.to_string())
}

/// A clone instance plus its per-instance owner key (for inline JSON / SARIF
/// rendering).
///
/// Each instance carries its own `owner` field alongside the standard
/// `CloneInstance` shape (file / start_line / end_line / start_col / end_col /
/// fragment), so consumers can attribute instances to resolver keys without
/// re-resolving paths.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AttributedInstance {
    /// The original clone instance.
    #[serde(flatten)]
    pub instance: CloneInstance,
    /// Resolver key for this specific instance (per-instance, not the
    /// group-level largest-owner).
    pub owner: String,
}

/// A clone group annotated with its largest-owner attribution and per-instance
/// owner keys.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AttributedCloneGroup {
    /// Largest-owner attribution: the resolver key with the most instances in
    /// this clone group. Ties broken alphabetically (smallest key wins).
    pub primary_owner: String,
    pub token_count: usize,
    pub line_count: usize,
    /// Each instance carries its own `owner` field alongside the standard
    /// CloneInstance shape.
    pub instances: Vec<AttributedInstance>,
}

impl AttributedCloneGroup {
    fn from_group(group: &CloneGroup, root: &Path, resolver: &OwnershipResolver) -> Self {
        let primary_owner = largest_owner(group, root, resolver);
        let instances = group
            .instances
            .iter()
            .map(|instance| AttributedInstance {
                owner: key_for_instance(instance, root, resolver),
                instance: instance.clone(),
            })
            .collect();
        Self {
            primary_owner,
            token_count: group.token_count,
            line_count: group.line_count,
            instances,
        }
    }

    fn fingerprint(&self, fingerprints: &CloneFingerprintSet) -> String {
        let instances: Vec<_> = self
            .instances
            .iter()
            .map(|instance| instance.instance.clone())
            .collect();
        fingerprints.fingerprint_for_parts(&instances, self.token_count, self.line_count)
    }
}

/// A single grouped duplication bucket. Per-group `stats` are dedup-aware and
/// computed over the FULL group BEFORE any `--top` truncation.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DuplicationGroup {
    /// Group label (owner / directory / package / section). `(unowned)` for
    /// files with no CODEOWNERS rule, `(no section)` for pre-section rules in
    /// section mode.
    pub key: String,
    pub stats: DuplicationStats,
    /// Clone groups attributed to this owner, each wrapped with the typed
    /// `actions[]` array. Each group's `primary_owner` is its largest-owner
    /// key; per-instance `owner` lets consumers see cross-bucket fan-out
    /// without re-resolving paths.
    pub clone_groups: Vec<AttributedCloneGroupFinding>,
    /// Clone families overlapping this bucket, each wrapped with the typed
    /// `actions[]` array.
    pub clone_families: Vec<CloneFamilyFinding>,
}

/// Wrapper carrying the resolver mode label and grouped buckets.
#[derive(Debug, Clone, Serialize)]
pub struct DuplicationGrouping {
    /// Resolver mode label (`"owner"`, `"directory"`, `"package"`, `"section"`).
    pub mode: &'static str,
    /// One bucket per resolver key, sorted most clone groups first with
    /// `(unowned)` pinned last.
    pub groups: Vec<DuplicationGroup>,
}

/// Build the grouped duplication payload from a project-level report.
///
/// Aggregation is performed BEFORE any `--top` truncation so per-group stats
/// reflect the full group, not just the rendered top-N.
pub fn build_duplication_grouping(
    report: &DuplicationReport,
    root: &Path,
    resolver: &OwnershipResolver,
) -> DuplicationGrouping {
    let fingerprints = CloneFingerprintSet::from_groups(&report.clone_groups);
    // Bucket clone groups by largest owner.
    let mut buckets: BTreeMap<String, Vec<AttributedCloneGroup>> = BTreeMap::new();
    for group in &report.clone_groups {
        let attributed = AttributedCloneGroup::from_group(group, root, resolver);
        buckets
            .entry(attributed.primary_owner.clone())
            .or_default()
            .push(attributed);
    }

    // For each bucket, recompute stats from its clone groups by reusing
    // `recompute_stats`. Use the original (non-attributed) clone groups to
    // feed the helper so we share the dedup logic with the project report.
    let mut groups: Vec<DuplicationGroup> = buckets
        .into_iter()
        .map(|(key, attributed_groups)| {
            // Reconstruct a partial DuplicationReport for stats recomputation.
            let original_groups: Vec<CloneGroup> = attributed_groups
                .iter()
                .map(|ag| CloneGroup {
                    instances: ag.instances.iter().map(|i| i.instance.clone()).collect(),
                    token_count: ag.token_count,
                    line_count: ag.line_count,
                })
                .collect();
            let mut subset = DuplicationReport {
                clone_groups: original_groups,
                clone_families: Vec::new(),
                mirrored_directories: Vec::new(),
                stats: DuplicationStats {
                    total_files: report.stats.total_files,
                    files_with_clones: 0,
                    total_lines: report.stats.total_lines,
                    duplicated_lines: 0,
                    total_tokens: report.stats.total_tokens,
                    duplicated_tokens: 0,
                    clone_groups: 0,
                    clone_instances: 0,
                    duplication_percentage: 0.0,
                    clone_groups_below_min_occurrences: report
                        .stats
                        .clone_groups_below_min_occurrences,
                },
            };
            subset.stats = recompute_stats(&subset);

            // Restrict clone families to those whose group memberships overlap
            // this bucket. Using a file-set membership check matches how the
            // project-level report treats families: a family's groups must all
            // share its file set.
            let bucket_files: FxHashSet<&Path> = attributed_groups
                .iter()
                .flat_map(|ag| ag.instances.iter().map(|i| i.instance.file.as_path()))
                .collect();
            let clone_families: Vec<CloneFamilyFinding> = report
                .clone_families
                .iter()
                .filter(|f| f.files.iter().any(|fp| bucket_files.contains(fp.as_path())))
                .map(|family| CloneFamilyFinding::with_fingerprints(family.clone(), &fingerprints))
                .collect();

            let clone_groups: Vec<AttributedCloneGroupFinding> = attributed_groups
                .into_iter()
                .map(|group| {
                    let fingerprint = group.fingerprint(&fingerprints);
                    AttributedCloneGroupFinding::with_fingerprint(group, fingerprint)
                })
                .collect();

            DuplicationGroup {
                key,
                stats: subset.stats,
                clone_groups,
                clone_families,
            }
        })
        .collect();

    // Sort: most clone groups first, alphabetical tiebreak, (unowned) last.
    groups.sort_by(|a, b| {
        let a_unowned = a.key == UNOWNED_LABEL;
        let b_unowned = b.key == UNOWNED_LABEL;
        match (a_unowned, b_unowned) {
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            _ => b
                .clone_groups
                .len()
                .cmp(&a.clone_groups.len())
                .then_with(|| a.key.cmp(&b.key)),
        }
    });

    DuplicationGrouping {
        mode: resolver.mode_label(),
        groups,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_core::duplicates::{CloneInstance, DuplicationStats};

    use super::*;
    use crate::codeowners::CodeOwners;

    fn instance(path: &str, start: usize, end: usize) -> CloneInstance {
        CloneInstance {
            file: PathBuf::from(path),
            start_line: start,
            end_line: end,
            start_col: 0,
            end_col: 0,
            fragment: String::new(),
        }
    }

    fn group(instances: Vec<CloneInstance>) -> CloneGroup {
        CloneGroup {
            instances,
            token_count: 50,
            line_count: 10,
        }
    }

    fn report(groups: Vec<CloneGroup>) -> DuplicationReport {
        DuplicationReport {
            clone_groups: groups,
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 10,
                total_lines: 1000,
                ..Default::default()
            },
        }
    }

    #[test]
    fn largest_owner_majority_wins() {
        let r = group(vec![
            instance("/root/src/a.ts", 1, 10),
            instance("/root/src/b.ts", 1, 10),
            instance("/root/lib/c.ts", 1, 10),
        ]);
        let key = largest_owner(&r, Path::new("/root"), &OwnershipResolver::Directory);
        assert_eq!(key, "src", "src has 2 instances vs lib's 1");
    }

    #[test]
    fn largest_owner_alphabetical_tiebreak() {
        let r = group(vec![
            instance("/root/src/a.ts", 1, 10),
            instance("/root/lib/b.ts", 1, 10),
        ]);
        // 1 vs 1 -- alphabetical: lib < src
        let key = largest_owner(&r, Path::new("/root"), &OwnershipResolver::Directory);
        assert_eq!(key, "lib");
    }

    #[test]
    fn largest_owner_three_way_tie_alphabetical() {
        let r = group(vec![
            instance("/root/zeta/a.ts", 1, 10),
            instance("/root/alpha/b.ts", 1, 10),
            instance("/root/beta/c.ts", 1, 10),
        ]);
        let key = largest_owner(&r, Path::new("/root"), &OwnershipResolver::Directory);
        assert_eq!(key, "alpha");
    }

    #[test]
    fn build_grouping_partitions_clone_groups() {
        let g1 = group(vec![
            instance("/root/src/a.ts", 1, 10),
            instance("/root/src/b.ts", 1, 10),
        ]);
        let g2 = group(vec![
            instance("/root/lib/x.ts", 1, 10),
            instance("/root/lib/y.ts", 1, 10),
        ]);
        let r = report(vec![g1, g2]);
        let grouping =
            build_duplication_grouping(&r, Path::new("/root"), &OwnershipResolver::Directory);
        assert_eq!(grouping.groups.len(), 2);
        let lib = grouping.groups.iter().find(|g| g.key == "lib").unwrap();
        let src = grouping.groups.iter().find(|g| g.key == "src").unwrap();
        assert_eq!(lib.clone_groups.len(), 1);
        assert_eq!(src.clone_groups.len(), 1);
    }

    #[test]
    fn build_grouping_unowned_pinned_last() {
        let co = CodeOwners::parse("/src/ @frontend\n").unwrap();
        let resolver = OwnershipResolver::Owner(co);
        // src group attributed to @frontend; docs group has no rule -> unowned
        let g_src = group(vec![
            instance("/root/src/a.ts", 1, 10),
            instance("/root/src/b.ts", 1, 10),
        ]);
        let g_docs = group(vec![
            instance("/root/docs/a.md", 1, 10),
            instance("/root/docs/b.md", 1, 10),
        ]);
        let r = report(vec![g_src, g_docs]);
        let grouping = build_duplication_grouping(&r, Path::new("/root"), &resolver);
        assert_eq!(grouping.groups.len(), 2);
        // unowned must be last
        assert_eq!(grouping.groups.last().unwrap().key, UNOWNED_LABEL);
    }

    #[test]
    fn build_grouping_per_instance_owner_inline() {
        let g = group(vec![
            instance("/root/src/a.ts", 1, 10),
            instance("/root/src/b.ts", 1, 10),
            instance("/root/lib/c.ts", 1, 10),
        ]);
        let r = report(vec![g]);
        let grouping =
            build_duplication_grouping(&r, Path::new("/root"), &OwnershipResolver::Directory);
        // Group has src=2, lib=1 -> primary src; instances carry their own owner.
        assert_eq!(grouping.groups.len(), 1);
        let bucket = &grouping.groups[0];
        assert_eq!(bucket.key, "src");
        assert_eq!(bucket.clone_groups.len(), 1);
        let finding = &bucket.clone_groups[0];
        let cg = &finding.group;
        assert_eq!(cg.primary_owner, "src");
        assert_eq!(cg.instances.len(), 3);
        let owners: Vec<&str> = cg.instances.iter().map(|i| i.owner.as_str()).collect();
        assert!(owners.contains(&"src"));
        assert!(owners.contains(&"lib"));
        // Each AttributedCloneGroupFinding carries the canonical 2-action array
        // (extract-shared + suppress-line).
        assert_eq!(finding.actions.len(), 2);
    }

    #[test]
    fn empty_report_produces_empty_grouping() {
        let r = DuplicationReport::default();
        let grouping =
            build_duplication_grouping(&r, Path::new("/root"), &OwnershipResolver::Directory);
        assert!(grouping.groups.is_empty());
    }
}
