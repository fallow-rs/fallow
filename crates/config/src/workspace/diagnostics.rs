//! Workspace discovery diagnostics.
//!
//! Surfaces malformed `package.json`, unreachable glob matches, missing
//! tsconfig references, and undeclared workspaces as typed
//! [`WorkspaceDiagnostic`] values. Each diagnostic also emits a deduplicated
//! `tracing::warn!` so users running fallow with default tracing filters see
//! the cause of "fallow doesn't see my package."
//!
//! Mirrors the dedupe + capture pattern in
//! `crates/config/src/config/parsing.rs::warn_on_unknown_rule_keys` (issue
//! #467).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use rustc_hash::{FxHashMap, FxHashSet};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Why a workspace-discovery candidate was rejected, or why a sibling
/// directory looked workspace-like but was not declared.
///
/// Wire-format names are kebab-case so JSON consumers (CI integrations, MCP
/// agents, LSP clients) get a stable, language-neutral identifier.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum WorkspaceDiagnosticKind {
    /// A directory contains `package.json` but is not declared as a workspace
    /// in `package.json` `workspaces`, `pnpm-workspace.yaml`, or
    /// `tsconfig.json` `references`. Surfaced by
    /// `find_undeclared_workspaces`.
    UndeclaredWorkspace,
    /// A declared workspace's `package.json` failed to parse. The directory is
    /// dropped from discovery, but analysis still proceeds (degraded).
    MalformedPackageJson {
        /// `serde_json` parse error text.
        error: String,
    },
    /// A workspace glob pattern matched a directory that contains no
    /// `package.json`. Honors the extended skip list and `ignorePatterns`
    /// before emitting.
    GlobMatchedNoPackageJson {
        /// The glob pattern that matched the directory.
        pattern: String,
    },
    /// `tsconfig.json` exists at the root but failed to parse. Project
    /// references cannot be discovered.
    MalformedTsconfig {
        /// JSONC parse error text.
        error: String,
    },
    /// `tsconfig.json` lists a `references[].path` that does not point to an
    /// existing directory.
    TsconfigReferenceDirMissing,
}

impl WorkspaceDiagnosticKind {
    /// Stable kebab-case identifier used in dedupe keys and tracing payloads.
    #[must_use]
    pub const fn id(&self) -> &'static str {
        match self {
            Self::UndeclaredWorkspace => "undeclared-workspace",
            Self::MalformedPackageJson { .. } => "malformed-package-json",
            Self::GlobMatchedNoPackageJson { .. } => "glob-matched-no-package-json",
            Self::MalformedTsconfig { .. } => "malformed-tsconfig",
            Self::TsconfigReferenceDirMissing => "tsconfig-reference-dir-missing",
        }
    }
}

/// A diagnostic about a workspace-discovery candidate.
///
/// The `message` field is a human-readable rendering derived from `kind`. It
/// always ends with a concrete next step ("fix the JSON syntax", "remove from
/// `workspaces`", "add to `ignorePatterns`") so first-time users have a path
/// forward.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceDiagnostic {
    /// Path to the directory or file that triggered the diagnostic.
    pub path: PathBuf,
    /// Kind discriminator with the typed payload.
    #[serde(flatten)]
    pub kind: WorkspaceDiagnosticKind,
    /// Human-readable rendering derived from `kind` + `path`. Always ends
    /// with a next-step hint.
    pub message: String,
}

impl WorkspaceDiagnostic {
    /// Construct a diagnostic with the message rendered from `kind` + `path`.
    ///
    /// `root` is used to produce project-relative paths in the message text
    /// AND inside the variant payload (e.g. the `error` field of
    /// `MalformedPackageJson` / `MalformedTsconfig` which embed the absolute
    /// file path from `PackageJson::load()`'s error text). Without the
    /// payload-side normalisation the embedded path would survive
    /// environment-specific differences (CI vs Docker vs local) because the
    /// post-serialisation `strip_root_prefix` only catches whole-string
    /// matches, not paths embedded mid-sentence.
    ///
    /// If `path` is not under `root` (e.g. canonicalisation crossed a
    /// symlink), the absolute path is emitted instead.
    #[must_use]
    pub fn new(root: &Path, path: PathBuf, kind: WorkspaceDiagnosticKind) -> Self {
        let kind = normalise_payload_paths(root, kind);
        let message = render_message(root, &path, &kind);
        Self {
            path,
            kind,
            message,
        }
    }
}

