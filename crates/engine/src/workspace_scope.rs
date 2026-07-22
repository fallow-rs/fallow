//! Workspace scoping owned by the engine boundary.

use std::path::{Path, PathBuf};

use fallow_config::WorkspaceInfo;
use globset::Glob;
use rustc_hash::FxHashSet;

/// User-facing workspace scope mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceScopeMode {
    /// Explicit workspace package names, paths, or globs.
    Workspace,
    /// Git-derived changed workspace scope.
    ChangedWorkspaces,
}

/// Typed workspace-scope failure. Surfaces decide their own wording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceScopeError {
    /// No workspace metadata exists for the requested scope.
    NoWorkspaces {
        mode: WorkspaceScopeMode,
        patterns: Vec<String>,
        git_ref: Option<String>,
    },
    /// A pattern was neither an exact name/path nor a valid glob.
    InvalidPattern { pattern: String, message: String },
    /// One or more positive patterns matched no workspace.
    UnmatchedPatterns {
        patterns: Vec<String>,
        available: String,
    },
    /// Negation removed every selected workspace.
    EmptyAfterExclusions { included: String, excluded: String },
    /// Git failed while resolving changed workspaces.
    ChangedWorkspacesFailed { git_ref: String, message: String },
    /// Both workspace scope modes were requested.
    MutuallyExclusive,
}

/// Resolve either explicit or changed workspace scope against discovered metadata.
///
/// # Errors
///
/// Returns a typed scope error when the selection is invalid or git cannot
/// resolve changed files.
pub fn resolve_workspace_scope_roots(
    root: &Path,
    workspace: Option<&[String]>,
    changed_workspaces: Option<&str>,
    workspaces: &[WorkspaceInfo],
) -> Result<Option<Vec<PathBuf>>, WorkspaceScopeError> {
    match (workspace, changed_workspaces) {
        (Some(patterns), None) => {
            resolve_workspace_filter_roots(root, patterns, workspaces).map(Some)
        }
        (None, Some(git_ref)) => {
            resolve_changed_workspace_roots(root, git_ref, workspaces).map(Some)
        }
        (None, None) => Ok(None),
        (Some(_), Some(_)) => Err(WorkspaceScopeError::MutuallyExclusive),
    }
}

/// Resolve either explicit or changed workspace scope by discovering workspace
/// metadata first.
///
/// # Errors
///
/// Returns a typed scope error when the selection is invalid, no workspaces are
/// available, or git cannot resolve changed files.
pub fn resolve_workspace_scope_roots_for_project(
    root: &Path,
    workspace: Option<&[String]>,
    changed_workspaces: Option<&str>,
) -> Result<Option<Vec<PathBuf>>, WorkspaceScopeError> {
    let workspaces = crate::discover::discover_workspace_packages(root);
    resolve_workspace_scope_roots(root, workspace, changed_workspaces, &workspaces)
}

/// Resolve explicit workspace filters by discovering workspace metadata first.
///
/// # Errors
///
/// Returns a typed scope error when no workspaces are available or the filter
/// cannot select a non-empty set.
pub fn resolve_workspace_filter_roots_for_project(
    root: &Path,
    patterns: &[String],
) -> Result<Vec<PathBuf>, WorkspaceScopeError> {
    let workspaces = crate::discover::discover_workspace_packages(root);
    resolve_workspace_filter_roots(root, patterns, &workspaces)
}

