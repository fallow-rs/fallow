#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "tests use unwrap and expect to keep fixture setup concise"
    )
)]

mod analysis;
#[doc(hidden)]
pub mod bench_support;
mod code_actions;
mod code_lens;
mod diagnostic_filter;
mod diagnostics;
mod document_state;
mod hover;
mod initialization;
mod markdown;
mod path_utils;
mod position;
mod protocol;
mod publish;
mod schedule;
mod server_capabilities;
mod session_store;

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

#[allow(clippy::wildcard_imports, reason = "many LSP types used")]
use ls_types::*;
use tokio::sync::RwLock;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

fn type_aware_resolution_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    name.starts_with("tsconfig")
        || name.starts_with("jsconfig")
        || matches!(
            name,
            "package.json"
                | "package-lock.json"
                | "pnpm-lock.yaml"
                | "yarn.lock"
                | "bun.lock"
                | "bun.lockb"
                | "fallow.json"
                | "fallow.jsonc"
                | "fallow.yaml"
                | "fallow.yml"
                | "fallow.toml"
        )
        || fallow_config::CONFIG_FILE_NAMES.contains(&name)
        || name.ends_with(".d.ts")
}

fn type_aware_source_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts")
    )
}

fn invalidate_type_aware_changes(changes: &mut fallow_api::TypeAwareFileChanges) {
    changes.invalidate_all = true;
    changes.changed.clear();
    changes.created.clear();
    changes.deleted.clear();
}

fn type_aware_changes_pending(changes: &fallow_api::TypeAwareFileChanges) -> bool {
    changes.invalidate_all
        || !changes.changed.is_empty()
        || !changes.created.is_empty()
        || !changes.deleted.is_empty()
}

fn restore_failed_type_aware_changes(
    pending: &StdMutex<fallow_api::TypeAwareFileChanges>,
    attempted: &fallow_api::TypeAwareFileChanges,
) {
    if !type_aware_changes_pending(attempted) {
        return;
    }
    let mut pending = pending.lock().unwrap_or_else(|error| error.into_inner());
    invalidate_type_aware_changes(&mut pending);
}

/// Put the type-aware changes of a run that stopped before its type-aware
/// pass back into the pending set. The sidecar never saw them, so they stay
/// incremental, unless the merged set exceeds the queue capacity.
fn requeue_unused_type_aware_changes(
    pending: &StdMutex<fallow_api::TypeAwareFileChanges>,
    attempted: &fallow_api::TypeAwareFileChanges,
) {
    if !type_aware_changes_pending(attempted) {
        return;
    }
    merge_type_aware_changes(
        &mut pending.lock().unwrap_or_else(|error| error.into_inner()),
        attempted,
    );
}

fn merge_type_aware_changes(
    pending: &mut fallow_api::TypeAwareFileChanges,
    attempted: &fallow_api::TypeAwareFileChanges,
) {
    if attempted.invalidate_all {
        invalidate_type_aware_changes(pending);
        return;
    }
    if pending.invalidate_all {
        return;
    }
    for (source, target) in [
        (&attempted.changed, &mut pending.changed),
        (&attempted.created, &mut pending.created),
        (&attempted.deleted, &mut pending.deleted),
    ] {
        for path in source {
            if !target.contains(path) {
                target.push(path.clone());
            }
        }
    }
    let pending_count = pending.changed.len() + pending.created.len() + pending.deleted.len();
    if pending_count > MAX_PENDING_TYPE_AWARE_CHANGES {
        invalidate_type_aware_changes(pending);
    }
}

fn record_type_aware_file_change(
    changes: &mut fallow_api::TypeAwareFileChanges,
    path: PathBuf,
    change_type: FileChangeType,
) {
    if type_aware_resolution_file(&path) || !type_aware_source_file(&path) {
        invalidate_type_aware_changes(changes);
        return;
    }
    if changes.invalidate_all {
        return;
    }
    let pending_count = changes.changed.len() + changes.created.len() + changes.deleted.len();
    if pending_count >= MAX_PENDING_TYPE_AWARE_CHANGES {
        invalidate_type_aware_changes(changes);
        return;
    }
    let target = match change_type {
        FileChangeType::CREATED => &mut changes.created,
        FileChangeType::DELETED => &mut changes.deleted,
        _ => &mut changes.changed,
    };
    if !target.contains(&path) {
        target.push(path);
    }
}

use analysis::{
    BlockingAnalysisInput, BlockingAnalysisOutput, LspAnalysisSnapshot, ProjectAnalysisError,
    SharedSessionStore, run_blocking_analysis,
};
#[cfg(test)]
use analysis::{ProjectRootAnalysisInput, analyze_project_root};
use diagnostic_filter::attach_changed_since_data;
#[cfg(test)]
use diagnostic_filter::filter_disabled_diagnostics;
#[cfg(test)]
use document_state::uri_is_stale;
#[cfg(test)]
use document_state::{DocumentSnapshot, partition_document_snapshot};
use document_state::{DocumentState, VersionSnapshot};
#[cfg(test)]
use fallow_api::EditorAnalysisOutput;
#[cfg(test)]
use fallow_api::EditorAnalysisResults as AnalysisResults;
#[cfg(test)]
use fallow_api::EditorDuplicationReport as DuplicationReport;
#[cfg(test)]
use fallow_api::EditorInlineComplexityExceeded as InlineComplexityExceeded;
#[cfg(test)]
use fallow_api::EditorInlineComplexityFinding as InlineComplexityFinding;
use fallow_api::resolve_git_toplevel;
#[cfg(test)]
use fallow_config::DetectionMode;
#[cfg(test)]
use fallow_config::DuplicatesConfig;
use initialization::{
    LspDuplicationOptions, LspInitializationOptions, LspTypeAwareOptions,
    initialization_config_path, parse_initialization_options,
};
use path_utils::canonicalize_for_lsp;
#[cfg(test)]
use protocol::analysis_complete_params_for_test;
#[cfg(test)]
use protocol::config_load_error_detail;
use protocol::{
    AnalysisComplete, AnalysisCompleteInput, IssueTypeInfo, analysis_complete_params,
    diagnostic_issue_types,
};
use publish::{DiagnosticCache, PlannedPublish, PublishContext, plan_clears, plan_new_diagnostics};
use schedule::{RunOutcome, RunScheduler};
use server_capabilities::{
    build_server_capabilities, client_supports_watched_file_registration,
    client_supports_workspace_diagnostic_refresh,
};
use session_store::{SESSION_REUSE_ENV, session_input_file, session_reuse_allowed};

