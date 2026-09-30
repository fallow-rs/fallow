//! The Git change scope of one analysis run.
//!
//! A run narrows its findings by at most one change scope: the changed files
//! of one global ref, or the per-workspace refs of `workspaces.changedSince`.
//! Every surface describes its inputs with a [`ChangeScopeRequest`] and calls
//! [`ChangeScope::resolve`], which owns the precedence rule and the failure
//! policy. The resolved value owns the four things that must agree: the
//! result filter, the `package_baselines` provenance rows, the
//! `package-baselines` request outcome, and the scope flag that baseline
//! comparison and finding-id queries read. A surface therefore cannot apply
//! the filter and forget the flag, or read the package map where the caller
//! owns the scope.
//!
//! The failure policy follows `--changed-since`. A malformed key or ref is
//! invalid input, and resolution fails. A map that cannot apply as written
//! stands down as a whole: a key that names no workspace of this project (for
//! example in a run from a package subdirectory), or a well-formed ref that
//! Git cannot resolve (for example in a shallow CI clone). The run then
//! reports in full scope and publishes a `not-applied` request outcome that
//! says so. The report is wider than asked, never narrower.
//!
//! The filter applies to the final result of the run. A surface that narrows
//! findings before type-aware refinement, to reduce sidecar work, applies the
//! same scope again after refinement, because refinement can add findings.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use fallow_config::{ResolvedConfig, WorkspaceInfo};
use fallow_output::{PackageBaselineStatus, RequestName, RequestOutcome};
use fallow_types::duplicates::DuplicationReport;
use fallow_types::results::AnalysisResults;
use rustc_hash::FxHashSet;

use crate::changed_files::{ChangedFilesError, NormalizedChangedFiles};
use crate::package_baselines::{PackageBaselineError, PackageChangeScope};

/// The `requested` value of the `package-baselines` request outcome.
const PACKAGE_BASELINES_REQUEST: &str = "workspaces.changedSince";

/// Who owns the change scope of a run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChangeScopeOwner {
    /// The run owns it: a global ref when one is requested, otherwise the
    /// configured package baselines.
    #[default]
    Run,
    /// The calling pipeline narrows the findings itself. `audit` is the
    /// example: it compares a head run and a base run against its own changed
    /// files. The run reads no package baselines, so it never resolves Git
    /// refs in a base snapshot and never hides a finding that the comparison
    /// needs.
    Caller,
}

/// The change-scope inputs of one run.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChangeScopeRequest<'a> {
    /// Who owns the scope.
    pub owner: ChangeScopeOwner,
    /// A global changed-since ref was requested. This is `true` also when the
    /// ref did not resolve: the run then reports in full scope, and the
    /// package baselines still do not apply.
    pub global_ref: bool,
    /// The changed files of the global ref, or a changed-file set that the
    /// caller supplied.
    pub files: Option<&'a FxHashSet<PathBuf>>,
    /// The run-wide memo of the resolved package map. The analyses of one run
    /// share it, so Git resolves each mapped ref once per run. `None`
    /// resolves the map without a memo.
    pub cache: Option<&'a PackageBaselineCache>,
    /// `--no-package-baselines`: this run ignores `workspaces.changedSince`
    /// and reports every package in full scope, for example to save or gate a
    /// whole-project baseline.
    pub no_package_baselines: bool,
}

impl ChangeScopeRequest<'_> {
    /// Whether [`ChangeScope::resolve`] reads `workspaces.changedSince` for
    /// this request. A surface that discovers workspaces only for the package
    /// map checks this first.
    #[must_use]
    pub fn reads_package_baselines(&self, config: &ResolvedConfig) -> bool {
        self.owner == ChangeScopeOwner::Run
            && !self.no_package_baselines
            && !self.global_ref
            && self.files.is_none()
            && !config.workspace_changed_since.is_empty()
    }
}

