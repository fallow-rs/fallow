//! Workspace and source-discovery diagnostics.
//!
//! Surfaces malformed `package.json`, unreachable glob matches, missing
//! tsconfig references, undeclared workspaces, and source files skipped during
//! source discovery as typed [`WorkspaceDiagnostic`] values. Each diagnostic
//! also emits a deduplicated `tracing::warn!` so users running fallow with
//! default tracing filters see the cause of "fallow doesn't see my package" or
//! "fallow ate all my memory."
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

pub use fallow_types::workspace::{WorkspaceDiagnostic, WorkspaceDiagnosticKind};

/// Render `path` relative to `root` with forward slashes. Mirrors the private
/// helper of the same name in `fallow_types::workspace`, kept here for the
/// aggregated stderr-message builders ([`build_glob_group_message`] and
/// [`build_tsconfig_refs_message`]) so the per-instance and aggregated message
/// surfaces format paths identically (the forward-slash normalisation is
/// load-bearing for cross-platform output stability).
fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

/// Workspace-discovery failures that prevent analysis from proceeding.
///
/// Returned only by `discover_workspaces_with_diagnostics` (in the parent
/// module) when a root package manifest itself is malformed: without a
/// parseable root, no workspace patterns can be collected, and analysis output
/// would be fiction. The CLI surfaces this as exit 2.
#[derive(Debug, Clone)]
pub enum WorkspaceLoadError {
    /// The project root's `package.json` exists but failed to parse.
    MalformedRootPackageJson {
        /// Path to the malformed manifest, shown in the diagnostic.
        path: PathBuf,
        /// Parser error message, embedded in the diagnostic.
        error: String,
    },
    /// The project root's `deno.json` or `deno.jsonc` exists but failed to parse.
    MalformedRootDenoConfig {
        /// Path to the malformed manifest, shown in the diagnostic.
        path: PathBuf,
        /// Parser error message, embedded in the diagnostic.
        error: String,
    },
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
            Self::MalformedRootDenoConfig { path, error } => write!(
                f,
                "root Deno config at '{}' is not valid JSONC ({error}). \
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

struct WarningGroups<'a> {
    plans: Vec<PlannedWarning>,
    glob_groups: Vec<(&'a str, Vec<&'a WorkspaceDiagnostic>)>,
    tsconfig_ref_misses: Vec<&'a WorkspaceDiagnostic>,
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
    let WarningGroups {
        mut plans,
        glob_groups,
        tsconfig_ref_misses,
    } = group_warning_diagnostics(diagnostics, &canonical);

    for (pattern, group) in glob_groups {
        if let [only] = group.as_slice() {
            plans.push(per_instance_warning(&canonical, only));
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
        plans.push(per_instance_warning(&canonical, only));
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

fn group_warning_diagnostics<'a>(
    diagnostics: &'a [WorkspaceDiagnostic],
    canonical: &Path,
) -> WarningGroups<'a> {
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
            _ => plans.push(per_instance_warning(canonical, diag)),
        }
    }
    WarningGroups {
        plans,
        glob_groups,
        tsconfig_ref_misses,
    }
}