/// Resolve explicit workspace filters against known workspace metadata.
///
/// # Errors
///
/// Returns a typed scope error when no workspaces are available, a pattern is
/// invalid, a positive pattern is unmatched, or negation excludes everything.
fn resolve_workspace_filter_roots(
    root: &Path,
    patterns: &[String],
    workspaces: &[WorkspaceInfo],
) -> Result<Vec<PathBuf>, WorkspaceScopeError> {
    if workspaces.is_empty() {
        return Err(WorkspaceScopeError::NoWorkspaces {
            mode: WorkspaceScopeMode::Workspace,
            patterns: patterns.to_vec(),
            git_ref: None,
        });
    }

    let rel_paths = workspace_relative_paths(root, workspaces);
    let (positive, negative) = split_workspace_patterns(patterns);
    let mut matched = match_positive_workspace_patterns(&positive, workspaces, &rel_paths)?;

    for pattern in &negative {
        for index in find_workspace_matches(pattern, workspaces, &rel_paths)? {
            matched.remove(&index);
        }
    }

    if matched.is_empty() {
        return Err(WorkspaceScopeError::EmptyAfterExclusions {
            included: describe_included_patterns(&positive),
            excluded: describe_excluded_patterns(&negative),
        });
    }

    let mut roots = matched
        .into_iter()
        .map(|index| workspaces[index].root.clone())
        .collect::<Vec<_>>();
    roots.sort();
    Ok(roots)
}

/// Resolve changed workspace roots by discovering workspace metadata first.
///
/// # Errors
///
/// Returns a typed scope error when no workspaces are available or git fails.
pub fn resolve_changed_workspace_roots_for_project(
    root: &Path,
    git_ref: &str,
) -> Result<Vec<PathBuf>, WorkspaceScopeError> {
    let workspaces = crate::discover::discover_workspace_packages(root);
    resolve_changed_workspace_roots(root, git_ref, &workspaces)
}

/// Resolve workspace roots that contain files changed since `git_ref`.
///
/// # Errors
///
/// Returns a typed scope error when no workspaces are available or git fails.
fn resolve_changed_workspace_roots(
    root: &Path,
    git_ref: &str,
    workspaces: &[WorkspaceInfo],
) -> Result<Vec<PathBuf>, WorkspaceScopeError> {
    if workspaces.is_empty() {
        return Err(WorkspaceScopeError::NoWorkspaces {
            mode: WorkspaceScopeMode::ChangedWorkspaces,
            patterns: Vec::new(),
            git_ref: Some(git_ref.to_owned()),
        });
    }

    let changed_files = crate::changed_files::changed_files(root, git_ref).map_err(|err| {
        WorkspaceScopeError::ChangedWorkspacesFailed {
            git_ref: git_ref.to_owned(),
            message: err.describe(),
        }
    })?;
    let mut roots = workspaces
        .iter()
        .filter(|workspace| {
            changed_files
                .iter()
                .any(|file| file.starts_with(&workspace.root))
        })
        .map(|workspace| workspace.root.clone())
        .collect::<Vec<_>>();
    roots.sort();
    Ok(roots)
}

fn match_positive_workspace_patterns(
    positive: &[&str],
    workspaces: &[WorkspaceInfo],
    rel_paths: &[String],
) -> Result<FxHashSet<usize>, WorkspaceScopeError> {
    let mut matched = FxHashSet::default();
    let mut unmatched = Vec::new();

    if positive.is_empty() {
        matched.extend(0..workspaces.len());
    } else {
        for pattern in positive {
            let hits = find_workspace_matches(pattern, workspaces, rel_paths)?;
            if hits.is_empty() {
                unmatched.push((*pattern).to_owned());
            }
            matched.extend(hits);
        }
    }

    if !unmatched.is_empty() {
        return Err(WorkspaceScopeError::UnmatchedPatterns {
            patterns: unmatched,
            available: format_available_workspaces(workspaces),
        });
    }

    Ok(matched)
}

fn find_workspace_matches(
    pattern: &str,
    workspaces: &[WorkspaceInfo],
    rel_paths: &[String],
) -> Result<Vec<usize>, WorkspaceScopeError> {
    if let Some(index) = workspaces
        .iter()
        .position(|workspace| workspace.name == pattern)
    {
        return Ok(vec![index]);
    }
    if let Some(index) = rel_paths.iter().position(|path| path == pattern) {
        return Ok(vec![index]);
    }

    let glob = Glob::new(pattern).map_err(|err| WorkspaceScopeError::InvalidPattern {
        pattern: pattern.to_owned(),
        message: err.to_string(),
    })?;
    let matcher = glob.compile_matcher();
    Ok(workspaces
        .iter()
        .enumerate()
        .filter_map(|(index, workspace)| {
            (matcher.is_match(&workspace.name) || matcher.is_match(&rel_paths[index]))
                .then_some(index)
        })
        .collect())
}

