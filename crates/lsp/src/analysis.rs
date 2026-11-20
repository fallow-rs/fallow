use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use fallow_api::{ChangeScope, ChangeScopeOwner, ChangeScopeRequest};
use fallow_api::{
    EditorAnalysisOutput, EditorAnalysisResults as AnalysisResults,
    EditorAnalysisSession as AnalysisSession, EditorDuplicationReport as DuplicationReport,
    EditorInlineComplexityFinding as InlineComplexityFinding, PackageChangeScope,
};
use fallow_config::{DuplicatesConfig, EnvSnapshot, RunInputs};
use ls_types::MessageType;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::config_patterns::{self, UnmatchedConfigPattern};
use crate::document_state::VersionSnapshot;
use crate::initialization::{LspDuplicationOptions, LspInitializationOptions, LspTypeAwareOptions};
use crate::path_utils::canonicalize_for_lsp;
use crate::protocol::{ChangedSinceScopeState, ChangedSinceScopeStatus, config_load_error_detail};
use crate::session_store::{ConfigSources, EditorSessionStore, SessionKey, TakenSession};

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

/// The session of a project root, with its config inputs.
pub struct LoadedSession {
    pub session: AnalysisSession,
    /// The config inputs, so a kept session can tell when its config
    /// changed.
    pub sources: ConfigSources,
    /// The warning of a discovered config that did not load. The session
    /// then uses the default config with the editor settings.
    pub fallback_warning: Option<String>,
}

/// Resolve the run of a project root and open its session. An explicit
/// editor `fallow.production` (on/off) acts as the CLI
/// `--production`/`--no-production` of the sidebar, so the two surfaces
/// agree; `None` leaves the project config in force (issue #1055).
///
/// # Errors
///
/// Returns the message for the user when the run does not resolve, for
/// example when an explicit `configPath` does not load.
pub fn load_project_session(
    project_root: &Path,
    inputs: &RunInputs,
) -> Result<LoadedSession, String> {
    let run = fallow_api::run::resolve_run(inputs).map_err(|err| {
        let explicit = inputs.config_path.as_ref().map(|given| {
            err.config_file()
                .map_or_else(|| project_root.join(&given.value), Path::to_path_buf)
        });
        config_load_error_detail(project_root, explicit.as_deref(), err.message())
    })?;
    let session = AnalysisSession::open(&run).map_err(|err| err.message)?;
    let fallback_warning = run
        .fallback_error()
        .map(|err| config_load_error_detail(project_root, None, err.message()));
    Ok(LoadedSession {
        session,
        sources: ConfigSources::of(run),
        fallback_warning,
    })
}

/// The log lines of the run notices other than the config fallback, such as
/// a shadowed config file: a warning for a notice that degrades the analysis.
fn notice_messages(
    project_root: &Path,
    notices: &[fallow_config::WorkspaceDiagnostic],
) -> impl Iterator<Item = (MessageType, String)> {
    notices
        .iter()
        .filter(|notice| {
            !matches!(
                notice.kind,
                fallow_config::WorkspaceDiagnosticKind::ConfigLoadFellBack { .. }
            )
        })
        .map(move |notice| {
            let level = if notice.degrades_analysis {
                MessageType::WARNING
            } else {
                MessageType::INFO
            };
            (
                level,
                format!("{}: {}", project_root.display(), notice.message),
            )
        })
}

/// The run inputs of a project root: the editor settings and the
/// environment of the server process now.
fn project_inputs(options: &LspInitializationOptions, project_root: &Path) -> RunInputs {
    crate::run_inputs::run_inputs(options, project_root, EnvSnapshot::capture())
}

/// The input of a prewarm: the project roots and the settings of the runs
/// that will reuse the sessions.
pub struct PrewarmInput {
    pub project_roots: Vec<PathBuf>,
    pub run_options: LspInitializationOptions,
    pub inline_complexity_enabled: bool,
    /// Shutdown flag. A set flag stops the parse, and the session is dropped.
    pub cancellation: Arc<AtomicBool>,
    pub sessions: SharedSessionStore,
}

