mod code_actions;
mod code_lens;
mod diagnostics;
mod hover;
mod markdown;

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::{Mutex, RwLock};
use tower_lsp::jsonrpc::Result;
#[allow(clippy::wildcard_imports, reason = "many LSP types used")]
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use serde::{Deserialize, Serialize};

use fallow_core::changed_files::{
    filter_duplication_by_changed_files, filter_results_by_changed_files, resolve_git_toplevel,
    try_get_changed_files_with_toplevel,
};
use fallow_core::duplicates::DuplicationReport;
use fallow_core::results::AnalysisResults;

// ── Custom LSP notification: fallow/analysisComplete ──────────────────────

/// Custom notification sent to the client after every analysis completes.
/// Carries summary stats so the extension can update the status bar, context
/// keys, and other UI without running a separate CLI process.
enum AnalysisComplete {}

impl notification::Notification for AnalysisComplete {
    type Params = AnalysisCompleteParams;
    const METHOD: &'static str = "fallow/analysisComplete";
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisCompleteParams {
    total_issues: usize,
    unused_files: usize,
    unused_exports: usize,
    unused_types: usize,
    private_type_leaks: usize,
    unused_dependencies: usize,
    unused_dev_dependencies: usize,
    unused_optional_dependencies: usize,
    unused_enum_members: usize,
    unused_class_members: usize,
    unresolved_imports: usize,
    unlisted_dependencies: usize,
    duplicate_exports: usize,
    type_only_dependencies: usize,
    test_only_dependencies: usize,
    circular_dependencies: usize,
    boundary_violations: usize,
    stale_suppressions: usize,
    unused_catalog_entries: usize,
    empty_catalog_groups: usize,
    unresolved_catalog_references: usize,
    unused_dependency_overrides: usize,
    misconfigured_dependency_overrides: usize,
    duplication_percentage: f64,
    clone_groups: usize,
}

/// Diagnostic codes that the LSP client can disable via initializationOptions.
/// The same table also backs the `fallow/issueTypes` custom request used by
/// editor clients that need user-facing labels for all emitted diagnostic codes.
#[derive(Debug, Clone, Copy)]
struct DiagnosticIssueType {
    config_key: Option<&'static str>,
    code: &'static str,
    label: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct IssueTypeInfo {
    code: String,
    label: String,
}

const DIAGNOSTIC_ISSUE_TYPES: &[DiagnosticIssueType] = &[
    DiagnosticIssueType {
        config_key: None,
        code: "code-duplication",
        label: "Code Duplication",
    },
    DiagnosticIssueType {
        config_key: Some("unused-files"),
        code: "unused-file",
        label: "Unused Files",
    },
    DiagnosticIssueType {
        config_key: Some("unused-exports"),
        code: "unused-export",
        label: "Unused Exports",
    },
    DiagnosticIssueType {
        config_key: Some("unused-types"),
        code: "unused-type",
        label: "Unused Types",
    },
    DiagnosticIssueType {
        config_key: Some("private-type-leaks"),
        code: "private-type-leak",
        label: "Private Type Leaks",
    },
    DiagnosticIssueType {
        config_key: Some("unused-dependencies"),
        code: "unused-dependency",
        label: "Unused Dependencies",
    },
    DiagnosticIssueType {
        config_key: Some("unused-dev-dependencies"),
        code: "unused-dev-dependency",
        label: "Unused Dev Dependencies",
    },
    DiagnosticIssueType {
        config_key: Some("unused-optional-dependencies"),
        code: "unused-optional-dependency",
        label: "Unused Optional Dependencies",
    },
    DiagnosticIssueType {
        config_key: Some("unused-enum-members"),
        code: "unused-enum-member",
        label: "Unused Enum Members",
    },
    DiagnosticIssueType {
        config_key: Some("unused-class-members"),
        code: "unused-class-member",
        label: "Unused Class Members",
    },
    DiagnosticIssueType {
        config_key: Some("unresolved-imports"),
        code: "unresolved-import",
        label: "Unresolved Imports",
    },
    DiagnosticIssueType {
        config_key: Some("unlisted-dependencies"),
        code: "unlisted-dependency",
        label: "Unlisted Dependencies",
    },
    DiagnosticIssueType {
        config_key: Some("duplicate-exports"),
        code: "duplicate-export",
        label: "Duplicate Exports",
    },
    DiagnosticIssueType {
        config_key: Some("type-only-dependencies"),
        code: "type-only-dependency",
        label: "Type-Only Dependencies",
    },
    DiagnosticIssueType {
        config_key: Some("test-only-dependencies"),
        code: "test-only-dependency",
        label: "Test-Only Dependencies",
    },
    DiagnosticIssueType {
        config_key: Some("circular-dependencies"),
        code: "circular-dependency",
        label: "Circular Dependencies",
    },
    DiagnosticIssueType {
        config_key: Some("boundary-violation"),
        code: "boundary-violation",
        label: "Boundary Violations",
    },
    DiagnosticIssueType {
        config_key: Some("stale-suppressions"),
        code: "stale-suppression",
        label: "Stale Suppressions",
    },
    DiagnosticIssueType {
        config_key: Some("unused-catalog-entries"),
        code: "unused-catalog-entry",
        label: "Unused Catalog Entries",
    },
    DiagnosticIssueType {
        config_key: Some("empty-catalog-groups"),
        code: "empty-catalog-group",
        label: "Empty Catalog Groups",
    },
    DiagnosticIssueType {
        config_key: Some("unresolved-catalog-references"),
        code: "unresolved-catalog-reference",
        label: "Unresolved Catalog References",
    },
    DiagnosticIssueType {
        config_key: Some("unused-dependency-overrides"),
        code: "unused-dependency-override",
        label: "Unused Dependency Overrides",
    },
    DiagnosticIssueType {
        config_key: Some("misconfigured-dependency-overrides"),
        code: "misconfigured-dependency-override",
        label: "Misconfigured Dependency Overrides",
    },
];

fn diagnostic_issue_types() -> Vec<IssueTypeInfo> {
    DIAGNOSTIC_ISSUE_TYPES
        .iter()
        .map(|issue_type| IssueTypeInfo {
            code: issue_type.code.to_string(),
            label: issue_type.label.to_string(),
        })
        .collect()
}

fn config_load_error_detail(
    project_root: &Path,
    explicit_config_path: Option<&Path>,
    err: impl std::fmt::Display,
) -> String {
    match explicit_config_path {
        Some(path) => format!(
            "fallow.configPath '{}' failed to load for {}: {err} (no diagnostics will be produced)",
            path.display(),
            project_root.display()
        ),
        None => format!("config error for {}: {err}", project_root.display()),
    }
}

/// Run dead-code + duplicates analysis for a single project root, appending
/// findings to the merged accumulators and a status message to
/// `config_messages`. Extracted out of `run_analysis` to keep that method
/// under the 150-line clippy ceiling.
fn analyze_project_root(
    project_root: &Path,
    config_path: Option<&Path>,
    merged_results: &mut AnalysisResults,
    merged_duplication: &mut DuplicationReport,
    config_messages: &mut Vec<(MessageType, String)>,
) {
    let (config, message) = match fallow_core::config_for_project(project_root, config_path) {
        Ok((config, Some(path))) => (
            config,
            (
                MessageType::INFO,
                format!("loaded config: {}", path.display()),
            ),
        ),
        Ok((config, None)) => (
            config,
            (
                MessageType::INFO,
                format!(
                    "no config file found for {}, using defaults",
                    project_root.display()
                ),
            ),
        ),
        Err(e) => {
            // WARNING (not INFO) so VS Code's notification system pops the
            // message; INFO goes only to the (hidden-by-default) Output
            // channel. Only fall back to defaults when the user has NOT
            // explicitly set a config path; an explicit-but-broken path
            // should fail loudly rather than silently using defaults.
            let detail = config_load_error_detail(project_root, config_path, &e);
            config_messages.push((MessageType::WARNING, detail));
            if config_path.is_none() {
                #[expect(
                    deprecated,
                    reason = "ADR-008 deprecates fallow_core::analyze_project externally; the LSP still uses the workspace path dependency"
                )]
                if let Ok(results) = fallow_core::analyze_project(project_root) {
                    merge_results(merged_results, results);
                }
                let duplication = fallow_core::duplicates::find_duplicates_in_project(
                    project_root,
                    &fallow_config::DuplicatesConfig::default(),
                );
                merge_duplication(merged_duplication, duplication);
            }
            return;
        }
    };
    config_messages.push(message);

    #[expect(
        deprecated,
        reason = "ADR-008 deprecates fallow_core::analyze_with_usages externally; the LSP still uses the workspace path dependency"
    )]
    if let Ok(results) = fallow_core::analyze_with_usages(&config) {
        merge_results(merged_results, results);
    }

    let files = fallow_core::discover::discover_files_with_plugin_scopes(&config);
    let duplication =
        fallow_core::duplicates::find_duplicates(project_root, &files, &config.duplicates);
    merge_duplication(merged_duplication, duplication);
}

/// Per-document state tracked by the LSP: the `version` integer supplied by
/// the client on every `did_open` / `did_change` plus the latest text. The
/// version is the load-bearing piece for the staleness check in
/// `publish_collected_diagnostics`; see `.claude/rules/lsp-server.md` for the
/// "diagnostic publish staleness" invariant.
#[derive(Debug, Clone)]
struct DocumentState {
    version: i32,
    text: String,
}

/// Per-URI version map captured at `run_analysis` entry, threaded through to
/// `publish_collected_diagnostics` so it can drop per-URI publishes whose
/// document has been edited during the analysis run. A type alias so future
/// readers can grep for the snapshot's identity (it is also a stable seam
/// for tests).
type VersionSnapshot = FxHashMap<Url, i32>;

fn initialization_config_path(opts: &serde_json::Value, root: Option<&Path>) -> Option<PathBuf> {
    let raw = opts.get("configPath").and_then(|v| v.as_str())?.trim();
    if raw.is_empty() {
        return None;
    }

    let path = PathBuf::from(raw);
    let path = if path.is_absolute() {
        path
    } else if let Some(root) = root {
        root.join(path)
    } else {
        path
    };

    Some(path.canonicalize().unwrap_or(path))
}

