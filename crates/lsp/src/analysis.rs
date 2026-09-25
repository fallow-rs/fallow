use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use fallow_api::{
    EditorAnalysisOutput, EditorAnalysisResults as AnalysisResults,
    EditorAnalysisSession as AnalysisSession, EditorDuplicationReport as DuplicationReport,
    EditorInlineComplexityFinding as InlineComplexityFinding,
};
use fallow_config::DuplicatesConfig;
use ls_types::MessageType;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::initialization::{LspDuplicationOptions, LspTypeAwareOptions};
use crate::protocol::{ChangedSinceScopeState, ChangedSinceScopeStatus, config_load_error_detail};
use crate::session_store::{EditorSessionStore, SessionKey};

/// The editor sessions kept between runs.
pub type SharedSessionStore = Arc<Mutex<EditorSessionStore>>;

/// The parse work of one run over all project roots.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunParseWork {
    /// Project sessions that loaded their config and walked the project for
    /// this run, as opposed to kept sessions.
    pub sessions_loaded: usize,
    /// Parse counts of the run. See [`fallow_api::EditorSessionParseCounts`].
    pub parse: fallow_api::EditorSessionParseCounts,
}

impl RunParseWork {
    fn add(&mut self, parse: fallow_api::EditorSessionParseCounts) {
        self.parse.modules_parsed += parse.modules_parsed;
        self.parse.disk_cache_hits += parse.disk_cache_hits;
        self.parse.modules_reused += parse.modules_reused;
    }
}

fn lock_store(store: &SharedSessionStore) -> std::sync::MutexGuard<'_, EditorSessionStore> {
    store
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Write the parse cache of sessions that the store no longer keeps.
pub fn flush_sessions(sessions: Vec<AnalysisSession>) {
    for session in sessions {
        session.flush_parse_cache();
    }
}

/// Load the config of a project root and walk the project.
///
/// # Errors
///
/// Returns the engine error message when the project config does not load.
pub fn load_project_session(
    project_root: &Path,
    key: &SessionKey,
) -> Result<AnalysisSession, String> {
    AnalysisSession::load_with_config_options(
        project_root,
        key.config_path.as_deref(),
        fallow_config::ConfigLoadOptions {
            allow_remote_extends: key.allow_remote_extends,
        },
        |config| {
            // Override the project config's production resolution when the
            // editor forwarded an explicit `fallow.production` (on/off).
            // Mirrors the CLI-driven sidebar receiving
            // `--production`/`--no-production`, so the two surfaces agree;
            // `None` leaves the project config in force (issue #1055).
            if let Some(production) = key.production_override {
                config.production = production;
            }
        },
    )
    .map_err(|error| error.to_string())
}

/// Run dead-code + duplicates analysis for a single project root, appending
/// findings to the merged accumulators and a status message to
/// `config_messages`. Extracted out of `run_analysis` to keep that method
/// under the 150-line clippy ceiling.
pub struct ProjectRootAnalysisInput<'a> {
    pub project_root: &'a Path,
    pub config_path: Option<&'a Path>,
    pub allow_remote_extends: bool,
    pub duplication_options: Option<&'a LspDuplicationOptions>,
    pub production_override: Option<bool>,
    pub inline_complexity_enabled: bool,
    pub type_aware_options: Option<&'a LspTypeAwareOptions>,
    pub type_aware_sessions: &'a Arc<Mutex<FxHashMap<PathBuf, fallow_api::TypeAwareSession>>>,
    pub type_aware_changes: &'a fallow_api::TypeAwareFileChanges,
    pub cancellation: &'a Arc<AtomicBool>,
    /// Set when a newer workspace event supersedes this run.
    pub run_cancellation: &'a Arc<AtomicBool>,
    pub changed_files: Option<&'a FxHashSet<PathBuf>>,
    pub sessions: &'a SharedSessionStore,
    pub parse_work: &'a mut RunParseWork,
    pub merged_analysis: &'a mut EditorAnalysisOutput,
    pub merged_inline_complexity: &'a mut Vec<InlineComplexityFinding>,
    pub config_messages: &'a mut Vec<(MessageType, String)>,
}

