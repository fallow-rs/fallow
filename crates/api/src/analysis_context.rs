//! Shared programmatic analysis context resolution.

use std::path::{Path, PathBuf};

use fallow_config::WorkspaceInfo;
use fallow_engine::workspace_scope::{WorkspaceScopeError, WorkspaceScopeMode};
use fallow_output::{DiffIndex, MAX_DIFF_BYTES};
use fallow_types::path_util::is_absolute_path_any_platform;
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
    pub(crate) allow_remote_extends: bool,
    pub(crate) no_cache: bool,
    pub(crate) threads: usize,
    pub(crate) pool: rayon::ThreadPool,
    pub(crate) diff: Option<DiffIndex>,
    pub(crate) production_override: Option<bool>,
    pub(crate) changed_since: Option<String>,
    pub(crate) workspace: Option<Vec<String>>,
    pub(crate) changed_workspaces: Option<String>,
    pub(crate) workspace_roots: Option<Vec<PathBuf>>,
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
    resolve_programmatic_analysis_context_inner(options, true)
}

pub fn resolve_programmatic_analysis_context_deferred_workspace(
    options: &AnalysisOptions,
) -> ProgrammaticResult<ProgrammaticAnalysisContext> {
    resolve_programmatic_analysis_context_inner(options, false)
}

fn resolve_programmatic_analysis_context_inner(
    options: &AnalysisOptions,
    resolve_workspace: bool,
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
    let workspace_roots = if resolve_workspace {
        resolve_workspace_scope(
            &root,
            options.workspace.as_deref(),
            options.changed_workspaces.as_deref(),
        )?
    } else {
        None
    };
    Ok(ProgrammaticAnalysisContext {
        root,
        config_path: options.config_path.clone(),
        allow_remote_extends: options.allow_remote_extends,
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
    fallow_engine::validate::validate_root(&root).map_err(|err| {
        ProgrammaticError::new(err, 2)
            .with_code("FALLOW_INVALID_ROOT")
            .with_context("analysis.root")
    })
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

    /// Whether this call permits remote config inheritance.
    #[must_use]
    pub const fn allow_remote_extends(&self) -> bool {
        self.allow_remote_extends
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
    fallow_engine::changed_files::changed_files(&resolved.root, git_ref)
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

pub fn workspace_roots_for_session(
    resolved: &ProgrammaticAnalysisContext,
    workspaces: &[WorkspaceInfo],
) -> ProgrammaticResult<Option<Vec<PathBuf>>> {
    resolve_workspace_scope_from_workspaces(
        &resolved.root,
        resolved.workspace.as_deref(),
        resolved.changed_workspaces.as_deref(),
        workspaces,
    )
}

fn resolve_workspace_scope(
    root: &Path,
    workspace: Option<&[String]>,
    changed_workspaces: Option<&str>,
) -> ProgrammaticResult<Option<Vec<PathBuf>>> {
    fallow_engine::workspace_scope::resolve_workspace_scope_roots_for_project(
        root,
        workspace,
        changed_workspaces,
    )
    .map_err(map_workspace_scope_error)
}

fn resolve_workspace_scope_from_workspaces(
    root: &Path,
    workspace: Option<&[String]>,
    changed_workspaces: Option<&str>,
    workspaces: &[WorkspaceInfo],
) -> ProgrammaticResult<Option<Vec<PathBuf>>> {
    fallow_engine::workspace_scope::resolve_workspace_scope_roots(
        root,
        workspace,
        changed_workspaces,
        workspaces,
    )
    .map_err(map_workspace_scope_error)
}

#[cfg(test)]
pub fn resolve_workspace_filters(
    root: &Path,
    patterns: &[String],
) -> ProgrammaticResult<Vec<PathBuf>> {
    fallow_engine::workspace_scope::resolve_workspace_filter_roots_for_project(root, patterns)
        .map_err(map_workspace_scope_error)
}

fn map_workspace_scope_error(err: WorkspaceScopeError) -> ProgrammaticError {
    match err {
        WorkspaceScopeError::NoWorkspaces {
            mode,
            patterns,
            git_ref,
        } => map_no_workspaces_error(mode, &patterns, git_ref.as_deref()),
        WorkspaceScopeError::InvalidPattern { pattern, message } => ProgrammaticError::new(
            format!("invalid `workspace` pattern '{pattern}': {message}"),
            2,
        )
        .with_code("FALLOW_INVALID_WORKSPACE_PATTERN")
        .with_context("analysis.workspace"),
        WorkspaceScopeError::UnmatchedPatterns {
            patterns,
            available,
        } => ProgrammaticError::new(
            format!(
                "`workspace` matched no workspace for pattern{}: {}. Available: {available}",
                if patterns.len() == 1 { "" } else { "s" },
                quote_owned_patterns(&patterns),
            ),
            2,
        )
        .with_code("FALLOW_WORKSPACE_PATTERN_UNMATCHED")
        .with_context("analysis.workspace"),
        WorkspaceScopeError::EmptyAfterExclusions { .. } => {
            ProgrammaticError::new("`workspace` excluded every discovered workspace", 2)
                .with_code("FALLOW_WORKSPACE_SCOPE_EMPTY")
                .with_context("analysis.workspace")
        }
        WorkspaceScopeError::ChangedWorkspacesFailed { git_ref, message } => {
            ProgrammaticError::new(
                format!("failed to resolve changed workspaces for ref `{git_ref}`: {message}"),
                2,
            )
            .with_code("FALLOW_CHANGED_WORKSPACES_FAILED")
            .with_context("analysis.changedWorkspaces")
        }
        WorkspaceScopeError::MutuallyExclusive => ProgrammaticError::new(
            "`workspace` and `changed_workspaces` are mutually exclusive",
            2,
        )
        .with_code("FALLOW_MUTUALLY_EXCLUSIVE_SCOPE")
        .with_context("analysis.workspace"),
    }
}

fn map_no_workspaces_error(
    mode: WorkspaceScopeMode,
    patterns: &[String],
    git_ref: Option<&str>,
) -> ProgrammaticError {
    match mode {
        WorkspaceScopeMode::Workspace => ProgrammaticError::new(
            format!(
                "`workspace` {} specified but no workspaces found. Ensure root package.json has a \"workspaces\" field, pnpm-workspace.yaml exists, or tsconfig.json has \"references\".",
                quote_owned_patterns(patterns)
            ),
            2,
        )
        .with_code("FALLOW_WORKSPACES_NOT_FOUND")
        .with_context("analysis.workspace"),
        WorkspaceScopeMode::ChangedWorkspaces => {
            let git_ref = git_ref.unwrap_or_default();
            ProgrammaticError::new(
                format!(
                    "`changed_workspaces` '{git_ref}' specified but no workspaces found. Ensure root package.json has a \"workspaces\" field, pnpm-workspace.yaml exists, or tsconfig.json has \"references\"."
                ),
                2,
            )
            .with_code("FALLOW_WORKSPACES_NOT_FOUND")
            .with_context("analysis.changedWorkspaces")
        }
    }
}

fn quote_owned_patterns(patterns: &[String]) -> String {
    patterns
        .iter()
        .map(|pattern| format!("'{pattern}'"))
        .collect::<Vec<_>>()
        .join(", ")
}
