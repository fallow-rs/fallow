//! Git-aware "changed files" filtering shared between fallow-cli and fallow-lsp.
//!
//! Provides:
//! - [`validate_git_ref`] for input validation at trust boundaries.
//! - [`ChangedFilesError`] / [`try_get_changed_files`] / [`get_changed_files`]
//!   for resolving a git ref into the set of changed files.
//! - [`filter_results_by_changed_files`] for narrowing an [`AnalysisResults`]
//!   to issues in those files.
//! - [`filter_duplication_by_changed_files`] for narrowing a
//!   [`DuplicationReport`] to clone groups touching at least one changed file.
//!
//! Both filters intentionally exclude dependency-level issues (unused deps,
//! type-only deps, test-only deps) since "unused dependency" is a function of
//! the entire import graph and can't be attributed to individual changed files.

use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::OnceLock;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::duplicates::{DuplicationReport, DuplicationStats, families};
use crate::results::AnalysisResults;

/// Function pointer signature used by `set_spawn_hook` to intercept the
/// short-running `git rev-parse` / `git diff` / `git ls-files` subprocesses
/// this module spawns. Lets the CLI route those git children through its
/// `ScopedChild` registry so a SIGINT delivered to the parent during
/// watch mode (or any analysis) reaps them instead of letting them run
/// to completion. See `crates/cli/src/signal/` and issue #477.
pub type ChangedFilesSpawnHook = fn(&mut std::process::Command) -> std::io::Result<Output>;

static SPAWN_HOOK: OnceLock<ChangedFilesSpawnHook> = OnceLock::new();

/// Install a spawn-hook for this module's git subprocesses. Idempotent;
/// subsequent calls are no-ops. Called once from the CLI's `main()` so
/// long-running watch sessions reap pending git children on Ctrl+C.
/// Defaults to `Command::output` when not set; the function-pointer
/// indirection costs nothing for embedders and tests that don't install
/// a hook.
pub fn set_spawn_hook(hook: ChangedFilesSpawnHook) {
    let _ = SPAWN_HOOK.set(hook);
}

fn spawn_output(command: &mut std::process::Command) -> std::io::Result<Output> {
    if let Some(hook) = SPAWN_HOOK.get() {
        hook(command)
    } else {
        command.output()
    }
}

/// Validate a user-supplied git ref before passing it to `git diff`.
///
/// Rejects empty strings, refs starting with `-` (which `git` would interpret
/// as an option flag), and characters outside the safe allowlist for branch
/// names, tags, SHAs, and reflog expressions (`HEAD~N`, `HEAD@{...}`).
///
/// Inside `@{...}` braces, colons and spaces are allowed so reflog timestamps
/// like `HEAD@{2025-01-01}` and `HEAD@{1 week ago}` round-trip.
///
/// Used by both the CLI (clap value parser) and the LSP (initializationOptions
/// trust boundary) to fail fast with a readable error rather than handing a
/// malformed ref to git.
pub fn validate_git_ref(s: &str) -> Result<&str, String> {
    if s.is_empty() {
        return Err("git ref cannot be empty".to_string());
    }
    if s.starts_with('-') {
        return Err("git ref cannot start with '-'".to_string());
    }
    let mut in_braces = false;
    for c in s.chars() {
        match c {
            '{' => in_braces = true,
            '}' => in_braces = false,
            ':' | ' ' if in_braces => {}
            c if c.is_ascii_alphanumeric()
                || matches!(c, '.' | '_' | '-' | '/' | '~' | '^' | '@' | '{' | '}') => {}
            _ => return Err(format!("git ref contains disallowed character: '{c}'")),
        }
    }
    if in_braces {
        return Err("git ref has unclosed '{'".to_string());
    }
    Ok(s)
}

/// Classification of a `git diff` failure, so callers can pick their own
/// wording (soft warning vs hard error) without re-parsing stderr.
#[derive(Debug)]
pub enum ChangedFilesError {
    /// Git ref failed validation before invoking `git`.
    InvalidRef(String),
    /// `git` binary not found / not executable.
    GitMissing(String),
    /// Command ran but the directory isn't a git repository.
    NotARepository,
    /// Command ran but the ref is invalid / another git error.
    GitFailed(String),
}

impl ChangedFilesError {
    /// Human-readable clause suitable for embedding in an error message.
    /// Does not include the flag name (e.g. "--changed-since") so callers can
    /// prepend their own context.
    pub fn describe(&self) -> String {
        match self {
            Self::InvalidRef(e) => format!("invalid git ref: {e}"),
            Self::GitMissing(e) => format!("failed to run git: {e}"),
            Self::NotARepository => "not a git repository".to_owned(),
            Self::GitFailed(stderr) => augment_git_failed(stderr),
        }
    }
}

/// Enrich a raw `git diff` stderr with actionable hints when the failure mode
/// is recognizable. Today: shallow-clone misses (`actions/checkout@v4` defaults
/// to `fetch-depth: 1`, GitLab CI to `GIT_DEPTH: 50`), where the baseline ref
/// predates the fetch boundary. Bare git stderr is famously cryptic; a hint
/// here is much more useful than a docs link the reader has to chase.
fn augment_git_failed(stderr: &str) -> String {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("not a valid object name")
        || lower.contains("unknown revision")
        || lower.contains("ambiguous argument")
    {
        format!(
            "{stderr} (shallow clone? try `git fetch --unshallow`, or set `fetch-depth: 0` on actions/checkout / `GIT_DEPTH: 0` in GitLab CI)"
        )
    } else {
        stderr.to_owned()
    }
}

/// Resolve the canonical git toplevel for `cwd`.
///
/// Runs `git rev-parse --show-toplevel`, which is git's own answer to "where
/// does this repository live?". The returned path is canonicalized so it
/// agrees with paths produced by `fs::canonicalize` elsewhere on macOS
/// (`/tmp` -> `/private/tmp`) and Windows (8.3 short paths).
///
/// Used by `try_get_changed_files` to produce changed-file paths whose
/// absolute form matches what the analysis pipeline emits, regardless of
/// whether the caller's `cwd` is the repo root or a subdirectory of it.
pub fn resolve_git_toplevel(cwd: &Path) -> Result<PathBuf, ChangedFilesError> {
    let output = spawn_output(&mut git_command(cwd, &["rev-parse", "--show-toplevel"]))
        .map_err(|e| ChangedFilesError::GitMissing(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(if stderr.contains("not a git repository") {
            ChangedFilesError::NotARepository
        } else {
            ChangedFilesError::GitFailed(stderr.trim().to_owned())
        });
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ChangedFilesError::GitFailed(
            "git rev-parse --show-toplevel returned empty output".to_owned(),
        ));
    }

    let path = PathBuf::from(trimmed);
    Ok(dunce::canonicalize(&path).unwrap_or(path))
}

