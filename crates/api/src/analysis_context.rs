//! Shared programmatic analysis context resolution.

use std::path::{Path, PathBuf};

use fallow_config::WorkspaceInfo;
use fallow_output::{DiffIndex, MAX_DIFF_BYTES};
use fallow_types::path_util::is_absolute_path_any_platform;
use globset::Glob;
use rustc_hash::FxHashSet;

use crate::{AnalysisOptions, ProgrammaticError};

type ProgrammaticResult<T> = Result<T, ProgrammaticError>;

/// Resolved common programmatic analysis context.
///
/// This owns validation, root/config/diff resolution, production overrides,
/// workspace scope, and the per-call thread pool shared by programmatic
/// analysis families. API runtimes and engine-backed runners use it directly.
pub struct ProgrammaticAnalysisContext {
    pub(crate) root: PathBuf,
    pub(crate) config_path: Option<PathBuf>,
    pub(crate) no_cache: bool,
    pub(crate) threads: usize,
    pub(crate) pool: rayon::ThreadPool,
    pub(crate) diff: Option<DiffIndex>,
    pub(crate) production_override: Option<bool>,
    pub(crate) changed_since: Option<String>,
    pub(crate) workspace: Option<Vec<String>>,
    pub(crate) changed_workspaces: Option<String>,
    pub(crate) workspace_roots: Option<Vec<PathBuf>>,
    pub(crate) legacy_envelope: bool,
    pub(crate) explain: bool,
}

/// Resolve common programmatic analysis options once for a concrete runtime.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid roots, configs, thread
/// counts, workspace scopes, or explicit diff files.
pub fn resolve_programmatic_analysis_context(
    options: &AnalysisOptions,
) -> ProgrammaticResult<ProgrammaticAnalysisContext> {
    validate_analysis_option_shape(options)?;
    let root = resolve_analysis_root(options.root.as_deref())?;
    validate_analysis_config_path(options.config_path.as_deref())?;
    let threads = options.threads.unwrap_or_else(default_threads);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|err| {
            ProgrammaticError::new(format!("failed to build analysis thread pool: {err}"), 2)
                .with_code("FALLOW_THREAD_POOL_INIT_FAILED")
                .with_context("analysis.threads")
        })?;
    let diff = options
        .diff_file
        .as_deref()
        .map(|path| load_explicit_diff_file(path, &root))
        .transpose()?;
    let workspace_roots = resolve_workspace_scope(
        &root,
        options.workspace.as_deref(),
        options.changed_workspaces.as_deref(),
    )?;
    Ok(ProgrammaticAnalysisContext {
        root,
        config_path: options.config_path.clone(),
        no_cache: options.no_cache,
        threads,
        pool,
        diff,
        production_override: options
            .production_override
            .or_else(|| options.production.then_some(true)),
        changed_since: options.changed_since.clone(),
        workspace: options.workspace.clone(),
        changed_workspaces: options.changed_workspaces.clone(),
        workspace_roots,
        legacy_envelope: options.legacy_envelope,
        explain: options.explain,
    })
}

fn validate_analysis_option_shape(options: &AnalysisOptions) -> ProgrammaticResult<()> {
    if options.threads == Some(0) {
        return Err(
            ProgrammaticError::new("`threads` must be greater than 0", 2)
                .with_code("FALLOW_INVALID_THREADS")
                .with_context("analysis.threads"),
        );
    }
    if options.workspace.is_some() && options.changed_workspaces.is_some() {
        return Err(ProgrammaticError::new(
            "`workspace` and `changed_workspaces` are mutually exclusive",
            2,
        )
        .with_code("FALLOW_MUTUALLY_EXCLUSIVE_SCOPE")
        .with_context("analysis.workspace"));
    }
    Ok(())
}