/// Strip the project root from absolute paths embedded inside variant
/// payloads (today: the `error` field of `MalformedPackageJson` and
/// `MalformedTsconfig`). Mirrors the per-platform `display()` byte sequence
/// so the substring match works on Windows too.
fn normalise_payload_paths(root: &Path, kind: WorkspaceDiagnosticKind) -> WorkspaceDiagnosticKind {
    let root_str = root.display().to_string();
    let root_alt = root_str.replace('\\', "/");
    let normalise = |text: String| -> String {
        let stripped = text
            .replace(&format!("{root_str}/"), "")
            .replace(&format!("{root_alt}/"), "");
        // Also strip a stray Windows-style trailing-separator form just in case
        // the diagnostic was constructed with a path whose `display()` keeps
        // backslashes.
        stripped
            .replace(&format!("{root_str}\\"), "")
            .replace(&format!("{root_alt}\\"), "")
    };
    match kind {
        WorkspaceDiagnosticKind::MalformedPackageJson { error } => {
            WorkspaceDiagnosticKind::MalformedPackageJson {
                error: normalise(error),
            }
        }
        WorkspaceDiagnosticKind::MalformedTsconfig { error } => {
            WorkspaceDiagnosticKind::MalformedTsconfig {
                error: normalise(error),
            }
        }
        other => other,
    }
}

fn render_message(root: &Path, path: &Path, kind: &WorkspaceDiagnosticKind) -> String {
    let display = path
        .strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/");
    match kind {
        WorkspaceDiagnosticKind::UndeclaredWorkspace => format!(
            "Directory '{display}' contains package.json but is not declared as a workspace. \
             Add it to package.json workspaces or pnpm-workspace.yaml, or add it to ignorePatterns."
        ),
        WorkspaceDiagnosticKind::MalformedPackageJson { error } => format!(
            "Dropped workspace '{display}': package.json is not valid JSON ({error}). \
             Fix the JSON syntax or remove '{display}' from the workspaces pattern."
        ),
        WorkspaceDiagnosticKind::GlobMatchedNoPackageJson { pattern } => format!(
            "Glob '{pattern}' matched '{display}' but no package.json is present. \
             Add a package.json, narrow the pattern, or add '{display}' to ignorePatterns."
        ),
        WorkspaceDiagnosticKind::MalformedTsconfig { error } => format!(
            "tsconfig.json at '{display}' failed to parse ({error}); \
             project references will be ignored. Fix the JSON syntax."
        ),
        WorkspaceDiagnosticKind::TsconfigReferenceDirMissing => format!(
            "tsconfig.json references '{display}' but the directory does not exist. \
             Update or remove the reference, or restore the missing directory."
        ),
    }
}

/// Workspace-discovery failures that prevent analysis from proceeding.
///
/// Returned only by `discover_workspaces_with_diagnostics` (in the parent
/// module) when the root `package.json` itself is malformed: without a
/// parseable root, no workspace patterns can be collected, and analysis
/// output would be fiction. The CLI surfaces this as exit 2.
#[derive(Debug, Clone)]
pub enum WorkspaceLoadError {
    /// The project root's `package.json` exists but failed to parse.
    MalformedRootPackageJson { path: PathBuf, error: String },
}

impl std::fmt::Display for WorkspaceLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MalformedRootPackageJson { path, error } => write!(
                f,
                "root package.json at '{}' is not valid JSON ({error}). \
                 Fix the syntax before re-running fallow.",
                path.display()
            ),
        }
    }
}

impl std::error::Error for WorkspaceLoadError {}