/// Load and parse the session of each project root that the store does not
/// keep yet, and keep it for the first run. Analyzes and publishes nothing.
/// A root whose config does not load is skipped, so the first run reports
/// the error. Returns the number of sessions that the prewarm kept.
pub fn prewarm_sessions(input: &PrewarmInput) -> usize {
    let mut kept = 0;
    for project_root in &input.project_roots {
        if input.cancellation.load(Ordering::SeqCst) {
            break;
        }
        let inputs = project_inputs(&input.run_options, project_root);
        let key = SessionKey::of(&inputs);
        if lock_store(&input.sessions).keeps(project_root, &key) {
            continue;
        }
        // A fallback session is not kept, so the first run reports the error.
        let Ok(LoadedSession {
            mut session,
            sources,
            fallback_warning: None,
        }) = load_project_session(project_root, &inputs)
        else {
            continue;
        };
        session.set_cancellation(Arc::clone(&input.cancellation));
        if session.prewarm(input.inline_complexity_enabled).is_err() {
            continue;
        }
        let returned = lock_store(&input.sessions).put(project_root, key, sources, session);
        match returned {
            None => kept += 1,
            Some(session) => session.flush_parse_cache(),
        }
    }
    kept
}

/// Run dead-code + duplicates analysis for a single project root, appending
/// findings to the merged accumulators and a status message to
/// `config_messages`. Extracted out of `run_analysis` to keep that method
/// under the 150-line clippy ceiling.
pub struct ProjectRootAnalysisInput<'a> {
    pub project_root: &'a Path,
    /// The initialization options that give the run inputs of the root.
    pub run_options: &'a LspInitializationOptions,
    pub duplication_options: Option<&'a LspDuplicationOptions>,
    pub inline_complexity_enabled: bool,
    pub type_aware_options: Option<&'a LspTypeAwareOptions>,
    pub type_aware_sessions: &'a Arc<Mutex<FxHashMap<PathBuf, fallow_api::TypeAwareSession>>>,
    pub type_aware_changes: &'a fallow_api::TypeAwareFileChanges,
    pub cancellation: &'a Arc<AtomicBool>,
    /// Set when a newer workspace event supersedes this run.
    pub run_cancellation: &'a Arc<AtomicBool>,
    pub changed_files: Option<&'a FxHashSet<PathBuf>>,
    pub global_changed_since_requested: bool,
    pub sessions: &'a SharedSessionStore,
    pub parse_work: &'a mut RunParseWork,
    pub merged_analysis: &'a mut EditorAnalysisOutput,
    pub merged_inline_complexity: &'a mut Vec<InlineComplexityFinding>,
    pub config_messages: &'a mut Vec<(MessageType, String)>,
    pub package_scopes: &'a mut Vec<PackageChangeScope>,
    /// Config entries that had no effect in this run.
    pub unmatched_config_patterns: &'a mut Vec<UnmatchedConfigPattern>,
}

pub struct BlockingAnalysisInput {
    pub project_roots: Vec<PathBuf>,
    /// The initialization options that give the run inputs of each root.
    pub run_options: LspInitializationOptions,
    pub duplication_options: Option<LspDuplicationOptions>,
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
    pub unmatched_config_patterns: Vec<UnmatchedConfigPattern>,
    pub changed_message: Option<(MessageType, String)>,
    pub applied_changed_since: Option<String>,
    pub changed_since_scope: Option<ChangedSinceScopeStatus>,
    pub package_scopes: Vec<PackageChangeScope>,
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
    pub document_versions: VersionSnapshot,
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
            document_versions: VersionSnapshot::default(),
        }
    }

    #[must_use]
    pub fn with_document_versions(mut self, versions: VersionSnapshot) -> Self {
        self.document_versions = versions;
        self
    }
}

