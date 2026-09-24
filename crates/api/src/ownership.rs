//! Owner-group reach of a changeset for the review brief.
//!
//! Maps the changed files and the impact closure to CODEOWNERS owner groups:
//! how many groups the change reaches, which groups it reaches only through
//! the closure, and whether each independent slice of the partition has one
//! owner. Reads the CODEOWNERS file only. It does not read git history, so it
//! does not depend on the churn walk behind `routing`.
//!
//! Advisory brief data; never gates.

use std::path::Path;

pub use fallow_output::{OWNER_GROUP_CAP, OwnerGroupFact, OwnershipFacts, OwnershipSliceFact};
use rustc_hash::{FxHashMap, FxHashSet};

use fallow_config::ResolvedConfig;
use fallow_engine::codeowners::{CodeOwners, UNOWNED_LABEL};
use fallow_engine::module_graph::PartitionOrderPaths;

/// Load the project CODEOWNERS file: the configured `codeowners` path when
/// set, else the first of the standard locations.
///
/// Returns `Ok(None)` when no path is configured and no standard location
/// holds a file that parses, so the brief omits the section.
///
/// # Errors
///
/// Returns the reason when a configured `codeowners` path cannot be read or
/// does not parse. The caller reports it, because the user asked for that
/// file.
pub fn load_codeowners(root: &Path, config: &ResolvedConfig) -> Result<Option<CodeOwners>, String> {
    match config.codeowners.as_deref() {
        Some(path) => CodeOwners::load(root, Some(path))
            .map(Some)
            .map_err(|error| format!("codeowners path `{path}`: {error}")),
        None => Ok(CodeOwners::discover(root).ok()),
    }
}

/// Compute the ownership section.
///
/// `changed` and `affected` are root-relative, forward-slashed paths.
/// `affected` is the full, uncapped impact closure (files affected but not in
/// the diff), never the serialized sample. `partition` supplies the
/// independent slices and the changed files of each unit.
#[must_use]
pub fn compute_ownership_facts(
    codeowners: &CodeOwners,
    changed: &[String],
    affected: &[String],
    partition: Option<&PartitionOrderPaths>,
) -> OwnershipFacts {
    let owner_of = |path: &str| -> String {
        codeowners
            .owner_of(Path::new(path))
            .unwrap_or(UNOWNED_LABEL)
            .to_string()
    };

    let mut counts: FxHashMap<String, (usize, usize)> = FxHashMap::default();
    for path in changed {
        counts.entry(owner_of(path)).or_default().0 += 1;
    }
    for path in affected {
        counts.entry(owner_of(path)).or_default().1 += 1;
    }

    let unowned_direct_count = counts.get(UNOWNED_LABEL).map_or(0, |&(direct, _)| direct);
    let transitive_only_count = counts.values().filter(|&&(direct, _)| direct == 0).count();
    let group_count = counts.len();

    let mut groups: Vec<OwnerGroupFact> = counts
        .into_iter()
        .map(|(owner, (direct_count, affected_count))| OwnerGroupFact {
            owner,
            direct_count,
            affected_count,
        })
        .collect();
    groups.sort_by(|a, b| {
        b.direct_count
            .cmp(&a.direct_count)
            .then_with(|| b.affected_count.cmp(&a.affected_count))
            .then_with(|| a.owner.cmp(&b.owner))
    });
    let groups_omitted = groups.len().saturating_sub(OWNER_GROUP_CAP);
    groups.truncate(OWNER_GROUP_CAP);

    OwnershipFacts {
        group_count,
        transitive_only_count,
        unowned_direct_count,
        groups,
        groups_omitted,
        slices: partition.map_or_else(Vec::new, |partition| slice_owners(partition, &owner_of)),
    }
}