/// Emit a `tracing::warn!` for a workspace diagnostic, dedupe-keyed on the
/// canonical workspace root, the diagnostic's kind identifier, and the
/// offending path.
///
/// `root` is canonicalised before hashing so watch-mode reruns and parallel
/// agents on the same root coalesce. Two distinct roots produce independent
/// keys, which is what nested-monorepo callers want.
pub(super) fn emit_warn(root: &Path, diag: &WorkspaceDiagnostic) {
    static WARNED: OnceLock<Mutex<FxHashSet<String>>> = OnceLock::new();
    let warned = WARNED.get_or_init(|| Mutex::new(FxHashSet::default()));

    let canonical = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let dedupe_key = format!(
        "{}::{}::{}",
        canonical.display(),
        diag.kind.id(),
        diag.path.display()
    );

    // Push into the test-only capture FIRST, before the dedupe gate. The
    // capture buffer is meant for assertion-friendly tests; two calls on the
    // same (root, kind, path) inside one test should both observe the
    // emission. The process-wide dedupe still suppresses repeated
    // `tracing::warn!` calls, which is the surface that matters for real
    // users (watch-mode reruns, combined-mode running check + dupes + health
    // through the same loader).
    #[cfg(test)]
    WORKSPACE_DIAGNOSTIC_CAPTURE.with(|cell| {
        if let Some(buf) = cell.borrow_mut().as_mut() {
            buf.push(diag.clone());
        }
    });

    // On a poisoned mutex, fall through and emit anyway: over-warning beats
    // swallowing a typo. Matches the parsing.rs::warn_on_unknown_rule_keys
    // pattern.
    if let Ok(mut set) = warned.lock()
        && !set.insert(dedupe_key)
    {
        return;
    }

    tracing::warn!("fallow: {}", diag.message);
}

thread_local! {
    /// Per-thread capture of workspace diagnostics, for tests that assert
    /// emission without inspecting tracing output. Parallel test execution
    /// stays race-free because the buffer is thread-local; production code
    /// keeps the cell empty so emission goes only to tracing.
    ///
    /// Mirrors `parsing::UNKNOWN_RULE_CAPTURE` (issue #467).
    #[cfg(test)]
    static WORKSPACE_DIAGNOSTIC_CAPTURE: std::cell::RefCell<Option<Vec<WorkspaceDiagnostic>>> =
        const { std::cell::RefCell::new(None) };
}

/// Install a thread-local capture buffer and run `body`. Returns the body's
/// result alongside every diagnostic emitted by [`emit_warn`] on the current
/// thread, in order.
///
/// Test-only. Diagnostics captured here also bypass the process-wide dedupe
/// (so two captures on the same root + kind + path inside one test both
/// observe the emission).
#[cfg(test)]
#[must_use]
pub fn capture_workspace_warnings<F: FnOnce() -> R, R>(body: F) -> (R, Vec<WorkspaceDiagnostic>) {
    WORKSPACE_DIAGNOSTIC_CAPTURE.with(|cell| {
        *cell.borrow_mut() = Some(Vec::new());
    });
    let result = body();
    let findings =
        WORKSPACE_DIAGNOSTIC_CAPTURE.with(|cell| cell.borrow_mut().take().unwrap_or_default());
    (result, findings)
}

/// Process-wide registry of workspace-discovery diagnostics, keyed by
/// canonical root. Populated by callers that run
/// [`super::discover_workspaces_with_diagnostics`] and (after config load
/// completes) by the analysis pipeline's `find_undeclared_workspaces_*`
/// pass. Consumers (`fallow list --workspaces`, the JSON envelope on
/// `fallow check / dupes / health`) read via [`workspace_diagnostics_for`].
///
/// Canonicalisation matches the dedupe-key canonicalisation in
/// [`emit_warn`]: two callers on the same physical root coalesce, and
/// nested-monorepo callers on different roots stay independent.
static WORKSPACE_DIAGNOSTICS: OnceLock<Mutex<FxHashMap<PathBuf, Vec<WorkspaceDiagnostic>>>> =
    OnceLock::new();

