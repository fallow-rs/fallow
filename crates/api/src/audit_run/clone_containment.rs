//! Head clone groups that lie inside a base clone group.
//!
//! An edit inside one copy of a base clone splits the base group into
//! smaller groups with new attribution keys. Each smaller group still holds
//! only duplication that the base already had, so it is inherited.

use std::path::Path;

use fallow_engine::changed_files::RenamedFile;
use fallow_engine::module_graph::relative_key_path;
use fallow_output::DiffIndex;
use fallow_types::duplicates::{CloneGroup, CloneInstance};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::audit_keys::dupe_group_key;
use crate::diff_risk::clones::{BaseCloneGroup, CloneSpan};

/// Keys of the head clone groups where each instance lies inside a different
/// instance of one base clone group, and at least one instance holds no added
/// line.
///
/// The check maps each head instance to its base path (through `renames`,
/// then through the renames of `diff`) and to its base lines (through the
/// line changes of `diff`). A group with an instance that has no base lines,
/// for example a pasted copy in a new file, is not in the set. A group where
/// each instance holds an added line is not in the set either: the change
/// wrote the same text into every copy, so it can have written new
/// duplication inside the old clone.
pub fn base_contained_dupe_group_keys<'a>(
    groups: impl IntoIterator<Item = &'a CloneGroup>,
    root: &Path,
    diff: &DiffIndex,
    base_groups: &[BaseCloneGroup],
    renames: &[RenamedFile],
) -> FxHashSet<String> {
    if base_groups.is_empty() {
        return FxHashSet::default();
    }
    let head_to_base: FxHashMap<String, String> = renames
        .iter()
        .map(|rename| {
            (
                relative_key_path(&rename.to, root),
                relative_key_path(&rename.from, root),
            )
        })
        .collect();
    let base_span = |instance: &CloneInstance| -> Option<(CloneSpan, bool)> {
        let rel = relative_key_path(&instance.file, root);
        let key = diff.key_for(&instance.file, root)?;
        let start = u64::try_from(instance.start_line).ok()?;
        let end = u64::try_from(instance.end_line).ok()?;
        let (base_start, base_end) = diff.base_line_range(&key, start, end)?;
        let touched = diff.range_overlaps_added(&key, start, end);
        let path = head_to_base.get(&rel).cloned().unwrap_or_else(|| {
            diff.old_path_for_root_relative(&rel)
                .map_or_else(|| rel.clone(), |old| old.into_owned())
        });
        let span = CloneSpan {
            path,
            start_line: u32::try_from(base_start).ok()?,
            end_line: u32::try_from(base_end).ok()?,
        };
        Some((span, touched))
    };
    groups
        .into_iter()
        .filter(|group| {
            let mapped: Option<Vec<(CloneSpan, bool)>> =
                group.instances.iter().map(base_span).collect();
            let Some(mapped) = mapped else {
                return false;
            };
            if mapped.iter().all(|(_, touched)| *touched) {
                return false;
            }
            let spans: Vec<CloneSpan> = mapped.into_iter().map(|(span, _)| span).collect();
            base_groups
                .iter()
                .any(|base| spans_fit_in_group(&spans, &base.instances))
        })
        .map(|group| dupe_group_key(group, root))
        .collect()
}