pub fn analyze_project_root(
    input: &mut ProjectRootAnalysisInput<'_>,
) -> Result<(), ProjectAnalysisError> {
    let inputs = project_inputs(input.run_options, input.project_root);
    let key = SessionKey::of(&inputs);
    let kept = lock_store(input.sessions).take(input.project_root, &key);
    let (mut session, sources, fallback_warning) = match kept {
        Some(TakenSession::SameSettings(mut session, sources)) if !sources.changed() => {
            session.refresh_discovery();
            (session, sources, None)
        }
        outdated => {
            if let Some(
                TakenSession::SameSettings(session, _) | TakenSession::OtherSettings(session),
            ) = outdated
            {
                session.flush_parse_cache();
            }
            let loaded = load_project_session(input.project_root, &inputs)
                .map_err(|detail| ProjectAnalysisError::failed(input.project_root, detail))?;
            input.parse_work.sessions_loaded += 1;
            (loaded.session, loaded.sources, loaded.fallback_warning)
        }
    };
    session.set_cancellation(Arc::clone(input.run_cancellation));

    // A fallback session is not kept, so each run reports the config error
    // until the config loads.
    let keep = fallback_warning.is_none();
    let message = fallback_warning.map_or_else(
        || {
            (
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
            )
        },
        |warning| (MessageType::WARNING, warning),
    );
    input.config_messages.push(message);
    input
        .config_messages
        .extend(notice_messages(input.project_root, sources.notices()));

    let duplicates_config = input.duplication_options.map_or_else(
        || session.config().duplicates.clone(),
        |options| session.config().duplicates.merged(&options.inputs()),
    );
    let before = session.parse_counts();
    let result = run_typed_project_analysis(input, &session, &duplicates_config);
    input.parse_work.add(session.parse_counts().since(before));
    // A failed run may leave a session in a state that the next run should
    // not trust, so only a finished or cancelled run keeps it.
    let returned = match &result {
        Err(error) if !error.is_cancelled() => Some(session),
        _ if !keep => Some(session),
        _ => lock_store(input.sessions).put(input.project_root, key, sources, session),
    };
    if let Some(session) = returned {
        session.flush_parse_cache();
    }
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
    if input.run_cancellation.load(Ordering::SeqCst) {
        return Err(ProjectAnalysisError::cancelled(input.project_root));
    }
    let change_scope = resolve_project_change_scope(input, session);
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
            enabled: Some(true),
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
    // A config pattern that matched nothing is a fact about the config, so
    // it becomes a diagnostic on the config file that declares it. The read
    // comes after the type-aware pass, because that pass compares its
    // findings with `ignoreFindings` too.
    input
        .unmatched_config_patterns
        .extend(config_patterns::collect(
            input.project_root,
            session.config_path().map(canonicalize_for_lsp).as_deref(),
            session.unmatched_config_patterns(),
        ));
    // The type-aware pass reads `unused_files` as its set of unreachable
    // files, so the change scope runs after it.
    session.apply_change_scope(&mut output, &change_scope);
    if input.inline_complexity_enabled {
        let mut findings =
            fallow_api::collect_inline_complexity(session.config(), &output.dead_code);
        fallow_api::filter_inline_complexity_by_change_scope(&mut findings, &change_scope);
        input.merged_inline_complexity.extend(findings);
    }
    input.merged_analysis.merge_project_output(output);
    if let Some(packages) = change_scope.packages() {
        input.package_scopes.push(packages.clone());
    }
    Ok(())
}

/// Resolve the change scope of one project. The editor keeps showing
/// findings when the package map cannot apply: a map error or a ref that Git
/// cannot resolve gives the full scope and a warning, as a config that fails
/// to load does.
fn resolve_project_change_scope(
    input: &mut ProjectRootAnalysisInput<'_>,
    session: &AnalysisSession,
) -> ChangeScope {
    let request = ChangeScopeRequest {
        owner: ChangeScopeOwner::Run,
        global_ref: input.global_changed_since_requested,
        files: input.changed_files,
        cache: None,
        no_package_baselines: input.run_options.package_baselines == Some(false),
    };
    let scope = match session.change_scope(request) {
        Ok(scope) => scope,
        Err(error) => {
            input.config_messages.push((
                MessageType::WARNING,
                format!(
                    "workspaces.changedSince was ignored for {}: {error}; showing findings in full scope",
                    input.project_root.display()
                ),
            ));
            return ChangeScope::default();
        }
    };
    if let Some(message) = scope.stand_down_message() {
        input
            .config_messages
            .push((MessageType::WARNING, message.to_owned()));
    }
    scope
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
    let mut package_scopes = Vec::new();
    let mut unmatched_config_patterns = Vec::new();
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
            run_options: &input.run_options,
            duplication_options: input.duplication_options.as_ref(),
            inline_complexity_enabled: input.inline_complexity_enabled,
            type_aware_options: input.type_aware_options.as_ref(),
            type_aware_sessions: &input.type_aware_sessions,
            type_aware_changes: &input.type_aware_changes,
            cancellation: &input.cancellation,
            run_cancellation: &input.run_cancellation,
            changed_files: changed_scope.files.as_ref(),
            global_changed_since_requested: input.changed_since.is_some(),
            sessions: &input.sessions,
            parse_work: &mut parse_work,
            merged_analysis: &mut analysis,
            merged_inline_complexity: &mut inline_complexity,
            config_messages: &mut config_messages,
            package_scopes: &mut package_scopes,
            unmatched_config_patterns: &mut unmatched_config_patterns,
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
        unmatched_config_patterns,
        changed_message: changed_scope.message,
        applied_changed_since: changed_scope.applied_ref,
        changed_since_scope: changed_scope.status,
        package_scopes,
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
