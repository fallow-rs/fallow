#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "tests use unwrap and expect to keep fixture setup concise"
    )
)]

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

use fallow_config::{DetectionMode, DuplicatesConfig};
use fallow_core::changed_files::{
    filter_duplication_by_changed_files, filter_results_by_changed_files, resolve_git_toplevel,
    try_get_changed_files_with_toplevel,
};
use fallow_core::duplicates::DuplicationReport;
use fallow_core::results::AnalysisResults;

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
    re_export_cycles: usize,
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
        config_key: Some("re-export-cycles"),
        code: "re-export-cycle",
        label: "Re-Export Cycles",
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
    duplication_options: Option<&LspDuplicationOptions>,
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
                    &DuplicatesConfig::default(),
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
    let duplicates_config = duplication_options.map_or_else(
        || config.duplicates.clone(),
        |options| options.merge_with(&config.duplicates),
    );
    let duplication =
        fallow_core::duplicates::find_duplicates(project_root, &files, &duplicates_config);
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

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct LspDuplicationOptions {
    mode: Option<DetectionMode>,
    threshold: Option<f64>,
    min_tokens: Option<usize>,
    min_lines: Option<usize>,
    min_occurrences: Option<usize>,
    skip_local: Option<bool>,
    cross_language: Option<bool>,
    ignore_imports: Option<bool>,
}

impl LspDuplicationOptions {
    fn merge_with(&self, config: &DuplicatesConfig) -> DuplicatesConfig {
        DuplicatesConfig {
            enabled: config.enabled,
            mode: self.mode.unwrap_or(config.mode),
            min_tokens: self.min_tokens.unwrap_or(config.min_tokens),
            min_lines: self.min_lines.unwrap_or(config.min_lines),
            min_occurrences: self
                .min_occurrences
                .filter(|min| *min >= 2)
                .unwrap_or(config.min_occurrences),
            threshold: self.threshold.unwrap_or(config.threshold),
            ignore: config.ignore.clone(),
            ignore_defaults: config.ignore_defaults,
            skip_local: self.skip_local.unwrap_or(config.skip_local),
            cross_language: self.cross_language.unwrap_or(config.cross_language),
            ignore_imports: self.ignore_imports.unwrap_or(config.ignore_imports),
            normalization: config.normalization.clone(),
            min_corpus_size_for_shingle_filter: config.min_corpus_size_for_shingle_filter,
            min_corpus_size_for_token_cache: config.min_corpus_size_for_token_cache,
        }
    }
}