struct FallowLspServer {
    client: Client,
    root: Arc<RwLock<Option<PathBuf>>>,
    results: Arc<RwLock<Option<AnalysisResults>>>,
    duplication: Arc<RwLock<Option<DuplicationReport>>>,
    previous_diagnostic_uris: Arc<RwLock<FxHashSet<Url>>>,
    last_analysis: Arc<Mutex<Instant>>,
    analysis_guard: Arc<tokio::sync::Mutex<()>>,
    /// Per-URI document state tracked from `did_open` / `did_change` /
    /// `did_close`. The `version` field is the LSP-supplied integer used by
    /// `run_analysis` to snapshot the document state at analysis start and
    /// by `publish_collected_diagnostics` to skip stale publishes; see
    /// `.claude/rules/lsp-server.md` for the staleness invariant.
    documents: Arc<RwLock<FxHashMap<Url, DocumentState>>>,
    /// Diagnostic codes to suppress (parsed from initializationOptions.issueTypes)
    disabled_diagnostic_codes: Arc<RwLock<FxHashSet<String>>>,
    /// Optional git ref from `initializationOptions.changedSince`. When set,
    /// analysis results and duplication reports are scoped to files changed
    /// since this ref, mirroring the CLI's `--changed-since`.
    changed_since: Arc<RwLock<Option<String>>>,
    /// Optional explicit config path from `initializationOptions.configPath`.
    /// Mirrors the CLI's `--config` flag for editor clients.
    config_path: Arc<RwLock<Option<PathBuf>>>,
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
    cached_diagnostics: Arc<RwLock<FxHashMap<Url, Vec<Diagnostic>>>>,
    /// Set by `shutdown()`. `run_analysis` checks this at the top and
    /// before publishing diagnostics so a closing client does not receive
    /// spurious post-shutdown publishes. The 250ms grace on the
    /// `analysis_guard` in `shutdown()` lets the current `spawn_blocking`
    /// settle, but does NOT interrupt rayon work already in flight; that
    /// work runs to completion on the blocking thread pool and its
    /// results are dropped. See issue #477.
    cancellation: Arc<AtomicBool>,
}

/// Build the `ServerCapabilities` advertised by `initialize`.
///
/// `diagnostic_provider` is required for strict LSP 3.17 clients
/// (Helix, Zed, and other editors that gate the pull-model diagnostic
/// request on the advertised capability). Without it, `textDocument/diagnostic`
/// is dead code for those clients even though the handler is wired up.
/// `inter_file_dependencies = true` because changing exports or imports in one
/// file can flip diagnostics in another (unused exports, unused dependencies).
/// `workspace_diagnostics = false` because we do not serve `workspace/diagnostic`.
fn build_server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
            ..Default::default()
        })),
        code_lens_provider: Some(CodeLensOptions {
            resolve_provider: Some(false),
        }),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        diagnostic_provider: Some(DiagnosticServerCapabilities::Options(DiagnosticOptions {
            identifier: Some("fallow".to_string()),
            inter_file_dependencies: true,
            workspace_diagnostics: false,
            work_done_progress_options: WorkDoneProgressOptions::default(),
        })),
        ..Default::default()
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for FallowLspServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let root = params
            .root_uri
            .and_then(|u| u.to_file_path().ok())
            .or_else(|| {
                params
                    .workspace_folders
                    .as_deref()
                    .and_then(|fs| fs.first())
                    .and_then(|f| f.uri.to_file_path().ok())
            });
        // Canonicalize the workspace root so absolute paths emitted by
        // `analyze_project` agree with paths produced by `resolve_git_toplevel`
        // (which is also canonicalized). On macOS, /tmp -> /private/tmp; on
        // Windows, 8.3 short paths get expanded. Without this, the
        // `--changed-since` filter silently fails to match because the two
        // sides start from different prefixes for the same files.
        let canonical_root = root.map(|path| path.canonicalize().unwrap_or(path));
        if let Some(path) = &canonical_root {
            *self.root.write().await = Some(path.clone());
        }

        // Parse initializationOptions for issue type toggles and changedSince
        if let Some(opts) = &params.initialization_options {
            if let Some(issue_types) = opts.get("issueTypes").and_then(|v| v.as_object()) {
                let mut disabled = FxHashSet::default();
                for issue_type in DIAGNOSTIC_ISSUE_TYPES {
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
                // "code-duplication" is controlled by the duplication.* settings,
                // not issueTypes (always enabled at the LSP level).
                *self.disabled_diagnostic_codes.write().await = disabled;
            }

            // changedSince: a git ref (tag, branch, or SHA). Empty string is
            // treated as "unset" so users can clear the setting via the
            // settings UI without restarting.
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
        }

        Ok(InitializeResult {
            capabilities: build_server_capabilities(),
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "fallow LSP server initialized")
            .await;

        // Run initial analysis
        self.run_analysis().await;
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
        // Debounce: skip if last analysis was less than 500ms ago
        {
            let now = Instant::now();
            let mut last = self.last_analysis.lock().await;
            if now.duration_since(*last) < std::time::Duration::from_millis(500) {
                return;
            }
            // Update timestamp under the lock to prevent TOCTOU races
            // where multiple saves pass the debounce check simultaneously
            *last = now;
        }

        // Re-run analysis on save
        self.run_analysis().await;
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let TextDocumentItem {
            uri, version, text, ..
        } = params.text_document;
        self.documents
            .write()
            .await
            .insert(uri, DocumentState { version, text });
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // Store the latest document text alongside the version supplied by
        // the client. Version is the load-bearing field for the staleness
        // check in `publish_collected_diagnostics`. `TextDocumentSyncKind::FULL`
        // ships the full text in one entry, so the last `content_changes`
        // entry is the new full document.
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
        let results = self.results.read().await;
        let Some(results) = results.as_ref() else {
            return Ok(None);
        };

        let uri = &params.text_document.uri;
        let Ok(file_path) = uri.to_file_path() else {
            return Ok(None);
        };

        let mut actions = Vec::new();

        // Read file content once for computing line positions and edit ranges.
        // Prefer in-memory document text (from did_open/did_change), fall back to disk.
        let documents = self.documents.read().await;
        let file_content = documents.get(uri).map_or_else(
            || std::fs::read_to_string(&file_path).unwrap_or_default(),
            |state| state.text.clone(),
        );
        drop(documents);
        let file_lines: Vec<&str> = file_content.lines().collect();

        // Generate "Remove export" code actions for unused exports
        actions.extend(code_actions::build_remove_export_actions(
            results,
            &file_path,
            uri,
            &params.range,
            &file_lines,
        ));

        // Generate "Delete this file" code actions for unused files
        actions.extend(code_actions::build_delete_file_actions(
            results,
            &file_path,
            uri,
            &params.range,
        ));

        // Generate "Remove unused catalog entry" code actions for
        // pnpm-workspace.yaml findings. `entry.path` is stored relative
        // to the analyzer root, so we pass the cached root through.
        // Pass `file_lines` (already computed above from in-memory
        // document text or disk fallback) so the deletion range
        // matches what the user actually sees in their editor when
        // they have unsaved edits to pnpm-workspace.yaml.
        let root = self.root.read().await.clone();
        if let Some(root) = root {
            actions.extend(code_actions::build_remove_catalog_entry_actions(
                results,
                &root,
                uri,
                &params.range,
                &file_lines,
            ));
            actions.extend(code_actions::build_remove_empty_catalog_group_actions(
                results,
                &root,
                uri,
                &params.range,
                &file_lines,
            ));
        }

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }

    #[expect(
        clippy::significant_drop_tightening,
        reason = "RwLock guard scope is intentional"
    )]
    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let results = self.results.read().await;
        let Some(results) = results.as_ref() else {
            return Ok(None);
        };

        let Ok(file_path) = params.text_document.uri.to_file_path() else {
            return Ok(None);
        };

        let lenses = code_lens::build_code_lenses(results, &file_path, &params.text_document.uri);

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
        let results = self.results.read().await;
        let Some(results) = results.as_ref() else {
            return Ok(None);
        };

        let uri = &params.text_document_position_params.text_document.uri;
        let Ok(file_path) = uri.to_file_path() else {
            return Ok(None);
        };

        let position = params.text_document_position_params.position;

        let duplication = self.duplication.read().await;
        let empty_report = fallow_core::duplicates::DuplicationReport::default();
        let duplication_ref = duplication.as_ref().unwrap_or(&empty_report);

        Ok(hover::build_hover(
            results,
            duplication_ref,
            &file_path,
            position,
        ))
    }
}

impl FallowLspServer {
    fn new(client: Client) -> Self {
        Self {
            client,
            root: Arc::new(RwLock::new(None)),
            results: Arc::new(RwLock::new(None)),
            duplication: Arc::new(RwLock::new(None)),
            previous_diagnostic_uris: Arc::new(RwLock::new(FxHashSet::default())),
            last_analysis: Arc::new(Mutex::new(
                Instant::now()
                    .checked_sub(std::time::Duration::from_secs(10))
                    .unwrap_or_else(Instant::now),
            )),
            analysis_guard: Arc::new(tokio::sync::Mutex::new(())),
            documents: Arc::new(RwLock::new(FxHashMap::default())),
            disabled_diagnostic_codes: Arc::new(RwLock::new(FxHashSet::default())),
            changed_since: Arc::new(RwLock::new(None)),
            config_path: Arc::new(RwLock::new(None)),
            git_toplevel: Arc::new(RwLock::new(None)),
            cached_diagnostics: Arc::new(RwLock::new(FxHashMap::default())),
            cancellation: Arc::new(AtomicBool::new(false)),
        }
    }

