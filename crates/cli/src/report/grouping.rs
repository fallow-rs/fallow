//! Grouping infrastructure for `--group-by owner|directory|package`.
//!
//! Partitions `AnalysisResults` into labeled groups by ownership (CODEOWNERS),
//! by first directory component, or by workspace package.

use std::path::{Path, PathBuf};

pub use fallow_api::ResultGroup;
use fallow_config::WorkspaceInfo;
use fallow_types::results::AnalysisResults;

use super::relative_path;
use crate::codeowners::{self, CodeOwners, NO_SECTION_LABEL, UNOWNED_LABEL};

/// Ownership resolver for `--group-by`.
///
/// Owns the `CodeOwners` data when grouping by owner, avoiding lifetime
/// complexity in the report context.
pub enum OwnershipResolver {
    /// Group by CODEOWNERS file (first owner, last matching rule).
    Owner(CodeOwners),
    /// Group by first directory component.
    Directory,
    /// Group by workspace package (monorepo).
    Package(PackageResolver),
    /// Group by GitLab CODEOWNERS section name (`[Section]` headers).
    Section(CodeOwners),
}

/// Resolves file paths to workspace package names via longest-prefix matching.
///
/// Stores workspace roots as paths relative to the project root so that
/// resolution works with the relative paths passed to `OwnershipResolver::resolve`.
pub struct PackageResolver {
    /// `(relative_root, package_name)` sorted by path length descending.
    workspaces: Vec<(PathBuf, String)>,
}

const ROOT_PACKAGE_LABEL: &str = "(root)";

impl PackageResolver {
    /// Build a resolver from discovered workspace info.
    ///
    /// Workspace roots are stored relative to `project_root` and sorted by path
    /// length descending so the first match is always the most specific prefix.
    pub(crate) fn new(project_root: &Path, workspaces: &[WorkspaceInfo]) -> Self {
        let mut ws: Vec<(PathBuf, String)> = workspaces
            .iter()
            .map(|w| {
                let rel = w.root.strip_prefix(project_root).unwrap_or(&w.root);
                (rel.to_path_buf(), w.name.clone())
            })
            .collect();
        ws.sort_by_key(|b| std::cmp::Reverse(b.0.as_os_str().len()));
        Self { workspaces: ws }
    }

    /// Find the workspace package that owns `rel_path`, or `"(root)"` if none match.
    fn resolve(&self, rel_path: &Path) -> &str {
        self.workspaces
            .iter()
            .find(|(root, _)| rel_path.starts_with(root))
            .map_or(ROOT_PACKAGE_LABEL, |(_, name)| name.as_str())
    }
}

impl OwnershipResolver {
    /// Resolve the group key for a file path (relative to project root).
    pub(crate) fn resolve(&self, rel_path: &Path) -> String {
        match self {
            Self::Owner(co) => co.owner_of(rel_path).unwrap_or(UNOWNED_LABEL).to_string(),
            Self::Directory => codeowners::directory_group(rel_path).to_string(),
            Self::Package(pr) => pr.resolve(rel_path).to_string(),
            Self::Section(co) => match co.section_of(rel_path) {
                Some(Some(name)) => name.to_string(),
                Some(None) => NO_SECTION_LABEL.to_string(),
                None => UNOWNED_LABEL.to_string(),
            },
        }
    }

    /// Resolve the group key and matching rule for a path.
    ///
    /// Returns `(owner, Some(pattern))` for Owner mode,
    /// `(directory, None)` for Directory/Package mode,
    /// `(section, Some(pattern))` for Section mode (pattern is the raw
    /// CODEOWNERS pattern from the last matching rule).
    pub(crate) fn resolve_with_rule(&self, rel_path: &Path) -> (String, Option<String>) {
        match self {
            Self::Owner(co) => {
                if let Some((owner, rule)) = co.owner_and_rule_of(rel_path) {
                    (owner.to_string(), Some(rule.to_string()))
                } else {
                    (UNOWNED_LABEL.to_string(), None)
                }
            }
            Self::Directory => (codeowners::directory_group(rel_path).to_string(), None),
            Self::Package(pr) => (pr.resolve(rel_path).to_string(), None),
            Self::Section(co) => {
                if let Some((section, _owners, rule)) = co.section_owners_and_rule_of(rel_path) {
                    let key = section.map_or_else(|| NO_SECTION_LABEL.to_string(), str::to_string);
                    (key, Some(rule.to_string()))
                } else {
                    (UNOWNED_LABEL.to_string(), None)
                }
            }
        }
    }

    /// Label for the grouping mode (used in JSON `grouped_by` field).
    pub(crate) fn mode_label(&self) -> &'static str {
        match self {
            Self::Owner(_) => "owner",
            Self::Directory => "directory",
            Self::Package(_) => "package",
            Self::Section(_) => "section",
        }
    }

    /// Look up the section default owners for a group key.
    ///
    /// Returns `Some(&[...])` only in Section mode when `rel_path` resolves
    /// to a rule inside a named section. Used to emit the `owners` metadata
    /// array in grouped JSON output.
    pub(crate) fn section_owners_of(&self, rel_path: &Path) -> Option<&[String]> {
        if let Self::Section(co) = self
            && let Some((_, owners)) = co.section_and_owners_of(rel_path)
        {
            Some(owners)
        } else {
            None
        }
    }
}