/// Resolve the canonical git *common* directory for `cwd`.
///
/// Runs `git rev-parse --path-format=absolute --git-common-dir`. Unlike
/// `--show-toplevel` (which returns each worktree's own working directory),
/// `--git-common-dir` returns the SHARED `.git` directory of the repository,
/// so every linked worktree of the same repo resolves to the SAME path. This
/// is what lets the Impact store collapse all worktrees of a repo onto a
/// single identity (one history per repo, not per checkout).
///
/// `--path-format=absolute` (git 2.31+) forces an absolute result, so the
/// bare-`.git` relative form `--git-common-dir` would otherwise emit at the
/// repo root is avoided. The path is canonicalized to agree with paths from
/// `fs::canonicalize` elsewhere (macOS `/tmp` -> `/private/tmp`, Windows 8.3).
pub fn resolve_git_common_dir(cwd: &Path) -> Result<PathBuf, ChangedFilesError> {
    let output = spawn_output(&mut git_command(
        cwd,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ))
    .map_err(|e| ChangedFilesError::GitMissing(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(if stderr.contains("not a git repository") {
            ChangedFilesError::NotARepository
        } else {
            ChangedFilesError::GitFailed(stderr.trim().to_owned())
        });
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ChangedFilesError::GitFailed(
            "git rev-parse --git-common-dir returned empty output".to_owned(),
        ));
    }

    let path = PathBuf::from(trimmed);
    Ok(dunce::canonicalize(&path).unwrap_or(path))
}

fn collect_git_paths(
    cwd: &Path,
    toplevel: &Path,
    args: &[&str],
) -> Result<FxHashSet<PathBuf>, ChangedFilesError> {
    let output = spawn_output(&mut git_command(cwd, args))
        .map_err(|e| ChangedFilesError::GitMissing(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(if stderr.contains("not a git repository") {
            ChangedFilesError::NotARepository
        } else {
            ChangedFilesError::GitFailed(stderr.trim().to_owned())
        });
    }

    #[cfg(windows)]
    let normalise_segment = |line: &str| line.replace('/', "\\");
    #[cfg(not(windows))]
    let normalise_segment = |line: &str| line.to_owned();

    let files: FxHashSet<PathBuf> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| toplevel.join(normalise_segment(line)))
        .collect();

    Ok(files)
}

fn git_command(cwd: &Path, args: &[&str]) -> std::process::Command {
    let mut command = crate::spawn::git();
    command.args(args).current_dir(cwd);
    command
}

/// Get files changed since a git ref. Returns `Err` (with details) when the
/// git invocation itself failed, so callers can choose between warn-and-ignore
/// and hard-error behavior.
///
/// Includes both:
/// - committed changes from the merge-base range `git_ref...HEAD`
/// - tracked staged/unstaged changes from `HEAD` to the current worktree
/// - untracked files not ignored by Git
///
/// This keeps `--changed-since` useful for local validation instead of only
/// reflecting the last committed `HEAD`.
///
/// All paths in the returned set are absolute and rooted at the canonical
/// git toplevel, not at `root`. This matters when the LSP / CLI is invoked
/// from a subdirectory of the repository (e.g., a Turborepo workspace at
/// `apps/web`): `git diff` emits root-relative paths, and we need to join
/// them against the actual repo root rather than the caller's cwd.
pub fn try_get_changed_files(
    root: &Path,
    git_ref: &str,
) -> Result<FxHashSet<PathBuf>, ChangedFilesError> {
    validate_git_ref(git_ref).map_err(ChangedFilesError::InvalidRef)?;
    let toplevel = resolve_git_toplevel(root)?;
    try_get_changed_files_with_toplevel(root, &toplevel, git_ref)
}

/// Like [`try_get_changed_files`], but takes a pre-resolved canonical
/// `toplevel` so callers (the LSP) can cache it across runs and avoid the
/// extra `git rev-parse --show-toplevel` subprocess on every save.
///
/// `toplevel` MUST be the canonical git toplevel for `cwd`; passing anything
/// else produces incorrect changed-file paths. The CLI does not call this
/// directly: it uses [`try_get_changed_files`] which resolves on each call.
pub fn try_get_changed_files_with_toplevel(
    cwd: &Path,
    toplevel: &Path,
    git_ref: &str,
) -> Result<FxHashSet<PathBuf>, ChangedFilesError> {
    validate_git_ref(git_ref).map_err(ChangedFilesError::InvalidRef)?;

    let mut files = collect_git_paths(
        cwd,
        toplevel,
        &[
            "diff",
            "--name-only",
            "--end-of-options",
            &format!("{git_ref}...HEAD"),
        ],
    )?;
    files.extend(collect_git_paths(
        cwd,
        toplevel,
        &["diff", "--name-only", "HEAD"],
    )?);
    files.extend(collect_git_paths(
        cwd,
        toplevel,
        &["ls-files", "--full-name", "--others", "--exclude-standard"],
    )?);
    Ok(files)
}