    #[expect(
        clippy::unused_async,
        reason = "tower-lsp custom_method handlers are async methods"
    )]
    async fn issue_types(&self) -> Result<Vec<IssueTypeInfo>> {
        Ok(diagnostic_issue_types())
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
        // Short-circuit any work post-shutdown. The cancellation flag
        // remains set for the rest of the process lifetime; subsequent
        // `did_save` deliveries are no-ops until tower-lsp's `exit`
        // notification tears down the runtime.
        if self.cancellation.load(Ordering::SeqCst) {
            return;
        }

        let root = self.root.read().await.clone();
        let Some(root) = root else { return };

        let Ok(_guard) = self.analysis_guard.try_lock() else {
            return; // analysis already running
        };

        // Capture the per-URI document-version snapshot ONCE at analysis
        // entry, holding it across the `spawn_blocking` join. The blocking
        // task does not need versions, so the snapshot stays in the async
        // scope. The snapshot is the load-bearing input to the staleness
        // check inside `publish_collected_diagnostics`: any URI whose live
        // version advances past the snapshot during the analysis run has
        // its publish skipped. See `.claude/rules/lsp-server.md` for the
        // "diagnostic publish staleness" invariant.
        let version_snapshot: VersionSnapshot = self
            .documents
            .read()
            .await
            .iter()
            .map(|(uri, state)| (uri.clone(), state.version))
            .collect();

        self.client
            .log_message(MessageType::INFO, "Running fallow analysis...")
            .await;

        // Discover all project roots: the workspace root itself, plus any
        // subdirectories with their own package.json (sub-projects, fixtures, etc.)
        let project_roots = find_project_roots(&root);

        self.client
            .log_message(
                MessageType::INFO,
                format!("Found {} project root(s)", project_roots.len()),
            )
            .await;

        let changed_since = self.changed_since.read().await.clone();
        // Keep an outer-scope copy: the spawn_blocking closure consumes
        // `changed_since` by move, but `attach_changed_since_data` (called
        // after the join) needs to know whether the filter was active so
        // it can stamp `Diagnostic.data.changedSince` accordingly.
        let changed_since_for_data = changed_since.clone();
        let config_path = self.config_path.read().await.clone();

        // Resolve and cache the canonical git toplevel for `root`. Done even
        // when `changed_since` is None so we can warn the user once if their
        // workspace differs from the toplevel; that mismatch is the most
        // common cause of "changedSince doesn't filter what I expect"
        // reports (issue #190). The warn-once is gated inside
        // `resolved_git_toplevel` so it does not spam the Output panel on
        // every save. Caching avoids an extra `git rev-parse
        // --show-toplevel` subprocess on every save.
        let resolved_toplevel = self.resolved_git_toplevel(&root).await;

        let blocking_root = root.clone();
        let blocking_toplevel = resolved_toplevel.clone();

        let join_result = tokio::task::spawn_blocking(move || {
            let mut merged_results = AnalysisResults::default();
            let mut merged_duplication = DuplicationReport::default();
            // Collect "loaded config: ..." messages alongside results so the
            // async caller can surface them via log_message without doing
            // blocking I/O on the async executor or calling find_and_load
            // twice per project root.
            let mut config_messages: Vec<(MessageType, String)> =
                Vec::with_capacity(project_roots.len());
            for project_root in &project_roots {
                analyze_project_root(
                    project_root,
                    config_path.as_deref(),
                    &mut merged_results,
                    &mut merged_duplication,
                    &mut config_messages,
                );
            }

            // Dedupe cross-root duplicates introduced by `merge_results`'s
            // `.extend()`. In monorepos where the workspace root and a
            // sub-package both walk the same source files, every finding
            // is accumulated once per overlapping root and produces N
            // stacked diagnostics on the same range. See `dedup_results`
            // for the per-type identity keys.
            dedup_results(&mut merged_results);

            // Apply --changed-since-equivalent filter, if configured. Paths
            // are joined against the canonical git toplevel resolved above
            // (or the workspace root as a fallback when not in a git repo)
            // so that file paths match what `analyze_project` produces in
            // monorepos where the workspace root is a subdirectory of the
            // repository. On git failure, log the reason and leave results
            // unfiltered so the user sees what's wrong instead of an
            // unexplained empty Problems panel.
            let changed_message = if let Some(ref git_ref) = changed_since {
                let toplevel = blocking_toplevel
                    .as_deref()
                    .unwrap_or(blocking_root.as_path());
                match try_get_changed_files_with_toplevel(&blocking_root, toplevel, git_ref) {
                    Ok(changed) => {
                        filter_results_by_changed_files(&mut merged_results, &changed);
                        filter_duplication_by_changed_files(
                            &mut merged_duplication,
                            &changed,
                            &blocking_root,
                        );
                        Some((
                            MessageType::INFO,
                            format!(
                                "changedSince '{git_ref}': scoped to {} changed file(s)",
                                changed.len()
                            ),
                        ))
                    }
                    Err(err) => Some((
                        MessageType::WARNING,
                        format!(
                            "changedSince '{git_ref}' ignored: {} (showing full-scope results)",
                            err.describe()
                        ),
                    )),
                }
            } else {
                None
            };

            (
                merged_results,
                merged_duplication,
                config_messages,
                changed_message,
            )
        })
        .await;

        match join_result {
            Ok((results, duplication, config_messages, changed_message)) => {
                // Re-check the cancellation flag after the blocking task
                // returns. The shutdown handler may have flipped it while
                // the analysis was running; in that case skip publish so
                // we don't push diagnostics into a closing client.
                if self.cancellation.load(Ordering::SeqCst) {
                    return;
                }

                // Surface which config was loaded for each project root so users
                // can verify their config is picked up (addresses silent
                // config-loss UX). Emitted from the async context after the
                // blocking task returns.
                for (level, msg) in config_messages {
                    self.client.log_message(level, msg).await;
                }

                // Report on changedSince outcome so users see why the Problems
                // panel is scoped (or why the filter was dropped).
                if let Some((level, msg)) = changed_message {
                    self.client.log_message(level, msg).await;
                }

                // Build diagnostics once from the merged results.
                // Each result item already carries its own file path, so a single
                // `build_diagnostics` call covers all roots. The workspace root is
                // used only for unlisted-dependency diagnostics (placed on its
                // package.json). Previously this looped per-root, duplicating every
                // diagnostic N times (#90).
                let mut all_diagnostics =
                    diagnostics::build_diagnostics(&results, &duplication, &root);
                attach_changed_since_data(&mut all_diagnostics, changed_since_for_data.as_deref());
                self.publish_collected_diagnostics(all_diagnostics, &version_snapshot)
                    .await;

                // Send summary stats to the client before storing results
                self.client
                    .send_notification::<AnalysisComplete>(AnalysisCompleteParams {
                        total_issues: results.total_issues(),
                        unused_files: results.unused_files.len(),
                        unused_exports: results.unused_exports.len(),
                        unused_types: results.unused_types.len(),
                        private_type_leaks: results.private_type_leaks.len(),
                        unused_dependencies: results.unused_dependencies.len(),
                        unused_dev_dependencies: results.unused_dev_dependencies.len(),
                        unused_optional_dependencies: results.unused_optional_dependencies.len(),
                        unused_enum_members: results.unused_enum_members.len(),
                        unused_class_members: results.unused_class_members.len(),
                        unresolved_imports: results.unresolved_imports.len(),
                        unlisted_dependencies: results.unlisted_dependencies.len(),
                        duplicate_exports: results.duplicate_exports.len(),
                        type_only_dependencies: results.type_only_dependencies.len(),
                        test_only_dependencies: results.test_only_dependencies.len(),
                        circular_dependencies: results.circular_dependencies.len(),
                        boundary_violations: results.boundary_violations.len(),
                        stale_suppressions: results.stale_suppressions.len(),
                        unused_catalog_entries: results.unused_catalog_entries.len(),
                        empty_catalog_groups: results.empty_catalog_groups.len(),
                        unresolved_catalog_references: results.unresolved_catalog_references.len(),
                        unused_dependency_overrides: results.unused_dependency_overrides.len(),
                        misconfigured_dependency_overrides: results
                            .misconfigured_dependency_overrides
                            .len(),
                        duplication_percentage: duplication.stats.duplication_percentage,
                        clone_groups: duplication.stats.clone_groups,
                    })
                    .await;

                *self.results.write().await = Some(results);
                *self.duplication.write().await = Some(duplication);

                let _ = self.client.code_lens_refresh().await;

                self.client
                    .log_message(MessageType::INFO, "Analysis complete")
                    .await;
            }
            Err(e) => {
                self.client
                    .log_message(MessageType::ERROR, format!("Analysis failed: {e}"))
                    .await;
            }
        }
    }

    /// Decide whether a URI is stale relative to a captured version snapshot.
    ///
    /// A URI is stale when we cannot prove that the analysis ran against the
    /// same document state the LSP currently holds for that URI. Three
    /// conditions count:
    ///   1. The URI was in the snapshot AND the live version advanced past it
    ///      (strict `>`; equal versions mean the same document state). The
    ///      user edited the file during the analysis run.
    ///   2. The URI was in the snapshot AND the live document is now absent
    ///      (closed via `did_close` between snapshot and publish; we cannot
    ///      prove the client still owns the document).
    ///   3. The URI is absent from the snapshot BUT present in `live_versions`
    ///      (opened via `did_open` between snapshot and publish; the analysis
    ///      ran without seeing the buffer the client now holds, and we have
    ///      no version to attach to the publish so the client cannot drop a
    ///      mismatched payload server-to-client). The next analysis triggered
    ///      by `did_save` will publish a fresh result with a version slot.
    ///
    /// Only URIs absent from BOTH the snapshot AND `live_versions` are NOT
    /// stale: these are cross-file diagnostics anchored to files the user
    /// never `did_open`'d via the LSP (e.g. `package.json` for unlisted
    /// dependencies, `pnpm-workspace.yaml` for catalog references). No
    /// version race exists for them.
    fn uri_is_stale(
        uri: &Url,
        snapshot: &VersionSnapshot,
        live_versions: &FxHashMap<Url, i32>,
    ) -> bool {
        match (snapshot.get(uri), live_versions.get(uri)) {
            (Some(&snapshot_version), Some(&live_version)) => live_version > snapshot_version,
            // (Some(_), None) closed-mid-run + (None, Some(_)) opened-mid-run.
            // Both share the same "skip publish" outcome but for distinct
            // reasons documented in the helper's doc comment.
            (Some(_), None) | (None, Some(_)) => true,
            (None, None) => false,
        }
    }

    #[expect(
        clippy::significant_drop_tightening,
        reason = "RwLock guard scope is intentional"
    )]
    async fn publish_collected_diagnostics(
        &self,
        diagnostics_by_file: FxHashMap<Url, Vec<Diagnostic>>,
        snapshot: &VersionSnapshot,
    ) {
        let disabled = self.disabled_diagnostic_codes.read().await;

        // Read the live per-URI versions ONCE at entry into a local map.
        // Doing it once avoids holding `documents.read()` across each
        // `publish_diagnostics().await` and pre-computes the values needed
        // by the stale-clearing branch below (which must NOT acquire
        // `documents.read()` while holding `cached_diagnostics.write()`,
        // to keep lock ordering clean).
        let live_versions: FxHashMap<Url, i32> = self
            .documents
            .read()
            .await
            .iter()
            .map(|(uri, state)| (uri.clone(), state.version))
            .collect();

        // Collect the set of URIs we are publishing to (or skipping). Stale
        // URIs ARE inserted into `new_uris` so the next-run stale-clearing
        // loop does not erase last-valid diagnostics from the client while
        // the user is still editing.
        let mut new_uris: FxHashSet<Url> = FxHashSet::default();

        // Publish diagnostics for current results, filtering out disabled
        // issue types and skipping stale URIs.
        for (uri, diags) in &diagnostics_by_file {
            new_uris.insert(uri.clone());

            if Self::uri_is_stale(uri, snapshot, &live_versions) {
                // Skip publish AND cache update. The cache stays at its
                // last-valid state; pull-model `textDocument/diagnostic`
                // consumers continue to see consistent v(N) data even
                // though the document is now at v(N+1).
                continue;
            }

            let filtered: Vec<Diagnostic> = if disabled.is_empty() {
                diags.clone()
            } else {
                diags
                    .iter()
                    .filter(|d| {
                        d.code.as_ref().is_none_or(|code| match code {
                            NumberOrString::String(s) => !disabled.contains(s.as_str()),
                            NumberOrString::Number(_) => true,
                        })
                    })
                    .cloned()
                    .collect()
            };

            // Pass `Some(version)` when we have a snapshotted version for
            // this URI so LSP 3.17 clients can use the standard
            // PublishDiagnosticsParams.version slot to discard any
            // already-superseded publish. URIs not in the snapshot (file
            // never `did_open`'d via the LSP) get `None`.
            self.client
                .publish_diagnostics(uri.clone(), filtered.clone(), snapshot.get(uri).copied())
                .await;

            // Cache for pull-model requests (textDocument/diagnostic)
            self.cached_diagnostics
                .write()
                .await
                .insert(uri.clone(), filtered);
        }

        // Clear stale diagnostics: send empty arrays for URIs that had
        // diagnostics in the previous run but not in this one. Skip the
        // empty publish (and the cache eviction) for URIs that have
        // themselves moved past the snapshot, so we do not erase
        // last-valid diagnostics on the client while the user is editing.
        {
            let previous_uris = self.previous_diagnostic_uris.read().await;
            let mut cache = self.cached_diagnostics.write().await;
            for old_uri in previous_uris.iter() {
                if new_uris.contains(old_uri) {
                    continue;
                }
                if Self::uri_is_stale(old_uri, snapshot, &live_versions) {
                    // Keep the URI tracked so the next valid run can
                    // either republish a fresh result or perform the
                    // clear once the analysis catches up.
                    new_uris.insert(old_uri.clone());
                    continue;
                }
                self.client
                    .publish_diagnostics(old_uri.clone(), vec![], snapshot.get(old_uri).copied())
                    .await;
                cache.remove(old_uri);
            }
        }

        // Update the tracked URIs for next run
        *self.previous_diagnostic_uris.write().await = new_uris;
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("fallow=info")
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::build(FallowLspServer::new)
        .custom_method("fallow/issueTypes", FallowLspServer::issue_types)
        .finish();

    Server::new(stdin, stdout, socket).serve(service).await;
}