const WATCHED_FILES_REGISTRATION_ID: &str = "fallow-watched-files";
const WATCHED_FILES_METHOD: &str = "workspace/didChangeWatchedFiles";
const MAX_PENDING_TYPE_AWARE_CHANGES: usize = 2_048;
/// How long `shutdown` waits for the kept sessions to write their parse cache.
const SHUTDOWN_CACHE_FLUSH_GRACE: Duration = Duration::from_secs(1);
/// Source and resolution inputs a client watches. The fixed session input
/// names and the names the loader accepts are appended by
/// [`watched_file_globs`].
const WATCHED_FILE_GLOBS: &[&str] = &[
    "**/*.{js,jsx,mjs,cjs,ts,tsx,mts,cts}",
    "**/*.d.ts",
    "**/{tsconfig*,jsconfig*}.json",
];

/// Glob patterns registered for `workspace/didChangeWatchedFiles`.
///
/// Derived from the session input list and the loader's own config-file list,
/// so a name added there starts being watched without a second list to keep
/// in step.
fn watched_file_globs() -> Vec<String> {
    WATCHED_FILE_GLOBS
        .iter()
        .map(|pattern| (*pattern).to_string())
        .chain(
            session_store::SESSION_INPUT_FILE_NAMES
                .iter()
                .chain(fallow_config::CONFIG_FILE_NAMES)
                .map(|name| format!("**/{name}")),
        )
        .collect()
}

fn disabled_diagnostic_codes(options: &LspInitializationOptions) -> FxHashSet<String> {
    let muted_categories: FxHashSet<&str> = options
        .muted_categories
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(String::as_str)
        .collect();

    fallow_types::issue_meta::diagnostic_issue_metas()
        .filter(|issue_type| {
            muted_categories.contains(issue_type.code)
                || issue_type.config_key.is_some_and(|config_key| {
                    options
                        .issue_types
                        .as_ref()
                        .and_then(|issue_types| issue_types.get(config_key))
                        == Some(&false)
                })
        })
        .map(|issue_type| issue_type.code.to_string())
        .collect()
}

/// The blocking analysis of one run. The server uses
/// [`run_blocking_analysis`]. Tests swap in a runner that they control.
type AnalysisRunner = Arc<
    dyn Fn(
            &BlockingAnalysisInput,
        ) -> std::result::Result<BlockingAnalysisOutput, ProjectAnalysisError>
        + Send
        + Sync,
>;

/// The result of one blocking analysis, with the state the run started from.
struct CompletedRun<'a> {
    result: std::result::Result<
        std::result::Result<BlockingAnalysisOutput, ProjectAnalysisError>,
        tokio::task::JoinError,
    >,
    root: &'a Path,
    version_snapshot: &'a VersionSnapshot,
    analysis_epoch: u64,
    attempted_type_aware_changes: &'a fallow_api::TypeAwareFileChanges,
}

#[derive(Clone)]
struct FallowLspServer {
    client: Client,
    root: Arc<RwLock<Option<PathBuf>>>,
    analysis: Arc<RwLock<Option<LspAnalysisSnapshot>>>,
    previous_diagnostic_uris: Arc<RwLock<FxHashSet<Uri>>>,
    analysis_guard: Arc<tokio::sync::Mutex<()>>,
    /// Monotonic workspace event generation. A run records the epoch it
    /// started at, so a queued run can see that a finished run already
    /// covers the current epoch.
    analysis_epoch: Arc<AtomicU64>,
    /// Watched-file event generation. It changes only while the documents
    /// write lock is held, so a run that compares it under that lock sees
    /// every event that cleared a `known_clean` flag.
    disk_generation: Arc<AtomicU64>,
    /// Epoch of the last successfully applied analysis. `run_analysis` skips
    /// the run when the current epoch already completed, so a burst of
    /// workspace events queued on `analysis_guard` coalesces into one
    /// analysis instead of N serialized full runs. Starts at `u64::MAX`
    /// ("no epoch completed") so the epoch-0 startup analysis is never
    /// skipped.
    last_completed_epoch: Arc<AtomicU64>,
    /// Per-URI document state tracked from `did_open` / `did_change` /
    /// `did_close`. The `version` field is the LSP-supplied integer used by
    /// `run_analysis` to snapshot the document state at analysis start and
    /// by `publish_collected_diagnostics` to skip stale publishes; see
    /// `.claude/rules/lsp-server.md` for the staleness invariant.
    documents: Arc<RwLock<FxHashMap<Uri, DocumentState>>>,
    /// Guards the first automatic analysis. Startup `initialized` is too early
    /// for VS Code and VS Codium, which can show provisional counts before open
    /// documents and project state are ready. The first open or save runs the
    /// initial analysis instead.
    startup_analysis_started: Arc<AtomicBool>,
    /// Diagnostic codes suppressed by `initializationOptions.issueTypes` or
    /// `initializationOptions.mutedCategories`.
    disabled_diagnostic_codes: Arc<RwLock<FxHashSet<String>>>,
    /// Optional git ref from `initializationOptions.changedSince`. When set,
    /// analysis results and duplication reports are scoped to files changed
    /// since this ref, mirroring the CLI's `--changed-since`.
    changed_since: Arc<RwLock<Option<String>>>,
    /// Optional explicit config path from `initializationOptions.configPath`.
    /// Mirrors the CLI's `--config` flag for editor clients.
    config_path: Arc<RwLock<Option<PathBuf>>>,
    /// Per-client opt-in for trusted HTTPS config inheritance.
    allow_remote_extends: Arc<RwLock<bool>>,
    /// Optional duplication overrides from `initializationOptions.duplication`.
    /// VS Code sends these so live diagnostics match the sidebar CLI run.
    duplication_options: Arc<RwLock<Option<LspDuplicationOptions>>>,
    /// Optional production-mode override from `initializationOptions.production`.
    /// `Some(true)`/`Some(false)` force production on/off so the editor's
    /// diagnostics match the CLI-driven sidebar (which receives
    /// `--production`/`--no-production`); `None` defers to the project config,
    /// mirroring the CLI default. Without this the sidebar and editor squiggles
    /// disagree whenever `fallow.production` is set (issue #1055).
    production_override: Arc<RwLock<Option<bool>>>,
    /// Whether the client opted in to heuristic complexity code lenses.
    inline_complexity_enabled: Arc<RwLock<bool>>,
    /// Optional semantic TypeScript refinement for editor diagnostics.
    type_aware_options: Arc<RwLock<Option<LspTypeAwareOptions>>>,
    type_aware_sessions: Arc<StdMutex<FxHashMap<PathBuf, fallow_api::TypeAwareSession>>>,
    /// Project sessions kept between runs. See `session_store.rs`.
    editor_sessions: SharedSessionStore,
    /// `initializationOptions.prewarm`: parse the project at `initialized`.
    prewarm: Arc<AtomicBool>,
    pending_type_aware_changes: Arc<StdMutex<fallow_api::TypeAwareFileChanges>>,
    /// Canonical git toplevel for the workspace `root`, resolved on first
    /// analysis run and reused thereafter. Cached so we do not pay for an
    /// extra `git rev-parse --show-toplevel` subprocess on every save.
    /// `None` means "not resolved yet"; `Some(Err)` is not stored, callers
    /// fall back to the workspace root and the existing per-call git error
    /// surfacing in `try_get_changed_files`.
    ///
    /// Assumption: the workspace `root` is immutable for the lifetime of
    /// the LSP instance. All mainstream LSP clients (VS Code, Helix,
    /// Neovim) restart the server on workspace folder change, so the
    /// cache cannot serve stale data in practice. If a future client
    /// reuses the server across workspace switches via
    /// `workspace/didChangeWorkspaceFolders`, that handler must clear
    /// this cache (and `self.root`) to avoid stale path joins.
    git_toplevel: Arc<RwLock<Option<PathBuf>>>,
    /// Cached diagnostics for pull-model support (textDocument/diagnostic)
    cached_diagnostics: Arc<RwLock<DiagnosticCache>>,
    /// Set to `true` the first time the client issues a `textDocument/diagnostic`
    /// request. This is the only reliable signal that a client genuinely
    /// consumes pull diagnostics. Advertising `workspace.diagnostics.refreshSupport`
    /// is NOT sufficient: refresh-capable clients can still choose not to pull
    /// for a given document. Keying push-suppression on the advertised capability
    /// silently blanked open-file diagnostics for such clients. Push-suppression,
    /// the `did_open` push clear, and the `workspace/diagnostic/refresh` nudge
    /// therefore all key on THIS flag so push-only clients keep receiving
    /// open-file diagnostics.
    client_pulls: Arc<AtomicBool>,
    /// Whether the client accepts dynamic watched-file registration.
    watched_file_registration: Arc<AtomicBool>,
    /// Set by `shutdown()`. `run_analysis` checks this at the top and
    /// before publishing diagnostics so a closing client does not receive
    /// spurious post-shutdown publishes. The 250ms grace on the
    /// `analysis_guard` in `shutdown()` lets the current `spawn_blocking`
    /// settle, but does NOT interrupt rayon work already in flight; that
    /// work runs to completion on the blocking thread pool and its
    /// results are dropped. See issue #477.
    cancellation: Arc<AtomicBool>,
    /// Debounce and cancellation state of the analysis runs. See
    /// `schedule.rs`.
    scheduler: Arc<StdMutex<RunScheduler>>,
    /// A debounce task waits for the scheduler deadline. One at a time.
    debounce_armed: Arc<AtomicBool>,
    analysis_runner: AnalysisRunner,
}