pub struct BlockingAnalysisInput {
    pub project_roots: Vec<PathBuf>,
    pub config_path: Option<PathBuf>,
    pub allow_remote_extends: bool,
    pub duplication_options: Option<LspDuplicationOptions>,
    pub production_override: Option<bool>,
    pub inline_complexity_enabled: bool,
    pub type_aware_options: Option<LspTypeAwareOptions>,
    pub type_aware_sessions: Arc<Mutex<FxHashMap<PathBuf, fallow_api::TypeAwareSession>>>,
    pub type_aware_changes: fallow_api::TypeAwareFileChanges,
    pub root: PathBuf,
    pub toplevel: Option<PathBuf>,
    pub changed_since: Option<String>,
    /// Shutdown flag, shared with the type-aware sessions.
    pub cancellation: Arc<AtomicBool>,
    /// Set when a newer workspace event supersedes this run. The run then
    /// stops at its next check and returns a cancelled error.
    pub run_cancellation: Arc<AtomicBool>,
    /// Sessions kept between runs. A disabled store gives each run a new
    /// session.
    pub sessions: SharedSessionStore,
}

pub struct BlockingAnalysisOutput {
    pub analysis: EditorAnalysisOutput,
    pub inline_complexity: Vec<InlineComplexityFinding>,
    pub config_messages: Vec<(MessageType, String)>,
    pub changed_message: Option<(MessageType, String)>,
    pub applied_changed_since: Option<String>,
    pub changed_since_scope: Option<ChangedSinceScopeStatus>,
    pub parse_work: RunParseWork,
}

#[derive(Debug)]
pub struct ProjectAnalysisError {
    project_root: PathBuf,
    message: String,
    cancelled: bool,
    /// An earlier project root of the run finished, so its type-aware pass
    /// may have used the pending changes.
    after_earlier_roots: bool,
}

impl ProjectAnalysisError {
    fn failed(project_root: &Path, message: String) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            message,
            cancelled: false,
            after_earlier_roots: false,
        }
    }

    /// The run stopped because a newer workspace event superseded it. A
    /// project root stops before its type-aware pass, never during it.
    pub fn cancelled(project_root: &Path) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            message: "a newer workspace event superseded the analysis".to_string(),
            cancelled: true,
            after_earlier_roots: false,
        }
    }

    /// Whether the run was cancelled rather than failed.
    pub const fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    /// Whether the run stopped before any type-aware pass could use the
    /// pending type-aware changes. The sidecar then never saw them.
    pub const fn type_aware_changes_unused(&self) -> bool {
        self.cancelled && !self.after_earlier_roots
    }

    pub const fn after_earlier_roots(mut self) -> Self {
        self.after_earlier_roots = true;
        self
    }
}

impl std::fmt::Display for ProjectAnalysisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "project analysis failed for {}: {}",
            self.project_root.display(),
            self.message
        )
    }
}

impl std::error::Error for ProjectAnalysisError {}

pub struct LspAnalysisSnapshot {
    pub results: AnalysisResults,
    pub duplication: DuplicationReport,
    pub inline_complexity: Vec<InlineComplexityFinding>,
}

impl LspAnalysisSnapshot {
    pub fn new(
        results: AnalysisResults,
        duplication: DuplicationReport,
        inline_complexity: Vec<InlineComplexityFinding>,
    ) -> Self {
        Self {
            results,
            duplication,
            inline_complexity,
        }
    }
}

