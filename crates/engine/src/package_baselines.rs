//! Per-workspace Git baselines for changed-file result scoping.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fallow_config::WorkspaceInfo;
use rustc_hash::FxHashSet;

use crate::changed_files::{ChangedFilesBatch, ChangedFilesError, ChangedPathScope};

/// Failure to resolve an authored workspace baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageBaselineError {
    /// The project or a discovered workspace root could not be resolved.
    UnavailableRoot {
        /// Root that could not be resolved.
        path: PathBuf,
        /// Filesystem error detail.
        message: String,
    },
    /// A key is not an exact, relative, slash-separated workspace root.
    InvalidWorkspaceKey {
        /// Authored key.
        key: String,
    },
    /// No discovered workspace has this root.
    UnknownWorkspace {
        /// Authored key.
        key: String,
        /// The closest discovered workspace root, when one is close.
        suggestion: Option<String>,
    },
    /// Git could not resolve a package's baseline ref.
    Git {
        /// Authored workspace key.
        key: String,
        /// Authored Git ref.
        reference: String,
        /// Underlying Git error.
        source: ChangedFilesError,
    },
}

impl fmt::Display for PackageBaselineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnavailableRoot { path, message } => {
                write!(
                    f,
                    "cannot resolve workspace root '{}': {message}",
                    path.display()
                )
            }
            Self::InvalidWorkspaceKey { key } => write!(
                f,
                "workspace baseline key '{key}' must be an exact project-relative workspace root"
            ),
            Self::UnknownWorkspace { key, suggestion } => {
                write!(
                    f,
                    "workspace baseline key '{key}' names no discovered workspace"
                )?;
                match suggestion {
                    Some(suggestion) => write!(f, "; did you mean '{suggestion}'?"),
                    None => Ok(()),
                }
            }
            Self::Git {
                key,
                reference,
                source,
            } => write!(
                f,
                "workspace baseline '{reference}' for '{key}' failed: {}",
                source.describe()
            ),
        }
    }
}

impl std::error::Error for PackageBaselineError {}

#[derive(Debug, Clone)]
enum WorkspaceBaseline {
    Full,
    Changed {
        /// The authored key, as discovery reports the workspace root.
        key: String,
        reference: String,
        files: Arc<FxHashSet<PathBuf>>,
    },
}

/// Resolved package baselines for one analysis project.
///
/// A file belongs to its nearest discovered workspace root. Workspaces with
/// no authored baseline and files outside workspace roots remain in full scope.
#[derive(Debug, Clone)]
pub struct PackageChangeScope {
    root: PathBuf,
    authored_root: PathBuf,
    workspaces: BTreeMap<PathBuf, WorkspaceBaseline>,
}

impl PackageChangeScope {
    /// Canonical analysis root used by this package scope.
    #[must_use]
    pub fn project_root(&self) -> &Path {
        &self.root
    }

    /// Whether this scope owns a project path, including root-level files.
    #[must_use]
    pub fn covers(&self, path: &Path) -> bool {
        self.absolute_path(path).starts_with(&self.root)
    }

    /// Authored workspace keys and their refs. A key is project-relative and
    /// uses the path that discovery reports, also for a symlinked workspace.
    pub fn configured_baselines(&self) -> impl Iterator<Item = (&str, &str)> {
        self.workspaces.values().filter_map(|baseline| {
            if let WorkspaceBaseline::Changed { key, reference, .. } = baseline {
                Some((key.as_str(), reference.as_str()))
            } else {
                None
            }
        })
    }