/// The package map of one run, resolved once.
#[derive(Debug, Clone)]
struct ResolvedPackages {
    packages: Option<PackageChangeScope>,
    outcome: RequestOutcome,
}

/// The run-wide memo of the resolved package map.
///
/// Every analysis of one run resolves the same map against the same
/// workspaces, so each surface keeps one memo per run: a process-wide value on
/// the CLI, a field of the programmatic call context, and one value per
/// project and run in the editor. The first resolution wins. A config error is
/// kept too, so every analysis of the run fails the same way.
#[derive(Debug, Default)]
pub struct PackageBaselineCache(OnceLock<Result<ResolvedPackages, PackageBaselineError>>);

impl PackageBaselineCache {
    /// An empty memo.
    #[must_use]
    pub const fn new() -> Self {
        Self(OnceLock::new())
    }

    /// Whether an analysis of the run already resolved the map.
    #[must_use]
    pub fn is_resolved(&self) -> bool {
        self.0.get().is_some()
    }

    /// The `package-baselines` request outcome of the run, once a run
    /// resolved the map. `None` when no analysis read the map.
    #[must_use]
    pub fn request_outcome(&self) -> Option<RequestOutcome> {
        match self.0.get()? {
            Ok(resolved) => Some(resolved.outcome.clone()),
            Err(_) => None,
        }
    }

    fn resolve(
        &self,
        config: &ResolvedConfig,
        workspaces: &[WorkspaceInfo],
    ) -> Result<ResolvedPackages, PackageBaselineError> {
        self.0
            .get_or_init(|| resolve_packages(config, workspaces))
            .clone()
    }
}

fn resolve_packages(
    config: &ResolvedConfig,
    workspaces: &[WorkspaceInfo],
) -> Result<ResolvedPackages, PackageBaselineError> {
    match PackageChangeScope::resolve(&config.root, &config.workspace_changed_since, workspaces) {
        Ok(packages) => Ok(ResolvedPackages {
            packages,
            outcome: RequestOutcome::applied(
                RequestName::PackageBaselines,
                PACKAGE_BASELINES_REQUEST,
            ),
        }),
        Err(PackageBaselineError::Git {
            key,
            reference,
            source,
        }) if !matches!(source, ChangedFilesError::InvalidRef(_)) => {
            let cause = source
                .describe()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            Ok(stood_down(
                source.reason(),
                &format!("the ref '{reference}' for '{key}' did not resolve ({cause})"),
                "Fetch the ref with full history, or map the package to a ref Git can resolve.",
            ))
        }
        Err(PackageBaselineError::UnknownWorkspace { key, suggestion }) => {
            let hint =
                suggestion.map_or_else(String::new, |root| format!(" (did you mean '{root}'?)"));
            Ok(stood_down(
                "unknown-workspace",
                &format!("'{key}' names no workspace package of this project{hint}"),
                "Use a workspace root as `fallow list --workspaces` prints it.",
            ))
        }
        Err(err) => Err(err),
    }
}

/// A package map that stood down: the run reports in full scope, and the
/// outcome carries the sentence that the CLI also writes to stderr.
fn stood_down(reason: &str, cause: &str, remedy: &str) -> ResolvedPackages {
    ResolvedPackages {
        packages: None,
        outcome: RequestOutcome::not_applied(
            RequestName::PackageBaselines,
            PACKAGE_BASELINES_REQUEST,
            reason,
            format!(
                "workspaces.changedSince was ignored because {cause}, so this report covers \
                 every workspace package in full scope. {remedy}"
            ),
        ),
    }
}

/// The resolved change scope of one run.
#[derive(Debug, Clone, Default)]
pub struct ChangeScope {
    kind: ChangeScopeKind,
    global_ref: bool,
    outcome: Option<RequestOutcome>,
}

#[derive(Debug, Clone, Default)]
enum ChangeScopeKind {
    #[default]
    Full,
    Files(NormalizedChangedFiles),
    Packages(PackageChangeScope),
}