pub fn analyze_project_root(
    input: &mut ProjectRootAnalysisInput<'_>,
) -> Result<(), ProjectAnalysisError> {
    let key = SessionKey {
        config_path: input.config_path.map(Path::to_path_buf),
        allow_remote_extends: input.allow_remote_extends,
        production_override: input.production_override,
    };
    let kept = lock_store(input.sessions).take(input.project_root, &key);
    let mut session = if let Some(mut session) = kept {
        session.refresh_discovery();
        session
    } else {
        match load_project_session(input.project_root, &key) {
            Ok(session) => {
                input.parse_work.sessions_loaded += 1;
                session
            }
            Err(e) => return analyze_project_root_config_fallback(input, &e),
        }
    };
    session.set_cancellation(Arc::clone(input.run_cancellation));

    let message = (
        MessageType::INFO,
        session.config_path().map_or_else(
            || {
                format!(
                    "no config file found for {}, using defaults",
                    input.project_root.display()
                )
            },
            |path| format!("loaded config: {}", path.display()),
        ),
    );

    input.config_messages.push(message);

    let duplicates_config = input.duplication_options.map_or_else(
        || session.config().duplicates.clone(),
        |options| options.merge_with(&session.config().duplicates),
    );
    let before = session.parse_counts();
    let result = run_typed_project_analysis(input, &session, &duplicates_config);
    input.parse_work.add(session.parse_counts().since(before));
    // A failed run may leave a session in a state that the next run should
    // not trust, so only a finished or cancelled run keeps it.
    let returned = match &result {
        Err(error) if !error.is_cancelled() => Some(session),
        _ => lock_store(input.sessions).put(input.project_root, key, session),
    };
    if let Some(session) = returned {
        session.flush_parse_cache();
    }
    result
}

/// Config-load failure path: record the warning, and when no explicit config
/// path was given, fall back to the path-based analysis + default duplication
/// scan so the editor still surfaces findings.
fn analyze_project_root_config_fallback(
    input: &mut ProjectRootAnalysisInput<'_>,
    err: &impl std::fmt::Display,
) -> Result<(), ProjectAnalysisError> {
    let detail = config_load_error_detail(input.project_root, input.config_path, err);
    if input.config_path.is_some() {
        return Err(ProjectAnalysisError::failed(input.project_root, detail));
    }
    input.config_messages.push((MessageType::WARNING, detail));
    let session = AnalysisSession::load_default(input.project_root)
        .with_cancellation(Arc::clone(input.run_cancellation));
    input.parse_work.sessions_loaded += 1;
    let before = session.parse_counts();
    let result = run_typed_project_analysis(input, &session, &DuplicatesConfig::default());
    input.parse_work.add(session.parse_counts().since(before));
    result
}

/// Run typed project analysis for a loaded config, with the optional
/// inline-complexity artifact retention when the client opted in, folding
/// results into the accumulators.
fn run_typed_project_analysis(
    input: &mut ProjectRootAnalysisInput<'_>,
    session: &AnalysisSession,
    duplicates_config: &DuplicatesConfig,
) -> Result<(), ProjectAnalysisError> {
    let mut output = session
        .analyze_project_with_changed_files(
            duplicates_config,
            input.inline_complexity_enabled,
            input.changed_files,
        )
        .map_err(|error| {
            if error.is_cancelled() {
                ProjectAnalysisError::cancelled(input.project_root)
            } else {
                ProjectAnalysisError::failed(input.project_root, error.to_string())
            }
        })?;
    // The type-aware pass shares a long-lived sidecar session, so it is not
    // stopped midway. A superseded run stops before it instead.
    if input.run_cancellation.load(Ordering::SeqCst) {
        return Err(ProjectAnalysisError::cancelled(input.project_root));
    }
    if !input.cancellation.load(Ordering::SeqCst)
        && let Some(options) = input.type_aware_options.filter(|options| options.enabled)
    {
        let type_aware = fallow_api::TypeAwareOptions {
            enabled: true,
            projects: options.projects.iter().map(PathBuf::from).collect(),
            require: options.require.unwrap_or_default(),
        };
        let changes = changes_for_project(input.type_aware_changes, input.project_root);
        let canonical_root = input
            .project_root
            .canonicalize()
            .unwrap_or_else(|_| input.project_root.to_path_buf());
        if let Err(message) = refine_type_aware_project(
            input.type_aware_sessions,
            TypeAwareProjectRefinement {
                canonical_root: &canonical_root,
                cancellation: input.cancellation,
                session,
                changes: changes.as_ref(),
                options: &type_aware,
                output: &mut output.dead_code,
            },
        ) {
            if type_aware.require == fallow_config::TypeAwareRequire::Complete {
                return Err(ProjectAnalysisError::failed(input.project_root, message));
            }
            input.config_messages.push((
                MessageType::WARNING,
                format!(
                    "type-aware refinement unavailable for {}: {message}; showing conservative syntactic findings",
                    input.project_root.display()
                ),
            ));
        }
    }
    // The type-aware pass reads `unused_files` as its set of unreachable
    // files, so the changed-files scope runs after it.
    session.apply_changed_files_scope(&mut output.dead_code, input.changed_files);
    if input.inline_complexity_enabled {
        input
            .merged_inline_complexity
            .extend(fallow_api::collect_inline_complexity(
                session.config(),
                &output.dead_code,
            ));
    }
    input.merged_analysis.merge_project_output(output);
    Ok(())
}

