//! Clone family grouping and refactoring suggestion generation.
//!
//! Groups clone groups by the set of files they span. When multiple clone groups
//! are duplicated across the same set of files, they form a "clone family",
//! indicating a deeper structural relationship that should be refactored together.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use super::types::{
    CloneFamily, CloneGroup, MirroredDirectory, RefactoringKind, RefactoringSuggestion,
};

/// The line threshold above which we suggest extracting a module rather than a function.
const MODULE_EXTRACTION_THRESHOLD: usize = 50;

/// Group clone groups into families by file set, sorted by total duplicated lines.
#[must_use]
pub fn group_into_families(clone_groups: &[CloneGroup], root: &Path) -> Vec<CloneFamily> {
    if clone_groups.is_empty() {
        return Vec::new();
    }

    let mut family_map: Vec<(BTreeSet<PathBuf>, Vec<CloneGroup>)> = Vec::new();

    for group in clone_groups {
        let file_set: BTreeSet<PathBuf> = group.instances.iter().map(|i| i.file.clone()).collect();

        if let Some(entry) = family_map.iter_mut().find(|(fs, _)| *fs == file_set) {
            entry.1.push(group.clone());
        } else {
            family_map.push((file_set, vec![group.clone()]));
        }
    }

    let mut families: Vec<CloneFamily> = family_map
        .into_iter()
        .map(|(file_set, groups)| {
            let total_duplicated_lines: usize = groups.iter().map(|g| g.line_count).sum();
            let total_duplicated_tokens: usize = groups.iter().map(|g| g.token_count).sum();
            let suggestions =
                generate_suggestions(&file_set, &groups, total_duplicated_lines, root);

            CloneFamily {
                files: file_set.into_iter().collect(),
                groups,
                total_duplicated_lines,
                total_duplicated_tokens,
                suggestions,
            }
        })
        .collect();

    families.sort_by(|a, b| {
        b.total_duplicated_lines
            .cmp(&a.total_duplicated_lines)
            .then(b.groups.len().cmp(&a.groups.len()))
    });

    families
}

/// Generate refactoring suggestions for a clone family.
fn generate_suggestions(
    file_set: &BTreeSet<PathBuf>,
    groups: &[CloneGroup],
    total_duplicated_lines: usize,
    root: &Path,
) -> Vec<RefactoringSuggestion> {
    let directories: BTreeSet<_> = file_set
        .iter()
        .filter_map(|p| p.parent().map(Path::to_path_buf))
        .collect();

    if total_duplicated_lines >= MODULE_EXTRACTION_THRESHOLD {
        vec![extract_module_suggestion(
            file_set,
            groups,
            total_duplicated_lines,
            &directories,
            root,
        )]
    } else {
        groups.iter().map(extract_function_suggestion).collect()
    }
}

/// Build the family-level extract-module suggestion, computing the location
/// hint from the family's directory set.
fn extract_module_suggestion(
    file_set: &BTreeSet<PathBuf>,
    groups: &[CloneGroup],
    total_duplicated_lines: usize,
    directories: &BTreeSet<PathBuf>,
    root: &Path,
) -> RefactoringSuggestion {
    let file_names: Vec<_> = file_set
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .collect();

    let location_hint = if directories.len() > 1 {
        "a shared directory".to_string()
    } else {
        directories.iter().next().map_or_else(
            || "the same directory".to_string(),
            |d| {
                let rel = d.strip_prefix(root).unwrap_or(d);
                if rel.as_os_str().is_empty() {
                    "the project root".to_string()
                } else {
                    format!("{}", rel.display())
                }
            },
        )
    };

    let estimated_savings: usize = groups
        .iter()
        .map(|g| g.line_count * (g.instances.len().saturating_sub(1)))
        .sum();

    RefactoringSuggestion {
        kind: RefactoringKind::ExtractModule,
        description: format!(
            "Extract {} shared clone group{} ({} lines) from {} into {}",
            groups.len(),
            if groups.len() == 1 { "" } else { "s" },
            total_duplicated_lines,
            file_names.join(", "),
            location_hint,
        ),
        estimated_savings,
    }
}