impl LanguageServer for FallowLspServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let root = params
            .workspace_folders
            .as_deref()
            .and_then(|fs| fs.first())
            .and_then(|f| f.uri.to_file_path().map(|path| path.into_owned()))
            .or_else(|| {
                #[expect(
                    deprecated,
                    reason = "root_uri remains a fallback for legacy LSP clients"
                )]
                params
                    .root_uri
                    .and_then(|u| u.to_file_path().map(|path| path.into_owned()))
            });
        let canonical_root = root.map(|path| canonicalize_for_lsp(&path));
        if let Some(path) = &canonical_root {
            *self.root.write().await = Some(path.clone());
        }

        if let Some(opts) = &params.initialization_options {
            let parsed_options = parse_initialization_options(Some(opts));
            *self.disabled_diagnostic_codes.write().await =
                disabled_diagnostic_codes(&parsed_options);

            if let Some(git_ref) = parsed_options.changed_since.as_deref() {
                let trimmed = git_ref.trim();
                *self.changed_since.write().await = if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                };
            }

            *self.config_path.write().await =
                initialization_config_path(opts, canonical_root.as_deref());
            *self.allow_remote_extends.write().await = parsed_options.allow_remote_extends;
            *self.duplication_options.write().await = parsed_options.duplication;
            *self.production_override.write().await = parsed_options.production;
            *self.inline_complexity_enabled.write().await = parsed_options
                .health
                .and_then(|health| health.inline_complexity)
                .unwrap_or(false);
            *self.type_aware_options.write().await = parsed_options.type_aware;
            self.prewarm.store(parsed_options.prewarm, Ordering::SeqCst);
        }

        let advertise_pull_diagnostics =
            client_supports_workspace_diagnostic_refresh(&params.capabilities);
        let watched_file_registration =
            client_supports_watched_file_registration(&params.capabilities);
        self.watched_file_registration
            .store(watched_file_registration, Ordering::SeqCst);
        // A kept session learns about a changed config input only through
        // watched-file events, so reuse needs a client that sends them.
        let reuse = watched_file_registration
            && session_reuse_allowed(std::env::var(SESSION_REUSE_ENV).ok().as_deref());
        let released = self.lock_sessions().set_enabled(reuse);
        analysis::flush_sessions(released);

        Ok(InitializeResult {
            capabilities: build_server_capabilities(advertise_pull_diagnostics),
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        if self.watched_file_registration.load(Ordering::SeqCst) {
            let watchers = watched_file_globs()
                .into_iter()
                .map(|pattern| FileSystemWatcher {
                    glob_pattern: GlobPattern::String(pattern),
                    kind: None,
                })
                .collect();
            let options = DidChangeWatchedFilesRegistrationOptions { watchers };
            let registration = Registration {
                id: WATCHED_FILES_REGISTRATION_ID.to_string(),
                method: WATCHED_FILES_METHOD.to_string(),
                register_options: serde_json::to_value(options).ok(),
            };
            if let Err(error) = self.client.register_capability(vec![registration]).await {
                self.client
                    .log_message(
                        MessageType::WARNING,
                        format!("could not register watched files: {error}"),
                    )
                    .await;
            }
        }
        self.client
            .log_message(MessageType::INFO, "fallow LSP server initialized")
            .await;
        if self.prewarm.load(Ordering::SeqCst) {
            self.spawn_prewarm().await;
        }
    }

    /// Cooperative shutdown.
    ///
    /// Sets the `cancellation` flag so any in-flight `run_analysis`
    /// short-circuits before publishing diagnostics, and awaits the
    /// `analysis_guard` for up to 250ms so a freshly-started blocking
    /// task can settle. NOTE: `tokio::task::spawn_blocking` is not
    /// interruptible; rayon work already running on the blocking thread
    /// pool continues to natural completion and its results are dropped.
    /// Active type-aware sidecars are terminated before the grace wait so
    /// semantic analysis does not keep the editor process alive.
    /// The grace is for quiescence, not for cancellation. See issue #477.
    async fn shutdown(&self) -> Result<()> {
        self.cancellation.store(true, Ordering::SeqCst);
        self.lock_scheduler().cancel_running();
        fallow_api::terminate_active_type_aware_sidecars();
        let _ = tokio::time::timeout(Duration::from_millis(250), self.analysis_guard.lock()).await;
        fallow_api::terminate_active_type_aware_sidecars();
        if let Ok(mut sessions) = self.type_aware_sessions.try_lock() {
            sessions.clear();
        }
        // Kept sessions hold parses that the persisted cache does not have
        // yet. The write is atomic, so an exit during it loses only the
        // update, never the cache file.
        // Turning reuse off also makes a run that is still in flight write
        // its own session to the cache instead of putting it back.
        let kept = self.lock_sessions().set_enabled(false);
        let flush = tokio::task::spawn_blocking(move || analysis::flush_sessions(kept));
        let _ = tokio::time::timeout(SHUTDOWN_CACHE_FLUSH_GRACE, flush).await;
        Ok(())
    }

    /// Pull-model diagnostic handler (`textDocument/diagnostic`, LSP 3.17).
    /// Returns cached diagnostics for the requested document.
    async fn diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> Result<DocumentDiagnosticReportResult> {
        let uri = params.text_document.uri;

        // The first pull request proves this client genuinely consumes pull
        // diagnostics. On that transition, clear any push-model
        // diagnostics emitted for open documents during startup (before the
        // first pull) so they do not double with the pull namespace in clients
        // like Neovim that surface both. The client re-pulls each open buffer,
        // so the pull namespace stays authoritative.
        if !self.client_pulls.swap(true, Ordering::SeqCst) {
            let open_uris: Vec<Uri> = self.documents.read().await.keys().cloned().collect();
            for open_uri in open_uris {
                self.client
                    .publish_diagnostics(open_uri, vec![], None)
                    .await;
            }
        }

        let items = self
            .cached_diagnostics
            .read()
            .await
            .get(&uri)
            .cloned()
            .unwrap_or_default();
        Ok(DocumentDiagnosticReportResult::Report(
            DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                related_documents: None,
                full_document_diagnostic_report: FullDocumentDiagnosticReport {
                    result_id: None,
                    items,
                },
            }),
        ))
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.mark_document_saved(&params.text_document.uri).await;
        if let Some(path) = params.text_document.uri.to_file_path() {
            if session_input_file(&path) {
                self.lock_sessions().mark_stale();
            }
            let mut changes = self
                .pending_type_aware_changes
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            record_type_aware_file_change(&mut changes, path.into_owned(), FileChangeType::CHANGED);
        }
        self.startup_analysis_started.store(true, Ordering::SeqCst);
        self.note_workspace_event();
    }

    async fn did_change_configuration(&self, _: DidChangeConfigurationParams) {
        self.lock_sessions().mark_stale();
        {
            let mut changes = self
                .pending_type_aware_changes
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            invalidate_type_aware_changes(&mut changes);
        }
        self.note_workspace_event();
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        self.mark_documents_changed_on_disk(params.changes.iter().map(|change| &change.uri))
            .await;
        {
            let mut changes = self
                .pending_type_aware_changes
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            for change in params.changes {
                let Some(path) = change.uri.to_file_path() else {
                    continue;
                };
                if session_input_file(&path) {
                    self.lock_sessions().mark_stale();
                }
                record_type_aware_file_change(&mut changes, path.into_owned(), change.typ);
            }
        }
        self.note_workspace_event();
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let TextDocumentItem {
            uri, version, text, ..
        } = params.text_document;
        self.documents
            .write()
            .await
            .insert(uri.clone(), DocumentState::new(version, text));

        if self.client_pulls.load(Ordering::SeqCst) {
            self.client
                .publish_diagnostics(uri, vec![], Some(version))
                .await;
            self.spawn_diagnostic_refresh();
        }

        if !self.startup_analysis_started.swap(true, Ordering::SeqCst) {
            self.spawn_analysis();
        }
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().last() {
            self.documents.write().await.insert(
                params.text_document.uri,
                DocumentState::new(params.text_document.version, change.text),
            );
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.write().await.remove(&uri);
        // For a pull client, `didOpen` and the first pull cleared the push
        // diagnostics of an open document, and a closed document gets only
        // pushes. The next run must push its diagnostics again.
        self.cached_diagnostics.write().await.forget_push(&uri);
    }

    #[expect(
        clippy::significant_drop_tightening,
        reason = "RwLock guard scope is intentional"
    )]
    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let analysis = self.analysis.read().await;
        let Some(analysis) = analysis.as_ref() else {
            return Ok(None);
        };

        let uri = &params.text_document.uri;
        let Some(file_path) = uri.to_file_path() else {
            return Ok(None);
        };

        let file_content = self.code_action_file_content(uri, &file_path).await;
        let file_lines: Vec<&str> = file_content.lines().collect();
        let root = self.root.read().await.clone();

        Ok(code_actions::build_code_action_response(
            code_actions::CodeActionInput::new(
                &analysis.results,
                root.as_deref(),
                &file_path,
                uri,
                &params.range,
                &file_lines,
            ),
        ))
    }

    #[expect(
        clippy::significant_drop_tightening,
        reason = "RwLock guard scope is intentional"
    )]
    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let analysis = self.analysis.read().await;
        let Some(analysis) = analysis.as_ref() else {
            return Ok(None);
        };

        let Some(file_path) = params.text_document.uri.to_file_path() else {
            return Ok(None);
        };

        let lenses = code_lens::build_code_lenses(code_lens::CodeLensInput::new(
            &analysis.results,
            &analysis.inline_complexity,
            &file_path,
            &params.text_document.uri,
        ));

        if lenses.is_empty() {
            Ok(None)
        } else {
            Ok(Some(lenses))
        }
    }

    #[expect(
        clippy::significant_drop_tightening,
        reason = "RwLock guard scope is intentional"
    )]
    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let analysis = self.analysis.read().await;
        let Some(analysis) = analysis.as_ref() else {
            return Ok(None);
        };

        let uri = &params.text_document_position_params.text_document.uri;
        let Some(file_path) = uri.to_file_path() else {
            return Ok(None);
        };

        let position = params.text_document_position_params.position;

        Ok(hover::build_hover(hover::HoverInput::new(
            &analysis.results,
            &analysis.duplication,
            &file_path,
            position,
        )))
    }
}

