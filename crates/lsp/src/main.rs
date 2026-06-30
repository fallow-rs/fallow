#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "tests use unwrap and expect to keep fixture setup concise"
    )
)]

mod analysis;
mod code_actions;
mod code_lens;
mod diagnostic_filter;
mod diagnostics;
mod document_state;
mod hover;
mod initialization;
mod markdown;
mod protocol;
mod server_capabilities;

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[allow(clippy::wildcard_imports, reason = "many LSP types used")]
use ls_types::*;
use tokio::sync::{Mutex, RwLock};
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

use analysis::{
    BlockingAnalysisInput, BlockingAnalysisOutput, LspAnalysisSnapshot, run_blocking_analysis,
};
#[cfg(test)]
use analysis::{ProjectRootAnalysisInput, analyze_project_root, merge_duplication, merge_results};
use diagnostic_filter::{attach_changed_since_data, filter_disabled_diagnostics};
use document_state::{
    DocumentSnapshot, DocumentState, VersionSnapshot, document_matches_disk, uri_is_stale,
};
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
    LspDuplicationOptions, initialization_config_path, initialization_duplication_options,
    initialization_inline_complexity_enabled, initialization_production_override,
};
#[cfg(test)]
use protocol::analysis_complete_params_for_test;
#[cfg(test)]
use protocol::config_load_error_detail;
use protocol::{
    AnalysisComplete, AnalysisCompleteInput, IssueTypeInfo, analysis_complete_params,
    diagnostic_issue_type_metas, diagnostic_issue_types,
};
use server_capabilities::{
    build_server_capabilities, client_supports_workspace_diagnostic_refresh,
};

#[derive(Clone)]
struct FallowLspServer {
    client: Client,
    root: Arc<RwLock<Option<PathBuf>>>,
    analysis: Arc<RwLock<Option<LspAnalysisSnapshot>>>,
    previous_diagnostic_uris: Arc<RwLock<FxHashSet<Uri>>>,
    last_analysis: Arc<Mutex<Instant>>,
    analysis_guard: Arc<tokio::sync::Mutex<()>>,
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
    /// Diagnostic codes to suppress (parsed from initializationOptions.issueTypes)
    disabled_diagnostic_codes: Arc<RwLock<FxHashSet<String>>>,
    /// Optional git ref from `initializationOptions.changedSince`. When set,
    /// analysis results and duplication reports are scoped to files changed
    /// since this ref, mirroring the CLI's `--changed-since`.
    changed_since: Arc<RwLock<Option<String>>>,
    /// Optional explicit config path from `initializationOptions.configPath`.
    /// Mirrors the CLI's `--config` flag for editor clients.
    config_path: Arc<RwLock<Option<PathBuf>>>,
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
    cached_diagnostics: Arc<RwLock<FxHashMap<Uri, Vec<Diagnostic>>>>,
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
    /// Set by `shutdown()`. `run_analysis` checks this at the top and
    /// before publishing diagnostics so a closing client does not receive
    /// spurious post-shutdown publishes. The 250ms grace on the
    /// `analysis_guard` in `shutdown()` lets the current `spawn_blocking`
    /// settle, but does NOT interrupt rayon work already in flight; that
    /// work runs to completion on the blocking thread pool and its
    /// results are dropped. See issue #477.
    cancellation: Arc<AtomicBool>,
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
        let canonical_root = root.map(|path| path.canonicalize().unwrap_or(path));
        if let Some(path) = &canonical_root {
            *self.root.write().await = Some(path.clone());
        }

        if let Some(opts) = &params.initialization_options {
            if let Some(issue_types) = opts.get("issueTypes").and_then(|v| v.as_object()) {
                let mut disabled = FxHashSet::default();
                for issue_type in diagnostic_issue_type_metas() {
                    let Some(config_key) = issue_type.config_key else {
                        continue;
                    };
                    if let Some(enabled) = issue_types
                        .get(config_key)
                        .and_then(serde_json::Value::as_bool)
                        && !enabled
                    {
                        disabled.insert(issue_type.code.to_string());
                    }
                }
                *self.disabled_diagnostic_codes.write().await = disabled;
            }

            if let Some(git_ref) = opts.get("changedSince").and_then(|v| v.as_str()) {
                let trimmed = git_ref.trim();
                *self.changed_since.write().await = if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                };
            }