/// Get the zero-context unified diff of the merge-base range `git_ref...HEAD`,
/// with paths relative to `root`, for the line-level security gate (issue #886).
///
/// Unlike [`get_changed_files`] (which falls back to full scope on failure), this
/// returns `Err` when the git invocation itself fails (missing/unfetched ref,
/// shallow clone, not a repo). The security gate hard-errors on `Err` rather than
/// emitting a green gate: a diff it could not compute must NEVER read as "no new
/// sinks". `--relative` emits paths relative to `root` (rewriting the prefix to
/// match the keys `DiffIndex` is queried with, `relative_to_diff_path(finding,
/// root)`) and, when fallow runs in a monorepo subpackage, omits changes outside
/// `root` from the output entirely; a sibling-package edit `git diff --relative`
/// did emit would carry a `../...` path that `relative_to_diff_path` cannot strip
/// (returns `None`), which is harmless because no findings exist for files
/// outside the analyzed `root`. An empty diff (no changes / docs-only) is
/// `Ok("")`, a clean pass, not an error.
pub fn try_get_changed_diff(root: &Path, git_ref: &str) -> Result<String, ChangedFilesError> {
    validate_git_ref(git_ref).map_err(ChangedFilesError::InvalidRef)?;
    let output = spawn_output(&mut git_command(
        root,
        &[
            "diff",
            "--relative",
            "--unified=0",
            "--end-of-options",
            &format!("{git_ref}...HEAD"),
        ],
    ))
    .map_err(|e| ChangedFilesError::GitMissing(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(if stderr.contains("not a git repository") {
            ChangedFilesError::NotARepository
        } else {
            ChangedFilesError::GitFailed(stderr.trim().to_owned())
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Get files changed since a git ref. Returns `None` on git failure after
/// printing a warning to stderr. Used by `--changed-since` and `--file`, where
/// a failure falls back to full-scope analysis.
#[expect(
    clippy::print_stderr,
    reason = "intentional user-facing warning for the CLI's --changed-since fallback path; LSP callers use try_get_changed_files instead"
)]
pub fn get_changed_files(root: &Path, git_ref: &str) -> Option<FxHashSet<PathBuf>> {
    match try_get_changed_files(root, git_ref) {
        Ok(files) => Some(files),
        Err(ChangedFilesError::InvalidRef(e)) => {
            eprintln!("Warning: --changed-since ignored: invalid git ref: {e}");
            None
        }
        Err(ChangedFilesError::GitMissing(e)) => {
            eprintln!("Warning: --changed-since ignored: failed to run git: {e}");
            None
        }
        Err(ChangedFilesError::NotARepository) => {
            eprintln!("Warning: --changed-since ignored: not a git repository");
            None
        }
        Err(ChangedFilesError::GitFailed(stderr)) => {
            eprintln!("Warning: --changed-since failed for ref '{git_ref}': {stderr}");
            None
        }
    }
}

/// Filter `results` to only include issues whose source file is in
/// `changed_files`.
///
/// Dependency-level issues (unused deps, dev deps, optional deps, type-only
/// deps, test-only deps) are intentionally NOT filtered here. Unlike
/// file-level issues, a dependency being "unused" is a function of the entire
/// import graph and can't be attributed to individual changed source files.
///
/// This destructure is deliberately exhaustive: adding a field to
/// `AnalysisResults` must fail compilation here so the author decides
/// explicitly whether the new finding type is file-attributable (add a retain)
/// or graph-global (bind with underscore and document why).
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across the workspace"
)]
pub fn filter_results_by_changed_files(
    results: &mut AnalysisResults,
    changed_files: &FxHashSet<PathBuf>,
) {
    let AnalysisResults {
        unused_files,
        unused_exports,
        unused_types,
        private_type_leaks,
        // Dependency-level issues are graph-global: "unused" is a function of
        // the whole import graph and cannot be attributed to a changed file.
        unused_dependencies: _unused_dependencies,
        unused_dev_dependencies: _unused_dev_dependencies,
        unused_optional_dependencies: _unused_optional_dependencies,
        unused_enum_members,
        unused_class_members,
        unused_store_members,
        unresolved_imports,
        unlisted_dependencies,
        duplicate_exports,
        // Type-only and test-only dependency issues are graph-global for the
        // same reason as the other dependency kinds above.
        type_only_dependencies: _type_only_dependencies,
        test_only_dependencies: _test_only_dependencies,
        circular_dependencies,
        re_export_cycles,
        boundary_violations,
        boundary_coverage_violations,
        boundary_call_violations,
        policy_violations,
        stale_suppressions,
        // Catalog entries are workspace-global: whether a catalog entry is
        // unused depends on all workspace packages, not a single changed file.
        unused_catalog_entries: _unused_catalog_entries,
        empty_catalog_groups,
        unresolved_catalog_references,
        unused_dependency_overrides,
        misconfigured_dependency_overrides,
        invalid_client_exports,
        mixed_client_server_barrels,
        misplaced_directives,
        // Non-finding fields: counts and metadata, not issue collections.
        suppression_count: _suppression_count,
        active_suppressions: _active_suppressions,
        feature_flags: _feature_flags,
        security_findings,
        security_unresolved_edge_files: _security_unresolved_edge_files,
        security_unresolved_callee_sites: _security_unresolved_callee_sites,
        security_unresolved_callee_diagnostics,
        // Export usages and entry-point summary are metadata, not issue
        // collections; they are not changed-files filtered.
        export_usages: _export_usages,
        entry_point_summary: _entry_point_summary,
    } = &mut *results;

    let cf = normalize_changed_files_set(changed_files);
    unused_files.retain(|f| contains_normalized(&cf, &f.file.path));
    unused_exports.retain(|e| contains_normalized(&cf, &e.export.path));
    unused_types.retain(|e| contains_normalized(&cf, &e.export.path));
    private_type_leaks.retain(|e| contains_normalized(&cf, &e.leak.path));
    unused_enum_members.retain(|m| contains_normalized(&cf, &m.member.path));
    unused_class_members.retain(|m| contains_normalized(&cf, &m.member.path));
    unused_store_members.retain(|m| contains_normalized(&cf, &m.member.path));
    unresolved_imports.retain(|i| contains_normalized(&cf, &i.import.path));

    unlisted_dependencies.retain(|d| {
        d.dep
            .imported_from
            .iter()
            .any(|s| contains_normalized(&cf, &s.path))
    });

    for dup in &mut *duplicate_exports {
        dup.export
            .locations
            .retain(|loc| contains_normalized(&cf, &loc.path));
    }
    duplicate_exports.retain(|d| d.export.locations.len() >= 2);

    circular_dependencies.retain(|c| c.cycle.files.iter().any(|f| contains_normalized(&cf, f)));

    re_export_cycles.retain(|c| c.cycle.files.iter().any(|f| contains_normalized(&cf, f)));

    boundary_violations.retain(|v| contains_normalized(&cf, &v.violation.from_path));
    boundary_coverage_violations.retain(|v| contains_normalized(&cf, &v.violation.path));
    boundary_call_violations.retain(|v| contains_normalized(&cf, &v.violation.path));
    policy_violations.retain(|v| contains_normalized(&cf, &v.violation.path));

    stale_suppressions.retain(|s| contains_normalized(&cf, &s.path));

    security_findings.retain(|f| {
        contains_normalized(&cf, &f.path)
            || f.trace
                .iter()
                .any(|hop| contains_normalized(&cf, &hop.path))
            || f.reachability.as_ref().is_some_and(|reachability| {
                reachability
                    .untrusted_source_trace
                    .iter()
                    .any(|hop| contains_normalized(&cf, &hop.path))
            })
    });
    security_unresolved_callee_diagnostics.retain(|d| contains_normalized(&cf, &d.path));

    unresolved_catalog_references.retain(|r| contains_normalized(&cf, &r.reference.path));
    empty_catalog_groups.retain(|g| normalized_set_contains_path(&cf, &g.group.path));

    unused_dependency_overrides.retain(|o| contains_normalized(&cf, &o.entry.path));
    misconfigured_dependency_overrides.retain(|o| contains_normalized(&cf, &o.entry.path));

    invalid_client_exports.retain(|e| contains_normalized(&cf, &e.export.path));
    mixed_client_server_barrels.retain(|b| contains_normalized(&cf, &b.barrel.path));
    misplaced_directives.retain(|d| contains_normalized(&cf, &d.directive_site.path));
}

/// Pre-normalise a `changed_files` set through `dunce::simplified` so each
/// per-entry comparison can normalise its lookup side and avoid the Windows
/// `\\?\` verbatim-vs-non-verbatim mismatch. On POSIX `dunce::simplified` is
/// a no-op, so this is identical to cloning the set.
///
/// Background: `try_get_changed_files` joins git-emitted segments onto the
/// `dunce::canonicalize`d toplevel, so entries land in non-verbatim shape.
/// Analysis-pipeline paths (clone instances, finding paths) inherit the
/// shape of `opts.root`, which `validate_root` / discovery / cache lookups
/// pre-canonicalise with `std::fs::canonicalize` in test fixtures and tools
/// (which yields verbatim paths on Windows). Comparing the two sides byte
/// for byte silently dropped every finding before this normalisation.
fn normalize_changed_files_set(changed_files: &FxHashSet<PathBuf>) -> FxHashSet<PathBuf> {
    changed_files
        .iter()
        .map(|p| dunce::simplified(p).to_path_buf())
        .collect()
}

fn contains_normalized(normalized: &FxHashSet<PathBuf>, path: &Path) -> bool {
    normalized.contains(dunce::simplified(path))
}

fn normalized_set_contains_path(normalized: &FxHashSet<PathBuf>, path: &Path) -> bool {
    contains_normalized(normalized, path)
        || (path.is_relative() && normalized.iter().any(|changed| changed.ends_with(path)))
}

/// Recompute duplication statistics after filtering.
///
/// Uses per-file line deduplication (matching `compute_stats` in
/// `duplicates/detect.rs`) so overlapping clone instances don't inflate the
/// duplicated line count.
fn recompute_duplication_stats(report: &DuplicationReport) -> DuplicationStats {
    let mut files_with_clones: FxHashSet<&Path> = FxHashSet::default();
    let mut file_dup_lines: FxHashMap<&Path, FxHashSet<usize>> = FxHashMap::default();
    let mut duplicated_tokens = 0_usize;
    let mut clone_instances = 0_usize;

    for group in &report.clone_groups {
        for instance in &group.instances {
            files_with_clones.insert(&instance.file);
            clone_instances += 1;
            let lines = file_dup_lines.entry(&instance.file).or_default();
            for line in instance.start_line..=instance.end_line {
                lines.insert(line);
            }
        }
        duplicated_tokens += group.token_count * group.instances.len();
    }

    let duplicated_lines: usize = file_dup_lines.values().map(FxHashSet::len).sum();

    DuplicationStats {
        total_files: report.stats.total_files,
        files_with_clones: files_with_clones.len(),
        total_lines: report.stats.total_lines,
        duplicated_lines,
        total_tokens: report.stats.total_tokens,
        duplicated_tokens,
        clone_groups: report.clone_groups.len(),
        clone_instances,
        #[expect(
            clippy::cast_precision_loss,
            reason = "stat percentages are display-only; precision loss at usize::MAX line counts is acceptable"
        )]
        duplication_percentage: if report.stats.total_lines > 0 {
            (duplicated_lines as f64 / report.stats.total_lines as f64) * 100.0
        } else {
            0.0
        },
        clone_groups_below_min_occurrences: report.stats.clone_groups_below_min_occurrences,
    }
}