impl ChangeScope {
    /// Resolve the change scope of a run.
    ///
    /// A changed-file set wins. A requested global ref without files, or a
    /// caller-owned scope, gives the full scope. Otherwise the configured
    /// package baselines apply, when the config has any. A mapped ref that
    /// Git cannot resolve stands the map down to the full scope.
    ///
    /// # Errors
    ///
    /// Returns an error when the package map has a malformed key or Git ref,
    /// or when a workspace root cannot be read.
    pub fn resolve(
        request: ChangeScopeRequest<'_>,
        config: &ResolvedConfig,
        workspaces: &[WorkspaceInfo],
    ) -> Result<Self, PackageBaselineError> {
        let global_ref = request.global_ref || request.files.is_some();
        if let Some(files) = request.files {
            return Ok(Self::changed_files(files));
        }
        if !request.reads_package_baselines(config) {
            return Ok(Self {
                global_ref,
                ..Self::default()
            });
        }
        let resolved = match request.cache {
            Some(cache) => cache.resolve(config, workspaces)?,
            None => resolve_packages(config, workspaces)?,
        };
        Ok(Self {
            kind: resolved
                .packages
                .map_or(ChangeScopeKind::Full, ChangeScopeKind::Packages),
            global_ref,
            outcome: Some(resolved.outcome),
        })
    }

    /// The scope of one global changed-file set.
    #[must_use]
    pub fn changed_files(files: &FxHashSet<PathBuf>) -> Self {
        Self {
            kind: ChangeScopeKind::Files(NormalizedChangedFiles::new(files)),
            global_ref: true,
            outcome: None,
        }
    }

    /// The `scope_reasons` channel that narrowed the run, when a change ref
    /// did: the package map, or a global ref. The saved-baseline comparison of
    /// `check` and finding-id queries read this: a finding outside the scope
    /// is hidden, not gone. `dupes` compares its baseline with the report
    /// before the package map narrows it, so it reads only the global ref.
    #[must_use]
    pub const fn scope_reason(&self) -> Option<fallow_output::ScopeReason> {
        match (&self.kind, self.global_ref) {
            (ChangeScopeKind::Packages(_), _) => Some(fallow_output::ScopeReason::PackageBaselines),
            (_, true) => Some(fallow_output::ScopeReason::ChangedSince),
            (_, false) => None,
        }
    }

    /// The applied package baselines, when the configured map scopes the run.
    #[must_use]
    pub const fn packages(&self) -> Option<&PackageChangeScope> {
        match &self.kind {
            ChangeScopeKind::Packages(packages) => Some(packages),
            ChangeScopeKind::Full | ChangeScopeKind::Files(_) => None,
        }
    }

    /// The `package-baselines` request outcome, when the run read the map.
    #[must_use]
    pub const fn request_outcome(&self) -> Option<&RequestOutcome> {
        self.outcome.as_ref()
    }

    /// The sentence to show when the package map stood down, or `None` when
    /// the map applied or the run did not read it.
    #[must_use]
    pub fn stand_down_message(&self) -> Option<&str> {
        self.outcome
            .as_ref()
            .filter(|outcome| outcome.status != fallow_output::RequestStatus::Applied)
            .and_then(|outcome| outcome.message.as_deref())
    }

    /// The `package_baselines` provenance rows of the run. Empty unless the
    /// configured map scopes the run.
    #[must_use]
    pub fn package_baselines(&self) -> Vec<PackageBaselineStatus> {
        self.packages().map_or_else(Vec::new, |packages| {
            package_baseline_statuses(std::slice::from_ref(packages), packages.project_root())
        })
    }