            *self.config_path.write().await =
                initialization_config_path(opts, canonical_root.as_deref());
            *self.duplication_options.write().await = initialization_duplication_options(opts);
            *self.production_override.write().await = initialization_production_override(opts);
            *self.inline_complexity_enabled.write().await =
                initialization_inline_complexity_enabled(opts);
        }

        let advertise_pull_diagnostics =
            client_supports_workspace_diagnostic_refresh(&params.capabilities);

        Ok(InitializeResult {
            capabilities: build_server_capabilities(advertise_pull_diagnostics),
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "fallow LSP server initialized")
            .await;
    }

    /// Cooperative shutdown.
    ///
    /// Sets the `cancellation` flag so any in-flight `run_analysis`
    /// short-circuits before publishing diagnostics, and awaits the
    /// `analysis_guard` for up to 250ms so a freshly-started blocking
    /// task can settle. NOTE: `tokio::task::spawn_blocking` is not
    /// interruptible; rayon work already running on the blocking thread
    /// pool continues to natural completion and its results are dropped.
    /// The grace is for quiescence, not for cancellation. See issue #477.
    async fn shutdown(&self) -> Result<()> {
        self.cancellation.store(true, Ordering::SeqCst);
        let _ = tokio::time::timeout(Duration::from_millis(250), self.analysis_guard.lock()).await;
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

    async fn did_save(&self, _params: DidSaveTextDocumentParams) {
        {
            let now = Instant::now();
            let mut last = self.last_analysis.lock().await;
            if now.duration_since(*last) < std::time::Duration::from_millis(500) {
                return;
            }
            *last = now;
        }

        self.startup_analysis_started.store(true, Ordering::SeqCst);
        self.run_analysis().await;
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let TextDocumentItem {
            uri, version, text, ..
        } = params.text_document;
        self.documents
            .write()
            .await
            .insert(uri.clone(), DocumentState { version, text });

        if self.client_pulls.load(Ordering::SeqCst) {
            self.client
                .publish_diagnostics(uri, vec![], Some(version))
                .await;
            self.spawn_diagnostic_refresh();
        }

        if !self.startup_analysis_started.swap(true, Ordering::SeqCst) {
            self.spawn_startup_analysis();
        }
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().last() {
            self.documents.write().await.insert(
                params.text_document.uri,
                DocumentState {
                    version: params.text_document.version,
                    text: change.text,
                },
            );
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents
            .write()
            .await
            .remove(&params.text_document.uri);
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
            last_analysis: Arc::new(Mutex::new(
                Instant::now()
                    .checked_sub(std::time::Duration::from_secs(10))
                    .unwrap_or_else(Instant::now),
            )),
            analysis_guard: Arc::new(tokio::sync::Mutex::new(())),
            documents: Arc::new(RwLock::new(FxHashMap::default())),
            startup_analysis_started: Arc::new(AtomicBool::new(false)),
            disabled_diagnostic_codes: Arc::new(RwLock::new(FxHashSet::default())),
            changed_since: Arc::new(RwLock::new(None)),
            config_path: Arc::new(RwLock::new(None)),
            duplication_options: Arc::new(RwLock::new(None)),
            production_override: Arc::new(RwLock::new(None)),
            inline_complexity_enabled: Arc::new(RwLock::new(false)),
            git_toplevel: Arc::new(RwLock::new(None)),
            cached_diagnostics: Arc::new(RwLock::new(FxHashMap::default())),
            client_pulls: Arc::new(AtomicBool::new(false)),
            cancellation: Arc::new(AtomicBool::new(false)),
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

    /// Run the first open-triggered analysis without blocking the `didOpen`
    /// response. The existing `analysis_guard` still prevents overlap with a
    /// concurrent save or restart-triggered analysis.
    fn spawn_startup_analysis(&self) {
        let server = self.clone();
        tokio::spawn(async move {
            server.run_analysis().await;
        });
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

        let version_snapshot = self.snapshot_document_versions().await;

        self.client
            .log_message(MessageType::INFO, "Running fallow analysis...")
            .await;

        let project_roots = find_project_roots(&root);

        self.client
            .log_message(MessageType::INFO, "Analyzing workspace root")
            .await;

        let changed_since = self.changed_since.read().await.clone();
        let changed_since_for_data = changed_since.clone();
        let config_path = self.config_path.read().await.clone();
        let duplication_options = self.duplication_options.read().await.clone();
        let production_override = *self.production_override.read().await;
        let inline_complexity_enabled = *self.inline_complexity_enabled.read().await;

        let resolved_toplevel = self.resolved_git_toplevel(&root).await;
        let blocking_root = root.clone();
        let blocking_toplevel = resolved_toplevel.clone();

        let join_result = tokio::task::spawn_blocking(move || {
            let input = BlockingAnalysisInput {
                project_roots,
                config_path,
                duplication_options,
                production_override,
                inline_complexity_enabled,
                root: blocking_root,
                toplevel: blocking_toplevel,
                changed_since,
            };
            run_blocking_analysis(&input)
        })
        .await;

        match join_result {
            Ok(output) => {
                self.apply_analysis_output(
                    output,
                    &root,
                    &version_snapshot,
                    changed_since_for_data.as_deref(),
                )
                .await;
            }
            Err(e) => {
                self.client
                    .log_message(MessageType::ERROR, format!("Analysis failed: {e}"))
                    .await;
            }
        }
    }

    /// Snapshot every open document's version + disk-match state at analysis
    /// entry, used by `publish_collected_diagnostics` for the staleness check.
    async fn snapshot_document_versions(&self) -> VersionSnapshot {
        self.documents
            .read()
            .await
            .iter()
            .map(|(uri, state)| {
                (
                    uri.clone(),
                    DocumentSnapshot {
                        version: state.version,
                        matches_disk: document_matches_disk(uri, &state.text),
                    },
                )
            })
            .collect()
    }

    /// Publish diagnostics and cache the results from a completed analysis,
    /// logging config / changed-since messages and firing the completion
    /// notification + code-lens refresh.
    async fn apply_analysis_output(
        &self,
        output: BlockingAnalysisOutput,
        root: &Path,
        version_snapshot: &VersionSnapshot,
        changed_since_for_data: Option<&str>,
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
        attach_changed_since_data(&mut all_diagnostics, changed_since_for_data);
        self.publish_collected_diagnostics(all_diagnostics, version_snapshot)
            .await;

        let complete_params = analysis_complete_params(AnalysisCompleteInput::new(
            &output.analysis.results,
            &output.analysis.duplication,
        ));
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

    #[expect(
        clippy::significant_drop_tightening,
        reason = "RwLock guard scope is intentional"
    )]
    async fn publish_collected_diagnostics(
        &self,
        diagnostics_by_file: FxHashMap<Uri, Vec<Diagnostic>>,
        snapshot: &VersionSnapshot,
    ) {
        let disabled = self.disabled_diagnostic_codes.read().await;

        let live_documents: FxHashMap<Uri, DocumentState> = self
            .documents
            .read()
            .await
            .iter()
            .map(|(uri, state)| (uri.clone(), state.clone()))
            .collect();

        let use_pull_diagnostics = self.client_pulls.load(Ordering::SeqCst);
        let mut new_uris: FxHashSet<Uri> = FxHashSet::default();

        for (uri, diags) in &diagnostics_by_file {
            new_uris.insert(uri.clone());

            if uri_is_stale(uri, snapshot, &live_documents) {
                continue;
            }

            let filtered = filter_disabled_diagnostics(diags, &disabled);

            if !use_pull_diagnostics || !live_documents.contains_key(uri) {
                self.client
                    .publish_diagnostics(
                        uri.clone(),
                        filtered.clone(),
                        snapshot.get(uri).map(|state| state.version),
                    )
                    .await;
            }

            self.cached_diagnostics
                .write()
                .await
                .insert(uri.clone(), filtered);
        }

        self.clear_stale_diagnostics(
            &mut new_uris,
            snapshot,
            &live_documents,
            use_pull_diagnostics,
        )
        .await;

        *self.previous_diagnostic_uris.write().await = new_uris;

        if use_pull_diagnostics {
            self.spawn_diagnostic_refresh();
        }
    }

    /// Clear diagnostics for URIs that had findings on the previous run but do
    /// not this run, skipping stale URIs (re-inserted into `new_uris` so their
    /// last-valid diagnostics survive) and removing them from the cache.
    async fn clear_stale_diagnostics(
        &self,
        new_uris: &mut FxHashSet<Uri>,
        snapshot: &VersionSnapshot,
        live_documents: &FxHashMap<Uri, DocumentState>,
        use_pull_diagnostics: bool,
    ) {
        let previous_uris = self.previous_diagnostic_uris.read().await;
        let mut cache = self.cached_diagnostics.write().await;
        for old_uri in previous_uris.iter() {
            if new_uris.contains(old_uri) {
                continue;
            }
            if uri_is_stale(old_uri, snapshot, live_documents) {
                new_uris.insert(old_uri.clone());
                continue;
            }
            if !use_pull_diagnostics || !live_documents.contains_key(old_uri) {
                self.client
                    .publish_diagnostics(
                        old_uri.clone(),
                        vec![],
                        snapshot.get(old_uri).map(|state| state.version),
                    )
                    .await;
            }
            cache.remove(old_uri);
        }
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

#[tokio::main]
async fn main() {
    // Honor `--version` / `-V` / `-v` before starting the stdio server. Without
    // this the server reads stdin, hits EOF, and exits silently, so a version
    // probe (the VS Code extension's binary-skew check) gets no output. Match
    // the CLI's clap output shape (`<bin> <version>`) so consumers can parse it.
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
        return;
    }

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
    let root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    vec![root]
}

#[cfg(test)]
mod tests;