fn initialization_duplication_options(opts: &serde_json::Value) -> Option<LspDuplicationOptions> {
    serde_json::from_value(opts.get("duplication")?.clone()).ok()
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
    /// Optional duplication overrides from `initializationOptions.duplication`.
    /// VS Code sends these so live diagnostics match the sidebar CLI run.
    duplication_options: Arc<RwLock<Option<LspDuplicationOptions>>>,
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
        let canonical_root = root.map(|path| path.canonicalize().unwrap_or(path));
        if let Some(path) = &canonical_root {
            *self.root.write().await = Some(path.clone());
        }

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
        {
            let now = Instant::now();
            let mut last = self.last_analysis.lock().await;
            if now.duration_since(*last) < std::time::Duration::from_millis(500) {
                return;
            }
            *last = now;
        }

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

        let documents = self.documents.read().await;
        let file_content = documents.get(uri).map_or_else(
            || std::fs::read_to_string(&file_path).unwrap_or_default(),
            |state| state.text.clone(),
        );
        drop(documents);
        let file_lines: Vec<&str> = file_content.lines().collect();

        actions.extend(code_actions::build_remove_export_actions(
            results,
            &file_path,
            uri,
            &params.range,
            &file_lines,
        ));

        actions.extend(code_actions::build_delete_file_actions(
            results,
            &file_path,
            uri,
            &params.range,
        ));

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
            duplication_options: Arc::new(RwLock::new(None)),
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
        if self.cancellation.load(Ordering::SeqCst) {
            return;
        }

        let root = self.root.read().await.clone();
        let Some(root) = root else { return };

        let Ok(_guard) = self.analysis_guard.try_lock() else {
            return; // analysis already running
        };

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

        let project_roots = find_project_roots(&root);

        self.client
            .log_message(MessageType::INFO, "Analyzing workspace root")
            .await;

        let changed_since = self.changed_since.read().await.clone();
        let changed_since_for_data = changed_since.clone();
        let config_path = self.config_path.read().await.clone();
        let duplication_options = self.duplication_options.read().await.clone();

        let resolved_toplevel = self.resolved_git_toplevel(&root).await;

        let blocking_root = root.clone();
        let blocking_toplevel = resolved_toplevel.clone();

        let join_result = tokio::task::spawn_blocking(move || {
            let mut merged_results = AnalysisResults::default();
            let mut merged_duplication = DuplicationReport::default();
            let mut config_messages: Vec<(MessageType, String)> =
                Vec::with_capacity(project_roots.len());
            for project_root in &project_roots {
                analyze_project_root(
                    project_root,
                    config_path.as_deref(),
                    duplication_options.as_ref(),
                    &mut merged_results,
                    &mut merged_duplication,
                    &mut config_messages,
                );
            }

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
                if self.cancellation.load(Ordering::SeqCst) {
                    return;
                }

                for (level, msg) in config_messages {
                    self.client.log_message(level, msg).await;
                }

                if let Some((level, msg)) = changed_message {
                    self.client.log_message(level, msg).await;
                }

                let mut all_diagnostics =
                    diagnostics::build_diagnostics(&results, &duplication, &root);
                attach_changed_since_data(&mut all_diagnostics, changed_since_for_data.as_deref());
                self.publish_collected_diagnostics(all_diagnostics, &version_snapshot)
                    .await;

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
                        re_export_cycles: results.re_export_cycles.len(),
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

        let live_versions: FxHashMap<Url, i32> = self
            .documents
            .read()
            .await
            .iter()
            .map(|(uri, state)| (uri.clone(), state.version))
            .collect();

        let mut new_uris: FxHashSet<Url> = FxHashSet::default();

        for (uri, diags) in &diagnostics_by_file {
            new_uris.insert(uri.clone());

            if Self::uri_is_stale(uri, snapshot, &live_versions) {
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

            self.client
                .publish_diagnostics(uri.clone(), filtered.clone(), snapshot.get(uri).copied())
                .await;

            self.cached_diagnostics
                .write()
                .await
                .insert(uri.clone(), filtered);
        }

        {
            let previous_uris = self.previous_diagnostic_uris.read().await;
            let mut cache = self.cached_diagnostics.write().await;
            for old_uri in previous_uris.iter() {
                if new_uris.contains(old_uri) {
                    continue;
                }
                if Self::uri_is_stale(old_uri, snapshot, &live_versions) {
                    new_uris.insert(old_uri.clone());
                    continue;
                }
                self.client
                    .publish_diagnostics(old_uri.clone(), vec![], snapshot.get(old_uri).copied())
                    .await;
                cache.remove(old_uri);
            }
        }

        *self.previous_diagnostic_uris.write().await = new_uris;
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
                Some(_) => {}
            }
        }
    }
}

/// Fold the analysis results from the single project root into the accumulator.
///
/// Thin wrapper over [`AnalysisResults::merge_into`], the single
/// field-exhaustive union (issue #444). The LSP analyzes one root per run
/// (see [`find_project_roots`]), so this folds exactly one result; the wrapper
/// stays because [`AnalysisResults::merge_into`] is the field-drift guard that
/// `merge_results_covers_all_fields` pins against new `AnalysisResults` fields.
fn merge_results(target: &mut AnalysisResults, source: AnalysisResults) {
    target.merge_into(source);
}

