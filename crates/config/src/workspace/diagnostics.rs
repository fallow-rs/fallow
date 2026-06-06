//! Workspace discovery diagnostics.
//!
//! Surfaces malformed `package.json`, unreachable glob matches, missing
//! tsconfig references, and undeclared workspaces as typed
//! [`WorkspaceDiagnostic`] values. Each diagnostic also emits a deduplicated
//! `tracing::warn!` so users running fallow with default tracing filters see
//! the cause of "fallow doesn't see my package."
//!
//! Repeated `GlobMatchedNoPackageJson` diagnostics are aggregated by glob
//! pattern at emission time so a wide glob matching hundreds of package-less
//! directories on a large monorepo collapses to one bounded summary line per
//! pattern instead of one line per directory (issue #637). The structured
//! `Vec<WorkspaceDiagnostic>` returned to callers stays full; only the stderr
//! surface is bounded.
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

/// Render `path` relative to `root` with forward slashes. Shared by
/// [`render_message`] and [`build_glob_group_message`] so the per-instance and
/// aggregated message surfaces format paths identically (the forward-slash
/// normalisation is load-bearing for cross-platform output stability).
fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

fn render_message(root: &Path, path: &Path, kind: &WorkspaceDiagnosticKind) -> String {
    let display = display_relative(root, path);
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

/// Maximum number of example directories named in an aggregated
/// `GlobMatchedNoPackageJson` warning before the tail is summarised as
/// "and N more". Keeps a fanned-out glob to one bounded stderr line.
const GLOB_EXAMPLE_CAP: usize = 3;

/// Process-wide set of already-emitted diagnostic dedupe keys. Per-instance
/// keys (`root::kind::path`) and aggregated per-pattern keys
/// (`root::glob-matched-no-package-json-agg::pattern`) share one set so
/// combined-mode (check + dupes + health through one loader) and watch-mode
/// reruns warn at most once per logical diagnostic. The two key namespaces are
/// disjoint, so there is no cross-talk.
fn warned_keys() -> &'static Mutex<FxHashSet<String>> {
    static WARNED: OnceLock<Mutex<FxHashSet<String>>> = OnceLock::new();
    WARNED.get_or_init(|| Mutex::new(FxHashSet::default()))
}

/// Insert `key` and return `true` when it was newly inserted (caller should
/// emit). On a poisoned mutex returns `true` so over-warning beats swallowing
/// a typo. Mirrors `parsing::warn_on_unknown_rule_keys` and
/// `plugins::registry::should_warn`.
fn should_emit(key: String) -> bool {
    warned_keys().lock().map_or(true, |mut set| set.insert(key))
}

/// A single planned stderr warning: its process-dedupe key and the rendered
/// message. The pure output of [`plan_warnings`] so the partition/aggregation
/// logic is unit-testable without a tracing subscriber or the process-wide
/// dedupe set.
#[derive(Debug, PartialEq, Eq)]
struct PlannedWarning {
    dedupe_key: String,
    message: String,
}

