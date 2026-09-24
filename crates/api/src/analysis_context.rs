//! Shared programmatic analysis context resolution.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use fallow_config::WorkspaceInfo;
use fallow_engine::workspace_scope::{WorkspaceScopeError, WorkspaceScopeMode};
use fallow_output::{DiffIndex, MAX_DIFF_BYTES, RequestName, RequestOutcome, RequestOutcomes};
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
    /// What became of the diff request, for the envelope's `request_outcomes`.
    pub(crate) diff_request: Option<RequestOutcome>,
    pub(crate) production_override: Option<bool>,
    /// The changed-since ref the call narrows by: the caller's own, or the
    /// ambient one when it resolved. `None` when an ambient ref stood down.
    pub(crate) changed_since: Option<String>,
    /// What became of the changed-since request, set once it resolved or
    /// stood down.
    pub(crate) changed_since_request: OnceLock<RequestOutcome>,
    /// The changed files of the resolved ref, normalized like the CLI's.
    pub(crate) changed_since_files: OnceLock<FxHashSet<PathBuf>>,
    /// The changed files the call's analyses kept, over every analysis that
    /// measured: the `scope_size` of the `changed-since` entry.
    pub(crate) changed_since_analyzed: Mutex<Option<FxHashSet<PathBuf>>>,
    pub(crate) workspace: Option<Vec<String>>,
    pub(crate) changed_workspaces: Option<String>,
    pub(crate) workspace_roots: Option<Vec<PathBuf>>,
    pub(crate) explain: bool,
    pub(crate) cancellation: Option<Arc<AtomicBool>>,
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
    let pool = fallow_engine::thread_pool::worker_pool_builder(threads)
        .build()
        .map_err(|err| {
            ProgrammaticError::new(format!("failed to build analysis thread pool: {err}"), 2)
                .with_code("FALLOW_THREAD_POOL_INIT_FAILED")
                .with_context("analysis.threads")
        })?;
    let (diff, diff_request) = resolve_diff(options, &root)?;
    let changed_since_request = OnceLock::new();
    let changed_since_files = OnceLock::new();
    let changed_since =
        resolve_changed_since(options, &root, &changed_since_request, &changed_since_files);
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
        diff_request,
        production_override: options
            .production_override
            .or_else(|| options.production.then_some(true)),
        changed_since,
        changed_since_request,
        changed_since_files,
        changed_since_analyzed: Mutex::new(None),
        workspace: options.workspace.clone(),
        changed_workspaces: options.changed_workspaces.clone(),
        workspace_roots,
        explain: options.explain,
        cancellation: options.cancellation.clone(),
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

pub fn resolve_analysis_root(root: Option<&Path>) -> ProgrammaticResult<PathBuf> {
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

pub fn validate_analysis_config_path(config_path: Option<&Path>) -> ProgrammaticResult<()> {
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

    /// Parsed diff for this call, explicit or ambient, if one applied.
    #[must_use]
    pub const fn diff_index(&self) -> Option<&DiffIndex> {
        self.diff.as_ref()
    }

    /// The call's `request_outcomes`, or `None` when it was asked for nothing.
    ///
    /// Carries the `diff-filter` entry, which is the one request this context
    /// resolves and can stand down. Same object as the CLI publishes for the
    /// same diff.
    #[must_use]
    pub fn request_outcomes(&self) -> Option<RequestOutcomes> {
        let mut requests = RequestOutcomes::new();
        requests.insert_if(RequestName::ChangedSince, self.changed_since_outcome());
        requests.insert_if(RequestName::DiffFilter, self.diff_request.clone());
        requests.into_option()
    }

    /// The `changed-since` entry, with the measured scope when the ref applied
    /// and an analysis measured it, as the CLI publishes it.
    fn changed_since_outcome(&self) -> Option<RequestOutcome> {
        let outcome = self.changed_since_request.get()?.clone();
        let size = self
            .changed_since_analyzed
            .lock()
            .ok()
            .and_then(|analyzed| analyzed.as_ref().map(|files| files.len() as u64));
        Some(match size {
            Some(size) if outcome.status == fallow_output::RequestStatus::Applied => {
                RequestOutcome {
                    scope_size: Some(size),
                    ..outcome
                }
            }
            _ => outcome,
        })
    }

    /// Add the changed files an analysis kept to the call's analyzed changed
    /// files. Does nothing when no ref resolved.
    pub(crate) fn measure_changed_since_scope<'a>(
        &self,
        analyzed: impl IntoIterator<Item = &'a Path>,
    ) {
        let Some(changed) = self.changed_since_files.get() else {
            return;
        };
        let Ok(mut union) = self.changed_since_analyzed.lock() else {
            return;
        };
        union.get_or_insert_with(FxHashSet::default).extend(
            analyzed
                .into_iter()
                .map(dunce::simplified)
                .filter(|path| changed.contains(*path))
                .map(Path::to_path_buf),
        );
    }

    /// Record the resolved changed files of the call's ref, and the `applied`
    /// entry, once.
    fn record_changed_since_applied(&self, git_ref: &str, files: &FxHashSet<PathBuf>) {
        let _ = self.changed_since_files.set(
            files
                .iter()
                .map(|path| dunce::simplified(path).to_path_buf())
                .collect(),
        );
        let _ = self
            .changed_since_request
            .set(RequestOutcome::applied(RequestName::ChangedSince, git_ref));
    }

    /// Record that an engine runner narrowed by the call's ref, with the
    /// changed files it kept. For a runner that resolves the ref itself.
    pub(crate) fn record_changed_since_from_runner(&self, kept: Option<&[PathBuf]>) {
        let (Some(git_ref), Some(kept)) = (self.changed_since.as_deref(), kept) else {
            return;
        };
        if self.changed_since_files.get().is_none() {
            let files: FxHashSet<PathBuf> = kept.iter().cloned().collect();
            self.record_changed_since_applied(git_ref, &files);
        }
        self.measure_changed_since_scope(kept.iter().map(PathBuf::as_path));
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

    /// The caller's cancellation token for this analysis, if it supplied one.
    #[must_use]
    pub fn cancellation(&self) -> Option<&Arc<AtomicBool>> {
        self.cancellation.as_ref()
    }

    /// Whether the caller has asked this analysis to stop.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation
            .as_ref()
            .is_some_and(|cancelled| cancelled.load(Ordering::SeqCst))
    }

    /// Stop the analysis at a stage boundary once the caller has cancelled it.
    ///
    /// `stage` names the work that has not been started, so the error says how
    /// far the run got rather than only that it was stopped.
    ///
    /// # Errors
    ///
    /// Returns a `FALLOW_CANCELLED` programmatic error when the caller's token
    /// is set. Cancellation is always an error, never an empty success: an
    /// empty report reads downstream as a clean project.
    pub fn ensure_not_cancelled(&self, stage: &str) -> ProgrammaticResult<()> {
        if self.is_cancelled() {
            return Err(cancelled_error(stage));
        }
        Ok(())
    }
}