/// Filter a duplication report to only retain clone groups where at least one
/// instance belongs to a changed file. Families, mirrored directories, and
/// stats are rebuilt from the surviving groups so consumers see consistent,
/// correctly-scoped numbers.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across the workspace"
)]
pub fn filter_duplication_by_changed_files(
    report: &mut DuplicationReport,
    changed_files: &FxHashSet<PathBuf>,
    root: &Path,
) {
    let cf = normalize_changed_files_set(changed_files);
    report.clone_groups.retain(|g| {
        g.instances
            .iter()
            .any(|i| contains_normalized(&cf, &i.file))
    });
    report.clone_families = families::group_into_families(&report.clone_groups, root);
    report.mirrored_directories =
        families::detect_mirrored_directories(&report.clone_families, root);
    report.stats = recompute_duplication_stats(report);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::duplicates::{CloneGroup, CloneInstance};
    use crate::results::{
        BoundaryViolation, CircularDependency, EmptyCatalogGroup, SecurityFinding,
        SecurityFindingKind, SecurityUnresolvedCalleeDiagnostic, TraceHop, TraceHopRole,
        UnusedExport, UnusedFile,
    };
    use fallow_types::extract::{SkippedSecurityCalleeExpressionKind, SkippedSecurityCalleeReason};
    use fallow_types::output_dead_code::{
        BoundaryViolationFinding, CircularDependencyFinding, EmptyCatalogGroupFinding,
        UnusedExportFinding, UnusedFileFinding,
    };
    use fallow_types::results::{SecurityReachability, SecuritySeverity};

    #[test]
    fn changed_files_error_describe_variants() {
        assert!(
            ChangedFilesError::InvalidRef("bad".to_owned())
                .describe()
                .contains("invalid git ref")
        );
        assert!(
            ChangedFilesError::GitMissing("oops".to_owned())
                .describe()
                .contains("oops")
        );
        assert_eq!(
            ChangedFilesError::NotARepository.describe(),
            "not a git repository"
        );
        assert!(
            ChangedFilesError::GitFailed("bad ref".to_owned())
                .describe()
                .contains("bad ref")
        );
    }

    #[test]
    fn augment_git_failed_appends_shallow_clone_hint_for_unknown_revision() {
        let stderr = "fatal: ambiguous argument 'fallow-baseline...HEAD': unknown revision or path not in the working tree.";
        let described = ChangedFilesError::GitFailed(stderr.to_owned()).describe();
        assert!(described.contains(stderr), "original stderr preserved");
        assert!(
            described.contains("shallow clone"),
            "hint surfaced: {described}"
        );
        assert!(
            described.contains("fetch-depth: 0") || described.contains("git fetch --unshallow"),
            "hint actionable: {described}"
        );
    }

    #[test]
    fn augment_git_failed_passthrough_for_other_errors() {
        let stderr = "fatal: refusing to merge unrelated histories";
        let described = ChangedFilesError::GitFailed(stderr.to_owned()).describe();
        assert_eq!(described, stderr);
    }

    #[test]
    fn validate_git_ref_rejects_leading_dash() {
        assert!(validate_git_ref("--upload-pack=evil").is_err());
        assert!(validate_git_ref("-flag").is_err());
    }

    #[test]
    fn validate_git_ref_accepts_baseline_tag() {
        assert_eq!(
            validate_git_ref("fallow-baseline").unwrap(),
            "fallow-baseline"
        );
    }

    #[test]
    fn changed_files_filter_scopes_unresolved_callee_diagnostics() {
        let mut results = AnalysisResults::default();
        results
            .security_unresolved_callee_diagnostics
            .push(SecurityUnresolvedCalleeDiagnostic {
                path: PathBuf::from("/repo/src/changed.ts"),
                line: 4,
                col: 0,
                reason: SkippedSecurityCalleeReason::DynamicDispatch,
                expression_kind: SkippedSecurityCalleeExpressionKind::Other,
            });
        results
            .security_unresolved_callee_diagnostics
            .push(SecurityUnresolvedCalleeDiagnostic {
                path: PathBuf::from("/repo/src/unchanged.ts"),
                line: 4,
                col: 0,
                reason: SkippedSecurityCalleeReason::ComputedMember,
                expression_kind: SkippedSecurityCalleeExpressionKind::ComputedMemberExpression,
            });

        let mut changed: FxHashSet<PathBuf> = FxHashSet::default();
        changed.insert(PathBuf::from("/repo/src/changed.ts"));

        filter_results_by_changed_files(&mut results, &changed);

        assert_eq!(results.security_unresolved_callee_diagnostics.len(), 1);
        assert_eq!(
            results.security_unresolved_callee_diagnostics[0].path,
            PathBuf::from("/repo/src/changed.ts")
        );
    }

    #[test]
    fn try_get_changed_files_rejects_invalid_ref() {
        let err = try_get_changed_files(Path::new("/"), "--evil")
            .expect_err("leading-dash ref must be rejected");
        assert!(matches!(err, ChangedFilesError::InvalidRef(_)));
        assert!(err.describe().contains("cannot start with"));
    }

    #[test]
    fn validate_git_ref_rejects_option_like_ref() {
        assert!(validate_git_ref("--output=/tmp/fallow-proof").is_err());
    }

    #[test]
    fn validate_git_ref_allows_reflog_relative_date() {
        assert!(validate_git_ref("HEAD@{1 week ago}").is_ok());
    }

    #[test]
    fn try_get_changed_files_rejects_option_like_ref_before_git() {
        let root = tempfile::tempdir().expect("create temp dir");
        let proof_path = root.path().join("proof");

        let result = try_get_changed_files(
            root.path(),
            &format!("--output={}", proof_path.to_string_lossy()),
        );

        assert!(matches!(result, Err(ChangedFilesError::InvalidRef(_))));
        assert!(
            !proof_path.exists(),
            "invalid changedSince ref must not be passed through to git as an option"
        );
    }

    #[test]
    fn git_command_clears_parent_git_environment() {
        let command = git_command(Path::new("."), &["status", "--short"]);
        let overrides: Vec<_> = command.get_envs().collect();

        for var in crate::git_env::AMBIENT_GIT_ENV_VARS {
            assert!(
                overrides
                    .iter()
                    .any(|(key, value)| key.to_str() == Some(*var) && value.is_none()),
                "git helper must clear inherited {var}",
            );
        }
    }

    #[test]
    fn filter_results_keeps_only_changed_files() {
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: "/a.ts".into(),
            }));
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: "/b.ts".into(),
            }));
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: "/a.ts".into(),
                export_name: "foo".into(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));

        let mut changed: FxHashSet<PathBuf> = FxHashSet::default();
        changed.insert("/a.ts".into());

        filter_results_by_changed_files(&mut results, &changed);

        assert_eq!(results.unused_files.len(), 1);
        assert_eq!(results.unused_files[0].file.path, PathBuf::from("/a.ts"));
        assert_eq!(results.unused_exports.len(), 1);
    }

    #[test]
    fn filter_results_preserves_dependency_level_issues() {
        let mut results = AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_types::output_dead_code::UnusedDependencyFinding::with_actions(
                crate::results::UnusedDependency {
                    package_name: "lodash".into(),
                    location: crate::results::DependencyLocation::Dependencies,
                    path: "/pkg.json".into(),
                    line: 3,
                    used_in_workspaces: Vec::new(),
                },
            ),
        );

        let changed: FxHashSet<PathBuf> = FxHashSet::default();
        filter_results_by_changed_files(&mut results, &changed);

        assert_eq!(results.unused_dependencies.len(), 1);
    }

    #[test]
    fn filter_results_keeps_circular_dep_when_any_file_changed() {
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec!["/a.ts".into(), "/b.ts".into()],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));

        let mut changed: FxHashSet<PathBuf> = FxHashSet::default();
        changed.insert("/b.ts".into());

        filter_results_by_changed_files(&mut results, &changed);
        assert_eq!(results.circular_dependencies.len(), 1);
    }

    #[test]
    fn filter_results_drops_circular_dep_when_no_file_changed() {
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec!["/a.ts".into(), "/b.ts".into()],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));

        let changed: FxHashSet<PathBuf> = FxHashSet::default();
        filter_results_by_changed_files(&mut results, &changed);
        assert!(results.circular_dependencies.is_empty());
    }

    #[test]
    fn filter_results_drops_boundary_violation_when_importer_unchanged() {
        let mut results = AnalysisResults::default();
        results
            .boundary_violations
            .push(BoundaryViolationFinding::with_actions(BoundaryViolation {
                from_path: "/a.ts".into(),
                to_path: "/b.ts".into(),
                from_zone: "ui".into(),
                to_zone: "data".into(),
                import_specifier: "../data/db".into(),
                line: 1,
                col: 0,
            }));

        let mut changed: FxHashSet<PathBuf> = FxHashSet::default();
        changed.insert("/b.ts".into());

        filter_results_by_changed_files(&mut results, &changed);
        assert!(results.boundary_violations.is_empty());
    }

    #[test]
    fn filter_results_keeps_security_finding_when_trace_file_changed() {
        let mut results = AnalysisResults::default();
        results.security_findings.push(SecurityFinding {
            finding_id: String::new(),
            candidate: fallow_types::results::SecurityCandidate::default(),
            taint_flow: None,
            attack_surface: None,
            kind: SecurityFindingKind::ClientServerLeak,
            category: None,
            cwe: None,
            path: "/project/src/client.tsx".into(),
            line: 2,
            col: 0,
            evidence: "candidate".into(),
            source_backed: false,
            source_read: None,
            severity: SecuritySeverity::Low,
            trace: vec![
                TraceHop {
                    path: "/project/src/client.tsx".into(),
                    line: 2,
                    col: 0,
                    role: TraceHopRole::ClientBoundary,
                },
                TraceHop {
                    path: "/project/src/server.ts".into(),
                    line: 1,
                    col: 0,
                    role: TraceHopRole::SecretSource,
                },
            ],
            actions: Vec::new(),
            dead_code: None,
            reachability: None,
            runtime: None,
        });

        let mut changed: FxHashSet<PathBuf> = FxHashSet::default();
        changed.insert("/project/src/server.ts".into());

        filter_results_by_changed_files(&mut results, &changed);

        assert_eq!(results.security_findings.len(), 1);
    }

    #[test]
    fn filter_results_keeps_security_finding_when_untrusted_source_trace_file_changed() {
        let mut results = AnalysisResults::default();
        results.security_findings.push(SecurityFinding {
            finding_id: String::new(),
            candidate: fallow_types::results::SecurityCandidate::default(),
            taint_flow: None,
            attack_surface: None,
            kind: SecurityFindingKind::TaintedSink,
            category: Some("command-injection".into()),
            cwe: Some(78),
            path: "/project/src/runner.ts".into(),
            line: 4,
            col: 2,
            evidence: "candidate".into(),
            source_backed: false,
            source_read: None,
            severity: SecuritySeverity::Low,
            trace: Vec::new(),
            actions: Vec::new(),
            dead_code: None,
            reachability: Some(SecurityReachability {
                reachable_from_entry: false,
                reachable_from_untrusted_source: true,
                taint_confidence: Some(fallow_types::results::TaintConfidence::ModuleLevel),
                untrusted_source_hop_count: Some(1),
                untrusted_source_trace: vec![
                    TraceHop {
                        path: "/project/src/route.ts".into(),
                        line: 1,
                        col: 0,
                        role: TraceHopRole::UntrustedSource,
                    },
                    TraceHop {
                        path: "/project/src/runner.ts".into(),
                        line: 4,
                        col: 2,
                        role: TraceHopRole::Sink,
                    },
                ],
                blast_radius: 0,
                crosses_boundary: false,
            }),
            runtime: None,
        });

        let mut changed: FxHashSet<PathBuf> = FxHashSet::default();
        changed.insert("/project/src/route.ts".into());

        filter_results_by_changed_files(&mut results, &changed);

        assert_eq!(results.security_findings.len(), 1);
    }

    #[test]
    fn filter_results_keeps_relative_empty_catalog_group_when_manifest_changed() {
        let mut results = AnalysisResults::default();
        results
            .empty_catalog_groups
            .push(EmptyCatalogGroupFinding::with_actions(EmptyCatalogGroup {
                catalog_name: "legacy".into(),
                path: PathBuf::from("pnpm-workspace.yaml"),
                line: 4,
            }));

        let mut changed: FxHashSet<PathBuf> = FxHashSet::default();
        changed.insert(PathBuf::from("/repo/pnpm-workspace.yaml"));

        filter_results_by_changed_files(&mut results, &changed);

        assert_eq!(results.empty_catalog_groups.len(), 1);
        assert_eq!(results.empty_catalog_groups[0].group.catalog_name, "legacy");
    }

    #[test]
    fn filter_duplication_keeps_groups_with_at_least_one_changed_instance() {
        let mut report = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![
                    CloneInstance {
                        file: "/a.ts".into(),
                        start_line: 1,
                        end_line: 5,
                        start_col: 0,
                        end_col: 10,
                        fragment: "code".into(),
                    },
                    CloneInstance {
                        file: "/b.ts".into(),
                        start_line: 1,
                        end_line: 5,
                        start_col: 0,
                        end_col: 10,
                        fragment: "code".into(),
                    },
                ],
                token_count: 20,
                line_count: 5,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 2,
                files_with_clones: 2,
                total_lines: 100,
                duplicated_lines: 10,
                total_tokens: 200,
                duplicated_tokens: 40,
                clone_groups: 1,
                clone_instances: 2,
                duplication_percentage: 10.0,
                clone_groups_below_min_occurrences: 0,
            },
        };

        let mut changed: FxHashSet<PathBuf> = FxHashSet::default();
        changed.insert("/a.ts".into());

        filter_duplication_by_changed_files(&mut report, &changed, Path::new(""));
        assert_eq!(report.clone_groups.len(), 1);
        assert_eq!(report.stats.clone_groups, 1);
        assert_eq!(report.stats.clone_instances, 2);
    }

    /// Regression for issue #561: on Windows, `try_get_changed_files` joins
    /// segments onto the `dunce::canonicalize`d toplevel (non-verbatim),
    /// while analysis-pipeline paths inherit the shape of `opts.root` which
    /// tools / test fixtures often pre-canonicalise with `std::fs::canonicalize`
    /// (verbatim). The byte-level lookup against `FxHashSet<PathBuf>` then
    /// silently dropped every clone group. Pin both sides through a synthetic
    /// verbatim path on one side and a plain path on the other.
    #[cfg(windows)]
    #[test]
    fn filter_duplication_normalises_verbatim_prefix_mismatch() {
        let mut report = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![
                    CloneInstance {
                        file: PathBuf::from(r"\\?\C:\repo\src\changed.ts"),
                        start_line: 1,
                        end_line: 5,
                        start_col: 0,
                        end_col: 10,
                        fragment: "code".into(),
                    },
                    CloneInstance {
                        file: PathBuf::from(r"\\?\C:\repo\src\focused-copy.ts"),
                        start_line: 1,
                        end_line: 5,
                        start_col: 0,
                        end_col: 10,
                        fragment: "code".into(),
                    },
                ],
                token_count: 20,
                line_count: 5,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 2,
                files_with_clones: 2,
                total_lines: 100,
                duplicated_lines: 10,
                total_tokens: 200,
                duplicated_tokens: 40,
                clone_groups: 1,
                clone_instances: 2,
                duplication_percentage: 10.0,
                clone_groups_below_min_occurrences: 0,
            },
        };

        let mut changed: FxHashSet<PathBuf> = FxHashSet::default();
        changed.insert(PathBuf::from(r"C:\repo\src\changed.ts"));

        filter_duplication_by_changed_files(&mut report, &changed, Path::new(""));
        assert_eq!(
            report.clone_groups.len(),
            1,
            "verbatim instance path must match non-verbatim changed-file entry"
        );
    }

    #[cfg(windows)]
    #[test]
    fn filter_results_normalises_verbatim_prefix_mismatch() {
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from(r"\\?\C:\repo\src\a.ts"),
                export_name: "foo".into(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));

        let mut changed: FxHashSet<PathBuf> = FxHashSet::default();
        changed.insert(PathBuf::from(r"C:\repo\src\a.ts"));

        filter_results_by_changed_files(&mut results, &changed);
        assert_eq!(
            results.unused_exports.len(),
            1,
            "verbatim finding path must match non-verbatim changed-file entry"
        );
    }

    /// Initialize a temp git repo with a single committed file plus a tag
    /// at HEAD. Returns the canonical repo root.
    ///
    /// Uses `dunce::canonicalize` rather than `std::fs::canonicalize` so the
    /// returned path agrees with what `resolve_git_toplevel` produces in
    /// production (PR #566 swapped that helper to `dunce::canonicalize` to
    /// strip the Windows `\\?\` verbatim prefix). `std::fs::canonicalize`
    /// still produces verbatim on Windows, so the prior shape diverged from
    /// the production helper and downstream `changed.contains(&expected)`
    /// assertions silently failed because one side was verbatim and the
    /// other was not. POSIX behaviour is identical to `std::fs::canonicalize`.
    fn init_repo(repo: &Path) -> PathBuf {
        run_git(repo, &["init", "--quiet", "--initial-branch=main"]);
        run_git(repo, &["config", "user.email", "test@example.com"]);
        run_git(repo, &["config", "user.name", "test"]);
        run_git(repo, &["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("seed.txt"), "seed\n").unwrap();
        run_git(repo, &["add", "seed.txt"]);
        run_git(repo, &["commit", "--quiet", "-m", "initial"]);
        run_git(repo, &["tag", "fallow-baseline"]);
        dunce::canonicalize(repo).unwrap()
    }

    fn run_git(cwd: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("git available");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Workspace at git root, an untracked file is included in the
    /// changed-files set with an absolute path joined from the repo root.
    #[test]
    fn try_get_changed_files_workspace_at_repo_root() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo(tmp.path());
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/new.ts"), "export const x = 1;\n").unwrap();

        let changed = try_get_changed_files(&repo, "fallow-baseline").unwrap();

        let expected = repo.join("src/new.ts");
        assert!(
            changed.contains(&expected),
            "changed set should contain {expected:?}; actual: {changed:?}"
        );
    }

    /// Regression test for #190. When the workspace is a subdirectory of
    /// the git repository, `git diff --name-only` emits paths relative to
    /// the repo root (e.g., `frontend/src/new.ts`). Without the
    /// rev-parse-based toplevel resolution the function joined those
    /// against the workspace root, producing bogus paths like
    /// `<repo>/frontend/frontend/src/new.ts` that never matched
    /// `analyze_project` output and silently dropped the filter.
    #[test]
    fn try_get_changed_files_workspace_in_subdirectory() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo(tmp.path());
        let frontend = repo.join("frontend");
        std::fs::create_dir_all(frontend.join("src")).unwrap();
        std::fs::write(frontend.join("src/new.ts"), "export const x = 1;\n").unwrap();

        let changed = try_get_changed_files(&frontend, "fallow-baseline").unwrap();

        let expected = repo.join("frontend/src/new.ts");
        assert!(
            changed.contains(&expected),
            "changed set should contain canonical {expected:?}; actual: {changed:?}"
        );
        let bogus = frontend.join("frontend/src/new.ts");
        assert!(
            !changed.contains(&bogus),
            "changed set must not contain double-frontend path {bogus:?}"
        );
    }

    /// A *committed* change in a sibling subdirectory (outside the
    /// workspace) appears in the changed-files set because `git diff`
    /// is repo-wide regardless of cwd. The downstream
    /// `filter_results_by_changed_files` retains it only if
    /// `analyze_project` saw it; for a workspace scoped to one subdir,
    /// the sibling file is not in the analysis paths and falls away at
    /// the result-merge boundary, not here. This test pins the contract:
    /// for committed changes, the set is repo-wide.
    ///
    /// Note: `git ls-files --others --exclude-standard` only lists
    /// untracked files in cwd's subtree, so untracked siblings are NOT
    /// in the set when invoked from a subdirectory. That's harmless for
    /// the LSP because `analyze_project` only walks files under the
    /// workspace root either way.
    #[test]
    fn try_get_changed_files_includes_committed_sibling_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo(tmp.path());
        let backend = repo.join("backend");
        std::fs::create_dir_all(&backend).unwrap();
        std::fs::write(backend.join("server.py"), "print('hi')\n").unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "--quiet", "-m", "add backend"]);

        let frontend = repo.join("frontend");
        std::fs::create_dir_all(&frontend).unwrap();

        let changed = try_get_changed_files(&frontend, "fallow-baseline").unwrap();

        let expected = repo.join("backend/server.py");
        assert!(
            changed.contains(&expected),
            "committed sibling backend/server.py should be in the set: {changed:?}"
        );
    }

    /// Modifying a tracked file shows up via `git diff --name-only HEAD`,
    /// not just via `ls-files --others`. Confirm the path-join fix
    /// applies to that codepath too.
    #[test]
    fn try_get_changed_files_includes_modified_tracked_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo(tmp.path());
        let frontend = repo.join("frontend");
        std::fs::create_dir_all(frontend.join("src")).unwrap();
        std::fs::write(frontend.join("src/old.ts"), "export const x = 1;\n").unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "--quiet", "-m", "add old"]);
        run_git(&repo, &["tag", "fallow-baseline-v2"]);
        std::fs::write(frontend.join("src/old.ts"), "export const x = 2;\n").unwrap();

        let changed = try_get_changed_files(&frontend, "fallow-baseline-v2").unwrap();

        let expected = repo.join("frontend/src/old.ts");
        assert!(
            changed.contains(&expected),
            "modified tracked file {expected:?} missing from set: {changed:?}"
        );
    }

    /// `resolve_git_toplevel` returns the canonical repo path even when
    /// invoked from inside a subdirectory and via a symlinked input path.
    /// On macOS this guards against the `/tmp` -> `/private/tmp`
    /// canonicalization gap that would otherwise make the LSP filter set
    /// disagree with `analyze_project` paths.
    #[test]
    fn resolve_git_toplevel_returns_canonical_path() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo(tmp.path());
        let frontend = repo.join("frontend");
        std::fs::create_dir_all(&frontend).unwrap();

        let toplevel = resolve_git_toplevel(&frontend).unwrap();
        assert_eq!(toplevel, repo, "toplevel should equal canonical repo root");
        assert_eq!(
            toplevel,
            dunce::canonicalize(&toplevel).unwrap(),
            "resolved toplevel should already be canonical"
        );
    }

    /// Outside any git repo, `resolve_git_toplevel` returns
    /// `NotARepository` rather than panicking or returning a wrong path.
    /// The LSP relies on this to fall back to the workspace root cleanly.
    #[test]
    fn resolve_git_toplevel_not_a_repository() {
        let tmp = tempfile::tempdir().unwrap();
        let result = resolve_git_toplevel(tmp.path());
        assert!(
            matches!(result, Err(ChangedFilesError::NotARepository)),
            "expected NotARepository, got {result:?}"
        );
    }

    /// Two linked worktrees of the same repo resolve to the SAME common dir
    /// (the shared `.git`), even though their `--show-toplevel` working
    /// directories differ. This is the invariant the Impact store relies on to
    /// collapse all worktrees of a repo onto one history.
    #[test]
    fn resolve_git_common_dir_collapses_worktrees() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo(tmp.path());
        let linked = tmp.path().join("linked-worktree");
        run_git(
            &repo,
            &[
                "worktree",
                "add",
                "--quiet",
                linked.to_str().unwrap(),
                "-b",
                "feat",
            ],
        );

        let main_common = resolve_git_common_dir(&repo).unwrap();
        let linked_common = resolve_git_common_dir(&linked).unwrap();
        assert_eq!(
            main_common, linked_common,
            "worktrees of one repo must share a common dir"
        );

        // The per-worktree toplevels DO differ, proving the collapse is real.
        let main_top = resolve_git_toplevel(&repo).unwrap();
        let linked_top = resolve_git_toplevel(&linked).unwrap();
        assert_ne!(
            main_top, linked_top,
            "the two worktrees should have distinct toplevels"
        );
    }

    /// Outside any git repo, `resolve_git_common_dir` returns `NotARepository`
    /// so the Impact key can fall back to the canonical root.
    #[test]
    fn resolve_git_common_dir_not_a_repository() {
        let tmp = tempfile::tempdir().unwrap();
        let result = resolve_git_common_dir(tmp.path());
        assert!(
            matches!(result, Err(ChangedFilesError::NotARepository)),
            "expected NotARepository, got {result:?}"
        );
    }

    /// `try_get_changed_files` propagates the not-a-repo error so the
    /// LSP can warn and fall back to full-scope results.
    #[test]
    fn try_get_changed_files_not_a_repository() {
        let tmp = tempfile::tempdir().unwrap();
        let result = try_get_changed_files(tmp.path(), "main");
        assert!(matches!(result, Err(ChangedFilesError::NotARepository)));
    }

    #[test]
    fn filter_duplication_drops_groups_with_no_changed_instance() {
        let mut report = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![CloneInstance {
                    file: "/a.ts".into(),
                    start_line: 1,
                    end_line: 5,
                    start_col: 0,
                    end_col: 10,
                    fragment: "code".into(),
                }],
                token_count: 20,
                line_count: 5,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 1,
                files_with_clones: 1,
                total_lines: 100,
                duplicated_lines: 5,
                total_tokens: 100,
                duplicated_tokens: 20,
                clone_groups: 1,
                clone_instances: 1,
                duplication_percentage: 5.0,
                clone_groups_below_min_occurrences: 0,
            },
        };

        let changed: FxHashSet<PathBuf> = FxHashSet::default();
        filter_duplication_by_changed_files(&mut report, &changed, Path::new(""));
        assert!(report.clone_groups.is_empty());
        assert_eq!(report.stats.clone_groups, 0);
        assert_eq!(report.stats.clone_instances, 0);
        assert!((report.stats.duplication_percentage - 0.0).abs() < f64::EPSILON);
    }
}