/// Fold the duplication report from the single project root into the
/// accumulator. The LSP analyzes one root per run (see [`find_project_roots`]),
/// so this folds exactly one report.
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

    #[test]
    fn initialization_duplication_options_reads_vscode_payload() {
        let opts = json!({
            "duplication": {
                "mode": "semantic",
                "threshold": 7.5,
                "minTokens": 64,
                "minLines": 8,
                "minOccurrences": 3,
                "skipLocal": true,
                "crossLanguage": true,
                "ignoreImports": true
            }
        });

        let parsed =
            initialization_duplication_options(&opts).expect("duplication options should parse");

        assert_eq!(parsed.mode, Some(DetectionMode::Semantic));
        assert_eq!(parsed.threshold, Some(7.5));
        assert_eq!(parsed.min_tokens, Some(64));
        assert_eq!(parsed.min_lines, Some(8));
        assert_eq!(parsed.min_occurrences, Some(3));
        assert_eq!(parsed.skip_local, Some(true));
        assert_eq!(parsed.cross_language, Some(true));
        assert_eq!(parsed.ignore_imports, Some(true));
    }

    #[test]
    fn lsp_duplication_options_override_project_config() {
        let project = DuplicatesConfig {
            mode: DetectionMode::Weak,
            min_tokens: 50,
            min_lines: 5,
            min_occurrences: 4,
            threshold: 2.0,
            skip_local: true,
            cross_language: false,
            ignore_imports: false,
            ignore: vec!["generated/**".to_string()],
            ..DuplicatesConfig::default()
        };
        let options = LspDuplicationOptions {
            mode: Some(DetectionMode::Semantic),
            threshold: Some(10.0),
            min_tokens: Some(80),
            min_lines: Some(9),
            min_occurrences: Some(3),
            skip_local: Some(false),
            cross_language: Some(true),
            ignore_imports: Some(true),
        };

        let merged = options.merge_with(&project);

        assert_eq!(merged.mode, DetectionMode::Semantic);
        assert!((merged.threshold - 10.0).abs() < f64::EPSILON);
        assert_eq!(merged.min_tokens, 80);
        assert_eq!(merged.min_lines, 9);
        assert_eq!(merged.min_occurrences, 3);
        assert!(!merged.skip_local);
        assert!(merged.cross_language);
        assert!(merged.ignore_imports);
        assert_eq!(merged.ignore, vec!["generated/**".to_string()]);
    }

    #[test]
    fn analyze_project_root_applies_lsp_duplication_options() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).expect("create src dir");
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"lsp-dupes-options","private":true,"main":"src/a.ts"}"#,
        )
        .expect("write package");
        std::fs::write(
            root.join(".fallowrc.jsonc"),
            r#"{"duplicates":{"minTokens":5,"minLines":1,"minOccurrences":2}}"#,
        )
        .expect("write config");
        let source = r"
            export const calculate = (input: number): number => {
                const doubled = input * 2;
                const incremented = doubled + 1;
                return incremented;
            };
        ";
        std::fs::write(root.join("src/a.ts"), source).expect("write a");
        std::fs::write(root.join("src/b.ts"), source).expect("write b");

        let mut baseline_results = AnalysisResults::default();
        let mut baseline_duplication = DuplicationReport::default();
        let mut baseline_messages = Vec::new();
        analyze_project_root(
            root,
            None,
            None,
            &mut baseline_results,
            &mut baseline_duplication,
            &mut baseline_messages,
        );

        assert!(
            baseline_duplication.stats.clone_groups > 0,
            "fixture should produce pair-only duplicate findings"
        );

        let mut filtered_results = AnalysisResults::default();
        let mut filtered_duplication = DuplicationReport::default();
        let mut filtered_messages = Vec::new();
        let options = LspDuplicationOptions {
            min_occurrences: Some(3),
            ..LspDuplicationOptions::default()
        };
        analyze_project_root(
            root,
            None,
            Some(&options),
            &mut filtered_results,
            &mut filtered_duplication,
            &mut filtered_messages,
        );

        assert_eq!(filtered_duplication.stats.clone_groups, 0);
    }

    #[test]
    fn find_project_roots_returns_only_workspace_root() {
        // A monorepo with two workspace packages must still yield exactly one
        // analysis root: the workspace root. The single root pass already walks
        // the whole tree and is workspace-aware, so the LSP no longer re-runs
        // the pipeline per sub-package (issue #971).
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::write(
            root.join("pnpm-workspace.yaml"),
            "packages:\n  - 'packages/*'\n",
        )
        .expect("write pnpm-workspace");
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"monorepo","private":true,"workspaces":["packages/*"]}"#,
        )
        .expect("write root package");
        for pkg in ["a", "b"] {
            let pkg_dir = root.join("packages").join(pkg);
            std::fs::create_dir_all(&pkg_dir).expect("create package dir");
            std::fs::write(
                pkg_dir.join("package.json"),
                format!(r#"{{"name":"@monorepo/{pkg}","main":"index.ts"}}"#),
            )
            .expect("write package");
        }

        // Sanity: the fixture really does have discoverable workspace packages,
        // so a single returned root proves the per-package loop is gone (not
        // that discovery found nothing).
        assert_eq!(
            fallow_config::discover_workspaces(root).len(),
            2,
            "fixture should expose two workspace packages"
        );

        let roots = find_project_roots(root);
        assert_eq!(roots.len(), 1, "LSP analyzes exactly one root per run");
        let expected = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        assert_eq!(roots[0], expected, "the single root is the workspace root");
    }

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

    #[expect(
        clippy::too_many_lines,
        reason = "intentionally names every AnalysisResults field (no ..Default::default()) so a new field is a compile error here; see #444"
    )]
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
                    edges: Vec::new(),
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
            private_type_leaks: vec![fallow_core::results::PrivateTypeLeakFinding::with_actions(
                fallow_core::results::PrivateTypeLeak {
                    path: "/f.ts".into(),
                    export_name: "pub_fn".to_string(),
                    type_name: "Secret".to_string(),
                    line: 14,
                    col: 0,
                    span_start: 0,
                },
            )],
            re_export_cycles: vec![fallow_core::results::ReExportCycleFinding::with_actions(
                fallow_core::results::ReExportCycle {
                    files: vec!["/barrel.ts".into()],
                    kind: fallow_core::results::ReExportCycleKind::SelfLoop,
                },
            )],
            stale_suppressions: vec![fallow_core::results::StaleSuppression {
                path: "/f.ts".into(),
                line: 15,
                col: 0,
                origin: fallow_core::results::SuppressionOrigin::Comment {
                    issue_kind: None,
                    is_file_level: false,
                    kind_known: true,
                },
            }],
            unused_catalog_entries: vec![
                fallow_core::results::UnusedCatalogEntryFinding::with_actions(
                    fallow_core::results::UnusedCatalogEntry {
                        entry_name: "react".to_string(),
                        catalog_name: "default".to_string(),
                        path: "/pnpm-workspace.yaml".into(),
                        line: 16,
                        hardcoded_consumers: vec![],
                    },
                ),
            ],
            empty_catalog_groups: vec![
                fallow_core::results::EmptyCatalogGroupFinding::with_actions(
                    fallow_core::results::EmptyCatalogGroup {
                        catalog_name: "ui".to_string(),
                        path: "/pnpm-workspace.yaml".into(),
                        line: 17,
                    },
                ),
            ],
            unresolved_catalog_references: vec![
                fallow_core::results::UnresolvedCatalogReferenceFinding::with_actions(
                    fallow_core::results::UnresolvedCatalogReference {
                        entry_name: "vue".to_string(),
                        catalog_name: "default".to_string(),
                        path: "/pkg.json".into(),
                        line: 18,
                        available_in_catalogs: vec![],
                    },
                ),
            ],
            unused_dependency_overrides: vec![
                fallow_core::results::UnusedDependencyOverrideFinding::with_actions(
                    fallow_core::results::UnusedDependencyOverride {
                        raw_key: "react".to_string(),
                        target_package: "react".to_string(),
                        parent_package: None,
                        version_constraint: None,
                        version_range: "18".to_string(),
                        source: fallow_core::results::DependencyOverrideSource::PnpmWorkspaceYaml,
                        path: "/pnpm-workspace.yaml".into(),
                        line: 19,
                        hint: None,
                    },
                ),
            ],
            misconfigured_dependency_overrides: vec![
                fallow_core::results::MisconfiguredDependencyOverrideFinding::with_actions(
                    fallow_core::results::MisconfiguredDependencyOverride {
                        raw_key: "bad>".to_string(),
                        target_package: None,
                        raw_value: String::new(),
                        reason: fallow_core::results::DependencyOverrideMisconfigReason::EmptyValue,
                        source: fallow_core::results::DependencyOverrideSource::PnpmPackageJson,
                        path: "/pkg.json".into(),
                        line: 20,
                    },
                ),
            ],
            suppression_count: 1,
            active_suppressions: Vec::new(),
            feature_flags: vec![fallow_core::results::FeatureFlag {
                path: "/f.ts".into(),
                flag_name: "ENABLE_X".to_string(),
                kind: fallow_core::results::FlagKind::EnvironmentVariable,
                confidence: fallow_core::results::FlagConfidence::High,
                line: 21,
                col: 0,
                guard_span_start: None,
                guard_span_end: None,
                sdk_name: None,
                guard_line_start: None,
                guard_line_end: None,
                guarded_dead_exports: vec![],
            }],
            entry_point_summary: Some(fallow_core::results::EntryPointSummary {
                total: 0,
                by_source: vec![],
            }),
            security_findings: vec![fallow_core::results::SecurityFinding {
                kind: fallow_core::results::SecurityFindingKind::ClientServerLeak,
                category: None,
                cwe: None,
                path: "/client.tsx".into(),
                line: 1,
                col: 0,
                evidence: "transitively reaches DATABASE_URL".to_string(),
                source_backed: false,
                trace: vec![],
                actions: vec![],
                reachability: None,
            }],
            security_unresolved_edge_files: 2,
            security_unresolved_callee_sites: 0,
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
        assert_eq!(target.private_type_leaks.len(), 1);
        assert_eq!(target.unused_dependencies.len(), 1);
        assert_eq!(target.unused_dev_dependencies.len(), 1);
        assert_eq!(target.unused_optional_dependencies.len(), 1);
        assert_eq!(target.unused_enum_members.len(), 1);
        assert_eq!(target.unused_class_members.len(), 1);
        assert_eq!(target.unresolved_imports.len(), 1);
        assert_eq!(target.unlisted_dependencies.len(), 1);
        assert_eq!(target.duplicate_exports.len(), 1);
        assert_eq!(target.type_only_dependencies.len(), 1);
        assert_eq!(target.test_only_dependencies.len(), 1);
        assert_eq!(target.circular_dependencies.len(), 1);
        assert_eq!(target.re_export_cycles.len(), 1);
        assert_eq!(target.boundary_violations.len(), 1);
        assert_eq!(target.stale_suppressions.len(), 1);
        assert_eq!(target.unused_catalog_entries.len(), 1);
        assert_eq!(target.empty_catalog_groups.len(), 1);
        assert_eq!(target.unresolved_catalog_references.len(), 1);
        assert_eq!(target.unused_dependency_overrides.len(), 1);
        assert_eq!(target.misconfigured_dependency_overrides.len(), 1);
        assert_eq!(target.export_usages.len(), 1);
        assert_eq!(target.feature_flags.len(), 1);
        assert_eq!(target.security_findings.len(), 1);
        assert_eq!(target.security_unresolved_edge_files, 2);
        assert_eq!(target.suppression_count, 1);
        assert!(target.entry_point_summary.is_some());
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

        assert_eq!(target.unused_files.len(), 1);
    }

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

        assert_eq!(target.clone_groups.len(), 1);
        assert_eq!(target.stats.total_files, 5);
        assert!((target.stats.duplication_percentage - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn issue_type_mapping_has_expected_entries() {
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
        for issue_type in DIAGNOSTIC_ISSUE_TYPES {
            let Some(config_key) = issue_type.config_key else {
                continue;
            };
            assert!(
                !issue_type.code.ends_with('s') || issue_type.code.ends_with("ss"),
                "Diagnostic code '{}' for config key '{config_key}' should be singular",
                issue_type.code
            );
        }
    }

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
        let (service, _) = LspService::build(FallowLspServer::new).finish();
        let backend = service.inner();

        let uri = Url::parse("file:///opened-mid-run.ts").unwrap();
        let snapshot: VersionSnapshot = FxHashMap::default();

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
        let (service, _) = LspService::build(FallowLspServer::new).finish();
        let backend = service.inner();

        let uri = Url::parse("file:///closed.ts").unwrap();
        install_document(backend, &uri, 1, "v1").await;
        let snapshot: VersionSnapshot = std::iter::once((uri.clone(), 1)).collect();

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