/// Whether each span lies inside a different instance of the base group.
/// Two head instances inside one base copy are a new clone of the copy with
/// itself, so each base instance takes at most one span.
fn spans_fit_in_group(spans: &[CloneSpan], base: &[CloneSpan]) -> bool {
    let mut used = vec![false; base.len()];
    spans.iter().all(|span| {
        let slot = base.iter().enumerate().position(|(index, outer)| {
            !used[index]
                && outer.path == span.path
                && outer.start_line <= span.start_line
                && span.end_line <= outer.end_line
        });
        slot.is_some_and(|index| {
            used[index] = true;
            true
        })
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn root() -> PathBuf {
        PathBuf::from("/project")
    }

    fn instance(path: &str, start_line: usize, end_line: usize) -> CloneInstance {
        CloneInstance {
            is_symlink: false,
            file: root().join(path),
            start_line,
            end_line,
            start_col: 0,
            end_col: 0,
            fragment: format!("{path}:{start_line}"),
        }
    }

    fn group(instances: Vec<CloneInstance>) -> CloneGroup {
        CloneGroup {
            instances,
            token_count: 40,
            line_count: 6,
            similarity: None,
        }
    }

    fn span(path: &str, start_line: u32, end_line: u32) -> CloneSpan {
        CloneSpan {
            path: path.to_owned(),
            start_line,
            end_line,
        }
    }

    fn base() -> Vec<BaseCloneGroup> {
        vec![BaseCloneGroup {
            instances: vec![span("src/a.js", 1, 16), span("src/b.js", 1, 16)],
        }]
    }

    const ONE_LINE_EDIT: &str = "diff --git a/src/a.js b/src/a.js\n--- a/src/a.js\n+++ b/src/a.js\n@@ -7 +7 @@\n-  old\n+  new\n";

    #[test]
    fn split_groups_inside_the_base_group_are_contained() {
        let diff = DiffIndex::from_unified_diff(ONE_LINE_EDIT);
        let before = group(vec![instance("src/a.js", 1, 7), instance("src/b.js", 1, 7)]);
        let after = group(vec![
            instance("src/a.js", 7, 16),
            instance("src/b.js", 7, 16),
        ]);
        let keys = base_contained_dupe_group_keys([&before, &after], &root(), &diff, &base(), &[]);
        assert!(keys.contains(&dupe_group_key(&before, &root())));
        assert!(keys.contains(&dupe_group_key(&after, &root())));
    }

    #[test]
    fn copy_in_a_new_file_is_not_contained() {
        let diff = DiffIndex::from_unified_diff(
            "diff --git a/src/c.js b/src/c.js\n--- /dev/null\n+++ b/src/c.js\n@@ -0,0 +1,16 @@\n+x\n",
        );
        let pasted = group(vec![
            instance("src/a.js", 1, 16),
            instance("src/c.js", 1, 16),
        ]);
        let keys = base_contained_dupe_group_keys([&pasted], &root(), &diff, &base(), &[]);
        assert!(keys.is_empty());
    }

    #[test]
    fn group_outside_every_base_instance_is_not_contained() {
        let diff = DiffIndex::from_unified_diff(ONE_LINE_EDIT);
        let wider = group(vec![
            instance("src/a.js", 1, 20),
            instance("src/b.js", 1, 20),
        ]);
        let other_pair = group(vec![instance("src/a.js", 1, 7), instance("src/d.js", 1, 7)]);
        let keys =
            base_contained_dupe_group_keys([&wider, &other_pair], &root(), &diff, &base(), &[]);
        assert!(keys.is_empty());
    }

    #[test]
    fn two_instances_inside_one_base_copy_are_not_contained() {
        let diff = DiffIndex::from_unified_diff(ONE_LINE_EDIT);
        let self_clone = group(vec![
            instance("src/a.js", 1, 5),
            instance("src/a.js", 9, 13),
        ]);
        let keys = base_contained_dupe_group_keys([&self_clone], &root(), &diff, &base(), &[]);
        assert!(keys.is_empty());
    }

    #[test]
    fn same_edit_in_every_copy_is_not_contained() {
        let diff = DiffIndex::from_unified_diff(
            "diff --git a/src/a.js b/src/a.js\n--- a/src/a.js\n+++ b/src/a.js\n@@ -7,0 +8,1 @@\n+  added\ndiff --git a/src/b.js b/src/b.js\n--- a/src/b.js\n+++ b/src/b.js\n@@ -7,0 +8,1 @@\n+  added\n",
        );
        let grown = group(vec![
            instance("src/a.js", 1, 17),
            instance("src/b.js", 1, 17),
        ]);
        let keys = base_contained_dupe_group_keys([&grown], &root(), &diff, &base(), &[]);
        assert!(keys.is_empty());
    }

    /// The change renames one copy and replaces the three lines above the
    /// other copy with two. The second copy moves up one line, and its clone
    /// starts on a replaced line, which holds only the clone's first token
    /// (issue #4580).
    #[test]
    fn renamed_copy_with_a_rewritten_first_line_in_the_other_copy_is_contained() {
        let diff = DiffIndex::from_unified_diff(
            "diff --git a/src/a.js b/src/r.js\nsimilarity index 100%\nrename from src/a.js\nrename to src/r.js\ndiff --git a/src/b.js b/src/b.js\n--- a/src/b.js\n+++ b/src/b.js\n@@ -1,3 +1,2 @@\n-import { a0 } from './a.js';\n-import { c0 } from './c.js';\n-export const b0 = a0 + c0;\n+import { a0 } from './r.js';\n+export const b0 = a0;\n",
        );
        let renames = [RenamedFile {
            from: root().join("src/a.js"),
            to: root().join("src/r.js"),
        }];
        let base = vec![BaseCloneGroup {
            instances: vec![span("src/a.js", 1, 14), span("src/b.js", 3, 16)],
        }];
        let moved = group(vec![
            instance("src/b.js", 2, 15),
            instance("src/r.js", 1, 14),
        ]);
        let keys = base_contained_dupe_group_keys([&moved], &root(), &diff, &base, &renames);
        assert_eq!(keys.len(), 1);
    }

    #[test]
    fn renamed_copy_maps_to_its_base_path() {
        let diff = DiffIndex::from_unified_diff(
            "diff --git a/src/a.js b/src/lib/a.js\nrename from src/a.js\nrename to src/lib/a.js\n--- a/src/a.js\n+++ b/src/lib/a.js\n@@ -7 +7 @@\n-  old\n+  new\n",
        );
        let renames = [RenamedFile {
            from: root().join("src/a.js"),
            to: root().join("src/lib/a.js"),
        }];
        let split = group(vec![
            instance("src/lib/a.js", 1, 7),
            instance("src/b.js", 1, 7),
        ]);
        let keys = base_contained_dupe_group_keys([&split], &root(), &diff, &base(), &renames);
        assert_eq!(keys.len(), 1);
        let keys = base_contained_dupe_group_keys([&split], &root(), &diff, &base(), &[]);
        assert_eq!(
            keys.len(),
            1,
            "the diff renames map the path without the list"
        );
    }
}