/// The owner set of each independent slice, aligned by index with
/// `independent_slices`. Empty when the partition has fewer than two slices,
/// the same rule that keeps `independent_slices` off the wire.
///
/// The owner set of a slice is never empty: the partition builds its slices
/// from its own units, so each slice directory has a unit with at least one
/// changed file, and each file has an owner or the unowned label.
///
/// The owners come from the changed files of each unit, not from the module
/// directory, because a CODEOWNERS rule can split a directory.
fn slice_owners(
    partition: &PartitionOrderPaths,
    owner_of: &dyn Fn(&str) -> String,
) -> Vec<OwnershipSliceFact> {
    if partition.independent_slices.len() < 2 {
        return Vec::new();
    }
    let files_by_dir: FxHashMap<&str, &[String]> = partition
        .units
        .iter()
        .map(|unit| (unit.module_dir.as_str(), unit.files.as_slice()))
        .collect();
    partition
        .independent_slices
        .iter()
        .map(|module_dirs| {
            let owners: FxHashSet<String> = module_dirs
                .iter()
                .filter_map(|dir| files_by_dir.get(dir.as_str()))
                .flat_map(|files| files.iter())
                .map(|file| owner_of(file))
                .collect();
            let mut owners: Vec<String> = owners.into_iter().collect();
            owners.sort_unstable();
            debug_assert!(
                !owners.is_empty(),
                "slice {module_dirs:?} has no unit with changed files"
            );
            OwnershipSliceFact {
                module_dirs: module_dirs.clone(),
                separable: owners.len() == 1,
                owners,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_engine::module_graph::ReviewUnitPaths;

    fn paths(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    fn owners(content: &str) -> CodeOwners {
        CodeOwners::parse(content).expect("valid CODEOWNERS")
    }

    fn group(owner: &str, direct_count: usize, affected_count: usize) -> OwnerGroupFact {
        OwnerGroupFact {
            owner: owner.to_string(),
            direct_count,
            affected_count,
        }
    }

    fn partition(units: &[(&str, &[&str])], slices: &[&[&str]]) -> PartitionOrderPaths {
        PartitionOrderPaths {
            units: units
                .iter()
                .map(|(dir, files)| ReviewUnitPaths {
                    module_dir: (*dir).to_string(),
                    files: paths(files),
                })
                .collect(),
            order: Vec::new(),
            independent_slices: slices.iter().map(|slice| paths(slice)).collect(),
        }
    }

    #[test]
    fn all_unowned_files_form_one_unowned_group() {
        let facts = compute_ownership_facts(
            &owners("docs/ @docs\n"),
            &paths(&["src/a.ts", "src/b.ts"]),
            &paths(&["src/c.ts"]),
            None,
        );
        assert_eq!(facts.group_count, 1);
        assert_eq!(facts.unowned_direct_count, 2);
        assert_eq!(facts.transitive_only_count, 0);
        assert_eq!(facts.groups, vec![group(UNOWNED_LABEL, 2, 1)]);
    }

    #[test]
    fn one_owner_owns_the_change_and_its_closure() {
        let facts = compute_ownership_facts(
            &owners("* @org/web\n"),
            &paths(&["src/a.ts"]),
            &paths(&["src/b.ts", "src/c.ts"]),
            None,
        );
        assert_eq!(facts.group_count, 1);
        assert_eq!(facts.unowned_direct_count, 0);
        assert_eq!(facts.groups, vec![group("@org/web", 1, 2)]);
    }

    #[test]
    fn a_group_reached_only_through_the_closure_is_transitive_only() {
        let facts = compute_ownership_facts(
            &owners("src/web/ @org/web\nsrc/tokens/ @org/design\nsrc/api/ @org/api\n"),
            &paths(&["src/web/a.ts", "src/web/b.ts", "src/tokens/t.ts"]),
            &paths(&["src/api/x.ts", "src/api/y.ts", "src/web/c.ts"]),
            None,
        );
        assert_eq!(facts.group_count, 3);
        assert_eq!(facts.transitive_only_count, 1);
        assert_eq!(
            facts.groups,
            vec![
                group("@org/web", 2, 1),
                group("@org/design", 1, 0),
                group("@org/api", 0, 2),
            ]
        );
    }

    #[test]
    fn a_gitlab_negation_makes_the_file_unowned() {
        let facts = compute_ownership_facts(
            &owners("src/ @org/web\n!src/generated/\n"),
            &paths(&["src/a.ts", "src/generated/types.ts"]),
            &[],
            None,
        );
        assert_eq!(facts.unowned_direct_count, 1);
        assert_eq!(
            facts.groups,
            vec![group(UNOWNED_LABEL, 1, 0), group("@org/web", 1, 0)]
        );
    }

    #[test]
    fn groups_beyond_the_cap_are_counted_not_dropped_silently() {
        let rules = (0..OWNER_GROUP_CAP + 3).fold(String::new(), |mut acc, i| {
            use std::fmt::Write as _;
            let _ = writeln!(acc, "pkg{i:02}/ @team{i:02}");
            acc
        });
        let changed: Vec<String> = (0..OWNER_GROUP_CAP + 3)
            .map(|i| format!("pkg{i:02}/index.ts"))
            .collect();
        let facts = compute_ownership_facts(&owners(&rules), &changed, &[], None);
        assert_eq!(facts.group_count, OWNER_GROUP_CAP + 3);
        assert_eq!(facts.groups.len(), OWNER_GROUP_CAP);
        assert_eq!(facts.groups_omitted, 3);
    }

    #[test]
    fn ties_sort_by_owner_so_the_order_is_deterministic() {
        let facts = compute_ownership_facts(
            &owners("c/ @c\na/ @a\nb/ @b\n"),
            &paths(&["c/x.ts", "b/x.ts", "a/x.ts"]),
            &[],
            None,
        );
        let order: Vec<&str> = facts.groups.iter().map(|g| g.owner.as_str()).collect();
        assert_eq!(order, vec!["@a", "@b", "@c"]);
    }

    #[test]
    fn slices_align_with_the_partition_and_flag_one_owner_as_separable() {
        let partition = partition(
            &[
                ("src/app", &["src/app/main.ts"]),
                ("src/core", &["src/core/lib.ts"]),
                ("src/tools", &["src/tools/cli.ts"]),
            ],
            &[&["src/app", "src/core"], &["src/tools"]],
        );
        let facts = compute_ownership_facts(
            &owners("src/app/ @team/app\nsrc/core/ @team/core\nsrc/tools/ @team/tools\n"),
            &paths(&["src/app/main.ts", "src/core/lib.ts", "src/tools/cli.ts"]),
            &[],
            Some(&partition),
        );
        assert_eq!(
            facts.slices,
            vec![
                OwnershipSliceFact {
                    module_dirs: paths(&["src/app", "src/core"]),
                    owners: paths(&["@team/app", "@team/core"]),
                    separable: false,
                },
                OwnershipSliceFact {
                    module_dirs: paths(&["src/tools"]),
                    owners: paths(&["@team/tools"]),
                    separable: true,
                },
            ]
        );
    }

    #[test]
    fn slice_owners_come_from_files_so_a_rule_can_split_a_directory() {
        let partition = partition(
            &[
                ("src/a", &["src/a/x.ts", "src/a/y.ts"]),
                ("src/b", &["src/b/z.ts"]),
            ],
            &[&["src/a"], &["src/b"]],
        );
        let facts = compute_ownership_facts(
            &owners("src/ @team/all\nsrc/a/y.ts @team/y\n"),
            &paths(&["src/a/x.ts", "src/a/y.ts", "src/b/z.ts"]),
            &[],
            Some(&partition),
        );
        assert_eq!(facts.slices[0].owners, paths(&["@team/all", "@team/y"]));
        assert!(!facts.slices[0].separable);
        assert!(facts.slices[1].separable);
    }

    #[test]
    fn an_unowned_slice_counts_the_unowned_label_as_its_owner() {
        let partition = partition(
            &[("src/a", &["src/a/x.ts"]), ("src/b", &["src/b/z.ts"])],
            &[&["src/a"], &["src/b"]],
        );
        let facts = compute_ownership_facts(
            &owners("src/a/ @team/a\n"),
            &paths(&["src/a/x.ts", "src/b/z.ts"]),
            &[],
            Some(&partition),
        );
        assert_eq!(facts.slices[1].owners, paths(&[UNOWNED_LABEL]));
        assert!(facts.slices[1].separable);
    }

    #[test]
    fn a_single_slice_emits_no_slice_owners() {
        let partition = partition(&[("src", &["src/a.ts"])], &[&["src"]]);
        let facts = compute_ownership_facts(
            &owners("* @org/web\n"),
            &paths(&["src/a.ts"]),
            &[],
            Some(&partition),
        );
        assert!(facts.slices.is_empty());
    }
}
