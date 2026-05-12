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

use rustc_hash::{FxHashMap, FxHashSet};

use crate::duplicates::{DuplicationReport, DuplicationStats, families};
use crate::results::AnalysisResults;

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
    let output = git_command(cwd, &["rev-parse", "--show-toplevel"])
        .output()
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
    Ok(path.canonicalize().unwrap_or(path))
}

fn collect_git_paths(
    cwd: &Path,
    toplevel: &Path,
    args: &[&str],
) -> Result<FxHashSet<PathBuf>, ChangedFilesError> {
    let output = git_command(cwd, args)
        .output()
        .map_err(|e| ChangedFilesError::GitMissing(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(if stderr.contains("not a git repository") {
            ChangedFilesError::NotARepository
        } else {
            ChangedFilesError::GitFailed(stderr.trim().to_owned())
        });
    }

    // All callers use modes whose output is repository-root-relative
    // (`git diff --name-only`, `git ls-files --full-name --others`). Joining
    // against `toplevel` yields absolute paths that line up with what
    // `analyze_project` emits when given a canonical workspace root, even if
    // the LSP / CLI was invoked from a subdirectory.
    let files: FxHashSet<PathBuf> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| toplevel.join(line))
        .collect();

    Ok(files)
}

fn git_command(cwd: &Path, args: &[&str]) -> std::process::Command {
    let mut command = std::process::Command::new("git");
    command.args(args).current_dir(cwd);
    crate::git_env::clear_ambient_git_env(&mut command);
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
    // Validate the ref BEFORE resolving the toplevel so the security-relevant
    // boundary check (rejects refs starting with `-`, etc.) runs even when
    // `cwd` happens to not be a git repo. Otherwise an attacker-controlled
    // `--changed-since=--upload-pack=evil` would leak through to
    // `git rev-parse` instead of being rejected at validation.
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
    // `--full-name` forces `ls-files` to emit repository-root-relative paths,
    // matching `git diff`'s default. Without it, `ls-files` emits paths
    // relative to cwd, which silently produces wrong joins when the caller
    // invokes from a subdirectory.
    files.extend(collect_git_paths(
        cwd,
        toplevel,
        &["ls-files", "--full-name", "--others", "--exclude-standard"],
    )?);
    Ok(files)
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
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across the workspace"
)]
pub fn filter_results_by_changed_files(
    results: &mut AnalysisResults,
    changed_files: &FxHashSet<PathBuf>,
) {
    results
        .unused_files
        .retain(|f| changed_files.contains(&f.path));
    results
        .unused_exports
        .retain(|e| changed_files.contains(&e.path));
    results
        .unused_types
        .retain(|e| changed_files.contains(&e.path));
    results
        .private_type_leaks
        .retain(|e| changed_files.contains(&e.path));
    results
        .unused_enum_members
        .retain(|m| changed_files.contains(&m.path));
    results
        .unused_class_members
        .retain(|m| changed_files.contains(&m.path));
    results
        .unresolved_imports
        .retain(|i| changed_files.contains(&i.path));

    // Unlisted deps: keep only if any importing file is changed
    results.unlisted_dependencies.retain(|d| {
        d.imported_from
            .iter()
            .any(|s| changed_files.contains(&s.path))
    });

    // Duplicate exports: filter locations to changed files, drop groups with < 2
    for dup in &mut results.duplicate_exports {
        dup.locations
            .retain(|loc| changed_files.contains(&loc.path));
    }
    results.duplicate_exports.retain(|d| d.locations.len() >= 2);

    // Circular deps: keep cycles where at least one file is changed
    results
        .circular_dependencies
        .retain(|c| c.files.iter().any(|f| changed_files.contains(f)));

    // Boundary violations: keep if the importing file changed
    results
        .boundary_violations
        .retain(|v| changed_files.contains(&v.from_path));

    // Stale suppressions: keep if the file changed
    results
        .stale_suppressions
        .retain(|s| changed_files.contains(&s.path));

    // Unresolved catalog references: anchored at the consumer package.json,
    // so keep only findings whose path is in the changed set.
    results
        .unresolved_catalog_references
        .retain(|r| changed_files.contains(&r.path));
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
    report
        .clone_groups
        .retain(|g| g.instances.iter().any(|i| changed_files.contains(&i.file)));
    report.clone_families = families::group_into_families(&report.clone_groups, root);
    report.mirrored_directories =
        families::detect_mirrored_directories(&report.clone_families, root);
    report.stats = recompute_duplication_stats(report);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::duplicates::{CloneGroup, CloneInstance};
    use crate::results::{BoundaryViolation, CircularDependency, UnusedExport, UnusedFile};

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
        // Errors that aren't shallow-clone-related stay verbatim
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
    fn try_get_changed_files_rejects_invalid_ref() {
        // Validation runs before git invocation, so any path will do
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
        results.unused_files.push(UnusedFile {
            path: "/a.ts".into(),
        });
        results.unused_files.push(UnusedFile {
            path: "/b.ts".into(),
        });
        results.unused_exports.push(UnusedExport {
            path: "/a.ts".into(),
            export_name: "foo".into(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });

        let mut changed: FxHashSet<PathBuf> = FxHashSet::default();
        changed.insert("/a.ts".into());

        filter_results_by_changed_files(&mut results, &changed);

        assert_eq!(results.unused_files.len(), 1);
        assert_eq!(results.unused_files[0].path, PathBuf::from("/a.ts"));
        assert_eq!(results.unused_exports.len(), 1);
    }

    #[test]
    fn filter_results_preserves_dependency_level_issues() {
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(crate::results::UnusedDependency {
                package_name: "lodash".into(),
                location: crate::results::DependencyLocation::Dependencies,
                path: "/pkg.json".into(),
                line: 3,
                used_in_workspaces: Vec::new(),
            });

        let changed: FxHashSet<PathBuf> = FxHashSet::default();
        filter_results_by_changed_files(&mut results, &changed);

        // Dependency-level issues survive even when no source files changed
        assert_eq!(results.unused_dependencies.len(), 1);
    }

    #[test]
    fn filter_results_keeps_circular_dep_when_any_file_changed() {
        let mut results = AnalysisResults::default();
        results.circular_dependencies.push(CircularDependency {
            files: vec!["/a.ts".into(), "/b.ts".into()],
            length: 2,
            line: 1,
            col: 0,
            is_cross_package: false,
        });

        let mut changed: FxHashSet<PathBuf> = FxHashSet::default();
        changed.insert("/b.ts".into());

        filter_results_by_changed_files(&mut results, &changed);
        assert_eq!(results.circular_dependencies.len(), 1);
    }

    #[test]
    fn filter_results_drops_circular_dep_when_no_file_changed() {
        let mut results = AnalysisResults::default();
        results.circular_dependencies.push(CircularDependency {
            files: vec!["/a.ts".into(), "/b.ts".into()],
            length: 2,
            line: 1,
            col: 0,
            is_cross_package: false,
        });

        let changed: FxHashSet<PathBuf> = FxHashSet::default();
        filter_results_by_changed_files(&mut results, &changed);
        assert!(results.circular_dependencies.is_empty());
    }

    #[test]
    fn filter_results_drops_boundary_violation_when_importer_unchanged() {
        let mut results = AnalysisResults::default();
        results.boundary_violations.push(BoundaryViolation {
            from_path: "/a.ts".into(),
            to_path: "/b.ts".into(),
            from_zone: "ui".into(),
            to_zone: "data".into(),
            import_specifier: "../data/db".into(),
            line: 1,
            col: 0,
        });

        let mut changed: FxHashSet<PathBuf> = FxHashSet::default();
        // only the imported file changed, not the importer
        changed.insert("/b.ts".into());

        filter_results_by_changed_files(&mut results, &changed);
        assert!(results.boundary_violations.is_empty());
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
            },
        };

        let mut changed: FxHashSet<PathBuf> = FxHashSet::default();
        changed.insert("/a.ts".into());

        filter_duplication_by_changed_files(&mut report, &changed, Path::new(""));
        assert_eq!(report.clone_groups.len(), 1);
        // stats recomputed from surviving groups
        assert_eq!(report.stats.clone_groups, 1);
        assert_eq!(report.stats.clone_instances, 2);
    }

    // -----------------------------------------------------------------------
    // Real git interactions (tempdir + git init). These exercise the
    // path-resolution boundary between `git rev-parse --show-toplevel`,
    // `git diff --name-only`, and `git ls-files --full-name --others` to
    // catch regressions like issue #190 where the LSP workspace was a
    // subdirectory of the git repo and changed-file paths were joined
    // against the wrong base.
    // -----------------------------------------------------------------------

    /// Initialize a temp git repo with a single committed file plus a tag
    /// at HEAD. Returns the canonical repo root.
    fn init_repo(repo: &Path) -> PathBuf {
        run_git(repo, &["init", "--quiet", "--initial-branch=main"]);
        run_git(repo, &["config", "user.email", "test@example.com"]);
        run_git(repo, &["config", "user.name", "test"]);
        run_git(repo, &["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("seed.txt"), "seed\n").unwrap();
        run_git(repo, &["add", "seed.txt"]);
        run_git(repo, &["commit", "--quiet", "-m", "initial"]);
        run_git(repo, &["tag", "fallow-baseline"]);
        repo.canonicalize().unwrap()
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
        // Verify the bogus double-frontend path is NOT in the set
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
        // Modify the tracked file (no commit, so diff-HEAD picks it up)
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
            toplevel.canonicalize().unwrap(),
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