    /// Whether a finding owned by `path` is in scope.
    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        use crate::changed_files::ChangedPathScope as _;
        match &self.kind {
            ChangeScopeKind::Full => true,
            ChangeScopeKind::Files(files) => files.contains(path),
            ChangeScopeKind::Packages(packages) => packages.contains(path),
        }
    }

    /// Keep the dead-code findings in scope.
    pub(crate) fn retain_dead_code(&self, results: &mut AnalysisResults) {
        match &self.kind {
            ChangeScopeKind::Full => {}
            ChangeScopeKind::Files(files) => {
                crate::changed_files::filter_results_by_path_scope(results, files);
            }
            ChangeScopeKind::Packages(packages) => {
                crate::changed_files::filter_results_by_path_scope(results, packages);
            }
        }
    }

    /// Keep the clone groups with at least one instance in scope.
    pub(crate) fn retain_duplication(&self, report: &mut DuplicationReport, root: &Path) {
        match &self.kind {
            ChangeScopeKind::Full => {}
            ChangeScopeKind::Files(files) => {
                crate::changed_files::filter_duplication_by_path_scope(report, files, root);
            }
            ChangeScopeKind::Packages(packages) => {
                crate::changed_files::filter_duplication_by_path_scope(report, packages, root);
            }
        }
    }
}

/// The `package_baselines` rows of a combined report: the rows of the first
/// section that applied the map. The sections of one run resolve the same map,
/// so a section without rows either did not run or used a global scope.
#[must_use]
pub fn first_package_baselines<'a>(
    sections: impl IntoIterator<Item = Option<&'a [PackageBaselineStatus]>>,
) -> Vec<PackageBaselineStatus> {
    sections
        .into_iter()
        .flatten()
        .find(|rows| !rows.is_empty())
        .map_or_else(Vec::new, <[PackageBaselineStatus]>::to_vec)
}

