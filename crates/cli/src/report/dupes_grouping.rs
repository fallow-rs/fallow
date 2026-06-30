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

use std::path::Path;

use fallow_api::DuplicationGrouping;
use fallow_types::duplicates::{CloneGroup, DuplicationReport};

use super::grouping::OwnershipResolver;
use super::relative_path;

/// Pick the largest owner for a clone group: most instances wins, ties broken
/// alphabetically (smallest key wins).
///
/// Iterates a `BTreeMap` so iteration order is alphabetical. The first key
/// to reach the running maximum wins, which means equal counts resolve to the
/// alphabetically-smallest key.
pub fn largest_owner(group: &CloneGroup, root: &Path, resolver: &OwnershipResolver) -> String {
    fallow_api::largest_clone_group_owner_with(group, |path| {
        resolver.resolve(relative_path(path, root))
    })
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
    fallow_api::build_duplication_grouping_with(report, resolver.mode_label(), |path| {
        resolver.resolve(relative_path(path, root))
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_types::duplicates::{CloneInstance, DuplicationStats};

    use super::*;
    use crate::codeowners::{CodeOwners, UNOWNED_LABEL};

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