    /// Resolve configured refs and discovered workspace ownership atomically.
    /// An empty map means no package scope was requested.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid workspace key, an undiscovered package,
    /// an unavailable root, or any Git ref that cannot be resolved.
    pub(crate) fn resolve(
        root: &Path,
        configured: &BTreeMap<String, String>,
        workspaces: &[WorkspaceInfo],
    ) -> Result<Option<Self>, PackageBaselineError> {
        if configured.is_empty() {
            return Ok(None);
        }

        let authored_root = dunce::simplified(root).to_path_buf();
        let root = canonical_root(root)?;
        let mut packages = BTreeMap::new();
        // A key names a workspace root exactly as discovery reports it, which
        // is also what `fallow list --workspaces` prints. A symlinked
        // workspace is therefore mapped under its link path, and a symlink
        // that discovery does not report is not an alias for a workspace.
        let mut roots_by_key = BTreeMap::new();
        for workspace in workspaces {
            let canonical = canonical_root(&workspace.root)?;
            if let Some(key) = workspace_key(&workspace.root, &authored_root, &root) {
                roots_by_key.insert(key, canonical.clone());
            }
            packages.insert(canonical, WorkspaceBaseline::Full);
        }

        let mut validated = Vec::with_capacity(configured.len());
        for (key, reference) in configured {
            if !fallow_config::glob_validation::is_exact_workspace_root(key) {
                return Err(PackageBaselineError::InvalidWorkspaceKey { key: key.clone() });
            }
            let Some(path) = roots_by_key.get(key) else {
                return Err(PackageBaselineError::UnknownWorkspace {
                    key: key.clone(),
                    suggestion: fallow_config::levenshtein::closest_match(
                        key,
                        roots_by_key.keys().map(String::as_str),
                    )
                    .map(str::to_owned),
                });
            };
            validated.push((key, reference, path.clone()));
        }

        let configured_owners: BTreeMap<PathBuf, &str> = validated
            .iter()
            .map(|(_, reference, path)| (path.clone(), reference.as_str()))
            .collect();
        let Some((first_key, first_ref, _)) = validated.first() else {
            return Ok(None);
        };
        let mut batch = ChangedFilesBatch::new(&root, first_ref).map_err(|source| {
            PackageBaselineError::Git {
                key: (*first_key).clone(),
                reference: (*first_ref).clone(),
                source,
            }
        })?;
        let mut refs = BTreeMap::<String, Arc<FxHashSet<PathBuf>>>::new();
        for (key, reference, path) in validated {
            let files = if let Some(files) = refs.get(reference) {
                Arc::clone(files)
            } else {
                let files =
                    batch
                        .changed_files(reference)
                        .map_err(|source| PackageBaselineError::Git {
                            key: key.clone(),
                            reference: reference.clone(),
                            source,
                        })?;
                let files = Arc::new(
                    files
                        .into_iter()
                        .map(|path| dunce::simplified(&path).to_path_buf())
                        .filter(|file| {
                            let owner = file
                                .ancestors()
                                .find(|ancestor| packages.contains_key(*ancestor));
                            owner
                                .and_then(|owner| configured_owners.get(owner))
                                .is_some_and(|owner_ref| *owner_ref == reference.as_str())
                        })
                        .collect(),
                );
                refs.insert(reference.clone(), Arc::clone(&files));
                files
            };
            packages.insert(
                path,
                WorkspaceBaseline::Changed {
                    key: key.clone(),
                    reference: reference.clone(),
                    files,
                },
            );
        }

        Ok(Some(Self {
            root,
            authored_root,
            workspaces: packages,
        }))
    }

    /// Effective Git ref for a file, or `None` in a full-scope workspace.
    #[must_use]
    pub fn baseline_for(&self, path: &Path) -> Option<&str> {
        match self.owner_absolute(&self.absolute_path(path)) {
            Some(WorkspaceBaseline::Changed { reference, .. }) => Some(reference),
            Some(WorkspaceBaseline::Full) | None => None,
        }
    }

    /// Whether a finding owner path belongs to this mixed scope.
    #[must_use]
    pub(crate) fn includes(&self, path: &Path) -> bool {
        let absolute = self.absolute_path(path);
        match self.owner_absolute(&absolute) {
            Some(WorkspaceBaseline::Changed { files, .. }) => files.contains(&absolute),
            Some(WorkspaceBaseline::Full) | None => true,
        }
    }

    fn owner_absolute(&self, path: &Path) -> Option<&WorkspaceBaseline> {
        path.ancestors()
            .find_map(|ancestor| self.workspaces.get(ancestor))
    }

    fn absolute_path(&self, path: &Path) -> PathBuf {
        let absolute = if path.is_absolute() {
            if let Ok(relative) = path.strip_prefix(&self.authored_root) {
                self.root.join(relative)
            } else {
                path.to_path_buf()
            }
        } else {
            self.root.join(path)
        };
        dunce::simplified(&absolute).to_path_buf()
    }
}

impl ChangedPathScope for PackageChangeScope {
    fn contains(&self, path: &Path) -> bool {
        self.includes(path)
    }
}

fn canonical_root(path: &Path) -> Result<PathBuf, PackageBaselineError> {
    dunce::canonicalize(path).map_err(|err| PackageBaselineError::UnavailableRoot {
        path: path.to_path_buf(),
        message: err.to_string(),
    })
}