/// Turn a batch of workspace diagnostics into the bounded set of stderr
/// warnings to emit, collapsing the two kinds that fan out on large monorepos
/// (issue #637):
/// - `GlobMatchedNoPackageJson`: aggregated by glob pattern, one summary line
///   per pattern instead of one line per package-less directory.
/// - `TsconfigReferenceDirMissing`: aggregated together, one summary line
///   instead of one per missing `references[]` entry in the root tsconfig.
///
/// Pure: no tracing, no dedupe-set mutation. A group of exactly one keeps
/// today's per-instance message byte-for-byte (no regression for the common
/// single-match case); every other kind plans one per-instance warning. The
/// returned plan lists non-aggregated diagnostics first (in first-seen order),
/// then the glob-pattern summaries, then the tsconfig summary; ordering does
/// not affect correctness since these are independent stderr lines.
fn plan_warnings(root: &Path, diagnostics: &[WorkspaceDiagnostic]) -> Vec<PlannedWarning> {
    let canonical = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let per_instance = |diag: &WorkspaceDiagnostic| PlannedWarning {
        dedupe_key: format!(
            "{}::{}::{}",
            canonical.display(),
            diag.kind.id(),
            diag.path.display()
        ),
        message: diag.message.clone(),
    };

    let mut plans: Vec<PlannedWarning> = Vec::new();
    let mut glob_groups: Vec<(&str, Vec<&WorkspaceDiagnostic>)> = Vec::new();
    let mut tsconfig_ref_misses: Vec<&WorkspaceDiagnostic> = Vec::new();
    for diag in diagnostics {
        match &diag.kind {
            WorkspaceDiagnosticKind::GlobMatchedNoPackageJson { pattern } => {
                match glob_groups.iter_mut().find(|(p, _)| *p == pattern.as_str()) {
                    Some((_, group)) => group.push(diag),
                    None => glob_groups.push((pattern.as_str(), vec![diag])),
                }
            }
            WorkspaceDiagnosticKind::TsconfigReferenceDirMissing => tsconfig_ref_misses.push(diag),
            _ => plans.push(per_instance(diag)),
        }
    }

    for (pattern, group) in glob_groups {
        if let [only] = group.as_slice() {
            plans.push(per_instance(only));
            continue;
        }
        let paths: Vec<&Path> = group.iter().map(|d| d.path.as_path()).collect();
        plans.push(PlannedWarning {
            dedupe_key: format!(
                "{}::glob-matched-no-package-json-agg::{pattern}",
                canonical.display()
            ),
            message: build_glob_group_message(root, pattern, &paths),
        });
    }

    if let [only] = tsconfig_ref_misses.as_slice() {
        plans.push(per_instance(only));
    } else if !tsconfig_ref_misses.is_empty() {
        let paths: Vec<&Path> = tsconfig_ref_misses
            .iter()
            .map(|d| d.path.as_path())
            .collect();
        plans.push(PlannedWarning {
            dedupe_key: format!(
                "{}::tsconfig-reference-dir-missing-agg",
                canonical.display()
            ),
            message: build_tsconfig_refs_message(root, &paths),
        });
    }

    plans
}

/// Emit `tracing::warn!` lines for a batch of workspace diagnostics.
///
/// Delegates the partition/aggregation decisions to the pure [`plan_warnings`]
/// and applies the process-wide dedupe so combined-mode (check + dupes + health
/// through one loader) and watch-mode reruns warn at most once per logical
/// diagnostic. The returned/stashed `Vec<WorkspaceDiagnostic>` is unaffected;
/// only the stderr surface is bounded, so structured JSON consumers still see
/// every diagnostic.
pub(super) fn emit_diagnostics(root: &Path, diagnostics: &[WorkspaceDiagnostic]) {
    #[cfg(test)]
    for diag in diagnostics {
        capture_diag(diag);
    }

    for plan in plan_warnings(root, diagnostics) {
        if should_emit(plan.dedupe_key) {
            tracing::warn!("fallow: {}", plan.message);
        }
    }
}

/// Render up to [`GLOB_EXAMPLE_CAP`] project-relative example paths (sorted for
/// deterministic output) with an "and N more" tail when the count exceeds the
/// cap. Returns the joined example string and the total path count. Shared by
/// the aggregated-message builders.
fn summarize_examples(root: &Path, paths: &[&Path]) -> (String, usize) {
    let mut examples: Vec<String> = paths.iter().map(|p| display_relative(root, p)).collect();
    examples.sort();
    let count = examples.len();
    let shown = examples
        .iter()
        .take(GLOB_EXAMPLE_CAP)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let remaining = count.saturating_sub(GLOB_EXAMPLE_CAP);
    let listed = if remaining > 0 {
        format!("{shown}, and {remaining} more")
    } else {
        shown
    };
    (listed, count)
}

/// Build the aggregated message for a glob pattern that matched `paths`
/// package-less directories (always called with `paths.len() >= 2`).
fn build_glob_group_message(root: &Path, pattern: &str, paths: &[&Path]) -> String {
    let (listed, count) = summarize_examples(root, paths);
    format!(
        "Glob '{pattern}' matched {count} directories with no package.json \
         (e.g. {listed}). Add a package.json, narrow the pattern, or add \
         them to ignorePatterns."
    )
}