fn resolve_analysis_root(root: Option<&Path>) -> ProgrammaticResult<PathBuf> {
    let root = match root {
        Some(root) => root.to_path_buf(),
        None => std::env::current_dir().map_err(|err| {
            ProgrammaticError::new(
                format!("failed to resolve current working directory: {err}"),
                2,
            )
            .with_code("FALLOW_CWD_UNAVAILABLE")
            .with_context("analysis.root")
        })?,
    };
    if !root.exists() {
        return Err(ProgrammaticError::new(
            format!("analysis root does not exist: {}", root.display()),
            2,
        )
        .with_code("FALLOW_INVALID_ROOT")
        .with_context("analysis.root"));
    }
    if !root.is_dir() {
        return Err(ProgrammaticError::new(
            format!("analysis root is not a directory: {}", root.display()),
            2,
        )
        .with_code("FALLOW_INVALID_ROOT")
        .with_context("analysis.root"));
    }
    Ok(root)
}

fn validate_analysis_config_path(config_path: Option<&Path>) -> ProgrammaticResult<()> {
    if let Some(config_path) = config_path
        && !config_path.exists()
    {
        return Err(ProgrammaticError::new(
            format!("config file does not exist: {}", config_path.display()),
            2,
        )
        .with_code("FALLOW_INVALID_CONFIG_PATH")
        .with_context("analysis.configPath"));
    }
    Ok(())
}

impl ProgrammaticAnalysisContext {
    /// Run work inside the per-call Rayon pool.
    pub fn install<R: Send>(&self, f: impl FnOnce() -> R + Send) -> R {
        self.pool.install(f)
    }

    /// Resolved analysis root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Config path supplied by the caller, if any.
    #[must_use]
    pub fn config_path(&self) -> &Option<PathBuf> {
        &self.config_path
    }

    /// Whether parser cache use is disabled for this call.
    #[must_use]
    pub const fn no_cache(&self) -> bool {
        self.no_cache
    }

    /// Effective parser thread count for this call.
    #[must_use]
    pub const fn threads(&self) -> usize {
        self.threads
    }

    /// Parsed explicit diff file, if supplied.
    #[must_use]
    pub const fn diff_index(&self) -> Option<&DiffIndex> {
        self.diff.as_ref()
    }

    /// Explicit production override supplied by the caller.
    #[must_use]
    pub const fn production_override(&self) -> Option<bool> {
        self.production_override
    }

    /// Git ref used to scope changed files.
    #[must_use]
    pub fn changed_since(&self) -> Option<&str> {
        self.changed_since.as_deref()
    }

    /// Workspace filter patterns supplied by the caller.
    #[must_use]
    pub fn workspace(&self) -> Option<&[String]> {
        self.workspace.as_deref()
    }

    /// Git ref used to scope changed workspaces.
    #[must_use]
    pub fn changed_workspaces(&self) -> Option<&str> {
        self.changed_workspaces.as_deref()
    }

    /// Whether API JSON should include explanatory metadata.
    #[must_use]
    pub const fn explain_enabled(&self) -> bool {
        self.explain
    }
}

fn default_threads() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}