/// Replace the workspace-discovery diagnostics for `root` with `diagnostics`.
///
/// Called at config-load time after [`super::discover_workspaces_with_diagnostics`]
/// completes; the analyze pipeline then APPENDS undeclared-workspace
/// diagnostics via [`append_workspace_diagnostics`].
pub fn stash_workspace_diagnostics(root: &Path, diagnostics: Vec<WorkspaceDiagnostic>) {
    let canonical = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let registry = WORKSPACE_DIAGNOSTICS.get_or_init(|| Mutex::new(FxHashMap::default()));
    if let Ok(mut map) = registry.lock() {
        map.insert(canonical, diagnostics);
    }
}

/// Append `additions` to the workspace-discovery diagnostics for `root`,
/// skipping any entry whose `(kind id, canonical path)` is already present.
///
/// Used by the analyze pipeline's undeclared-workspace pass to fold its
/// findings into the registry without re-emitting diagnostics that the
/// config-load pass already surfaced (e.g. a directory whose `package.json`
/// is malformed should NOT also produce a separate "undeclared" diagnostic
/// alongside the malformed-package-json one).
pub fn append_workspace_diagnostics(root: &Path, additions: Vec<WorkspaceDiagnostic>) {
    if additions.is_empty() {
        return;
    }
    let canonical = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let registry = WORKSPACE_DIAGNOSTICS.get_or_init(|| Mutex::new(FxHashMap::default()));
    if let Ok(mut map) = registry.lock() {
        let existing = map.entry(canonical).or_default();
        let mut seen: FxHashSet<(String, String)> = existing
            .iter()
            .map(|d| {
                (
                    d.kind.id().to_owned(),
                    dunce::canonicalize(&d.path)
                        .unwrap_or_else(|_| d.path.clone())
                        .display()
                        .to_string(),
                )
            })
            .collect();
        for addition in additions {
            let key = (
                addition.kind.id().to_owned(),
                dunce::canonicalize(&addition.path)
                    .unwrap_or_else(|_| addition.path.clone())
                    .display()
                    .to_string(),
            );
            if seen.insert(key) {
                existing.push(addition);
            }
        }
    }
}

/// Read the workspace-discovery diagnostics produced by the most recent
/// `stash_workspace_diagnostics` + any subsequent
/// `append_workspace_diagnostics` calls for `root`. Returns an empty vector
/// when nothing has been stashed for this root yet (e.g. programmatic
/// callers bypassing the standard loader).
#[must_use]
pub fn workspace_diagnostics_for(root: &Path) -> Vec<WorkspaceDiagnostic> {
    let canonical = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let Some(registry) = WORKSPACE_DIAGNOSTICS.get() else {
        return Vec::new();
    };
    registry
        .lock()
        .ok()
        .and_then(|map| map.get(&canonical).cloned())
        .unwrap_or_default()
}

/// Directories that are conventionally NOT workspace packages even when a
/// glob like `packages/*` matches them. Mirrors pnpm/npm/yarn behavior of
/// silently filtering these out, and extends fallow's existing
/// `should_skip_workspace_scan_dir` list with build artifacts and tooling
/// caches.
#[must_use]
pub(super) fn is_skip_listed_dir(name: &str) -> bool {
    // Dot-prefixed names (`.next`, `.turbo`, `.nuxt`, `.svelte-kit`, `.cache`)
    // are caught by the `starts_with('.')` arm; do not duplicate them in the
    // explicit list. The explicit list is reserved for non-dot conventional
    // build / output / tooling directories that pnpm/npm/yarn also filter.
    name.starts_with('.') || matches!(name, "node_modules" | "build" | "dist" | "coverage")
}

/// Test if a project-root-relative directory path is excluded by user
/// `ignorePatterns`. The directory itself and its `package.json` are both
/// checked because users variably write `packages/legacy/**` or
/// `packages/legacy/package.json` in their ignore globs.
#[must_use]
pub(super) fn is_ignored_workspace_dir(
    relative_dir: &Path,
    ignore_patterns: &globset::GlobSet,
) -> bool {
    if ignore_patterns.is_empty() {
        return false;
    }
    let relative_str = relative_dir.to_string_lossy().replace('\\', "/");
    ignore_patterns.is_match(relative_str.as_str())
        || ignore_patterns.is_match(format!("{relative_str}/package.json").as_str())
}