/// Find all project roots under a workspace directory.
///
/// Uses the workspace root plus any configured monorepo workspaces
/// (package.json `workspaces`, pnpm-workspace.yaml, tsconfig references).
/// All returned paths are canonicalized so they agree with the canonical
/// `git_toplevel` used by the `--changed-since` filter; otherwise file
/// paths in `AnalysisResults` and the changed-files set start from
/// different prefixes for the same files (e.g. `/tmp/x` vs `/private/tmp/x`
/// on macOS) and the filter silently drops everything.
fn find_project_roots(workspace_root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut roots = vec![workspace_root.to_path_buf()];

    let workspaces = fallow_config::discover_workspaces(workspace_root);
    for ws in &workspaces {
        roots.push(ws.root.clone());
    }

    for root in &mut roots {
        if let Ok(canon) = root.canonicalize() {
            *root = canon;
        }
    }

    roots.sort();
    roots.dedup();
    roots
}

/// Stamp `Diagnostic.data` with `{ "changedSince": "<git_ref>" }` on every
/// diagnostic when the LSP applied a `changedSince` filter to this run.
///
/// AI agents reading the Problems panel via `vscode.languages
/// .getDiagnostics()` can use this payload to verify that the filter is
/// active and skip "fixing" findings that the user has explicitly
/// baselined out. Standard LSP `Diagnostic.data` slot, no invented
/// top-level field. No-op when `changed_since` is `None` so unfiltered
/// runs ship a clean schema.
///
/// Merges into any existing `data` object rather than overwriting, so a
/// future `build_diagnostics` that stamps `data` for `codeAction/resolve`
/// tokens (the natural next step for code-action performance) does not
/// silently lose its payload to this stamp. If `data` is already a
/// non-object (string / number / array), the existing value is left alone
/// and `changedSince` is not stamped on that one diagnostic; that case is
/// not used by `build_diagnostics` today and is logged via the structured
/// fact that `data` for any fallow diagnostic should be an object.
fn attach_changed_since_data(
    diagnostics_by_file: &mut FxHashMap<Url, Vec<Diagnostic>>,
    changed_since: Option<&str>,
) {
    let Some(git_ref) = changed_since else {
        return;
    };
    let value = serde_json::Value::String(git_ref.to_string());
    for diags in diagnostics_by_file.values_mut() {
        for d in diags {
            match d.data.as_mut() {
                None => {
                    d.data = Some(serde_json::json!({ "changedSince": git_ref }));
                }
                Some(serde_json::Value::Object(obj)) => {
                    obj.insert("changedSince".to_string(), value.clone());
                }
                // Non-object existing payload: leave it intact. Fallow's
                // own diagnostics never set `data` to a non-object today;
                // if a future caller does, they get to keep their value.
                Some(_) => {}
            }
        }
    }
}

/// Drop entries with duplicate identity keys, preserving the original
/// insertion order of the first occurrence.
///
/// Identity-based dedup helper: two entries with the same key are
/// considered the same finding (e.g., same file at same line/col)
/// regardless of any other fields. Used by [`dedup_results`] to collapse
/// the cross-root duplicates that `merge_results` accumulates when a
/// monorepo's workspace root and a sub-package both walk the same source
/// files.
///
/// Order preservation matters: `build_diagnostics` and downstream
/// consumers receive results in the order detection emitted them, which
/// for many issue types is source-position-aligned. Sort-then-dedup would
/// silently reorder diagnostics; the `FxHashSet`-backed retain here
/// keeps the contract intact.
fn dedup_by_key_preserving_order<T, K, F>(vec: &mut Vec<T>, mut key: F)
where
    K: Eq + std::hash::Hash,
    F: FnMut(&T) -> K,
{
    let mut seen: FxHashSet<K> = FxHashSet::default();
    vec.retain(|item| seen.insert(key(item)));
}

/// Collapse cross-root duplicates in `target`.
///
/// `merge_results` accumulates findings from every project root (the
/// workspace root plus each sub-package in `find_project_roots`). When two
/// roots overlap (the most common case is the workspace root and a
/// sub-package both walking `apps/web/src/foo.ts`), the same finding
/// appears N times in the merged vec and `build_diagnostics` produces N
/// stacked diagnostics on the same range. Identity-based dedup here
/// removes the duplicates without collapsing genuinely distinct findings:
/// the same export *name* in two different files keeps both entries
/// because the keys include the file path.
///
/// `UnlistedDependency` is the one case that gets a real merge instead of
/// a plain dedup: two roots typically observe overlapping but non-equal
/// `imported_from` site lists for the same package, and the union is the
/// correct combined view (no over- or under-reporting). All other types
/// are deterministic per (path, position) so plain key-based dedup is
/// sufficient.
fn dedup_results(target: &mut AnalysisResults) {
    dedup_by_key_preserving_order(&mut target.unused_files, |f| f.file.path.clone());
    dedup_by_key_preserving_order(&mut target.unused_exports, |e| {
        (
            e.export.path.clone(),
            e.export.export_name.clone(),
            e.export.line,
            e.export.col,
        )
    });
    dedup_by_key_preserving_order(&mut target.unused_types, |e| {
        (
            e.export.path.clone(),
            e.export.export_name.clone(),
            e.export.line,
            e.export.col,
        )
    });
    dedup_by_key_preserving_order(&mut target.private_type_leaks, |e| {
        (
            e.leak.path.clone(),
            e.leak.export_name.clone(),
            e.leak.type_name.clone(),
            e.leak.line,
            e.leak.col,
        )
    });
    dedup_by_key_preserving_order(&mut target.unused_dependencies, |d| {
        (d.dep.package_name.clone(), d.dep.path.clone(), d.dep.line)
    });
    dedup_by_key_preserving_order(&mut target.unused_dev_dependencies, |d| {
        (d.dep.package_name.clone(), d.dep.path.clone(), d.dep.line)
    });
    dedup_by_key_preserving_order(&mut target.unused_optional_dependencies, |d| {
        (d.dep.package_name.clone(), d.dep.path.clone(), d.dep.line)
    });
    dedup_by_key_preserving_order(&mut target.unused_enum_members, |m| {
        (
            m.member.path.clone(),
            m.member.parent_name.clone(),
            m.member.member_name.clone(),
        )
    });
    dedup_by_key_preserving_order(&mut target.unused_class_members, |m| {
        (
            m.member.path.clone(),
            m.member.parent_name.clone(),
            m.member.member_name.clone(),
        )
    });
    dedup_by_key_preserving_order(&mut target.unresolved_imports, |i| {
        (
            i.import.path.clone(),
            i.import.specifier.clone(),
            i.import.line,
            i.import.col,
        )
    });
    dedup_by_key_preserving_order(&mut target.duplicate_exports, |d| {
        // `locations` is a Vec<DuplicateLocation>; sort the paths so two
        // roots that emitted the same group in different orders collapse
        // to one identity.
        let mut locs: Vec<_> = d
            .export
            .locations
            .iter()
            .map(|l| (l.path.clone(), l.line, l.col))
            .collect();
        locs.sort();
        (d.export.export_name.clone(), locs)
    });
    dedup_by_key_preserving_order(&mut target.type_only_dependencies, |d| {
        (d.dep.package_name.clone(), d.dep.path.clone(), d.dep.line)
    });
    dedup_by_key_preserving_order(&mut target.test_only_dependencies, |d| {
        (d.dep.package_name.clone(), d.dep.path.clone(), d.dep.line)
    });
    dedup_by_key_preserving_order(&mut target.circular_dependencies, |c| {
        let mut files: Vec<_> = c.cycle.files.clone();
        files.sort();
        (files, c.cycle.length)
    });
    dedup_by_key_preserving_order(&mut target.boundary_violations, |v| {
        (
            v.violation.from_path.clone(),
            v.violation.to_path.clone(),
            v.violation.import_specifier.clone(),
            v.violation.line,
            v.violation.col,
        )
    });
    dedup_by_key_preserving_order(&mut target.export_usages, |u| {
        (u.path.clone(), u.export_name.clone(), u.line, u.col)
    });
    dedup_by_key_preserving_order(&mut target.stale_suppressions, |s| {
        (s.path.clone(), s.line, s.col)
    });
    dedup_by_key_preserving_order(&mut target.unused_catalog_entries, |e| {
        (
            e.entry.path.clone(),
            e.entry.catalog_name.clone(),
            e.entry.entry_name.clone(),
        )
    });
    dedup_by_key_preserving_order(&mut target.empty_catalog_groups, |g| {
        (g.group.path.clone(), g.group.catalog_name.clone())
    });
    dedup_by_key_preserving_order(&mut target.unresolved_catalog_references, |f| {
        (
            f.reference.path.clone(),
            f.reference.line,
            f.reference.catalog_name.clone(),
            f.reference.entry_name.clone(),
        )
    });
    dedup_by_key_preserving_order(&mut target.unused_dependency_overrides, |o| {
        (
            o.entry.path.clone(),
            o.entry.source,
            o.entry.raw_key.clone(),
        )
    });
    dedup_by_key_preserving_order(&mut target.misconfigured_dependency_overrides, |o| {
        (
            o.entry.path.clone(),
            o.entry.source,
            o.entry.raw_key.clone(),
        )
    });

    // UnlistedDependency: real merge, not plain dedup. The same package can
    // be reported by two roots with different `imported_from` site lists
    // (each root sees only the imports inside its subtree). Collapse to
    // one entry per package_name with the union of import sites; keep
    // sites stable-sorted for deterministic output.
    if target.unlisted_dependencies.len() > 1 {
        let mut merged: FxHashMap<String, fallow_core::results::UnlistedDependencyFinding> =
            FxHashMap::default();
        for dep in target.unlisted_dependencies.drain(..) {
            merged
                .entry(dep.dep.package_name.clone())
                .and_modify(|existing| {
                    existing
                        .dep
                        .imported_from
                        .extend(dep.dep.imported_from.clone());
                })
                .or_insert(dep);
        }
        target.unlisted_dependencies = merged.into_values().collect();
        for dep in &mut target.unlisted_dependencies {
            // Dedup imported_from by (path, line, col) so a site that two
            // roots both observed lands as a single ImportSite.
            dedup_by_key_preserving_order(&mut dep.dep.imported_from, |s| {
                (s.path.clone(), s.line, s.col)
            });
        }
        target
            .unlisted_dependencies
            .sort_by(|a, b| a.dep.package_name.cmp(&b.dep.package_name));
    }
}