/// Stop before any work starts when the caller's token is already set.
///
/// Runtimes that never build a [`ProgrammaticAnalysisContext`] read the token
/// straight off the options with this.
///
/// # Errors
///
/// Returns a `FALLOW_CANCELLED` programmatic error when the token is set.
pub fn ensure_options_not_cancelled(
    options: &AnalysisOptions,
    stage: &str,
) -> ProgrammaticResult<()> {
    if options
        .cancellation
        .as_ref()
        .is_some_and(|cancelled| cancelled.load(Ordering::SeqCst))
    {
        return Err(cancelled_error(stage));
    }
    Ok(())
}

/// The single `FALLOW_CANCELLED` error shape for the programmatic API.
///
/// `stage` names the work the run never started, so the error says how far it
/// got and not only that it stopped.
#[must_use]
pub fn cancelled_error(stage: &str) -> ProgrammaticError {
    cancelled_error_message(&format!("analysis was cancelled before {stage}"))
}

/// A `FALLOW_CANCELLED` error carrying a message a lower layer already built.
#[must_use]
pub fn cancelled_error_message(message: &str) -> ProgrammaticError {
    ProgrammaticError::new(message, 2)
        .with_code("FALLOW_CANCELLED")
        .with_context("analysis.cancellation")
}

fn default_threads() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}

/// Resolve the call's diff from its two sources, which fail differently.
///
/// An explicit `diff_file` is the caller's own argument, so a bad file is a
/// `FALLOW_INVALID_DIFF_FILE` error. An ambient `FALLOW_DIFF_FILE` comes from
/// the environment the caller inherited, so a bad file stands down: no diff,
/// full scope, and a `not-applied` outcome with the CLI's reason token and
/// sentence. The source decides the behavior, never the text of an error.
fn resolve_diff(
    options: &AnalysisOptions,
    root: &Path,
) -> ProgrammaticResult<(Option<DiffIndex>, Option<RequestOutcome>)> {
    if let Some(path) = options.diff_file.as_deref() {
        let index = load_explicit_diff_file(path, root)?;
        let request = diff_applied(format!("diffFile {}", path.display()), &index);
        return Ok((Some(index), Some(request)));
    }
    let Some(path) = options.ambient_diff_file.as_deref() else {
        return Ok((None, None));
    };
    Ok(load_ambient_diff_file(path, root))
}