/// Build a per-group extract-function suggestion.
fn extract_function_suggestion(group: &CloneGroup) -> RefactoringSuggestion {
    let estimated_savings = group.line_count * (group.instances.len().saturating_sub(1));
    let file_names: Vec<_> = group
        .instances
        .iter()
        .filter_map(|i| i.file.file_name().map(|n| n.to_string_lossy().to_string()))
        .collect();

    RefactoringSuggestion {
        kind: RefactoringKind::ExtractFunction,
        description: format!(
            "Extract shared function ({} lines) from {}",
            group.line_count,
            file_names.join(", "),
        ),
        estimated_savings,
    }
}

/// Split a path string into (directory, filename).
///
/// Returns `("", path)` if there is no directory separator.
fn split_dir_file(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(pos) => (&path[..=pos], &path[pos + 1..]),
        None => ("", path),
    }
}

/// Detect mirrored directory patterns in clone families.
///
/// Scans families with exactly 2 files. If multiple families share the same
/// directory prefix pair (after stripping to the common filename), they are
/// grouped into a [`MirroredDirectory`]. Minimum 3 families must share a
/// pattern to qualify as "mirrored".
#[must_use]
pub fn detect_mirrored_directories(
    families: &[CloneFamily],
    root: &Path,
) -> Vec<MirroredDirectory> {
    const MIN_MIRROR_FAMILIES: usize = 3;

    type MirrorEntry = (String, usize);
    let mut pair_map: FxHashMap<(String, String), Vec<MirrorEntry>> = FxHashMap::default();

    for family in families {
        if family.files.len() != 2 {
            continue;
        }
        let rel_a = family.files[0]
            .strip_prefix(root)
            .unwrap_or(&family.files[0]);
        let rel_b = family.files[1]
            .strip_prefix(root)
            .unwrap_or(&family.files[1]);
        let path_a = rel_a.to_string_lossy().replace('\\', "/");
        let path_b = rel_b.to_string_lossy().replace('\\', "/");

        let (dir_a, file_a) = split_dir_file(&path_a);
        let (dir_b, file_b) = split_dir_file(&path_b);

        if file_a != file_b {
            continue;
        }

        let (da, db) = if dir_a <= dir_b {
            (dir_a.to_string(), dir_b.to_string())
        } else {
            (dir_b.to_string(), dir_a.to_string())
        };

        pair_map
            .entry((da, db))
            .or_default()
            .push((file_a.to_string(), family.total_duplicated_lines));
    }

    let mut mirrors: Vec<MirroredDirectory> = Vec::new();

    for ((dir_a, dir_b), entries) in &pair_map {
        if entries.len() < MIN_MIRROR_FAMILIES {
            continue;
        }
        let total_lines: usize = entries.iter().map(|(_, lines)| lines).sum();
        let mut files: Vec<String> = entries.iter().map(|(f, _)| f.clone()).collect();
        files.sort();
        mirrors.push(MirroredDirectory {
            dir_a: dir_a.clone(),
            dir_b: dir_b.clone(),
            shared_files: files,
            total_lines,
        });
    }

    mirrors.sort_by_key(|b| std::cmp::Reverse(b.total_lines));

    mirrors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::duplicates::types::CloneInstance;

    fn instance(file: &str, start: usize, end: usize) -> CloneInstance {
        CloneInstance {
            file: PathBuf::from(file),
            start_line: start,
            end_line: end,
            start_col: 0,
            end_col: 0,
            fragment: String::new(),
        }
    }

    fn root() -> PathBuf {
        PathBuf::from("")
    }

    #[test]
    fn empty_groups_produce_no_families() {
        assert!(group_into_families(&[], &root()).is_empty());
    }

    #[test]
    fn single_group_produces_one_family() {
        let groups = vec![CloneGroup {
            instances: vec![instance("src/a.ts", 1, 10), instance("src/b.ts", 1, 10)],
            token_count: 30,
            line_count: 10,
        }];

        let families = group_into_families(&groups, &root());
        assert_eq!(families.len(), 1);
        assert_eq!(families[0].groups.len(), 1);
        assert_eq!(families[0].files.len(), 2);
        assert_eq!(families[0].total_duplicated_lines, 10);
    }

    #[test]
    fn groups_sharing_same_files_form_one_family() {
        let groups = vec![
            CloneGroup {
                instances: vec![instance("src/a.ts", 1, 10), instance("src/b.ts", 1, 10)],
                token_count: 30,
                line_count: 10,
            },
            CloneGroup {
                instances: vec![instance("src/a.ts", 20, 30), instance("src/b.ts", 20, 30)],
                token_count: 30,
                line_count: 11,
            },
        ];

        let families = group_into_families(&groups, &root());
        assert_eq!(families.len(), 1);
        assert_eq!(families[0].groups.len(), 2);
        assert_eq!(families[0].total_duplicated_lines, 21);
    }

    #[test]
    fn groups_with_different_files_form_separate_families() {
        let groups = vec![
            CloneGroup {
                instances: vec![instance("src/a.ts", 1, 10), instance("src/b.ts", 1, 10)],
                token_count: 30,
                line_count: 10,
            },
            CloneGroup {
                instances: vec![instance("src/c.ts", 1, 10), instance("src/d.ts", 1, 10)],
                token_count: 30,
                line_count: 10,
            },
        ];

        let families = group_into_families(&groups, &root());
        assert_eq!(families.len(), 2);
    }

    #[test]
    fn families_sorted_by_total_lines_descending() {
        let groups = vec![
            CloneGroup {
                instances: vec![instance("src/a.ts", 1, 5), instance("src/b.ts", 1, 5)],
                token_count: 15,
                line_count: 5,
            },
            CloneGroup {
                instances: vec![instance("src/c.ts", 1, 20), instance("src/d.ts", 1, 20)],
                token_count: 60,
                line_count: 20,
            },
        ];

        let families = group_into_families(&groups, &root());
        assert_eq!(families.len(), 2);
        assert_eq!(families[0].total_duplicated_lines, 20);
        assert_eq!(families[1].total_duplicated_lines, 5);
    }

    #[test]
    fn large_family_gets_extract_module_suggestion() {
        let groups = vec![
            CloneGroup {
                instances: vec![instance("src/a.ts", 1, 30), instance("lib/b.ts", 1, 30)],
                token_count: 100,
                line_count: 30,
            },
            CloneGroup {
                instances: vec![instance("src/a.ts", 40, 65), instance("lib/b.ts", 40, 65)],
                token_count: 80,
                line_count: 26,
            },
        ];

        let families = group_into_families(&groups, &root());
        assert_eq!(families.len(), 1);
        assert!(!families[0].suggestions.is_empty());
        assert_eq!(
            families[0].suggestions[0].kind,
            RefactoringKind::ExtractModule
        );
    }

    #[test]
    fn small_family_gets_extract_function_suggestion() {
        let groups = vec![CloneGroup {
            instances: vec![instance("src/a.ts", 1, 10), instance("src/b.ts", 1, 10)],
            token_count: 30,
            line_count: 10,
        }];

        let families = group_into_families(&groups, &root());
        assert_eq!(families.len(), 1);
        assert!(!families[0].suggestions.is_empty());
        assert_eq!(
            families[0].suggestions[0].kind,
            RefactoringKind::ExtractFunction
        );
    }

    #[test]
    fn estimated_savings_for_extract_function() {
        let groups = vec![CloneGroup {
            instances: vec![
                instance("src/a.ts", 1, 10),
                instance("src/b.ts", 1, 10),
                instance("src/c.ts", 1, 10),
            ],
            token_count: 30,
            line_count: 10,
        }];

        let families = group_into_families(&groups, &root());
        assert_eq!(families.len(), 1);
        assert_eq!(families[0].suggestions[0].estimated_savings, 20);
    }

    #[test]
    fn estimated_savings_for_extract_module() {
        let groups = vec![
            CloneGroup {
                instances: vec![instance("src/a.ts", 1, 30), instance("lib/b.ts", 1, 30)],
                token_count: 100,
                line_count: 30,
            },
            CloneGroup {
                instances: vec![instance("src/a.ts", 40, 65), instance("lib/b.ts", 40, 65)],
                token_count: 80,
                line_count: 26,
            },
        ];

        let families = group_into_families(&groups, &root());
        assert_eq!(families.len(), 1);
        assert_eq!(families[0].suggestions[0].estimated_savings, 56);
    }

    #[test]
    fn same_directory_files_get_specific_location_hint() {
        let groups = vec![
            CloneGroup {
                instances: vec![
                    instance("src/utils/a.ts", 1, 30),
                    instance("src/utils/b.ts", 1, 30),
                ],
                token_count: 100,
                line_count: 30,
            },
            CloneGroup {
                instances: vec![
                    instance("src/utils/a.ts", 40, 65),
                    instance("src/utils/b.ts", 40, 65),
                ],
                token_count: 80,
                line_count: 26,
            },
        ];

        let families = group_into_families(&groups, &root());
        assert_eq!(families.len(), 1);
        assert_eq!(
            families[0].suggestions[0].kind,
            RefactoringKind::ExtractModule
        );
        assert!(
            !families[0].suggestions[0]
                .description
                .contains("a shared directory"),
            "Same-directory clones should mention the specific directory"
        );
    }

    #[test]
    fn cross_directory_files_get_shared_directory_hint() {
        let groups = vec![
            CloneGroup {
                instances: vec![instance("src/a.ts", 1, 30), instance("lib/b.ts", 1, 30)],
                token_count: 100,
                line_count: 30,
            },
            CloneGroup {
                instances: vec![instance("src/a.ts", 40, 65), instance("lib/b.ts", 40, 65)],
                token_count: 80,
                line_count: 26,
            },
        ];

        let families = group_into_families(&groups, &root());
        assert_eq!(families.len(), 1);
        assert!(
            families[0].suggestions[0]
                .description
                .contains("a shared directory"),
            "Cross-directory clones should suggest a shared directory"
        );
    }

    #[test]
    fn total_duplicated_tokens_is_sum() {
        let groups = vec![
            CloneGroup {
                instances: vec![instance("src/a.ts", 1, 10), instance("src/b.ts", 1, 10)],
                token_count: 30,
                line_count: 10,
            },
            CloneGroup {
                instances: vec![instance("src/a.ts", 20, 30), instance("src/b.ts", 20, 30)],
                token_count: 45,
                line_count: 11,
            },
        ];

        let families = group_into_families(&groups, &root());
        assert_eq!(families.len(), 1);
        assert_eq!(families[0].total_duplicated_tokens, 75);
    }

    #[test]
    fn multiple_small_groups_each_get_function_suggestion() {
        let groups = vec![
            CloneGroup {
                instances: vec![instance("src/a.ts", 1, 10), instance("src/b.ts", 1, 10)],
                token_count: 30,
                line_count: 10,
            },
            CloneGroup {
                instances: vec![instance("src/a.ts", 20, 30), instance("src/b.ts", 20, 30)],
                token_count: 30,
                line_count: 11,
            },
        ];

        let families = group_into_families(&groups, &root());
        assert_eq!(families.len(), 1);
        assert_eq!(families[0].suggestions.len(), 2);
        assert!(
            families[0]
                .suggestions
                .iter()
                .all(|s| s.kind == RefactoringKind::ExtractFunction)
        );
    }

    #[test]
    fn single_instance_group_zero_savings() {
        let groups = vec![CloneGroup {
            instances: vec![instance("src/a.ts", 1, 10)],
            token_count: 30,
            line_count: 10,
        }];

        let families = group_into_families(&groups, &root());
        assert_eq!(families.len(), 1);
        assert_eq!(families[0].suggestions[0].estimated_savings, 0);
    }

    #[test]
    fn absolute_paths_get_relativized_in_location_hint() {
        let groups = vec![
            CloneGroup {
                instances: vec![
                    instance("/home/user/project/src/utils/a.ts", 1, 30),
                    instance("/home/user/project/src/utils/b.ts", 1, 30),
                ],
                token_count: 100,
                line_count: 30,
            },
            CloneGroup {
                instances: vec![
                    instance("/home/user/project/src/utils/a.ts", 40, 65),
                    instance("/home/user/project/src/utils/b.ts", 40, 65),
                ],
                token_count: 80,
                line_count: 26,
            },
        ];

        let root = PathBuf::from("/home/user/project");
        let families = group_into_families(&groups, &root);
        assert_eq!(families.len(), 1);
        let desc = &families[0].suggestions[0].description;
        assert!(
            desc.contains("src/utils"),
            "Should contain relative path, got: {desc}"
        );
        assert!(
            !desc.contains("/home/user/project"),
            "Should not contain absolute root path, got: {desc}"
        );
    }
}