impl FallowLspServer {
    fn new(client: Client) -> Self {
        Self {
            client,
            root: Arc::new(RwLock::new(None)),
            analysis: Arc::new(RwLock::new(None)),
            previous_diagnostic_uris: Arc::new(RwLock::new(FxHashSet::default())),
            analysis_guard: Arc::new(tokio::sync::Mutex::new(())),
            analysis_epoch: Arc::new(AtomicU64::new(0)),
            disk_generation: Arc::new(AtomicU64::new(0)),
            last_completed_epoch: Arc::new(AtomicU64::new(u64::MAX)),
            documents: Arc::new(RwLock::new(FxHashMap::default())),
            startup_analysis_started: Arc::new(AtomicBool::new(false)),
            disabled_diagnostic_codes: Arc::new(RwLock::new(FxHashSet::default())),
            changed_since: Arc::new(RwLock::new(None)),
            config_path: Arc::new(RwLock::new(None)),
            allow_remote_extends: Arc::new(RwLock::new(false)),
            duplication_options: Arc::new(RwLock::new(None)),
            production_override: Arc::new(RwLock::new(None)),
            inline_complexity_enabled: Arc::new(RwLock::new(false)),
            type_aware_options: Arc::new(RwLock::new(None)),
            type_aware_sessions: Arc::new(StdMutex::new(FxHashMap::default())),
            editor_sessions: Arc::default(),
            prewarm: Arc::new(AtomicBool::new(false)),
            pending_type_aware_changes: Arc::new(StdMutex::new(
                fallow_api::TypeAwareFileChanges::default(),
            )),
            git_toplevel: Arc::new(RwLock::new(None)),
            cached_diagnostics: Arc::new(RwLock::new(DiagnosticCache::default())),
            client_pulls: Arc::new(AtomicBool::new(false)),
            watched_file_registration: Arc::new(AtomicBool::new(false)),
            cancellation: Arc::new(AtomicBool::new(false)),
            scheduler: Arc::new(StdMutex::new(RunScheduler::default())),
            debounce_armed: Arc::new(AtomicBool::new(false)),
            analysis_runner: Arc::new(run_blocking_analysis),
        }
    }