/// Project-relative package baseline rows, sorted by workspace root.
///
/// An editor that analyzes several project roots passes every applied scope
/// and its own root, so the rows of all projects share one path base.
#[must_use]
pub fn package_baseline_statuses(
    scopes: &[PackageChangeScope],
    root: &Path,
) -> Vec<PackageBaselineStatus> {
    let root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut rows = scopes
        .iter()
        .flat_map(|scope| {
            let prefix = scope
                .project_root()
                .strip_prefix(&root)
                .map(|relative| {
                    relative
                        .components()
                        .map(|component| component.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/")
                })
                .unwrap_or_default();
            scope
                .configured_baselines()
                .map(move |(key, reference)| PackageBaselineStatus {
                    workspace_root: if prefix.is_empty() {
                        key.to_owned()
                    } else {
                        format!("{prefix}/{key}")
                    },
                    reference: reference.to_owned(),
                })
        })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| a.workspace_root.cmp(&b.workspace_root));
    rows
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use fallow_config::{FallowConfig, OutputFormat};

    use super::*;

    fn config_with_map(root: &Path, map: &[(&str, &str)]) -> ResolvedConfig {
        let mut config = FallowConfig::default().resolve(
            root.to_path_buf(),
            OutputFormat::Json,
            1,
            true,
            true,
            None,
        );
        config.workspace_changed_since = map
            .iter()
            .map(|(key, reference)| ((*key).to_owned(), (*reference).to_owned()))
            .collect::<BTreeMap<_, _>>();
        config
    }

    /// A map that names no discovered workspace fails resolution. The tests
    /// below use it to prove that a request never reads the map.
    fn unknown_workspace_map(root: &Path) -> ResolvedConfig {
        config_with_map(root, &[("packages/missing", "HEAD")])
    }

    #[test]
    fn caller_owned_scope_never_reads_the_package_map() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config = unknown_workspace_map(temp.path());
        let request = ChangeScopeRequest {
            owner: ChangeScopeOwner::Caller,
            ..ChangeScopeRequest::default()
        };
        assert!(!request.reads_package_baselines(&config));
        let scope = ChangeScope::resolve(request, &config, &[]).expect("caller-owned scope");
        assert!(scope.scope_reason().is_none());
        assert!(scope.package_baselines().is_empty());
        assert!(scope.contains(&temp.path().join("packages/a/index.ts")));
    }

    #[test]
    fn a_requested_global_ref_suppresses_the_map_even_without_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config = unknown_workspace_map(temp.path());
        let request = ChangeScopeRequest {
            global_ref: true,
            ..ChangeScopeRequest::default()
        };
        let scope = ChangeScope::resolve(request, &config, &[]).expect("global scope");
        assert!(scope.scope_reason().is_some());
        assert!(scope.packages().is_none());
    }

    #[test]
    fn a_changed_file_set_wins_over_the_map() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config = unknown_workspace_map(temp.path());
        let changed: FxHashSet<PathBuf> = std::iter::once(temp.path().join("a.ts")).collect();
        let request = ChangeScopeRequest {
            files: Some(&changed),
            ..ChangeScopeRequest::default()
        };
        let scope = ChangeScope::resolve(request, &config, &[]).expect("file scope");
        assert!(scope.scope_reason().is_some());
        assert!(scope.contains(&temp.path().join("a.ts")));
        assert!(!scope.contains(&temp.path().join("b.ts")));
    }

    /// A map that cannot apply as written stands down as a whole. The report
    /// is then wider than asked, and the outcome says so.
    #[test]
    fn a_key_that_names_no_workspace_stands_the_map_down() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(temp.path().join("packages/web")).expect("package");
        let config = config_with_map(temp.path(), &[("packages/wbe", "HEAD")]);
        let workspaces = [WorkspaceInfo {
            root: temp.path().join("packages/web"),
            name: "web".to_owned(),
            is_internal_dependency: false,
        }];
        let scope = ChangeScope::resolve(ChangeScopeRequest::default(), &config, &workspaces)
            .expect("an unknown key stands down");
        assert!(scope.scope_reason().is_none());
        assert!(scope.packages().is_none());
        assert!(scope.package_baselines().is_empty());
        let outcome = scope.request_outcome().expect("the map was read");
        assert_eq!(outcome.status, fallow_output::RequestStatus::NotApplied);
        assert_eq!(outcome.reason.as_deref(), Some("unknown-workspace"));
        let message = scope.stand_down_message().expect("a stand-down sentence");
        assert!(
            message.contains("did you mean 'packages/web'?"),
            "{message}"
        );
    }

    #[test]
    fn a_malformed_key_or_ref_is_invalid_input() {
        let temp = tempfile::tempdir().expect("tempdir");
        for map in [
            [("./packages/web", "HEAD")],
            [("packages/web", "-malformed")],
        ] {
            std::fs::create_dir_all(temp.path().join("packages/web")).expect("package");
            let config = config_with_map(temp.path(), &map);
            let workspaces = [WorkspaceInfo {
                root: temp.path().join("packages/web"),
                name: "web".to_owned(),
                is_internal_dependency: false,
            }];
            assert!(
                ChangeScope::resolve(ChangeScopeRequest::default(), &config, &workspaces).is_err(),
                "{map:?}"
            );
        }
    }

    #[test]
    fn the_run_memo_resolves_the_map_once() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config = unknown_workspace_map(temp.path());
        let cache = PackageBaselineCache::new();
        assert!(!cache.is_resolved());
        assert!(cache.request_outcome().is_none());
        let request = ChangeScopeRequest {
            cache: Some(&cache),
            ..ChangeScopeRequest::default()
        };
        ChangeScope::resolve(request, &config, &[]).expect("first resolution");
        assert!(cache.is_resolved());
        let later_config = config_with_map(temp.path(), &[]);
        let later = ChangeScope::resolve(
            ChangeScopeRequest {
                cache: Some(&cache),
                ..ChangeScopeRequest::default()
            },
            &config,
            &[],
        )
        .expect("memo hit");
        assert_eq!(later.request_outcome(), cache.request_outcome().as_ref());
        let empty = ChangeScope::resolve(ChangeScopeRequest::default(), &later_config, &[])
            .expect("no map, full scope");
        assert!(empty.scope_reason().is_none());
        assert!(empty.request_outcome().is_none());
    }
}
