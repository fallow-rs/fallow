//! Grouping infrastructure for `--group-by owner|directory|package`.
//!
//! Partitions `AnalysisResults` into labeled groups by ownership (CODEOWNERS),
//! by first directory component, or by workspace package.

use std::path::{Path, PathBuf};

use fallow_config::WorkspaceInfo;
use fallow_core::results::AnalysisResults;
use rustc_hash::FxHashMap;

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
    pub fn new(project_root: &Path, workspaces: &[WorkspaceInfo]) -> Self {
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
    pub fn resolve(&self, rel_path: &Path) -> String {
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
    pub fn resolve_with_rule(&self, rel_path: &Path) -> (String, Option<String>) {
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
    pub fn mode_label(&self) -> &'static str {
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
    pub fn section_owners_of(&self, rel_path: &Path) -> Option<&[String]> {
        if let Self::Section(co) = self
            && let Some((_, owners)) = co.section_and_owners_of(rel_path)
        {
            Some(owners)
        } else {
            None
        }
    }
}

/// A single group: a label and its subset of results.
pub struct ResultGroup {
    /// Group label (owner name, directory, package, or section).
    pub key: String,
    /// Section default owners for `--group-by section`.
    ///
    /// `None` for all other grouping modes. `Some(vec![])` for the
    /// `(no section)` and `(unowned)` buckets in Section mode.
    pub owners: Option<Vec<String>>,
    /// Issues belonging to this group.
    pub results: AnalysisResults,
}

/// Partition analysis results into groups by ownership or directory.
///
/// Each issue is assigned to a group by extracting its primary file path
/// and resolving the group key via the `OwnershipResolver`.
/// Returns groups sorted alphabetically by key, with `(unowned)` last.
pub fn group_analysis_results(
    results: &AnalysisResults,
    root: &Path,
    resolver: &OwnershipResolver,
) -> Vec<ResultGroup> {
    let mut group_owners: FxHashMap<String, Vec<String>> = FxHashMap::default();
    let is_section_mode = matches!(resolver, OwnershipResolver::Section(_));

    let key_for = |path: &Path| -> String {
        let rel = relative_path(path, root);
        let key = resolver.resolve(rel);
        if is_section_mode && !group_owners.contains_key(&key) {
            let owners = resolver
                .section_owners_of(rel)
                .map(<[String]>::to_vec)
                .unwrap_or_default();
            group_owners.insert(key.clone(), owners);
        }
        key
    };

    let mut builder = GroupingBuilder::new(key_for);
    builder.group_symbol_issues(results);
    builder.group_dependency_issues(results);
    builder.group_relationship_issues(results);
    builder.group_workspace_config_issues(results);

    finalize_groups(builder.into_groups(), group_owners, is_section_mode)
}

struct GroupingBuilder<F> {
    groups: FxHashMap<String, AnalysisResults>,
    key_for: F,
}

impl<F> GroupingBuilder<F>
where
    F: FnMut(&Path) -> String,
{
    fn new(key_for: F) -> Self {
        Self {
            groups: FxHashMap::default(),
            key_for,
        }
    }

    fn entry_for_path(&mut self, path: &Path) -> &mut AnalysisResults {
        let key = (self.key_for)(path);
        self.groups.entry(key).or_default()
    }

    fn entry_for_key(&mut self, key: String) -> &mut AnalysisResults {
        self.groups.entry(key).or_default()
    }

    fn into_groups(self) -> FxHashMap<String, AnalysisResults> {
        self.groups
    }

    fn group_symbol_issues(&mut self, results: &AnalysisResults) {
        for item in &results.unused_files {
            self.entry_for_path(&item.file.path)
                .unused_files
                .push(item.clone());
        }
        for item in &results.unused_exports {
            self.entry_for_path(&item.export.path)
                .unused_exports
                .push(item.clone());
        }
        for item in &results.unused_types {
            self.entry_for_path(&item.export.path)
                .unused_types
                .push(item.clone());
        }
        for item in &results.private_type_leaks {
            self.entry_for_path(&item.leak.path)
                .private_type_leaks
                .push(item.clone());
        }
        for item in &results.unused_enum_members {
            self.entry_for_path(&item.member.path)
                .unused_enum_members
                .push(item.clone());
        }
        for item in &results.unused_class_members {
            self.entry_for_path(&item.member.path)
                .unused_class_members
                .push(item.clone());
        }
        for item in &results.unused_store_members {
            self.entry_for_path(&item.member.path)
                .unused_store_members
                .push(item.clone());
        }
        for item in &results.unresolved_imports {
            self.entry_for_path(&item.import.path)
                .unresolved_imports
                .push(item.clone());
        }
    }

    fn group_dependency_issues(&mut self, results: &AnalysisResults) {
        for item in &results.unused_dependencies {
            self.entry_for_path(&item.dep.path)
                .unused_dependencies
                .push(item.clone());
        }
        for item in &results.unused_dev_dependencies {
            self.entry_for_path(&item.dep.path)
                .unused_dev_dependencies
                .push(item.clone());
        }
        for item in &results.unused_optional_dependencies {
            self.entry_for_path(&item.dep.path)
                .unused_optional_dependencies
                .push(item.clone());
        }
        for item in &results.type_only_dependencies {
            self.entry_for_path(&item.dep.path)
                .type_only_dependencies
                .push(item.clone());
        }
        for item in &results.test_only_dependencies {
            self.entry_for_path(&item.dep.path)
                .test_only_dependencies
                .push(item.clone());
        }

        for item in &results.unlisted_dependencies {
            let key = item.dep.imported_from.first().map_or_else(
                || UNOWNED_LABEL.to_string(),
                |site| (self.key_for)(&site.path),
            );
            self.entry_for_key(key)
                .unlisted_dependencies
                .push(item.clone());
        }
        for item in &results.duplicate_exports {
            let key = item.export.locations.first().map_or_else(
                || UNOWNED_LABEL.to_string(),
                |loc| (self.key_for)(&loc.path),
            );
            self.entry_for_key(key).duplicate_exports.push(item.clone());
        }
    }

    fn group_relationship_issues(&mut self, results: &AnalysisResults) {
        for item in &results.circular_dependencies {
            let key = item
                .cycle
                .files
                .first()
                .map_or_else(|| UNOWNED_LABEL.to_string(), |f| (self.key_for)(f));
            self.entry_for_key(key)
                .circular_dependencies
                .push(item.clone());
        }
        for item in &results.boundary_violations {
            self.entry_for_path(&item.violation.from_path)
                .boundary_violations
                .push(item.clone());
        }
        for item in &results.boundary_coverage_violations {
            self.entry_for_path(&item.violation.path)
                .boundary_coverage_violations
                .push(item.clone());
        }
        for item in &results.boundary_call_violations {
            self.entry_for_path(&item.violation.path)
                .boundary_call_violations
                .push(item.clone());
        }
        for item in &results.policy_violations {
            self.entry_for_path(&item.violation.path)
                .policy_violations
                .push(item.clone());
        }
        for item in &results.invalid_client_exports {
            self.entry_for_path(&item.export.path)
                .invalid_client_exports
                .push(item.clone());
        }
        for item in &results.mixed_client_server_barrels {
            self.entry_for_path(&item.barrel.path)
                .mixed_client_server_barrels
                .push(item.clone());
        }
        for item in &results.misplaced_directives {
            self.entry_for_path(&item.directive_site.path)
                .misplaced_directives
                .push(item.clone());
        }
        for item in &results.unprovided_injects {
            self.entry_for_path(&item.inject.path)
                .unprovided_injects
                .push(item.clone());
        }
        for item in &results.unrendered_components {
            self.entry_for_path(&item.component.path)
                .unrendered_components
                .push(item.clone());
        }
        for item in &results.unused_component_props {
            self.entry_for_path(&item.prop.path)
                .unused_component_props
                .push(item.clone());
        }
        for item in &results.unused_component_emits {
            self.entry_for_path(&item.emit.path)
                .unused_component_emits
                .push(item.clone());
        }
        for item in &results.unused_server_actions {
            self.entry_for_path(&item.action.path)
                .unused_server_actions
                .push(item.clone());
        }
        for item in &results.stale_suppressions {
            self.entry_for_path(&item.path)
                .stale_suppressions
                .push(item.clone());
        }
    }

    fn group_workspace_config_issues(&mut self, results: &AnalysisResults) {
        for item in &results.unused_catalog_entries {
            self.entry_for_path(&item.entry.path)
                .unused_catalog_entries
                .push(item.clone());
        }
        for item in &results.empty_catalog_groups {
            self.entry_for_path(&item.group.path)
                .empty_catalog_groups
                .push(item.clone());
        }
        for item in &results.unresolved_catalog_references {
            self.entry_for_path(&item.reference.path)
                .unresolved_catalog_references
                .push(item.clone());
        }
        for item in &results.unused_dependency_overrides {
            self.entry_for_path(&item.entry.path)
                .unused_dependency_overrides
                .push(item.clone());
        }
        for item in &results.misconfigured_dependency_overrides {
            self.entry_for_path(&item.entry.path)
                .misconfigured_dependency_overrides
                .push(item.clone());
        }
    }
}

/// Merge per-key results and owners into sorted `ResultGroup`s.
///
/// Ordering: most issues first, alphabetical tiebreaker, `(unowned)` pinned to
/// the end. `group_owners` is consumed only when `is_section_mode` is true.
fn finalize_groups(
    groups: FxHashMap<String, AnalysisResults>,
    mut group_owners: FxHashMap<String, Vec<String>>,
    is_section_mode: bool,
) -> Vec<ResultGroup> {
    let mut sorted: Vec<_> = groups
        .into_iter()
        .map(|(key, results)| {
            let owners = if is_section_mode {
                Some(group_owners.remove(&key).unwrap_or_default())
            } else {
                None
            };
            ResultGroup {
                key,
                owners,
                results,
            }
        })
        .collect();
    sorted.sort_by(|a, b| {
        let a_unowned = a.key == UNOWNED_LABEL;
        let b_unowned = b.key == UNOWNED_LABEL;
        match (a_unowned, b_unowned) {
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            _ => b
                .results
                .total_issues()
                .cmp(&a.results.total_issues())
                .then_with(|| a.key.cmp(&b.key)),
        }
    });
    sorted
}

/// Resolve the group key for a single path (for per-result tagging in SARIF/CodeClimate).
pub fn resolve_owner(path: &Path, root: &Path, resolver: &OwnershipResolver) -> String {
    resolver.resolve(relative_path(path, root))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use fallow_core::results::*;

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

    fn unused_export(path: &str, name: &str) -> UnusedExportFinding {
        UnusedExportFinding::with_actions(UnusedExport {
            path: PathBuf::from(path),
            export_name: name.to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
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
    fn empty_results_returns_empty_vec() {
        let results = AnalysisResults::default();
        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);
        assert!(groups.is_empty());
    }

    #[test]
    fn single_group_all_same_directory() {
        let mut results = AnalysisResults::default();
        results.unused_files.push(unused_file("/root/src/a.ts"));
        results.unused_files.push(unused_file("/root/src/b.ts"));
        results
            .unused_exports
            .push(unused_export("/root/src/c.ts", "foo"));

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key, "src");
        assert_eq!(groups[0].results.unused_files.len(), 2);
        assert_eq!(groups[0].results.unused_exports.len(), 1);
        assert_eq!(groups[0].results.total_issues(), 3);
    }

    #[test]
    fn multiple_groups_split_by_directory() {
        let mut results = AnalysisResults::default();
        results.unused_files.push(unused_file("/root/src/a.ts"));
        results.unused_files.push(unused_file("/root/lib/b.ts"));
        results
            .unused_exports
            .push(unused_export("/root/src/c.ts", "bar"));

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);

        assert_eq!(groups.len(), 2);

        let src_group = groups.iter().find(|g| g.key == "src").unwrap();
        let lib_group = groups.iter().find(|g| g.key == "lib").unwrap();

        assert_eq!(src_group.results.total_issues(), 2);
        assert_eq!(lib_group.results.total_issues(), 1);
    }

    #[test]
    fn sort_order_descending_by_total_issues() {
        let mut results = AnalysisResults::default();
        results.unused_files.push(unused_file("/root/lib/a.ts"));
        results.unused_files.push(unused_file("/root/src/a.ts"));
        results.unused_files.push(unused_file("/root/src/b.ts"));
        results
            .unused_exports
            .push(unused_export("/root/src/c.ts", "x"));
        results.unused_files.push(unused_file("/root/test/a.ts"));
        results.unused_files.push(unused_file("/root/test/b.ts"));

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);

        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].key, "src");
        assert_eq!(groups[0].results.total_issues(), 3);
        assert_eq!(groups[1].key, "test");
        assert_eq!(groups[1].results.total_issues(), 2);
        assert_eq!(groups[2].key, "lib");
        assert_eq!(groups[2].results.total_issues(), 1);
    }

    #[test]
    fn sort_order_alphabetical_tiebreaker() {
        let mut results = AnalysisResults::default();
        results.unused_files.push(unused_file("/root/beta/a.ts"));
        results.unused_files.push(unused_file("/root/alpha/a.ts"));

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].key, "alpha");
        assert_eq!(groups[1].key, "beta");
    }

    #[test]
    fn unowned_sorts_last_regardless_of_count() {
        let mut results = AnalysisResults::default();
        results.unused_files.push(unused_file("/root/src/a.ts"));
        results
            .unlisted_dependencies
            .push(unlisted_dep("pkg-a", vec![]));
        results
            .unlisted_dependencies
            .push(unlisted_dep("pkg-b", vec![]));
        results
            .unlisted_dependencies
            .push(unlisted_dep("pkg-c", vec![]));

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].key, "src");
        assert_eq!(groups[1].key, UNOWNED_LABEL);
        assert_eq!(groups[1].results.total_issues(), 3);
    }

    #[test]
    fn unlisted_dep_empty_imported_from_goes_to_unowned() {
        let mut results = AnalysisResults::default();
        results
            .unlisted_dependencies
            .push(unlisted_dep("missing-pkg", vec![]));

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key, UNOWNED_LABEL);
        assert_eq!(groups[0].results.unlisted_dependencies.len(), 1);
    }

    #[test]
    fn unlisted_dep_with_import_site_goes_to_directory() {
        let mut results = AnalysisResults::default();
        results.unlisted_dependencies.push(unlisted_dep(
            "lodash",
            vec![import_site("/root/src/util.ts")],
        ));

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key, "src");
        assert_eq!(groups[0].results.unlisted_dependencies.len(), 1);
    }

    #[test]
    fn directory_mode_groups_by_first_path_component() {
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(unused_file("/root/packages/ui/Button.ts"));
        results
            .unused_files
            .push(unused_file("/root/packages/auth/login.ts"));
        results
            .unused_exports
            .push(unused_export("/root/apps/web/index.ts", "main"));

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);

        assert_eq!(groups.len(), 2);

        let pkgs = groups.iter().find(|g| g.key == "packages").unwrap();
        let apps = groups.iter().find(|g| g.key == "apps").unwrap();

        assert_eq!(pkgs.results.total_issues(), 2);
        assert_eq!(apps.results.total_issues(), 1);
    }

    #[test]
    fn owner_mode_groups_by_codeowners_owner() {
        let co = CodeOwners::parse("* @default\n/src/ @frontend\n").unwrap();
        let resolver = OwnershipResolver::Owner(co);

        let mut results = AnalysisResults::default();
        results.unused_files.push(unused_file("/root/src/app.ts"));
        results.unused_files.push(unused_file("/root/README.md"));

        let groups = group_analysis_results(&results, &root(), &resolver);

        assert_eq!(groups.len(), 2);

        let frontend = groups.iter().find(|g| g.key == "@frontend").unwrap();
        let default = groups.iter().find(|g| g.key == "@default").unwrap();

        assert_eq!(frontend.results.unused_files.len(), 1);
        assert_eq!(default.results.unused_files.len(), 1);
    }

    #[test]
    fn owner_mode_unmatched_goes_to_unowned() {
        let co = CodeOwners::parse("/src/ @frontend\n").unwrap();
        let resolver = OwnershipResolver::Owner(co);

        let mut results = AnalysisResults::default();
        results.unused_files.push(unused_file("/root/README.md"));

        let groups = group_analysis_results(&results, &root(), &resolver);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key, UNOWNED_LABEL);
    }

    #[test]
    fn boundary_violations_grouped_by_from_path() {
        let mut results = AnalysisResults::default();
        results
            .boundary_violations
            .push(BoundaryViolationFinding::with_actions(BoundaryViolation {
                from_path: PathBuf::from("/root/src/bad.ts"),
                to_path: PathBuf::from("/root/lib/secret.ts"),
                from_zone: "src".to_string(),
                to_zone: "lib".to_string(),
                import_specifier: "../lib/secret".to_string(),
                line: 1,
                col: 0,
            }));

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key, "src");
        assert_eq!(groups[0].results.boundary_violations.len(), 1);
    }

    #[test]
    fn circular_dep_empty_files_goes_to_unowned() {
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![],
                    length: 0,
                    line: 0,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key, UNOWNED_LABEL);
    }

    #[test]
    fn circular_dep_uses_first_file() {
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![
                        PathBuf::from("/root/src/a.ts"),
                        PathBuf::from("/root/lib/b.ts"),
                    ],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key, "src");
    }

    #[test]
    fn duplicate_exports_empty_locations_goes_to_unowned() {
        let mut results = AnalysisResults::default();
        results
            .duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "dup".to_string(),
                locations: vec![],
            }));

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key, UNOWNED_LABEL);
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
        let co = CodeOwners::parse("/src/ @team\n").unwrap();
        let resolver = OwnershipResolver::Owner(co);
        let owner = resolve_owner(Path::new("/root/src/file.ts"), &root(), &resolver);
        assert_eq!(owner, "@team");
    }

    #[test]
    fn mode_label_owner() {
        let co = CodeOwners::parse("").unwrap();
        let resolver = OwnershipResolver::Owner(co);
        assert_eq!(resolver.mode_label(), "owner");
    }

    #[test]
    fn mode_label_directory() {
        assert_eq!(OwnershipResolver::Directory.mode_label(), "directory");
    }

    #[test]
    fn mode_label_package() {
        let pr = PackageResolver { workspaces: vec![] };
        assert_eq!(OwnershipResolver::Package(pr).mode_label(), "package");
    }

    #[test]
    fn mode_label_section() {
        let co = CodeOwners::parse("[S] @owner\nfoo/\n").unwrap();
        assert_eq!(OwnershipResolver::Section(co).mode_label(), "section");
    }

    #[test]
    fn section_mode_groups_distinct_sections_with_shared_owners() {
        let content = "\
            [billing] @core-reviewers @alice @bob\n\
            src/billing/\n\
            [notifications] @core-reviewers @alice @bob\n\
            src/notifications/\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        let resolver = OwnershipResolver::Section(co);

        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(unused_file("/root/src/billing/a.ts"));
        results
            .unused_files
            .push(unused_file("/root/src/billing/b.ts"));
        results
            .unused_files
            .push(unused_file("/root/src/notifications/c.ts"));

        let groups = group_analysis_results(&results, &root(), &resolver);

        assert_eq!(groups.len(), 2);
        let billing = groups.iter().find(|g| g.key == "billing").unwrap();
        let notifications = groups.iter().find(|g| g.key == "notifications").unwrap();
        assert_eq!(billing.results.total_issues(), 2);
        assert_eq!(notifications.results.total_issues(), 1);
        assert_eq!(
            billing.owners.as_deref(),
            Some(
                [
                    "@core-reviewers".to_string(),
                    "@alice".to_string(),
                    "@bob".to_string()
                ]
                .as_slice()
            )
        );
        assert_eq!(
            notifications.owners.as_deref(),
            Some(
                [
                    "@core-reviewers".to_string(),
                    "@alice".to_string(),
                    "@bob".to_string()
                ]
                .as_slice()
            )
        );
    }

    #[test]
    fn section_mode_pre_section_rule_goes_to_no_section() {
        let content = "\
            * @default\n\
            [Utilities] @utils\n\
            src/utils/\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        let resolver = OwnershipResolver::Section(co);

        let mut results = AnalysisResults::default();
        results.unused_files.push(unused_file("/root/README.md"));
        results
            .unused_files
            .push(unused_file("/root/src/utils/greet.ts"));

        let groups = group_analysis_results(&results, &root(), &resolver);

        assert_eq!(groups.len(), 2);
        let no_section = groups.iter().find(|g| g.key == "(no section)").unwrap();
        let utils = groups.iter().find(|g| g.key == "Utilities").unwrap();
        assert_eq!(no_section.owners.as_deref(), Some([].as_slice()));
        assert_eq!(
            utils.owners.as_deref(),
            Some(["@utils".to_string()].as_slice())
        );
    }

    #[test]
    fn section_mode_unmatched_goes_to_unowned() {
        let co = CodeOwners::parse("[Utilities] @utils\nsrc/utils/\n").unwrap();
        let resolver = OwnershipResolver::Section(co);

        let mut results = AnalysisResults::default();
        results.unused_files.push(unused_file("/root/README.md"));

        let groups = group_analysis_results(&results, &root(), &resolver);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key, UNOWNED_LABEL);
        assert_eq!(groups[0].owners.as_deref(), Some([].as_slice()));
    }

    #[test]
    fn directory_mode_groups_have_no_owners_metadata() {
        let mut results = AnalysisResults::default();
        results.unused_files.push(unused_file("/root/src/a.ts"));
        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);
        assert_eq!(groups[0].owners, None);
    }

    #[test]
    fn package_resolver_matches_longest_prefix() {
        let ws = vec![
            fallow_config::WorkspaceInfo {
                name: "packages/ui".to_string(),
                root: PathBuf::from("/root/packages/ui"),
                is_internal_dependency: false,
            },
            fallow_config::WorkspaceInfo {
                name: "packages".to_string(),
                root: PathBuf::from("/root/packages"),
                is_internal_dependency: false,
            },
        ];
        let pr = PackageResolver::new(Path::new("/root"), &ws);
        assert_eq!(
            pr.resolve(Path::new("packages/ui/Button.ts")),
            "packages/ui"
        );
    }

    #[test]
    fn package_resolver_root_fallback() {
        let ws = vec![fallow_config::WorkspaceInfo {
            name: "packages/ui".to_string(),
            root: PathBuf::from("/root/packages/ui"),
            is_internal_dependency: false,
        }];
        let pr = PackageResolver::new(Path::new("/root"), &ws);
        assert_eq!(pr.resolve(Path::new("src/app.ts")), ROOT_PACKAGE_LABEL);
    }

    #[test]
    fn package_mode_groups_by_workspace() {
        let ws = vec![
            fallow_config::WorkspaceInfo {
                name: "ui".to_string(),
                root: PathBuf::from("/root/packages/ui"),
                is_internal_dependency: false,
            },
            fallow_config::WorkspaceInfo {
                name: "auth".to_string(),
                root: PathBuf::from("/root/packages/auth"),
                is_internal_dependency: false,
            },
        ];
        let pr = PackageResolver::new(Path::new("/root"), &ws);
        let resolver = OwnershipResolver::Package(pr);

        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(unused_file("/root/packages/ui/Button.ts"));
        results
            .unused_files
            .push(unused_file("/root/packages/auth/login.ts"));
        results.unused_files.push(unused_file("/root/src/main.ts"));

        let groups = group_analysis_results(&results, &root(), &resolver);
        assert_eq!(groups.len(), 3);

        let ui_group = groups.iter().find(|g| g.key == "ui");
        let auth_group = groups.iter().find(|g| g.key == "auth");
        let root_group = groups.iter().find(|g| g.key == ROOT_PACKAGE_LABEL);

        assert!(ui_group.is_some());
        assert!(auth_group.is_some());
        assert!(root_group.is_some());
    }

    #[test]
    fn resolve_with_rule_directory_mode_no_rule() {
        let (key, rule) = OwnershipResolver::Directory.resolve_with_rule(Path::new("src/file.ts"));
        assert_eq!(key, "src");
        assert!(rule.is_none());
    }

    #[test]
    fn resolve_with_rule_owner_mode_with_match() {
        let co = CodeOwners::parse("/src/ @frontend\n").unwrap();
        let resolver = OwnershipResolver::Owner(co);
        let (key, rule) = resolver.resolve_with_rule(Path::new("src/file.ts"));
        assert_eq!(key, "@frontend");
        assert!(rule.is_some());
        assert!(rule.unwrap().contains("src"));
    }

    #[test]
    fn resolve_with_rule_owner_mode_no_match() {
        let co = CodeOwners::parse("/src/ @frontend\n").unwrap();
        let resolver = OwnershipResolver::Owner(co);
        let (key, rule) = resolver.resolve_with_rule(Path::new("docs/readme.md"));
        assert_eq!(key, UNOWNED_LABEL);
        assert!(rule.is_none());
    }

    #[test]
    fn resolve_with_rule_package_mode_no_rule() {
        let pr = PackageResolver { workspaces: vec![] };
        let resolver = OwnershipResolver::Package(pr);
        let (key, rule) = resolver.resolve_with_rule(Path::new("src/file.ts"));
        assert_eq!(key, ROOT_PACKAGE_LABEL);
        assert!(rule.is_none());
    }

    #[test]
    fn group_unused_optional_deps() {
        let mut results = AnalysisResults::default();
        results
            .unused_optional_dependencies
            .push(UnusedOptionalDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "fsevents".to_string(),
                    location: fallow_core::results::DependencyLocation::OptionalDependencies,
                    path: PathBuf::from("/root/package.json"),
                    line: 5,
                    used_in_workspaces: Vec::new(),
                },
            ));

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].results.unused_optional_dependencies.len(), 1);
    }

    #[test]
    fn group_type_only_deps() {
        let mut results = AnalysisResults::default();
        results.type_only_dependencies.push(
            fallow_core::results::TypeOnlyDependencyFinding::with_actions(TypeOnlyDependency {
                package_name: "zod".to_string(),
                path: PathBuf::from("/root/package.json"),
                line: 8,
            }),
        );

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].results.type_only_dependencies.len(), 1);
    }

    #[test]
    fn group_test_only_deps() {
        let mut results = AnalysisResults::default();
        results.test_only_dependencies.push(
            fallow_core::results::TestOnlyDependencyFinding::with_actions(TestOnlyDependency {
                package_name: "vitest".to_string(),
                path: PathBuf::from("/root/package.json"),
                line: 10,
            }),
        );

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].results.test_only_dependencies.len(), 1);
    }

    #[test]
    fn group_unused_enum_members() {
        let mut results = AnalysisResults::default();
        results.unused_enum_members.push(
            fallow_core::results::UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/root/src/types.ts"),
                parent_name: "Status".to_string(),
                member_name: "Deprecated".to_string(),
                kind: fallow_core::extract::MemberKind::EnumMember,
                line: 5,
                col: 0,
            }),
        );

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key, "src");
        assert_eq!(groups[0].results.unused_enum_members.len(), 1);
    }

    #[test]
    fn group_unused_class_members() {
        let mut results = AnalysisResults::default();
        results.unused_class_members.push(
            fallow_core::results::UnusedClassMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/root/lib/service.ts"),
                parent_name: "UserService".to_string(),
                member_name: "legacyMethod".to_string(),
                kind: fallow_core::extract::MemberKind::ClassMethod,
                line: 42,
                col: 0,
            }),
        );

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key, "lib");
        assert_eq!(groups[0].results.unused_class_members.len(), 1);
    }

    #[test]
    fn group_unresolved_imports() {
        let mut results = AnalysisResults::default();
        results.unresolved_imports.push(
            fallow_types::output_dead_code::UnresolvedImportFinding::with_actions(
                fallow_core::results::UnresolvedImport {
                    path: PathBuf::from("/root/src/app.ts"),
                    specifier: "./missing".to_string(),
                    line: 1,
                    col: 0,
                    specifier_col: 0,
                },
            ),
        );

        let groups = group_analysis_results(&results, &root(), &OwnershipResolver::Directory);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key, "src");
        assert_eq!(groups[0].results.unresolved_imports.len(), 1);
    }
}