    async fn code_action_file_content(&self, uri: &Uri, file_path: &Path) -> String {
        let documents = self.documents.read().await;
        documents.get(uri).map_or_else(
            || std::fs::read_to_string(file_path).unwrap_or_default(),
            |state| state.text.clone(),
        )
    }

    #[expect(
        clippy::unused_async,
        reason = "tower-lsp-server custom_method handlers are async methods"
    )]
    async fn issue_types(&self) -> Result<Vec<IssueTypeInfo>> {
        Ok(diagnostic_issue_types())
    }

    /// Re-drive `workspace/diagnostic/refresh` on demand.
    ///
    /// The editor's mute toggle changes only the client-side diagnostic filter
    /// (no server round-trip), so open-file pull diagnostics never re-render
    /// until the next edit. The client-side re-pull (`triggerPullDiagnosticRefresh`)
    /// is gated per document by `getProvider(document)`, which can silently
    /// match nothing; the server-driven refresh fires every registered provider
    /// via `getAllProviders()`, the SAME path proven to re-render after analysis
    /// and on `did_open`. Routing the un-hide through here makes revealing
    /// findings reliable, not best-effort (discussion #287).
    ///
    /// No-op for push-only clients: without pull diagnostics the editor
    /// re-publishes the push collection from its own cache, so a
    /// `workspace/diagnostic/refresh` would do nothing useful.
    #[expect(
        clippy::unused_async,
        reason = "tower-lsp-server custom_method handlers are async methods"
    )]
    async fn refresh_diagnostics(&self) -> Result<()> {
        if self.client_pulls.load(Ordering::SeqCst) {
            self.spawn_diagnostic_refresh();
        }
        Ok(())
    }

    /// Run an analysis without blocking the triggering notification handler.
    ///
    /// tower-lsp-server dispatches requests and notifications through one
    /// small concurrency-limited pool, so awaiting a full workspace analysis
    /// inline parks a dispatch slot for the whole run; a burst of workspace
    /// events would exhaust the pool and freeze `didChange`, hover, code
    /// actions, and shutdown behind serialized analyses. `analysis_guard`
    /// still prevents overlapping runs and `last_completed_epoch` coalesces
    /// queued runs whose epoch already completed.
    fn spawn_analysis(&self) {
        let server = self.clone();
        tokio::spawn(async move {
            server.run_analysis().await;
        });
    }

    /// Parse the project in the background so the first run starts warm.
    ///
    /// The prewarm takes the analysis slot before this returns, so every run
    /// waits for it and then reuses its sessions. It publishes nothing and
    /// leaves the startup gate armed: the first open still starts the first
    /// run, for the reason on `startup_analysis_started`. It runs only when
    /// sessions are kept and the workspace root has a `package.json`.
    async fn spawn_prewarm(&self) {
        let Some(root) = self.root.read().await.clone() else {
            return;
        };
        if !self.lock_sessions().is_enabled() || !root.join("package.json").is_file() {
            return;
        }
        let slot = Arc::clone(&self.analysis_guard).lock_owned().await;
        let input = analysis::PrewarmInput {
            project_roots: find_project_roots(&root),
            key: session_store::SessionKey {
                config_path: self.config_path.read().await.clone(),
                allow_remote_extends: *self.allow_remote_extends.read().await,
                production_override: *self.production_override.read().await,
            },
            inline_complexity_enabled: *self.inline_complexity_enabled.read().await,
            cancellation: Arc::clone(&self.cancellation),
            sessions: Arc::clone(&self.editor_sessions),
        };
        let client = self.client.clone();
        tokio::spawn(async move {
            let kept = tokio::task::spawn_blocking(move || analysis::prewarm_sessions(&input))
                .await
                .unwrap_or(0);
            drop(slot);
            client
                .log_message(
                    MessageType::INFO,
                    format!("fallow prewarmed {kept} project session(s)"),
                )
                .await;
        });
    }

    fn lock_sessions(&self) -> std::sync::MutexGuard<'_, session_store::EditorSessionStore> {
        self.editor_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn lock_scheduler(&self) -> std::sync::MutexGuard<'_, RunScheduler> {
        self.scheduler
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    /// A save, watched-file change or configuration change arrived. Bump the
    /// epoch, let the scheduler cancel a superseded run, and make sure a
    /// debounce task waits for the next run.
    fn note_workspace_event(&self) {
        {
            // One lock for both steps, so a run that starts in between
            // cannot cover the new epoch while missing the pending event.
            let mut scheduler = self.lock_scheduler();
            self.analysis_epoch.fetch_add(1, Ordering::SeqCst);
            scheduler.record_event(tokio::time::Instant::now());
        }
        self.spawn_debounced_analysis();
    }

    /// Start a task that runs the analysis at the scheduler deadline, unless
    /// such a task already waits.
    fn spawn_debounced_analysis(&self) {
        if self.debounce_armed.swap(true, Ordering::SeqCst) {
            return;
        }
        let server = self.clone();
        tokio::spawn(async move {
            server.debounce_then_run().await;
        });
    }

    async fn debounce_then_run(&self) {
        loop {
            let deadline = self.lock_scheduler().run_deadline();
            match deadline {
                Some(deadline) if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep_until(deadline).await;
                }
                Some(_) => {
                    // Disarm first: an event during the run arms the next task.
                    self.debounce_armed.store(false, Ordering::SeqCst);
                    self.run_analysis().await;
                    return;
                }
                None => {
                    self.debounce_armed.store(false, Ordering::SeqCst);
                    // An event between the check above and the disarm saw the
                    // task as armed and did not start one.
                    let pending = self.lock_scheduler().run_deadline().is_some();
                    if !pending || self.debounce_armed.swap(true, Ordering::SeqCst) {
                        return;
                    }
                }
            }
        }
    }

    /// Resolve the canonical git toplevel for `root`, populating the cache
    /// on first call. Returns `None` if the workspace is not in a git
    /// repository or git is unavailable; callers should fall back to
    /// treating the workspace root as the toplevel for path joining.
    ///
    /// On the first successful resolution, emits a one-line WARN log when
    /// the toplevel differs from `root`. Doing the warning here (instead
    /// of on every `run_analysis`) means the user sees the message exactly
    /// once per LSP session in monorepo subdirectory workspaces. Without
    /// this gating the Output panel would fill with the same line every
    /// 500ms while the user works.
    async fn resolved_git_toplevel(&self, root: &Path) -> Option<PathBuf> {
        let cached = self.git_toplevel.read().await.clone();
        if let Some(t) = cached {
            return Some(t);
        }
        match resolve_git_toplevel(root) {
            Ok(t) => {
                if t.as_path() != root {
                    self.client
                        .log_message(
                            MessageType::WARNING,
                            format!(
                                "fallow workspace root ({}) is a subdirectory of git toplevel ({}). \
                                 Diagnostics for files outside the workspace are not produced; the \
                                 changedSince filter joins paths against the toplevel.",
                                root.display(),
                                t.display()
                            ),
                        )
                        .await;
                }
                *self.git_toplevel.write().await = Some(t.clone());
                Some(t)
            }
            Err(_) => None,
        }
    }

    async fn run_analysis(&self) {
        if self.cancellation.load(Ordering::SeqCst) {
            return;
        }

        let root = self.root.read().await.clone();
        let Some(root) = root else { return };

        let _guard = self.analysis_guard.lock().await;
        if self.cancellation.load(Ordering::SeqCst) {
            return;
        }

        let (analysis_epoch, run_cancellation) = {
            let mut scheduler = self.lock_scheduler();
            let analysis_epoch = self.analysis_epoch.load(Ordering::SeqCst);
            if self.last_completed_epoch.load(Ordering::SeqCst) == analysis_epoch {
                scheduler.clear_pending();
                return;
            }
            (analysis_epoch, scheduler.start_run())
        };

        let version_snapshot = self.snapshot_document_versions().await;

        self.client
            .log_message(MessageType::INFO, "Running fallow analysis...")
            .await;

        let project_roots = find_project_roots(&root);

        self.client
            .log_message(MessageType::INFO, "Analyzing workspace root")
            .await;

        let changed_since = self.changed_since.read().await.clone();
        let config_path = self.config_path.read().await.clone();
        let allow_remote_extends = *self.allow_remote_extends.read().await;
        let duplication_options = self.duplication_options.read().await.clone();
        let production_override = *self.production_override.read().await;
        let inline_complexity_enabled = *self.inline_complexity_enabled.read().await;
        let type_aware_options = self.type_aware_options.read().await.clone();
        let type_aware_sessions = Arc::clone(&self.type_aware_sessions);
        let type_aware_changes = {
            let mut pending = self
                .pending_type_aware_changes
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            std::mem::take(&mut *pending)
        };
        let attempted_type_aware_changes = type_aware_changes.clone();

        let resolved_toplevel = self.resolved_git_toplevel(&root).await;
        let blocking_root = root.clone();
        let blocking_toplevel = resolved_toplevel.clone();
        let cancellation = Arc::clone(&self.cancellation);
        let runner = Arc::clone(&self.analysis_runner);
        let sessions = Arc::clone(&self.editor_sessions);

        let join_result = tokio::task::spawn_blocking(move || {
            let input = BlockingAnalysisInput {
                project_roots,
                config_path,
                allow_remote_extends,
                duplication_options,
                production_override,
                inline_complexity_enabled,
                type_aware_options,
                type_aware_sessions,
                type_aware_changes,
                root: blocking_root,
                toplevel: blocking_toplevel,
                changed_since,
                cancellation,
                run_cancellation,
                sessions,
            };
            runner(&input)
        })
        .await;

        let outcome = self
            .complete_run(CompletedRun {
                result: join_result,
                root: &root,
                version_snapshot: &version_snapshot,
                analysis_epoch,
                attempted_type_aware_changes: &attempted_type_aware_changes,
            })
            .await;
        self.lock_scheduler().finish_run(outcome);
    }

    /// Publish a finished run, or report a cancelled or failed one. A run
    /// that did not finish returns its type-aware changes to the pending set:
    /// as they were when no type-aware pass used them, else as a full
    /// invalidation.
    async fn complete_run(&self, run: CompletedRun<'_>) -> RunOutcome {
        let (level, message, outcome) = match run.result {
            // A finished run publishes even when newer events arrived during
            // it. The per-URI staleness check keeps its results off buffers
            // that changed since the run started, and the newer events have
            // their own run. Without this, autosave faster than the analysis
            // would discard every run.
            Ok(Ok(output)) => {
                self.apply_analysis_output(output, run.root, run.version_snapshot)
                    .await;
                self.last_completed_epoch
                    .store(run.analysis_epoch, Ordering::SeqCst);
                return RunOutcome::Published;
            }
            Ok(Err(error)) if error.is_cancelled() => {
                if error.type_aware_changes_unused() {
                    requeue_unused_type_aware_changes(
                        &self.pending_type_aware_changes,
                        run.attempted_type_aware_changes,
                    );
                } else {
                    restore_failed_type_aware_changes(
                        &self.pending_type_aware_changes,
                        run.attempted_type_aware_changes,
                    );
                }
                (
                    MessageType::INFO,
                    "Cancelled a fallow analysis that a newer workspace event superseded"
                        .to_string(),
                    RunOutcome::Cancelled,
                )
            }
            Ok(Err(error)) => {
                restore_failed_type_aware_changes(
                    &self.pending_type_aware_changes,
                    run.attempted_type_aware_changes,
                );
                (
                    MessageType::ERROR,
                    format!("Analysis failed: {error}"),
                    RunOutcome::Failed,
                )
            }
            Err(error) => {
                restore_failed_type_aware_changes(
                    &self.pending_type_aware_changes,
                    run.attempted_type_aware_changes,
                );
                (
                    MessageType::ERROR,
                    format!("Analysis failed: {error}"),
                    RunOutcome::Failed,
                )
            }
        };
        self.client.log_message(level, message).await;
        outcome
    }

    /// Snapshot every open document's version + disk-match state at analysis
    /// entry, used by `publish_collected_diagnostics` for the staleness check.
    ///
    /// A known-clean document needs no file read. The other documents are
    /// read on the blocking pool after the documents lock is dropped, and a
    /// confirmed match is remembered for that document version.
    async fn snapshot_document_versions(&self) -> VersionSnapshot {
        let (mut snapshot, checks, generation) = self.partition_documents().await;
        if checks.is_empty() {
            return snapshot;
        }
        let checked = tokio::task::spawn_blocking(move || document_state::check_disk(checks))
            .await
            .unwrap_or_default();
        self.remember_disk_matches(checked, generation, &mut snapshot)
            .await;
        snapshot
    }

    /// Split the open documents into known-clean snapshots and pending disk
    /// reads, with the disk generation at that moment.
    async fn partition_documents(
        &self,
    ) -> (VersionSnapshot, Vec<document_state::PendingDiskCheck>, u64) {
        let documents = self.documents.read().await;
        let (snapshot, checks) = document_state::partition_document_snapshot(&documents);
        let generation = self.disk_generation.load(Ordering::SeqCst);
        drop(documents);
        (snapshot, checks, generation)
    }

    /// Add the disk reads to `snapshot`, and mark a matched document clean
    /// for its version when no watched-file event arrived after
    /// `generation_before_reads`.
    async fn remember_disk_matches(
        &self,
        checked: Vec<(Uri, document_state::DocumentSnapshot)>,
        generation_before_reads: u64,
        snapshot: &mut VersionSnapshot,
    ) {
        let mut documents = self.documents.write().await;
        let reads_are_current =
            self.disk_generation.load(Ordering::SeqCst) == generation_before_reads;
        for (uri, state) in checked {
            if reads_are_current
                && state.matches_disk
                && let Some(live) = documents.get_mut(&uri)
                && live.version == state.version
            {
                live.known_clean = true;
            }
            snapshot.insert(uri, state);
        }
        drop(documents);
    }

    /// The client saved `uri`, so its buffer equals the file on disk.
    async fn mark_document_saved(&self, uri: &Uri) {
        if let Some(state) = self.documents.write().await.get_mut(uri) {
            state.known_clean = true;
        }
    }

    /// The files behind these URIs changed on disk, so an open buffer for
    /// one of them is no longer known to match.
    async fn mark_documents_changed_on_disk<'a>(&self, uris: impl Iterator<Item = &'a Uri>) {
        let mut documents = self.documents.write().await;
        self.disk_generation.fetch_add(1, Ordering::SeqCst);
        for uri in uris {
            if let Some(state) = documents.get_mut(uri) {
                state.known_clean = false;
            }
        }
    }

    /// Publish diagnostics and cache the results from a completed analysis,
    /// logging config / changed-since messages and firing the completion
    /// notification + code-lens refresh.
    async fn apply_analysis_output(
        &self,
        output: BlockingAnalysisOutput,
        root: &Path,
        version_snapshot: &VersionSnapshot,
    ) {
        if self.cancellation.load(Ordering::SeqCst) {
            return;
        }

        for (level, msg) in output.config_messages {
            self.client.log_message(level, msg).await;
        }

        if let Some((level, msg)) = output.changed_message {
            self.client.log_message(level, msg).await;
        }

        let mut all_diagnostics =
            diagnostics::build_diagnostics(diagnostics::DiagnosticInput::new(
                &output.analysis.results,
                &output.analysis.duplication,
                root,
            ));
        attach_changed_since_data(
            &mut all_diagnostics,
            output.applied_changed_since.as_deref(),
        );
        self.publish_collected_diagnostics(all_diagnostics, version_snapshot)
            .await;

        let complete_params = analysis_complete_params(
            AnalysisCompleteInput::new(&output.analysis.results, &output.analysis.duplication)
                .with_changed_since_scope(output.changed_since_scope.as_ref()),
        );
        *self.analysis.write().await = Some(LspAnalysisSnapshot::new(
            output.analysis.results,
            output.analysis.duplication,
            output.inline_complexity,
        ));

        self.client
            .send_notification::<AnalysisComplete>(complete_params)
            .await;

        self.spawn_code_lens_refresh();

        self.client
            .log_message(MessageType::INFO, "Analysis complete")
            .await;
    }

    async fn publish_collected_diagnostics(
        &self,
        diagnostics_by_file: FxHashMap<Uri, Vec<Diagnostic>>,
        snapshot: &VersionSnapshot,
    ) {
        let disabled = self.disabled_diagnostic_codes.read().await.clone();

        let live_documents: FxHashMap<Uri, DocumentState> = self
            .documents
            .read()
            .await
            .iter()
            .map(|(uri, state)| (uri.clone(), state.clone()))
            .collect();
        let context = PublishContext {
            disabled: &disabled,
            snapshot,
            live_documents: &live_documents,
        };

        // One cache lock for the whole run. The plan owns the messages, so no
        // lock is held while they go out.
        let plan = {
            let mut cache = self.cached_diagnostics.write().await;
            plan_new_diagnostics(&mut cache, diagnostics_by_file, &context)
        };
        let mut new_uris = plan.new_uris;
        let changed_uris = plan.publishes.len();

        // Live-document URIs pushed while the client had not pulled yet. The
        // first-pull transition clears push diagnostics for open documents,
        // but a pull landing mid-loop cannot clear pushes emitted after its
        // clear; those URIs are re-cleared below once the flip is observed.
        let mut pushed_live_uris: Vec<Uri> = Vec::new();
        for planned in plan.publishes {
            let has_findings = !planned.diagnostics.is_empty();
            if let Some(uri) = self.push_planned(planned).await
                && has_findings
            {
                pushed_live_uris.push(uri);
            }
        }

        let clears = {
            let previous_uris = self.previous_diagnostic_uris.read().await;
            let mut cache = self.cached_diagnostics.write().await;
            plan_clears(&mut cache, &previous_uris, &mut new_uris, &context)
        };
        let cache_changed = changed_uris > 0 || !clears.is_empty();
        for planned in clears {
            self.push_planned(planned).await;
        }

        *self.previous_diagnostic_uris.write().await = new_uris;

        // A pull client re-pulls on the refresh. When no cache entry changed,
        // what it holds is still current, so the refresh is skipped.
        if cache_changed && self.client_pulls.load(Ordering::SeqCst) {
            // The first pull landed mid-loop: its open-document clear ran
            // before some pushes above, so those would otherwise double with
            // the pull namespace forever (subsequent runs skip live-document
            // pushes and clears). Re-clear only the push namespace; the pull
            // cache stays authoritative.
            for uri in pushed_live_uris {
                self.client.publish_diagnostics(uri, vec![], None).await;
            }
            self.spawn_diagnostic_refresh();
        }
    }

    /// Push one planned publish unless a pull client reads it from the cache.
    /// Returns the URI when the push went to an open document.
    ///
    /// `client_pulls` is read again for each message: the first
    /// `textDocument/diagnostic` request can arrive while a run publishes,
    /// and it moves the client into pull mode mid-run.
    async fn push_planned(&self, planned: PlannedPublish) -> Option<Uri> {
        if self.client_pulls.load(Ordering::SeqCst) && planned.is_live {
            return None;
        }
        let live_uri = planned.is_live.then(|| planned.uri.clone());
        self.client
            .publish_diagnostics(planned.uri, planned.diagnostics, planned.version)
            .await;
        live_uri
    }

    /// Fire `workspace/diagnostic/refresh` without blocking on the client's
    /// response. The refresh is a server-to-client request that
    /// `tower-lsp-server` resolves only once the client replies; awaiting it
    /// inline would let a slow or unresponsive client stall `run_analysis`
    /// (which holds `analysis_guard`) and delay the `fallow/analysisComplete`
    /// signal. Spawning keeps the request on the wire while decoupling analysis
    /// throughput from client responsiveness.
    fn spawn_diagnostic_refresh(&self) {
        let client = self.client.clone();
        tokio::spawn(async move {
            let _ = client.workspace_diagnostic_refresh().await;
        });
    }

    /// Fire `workspace/codeLens/refresh` detached, for the same reason as
    /// [`Self::spawn_diagnostic_refresh`]: it is a server-to-client request whose
    /// reply must not gate `run_analysis` completion.
    fn spawn_code_lens_refresh(&self) {
        let client = self.client.clone();
        tokio::spawn(async move {
            let _ = client.code_lens_refresh().await;
        });
    }
}