/// Merge analysis results from a sub-project into the accumulated results.
fn merge_results(target: &mut AnalysisResults, source: AnalysisResults) {
    target.unused_files.extend(source.unused_files);
    target.unused_exports.extend(source.unused_exports);
    target.unused_types.extend(source.unused_types);
    target.private_type_leaks.extend(source.private_type_leaks);
    target
        .unused_dependencies
        .extend(source.unused_dependencies);
    target
        .unused_dev_dependencies
        .extend(source.unused_dev_dependencies);
    target
        .unused_optional_dependencies
        .extend(source.unused_optional_dependencies);
    target
        .unused_enum_members
        .extend(source.unused_enum_members);
    target
        .unused_class_members
        .extend(source.unused_class_members);
    target.unresolved_imports.extend(source.unresolved_imports);
    target
        .unlisted_dependencies
        .extend(source.unlisted_dependencies);
    target.duplicate_exports.extend(source.duplicate_exports);
    target
        .type_only_dependencies
        .extend(source.type_only_dependencies);
    target
        .circular_dependencies
        .extend(source.circular_dependencies);
    target
        .test_only_dependencies
        .extend(source.test_only_dependencies);
    target
        .boundary_violations
        .extend(source.boundary_violations);
    target.export_usages.extend(source.export_usages);
    target.stale_suppressions.extend(source.stale_suppressions);
    target
        .unused_catalog_entries
        .extend(source.unused_catalog_entries);
    target
        .empty_catalog_groups
        .extend(source.empty_catalog_groups);
    target
        .unresolved_catalog_references
        .extend(source.unresolved_catalog_references);
    target
        .unused_dependency_overrides
        .extend(source.unused_dependency_overrides);
    target
        .misconfigured_dependency_overrides
        .extend(source.misconfigured_dependency_overrides);
}