/// Load and place an ambient diff the way the CLI loads `$FALLOW_DIFF_FILE`,
/// with the same label, so both routes publish the same outcome object.
fn load_ambient_diff_file(path: &Path, root: &Path) -> (Option<DiffIndex>, Option<RequestOutcome>) {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let label = format!("$FALLOW_DIFF_FILE {}", abs.display());
    let placed = fallow_engine::diff_source::read_diff_file(&abs, &label).and_then(|text| {
        fallow_engine::diff_source::place_diff(
            DiffIndex::from_unified_diff(&text),
            root,
            &fallow_engine::diff_source::diff_base_candidates(root),
            &label,
        )
    });
    match placed {
        Ok(index) => {
            let request = diff_applied(label, &index);
            (Some(index), Some(request))
        }
        Err(stand_down) => {
            let (reason, message) = stand_down.into_parts();
            let request =
                RequestOutcome::not_applied(RequestName::DiffFilter, label, reason, message);
            (None, Some(request))
        }
    }
}

/// An applied diff filter, sized in added lines like the CLI's.
fn diff_applied(label: String, index: &DiffIndex) -> RequestOutcome {
    RequestOutcome::applied_with_scope_size(
        RequestName::DiffFilter,
        label,
        index.added_line_count() as u64,
    )
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

/// Resolve the call's changed-since ref once, when it comes from the
/// environment.
///
/// The two sources fail differently, like the two diff sources. An explicit
/// `changed_since` is the caller's own argument, so a ref that does not
/// resolve fails the call later, in [`changed_files_for_run`]. An ambient
/// `FALLOW_CHANGED_SINCE` comes from the environment the caller inherited, so
/// a ref that does not resolve stands down here: the call runs at full scope
/// and publishes `not-applied` with the CLI's reason token and sentence.
fn resolve_changed_since(
    options: &AnalysisOptions,
    root: &Path,
    request: &OnceLock<RequestOutcome>,
    files: &OnceLock<FxHashSet<PathBuf>>,
) -> Option<String> {
    if let Some(git_ref) = options.changed_since.as_deref() {
        return Some(git_ref.to_owned());
    }
    let git_ref = options.ambient_changed_since.as_deref()?;
    match fallow_engine::changed_files::changed_files(root, git_ref) {
        Ok(changed) => {
            let _ = files.set(
                changed
                    .iter()
                    .map(|path| dunce::simplified(path).to_path_buf())
                    .collect(),
            );
            let _ = request.set(RequestOutcome::applied(RequestName::ChangedSince, git_ref));
            Some(git_ref.to_owned())
        }
        Err(err) => {
            let _ = request.set(RequestOutcome::not_applied(
                RequestName::ChangedSince,
                git_ref,
                err.reason(),
                err.changed_since_message(git_ref),
            ));
            None
        }
    }
}

pub fn changed_files_for_run(
    resolved: &ProgrammaticAnalysisContext,
) -> ProgrammaticResult<Option<FxHashSet<PathBuf>>> {
    let Some(git_ref) = resolved.changed_since.as_deref() else {
        return Ok(None);
    };
    fallow_engine::changed_files::changed_files(&resolved.root, git_ref)
        .inspect(|files| resolved.record_changed_since_applied(git_ref, files))
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

#[cfg(test)]
mod tests {
    use std::process::Command;

    use crate::AnalysisOptions;

    const STACK_PROBE_ENV: &str = "FALLOW_API_STACK_PROBE_CHILD";
    const STACK_PROBE_TEST: &str =
        "analysis_context::tests::programmatic_pool_survives_deep_worker_stack_probe";

    // A stack overflow aborts the whole process, so the probe re-runs this
    // test binary as a child and asserts on its exit status; the same pattern
    // guards the CLI global pool in crates/cli/src/rayon_pool.rs. The child
    // drops RUST_MIN_STACK (pinned to 16 MiB in .cargo/config.toml, and
    // inherited by default-sized rayon workers) so the probe still fails if
    // the pool loses its explicit stack_size.
    #[test]
    fn programmatic_pool_survives_deep_worker_stack_probe() {
        if std::env::var_os(STACK_PROBE_ENV).is_some() {
            run_stack_probe_child();
            return;
        }

        let current_exe = std::env::current_exe().expect("current test binary should be known");
        let output = Command::new(current_exe)
            .arg("--exact")
            .arg(STACK_PROBE_TEST)
            .arg("--nocapture")
            .env(STACK_PROBE_ENV, "1")
            .env_remove("RUST_MIN_STACK")
            .output()
            .expect("stack probe child should start");

        assert!(
            output.status.success(),
            "stack probe child failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn run_stack_probe_child() {
        let root = tempfile::tempdir().expect("stack probe needs a temp analysis root");
        let options = AnalysisOptions {
            root: Some(root.path().to_path_buf()),
            threads: Some(1),
            ..AnalysisOptions::default()
        };
        let context = super::resolve_programmatic_analysis_context(&options)
            .expect("stack probe context should resolve");
        assert_eq!(context.install(|| consume_stack(5_000)), 5_000);
    }

    #[inline(never)]
    fn consume_stack(depth: usize) -> usize {
        let frame = [0_u8; 2048];
        std::hint::black_box(&frame);
        if depth == 0 {
            usize::from(frame[0])
        } else {
            1 + consume_stack(depth - 1)
        }
    }
}