/// Build the aggregated message for `paths` `tsconfig.json` `references[]`
/// entries that point at missing directories (always called with
/// `paths.len() >= 2`).
fn build_tsconfig_refs_message(root: &Path, paths: &[&Path]) -> String {
    let (listed, count) = summarize_examples(root, paths);
    format!(
        "tsconfig.json references {count} directories that do not exist \
         (e.g. {listed}). Update or remove the references, or restore the \
         missing directories."
    )
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

/// Push `diag` into the thread-local capture buffer when one is installed.
/// No-op when no test has called [`capture_workspace_warnings`] on the current
/// thread, so production code never allocates. Called once per diagnostic by
/// [`emit_diagnostics`] before the dedupe gate, so every diagnostic is observed
/// regardless of whether it was emitted per-instance or aggregated.
#[cfg(test)]
fn capture_diag(diag: &WorkspaceDiagnostic) {
    WORKSPACE_DIAGNOSTIC_CAPTURE.with(|cell| {
        if let Some(buf) = cell.borrow_mut().as_mut() {
            buf.push(diag.clone());
        }
    });
}

/// Install a thread-local capture buffer and run `body`. Returns the body's
/// result alongside every diagnostic passed through [`emit_diagnostics`] on the
/// current thread, in order.
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
/// `fallow dead-code / dupes / health`) read via [`workspace_diagnostics_for`].
///
/// Canonicalisation matches the dedupe-key canonicalisation in
/// [`plan_warnings`]: two callers on the same physical root coalesce, and
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

#[cfg(test)]
mod tests {
    use super::*;

    fn glob_diag(root: &Path, pattern: &str, rel_path: &str) -> WorkspaceDiagnostic {
        WorkspaceDiagnostic::new(
            root,
            root.join(rel_path),
            WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
                pattern: pattern.to_owned(),
            },
        )
    }

    #[test]
    fn build_glob_group_message_caps_examples_and_summarises_tail() {
        let root = Path::new("/project");
        let paths = [
            root.join("playground/cli"),
            root.join("playground/lib-types"),
            root.join("playground/minify"),
            root.join("playground/ssr"),
            root.join("playground/worker"),
        ];
        let refs: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
        let message = build_glob_group_message(root, "playground/**", &refs);

        assert!(
            message.starts_with("Glob 'playground/**' matched 5 directories with no package.json"),
            "count and pattern lead the message: {message}"
        );
        assert!(
            message.contains(
                "(e.g. playground/cli, playground/lib-types, playground/minify, and 2 more)"
            ),
            "three sorted examples + tail count: {message}"
        );
        assert!(
            message.ends_with(
                "Add a package.json, narrow the pattern, or add them to ignorePatterns."
            ),
            "next-step hint preserved: {message}"
        );
        assert!(
            !message.contains("playground/ssr"),
            "tail example not named: {message}"
        );
    }

    #[test]
    fn build_glob_group_message_no_tail_when_at_or_below_cap() {
        let root = Path::new("/project");
        let paths = [root.join("packages/a"), root.join("packages/b")];
        let refs: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
        let message = build_glob_group_message(root, "packages/*", &refs);

        assert!(message.contains("matched 2 directories"), "{message}");
        assert!(
            message.contains("(e.g. packages/a, packages/b)"),
            "both examples named, no `and N more`: {message}"
        );
        assert!(!message.contains("more)"), "no tail clause: {message}");
    }

    #[test]
    fn plan_warnings_aggregates_repeated_glob_diagnostics_to_one_line() {
        let root = Path::new("/project");
        let diagnostics: Vec<WorkspaceDiagnostic> = (0..50)
            .map(|i| glob_diag(root, "playground/**", &format!("playground/p{i}")))
            .collect();

        let plans = plan_warnings(root, &diagnostics);

        assert_eq!(
            plans.len(),
            1,
            "50 same-pattern diagnostics collapse to one plan"
        );
        assert!(
            plans[0]
                .dedupe_key
                .ends_with("::glob-matched-no-package-json-agg::playground/**")
        );
        assert!(plans[0].message.contains("matched 50 directories"));
    }

    #[test]
    fn plan_warnings_keeps_distinct_patterns_separate() {
        let root = Path::new("/project");
        let diagnostics = vec![
            glob_diag(root, "apps/*", "apps/a"),
            glob_diag(root, "apps/*", "apps/b"),
            glob_diag(root, "packages/*", "packages/x"),
            glob_diag(root, "packages/*", "packages/y"),
        ];

        let plans = plan_warnings(root, &diagnostics);

        assert_eq!(plans.len(), 2, "one aggregated plan per distinct pattern");
        let messages: Vec<&str> = plans.iter().map(|p| p.message.as_str()).collect();
        assert!(
            messages
                .iter()
                .any(|m| m.contains("Glob 'apps/*' matched 2")),
            "{messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("Glob 'packages/*' matched 2")),
            "{messages:?}"
        );
    }

    #[test]
    fn plan_warnings_single_match_keeps_per_instance_message_and_key() {
        let root = Path::new("/project");
        let diag = glob_diag(root, "packages/*", "packages/scratch");

        let plans = plan_warnings(root, std::slice::from_ref(&diag));

        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].message, diag.message);
        assert!(
            plans[0]
                .dedupe_key
                .contains("::glob-matched-no-package-json::")
                && plans[0].dedupe_key.ends_with("packages/scratch"),
            "per-instance key is `root::kind::path`, not the `-agg::pattern` form: {}",
            plans[0].dedupe_key
        );
        assert!(
            !plans[0].message.contains("directories"),
            "single match is not aggregated"
        );
    }

    #[test]
    fn plan_warnings_non_glob_kinds_stay_per_instance() {
        let root = Path::new("/project");
        let diagnostics = vec![
            WorkspaceDiagnostic::new(
                root,
                root.join("packages/a"),
                WorkspaceDiagnosticKind::UndeclaredWorkspace,
            ),
            WorkspaceDiagnostic::new(
                root,
                root.join("packages/b"),
                WorkspaceDiagnosticKind::MalformedPackageJson {
                    error: "trailing comma".to_owned(),
                },
            ),
        ];

        let plans = plan_warnings(root, &diagnostics);

        assert_eq!(
            plans.len(),
            2,
            "each non-glob diagnostic plans its own warning"
        );
        assert!(
            plans
                .iter()
                .all(|p| !p.message.contains("directories with no package.json"))
        );
    }

    fn tsconfig_ref_diag(root: &Path, rel_path: &str) -> WorkspaceDiagnostic {
        WorkspaceDiagnostic::new(
            root,
            root.join(rel_path),
            WorkspaceDiagnosticKind::TsconfigReferenceDirMissing,
        )
    }

    #[test]
    fn plan_warnings_aggregates_repeated_tsconfig_ref_misses_to_one_line() {
        let root = Path::new("/project");
        let diagnostics: Vec<WorkspaceDiagnostic> = (0..30)
            .map(|i| tsconfig_ref_diag(root, &format!("packages/p{i:02}/tsconfig.json")))
            .collect();

        let plans = plan_warnings(root, &diagnostics);

        assert_eq!(plans.len(), 1, "30 missing references collapse to one plan");
        assert!(
            plans[0]
                .dedupe_key
                .ends_with("::tsconfig-reference-dir-missing-agg")
        );
        assert!(
            plans[0]
                .message
                .starts_with("tsconfig.json references 30 directories that do not exist"),
            "{}",
            plans[0].message
        );
        assert!(
            plans[0].message.contains(
                "(e.g. packages/p00/tsconfig.json, packages/p01/tsconfig.json, \
                 packages/p02/tsconfig.json, and 27 more)"
            ),
            "three sorted examples + tail: {}",
            plans[0].message
        );
        assert!(
            plans[0]
                .message
                .ends_with("Update or remove the references, or restore the missing directories."),
            "{}",
            plans[0].message
        );
    }

    #[test]
    fn plan_warnings_single_tsconfig_ref_miss_keeps_per_instance_message() {
        let root = Path::new("/project");
        let diag = tsconfig_ref_diag(root, "packages/only/tsconfig.json");

        let plans = plan_warnings(root, std::slice::from_ref(&diag));

        assert_eq!(plans.len(), 1);
        assert_eq!(
            plans[0].message, diag.message,
            "single miss is not aggregated"
        );
        assert!(!plans[0].message.contains("directories that do not exist"));
    }

    #[test]
    fn plan_warnings_mixed_aggregatable_kinds_each_collapse_independently() {
        let root = Path::new("/project");
        let mut diagnostics: Vec<WorkspaceDiagnostic> = (0..5)
            .map(|i| glob_diag(root, "packages/*", &format!("packages/g{i}")))
            .collect();
        diagnostics.extend(
            (0..4).map(|i| tsconfig_ref_diag(root, &format!("packages/t{i}/tsconfig.json"))),
        );

        let plans = plan_warnings(root, &diagnostics);

        assert_eq!(plans.len(), 2, "one glob summary + one tsconfig summary");
        assert!(
            plans
                .iter()
                .any(|p| p.message.contains("matched 5 directories"))
        );
        assert!(
            plans
                .iter()
                .any(|p| p.message.contains("references 4 directories"))
        );
    }
}