/// Merge duplication reports from a sub-project into the accumulated report.
fn merge_duplication(target: &mut DuplicationReport, source: DuplicationReport) {
    target.clone_groups.extend(source.clone_groups);
    target.clone_families.extend(source.clone_families);
    target
        .mirrored_directories
        .extend(source.mirrored_directories);
    target.stats.clone_groups += source.stats.clone_groups;
    target.stats.clone_instances += source.stats.clone_instances;
    target.stats.total_files += source.stats.total_files;
    target.stats.files_with_clones += source.stats.files_with_clones;
    target.stats.total_lines += source.stats.total_lines;
    target.stats.duplicated_lines += source.stats.duplicated_lines;
    target.stats.total_tokens += source.stats.total_tokens;
    target.stats.duplicated_tokens += source.stats.duplicated_tokens;
    // Recompute percentage from merged totals (don't sum sub-project percentages)
    target.stats.duplication_percentage = if target.stats.total_lines > 0 {
        (target.stats.duplicated_lines as f64 / target.stats.total_lines as f64) * 100.0
    } else {
        0.0
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    use fallow_core::duplicates::{CloneGroup, CloneInstance, DuplicationStats};
    use fallow_core::results::{
        BoundaryViolation, BoundaryViolationFinding, CircularDependency, CircularDependencyFinding,
        ExportUsage, TestOnlyDependency, TestOnlyDependencyFinding, TypeOnlyDependency,
        UnlistedDependency, UnlistedDependencyFinding, UnusedClassMemberFinding, UnusedDependency,
        UnusedDependencyFinding, UnusedDevDependencyFinding, UnusedEnumMemberFinding, UnusedExport,
        UnusedExportFinding, UnusedFile, UnusedFileFinding, UnusedMember,
        UnusedOptionalDependencyFinding, UnusedTypeFinding,
    };
    use serde_json::json;
    use tower::{Service, ServiceExt};
    use tower_lsp::jsonrpc::Request;

    // -----------------------------------------------------------------------
    // build_server_capabilities
    // -----------------------------------------------------------------------

    #[test]
    fn server_capabilities_advertise_pull_diagnostics() {
        let caps = build_server_capabilities();
        let provider = caps
            .diagnostic_provider
            .expect("diagnostic_provider must be advertised so strict LSP 3.17 clients (Helix, Zed) call textDocument/diagnostic");
        match provider {
            DiagnosticServerCapabilities::Options(opts) => {
                assert_eq!(opts.identifier.as_deref(), Some("fallow"));
                assert!(
                    opts.inter_file_dependencies,
                    "fallow diagnostics span files; clients must re-pull related files on changes"
                );
                assert!(
                    !opts.workspace_diagnostics,
                    "no workspace/diagnostic handler is registered"
                );
            }
            DiagnosticServerCapabilities::RegistrationOptions(_) => {
                panic!("dynamic registration not supported");
            }
        }
    }

    #[test]
    fn server_capabilities_keep_existing_providers() {
        let caps = build_server_capabilities();
        assert!(caps.text_document_sync.is_some());
        assert!(caps.code_action_provider.is_some());
        assert!(caps.code_lens_provider.is_some());
        assert!(caps.hover_provider.is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_sets_cancellation_flag() {
        let (service, _) = LspService::build(FallowLspServer::new).finish();
        let backend = service.inner();
        assert!(
            !backend.cancellation.load(Ordering::SeqCst),
            "cancellation flag must start cleared",
        );
        backend.shutdown().await.expect("shutdown returns Ok");
        assert!(
            backend.cancellation.load(Ordering::SeqCst),
            "shutdown must flip the cancellation flag so subsequent did_save short-circuits",
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_analysis_short_circuits_after_shutdown() {
        let (service, _) = LspService::build(FallowLspServer::new).finish();
        let backend = service.inner();
        // Set a workspace root so the flag check, not the missing-root
        // check, is what would normally let analysis proceed. After
        // shutdown the cancellation gate at the top of `run_analysis`
        // must short-circuit before `spawn_blocking` populates
        // `self.results`. Asserting on `results.is_none()` is the
        // post-condition that proves the short-circuit fired; a
        // try_lock-based assertion would be vacuous because try_lock
        // is non-blocking and the guard is released on return.
        *backend.root.write().await = Some(std::env::temp_dir());
        backend.shutdown().await.expect("shutdown returns Ok");
        backend.run_analysis().await;
        assert!(
            backend.results.read().await.is_none(),
            "results must stay None when run_analysis short-circuits on cancellation",
        );
    }

    #[test]
    fn diagnostic_issue_types_include_all_lsp_codes_in_user_order() {
        let issue_types = diagnostic_issue_types();
        let codes: Vec<&str> = issue_types
            .iter()
            .map(|issue| issue.code.as_str())
            .collect();

        assert_eq!(codes.first(), Some(&"code-duplication"));
        assert!(codes.contains(&"unused-file"));
        assert!(codes.contains(&"private-type-leak"));
        assert!(codes.contains(&"test-only-dependency"));
        assert!(codes.contains(&"boundary-violation"));
        assert!(codes.contains(&"stale-suppression"));
        assert_eq!(
            issue_types
                .iter()
                .find(|issue| issue.code == "test-only-dependency")
                .map(|issue| issue.label.as_str()),
            Some("Test-Only Dependencies")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn text_document_diagnostic_request_is_served() {
        let (mut service, _) = LspService::build(FallowLspServer::new).finish();

        let initialize = Request::build("initialize")
            .params(json!({"capabilities": {}}))
            .id(1)
            .finish();
        let response = service
            .ready()
            .await
            .expect("service should be ready")
            .call(initialize)
            .await
            .expect("initialize request should be handled")
            .expect("initialize request should return a response");
        assert!(response.is_ok());

        let diagnostics = Request::build("textDocument/diagnostic")
            .params(json!({
                "textDocument": {
                    "uri": "file:///workspace/src/example.ts"
                },
                "identifier": "fallow"
            }))
            .id(2)
            .finish();
        let response = service
            .ready()
            .await
            .expect("service should be ready")
            .call(diagnostics)
            .await
            .expect("diagnostic request should be handled")
            .expect("diagnostic request should return a response");

        assert!(
            response.is_ok(),
            "textDocument/diagnostic must not return method_not_found"
        );
        let result = response.result().expect("diagnostic response should be ok");
        assert_eq!(result["kind"], json!("full"));
        assert_eq!(result["items"], json!([]));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fallow_issue_types_request_is_served() {
        let (mut service, _) = LspService::build(FallowLspServer::new)
            .custom_method("fallow/issueTypes", FallowLspServer::issue_types)
            .finish();

        let initialize = Request::build("initialize")
            .params(json!({"capabilities": {}}))
            .id(1)
            .finish();
        let response = service
            .ready()
            .await
            .expect("service should be ready")
            .call(initialize)
            .await
            .expect("initialize request should be handled")
            .expect("initialize request should return a response");
        assert!(response.is_ok());

        let issue_types = Request::build("fallow/issueTypes").id(2).finish();
        let response = service
            .ready()
            .await
            .expect("service should be ready")
            .call(issue_types)
            .await
            .expect("custom request should be handled")
            .expect("custom request should return a response");

        assert!(
            response.is_ok(),
            "fallow/issueTypes must not return method_not_found"
        );
        let result = response
            .result()
            .expect("issue type response should be ok")
            .as_array()
            .expect("issue type response should be an array");
        assert_eq!(
            result.first().and_then(|v| v["code"].as_str()),
            Some("code-duplication")
        );
        assert!(
            result
                .iter()
                .any(|v| v["code"] == json!("test-only-dependency")
                    && v["label"] == json!("Test-Only Dependencies")),
            "response should include every diagnostic code emitted by fallow-lsp"
        );
    }

    #[test]
    fn initialization_config_path_resolves_workspace_relative_path() {
        let opts = json!({"configPath": "config/fallow.json"});
        let root = Path::new("/workspace");

        assert_eq!(
            initialization_config_path(&opts, Some(root)),
            Some(PathBuf::from("/workspace/config/fallow.json"))
        );
    }

    #[test]
    fn initialization_config_path_ignores_blank_path() {
        let opts = json!({"configPath": "   "});

        assert_eq!(initialization_config_path(&opts, None), None);
    }

    #[test]
    fn initialization_config_path_passes_through_absolute_path() {
        #[cfg(windows)]
        let absolute = "C:/configs/fallow.json";
        #[cfg(not(windows))]
        let absolute = "/etc/fallow.json";

        let opts = json!({ "configPath": absolute });
        assert_eq!(
            initialization_config_path(&opts, None),
            Some(PathBuf::from(absolute))
        );
    }

    #[test]
    fn initialization_config_path_keeps_relative_path_without_root() {
        let opts = json!({"configPath": "config/fallow.json"});

        assert_eq!(
            initialization_config_path(&opts, None),
            Some(PathBuf::from("config/fallow.json"))
        );
    }

    #[test]
    fn initialization_config_path_returns_none_for_missing_key() {
        let opts = json!({});

        assert_eq!(initialization_config_path(&opts, None), None);
    }

    #[test]
    fn initialization_config_path_returns_none_for_non_string_value() {
        let opts = json!({"configPath": 42});

        assert_eq!(initialization_config_path(&opts, None), None);
    }

    // -----------------------------------------------------------------------
    // merge_results
    // -----------------------------------------------------------------------

    #[test]
    fn merge_results_into_empty_target() {
        let mut target = AnalysisResults::default();
        let mut source = AnalysisResults::default();
        source
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: "/a.ts".into(),
            }));
        source
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: "/a.ts".into(),
                export_name: "foo".to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));

        merge_results(&mut target, source);

        assert_eq!(target.unused_files.len(), 1);
        assert_eq!(target.unused_exports.len(), 1);
    }

    #[test]
    fn merge_results_accumulates_from_multiple_sources() {
        let mut target = AnalysisResults::default();

        let mut source_a = AnalysisResults::default();
        source_a
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: "/a.ts".into(),
            }));
        source_a.unresolved_imports.push(
            fallow_core::results::UnresolvedImportFinding::with_actions(
                fallow_core::results::UnresolvedImport {
                    path: "/a.ts".into(),
                    specifier: "./missing".to_string(),
                    line: 1,
                    col: 0,
                    specifier_col: 10,
                },
            ),
        );

        let mut source_b = AnalysisResults::default();
        source_b
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: "/b.ts".into(),
            }));
        source_b
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: "/b.ts".into(),
                export_name: "bar".to_string(),
                is_type_only: false,
                line: 5,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));

        merge_results(&mut target, source_a);
        merge_results(&mut target, source_b);

        assert_eq!(target.unused_files.len(), 2);
        assert_eq!(target.unused_exports.len(), 1);
        assert_eq!(target.unresolved_imports.len(), 1);
    }

    fn merge_test_unused_export(
        path: &str,
        export_name: &str,
        is_type_only: bool,
        line: u32,
    ) -> UnusedExport {
        UnusedExport {
            path: path.into(),
            export_name: export_name.to_string(),
            is_type_only,
            line,
            col: 0,
            span_start: 0,
            is_re_export: false,
        }
    }

    fn merge_test_unused_dependency(
        package_name: &str,
        location: fallow_core::results::DependencyLocation,
        line: u32,
    ) -> UnusedDependency {
        UnusedDependency {
            package_name: package_name.to_string(),
            location,
            path: "/pkg.json".into(),
            line,
            used_in_workspaces: Vec::new(),
        }
    }

    fn merge_test_unused_member(
        parent_name: &str,
        member_name: &str,
        kind: fallow_core::extract::MemberKind,
        line: u32,
    ) -> UnusedMember {
        UnusedMember {
            path: "/f.ts".into(),
            parent_name: parent_name.to_string(),
            member_name: member_name.to_string(),
            kind,
            line,
            col: 0,
        }
    }

    fn merge_test_source_with_all_fields() -> AnalysisResults {
        AnalysisResults {
            unused_files: vec![UnusedFileFinding::with_actions(UnusedFile {
                path: "/f.ts".into(),
            })],
            unused_exports: vec![UnusedExportFinding::with_actions(merge_test_unused_export(
                "/f.ts", "e", false, 1,
            ))],
            unused_types: vec![UnusedTypeFinding::with_actions(merge_test_unused_export(
                "/f.ts", "T", true, 2,
            ))],
            unused_dependencies: vec![UnusedDependencyFinding::with_actions(
                merge_test_unused_dependency(
                    "dep",
                    fallow_core::results::DependencyLocation::Dependencies,
                    3,
                ),
            )],
            unused_dev_dependencies: vec![UnusedDevDependencyFinding::with_actions(
                merge_test_unused_dependency(
                    "dev-dep",
                    fallow_core::results::DependencyLocation::DevDependencies,
                    4,
                ),
            )],
            unused_optional_dependencies: vec![UnusedOptionalDependencyFinding::with_actions(
                merge_test_unused_dependency(
                    "opt-dep",
                    fallow_core::results::DependencyLocation::OptionalDependencies,
                    5,
                ),
            )],
            unused_enum_members: vec![UnusedEnumMemberFinding::with_actions(
                merge_test_unused_member("E", "A", fallow_core::extract::MemberKind::EnumMember, 6),
            )],
            unused_class_members: vec![UnusedClassMemberFinding::with_actions(
                merge_test_unused_member(
                    "C",
                    "m",
                    fallow_core::extract::MemberKind::ClassMethod,
                    7,
                ),
            )],
            unresolved_imports: vec![fallow_core::results::UnresolvedImportFinding::with_actions(
                fallow_core::results::UnresolvedImport {
                    path: "/f.ts".into(),
                    specifier: "./gone".to_string(),
                    line: 8,
                    col: 0,
                    specifier_col: 10,
                },
            )],
            unlisted_dependencies: vec![UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "unlisted".to_string(),
                    imported_from: vec![],
                },
            )],
            duplicate_exports: vec![fallow_core::results::DuplicateExportFinding::with_actions(
                fallow_core::results::DuplicateExport {
                    export_name: "dup".to_string(),
                    locations: vec![],
                },
            )],
            type_only_dependencies: vec![
                fallow_core::results::TypeOnlyDependencyFinding::with_actions(TypeOnlyDependency {
                    package_name: "type-only".to_string(),
                    path: "/pkg.json".into(),
                    line: 9,
                }),
            ],
            circular_dependencies: vec![CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec!["/a.ts".into(), "/b.ts".into()],
                    length: 2,
                    line: 10,
                    col: 0,
                    is_cross_package: false,
                },
            )],
            test_only_dependencies: vec![TestOnlyDependencyFinding::with_actions(
                TestOnlyDependency {
                    package_name: "test-only".to_string(),
                    path: "/pkg.json".into(),
                    line: 11,
                },
            )],
            boundary_violations: vec![BoundaryViolationFinding::with_actions(BoundaryViolation {
                from_path: "/a.ts".into(),
                to_path: "/b.ts".into(),
                from_zone: "ui".to_string(),
                to_zone: "data".to_string(),
                import_specifier: "../data/db".to_string(),
                line: 12,
                col: 0,
            })],
            export_usages: vec![ExportUsage {
                path: "/f.ts".into(),
                export_name: "used".to_string(),
                line: 13,
                col: 0,
                reference_count: 3,
                reference_locations: vec![],
            }],
            ..Default::default()
        }
    }

    #[test]
    fn merge_results_covers_all_fields() {
        let mut target = AnalysisResults::default();
        let source = merge_test_source_with_all_fields();

        merge_results(&mut target, source);

        assert_eq!(target.unused_files.len(), 1);
        assert_eq!(target.unused_exports.len(), 1);
        assert_eq!(target.unused_types.len(), 1);
        assert_eq!(target.unused_dependencies.len(), 1);
        assert_eq!(target.unused_dev_dependencies.len(), 1);
        assert_eq!(target.unused_optional_dependencies.len(), 1);
        assert_eq!(target.unused_enum_members.len(), 1);
        assert_eq!(target.unused_class_members.len(), 1);
        assert_eq!(target.unresolved_imports.len(), 1);
        assert_eq!(target.unlisted_dependencies.len(), 1);
        assert_eq!(target.duplicate_exports.len(), 1);
        assert_eq!(target.type_only_dependencies.len(), 1);
        assert_eq!(target.circular_dependencies.len(), 1);
        assert_eq!(target.test_only_dependencies.len(), 1);
        assert_eq!(target.boundary_violations.len(), 1);
        assert_eq!(target.export_usages.len(), 1);
    }

    #[test]
    fn merge_results_with_empty_source() {
        let mut target = AnalysisResults::default();
        target
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: "/a.ts".into(),
            }));

        let source = AnalysisResults::default();
        merge_results(&mut target, source);

        // Target should be unchanged
        assert_eq!(target.unused_files.len(), 1);
    }

    // -----------------------------------------------------------------------
    // dedup_results: cross-root collapse.
    //
    // In monorepos `find_project_roots` returns the workspace root plus
    // each sub-package. Two roots that overlap walk the same source files
    // and emit identical findings; `merge_results` extends both into the
    // accumulated vec. Without `dedup_results`, the LSP publishes N
    // stacked diagnostics on the same range. These tests pin the per-type
    // identity keys so a future refactor that collapses two genuinely
    // distinct findings (e.g., same export name in two different files)
    // breaks loudly.
    // -----------------------------------------------------------------------

    #[test]
    fn dedup_results_collapses_cross_root_unused_files() {
        let mut results = AnalysisResults::default();
        // Workspace-root pass and sub-package pass both walked the same file.
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: "/repo/apps/web/src/foo.ts".into(),
            }));
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: "/repo/apps/web/src/foo.ts".into(),
            }));
        // A genuinely distinct unused file.
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: "/repo/apps/api/src/bar.ts".into(),
            }));

        dedup_results(&mut results);

        assert_eq!(results.unused_files.len(), 2);
    }

    #[test]
    fn dedup_results_keeps_same_export_name_in_distinct_files() {
        // Two files both export `helper`. Identity is (path, name, line, col),
        // so these stay as two separate findings even though the name is
        // identical. The user explicitly called this out as a regression
        // we must not introduce.
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: "/a.ts".into(),
                export_name: "helper".to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: "/b.ts".into(),
                export_name: "helper".to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        // Cross-root duplicate of the first.
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: "/a.ts".into(),
                export_name: "helper".to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));

        dedup_results(&mut results);

        assert_eq!(results.unused_exports.len(), 2);
    }

    #[test]
    fn dedup_results_keeps_distinct_circular_dependencies() {
        let mut results = AnalysisResults::default();
        let cycle_ab = CircularDependencyFinding::with_actions(CircularDependency {
            files: vec!["/a.ts".into(), "/b.ts".into()],
            length: 2,
            line: 1,
            col: 0,
            is_cross_package: false,
        });
        let cycle_cd = CircularDependencyFinding::with_actions(CircularDependency {
            files: vec!["/c.ts".into(), "/d.ts".into()],
            length: 2,
            line: 5,
            col: 0,
            is_cross_package: false,
        });
        // Same cycle observed by two roots, with files in different orders.
        let cycle_ab_reversed = CircularDependencyFinding::with_actions(CircularDependency {
            files: vec!["/b.ts".into(), "/a.ts".into()],
            length: 2,
            line: 1,
            col: 0,
            is_cross_package: false,
        });
        results
            .circular_dependencies
            .extend([cycle_ab, cycle_cd, cycle_ab_reversed]);

        dedup_results(&mut results);

        // {a,b} and {c,d} survive; the reordered duplicate of {a,b}
        // collapses because the dedup key sorts the file list.
        assert_eq!(results.circular_dependencies.len(), 2);
    }

    #[test]
    fn dedup_results_merges_unlisted_dependency_imported_from() {
        // Workspace root sees `lodash` imported from packages/a + packages/b.
        // Sub-package root for packages/a sees `lodash` imported from
        // packages/a only. Without merging, the user gets two `lodash`
        // entries in the Problems panel; with merging, they get one with
        // the union of import sites.
        let mut results = AnalysisResults::default();
        results
            .unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "lodash".to_string(),
                    imported_from: vec![
                        fallow_core::results::ImportSite {
                            path: "/repo/packages/a/x.ts".into(),
                            line: 1,
                            col: 0,
                        },
                        fallow_core::results::ImportSite {
                            path: "/repo/packages/b/y.ts".into(),
                            line: 2,
                            col: 0,
                        },
                    ],
                },
            ));
        results
            .unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "lodash".to_string(),
                    imported_from: vec![fallow_core::results::ImportSite {
                        path: "/repo/packages/a/x.ts".into(),
                        line: 1,
                        col: 0,
                    }],
                },
            ));

        dedup_results(&mut results);

        assert_eq!(results.unlisted_dependencies.len(), 1);
        let merged = &results.unlisted_dependencies[0];
        assert_eq!(merged.dep.package_name, "lodash");
        assert_eq!(
            merged.dep.imported_from.len(),
            2,
            "imported_from should be the union of import sites, not duplicated"
        );
    }

    // -----------------------------------------------------------------------
    // attach_changed_since_data
    //
    // When the LSP scopes diagnostics with `changedSince`, every published
    // Diagnostic must carry a standard LSP `data` payload with the active
    // ref so AI agents reading via `vscode.languages.getDiagnostics()` can
    // verify the filter and avoid acting on baseline-excluded findings.
    // When changedSince is None, no `data` is set so unfiltered runs
    // remain clean.
    // -----------------------------------------------------------------------

    fn make_diagnostic() -> Diagnostic {
        Diagnostic {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 5,
                },
            },
            severity: Some(DiagnosticSeverity::HINT),
            code: Some(NumberOrString::String("unused-export".to_string())),
            source: Some("fallow".to_string()),
            message: "Export 'helper' is unused".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn attach_changed_since_data_sets_payload_when_active() {
        let mut map: FxHashMap<Url, Vec<Diagnostic>> = FxHashMap::default();
        let uri = Url::parse("file:///a.ts").unwrap();
        map.insert(uri.clone(), vec![make_diagnostic(), make_diagnostic()]);

        attach_changed_since_data(&mut map, Some("fallow-baseline"));

        let diags = &map[&uri];
        for d in diags {
            assert_eq!(
                d.data,
                Some(serde_json::json!({ "changedSince": "fallow-baseline" })),
                "every diagnostic must carry data.changedSince when filter is active"
            );
        }
    }

    #[test]
    fn attach_changed_since_data_noop_when_filter_absent() {
        let mut map: FxHashMap<Url, Vec<Diagnostic>> = FxHashMap::default();
        let uri = Url::parse("file:///a.ts").unwrap();
        map.insert(uri.clone(), vec![make_diagnostic()]);

        attach_changed_since_data(&mut map, None);

        assert!(
            map[&uri][0].data.is_none(),
            "unfiltered runs must not stamp data.changedSince"
        );
    }

    #[test]
    fn attach_changed_since_data_handles_empty_map() {
        let mut map: FxHashMap<Url, Vec<Diagnostic>> = FxHashMap::default();
        attach_changed_since_data(&mut map, Some("origin/main"));
        assert!(map.is_empty());
    }

    #[test]
    fn attach_changed_since_data_merges_into_existing_object_data() {
        // Regression for the case where a future `build_diagnostics`
        // pre-populates `Diagnostic.data` (e.g., codeAction/resolve token).
        // The stamp must merge into that object, not overwrite it. Without
        // merge logic the resolve token would silently disappear and the
        // editor's lightbulb fix flow would break.
        let mut map: FxHashMap<Url, Vec<Diagnostic>> = FxHashMap::default();
        let uri = Url::parse("file:///a.ts").unwrap();
        let mut d = make_diagnostic();
        d.data = Some(serde_json::json!({ "resolveToken": "abc-123" }));
        map.insert(uri.clone(), vec![d]);

        attach_changed_since_data(&mut map, Some("fallow-baseline"));

        let merged = map[&uri][0].data.as_ref().unwrap();
        assert_eq!(merged["resolveToken"], "abc-123");
        assert_eq!(merged["changedSince"], "fallow-baseline");
    }

    #[test]
    fn attach_changed_since_data_leaves_non_object_data_intact() {
        // If a future caller stamped `data` to a non-object (string,
        // number, array), don't silently coerce or destroy it. This
        // shouldn't happen for fallow's own diagnostics (we always use
        // objects), but the stamp must be defensive.
        let mut map: FxHashMap<Url, Vec<Diagnostic>> = FxHashMap::default();
        let uri = Url::parse("file:///a.ts").unwrap();
        let mut d = make_diagnostic();
        d.data = Some(serde_json::Value::String("custom-token".to_string()));
        map.insert(uri.clone(), vec![d]);

        attach_changed_since_data(&mut map, Some("fallow-baseline"));

        assert_eq!(
            map[&uri][0].data,
            Some(serde_json::Value::String("custom-token".to_string())),
            "non-object data must be preserved verbatim"
        );
    }

    #[test]
    fn dedup_results_collapses_cross_root_dependencies() {
        let mut results = AnalysisResults::default();
        // Same package.json analyzed twice.
        for _ in 0..2 {
            results
                .unused_dependencies
                .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                    package_name: "lodash".to_string(),
                    location: fallow_core::results::DependencyLocation::Dependencies,
                    path: "/repo/package.json".into(),
                    line: 5,
                    used_in_workspaces: Vec::new(),
                }));
        }
        // Genuinely distinct: different package.json (sub-package).
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".to_string(),
                location: fallow_core::results::DependencyLocation::Dependencies,
                path: "/repo/packages/web/package.json".into(),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));

        dedup_results(&mut results);

        assert_eq!(results.unused_dependencies.len(), 2);
    }

    // -----------------------------------------------------------------------
    // merge_duplication
    // -----------------------------------------------------------------------

    #[test]
    fn merge_duplication_into_empty_target() {
        let mut target = DuplicationReport::default();
        let source = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![CloneInstance {
                    file: "/a.ts".into(),
                    start_line: 1,
                    end_line: 5,
                    start_col: 0,
                    end_col: 10,
                    fragment: "code".to_string(),
                }],
                token_count: 20,
                line_count: 5,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 10,
                files_with_clones: 2,
                total_lines: 100,
                duplicated_lines: 10,
                total_tokens: 500,
                duplicated_tokens: 50,
                clone_groups: 1,
                clone_instances: 1,
                duplication_percentage: 10.0,
                clone_groups_below_min_occurrences: 0,
            },
        };

        merge_duplication(&mut target, source);

        assert_eq!(target.clone_groups.len(), 1);
        assert_eq!(target.stats.total_files, 10);
        assert_eq!(target.stats.total_lines, 100);
        assert_eq!(target.stats.duplicated_lines, 10);
        assert!((target.stats.duplication_percentage - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn merge_duplication_recomputes_percentage() {
        let mut target = DuplicationReport {
            clone_groups: vec![],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 5,
                files_with_clones: 1,
                total_lines: 200,
                duplicated_lines: 20,
                total_tokens: 1000,
                duplicated_tokens: 100,
                clone_groups: 1,
                clone_instances: 2,
                duplication_percentage: 10.0, // 20/200 * 100
                clone_groups_below_min_occurrences: 0,
            },
        };
        let source = DuplicationReport {
            clone_groups: vec![],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 3,
                files_with_clones: 1,
                total_lines: 300,
                duplicated_lines: 60,
                total_tokens: 1500,
                duplicated_tokens: 300,
                clone_groups: 2,
                clone_instances: 4,
                duplication_percentage: 20.0, // 60/300 * 100
                clone_groups_below_min_occurrences: 0,
            },
        };

        merge_duplication(&mut target, source);

        // Merged: total_lines=500, duplicated_lines=80
        // Recomputed: 80/500 * 100 = 16.0 (NOT 10.0 + 20.0 = 30.0)
        assert_eq!(target.stats.total_files, 8);
        assert_eq!(target.stats.files_with_clones, 2);
        assert_eq!(target.stats.total_lines, 500);
        assert_eq!(target.stats.duplicated_lines, 80);
        assert_eq!(target.stats.total_tokens, 2500);
        assert_eq!(target.stats.duplicated_tokens, 400);
        assert_eq!(target.stats.clone_groups, 3);
        assert_eq!(target.stats.clone_instances, 6);
        assert!((target.stats.duplication_percentage - 16.0).abs() < f64::EPSILON);
    }

    #[test]
    fn merge_duplication_zero_total_lines_yields_zero_percentage() {
        let mut target = DuplicationReport::default();
        let source = DuplicationReport::default();

        merge_duplication(&mut target, source);

        assert_eq!(target.stats.total_lines, 0);
        assert!((target.stats.duplication_percentage - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn merge_duplication_with_empty_source() {
        let mut target = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![],
                token_count: 10,
                line_count: 3,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 5,
                files_with_clones: 1,
                total_lines: 100,
                duplicated_lines: 10,
                total_tokens: 500,
                duplicated_tokens: 50,
                clone_groups: 1,
                clone_instances: 1,
                duplication_percentage: 10.0,
                clone_groups_below_min_occurrences: 0,
            },
        };

        let source = DuplicationReport::default();
        merge_duplication(&mut target, source);

        // Target stats should remain the same (merged with zeros)
        assert_eq!(target.clone_groups.len(), 1);
        assert_eq!(target.stats.total_files, 5);
        assert!((target.stats.duplication_percentage - 10.0).abs() < f64::EPSILON);
    }

    // -----------------------------------------------------------------------
    // DIAGNOSTIC_ISSUE_TYPES
    // -----------------------------------------------------------------------

    #[test]
    fn issue_type_mapping_has_expected_entries() {
        // Verify all expected issue types are present
        let keys: Vec<&str> = DIAGNOSTIC_ISSUE_TYPES
            .iter()
            .filter_map(|issue_type| issue_type.config_key)
            .collect();

        assert!(keys.contains(&"unused-files"));
        assert!(keys.contains(&"unused-exports"));
        assert!(keys.contains(&"unused-types"));
        assert!(keys.contains(&"private-type-leaks"));
        assert!(keys.contains(&"unused-dependencies"));
        assert!(keys.contains(&"unused-dev-dependencies"));
        assert!(keys.contains(&"unused-optional-dependencies"));
        assert!(keys.contains(&"unused-enum-members"));
        assert!(keys.contains(&"unused-class-members"));
        assert!(keys.contains(&"unresolved-imports"));
        assert!(keys.contains(&"unlisted-dependencies"));
        assert!(keys.contains(&"duplicate-exports"));
        assert!(keys.contains(&"type-only-dependencies"));
        assert!(keys.contains(&"test-only-dependencies"));
        assert!(keys.contains(&"circular-dependencies"));
        assert!(keys.contains(&"boundary-violation"));
        assert!(keys.contains(&"stale-suppressions"));
    }

    #[test]
    fn issue_type_mapping_codes_are_singular() {
        // All diagnostic codes should be singular (e.g., "unused-file" not "unused-files")
        for issue_type in DIAGNOSTIC_ISSUE_TYPES {
            let Some(config_key) = issue_type.config_key else {
                continue;
            };
            // Config keys are plural, diagnostic codes are singular
            assert!(
                !issue_type.code.ends_with('s') || issue_type.code.ends_with("ss"),
                "Diagnostic code '{}' for config key '{config_key}' should be singular",
                issue_type.code
            );
        }
    }

    // -----------------------------------------------------------------------
    // publish_collected_diagnostics: stale-publish guard (issue #450)
    //
    // The LSP captures a per-URI version snapshot at `run_analysis` entry
    // and threads it into `publish_collected_diagnostics`. Any URI whose
    // live document version has advanced past the snapshot (or that has
    // been closed mid-run) is treated as STALE: its publish + cache update
    // are skipped, but the URI is still tracked so the next-run stale
    // clearer does not erase prior valid diagnostics from the client.
    // -----------------------------------------------------------------------

    async fn install_document(backend: &FallowLspServer, uri: &Url, version: i32, text: &str) {
        backend.documents.write().await.insert(
            uri.clone(),
            DocumentState {
                version,
                text: text.to_string(),
            },
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn publish_skips_uri_when_live_version_advanced_past_snapshot() {
        let (service, _) = LspService::build(FallowLspServer::new).finish();
        let backend = service.inner();

        let uri = Url::parse("file:///stale.ts").unwrap();
        install_document(backend, &uri, 1, "v1").await;
        let snapshot: VersionSnapshot = std::iter::once((uri.clone(), 1)).collect();

        // Simulate did_change landing between snapshot capture and publish.
        install_document(backend, &uri, 2, "v2").await;

        let mut diags_by_file: FxHashMap<Url, Vec<Diagnostic>> = FxHashMap::default();
        diags_by_file.insert(uri.clone(), vec![make_diagnostic()]);
        backend
            .publish_collected_diagnostics(diags_by_file, &snapshot)
            .await;

        assert!(
            !backend.cached_diagnostics.read().await.contains_key(&uri),
            "stale URI must not be cached: the diagnostics belong to the pre-edit document"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn publish_emits_when_live_version_equals_snapshot() {
        // Boundary case for the strict `>` comparison: equal versions are
        // NOT stale; the analysis ran against exactly the document the
        // client still holds.
        let (service, _) = LspService::build(FallowLspServer::new).finish();
        let backend = service.inner();

        let uri = Url::parse("file:///fresh.ts").unwrap();
        install_document(backend, &uri, 1, "v1").await;
        let snapshot: VersionSnapshot = std::iter::once((uri.clone(), 1)).collect();

        let mut diags_by_file: FxHashMap<Url, Vec<Diagnostic>> = FxHashMap::default();
        diags_by_file.insert(uri.clone(), vec![make_diagnostic()]);
        backend
            .publish_collected_diagnostics(diags_by_file, &snapshot)
            .await;

        let cached_len = backend
            .cached_diagnostics
            .read()
            .await
            .get(&uri)
            .map(Vec::len);
        assert_eq!(
            cached_len,
            Some(1),
            "equal versions are not stale; publish must reach the cache"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn publish_emits_when_uri_absent_from_snapshot_and_live() {
        // Diagnostics on files the user never `did_open`'d via the LSP
        // (e.g. unlisted-dependency findings on a `package.json`, catalog
        // reference findings on a `pnpm-workspace.yaml`) must publish
        // normally. With the URI absent from BOTH the snapshot AND the
        // live `documents` map, no version race exists.
        let (service, _) = LspService::build(FallowLspServer::new).finish();
        let backend = service.inner();

        let uri = Url::parse("file:///never-opened/package.json").unwrap();
        let snapshot: VersionSnapshot = FxHashMap::default();

        let mut diags_by_file: FxHashMap<Url, Vec<Diagnostic>> = FxHashMap::default();
        diags_by_file.insert(uri.clone(), vec![make_diagnostic()]);
        backend
            .publish_collected_diagnostics(diags_by_file, &snapshot)
            .await;

        assert!(
            backend.cached_diagnostics.read().await.contains_key(&uri),
            "URIs absent from BOTH snapshot AND live documents must publish",
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn publish_skips_uri_when_opened_mid_run() {
        // URI was absent from the snapshot (file was not open via the LSP
        // when analysis started) but is now present in live `documents`
        // (did_open landed between snapshot capture and publish). The
        // analysis ran without seeing this buffer; we have no version to
        // attach to a publish so the client cannot drop a mismatched
        // payload server-to-client. Skip until the next analysis cycle.
        let (service, _) = LspService::build(FallowLspServer::new).finish();
        let backend = service.inner();

        let uri = Url::parse("file:///opened-mid-run.ts").unwrap();
        let snapshot: VersionSnapshot = FxHashMap::default();

        // Simulate did_open landing between snapshot capture and publish.
        install_document(backend, &uri, 1, "v1").await;

        let mut diags_by_file: FxHashMap<Url, Vec<Diagnostic>> = FxHashMap::default();
        diags_by_file.insert(uri.clone(), vec![make_diagnostic()]);
        backend
            .publish_collected_diagnostics(diags_by_file, &snapshot)
            .await;

        assert!(
            !backend.cached_diagnostics.read().await.contains_key(&uri),
            "opened-mid-run URI must skip publish + cache update; analysis \
             did not see this buffer and we cannot version-stamp the publish",
        );
        assert!(
            backend.previous_diagnostic_uris.read().await.contains(&uri),
            "skipped opened-mid-run URI must still be tracked in new_uris \
             so the next-run stale-clearer does not fire an empty publish",
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn publish_skips_uri_when_closed_mid_run() {
        // URI was in the snapshot (file was open when analysis started)
        // but has since been removed from `documents` via did_close. We
        // cannot prove the client still owns the document, so treat as
        // stale and skip publish.
        let (service, _) = LspService::build(FallowLspServer::new).finish();
        let backend = service.inner();

        let uri = Url::parse("file:///closed.ts").unwrap();
        install_document(backend, &uri, 1, "v1").await;
        let snapshot: VersionSnapshot = std::iter::once((uri.clone(), 1)).collect();

        // Simulate did_close between snapshot capture and publish.
        backend.documents.write().await.remove(&uri);

        let mut diags_by_file: FxHashMap<Url, Vec<Diagnostic>> = FxHashMap::default();
        diags_by_file.insert(uri.clone(), vec![make_diagnostic()]);
        backend
            .publish_collected_diagnostics(diags_by_file, &snapshot)
            .await;

        assert!(
            !backend.cached_diagnostics.read().await.contains_key(&uri),
            "closed-mid-run URI must skip publish + cache update"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn publish_threads_snapshot_version_to_client() {
        // Drain the ClientSocket to inspect the actual JSON-RPC notification
        // emitted by `publish_diagnostics`. Asserts that the LSP 3.17
        // `version` slot carries the snapshot version (was always `None`
        // before this change). Must drive `initialize` first because
        // tower-lsp's `Client::send_notification` suppresses messages
        // until the server state is `Initialized`.
        use futures::StreamExt;

        let (mut service, socket) = LspService::build(FallowLspServer::new).finish();

        let initialize = Request::build("initialize")
            .params(json!({"capabilities": {}}))
            .id(1)
            .finish();
        service
            .ready()
            .await
            .expect("service ready")
            .call(initialize)
            .await
            .expect("initialize call")
            .expect("initialize response");

        let backend = service.inner();

        let uri = Url::parse("file:///versioned.ts").unwrap();
        install_document(backend, &uri, 7, "v7").await;
        let snapshot: VersionSnapshot = std::iter::once((uri.clone(), 7)).collect();

        let mut diags_by_file: FxHashMap<Url, Vec<Diagnostic>> = FxHashMap::default();
        diags_by_file.insert(uri.clone(), vec![make_diagnostic()]);
        backend
            .publish_collected_diagnostics(diags_by_file, &snapshot)
            .await;

        let mut socket = socket;
        // The client emits each log_message + publish through the same
        // socket stream. Drain until we find the publishDiagnostics
        // notification (skip the initialized acks / log messages).
        let request = loop {
            let next = tokio::time::timeout(Duration::from_millis(500), socket.next())
                .await
                .expect("publishDiagnostics notification must arrive within timeout")
                .expect("ClientSocket stream ended before yielding the notification");
            if next.method() == "textDocument/publishDiagnostics" {
                break next;
            }
        };

        let params = request
            .params()
            .expect("publishDiagnostics carries params on every call");
        assert_eq!(
            params["version"],
            serde_json::json!(7),
            "version slot must carry the snapshot version, not None",
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stale_clearing_skips_uri_when_live_version_advanced() {
        // Seed previous_diagnostic_uris with a URI by running a first
        // publish, then run a second publish with empty diagnostics_by_file
        // and a snapshot capturing the pre-edit version. The URI's live
        // version has moved on; the stale-clearing branch must NOT emit
        // an empty publish for it (which would erase last-valid diagnostics
        // from the client) and must NOT evict the cached entry.
        let (service, _) = LspService::build(FallowLspServer::new).finish();
        let backend = service.inner();

        let uri = Url::parse("file:///clearing.ts").unwrap();
        install_document(backend, &uri, 1, "v1").await;
        let snapshot_v1: VersionSnapshot = std::iter::once((uri.clone(), 1)).collect();

        let mut first_run: FxHashMap<Url, Vec<Diagnostic>> = FxHashMap::default();
        first_run.insert(uri.clone(), vec![make_diagnostic()]);
        backend
            .publish_collected_diagnostics(first_run, &snapshot_v1)
            .await;
        assert!(
            backend.cached_diagnostics.read().await.contains_key(&uri),
            "precondition: first run must seed the cache",
        );

        // User edits the file between runs; live version is now 2 but the
        // SECOND analysis ran against v1 (snapshot still v1).
        install_document(backend, &uri, 2, "v2").await;

        let empty: FxHashMap<Url, Vec<Diagnostic>> = FxHashMap::default();
        backend
            .publish_collected_diagnostics(empty, &snapshot_v1)
            .await;

        assert!(
            backend.cached_diagnostics.read().await.contains_key(&uri),
            "stale URI must NOT be evicted by the stale-clearing branch \
             when its live version has advanced past the snapshot"
        );
        assert!(
            backend.previous_diagnostic_uris.read().await.contains(&uri),
            "URI must remain tracked for the next-run stale-clearing pass",
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn publish_inserts_skipped_uri_into_new_uris() {
        // Positive assertion guarding the load-bearing detail that even
        // skipped (stale) URIs are inserted into `new_uris`. Without this,
        // the next run's stale-clearing loop would treat the URI as
        // "disappeared" and erase its last-valid diagnostics on the
        // client. Detects a regression where a future refactor "fixes"
        // the skip by also dropping the new_uris insertion.
        let (service, _) = LspService::build(FallowLspServer::new).finish();
        let backend = service.inner();

        let uri = Url::parse("file:///tracked.ts").unwrap();
        install_document(backend, &uri, 1, "v1").await;
        let snapshot: VersionSnapshot = std::iter::once((uri.clone(), 1)).collect();
        install_document(backend, &uri, 2, "v2").await;

        let mut diags_by_file: FxHashMap<Url, Vec<Diagnostic>> = FxHashMap::default();
        diags_by_file.insert(uri.clone(), vec![make_diagnostic()]);
        backend
            .publish_collected_diagnostics(diags_by_file, &snapshot)
            .await;

        assert!(
            backend.previous_diagnostic_uris.read().await.contains(&uri),
            "skipped stale URI must still be tracked in previous_diagnostic_uris",
        );
    }
}