fn per_instance_warning(canonical: &Path, diag: &WorkspaceDiagnostic) -> PlannedWarning {
    PlannedWarning {
        dedupe_key: format!(
            "{}::{}::{}",
            canonical.display(),
            diag.kind.id(),
            diag.path.display()
        ),
        message: diag.message.clone(),
    }
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

/// Replace the workspace-discovery diagnostics for `root` with `diagnostics`,
/// PRESERVING any source-discovery diagnostics (see
/// [`WorkspaceDiagnosticKind::is_source_discovery`]) and analysis-stage
/// diagnostics (see [`WorkspaceDiagnosticKind::is_analysis_stage`]) already
/// appended for the root.
///
/// Called at config-load time after [`super::discover_workspaces_with_diagnostics`]
/// completes; the analyze pipeline then APPENDS undeclared-workspace and
/// source-discovery (`skipped-large-file`, `skipped-source-dotdir`, and the
/// other kinds [`WorkspaceDiagnosticKind::is_source_discovery`] covers)
/// diagnostics via
/// [`append_workspace_diagnostics`]. The workspace-discovery set is authoritative
/// and replaced wholesale (so a fixed `package.json` clears its stale diagnostic
/// across watch-mode reruns), but source-discovery diagnostics are appended
/// AFTER this stash, so combined-mode's per-analysis config re-loads would
/// otherwise wipe a `skipped-large-file` entry that the first analysis's
/// discovery already recorded (issue #1086). Analysis-stage diagnostics
/// (`malformed-pnpm-workspace-yaml`, `bun-lockb-override-resolution-skipped`)
/// are recorded by the analyze pass through [`record_workspace_diagnostics`],
/// also after this stash, and are preserved for the same reason; each analyze
/// pass refreshes them through [`clear_analysis_stage_diagnostics`] (issue
/// #2366).
///
/// The stored set is deduplicated on the whole `(kind, path)` the way every
/// fold is: a repository that declares one glob in both `package.json` and
/// `pnpm-workspace.yaml` produces the same diagnostic twice at config load, and
/// the standalone envelopes read this registry verbatim (issue #2366).
pub fn stash_workspace_diagnostics(root: &Path, diagnostics: Vec<WorkspaceDiagnostic>) {
    let canonical = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let registry = WORKSPACE_DIAGNOSTICS.get_or_init(|| Mutex::new(FxHashMap::default()));
    if let Ok(mut map) = registry.lock() {
        let preserved = map.get(&canonical).map_or_else(Vec::new, |existing| {
            existing
                .iter()
                .filter(|d| d.kind.is_source_discovery() || d.kind.is_analysis_stage())
                .cloned()
                .collect()
        });
        map.insert(
            canonical,
            fallow_types::workspace::merge_workspace_diagnostics(diagnostics, preserved),
        );
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

/// Append `diagnostics` to the registry for `root` AND emit their deduplicated
/// stderr warnings, for analysis-stage callers outside this crate (e.g. the
/// pnpm catalog/override gathers in `fallow-core`) that surface a diagnostic
/// after config load completed. [`append_workspace_diagnostics`] alone would
/// reach `workspace_diagnostics[]` JSON but never warn a human on stderr.
pub fn record_workspace_diagnostics(root: &Path, diagnostics: Vec<WorkspaceDiagnostic>) {
    if diagnostics.is_empty() {
        return;
    }
    emit_diagnostics(root, &diagnostics);
    append_workspace_diagnostics(root, diagnostics);
}

/// Replace source-read-failure diagnostics for `root` with the failures from
/// the current parse while preserving every workspace and discovery diagnostic
/// produced by other stages.
///
/// Returns the structured diagnostics so session-owned outputs can carry the
/// exact same values as the process registry used by direct core and CLI paths.
#[must_use]
pub fn record_source_read_failures(
    root: &Path,
    failures: &[fallow_types::extract::SourceReadFailure],
) -> Vec<WorkspaceDiagnostic> {
    let diagnostics: Vec<WorkspaceDiagnostic> = failures
        .iter()
        .map(|failure| {
            WorkspaceDiagnostic::new(
                root,
                failure.path.clone(),
                WorkspaceDiagnosticKind::SourceReadFailure {
                    error: failure.error.clone(),
                },
            )
        })
        .collect();
    let canonical = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let registry = WORKSPACE_DIAGNOSTICS.get_or_init(|| Mutex::new(FxHashMap::default()));
    if let Ok(mut map) = registry.lock() {
        let existing = map.entry(canonical).or_default();
        existing.retain(|diagnostic| {
            !matches!(
                diagnostic.kind,
                WorkspaceDiagnosticKind::SourceReadFailure { .. }
            )
        });
        existing.extend(diagnostics.iter().cloned());
    }
    emit_diagnostics(root, &diagnostics);
    diagnostics
}

/// Replace every source-discovery diagnostic for `root` with `diagnostics` in
/// ONE registry operation, and hand the same list back to the caller.
///
/// Called at the END of each source walk (`discover_files`) so a stale
/// `skipped-large-file` entry from a previous analysis pass (a watch-mode
/// rerun after the user raised `--max-file-size` or added the file to
/// `ignorePatterns`) is dropped while the current walk's skips are written.
/// Pairs with the preserve in [`stash_workspace_diagnostics`]: this call keeps
/// the set CURRENT across reruns, the preserve keeps it ALIVE across
/// combined-mode's per-analysis config re-loads (issue #1086).
///
/// The clear-then-append pair this replaces was two separate lock
/// acquisitions, so a second source walk running concurrently on the same root
/// (combined mode runs the dead-code and duplication walks under `rayon::join`
/// whenever a per-analysis `production` split stops them from sharing a file
/// list) could interleave its clear between this walk's clear and its appends,
/// or between the appends and the walk's own read-back. Holding the lock across
/// the whole replacement makes the registry state a clean last-writer-wins, and
/// returning the list lets each analysis carry exactly what ITS walk skipped
/// without reading the shared registry back at all (issue #2366).
///
/// The retain also drops the parse stage's `source-read-failure` entries,
/// because [`WorkspaceDiagnosticKind::is_source_discovery`] covers that kind
/// too, so a concurrent walk on the same root can clear a read failure another
/// analysis's parse recorded. That window closes on its own:
/// [`record_source_read_failures`] replaces the read-failure set from each
/// analysis's own parse, and a fold's closing registry leg reads after both
/// walks have finished. Narrowing this retain to
/// [`WorkspaceDiagnosticKind::is_source_walk_recorded`] would leave the
/// read-failure set to its own recorder entirely.
#[must_use]
pub fn replace_source_discovery_diagnostics(
    root: &Path,
    diagnostics: Vec<WorkspaceDiagnostic>,
) -> Vec<WorkspaceDiagnostic> {
    let canonical = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let registry = WORKSPACE_DIAGNOSTICS.get_or_init(|| Mutex::new(FxHashMap::default()));
    if let Ok(mut map) = registry.lock() {
        let existing = map.entry(canonical).or_default();
        existing.retain(|d| !d.kind.is_source_discovery());
        existing.extend(diagnostics.iter().cloned());
    }
    diagnostics
}

/// Remove all analysis-stage diagnostics (see
/// [`WorkspaceDiagnosticKind::is_analysis_stage`]) for `root` from the
/// registry, keeping every workspace-discovery and source-discovery entry.
///
/// Called at the START of each dead-code analyze pass so a stale
/// `malformed-pnpm-workspace-yaml` or `bun-lockb-override-resolution-skipped`
/// entry from a previous pass (a watch-mode rerun or a long-lived engine
/// session after the YAML was fixed or a text `bun.lock` was written) is
/// dropped before the detectors re-record only what still applies. Mirrors
/// [`replace_source_discovery_diagnostics`] and pairs with the preserve in
/// [`stash_workspace_diagnostics`] (issue #2366).
pub fn clear_analysis_stage_diagnostics(root: &Path) {
    let canonical = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let Some(registry) = WORKSPACE_DIAGNOSTICS.get() else {
        return;
    };
    if let Ok(mut map) = registry.lock()
        && let Some(existing) = map.get_mut(&canonical)
    {
        existing.retain(|d| !d.kind.is_analysis_stage());
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

/// Read the registry leg of a diagnostics FOLD: everything
/// [`workspace_diagnostics_for`] holds for `root` EXCEPT the entries a source
/// walk records (see
/// [`WorkspaceDiagnosticKind::is_source_walk_recorded`]).
///
/// A fold combines an analysis's own captured list with the registry. The
/// analysis already carries its own walk's skips by value, and each walk
/// replaces the registry's source-discovery set for the root, so an unfiltered
/// registry read imports ANOTHER walk's file set: under a per-analysis
/// `production` split the dead-code and duplication walks see different files,
/// and the read answers whichever walk wrote last. That made the audit family
/// report a skip its dead-code analysis never saw, disagreeing with the MCP
/// `audit` tool, and made the order of the combined root's union depend on
/// which parallel walk won the race (issue #2366).
///
/// `source-read-failure` is deliberately still read: the parse stage records
/// it after the walk, so the registry is the only place it exists. Every
/// non-walk kind (workspace discovery, analysis stage) is likewise still read,
/// which is what lets `--skip check` and `--only health` report what their
/// analyses recorded after the section captured its list.
#[must_use]
pub fn registry_diagnostics_to_fold(root: &Path) -> Vec<WorkspaceDiagnostic> {
    workspace_diagnostics_for(root)
        .into_iter()
        .filter(|diagnostic| !diagnostic.kind.is_source_walk_recorded())
        .collect()
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
    use fallow_types::discover::FileId;
    use fallow_types::extract::SourceReadFailure;

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
    fn skipped_large_file_diagnostic_id_and_message() {
        let root = Path::new("/project");
        let diag = WorkspaceDiagnostic::new(
            root,
            root.join("src/vendor/app.bundle.js"),
            WorkspaceDiagnosticKind::SkippedLargeFile {
                size_bytes: 6 * 1024 * 1024,
            },
        );
        assert_eq!(diag.kind.id(), "skipped-large-file");
        assert!(
            diag.message.contains("src/vendor/app.bundle.js"),
            "message names the project-relative path: {}",
            diag.message
        );
        assert!(
            diag.message.contains("6.0 MB"),
            "message reports the size: {}",
            diag.message
        );
        assert!(
            diag.message.contains("--max-file-size"),
            "message names the override flag: {}",
            diag.message
        );
    }

    #[test]
    fn skipped_minified_file_diagnostic_id_and_message() {
        let root = Path::new("/project");
        let diag = WorkspaceDiagnostic::new(
            root,
            root.join("src/assets/index-abc123.js"),
            WorkspaceDiagnosticKind::SkippedMinifiedFile {
                size_bytes: 2 * 1024 * 1024,
            },
        );
        assert_eq!(diag.kind.id(), "skipped-minified-file");
        assert!(
            diag.message.contains("src/assets/index-abc123.js"),
            "message names the project-relative path: {}",
            diag.message
        );
        assert!(
            diag.message.contains("2.0 MB"),
            "message reports the size: {}",
            diag.message
        );
        assert!(
            diag.message.contains("--max-file-size 0"),
            "message names the opt-out: {}",
            diag.message
        );
    }

    #[test]
    fn skipped_source_dotdir_diagnostic_id_and_message() {
        let root = Path::new("/project");
        let diag = WorkspaceDiagnostic::new(
            root,
            root.join(".claude"),
            WorkspaceDiagnosticKind::SkippedSourceDotdir,
        );
        assert_eq!(diag.kind.id(), "skipped-source-dotdir");
        assert!(
            diag.message.contains(".claude"),
            "message names the project-relative path: {}",
            diag.message
        );
        assert!(
            diag.message
                .contains("Its imports and exports are not analyzed."),
            "message states the consequence: {}",
            diag.message
        );
        assert!(
            diag.message.contains("--root"),
            "message names the real remedy: {}",
            diag.message
        );
        assert!(
            diag.message.contains("no config field"),
            "the message must say plainly that no config field traverses it: {}",
            diag.message
        );
    }

    #[test]
    fn stash_preserves_appended_skipped_large_file_across_restash() {
        // Unique synthetic root so the process-global registry does not collide
        // with sibling tests.
        let root = Path::new("/fallow-test-1086-stash-preserve");
        let undeclared = || {
            WorkspaceDiagnostic::new(
                root,
                root.join("pkg"),
                WorkspaceDiagnosticKind::UndeclaredWorkspace,
            )
        };
        // First analysis loads config and stashes the workspace-discovery set.
        stash_workspace_diagnostics(root, vec![undeclared()]);
        // Its source discovery appends a skipped-large-file diagnostic.
        append_workspace_diagnostics(
            root,
            vec![WorkspaceDiagnostic::new(
                root,
                root.join("vendor/big.js"),
                WorkspaceDiagnosticKind::SkippedLargeFile {
                    size_bytes: 9_999_999,
                },
            )],
        );
        // A sibling analysis (combined-mode dupes/health) re-loads config and
        // re-stashes the same workspace-discovery set.
        stash_workspace_diagnostics(root, vec![undeclared()]);

        let after = workspace_diagnostics_for(root);
        assert_eq!(
            after
                .iter()
                .filter(|d| d.kind.is_source_discovery())
                .count(),
            1,
            "skipped-large-file survives the combined-mode re-stash exactly once (#1086): {after:?}"
        );
        assert_eq!(
            after
                .iter()
                .filter(|d| matches!(d.kind, WorkspaceDiagnosticKind::UndeclaredWorkspace))
                .count(),
            1,
            "the workspace-discovery diagnostic is replaced, not duplicated"
        );
    }

    fn analysis_stage_diagnostics(root: &Path) -> Vec<WorkspaceDiagnostic> {
        vec![
            WorkspaceDiagnostic::new(
                root,
                root.join("pnpm-workspace.yaml"),
                WorkspaceDiagnosticKind::MalformedPnpmWorkspaceYaml {
                    error: "could not find expected ':'".to_owned(),
                },
            ),
            WorkspaceDiagnostic::new(
                root,
                root.join("package.json"),
                WorkspaceDiagnosticKind::BunLockbOverrideResolutionSkipped,
            ),
        ]
    }

    fn count_kind(diagnostics: &[WorkspaceDiagnostic], id: &str) -> usize {
        diagnostics.iter().filter(|d| d.kind.id() == id).count()
    }

    #[test]
    fn stash_preserves_recorded_analysis_stage_diagnostics_across_restash() {
        let root = Path::new("/fallow-test-2366-stash-preserve");
        let undeclared = || {
            WorkspaceDiagnostic::new(
                root,
                root.join("pkg"),
                WorkspaceDiagnosticKind::UndeclaredWorkspace,
            )
        };
        // The check analysis loads config, then its analyze pass records both
        // analysis-stage kinds.
        stash_workspace_diagnostics(root, vec![undeclared()]);
        record_workspace_diagnostics(root, analysis_stage_diagnostics(root));
        // Combined-mode dupes/health re-load config and re-stash the same
        // workspace-discovery set before the JSON envelope is built.
        stash_workspace_diagnostics(root, vec![undeclared()]);

        let after = workspace_diagnostics_for(root);
        assert_eq!(
            count_kind(&after, "malformed-pnpm-workspace-yaml"),
            1,
            "malformed-pnpm-workspace-yaml survives the combined-mode re-stash exactly once (#2366): {after:?}"
        );
        assert_eq!(
            count_kind(&after, "bun-lockb-override-resolution-skipped"),
            1,
            "bun-lockb-override-resolution-skipped survives the combined-mode re-stash exactly once (#2366): {after:?}"
        );
        assert_eq!(
            count_kind(&after, "undeclared-workspace"),
            1,
            "the workspace-discovery diagnostic is replaced, not duplicated"
        );
    }

    #[test]
    fn source_read_failures_replace_only_their_previous_parse_set() {
        let root = Path::new("/fallow-test-source-read-replace");
        stash_workspace_diagnostics(
            root,
            vec![WorkspaceDiagnostic::new(
                root,
                root.join("pkg"),
                WorkspaceDiagnosticKind::UndeclaredWorkspace,
            )],
        );
        append_workspace_diagnostics(
            root,
            vec![WorkspaceDiagnostic::new(
                root,
                root.join("vendor/big.js"),
                WorkspaceDiagnosticKind::SkippedLargeFile { size_bytes: 99 },
            )],
        );
        let first = SourceReadFailure {
            file_id: FileId(1),
            path: root.join("src/first.ts"),
            error: "removed".to_string(),
        };
        let _ = record_source_read_failures(root, &[first]);
        let second = SourceReadFailure {
            file_id: FileId(2),
            path: root.join("src/second.ts"),
            error: "permission denied".to_string(),
        };

        let _ = record_source_read_failures(root, std::slice::from_ref(&second));

        let diagnostics = workspace_diagnostics_for(root);
        let source_failures: Vec<_> = diagnostics
            .iter()
            .filter(|diagnostic| {
                matches!(
                    diagnostic.kind,
                    WorkspaceDiagnosticKind::SourceReadFailure { .. }
                )
            })
            .collect();
        assert_eq!(source_failures.len(), 1);
        assert_eq!(source_failures[0].path, second.path);
        assert!(diagnostics.iter().any(|diagnostic| matches!(
            diagnostic.kind,
            WorkspaceDiagnosticKind::UndeclaredWorkspace
        )));
        assert!(diagnostics.iter().any(|diagnostic| matches!(
            diagnostic.kind,
            WorkspaceDiagnosticKind::SkippedLargeFile { .. }
        )));

        let _ = record_source_read_failures(root, &[]);
        assert!(workspace_diagnostics_for(root).iter().all(|diagnostic| {
            !matches!(
                diagnostic.kind,
                WorkspaceDiagnosticKind::SourceReadFailure { .. }
            )
        }));
    }

    #[test]
    fn clear_source_discovery_drops_stale_skip_keeps_workspace_diag() {
        let root = Path::new("/fallow-test-1086-clear-stale");
        stash_workspace_diagnostics(
            root,
            vec![WorkspaceDiagnostic::new(
                root,
                root.join("pkg"),
                WorkspaceDiagnosticKind::UndeclaredWorkspace,
            )],
        );
        append_workspace_diagnostics(
            root,
            vec![WorkspaceDiagnostic::new(
                root,
                root.join("vendor/big.js"),
                WorkspaceDiagnosticKind::SkippedLargeFile {
                    size_bytes: 9_999_999,
                },
            )],
        );
        // A later walk (the file is no longer skipped) clears the stale entry.
        let replaced = replace_source_discovery_diagnostics(root, Vec::new());
        assert!(
            replaced.is_empty(),
            "the walk's own list is what it wrote, not what it removed"
        );

        let after = workspace_diagnostics_for(root);
        assert!(
            !after.iter().any(|d| d.kind.is_source_discovery()),
            "stale skipped-large-file is dropped on the next walk (#1086 watch-mode): {after:?}"
        );
        assert!(
            after
                .iter()
                .any(|d| matches!(d.kind, WorkspaceDiagnosticKind::UndeclaredWorkspace)),
            "the workspace-discovery diagnostic survives the source-discovery clear"
        );
    }

    #[test]
    fn clear_analysis_stage_drops_stale_entries_keeps_other_kinds() {
        let root = Path::new("/fallow-test-2366-clear-stale");
        stash_workspace_diagnostics(
            root,
            vec![WorkspaceDiagnostic::new(
                root,
                root.join("pkg"),
                WorkspaceDiagnosticKind::UndeclaredWorkspace,
            )],
        );
        append_workspace_diagnostics(
            root,
            vec![WorkspaceDiagnostic::new(
                root,
                root.join("vendor/big.js"),
                WorkspaceDiagnosticKind::SkippedLargeFile {
                    size_bytes: 9_999_999,
                },
            )],
        );
        record_workspace_diagnostics(root, analysis_stage_diagnostics(root));
        // The next analyze pass (the yaml is fixed, a text bun.lock exists)
        // clears the stale entries before re-recording nothing.
        clear_analysis_stage_diagnostics(root);

        let after = workspace_diagnostics_for(root);
        assert!(
            !after.iter().any(|d| d.kind.is_analysis_stage()),
            "stale analysis-stage entries are dropped on the next analyze pass (#2366): {after:?}"
        );
        assert_eq!(
            count_kind(&after, "undeclared-workspace"),
            1,
            "the workspace-discovery diagnostic survives the analysis-stage clear"
        );
        assert_eq!(
            count_kind(&after, "skipped-large-file"),
            1,
            "the source-discovery diagnostic survives the analysis-stage clear"
        );
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
        // The key embeds `diag.path` through a raw `Display`, so the expected
        // tail carries the platform separator: the stored path is rebuilt from
        // its components and renders with backslashes on Windows.
        let expected_tail = Path::new("packages").join("scratch");
        assert!(
            plans[0]
                .dedupe_key
                .contains("::glob-matched-no-package-json::")
                && plans[0]
                    .dedupe_key
                    .ends_with(&expected_tail.display().to_string()),
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

    /// Issue #2366: the aggregated warning groups the diagnostics it is
    /// handed and counts the group, so a list that still holds the duplicate
    /// entries of one glob declared in two manifests reports a directory
    /// count that does not exist and names one directory twice among its
    /// examples. Deduplicating at discovery is what makes the summary true.
    #[test]
    fn two_manifest_glob_warning_counts_each_directory_once() {
        let dir = tempfile::tempdir().expect("create temp dir");
        crate::workspace::write_two_manifest_glob_project(dir.path());

        let (_, diagnostics) = crate::workspace::discover_workspaces_with_diagnostics(
            dir.path(),
            &globset::GlobSet::empty(),
        )
        .expect("root package.json is valid");

        let messages: Vec<String> = plan_warnings(dir.path(), &diagnostics)
            .into_iter()
            .map(|plan| plan.message)
            .collect();

        assert_eq!(
            messages,
            vec![
                "Glob 'pkgs/*' matched 2 directories with no package.json \
                 (e.g. pkgs/aaa, pkgs/bbb). Add a package.json, narrow the \
                 pattern, or add them to ignorePatterns."
                    .to_owned()
            ],
            "the summary names the true directory count and each example once"
        );
    }
}