/// Run the bundled LSP server over stdio and return the process exit code.
///
/// The standalone `fallow-lsp` binary and the multicall `fallow lsp-server`
/// subcommand both delegate here, so the runtime construction, stdio wiring,
/// and version-probe semantics stay identical regardless of entry point. A
/// dedicated multi-threaded runtime is built here (rather than via
/// `#[tokio::main]`) so the synchronous CLI can call this without an ambient
/// async context.
pub fn run_stdio_server() -> std::process::ExitCode {
    // Honor `--version` / `-V` / `-v` before starting the stdio server. Without
    // this the server reads stdin, hits EOF, and exits silently, so a version
    // probe (the VS Code extension's binary-skew check) gets no output. Match
    // the CLI's clap output shape (`<bin> <version>`) so consumers can parse it.
    // The multicall entry passes argv as `fallow lsp-server --version`, which
    // still matches here because the scan skips only the program name.
    if std::env::args()
        .skip(1)
        .any(|arg| arg == "--version" || arg == "-V" || arg == "-v")
    {
        #[expect(
            clippy::print_stdout,
            reason = "version query writes to stdout by design"
        )]
        {
            println!("fallow-lsp {}", env!("CARGO_PKG_VERSION"));
        }
        return std::process::ExitCode::SUCCESS;
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            #[expect(
                clippy::print_stderr,
                reason = "startup failure diagnostic writes to stderr by design"
            )]
            {
                eprintln!("fallow-lsp: failed to start tokio runtime: {error}");
            }
            return std::process::ExitCode::FAILURE;
        }
    };

    runtime.block_on(serve_stdio());
    std::process::ExitCode::SUCCESS
}