/// The project-relative, slash-separated root of a discovered workspace.
fn workspace_key(workspace_root: &Path, authored_root: &Path, root: &Path) -> Option<String> {
    let workspace_root = dunce::simplified(workspace_root);
    let relative = workspace_root
        .strip_prefix(authored_root)
        .or_else(|_| workspace_root.strip_prefix(root))
        .ok()?;
    let key = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    (!key.is_empty()).then_some(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    use fallow_types::output_dead_code::{UnresolvedCatalogReferenceFinding, UnusedFileFinding};
    use fallow_types::results::{AnalysisResults, UnresolvedCatalogReference, UnusedFile};

    fn git(root: &Path, args: &[&str]) {
        let mut command = Command::new("git");
        crate::changed_files::clear_ambient_git_env(&mut command);
        let output = command
            .args(args)
            .current_dir(root)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn workspace(root: &Path, relative: &str) -> WorkspaceInfo {
        WorkspaceInfo {
            root: root.join(relative),
            name: relative.to_owned(),
            is_internal_dependency: false,
        }
    }

    fn nested_repo() -> tempfile::TempDir {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        let parent = "packages/parent";
        let child = "packages/parent/packages/child";
        let other = "packages/other";
        for package in [parent, child, other] {
            fs::create_dir_all(root.join(package)).expect("package directory");
            fs::write(
                root.join(package).join("index.ts"),
                "export const value = 1;",
            )
            .expect("source");
            fs::write(root.join(package).join("package.json"), "{}").expect("manifest");
        }
        git(root, &["init", "-q"]);
        git(root, &["add", "."]);
        git(
            root,
            &[
                "-c",
                "user.name=Fallow Test",
                "-c",
                "user.email=fallow@example.test",
                "commit",
                "-qm",
                "base",
            ],
        );
        git(root, &["branch", "base"]);
        fs::write(root.join(child).join("index.ts"), "export const value = 2;")
            .expect("child change");
        git(root, &["add", "."]);
        git(
            root,
            &[
                "-c",
                "user.name=Fallow Test",
                "-c",
                "user.email=fallow@example.test",
                "commit",
                "-qm",
                "child change",
            ],
        );
        fs::write(
            root.join(parent).join("index.ts"),
            "export const value = 3;",
        )
        .expect("parent change");

        temp
    }

    #[test]
    fn nested_refs_scope_source_and_manifest_owners() {
        let temp = nested_repo();
        let root = temp.path();
        let parent = "packages/parent";
        let child = "packages/parent/packages/child";
        let other = "packages/other";
        let workspaces = [
            workspace(root, parent),
            workspace(root, child),
            workspace(root, other),
        ];
        let configured = BTreeMap::from([
            (parent.to_owned(), "base".to_owned()),
            (child.to_owned(), "HEAD".to_owned()),
        ]);
        let scope = PackageChangeScope::resolve(root, &configured, &workspaces)
            .expect("valid refs")
            .expect("package scope");
        assert!(scope.includes(&root.join(parent).join("index.ts")));
        assert!(!scope.includes(&root.join(child).join("index.ts")));
        assert!(scope.includes(&root.join(other).join("package.json")));
        assert!(scope.includes(&root.join("root.ts")));
        assert_eq!(
            scope.baseline_for(&root.join(child).join("index.ts")),
            Some("HEAD")
        );
        assert_eq!(scope.baseline_for(&root.join(other).join("index.ts")), None);
        let parent_root = canonical_root(&root.join(parent)).expect("canonical package");
        let Some(WorkspaceBaseline::Changed { files, .. }) = scope.workspaces.get(&parent_root)
        else {
            panic!("parent baseline missing");
        };
        assert_eq!(files.len(), 1, "retain only parent-owned changed paths");
        assert!(files.contains(&parent_root.join("index.ts")));

        let mut results = AnalysisResults::default();
        for package in [parent, child] {
            results
                .unused_files
                .push(UnusedFileFinding::with_actions(UnusedFile {
                    path: root.join(package).join("index.ts"),
                }));
        }
        results.unresolved_catalog_references.push(
            UnresolvedCatalogReferenceFinding::with_actions(UnresolvedCatalogReference {
                entry_name: "react".to_owned(),
                catalog_name: "default".to_owned(),
                path: root.join(other).join("package.json"),
                line: 1,
                available_in_catalogs: Vec::new(),
            }),
        );
        crate::changed_files::filter_results_by_path_scope(&mut results, &scope);
        assert_eq!(results.unused_files.len(), 1);
        assert_eq!(
            results.unused_files[0].file.path,
            root.join(parent).join("index.ts")
        );
        assert_eq!(results.unresolved_catalog_references.len(), 1);
    }

    #[test]
    fn invalid_ref_and_clean_packages_have_explicit_scope() {
        let temp = nested_repo();
        let root = temp.path();
        let parent = "packages/parent";
        let child = "packages/parent/packages/child";
        let other = "packages/other";
        let workspaces = [
            workspace(root, parent),
            workspace(root, child),
            workspace(root, other),
        ];
        let invalid_ref = BTreeMap::from([(parent.to_owned(), "missing-ref".to_owned())]);
        assert!(matches!(
            PackageChangeScope::resolve(root, &invalid_ref, &workspaces),
            Err(PackageBaselineError::Git { .. })
        ));

        git(root, &["add", "."]);
        git(
            root,
            &[
                "-c",
                "user.name=Fallow Test",
                "-c",
                "user.email=fallow@example.test",
                "commit",
                "-qm",
                "parent change",
            ],
        );
        let all_mapped = BTreeMap::from([
            (parent.to_owned(), "HEAD".to_owned()),
            (child.to_owned(), "HEAD".to_owned()),
            (other.to_owned(), "HEAD".to_owned()),
        ]);
        let clean = PackageChangeScope::resolve(root, &all_mapped, &workspaces)
            .expect("valid HEAD")
            .expect("package scope");
        assert!(!clean.includes(&root.join(parent).join("index.ts")));
        assert!(!clean.includes(&root.join(child).join("index.ts")));
        assert!(!clean.includes(&root.join(other).join("package.json")));
        assert!(clean.includes(&root.join("root.ts")));
    }

    #[test]
    fn invalid_mapping_fails_before_scope_is_applied() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        fs::create_dir_all(root.join("packages/app")).expect("package directory");
        let workspaces = [workspace(root, "packages/app")];
        let invalid_key = BTreeMap::from([("packages/../app".to_owned(), "HEAD".to_owned())]);
        assert!(matches!(
            PackageChangeScope::resolve(root, &invalid_key, &workspaces),
            Err(PackageBaselineError::InvalidWorkspaceKey { .. })
        ));
        let unknown = BTreeMap::from([("packages/missing".to_owned(), "HEAD".to_owned())]);
        assert!(matches!(
            PackageChangeScope::resolve(root, &unknown, &workspaces),
            Err(PackageBaselineError::UnknownWorkspace { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_alias_is_not_an_exact_workspace_root() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        fs::create_dir_all(root.join("packages/app")).expect("package directory");
        std::os::unix::fs::symlink(root.join("packages/app"), root.join("alias"))
            .expect("workspace alias");
        let configured = BTreeMap::from([("alias".to_owned(), "HEAD".to_owned())]);
        assert!(matches!(
            PackageChangeScope::resolve(root, &configured, &[workspace(root, "packages/app")]),
            Err(PackageBaselineError::UnknownWorkspace { .. })
        ));
    }

    /// Discovery reports a symlinked workspace under its link path, and
    /// `fallow list --workspaces` prints that path. The map accepts it.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_workspace_is_mapped_under_its_discovered_root() {
        let temp = nested_repo();
        let root = temp.path();
        fs::create_dir_all(root.join("external/linked")).expect("link target");
        fs::write(root.join("external/linked/index.ts"), "export const x = 1;").expect("source");
        std::os::unix::fs::symlink(root.join("external/linked"), root.join("packages/linked"))
            .expect("workspace link");
        let workspaces = [workspace(root, "packages/linked")];
        let configured = BTreeMap::from([("packages/linked".to_owned(), "HEAD".to_owned())]);
        let scope = PackageChangeScope::resolve(root, &configured, &workspaces)
            .expect("the discovered root is a valid key")
            .expect("package scope");
        assert_eq!(
            scope.baseline_for(&root.join("external/linked/index.ts")),
            Some("HEAD")
        );
        assert!(scope.includes(&root.join("external/linked/index.ts")));
        let rows =
            crate::change_scope::package_baseline_statuses(std::slice::from_ref(&scope), root);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].workspace_root, "packages/linked");
    }

    #[test]
    fn an_unknown_key_suggests_the_closest_workspace_root() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        fs::create_dir_all(root.join("packages/web")).expect("package directory");
        let configured = BTreeMap::from([("packages/wbe".to_owned(), "HEAD".to_owned())]);
        let Err(PackageBaselineError::UnknownWorkspace { suggestion, .. }) =
            PackageChangeScope::resolve(root, &configured, &[workspace(root, "packages/web")])
        else {
            panic!("an unknown key must fail to resolve");
        };
        assert_eq!(suggestion.as_deref(), Some("packages/web"));
    }
}