fn load_explicit_diff_file(path: &Path, root: &Path) -> ProgrammaticResult<DiffIndex> {
    if path == Path::new("-") {
        return Err(ProgrammaticError::new(
            "`diff_file` does not support stdin; pass a file path",
            2,
        )
        .with_code("FALLOW_INVALID_DIFF_FILE")
        .with_context("analysis.diffFile"));
    }
    let abs = if is_absolute_path_any_platform(path) {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let meta = std::fs::metadata(&abs).map_err(|err| {
        ProgrammaticError::new(
            format!(
                "diff file does not exist or cannot be read: {} ({err})",
                abs.display()
            ),
            2,
        )
        .with_code("FALLOW_INVALID_DIFF_FILE")
        .with_context("analysis.diffFile")
    })?;
    if !meta.is_file() {
        return Err(ProgrammaticError::new(
            format!("diff path is not a file: {}", abs.display()),
            2,
        )
        .with_code("FALLOW_INVALID_DIFF_FILE")
        .with_context("analysis.diffFile"));
    }
    if meta.len() > MAX_DIFF_BYTES {
        return Err(ProgrammaticError::new(
            format!(
                "diff file is {} bytes, above the {MAX_DIFF_BYTES} byte limit: {}",
                meta.len(),
                abs.display()
            ),
            2,
        )
        .with_code("FALLOW_INVALID_DIFF_FILE")
        .with_context("analysis.diffFile"));
    }
    let text = std::fs::read_to_string(&abs).map_err(|err| {
        ProgrammaticError::new(
            format!("failed to read diff file {}: {err}", abs.display()),
            2,
        )
        .with_code("FALLOW_INVALID_DIFF_FILE")
        .with_context("analysis.diffFile")
    })?;
    Ok(DiffIndex::from_unified_diff(&text))
}

pub fn changed_files_for_run(
    resolved: &ProgrammaticAnalysisContext,
) -> ProgrammaticResult<Option<FxHashSet<PathBuf>>> {
    let Some(git_ref) = resolved.changed_since.as_deref() else {
        return Ok(None);
    };
    fallow_engine::changed_files(&resolved.root, git_ref)
        .map(Some)
        .map_err(|err| {
            ProgrammaticError::new(
                format!(
                    "failed to resolve changed files for ref `{git_ref}`: {}",
                    err.describe()
                ),
                2,
            )
            .with_code("FALLOW_CHANGED_FILES_FAILED")
            .with_context("analysis.changedSince")
        })
}

fn resolve_workspace_scope(
    root: &Path,
    workspace: Option<&[String]>,
    changed_workspaces: Option<&str>,
) -> ProgrammaticResult<Option<Vec<PathBuf>>> {
    match (workspace, changed_workspaces) {
        (Some(patterns), None) => resolve_workspace_filters(root, patterns).map(Some),
        (None, Some(git_ref)) => resolve_changed_workspaces(root, git_ref).map(Some),
        (None, None) => Ok(None),
        (Some(_), Some(_)) => Err(ProgrammaticError::new(
            "`workspace` and `changed_workspaces` are mutually exclusive",
            2,
        )
        .with_code("FALLOW_MUTUALLY_EXCLUSIVE_SCOPE")
        .with_context("analysis.workspace")),
    }
}

pub fn resolve_workspace_filters(
    root: &Path,
    patterns: &[String],
) -> ProgrammaticResult<Vec<PathBuf>> {
    let workspaces = fallow_config::discover_workspaces(root);
    if workspaces.is_empty() {
        let joined = patterns
            .iter()
            .map(|pattern| format!("'{pattern}'"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(ProgrammaticError::new(
            format!(
                "`workspace` {joined} specified but no workspaces found. Ensure root package.json has a \"workspaces\" field, pnpm-workspace.yaml exists, or tsconfig.json has \"references\"."
            ),
            2,
        )
        .with_code("FALLOW_WORKSPACES_NOT_FOUND")
        .with_context("analysis.workspace"));
    }

    let rel_paths = workspaces
        .iter()
        .map(|workspace| relative_workspace_path(&workspace.root, root))
        .collect::<Vec<_>>();
    let (positive, negative) = split_workspace_patterns(patterns);
    let mut matched = match_positive_workspace_patterns(&positive, &workspaces, &rel_paths)?;

    for pattern in &negative {
        for index in find_workspace_matches(pattern, &workspaces, &rel_paths)? {
            matched.remove(&index);
        }
    }

    if matched.is_empty() {
        return Err(
            ProgrammaticError::new("`workspace` excluded every discovered workspace", 2)
                .with_code("FALLOW_WORKSPACE_SCOPE_EMPTY")
                .with_context("analysis.workspace"),
        );
    }

    let mut roots = matched
        .into_iter()
        .map(|index| workspaces[index].root.clone())
        .collect::<Vec<_>>();
    roots.sort();
    Ok(roots)
}

fn resolve_changed_workspaces(root: &Path, git_ref: &str) -> ProgrammaticResult<Vec<PathBuf>> {
    let workspaces = fallow_config::discover_workspaces(root);
    if workspaces.is_empty() {
        return Err(ProgrammaticError::new(
            format!(
                "`changed_workspaces` '{git_ref}' specified but no workspaces found. Ensure root package.json has a \"workspaces\" field, pnpm-workspace.yaml exists, or tsconfig.json has \"references\"."
            ),
            2,
        )
        .with_code("FALLOW_WORKSPACES_NOT_FOUND")
        .with_context("analysis.changedWorkspaces"));
    }
    let changed_files = fallow_engine::changed_files(root, git_ref).map_err(|err| {
        ProgrammaticError::new(
            format!(
                "failed to resolve changed workspaces for ref `{git_ref}`: {}",
                err.describe()
            ),
            2,
        )
        .with_code("FALLOW_CHANGED_WORKSPACES_FAILED")
        .with_context("analysis.changedWorkspaces")
    })?;
    let mut roots = workspaces
        .into_iter()
        .filter(|workspace| {
            changed_files
                .iter()
                .any(|file| file.starts_with(&workspace.root))
        })
        .map(|workspace| workspace.root)
        .collect::<Vec<_>>();
    roots.sort();
    Ok(roots)
}

fn match_positive_workspace_patterns(
    positive: &[&str],
    workspaces: &[WorkspaceInfo],
    rel_paths: &[String],
) -> ProgrammaticResult<FxHashSet<usize>> {
    let mut matched = FxHashSet::default();
    let mut unmatched = Vec::new();

    if positive.is_empty() {
        matched.extend(0..workspaces.len());
    } else {
        for pattern in positive {
            let hits = find_workspace_matches(pattern, workspaces, rel_paths)?;
            if hits.is_empty() {
                unmatched.push((*pattern).to_string());
            }
            matched.extend(hits);
        }
    }

    if !unmatched.is_empty() {
        return Err(ProgrammaticError::new(
            format!(
                "`workspace` matched no workspace for pattern{}: {}. Available: {}",
                if unmatched.len() == 1 { "" } else { "s" },
                unmatched
                    .iter()
                    .map(|pattern| format!("'{pattern}'"))
                    .collect::<Vec<_>>()
                    .join(", "),
                format_available_workspaces(workspaces),
            ),
            2,
        )
        .with_code("FALLOW_WORKSPACE_PATTERN_UNMATCHED")
        .with_context("analysis.workspace"));
    }

    Ok(matched)
}

fn find_workspace_matches(
    pattern: &str,
    workspaces: &[WorkspaceInfo],
    rel_paths: &[String],
) -> ProgrammaticResult<Vec<usize>> {
    if let Some(index) = workspaces
        .iter()
        .position(|workspace| workspace.name == pattern)
    {
        return Ok(vec![index]);
    }
    if let Some(index) = rel_paths.iter().position(|path| path == pattern) {
        return Ok(vec![index]);
    }

    let glob = Glob::new(pattern).map_err(|err| {
        ProgrammaticError::new(format!("invalid `workspace` pattern '{pattern}': {err}"), 2)
            .with_code("FALLOW_INVALID_WORKSPACE_PATTERN")
            .with_context("analysis.workspace")
    })?;
    let matcher = glob.compile_matcher();
    let hits = workspaces
        .iter()
        .enumerate()
        .filter_map(|(index, workspace)| {
            (matcher.is_match(&workspace.name) || matcher.is_match(&rel_paths[index]))
                .then_some(index)
        })
        .collect();
    Ok(hits)
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

fn relative_workspace_path(workspace_root: &Path, root: &Path) -> String {
    workspace_root
        .strip_prefix(root)
        .unwrap_or(workspace_root)
        .to_string_lossy()
        .replace('\\', "/")
}