fn split_workspace_patterns(patterns: &[String]) -> (Vec<&str>, Vec<&str>) {
    let mut positive = Vec::new();
    let mut negative = Vec::new();
    for pattern in patterns {
        let trimmed = pattern.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(negative_pattern) = trimmed.strip_prefix('!') {
            let negative_pattern = negative_pattern.trim();
            if !negative_pattern.is_empty() {
                negative.push(negative_pattern);
            }
        } else {
            positive.push(trimmed);
        }
    }
    (positive, negative)
}

fn workspace_relative_paths(root: &Path, workspaces: &[WorkspaceInfo]) -> Vec<String> {
    workspaces
        .iter()
        .map(|workspace| relative_workspace_path(&workspace.root, root))
        .collect()
}

fn relative_workspace_path(workspace_root: &Path, root: &Path) -> String {
    workspace_root
        .strip_prefix(root)
        .unwrap_or(workspace_root)
        .to_string_lossy()
        .replace('\\', "/")
}

fn describe_included_patterns(positive: &[&str]) -> String {
    if positive.is_empty() {
        "<all>".to_owned()
    } else {
        quote_patterns(positive)
    }
}

fn describe_excluded_patterns(negative: &[&str]) -> String {
    quote_patterns(negative)
}

fn quote_patterns(patterns: &[&str]) -> String {
    patterns
        .iter()
        .map(|pattern| format!("'{pattern}'"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_available_workspaces(workspaces: &[WorkspaceInfo]) -> String {
    const MAX_SHOWN: usize = 10;
    let total = workspaces.len();
    if total <= MAX_SHOWN {
        return workspaces
            .iter()
            .map(|workspace| workspace.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
    }
    let shown = workspaces
        .iter()
        .take(MAX_SHOWN)
        .map(|workspace| workspace.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{shown}, ... and {} more ({total} total)",
        total - MAX_SHOWN
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(name: &str, rel: &str) -> WorkspaceInfo {
        WorkspaceInfo {
            root: PathBuf::from("/project").join(rel),
            name: name.to_owned(),
            is_internal_dependency: false,
        }
    }

    #[test]
    fn workspace_filter_exact_name_short_circuits_glob_metachars() {
        let workspaces = vec![ws("web-[staging]", "apps/web-staging")];
        let roots = resolve_workspace_filter_roots(
            Path::new("/project"),
            &["web-[staging]".to_owned()],
            &workspaces,
        )
        .expect("resolve workspace");

        assert_eq!(roots, vec![PathBuf::from("/project/apps/web-staging")]);
    }

    #[test]
    fn workspace_filter_globs_against_name_and_path() {
        let workspaces = vec![
            ws("@scope/ui", "packages/ui"),
            ws("admin", "apps/admin"),
            ws("web", "apps/web"),
        ];
        let roots = resolve_workspace_filter_roots(
            Path::new("/project"),
            &["apps/*".to_owned()],
            &workspaces,
        )
        .expect("resolve workspace");

        assert_eq!(
            roots,
            vec![
                PathBuf::from("/project/apps/admin"),
                PathBuf::from("/project/apps/web")
            ]
        );

        let roots = resolve_workspace_filter_roots(
            Path::new("/project"),
            &["@scope/*".to_owned()],
            &workspaces,
        )
        .expect("resolve workspace");
        assert_eq!(roots, vec![PathBuf::from("/project/packages/ui")]);
    }

    #[test]
    fn workspace_filter_reports_invalid_glob_after_no_literal_match() {
        let workspaces = vec![ws("web", "apps/web")];
        let err = resolve_workspace_filter_roots(
            Path::new("/project"),
            &["web-[bad".to_owned()],
            &workspaces,
        )
        .expect_err("invalid glob");

        assert!(matches!(err, WorkspaceScopeError::InvalidPattern { .. }));
    }

    #[test]
    fn workspace_filter_negation_can_exclude_selected_workspaces() {
        let workspaces = vec![
            ws("web", "apps/web"),
            ws("docs", "apps/docs"),
            ws("legacy", "apps/legacy"),
        ];
        let roots = resolve_workspace_filter_roots(
            Path::new("/project"),
            &["apps/*".to_owned(), "!apps/legacy".to_owned()],
            &workspaces,
        )
        .expect("resolve workspace");

        assert_eq!(
            roots,
            vec![
                PathBuf::from("/project/apps/docs"),
                PathBuf::from("/project/apps/web")
            ]
        );
    }

    #[test]
    fn workspace_filter_only_negation_starts_from_all_workspaces() {
        let workspaces = vec![ws("web", "apps/web"), ws("legacy", "apps/legacy")];
        let roots = resolve_workspace_filter_roots(
            Path::new("/project"),
            &["!apps/legacy".to_owned()],
            &workspaces,
        )
        .expect("resolve workspace");

        assert_eq!(roots, vec![PathBuf::from("/project/apps/web")]);
    }

    #[test]
    fn workspace_filter_reports_unmatched_patterns_with_available_list() {
        let workspaces = vec![ws("web", "apps/web"), ws("docs", "apps/docs")];
        let err = resolve_workspace_filter_roots(
            Path::new("/project"),
            &["missing".to_owned()],
            &workspaces,
        )
        .expect_err("unmatched pattern");

        assert_eq!(
            err,
            WorkspaceScopeError::UnmatchedPatterns {
                patterns: vec!["missing".to_owned()],
                available: "web, docs".to_owned(),
            }
        );
    }

    #[test]
    fn workspace_filter_reports_empty_after_exclusions() {
        let workspaces = vec![ws("web", "apps/web")];
        let err = resolve_workspace_filter_roots(
            Path::new("/project"),
            &["!apps/web".to_owned()],
            &workspaces,
        )
        .expect_err("empty selection");

        assert_eq!(
            err,
            WorkspaceScopeError::EmptyAfterExclusions {
                included: "<all>".to_owned(),
                excluded: "'apps/web'".to_owned(),
            }
        );
    }

    #[test]
    fn workspace_available_list_truncates_when_above_cap() {
        let workspaces = (0..15)
            .map(|index| ws(&format!("pkg-{index}"), &format!("packages/pkg-{index}")))
            .collect::<Vec<_>>();

        let rendered = format_available_workspaces(&workspaces);

        assert!(rendered.starts_with("pkg-0, pkg-1,"));
        assert!(rendered.contains("and 5 more"));
        assert!(rendered.contains("15 total"));
    }

    #[test]
    fn changed_workspace_scope_ignores_root_only_changes() {
        let workspaces = vec![ws("ui", "packages/ui"), ws("api", "packages/api")];
        let mut changed = FxHashSet::default();
        changed.insert(PathBuf::from("/project/package.json"));
        changed.insert(PathBuf::from("/project/pnpm-lock.yaml"));

        let roots = roots_for_changed_files(&workspaces, &changed);

        assert!(roots.is_empty());
    }

    #[test]
    fn changed_workspace_scope_maps_files_to_workspace_roots() {
        let workspaces = vec![
            ws("ui", "packages/ui"),
            ws("api", "packages/api"),
            ws("cli", "packages/cli"),
        ];
        let mut changed = FxHashSet::default();
        changed.insert(PathBuf::from("/project/packages/api/src/b.ts"));
        changed.insert(PathBuf::from("/project/packages/ui/src/a.ts"));

        let roots = roots_for_changed_files(&workspaces, &changed);

        assert_eq!(
            roots,
            vec![
                PathBuf::from("/project/packages/api"),
                PathBuf::from("/project/packages/ui")
            ]
        );
    }

    fn roots_for_changed_files(
        workspaces: &[WorkspaceInfo],
        changed_files: &FxHashSet<PathBuf>,
    ) -> Vec<PathBuf> {
        let mut roots = workspaces
            .iter()
            .filter(|workspace| {
                changed_files
                    .iter()
                    .any(|file| file.starts_with(&workspace.root))
            })
            .map(|workspace| workspace.root.clone())
            .collect::<Vec<_>>();
        roots.sort();
        roots
    }
}