/// Partition analysis results into groups by ownership or directory.
///
/// Each issue is assigned to a group by extracting its primary file path
/// and resolving the group key via the `OwnershipResolver`.
/// Returns groups sorted alphabetically by key, with `(unowned)` last.
pub(crate) fn group_analysis_results(
    results: &AnalysisResults,
    root: &Path,
    resolver: &OwnershipResolver,
) -> Vec<ResultGroup> {
    let is_section_mode = matches!(resolver, OwnershipResolver::Section(_));
    fallow_api::group_analysis_results_with(
        results,
        |path| {
            let rel = relative_path(path, root);
            resolver.resolve(rel)
        },
        |path| {
            let rel = relative_path(path, root);
            resolver.section_owners_of(rel).map(<[String]>::to_vec)
        },
        is_section_mode,
    )
}

/// Resolve the group key for a single path (for per-result tagging in SARIF/CodeClimate).
pub(crate) fn resolve_owner(path: &Path, root: &Path, resolver: &OwnershipResolver) -> String {
    resolver.resolve(relative_path(path, root))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use fallow_types::output_dead_code::*;
    use fallow_types::results::*;

    use super::*;
    use crate::codeowners::CodeOwners;

    fn root() -> PathBuf {
        PathBuf::from("/root")
    }

    fn unused_file(path: &str) -> UnusedFileFinding {
        UnusedFileFinding::with_actions(UnusedFile {
            path: PathBuf::from(path),
        })
    }

    fn unlisted_dep(name: &str, sites: Vec<ImportSite>) -> UnlistedDependencyFinding {
        UnlistedDependencyFinding::with_actions(UnlistedDependency {
            package_name: name.to_string(),
            imported_from: sites,
        })
    }

    fn import_site(path: &str) -> ImportSite {
        ImportSite {
            path: PathBuf::from(path),
            line: 1,
            col: 0,
        }
    }

    #[test]
    fn groups_unused_files_by_directory() {
        let mut results = AnalysisResults::default();
        results.unused_files.push(unused_file("/root/src/a.ts"));
        results.unused_files.push(unused_file("/root/lib/b.ts"));

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].key, "lib");
        assert_eq!(groups[1].key, "src");
    }

    #[test]
    fn groups_dependencies_with_empty_locations_as_unowned() {
        let mut results = AnalysisResults::default();
        results
            .unlisted_dependencies
            .push(unlisted_dep("react", vec![]));

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);
        assert_eq!(groups[0].key, UNOWNED_LABEL);
        assert_eq!(groups[0].results.unlisted_dependencies.len(), 1);
    }

    #[test]
    fn groups_dependencies_by_first_import_site() {
        let mut results = AnalysisResults::default();
        results.unlisted_dependencies.push(unlisted_dep(
            "react",
            vec![import_site("/root/app/page.ts")],
        ));

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);
        assert_eq!(groups[0].key, "app");
    }

    #[test]
    fn owner_grouping_uses_codeowners() {
        let co = CodeOwners::parse("/src/ @frontend\n").unwrap();
        let resolver = OwnershipResolver::Owner(co);
        let mut results = AnalysisResults::default();
        results.unused_files.push(unused_file("/root/src/a.ts"));
        results
            .unused_files
            .push(unused_file("/root/docs/readme.md"));

        let groups = group_analysis_results(&results, &root(), &resolver);
        assert_eq!(groups[0].key, "@frontend");
        assert_eq!(groups[1].key, UNOWNED_LABEL);
    }

    #[test]
    fn unowned_group_is_pinned_last() {
        let co = CodeOwners::parse("/src/ @frontend\n").unwrap();
        let resolver = OwnershipResolver::Owner(co);
        let mut results = AnalysisResults::default();
        results.unused_files.push(unused_file("/root/aaa/a.ts"));
        results.unused_files.push(unused_file("/root/src/a.ts"));

        let groups = group_analysis_results(&results, &root(), &resolver);
        assert_eq!(groups.last().unwrap().key, UNOWNED_LABEL);
    }

    #[test]
    fn sorts_by_issue_count_then_key() {
        let mut results = AnalysisResults::default();
        results.unused_files.push(unused_file("/root/src/a.ts"));
        results.unused_files.push(unused_file("/root/app/a.ts"));
        results.unused_files.push(unused_file("/root/app/b.ts"));

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);
        assert_eq!(groups[0].key, "app");
        assert_eq!(groups[1].key, "src");
    }

    #[test]
    fn resolve_owner_returns_directory() {
        let owner = resolve_owner(
            Path::new("/root/src/file.ts"),
            &root(),
            &OwnershipResolver::Directory,
        );
        assert_eq!(owner, "src");
    }

    #[test]
    fn resolve_owner_returns_codeowner() {
        let co = CodeOwners::parse("/src/ @frontend\n").unwrap();
        let resolver = OwnershipResolver::Owner(co);
        let owner = resolve_owner(Path::new("/root/src/file.ts"), &root(), &resolver);
        assert_eq!(owner, "@frontend");
    }

    #[test]
    fn mode_label_matches_group_by_flag() {
        assert_eq!(OwnershipResolver::Directory.mode_label(), "directory");
        let pr = PackageResolver {
            workspaces: vec![(PathBuf::from("packages/a"), "a".to_string())],
        };
        assert_eq!(OwnershipResolver::Package(pr).mode_label(), "package");
        let co = CodeOwners::parse("[Docs]\n/docs/ @docs\n").unwrap();
        assert_eq!(OwnershipResolver::Section(co).mode_label(), "section");
    }
}