/// Serve the language server over stdin/stdout until the client closes the
/// stream. Split out of [`run_stdio_server`] so runtime construction stays
/// synchronous.
async fn serve_stdio() {
    tracing_subscriber::fmt()
        .with_env_filter("fallow=info")
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::build(FallowLspServer::new)
        .custom_method("fallow/issueTypes", FallowLspServer::issue_types)
        .custom_method(
            "fallow/refreshDiagnostics",
            FallowLspServer::refresh_diagnostics,
        )
        .finish();

    Server::new(stdin, stdout, socket).serve(service).await;
}

/// Resolve the single analysis root for an LSP run: the canonicalized
/// workspace root.
///
/// The LSP analyzes the workspace root ONCE over the whole tree, matching the
/// CLI (`fallow dead-code` loads one config via `find_and_load(root)` and runs one
/// `analyze_full` pass). `analyze_full` is already workspace-aware: it discovers
/// every workspace package and runs `run_workspace_fast` per package for plugin
/// and script detection, so a single root pass covers all sub-package source
/// files, all per-package plugin configs, and full cross-package reachability.
///
/// The root is canonicalized so it agrees with the canonical `git_toplevel`
/// used by the `--changed-since` filter; otherwise file paths in
/// `AnalysisResults` and the changed-files set start from different prefixes
/// for the same files (e.g. `/tmp/x` vs `/private/tmp/x` on macOS) and the
/// filter silently drops everything.
///
/// Earlier revisions returned the workspace root plus every sub-package and
/// re-ran the entire pipeline per root (issue #971). That re-walked overlapping
/// files once per root, and analyzing a sub-package in isolation lost
/// cross-package reachability, surfacing false-positive `unused-export`
/// findings the root pass resolves. Single-root removes both and keeps the LSP
/// in agreement with the CLI. A `Vec` is returned (always length one) so the
/// caller's accumulate-then-publish structure stays uniform.
fn find_project_roots(workspace_root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let root = canonicalize_for_lsp(workspace_root);
    vec![root]
}

#[cfg(test)]
mod tests;