struct TypeAwareProjectRefinement<'a> {
    canonical_root: &'a Path,
    cancellation: &'a Arc<AtomicBool>,
    session: &'a AnalysisSession,
    changes: Option<&'a fallow_api::TypeAwareFileChanges>,
    options: &'a fallow_api::TypeAwareOptions,
    output: &'a mut fallow_api::EditorDeadCodeAnalysisOutput,
}

fn refine_type_aware_project(
    sessions: &Arc<Mutex<FxHashMap<PathBuf, fallow_api::TypeAwareSession>>>,
    refinement: TypeAwareProjectRefinement<'_>,
) -> Result<(), String> {
    let TypeAwareProjectRefinement {
        canonical_root,
        cancellation,
        session,
        changes,
        options,
        output,
    } = refinement;
    if cancellation.load(Ordering::SeqCst) {
        return Err("editor analysis is closing".to_string());
    }
    let mut sessions = sessions.lock().unwrap_or_else(|error| error.into_inner());
    if !sessions.contains_key(canonical_root) {
        let semantic_session = match fallow_api::TypeAwareSession::new_cancellable(
            canonical_root,
            Arc::clone(cancellation),
        ) {
            Ok(session) => session,
            Err(error) => {
                fallow_api::discard_unverified_semantic_candidates(&mut output.results);
                return Err(error.to_string());
            }
        };
        sessions.insert(canonical_root.to_path_buf(), semantic_session);
    }
    let semantic_session = sessions
        .get_mut(canonical_root)
        .ok_or_else(|| "semantic session registry lost its root entry".to_string())?;
    let result = session.refine_type_aware_dead_code_in_session(
        semantic_session,
        changes,
        options,
        &fallow_api::DeadCodeFilters::default(),
        output,
    );
    if let Err(error) = result {
        sessions.remove(canonical_root);
        return Err(error.message);
    }
    drop(sessions);
    Ok(())
}

fn changes_for_project(
    changes: &fallow_api::TypeAwareFileChanges,
    root: &Path,
) -> Option<fallow_api::TypeAwareFileChanges> {
    if changes.invalidate_all {
        return Some(fallow_api::TypeAwareFileChanges {
            invalidate_all: true,
            ..fallow_api::TypeAwareFileChanges::default()
        });
    }
    let relative = |paths: &[PathBuf]| {
        paths
            .iter()
            .filter_map(|path| path.strip_prefix(root).ok().map(Path::to_path_buf))
            .collect::<Vec<_>>()
    };
    let project_changes = fallow_api::TypeAwareFileChanges {
        changed: relative(&changes.changed),
        created: relative(&changes.created),
        deleted: relative(&changes.deleted),
        invalidate_all: false,
    };
    (!project_changes.changed.is_empty()
        || !project_changes.created.is_empty()
        || !project_changes.deleted.is_empty())
    .then_some(project_changes)
}

