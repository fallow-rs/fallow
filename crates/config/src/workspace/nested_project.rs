//! Independent pnpm projects nested below the workspace root.

use std::path::{Path, PathBuf};

use super::{
    ManifestCache, WorkspaceDiagnostic, WorkspaceDiagnosticKind, WorkspaceInfo,
    collect_declared_patterns, register_matched_workspace,
    undeclared::{is_ignored_scan_dir, is_undeclared_scan_skip_dir},
};

/// Whether `dir` holds both a `package.json` and a `pnpm-workspace.yaml`, so
/// pnpm treats it as the root of its own project.
pub(super) fn is_nested_project_dir(dir: &Path) -> bool {
    dir.join("package.json").is_file() && dir.join(PNPM_WORKSPACE_FILE).is_file()
}

const PNPM_WORKSPACE_FILE: &str = "pnpm-workspace.yaml";

/// Matches project-root-relative directories against the workspace patterns
/// that the root manifests declare. The fallow config `workspaces.patterns`
/// is not part of it: those patterns add directories to the analysis, and
/// pnpm does not read them.
struct ManifestPatternMatcher {
    positive: globset::GlobSet,
    negative: globset::GlobSet,
}

impl ManifestPatternMatcher {
    /// Build the matcher. A root manifest that does not parse gives a matcher
    /// that declares nothing; discovery reports the parse error.
    fn load(root: &Path) -> Self {
        let mut positive = globset::GlobSetBuilder::new();
        let mut negative = globset::GlobSetBuilder::new();
        for declared in collect_declared_patterns(root, &[]).unwrap_or_default() {
            let (builder, pattern) = match declared.pattern.strip_prefix('!') {
                Some(rest) => (&mut negative, rest),
                None => (&mut positive, declared.pattern.as_str()),
            };
            let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
            let pattern = if pattern.ends_with('/') {
                format!("{pattern}*")
            } else {
                pattern.to_owned()
            };
            if let Ok(glob) = globset::GlobBuilder::new(&pattern)
                .literal_separator(true)
                .build()
            {
                builder.add(glob);
            }
        }
        Self {
            positive: positive
                .build()
                .unwrap_or_else(|_| globset::GlobSet::empty()),
            negative: negative
                .build()
                .unwrap_or_else(|_| globset::GlobSet::empty()),
        }
    }

    fn declares(&self, root: &Path, dir: &Path) -> bool {
        let relative = dir.strip_prefix(root).unwrap_or(dir);
        let relative = relative.to_string_lossy().replace('\\', "/");
        self.positive.is_match(relative.as_str()) && !self.negative.is_match(relative.as_str())
    }
}

/// Whether the workspace at `ws_root` is an independent pnpm project: it has
/// its own `pnpm-workspace.yaml` and no workspace pattern of the root
/// manifests declares it.
///
/// pnpm resolves the `catalog:` references of such a project against its own
/// catalog, so the root catalog checks leave its manifest out. The rule also
/// holds when the fallow config `workspaces.patterns` adds the directory to
/// the analysis.
#[must_use]
pub fn is_independent_pnpm_project(root: &Path, ws_root: &Path) -> bool {
    if !ws_root.join(PNPM_WORKSPACE_FILE).is_file() || ws_root.strip_prefix(root).is_err() {
        return false;
    }
    let canonical = |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if canonical(ws_root) == canonical(root) {
        return false;
    }
    !ManifestPatternMatcher::load(root).declares(root, ws_root)
}

/// Borrowed inputs of the nested-project scan.
pub(super) struct NestedProjectScan<'a> {
    pub(super) root: &'a Path,
    pub(super) canonical_root: &'a Path,
    pub(super) ignore_patterns: &'a crate::IgnorePatternSet,
}

/// Directories up to two levels below the root that are independent pnpm
/// projects, in sorted order. The scan skips the same directories as the
/// undeclared-workspace scan, skips directories that `ignore_patterns`
/// excludes, and does not look inside a project it found.
fn nested_project_roots(scan: &NestedProjectScan<'_>) -> Vec<PathBuf> {
    let matcher = ManifestPatternMatcher::load(scan.root);
    let is_candidate = |dir: &Path| {
        is_nested_project_dir(dir)
            && !matcher.declares(scan.root, dir)
            && !is_ignored_scan_dir(scan.root, dir, scan.ignore_patterns)
    };
    let mut roots = Vec::new();
    for top in scan_dirs(scan.root) {
        if is_nested_project_dir(&top) {
            if is_candidate(&top) {
                roots.push(top);
            }
            continue;
        }
        roots.extend(
            scan_dirs(&top)
                .into_iter()
                .filter(|child| is_candidate(child)),
        );
    }
    roots.sort();
    roots
}

/// The child directories of `parent` that the workspace scans visit.
fn scan_dirs(parent: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| !is_undeclared_scan_skip_dir(&entry.file_name().to_string_lossy()))
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect()
}

/// Register each independent pnpm project below the root as a workspace, with
/// one `nested-project-root` diagnostic.
///
/// The project's `package.json` then owns its files and dependencies, and
/// framework plugins activate from it. A directory that an earlier source
/// already registered, for example the fallow config `workspaces.patterns`,
/// keeps that registration and gets no diagnostic.
pub(super) fn collect_nested_project_workspaces(
    scan: &NestedProjectScan<'_>,
    workspaces: &mut Vec<(WorkspaceInfo, Vec<String>)>,
    diagnostics: &mut Vec<WorkspaceDiagnostic>,
    manifest_cache: &mut ManifestCache,
) {
    let canonical = |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let known: rustc_hash::FxHashSet<PathBuf> = workspaces
        .iter()
        .map(|(ws, _)| canonical(&ws.root))
        .collect();
    for dir in nested_project_roots(scan) {
        let canonical_dir = canonical(&dir);
        if known.contains(&canonical_dir) || !canonical_dir.starts_with(scan.canonical_root) {
            continue;
        }
        let registered_before = workspaces.len();
        register_matched_workspace(
            scan.root,
            dir.clone(),
            workspaces,
            diagnostics,
            manifest_cache,
        );
        if workspaces.len() > registered_before {
            diagnostics.push(WorkspaceDiagnostic::new(
                scan.root,
                dir,
                WorkspaceDiagnosticKind::NestedProjectRoot,
            ));
        }
    }
}