pub fn run_blocking_analysis(
    input: &BlockingAnalysisInput,
) -> Result<BlockingAnalysisOutput, ProjectAnalysisError> {
    let mut analysis = EditorAnalysisOutput::default();
    let mut inline_complexity = Vec::new();
    let mut config_messages: Vec<(MessageType, String)> =
        Vec::with_capacity(input.project_roots.len());
    let changed_scope = resolve_changed_since_scope(
        input.changed_since.as_deref(),
        input.toplevel.as_deref().unwrap_or(input.root.as_path()),
        &input.root,
    );
    let retired = lock_store(&input.sessions).retire(&input.project_roots);
    flush_sessions(retired);
    let mut parse_work = RunParseWork::default();
    for (index, project_root) in input.project_roots.iter().enumerate() {
        analyze_project_root(&mut ProjectRootAnalysisInput {
            project_root,
            config_path: input.config_path.as_deref(),
            allow_remote_extends: input.allow_remote_extends,
            duplication_options: input.duplication_options.as_ref(),
            production_override: input.production_override,
            inline_complexity_enabled: input.inline_complexity_enabled,
            type_aware_options: input.type_aware_options.as_ref(),
            type_aware_sessions: &input.type_aware_sessions,
            type_aware_changes: &input.type_aware_changes,
            cancellation: &input.cancellation,
            run_cancellation: &input.run_cancellation,
            changed_files: changed_scope.files.as_ref(),
            sessions: &input.sessions,
            parse_work: &mut parse_work,
            merged_analysis: &mut analysis,
            merged_inline_complexity: &mut inline_complexity,
            config_messages: &mut config_messages,
        })
        .map_err(|error| {
            if index == 0 {
                error
            } else {
                error.after_earlier_roots()
            }
        })?;
    }

    if let Some(changed_files) = changed_scope.files.as_ref() {
        analysis.filter_by_changed_files(changed_files, &input.root);
        fallow_api::filter_inline_complexity_by_changed_files(
            &mut inline_complexity,
            changed_files,
        );
    }

    Ok(BlockingAnalysisOutput {
        analysis,
        inline_complexity,
        config_messages,
        changed_message: changed_scope.message,
        applied_changed_since: changed_scope.applied_ref,
        changed_since_scope: changed_scope.status,
        parse_work,
    })
}

struct ChangedSinceScope {
    files: Option<FxHashSet<PathBuf>>,
    message: Option<(MessageType, String)>,
    applied_ref: Option<String>,
    status: Option<ChangedSinceScopeStatus>,
}

const MAX_CHANGED_SINCE_REASON_CHARS: usize = 160;

fn concise_changed_since_reason(raw: &str) -> String {
    let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= MAX_CHANGED_SINCE_REASON_CHARS {
        return normalized;
    }

    let mut truncated = normalized
        .chars()
        .take(MAX_CHANGED_SINCE_REASON_CHARS - 3)
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

fn resolve_changed_since_scope(
    changed_since: Option<&str>,
    toplevel: &Path,
    root: &Path,
) -> ChangedSinceScope {
    let Some(git_ref) = changed_since else {
        return ChangedSinceScope {
            files: None,
            message: None,
            applied_ref: None,
            status: None,
        };
    };

    match fallow_api::try_get_changed_files_with_toplevel(root, toplevel, git_ref) {
        Ok(changed) => {
            let count = changed.len();
            ChangedSinceScope {
                files: Some(changed),
                applied_ref: Some(git_ref.to_string()),
                status: Some(ChangedSinceScopeStatus {
                    requested_ref: git_ref.to_string(),
                    state: ChangedSinceScopeState::Applied,
                    reason: None,
                }),
                message: Some((
                    MessageType::INFO,
                    format!("changedSince '{git_ref}': scoped to {count} changed file(s)"),
                )),
            }
        }
        Err(err) => {
            let message_reason = err.describe();
            let reason = concise_changed_since_reason(&message_reason);
            ChangedSinceScope {
                files: None,
                applied_ref: None,
                status: Some(ChangedSinceScopeStatus {
                    requested_ref: git_ref.to_string(),
                    state: ChangedSinceScopeState::Dropped,
                    reason: Some(reason),
                }),
                message: Some((
                    MessageType::WARNING,
                    format!(
                        "changedSince '{git_ref}' ignored: {message_reason} (showing full-scope results)"
                    ),
                )),
            }
        }
    }
}
