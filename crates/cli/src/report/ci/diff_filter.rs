use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use rustc_hash::{FxHashMap, FxHashSet};

use super::pr_comment::CiIssue;

/// Refuse to parse a unified diff larger than this. The cap matches the
/// SARIF upload limit (10 MiB) and is also far above what any sane PR
/// produces. A pathologically large diff (binary blob, vendored dump) would
/// otherwise eat memory proportional to its size before we can inspect it.
pub const MAX_DIFF_BYTES: u64 = 10 * 1024 * 1024;

/// Stop indexing added lines past this count. A 1M-line "diff" is a sign of
/// a regenerated lockfile or vendored bundle and is not useful for filtering;
/// emit a warning and proceed with whatever we already indexed.
const MAX_ADDED_LINES: usize = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffFilterMode {
    Added,
    DiffContext,
    File,
    NoFilter,
}

impl DiffFilterMode {
    #[must_use]
    pub fn from_env() -> Self {
        match std::env::var("FALLOW_DIFF_FILTER")
            .unwrap_or_else(|_| "added".into())
            .as_str()
        {
            "diff_context" | "context" => Self::DiffContext,
            "file" => Self::File,
            "nofilter" | "none" => Self::NoFilter,
            _ => Self::Added,
        }
    }
}

#[derive(Debug, Default)]
pub struct DiffIndex {
    added_lines: FxHashMap<String, FxHashSet<u64>>,
    touched_files: FxHashSet<String>,
    added_line_count: usize,
    /// `head_path -> base_path` pairs for renames in the diff. Populated by
    /// parsing `rename from <old>` and `rename to <new>` extended-header
    /// lines that git diff emits between the `diff --git` line and the
    /// `--- a/...` / `+++ b/...` body. Consumed by
    /// `crates/cli/src/report/ci/review.rs` to populate GitLab's
    /// `position.old_path` with the base-side filename, which the GitLab
    /// API requires when anchoring an inline comment on a renamed file.
    rename_pairs: FxHashMap<String, String>,
}

impl DiffIndex {
    #[must_use]
    pub fn from_unified_diff(diff: &str) -> Self {
        let mut index = Self::default();
        let mut current_file: Option<String> = None;
        let mut new_line = 0_u64;
        let mut warned_overflow = false;
        // `rename from <old>` always precedes `rename to <new>` in git's
        // extended-header block. Track the most-recent `from` so the
        // subsequent `to` can pair them. Reset on every new `diff --git`
        // header so an unpaired `from` in one block does not bleed into
        // the next file's pair.
        let mut pending_rename_from: Option<String> = None;

        for line in diff.lines() {
            if line.starts_with("diff --git ") {
                pending_rename_from = None;
                continue;
            }
            if let Some(rest) = line.strip_prefix("rename from ") {
                pending_rename_from = Some(rest.to_owned());
                continue;
            }
            if let Some(rest) = line.strip_prefix("rename to ") {
                if let Some(from) = pending_rename_from.take() {
                    index.rename_pairs.insert(rest.to_owned(), from);
                    // A pure-rename diff (similarity 100%) has no `+++ b/`
                    // line, so the touched-files set never records the new
                    // path otherwise. Seed it here for the filter-side
                    // `touches_file` check.
                    index.touched_files.insert(rest.to_owned());
                }
                continue;
            }
            if let Some(path) = line.strip_prefix("+++ b/") {
                current_file = Some(path.to_string());
                index.touched_files.insert(path.to_string());
                continue;
            }
            if line.starts_with("+++ /dev/null") {
                current_file = None;
                continue;
            }
            if let Some(header) = line.strip_prefix("@@ ") {
                if let Some(start) = parse_new_hunk_start(header) {
                    new_line = start;
                }
                continue;
            }
            let Some(path) = current_file.as_ref() else {
                continue;
            };
            if line.starts_with('+') && !line.starts_with("+++") {
                if index.added_line_count < MAX_ADDED_LINES {
                    index
                        .added_lines
                        .entry(path.clone())
                        .or_default()
                        .insert(new_line);
                    index.added_line_count += 1;
                } else if !warned_overflow {
                    eprintln!(
                        "fallow: diff exceeds {MAX_ADDED_LINES} added lines; \
                         indexed prefix only, later additions skipped"
                    );
                    warned_overflow = true;
                }
                new_line += 1;
            } else if !line.starts_with('-') {
                new_line += 1;
            }
        }

        index
    }

    /// Base-side path for a renamed file whose head-side path is
    /// `head_path`. Returns `None` when `head_path` is not part of any
    /// rename pair in the indexed diff (the common case: edits, additions,
    /// deletions). Consumed by the review envelope formatter at
    /// `crates/cli/src/report/ci/review.rs::render_comment` to populate the
    /// GitLab `position.old_path` field for renamed files; without this the
    /// inline comment fails to anchor in GitLab's position API.
    #[must_use]
    pub fn old_path_for(&self, head_path: &str) -> Option<&str> {
        self.rename_pairs.get(head_path).map(String::as_str)
    }

    /// Count of `+` lines indexed across every file. Used by the
    /// finding-level filter to warn when an opt-in `--diff-file` produced
    /// an empty index (typo, wrong path, pure-rename diff with no body),
    /// since `from_unified_diff` is intentionally infallible and would
    /// otherwise silently drop every finding.
    #[must_use]
    pub fn added_line_count(&self) -> usize {
        self.added_line_count
    }

    /// True when `path` (repo-root-relative, forward-slashed) appears as a
    /// `+++ b/<path>` header in the diff. Used by the finding-level filter
    /// to short-circuit before line-range checks: a file not in the diff
    /// has no overlapping ranges by definition.
    #[must_use]
    pub fn touches_file(&self, path: &str) -> bool {
        self.touched_files.contains(path)
    }

    /// True when the closed line range `[start..=end]` for `path` overlaps
    /// any added line indexed from the diff. `start == 0` collapses to
    /// `1..=end` so callers can pass an unsigned `line + line_count` shape
    /// without a special case for zero-length ranges. When `end < start`,
    /// returns `false` (degenerate range).
    ///
    /// Range semantics: a complexity hotspot at `[10..=120]` with a PR that
    /// touches line 115 returns `true` (115 is in the closed range and is
    /// also an added line). A clone instance at `[200..=210]` with the PR
    /// touching only line 50 returns `false`.
    #[must_use]
    pub fn range_overlaps_added(&self, path: &str, start: u64, end: u64) -> bool {
        if end < start {
            return false;
        }
        let Some(added) = self.added_lines.get(path) else {
            return false;
        };
        let lo = start.max(1);
        added.iter().any(|&line| line >= lo && line <= end)
    }

    #[cfg(test)]
    #[must_use]
    pub fn keeps(&self, issue: &CiIssue, mode: DiffFilterMode) -> bool {
        self.keeps_with_context(issue, mode, context_radius_from_env())
    }

    #[must_use]
    pub fn keeps_with_context(&self, issue: &CiIssue, mode: DiffFilterMode, radius: u64) -> bool {
        match mode {
            DiffFilterMode::NoFilter => true,
            DiffFilterMode::File => self.touched_files.contains(&issue.path),
            DiffFilterMode::DiffContext => self.added_lines.get(&issue.path).is_some_and(|lines| {
                lines
                    .iter()
                    .any(|line| issue.line.abs_diff(*line) <= radius)
            }),
            DiffFilterMode::Added => self
                .added_lines
                .get(&issue.path)
                .is_some_and(|lines| lines.contains(&issue.line)),
        }
    }

    /// Added-line numbers for `path` (repo-root-relative, forward-slashed),
    /// or `None` when the file does not appear in the diff. Used by the
    /// runtime-coverage filter to do line-overlap matching against hot-path
    /// `[start_line, end_line]` ranges, so a PR touching the body of a hot
    /// function flips the verdict to `hot-path-touched` while edits to
    /// other functions in the same file do not.
    #[must_use]
    pub fn added_lines_in(&self, path: &str) -> Option<&FxHashSet<u64>> {
        self.added_lines.get(path)
    }
}

/// Reduce `path` to a forward-slashed string suitable for matching against
/// a unified diff's `+++ b/<path>` keys. Strips the project root prefix
/// when `path` is absolute. Returns `None` when `path` lives outside
/// `root` (different drive, traversal escape) so the caller can keep the
/// finding rather than silently drop it on an unfilterable path.
///
/// Windows checkouts emit backslash-separated paths from `to_string_lossy`;
/// the replace normalizes them so they compare equal to the forward-slash
/// keys `git diff` writes.
#[must_use]
pub fn relative_to_diff_path(path: &Path, root: &Path) -> Option<String> {
    let relative = if path.is_absolute() {
        path.strip_prefix(root).ok()?
    } else {
        path
    };
    Some(relative.to_string_lossy().replace('\\', "/"))
}

/// How a diff source was located. Tracked separately from the parsed
/// `DiffIndex` so callers can compose precedence + empty-parse warnings
/// that name the source the user actually supplied.
#[derive(Debug, Clone)]
pub enum DiffSource {
    /// `--diff-file <path>` (absolute after root-join).
    Flag(PathBuf),
    /// `--diff-stdin` or `--diff-file -`. Stdin is consumed exactly once;
    /// repeated calls to [`resolve_diff_source`] would observe EOF.
    Stdin,
    /// `$FALLOW_DIFF_FILE` (absolute after root-join). The env-var path is
    /// the load-bearing breadcrumb for the GitHub Action and the GitLab CI
    /// template, both of which set the var before invoking fallow.
    EnvVar(PathBuf),
}

impl DiffSource {
    /// Short, user-facing label for warning messages.
    #[must_use]
    fn label(&self) -> String {
        match self {
            Self::Flag(p) => format!("--diff-file {}", p.display()),
            Self::Stdin => "--diff-stdin".to_owned(),
            Self::EnvVar(p) => format!("$FALLOW_DIFF_FILE {}", p.display()),
        }
    }
}

/// Result of [`load_diff_index_for_findings`]. Carries the parsed
/// `DiffIndex`; the source breadcrumb is consumed by the function during
/// load to compose warning messages and is not retained beyond that.
#[derive(Debug)]
pub struct LoadedDiff {
    pub index: DiffIndex,
}

/// Resolve a diff source from CLI input.
///
/// Precedence (highest first):
///   1. `--diff-stdin` -> stdin
///   2. `--diff-file -` -> stdin
///   3. `--diff-file <path>` -> path (root-joined if relative)
///   4. `$FALLOW_DIFF_FILE` -> path (root-joined if relative)
///   5. None set -> returns `Ok(None)`
///
/// Returns `Err` only on a configuration conflict (e.g. `--diff-stdin`
/// combined with an explicit path), so callers can surface the precise
/// reason to the user via [`crate::error::emit_error`].
///
/// # Errors
///
/// Returns a human-readable message when the CLI input is internally
/// inconsistent (e.g. `--diff-stdin` and `--diff-file pr.diff` both set,
/// or `--diff-file ""` after env-var fallback failed).
pub fn resolve_diff_source(
    diff_file: Option<&Path>,
    diff_stdin: bool,
    root: &Path,
) -> Result<Option<DiffSource>, String> {
    let path_is_stdin_sentinel = diff_file.is_some_and(|p| p == Path::new("-"));

    if diff_stdin
        && let Some(path) = diff_file
        && !path_is_stdin_sentinel
    {
        return Err(format!(
            "--diff-stdin and --diff-file {} are mutually exclusive. \
             Pick one: --diff-stdin to pipe via stdin, --diff-file PATH \
             to point at a file on disk.",
            path.display()
        ));
    }

    if diff_stdin || path_is_stdin_sentinel {
        return Ok(Some(DiffSource::Stdin));
    }

    if let Some(path) = diff_file {
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.join(path)
        };
        return Ok(Some(DiffSource::Flag(abs)));
    }

    if let Some(env) = std::env::var_os("FALLOW_DIFF_FILE")
        && !env.is_empty()
    {
        let raw = PathBuf::from(env);
        let abs = if raw.is_absolute() {
            raw
        } else {
            root.join(raw)
        };
        return Ok(Some(DiffSource::EnvVar(abs)));
    }

    Ok(None)
}

/// Read + parse the resolved diff source into a `DiffIndex` for
/// finding-level filtering. Failure modes (file missing, oversize,
/// unreadable, empty index) emit a `fallow: warning [diff-file]` line on
/// stderr unless `quiet` is set, and return `None` so the analysis runs
/// at full scope rather than failing for a CI-script issue.
///
/// Stdin is consumed exactly once. The first call drains it; downstream
/// callers must reuse the returned `LoadedDiff` rather than re-loading.
#[must_use]
pub fn load_diff_index_for_findings(source: &DiffSource, quiet: bool) -> Option<LoadedDiff> {
    match source {
        DiffSource::Stdin => {
            let mut buf = String::new();
            match std::io::stdin().read_to_string(&mut buf) {
                Ok(_) => {
                    let index = DiffIndex::from_unified_diff(&buf);
                    if !quiet && index.added_line_count() == 0 {
                        eprintln!(
                            "fallow: warning [diff-file]: --diff-stdin parsed \
                             0 added lines; no findings will pass the diff filter. \
                             Did you pipe a non-unified diff or an empty stream? \
                             (Pure-rename, binary-only, and deletion-only diffs \
                             also produce empty indices.)"
                        );
                    }
                    Some(LoadedDiff { index })
                }
                Err(err) => {
                    if !quiet {
                        eprintln!(
                            "fallow: warning [diff-file]: could not read stdin: {err} \
                             (line-level filtering disabled; rerun with \
                             --diff-file PATH to point at a file on disk)"
                        );
                    }
                    None
                }
            }
        }
        DiffSource::Flag(path) | DiffSource::EnvVar(path) => {
            let label = source.label();
            if let Ok(meta) = std::fs::metadata(path)
                && meta.len() > MAX_DIFF_BYTES
            {
                if !quiet {
                    eprintln!(
                        "fallow: warning [diff-file]: {label} is {} bytes (cap {MAX_DIFF_BYTES}); \
                         line-level filtering disabled, reporting all findings",
                        meta.len()
                    );
                }
                return None;
            }
            match std::fs::read_to_string(path) {
                Ok(text) => {
                    let index = DiffIndex::from_unified_diff(&text);
                    if !quiet && index.added_line_count() == 0 {
                        eprintln!(
                            "fallow: warning [diff-file]: {label} parsed 0 added \
                             lines; no findings will pass the diff filter. \
                             Verify the file is a unified diff (look for \
                             `+++ b/<path>` headers). Pure-rename, binary-only, \
                             and deletion-only diffs also produce empty indices."
                        );
                    }
                    Some(LoadedDiff { index })
                }
                Err(err) => {
                    if !quiet {
                        eprintln!(
                            "fallow: warning [diff-file]: could not read {label}: {err} \
                             (line-level filtering disabled)"
                        );
                    }
                    None
                }
            }
        }
    }
}

/// Process-wide cache for the diff index resolved at startup, so combined
/// runs do not re-read stdin (impossible) or re-parse the same file three
/// times across `check`, `dupes`, and `health`.
///
/// Populated once by `main()` via [`init_shared_diff`] after CLI parsing;
/// every subsystem queries it via [`shared_diff_index`] at filter time.
///
/// Programmatic callers (Node bindings, in-process embedders) that never
/// call `init_shared_diff` see `None` here, which means no line-level
/// filter applies: the diff filter is strictly opt-in.
static SHARED_DIFF: OnceLock<Option<LoadedDiff>> = OnceLock::new();

/// Resolve, read, and parse the diff source once for the lifetime of the
/// process. Idempotent: only the first call populates the cache; later
/// calls observe the original value. Returns the resolved index for the
/// caller to inspect (e.g. to log "0 hunks" or to skip a filtering step
/// when nothing was loaded).
///
/// Pass `None` to lock the cache to "no diff" without reading anything,
/// so a subsequent errant load attempt cannot accidentally populate the
/// cache later.
pub fn init_shared_diff(source: Option<&DiffSource>, quiet: bool) -> Option<&'static DiffIndex> {
    let loaded = source.and_then(|src| load_diff_index_for_findings(src, quiet));
    let _ = SHARED_DIFF.set(loaded);
    shared_diff_index()
}

/// Read the cached diff index populated by [`init_shared_diff`]. Returns
/// `None` when the cache is empty (no diff was supplied, or
/// `init_shared_diff` was never called).
#[must_use]
pub fn shared_diff_index() -> Option<&'static DiffIndex> {
    SHARED_DIFF.get().and_then(|v| v.as_ref()).map(|l| &l.index)
}

fn context_radius_from_env() -> u64 {
    std::env::var("FALLOW_DIFF_CONTEXT")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(3)
}

fn parse_new_hunk_start(header: &str) -> Option<u64> {
    let plus = header.find('+')?;
    let rest = &header[plus + 1..];
    let end = rest
        .find(|c: char| c == ',' || c.is_ascii_whitespace())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

#[must_use]
pub fn filter_issues_from_env(issues: Vec<CiIssue>) -> Vec<CiIssue> {
    let Some(raw_path) = std::env::var_os("FALLOW_DIFF_FILE") else {
        return issues;
    };
    filter_issues_from_path(
        issues,
        Path::new(&raw_path),
        DiffFilterMode::from_env(),
        context_radius_from_env(),
    )
}

/// Filter for the typed PR-comment renderer (`print_pr_comment`).
///
/// Project-level rule findings (dependency / catalog / override hygiene that
/// lives in `package.json` / `pnpm-workspace.yaml`) bypass the diff filter
/// because the PR diff rarely touches the anchored line even though the
/// finding is the reason CI fails. Source-anchored findings still go through
/// the configured filter (`FALLOW_DIFF_FILTER`, default `added`) so the
/// comment stays focused on what the PR actually changed.
///
/// Sorting is restored after the partition + merge so downstream rendering
/// sees the same `(path, line, fingerprint)` order as the unfiltered input.
#[must_use]
pub fn filter_issues_for_summary(issues: Vec<CiIssue>) -> Vec<CiIssue> {
    summary_filter_with(issues, filter_issues_from_env)
}

/// Partition + delegate helper for `filter_issues_for_summary`. Generic over
/// the source-level filter so tests can call it with `filter_issues_from_path`
/// against a tempdir diff without poking at `FALLOW_DIFF_FILE`.
fn summary_filter_with<F>(issues: Vec<CiIssue>, source_filter: F) -> Vec<CiIssue>
where
    F: FnOnce(Vec<CiIssue>) -> Vec<CiIssue>,
{
    let (project_level, diff_relevant): (Vec<CiIssue>, Vec<CiIssue>) = issues
        .into_iter()
        .partition(|issue| super::pr_comment::is_project_level_rule(&issue.rule_id));
    let mut kept = source_filter(diff_relevant);
    kept.extend(project_level);
    kept.sort_by(|a, b| (&a.path, a.line, &a.fingerprint).cmp(&(&b.path, b.line, &b.fingerprint)));
    kept
}

#[must_use]
pub fn filter_issues_from_path(
    issues: Vec<CiIssue>,
    path: &Path,
    mode: DiffFilterMode,
    radius: u64,
) -> Vec<CiIssue> {
    // Reject diffs above the size cap before reading them into memory. A
    // pathological diff (vendored dump, binary blob mistakenly committed)
    // would otherwise allocate proportional memory before we can filter.
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() > MAX_DIFF_BYTES => {
            eprintln!(
                "fallow: FALLOW_DIFF_FILE '{}' is {} bytes (cap {MAX_DIFF_BYTES}); \
                 skipping diff filter, reporting all findings",
                path.display(),
                meta.len()
            );
            return issues;
        }
        Ok(_) => {}
        Err(err) => {
            eprintln!(
                "fallow: FALLOW_DIFF_FILE '{}' could not be stat'd ({err}); \
                 skipping diff filter, reporting all findings",
                path.display()
            );
            return issues;
        }
    }

    let Ok(diff) = std::fs::read_to_string(path) else {
        eprintln!(
            "fallow: FALLOW_DIFF_FILE '{}' could not be read; \
             skipping diff filter, reporting all findings",
            path.display()
        );
        return issues;
    };
    let index = DiffIndex::from_unified_diff(&diff);
    issues
        .into_iter()
        .filter(|issue| index.keeps_with_context(issue, mode, radius))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    #[test]
    fn from_unified_diff_caps_added_lines_at_threshold() {
        // Synthesize a diff with MAX_ADDED_LINES + 100 added lines and verify
        // we stop indexing past the cap. The exact split: index size <= cap.
        let header =
            "diff --git a/big.txt b/big.txt\n--- a/big.txt\n+++ b/big.txt\n@@ -0,0 +1,100 @@\n";
        let mut body = String::with_capacity(MAX_ADDED_LINES * 16);
        for _ in 0..(MAX_ADDED_LINES + 100) {
            body.push_str("+x\n");
        }
        let mut diff = String::with_capacity(header.len() + body.len());
        diff.push_str(header);
        diff.push_str(&body);

        let index = DiffIndex::from_unified_diff(&diff);
        let total: usize = index.added_lines.values().map(FxHashSet::len).sum();
        assert!(
            total <= MAX_ADDED_LINES,
            "indexed {total} lines, cap is {MAX_ADDED_LINES}"
        );
    }

    #[test]
    fn filter_issues_from_path_skips_oversize_diff() {
        // Write a diff just over the byte cap and verify the cap-check
        // short-circuits, returning issues unfiltered with a warning.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("oversize.diff");
        let mut file = std::fs::File::create(&path).expect("create");
        let chunk = "+ filler line\n";
        let bytes_per_chunk = chunk.len() as u64;
        let chunks_needed = (MAX_DIFF_BYTES / bytes_per_chunk) + 100_000;
        for _ in 0..chunks_needed {
            file.write_all(chunk.as_bytes()).expect("write");
        }
        drop(file);

        let issue = CiIssue {
            rule_id: "r".into(),
            description: "d".into(),
            severity: "minor".into(),
            path: "src/a.ts".into(),
            line: 1,
            fingerprint: "abc".into(),
        };
        let kept = filter_issues_from_path(vec![issue], &path, DiffFilterMode::Added, 3);
        assert_eq!(kept.len(), 1, "oversize diff must fall through unfiltered");
    }

    #[test]
    fn filter_issues_from_path_handles_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("does-not-exist.diff");
        let issue = CiIssue {
            rule_id: "r".into(),
            description: "d".into(),
            severity: "minor".into(),
            path: "src/a.ts".into(),
            line: 1,
            fingerprint: "abc".into(),
        };
        let kept = filter_issues_from_path(vec![issue], &path, DiffFilterMode::Added, 3);
        assert_eq!(kept.len(), 1, "missing diff must fall through unfiltered");
    }

    #[test]
    fn summary_filter_keeps_project_level_findings_when_diff_misses_them() {
        // The bug from #381: a `pnpm.overrides` entry in `package.json`
        // becomes unused (transitive dep no longer in the resolved tree),
        // but the PR diff doesn't touch the override line. The default
        // `Added` filter drops the finding from the summary even though CI
        // exits non-zero because of the same finding, leaving the user with
        // a comment body that says "No findings."
        //
        // `summary_filter_with` bypasses the filter for project-level rules
        // so the override finding stays in the body. The diff used here
        // doesn't even include `package.json` to make the bypass clear.
        let dir = tempfile::tempdir().expect("tempdir");
        let diff_path = dir.path().join("pr.diff");
        std::fs::write(
            &diff_path,
            "diff --git a/src/a.ts b/src/a.ts\n\
             --- a/src/a.ts\n\
             +++ b/src/a.ts\n\
             @@ -0,0 +1,1 @@\n\
             +new line\n",
        )
        .expect("write");

        let project_level = CiIssue {
            rule_id: "fallow/unused-dependency-override".into(),
            description: "Override stale".into(),
            severity: "minor".into(),
            path: "package.json".into(),
            line: 42,
            fingerprint: "override".into(),
        };
        let source_level_in_diff = CiIssue {
            rule_id: "fallow/unused-export".into(),
            description: "Export unused".into(),
            severity: "minor".into(),
            path: "src/a.ts".into(),
            line: 1,
            fingerprint: "in-diff".into(),
        };
        let source_level_outside_diff = CiIssue {
            rule_id: "fallow/unused-export".into(),
            description: "Export unused".into(),
            severity: "minor".into(),
            path: "src/b.ts".into(),
            line: 1,
            fingerprint: "out-diff".into(),
        };
        let kept = summary_filter_with(
            vec![
                project_level,
                source_level_in_diff,
                source_level_outside_diff,
            ],
            |src| filter_issues_from_path(src, &diff_path, DiffFilterMode::Added, 3),
        );
        let fingerprints: Vec<&str> = kept.iter().map(|i| i.fingerprint.as_str()).collect();
        assert!(
            fingerprints.contains(&"override"),
            "project-level finding must survive missing-diff: {fingerprints:?}"
        );
        assert!(
            fingerprints.contains(&"in-diff"),
            "source-level finding inside diff must be kept: {fingerprints:?}"
        );
        assert!(
            !fingerprints.contains(&"out-diff"),
            "source-level finding outside diff must be dropped: {fingerprints:?}"
        );
    }

    #[test]
    fn summary_filter_preserves_path_line_fingerprint_sort_order() {
        // The partition step shuffles project-level issues to the back of
        // the vec; the post-merge sort restores the canonical ordering the
        // renderer expects.
        let a = CiIssue {
            rule_id: "fallow/unused-export".into(),
            description: "a".into(),
            severity: "minor".into(),
            path: "src/a.ts".into(),
            line: 1,
            fingerprint: "a".into(),
        };
        let b = CiIssue {
            rule_id: "fallow/unused-dependency".into(),
            description: "b".into(),
            severity: "minor".into(),
            path: "package.json".into(),
            line: 5,
            fingerprint: "b".into(),
        };
        let kept = summary_filter_with(vec![a, b], |issues| issues);
        // `package.json` sorts before `src/a.ts` lexicographically.
        assert_eq!(kept[0].fingerprint, "b");
        assert_eq!(kept[1].fingerprint, "a");
    }

    #[test]
    fn range_overlaps_added_hotspot_starting_before_diff_touches_inside() {
        // Complexity hotspot [10..=120] with PR touching line 115 inside
        // the function body must count as touched. Edges (start, end) are
        // both inclusive: a PR at line 10 OR line 120 also counts.
        let diff = "\
diff --git a/src/big.ts b/src/big.ts
--- a/src/big.ts
+++ b/src/big.ts
@@ -114,1 +114,2 @@
 ctx
+touched
";
        let index = DiffIndex::from_unified_diff(diff);
        assert!(index.range_overlaps_added("src/big.ts", 10, 120));
        // Different file: never overlaps even at matching lines.
        assert!(!index.range_overlaps_added("src/other.ts", 10, 120));
        // Range strictly before the touched line.
        assert!(!index.range_overlaps_added("src/big.ts", 10, 100));
        // Degenerate range (end < start) is never overlap.
        assert!(!index.range_overlaps_added("src/big.ts", 200, 100));
    }

    #[test]
    fn range_overlaps_added_handles_single_line_range_at_added_line() {
        let diff = "\
diff --git a/src/a.ts b/src/a.ts
--- a/src/a.ts
+++ b/src/a.ts
@@ -1,1 +1,2 @@
 ctx
+new
";
        let index = DiffIndex::from_unified_diff(diff);
        // ComplexityViolation with line=2, line_count=1 -> [2..=2].
        assert!(index.range_overlaps_added("src/a.ts", 2, 2));
    }

    #[test]
    fn range_overlaps_added_range_starting_at_zero_collapses_to_one() {
        // Callers passing `line + line_count` with line=0 must not match
        // every diff. The implementation lifts `start` to max(start, 1)
        // so a 1-based diff key never matches a 0-based caller range.
        let diff = "\
diff --git a/src/a.ts b/src/a.ts
--- a/src/a.ts
+++ b/src/a.ts
@@ -1,1 +1,2 @@
 ctx
+new
";
        let index = DiffIndex::from_unified_diff(diff);
        // Range [0..=0]: lifted to [1..=0], which is empty.
        assert!(!index.range_overlaps_added("src/a.ts", 0, 0));
        // Range [0..=5]: lifted to [1..=5]; line 2 is in the set.
        assert!(index.range_overlaps_added("src/a.ts", 0, 5));
    }

    #[test]
    fn added_line_count_tracks_total_across_files() {
        let diff = "\
diff --git a/a b/a
--- a/a
+++ b/a
@@ -1,0 +1,2 @@
+one
+two
diff --git a/b b/b
--- a/b
+++ b/b
@@ -1,0 +1,1 @@
+three
";
        let index = DiffIndex::from_unified_diff(diff);
        assert_eq!(index.added_line_count(), 3);
        assert!(index.touches_file("a"));
        assert!(index.touches_file("b"));
        assert!(!index.touches_file("c"));
    }

    #[test]
    fn empty_diff_has_zero_added_lines_and_no_touched_files() {
        let index = DiffIndex::from_unified_diff("");
        assert_eq!(index.added_line_count(), 0);
        assert!(!index.touches_file("any/path"));
    }

    #[test]
    fn delete_only_diff_records_no_added_lines() {
        // A pure deletion: `+++ /dev/null` keeps current_file = None so
        // no added lines accumulate. The path NOT touched on the right side
        // is genuinely absent; range_overlaps_added must return false.
        let diff = "\
diff --git a/dead.ts b/dead.ts
deleted file mode 100644
--- a/dead.ts
+++ /dev/null
@@ -1,3 +0,0 @@
-one
-two
-three
";
        let index = DiffIndex::from_unified_diff(diff);
        assert_eq!(index.added_line_count(), 0);
        assert!(!index.touches_file("dead.ts"));
        assert!(!index.range_overlaps_added("dead.ts", 1, 3));
    }

    #[test]
    fn rename_with_content_hunk_indexes_under_new_path() {
        // Renames carry the new name in the `+++ b/...` header. The added
        // line for the new content sits under the new path; callers that
        // emit findings keyed to the OLD path will miss the overlap, which
        // is the expected behavior (the old file no longer exists).
        let diff = "\
diff --git a/src/old.ts b/src/new.ts
similarity index 90%
rename from src/old.ts
rename to src/new.ts
--- a/src/old.ts
+++ b/src/new.ts
@@ -1,2 +1,3 @@
 keep
+added on rename
 still
";
        let index = DiffIndex::from_unified_diff(diff);
        assert!(index.touches_file("src/new.ts"));
        assert!(!index.touches_file("src/old.ts"));
        assert!(index.range_overlaps_added("src/new.ts", 1, 5));
        assert!(!index.range_overlaps_added("src/old.ts", 1, 5));
        // Issue #528: rename pair must be recorded so the review envelope
        // can populate GitLab's position.old_path with the base-side path.
        assert_eq!(index.old_path_for("src/new.ts"), Some("src/old.ts"));
        assert_eq!(index.old_path_for("src/other.ts"), None);
    }

    #[test]
    fn rename_only_diff_records_pair_and_seeds_touched_files() {
        // Pure rename (100% similarity) has no content hunk and no
        // `+++ b/` line. The rename pair must still land in the index, and
        // the new path must show up in `touches_file` so downstream filters
        // recognise it as part of the change set.
        let diff = "\
diff --git a/src/keep.ts b/src/moved.ts
similarity index 100%
rename from src/keep.ts
rename to src/moved.ts
";
        let index = DiffIndex::from_unified_diff(diff);
        assert_eq!(index.old_path_for("src/moved.ts"), Some("src/keep.ts"));
        assert!(index.touches_file("src/moved.ts"));
        assert!(!index.touches_file("src/keep.ts"));
        assert_eq!(index.added_line_count(), 0);
    }

    #[test]
    fn unpaired_rename_from_does_not_bleed_into_next_file() {
        // Defensive: if a malformed diff has a `rename from` without a
        // matching `rename to` (truncated input, hand-crafted patch), the
        // pending `from` must be cleared at the next `diff --git` header so
        // it cannot accidentally pair with a later block's `rename to`.
        let diff = "\
diff --git a/src/a.ts b/src/a.ts
rename from src/dropped-from.ts
--- a/src/a.ts
+++ b/src/a.ts
@@ -1,1 +1,1 @@
-old
+new
diff --git a/src/b.ts b/src/c.ts
rename from src/b.ts
rename to src/c.ts
";
        let index = DiffIndex::from_unified_diff(diff);
        // The well-formed pair lands.
        assert_eq!(index.old_path_for("src/c.ts"), Some("src/b.ts"));
        // The unpaired from does NOT leak into anything else.
        assert_eq!(index.old_path_for("src/dropped-from.ts"), None);
        assert_eq!(index.old_path_for("src/a.ts"), None);
    }

    #[test]
    fn relative_to_diff_path_strips_absolute_root() {
        let root = Path::new("/project");
        let p = Path::new("/project/src/a.ts");
        assert_eq!(relative_to_diff_path(p, root).as_deref(), Some("src/a.ts"));
    }

    #[test]
    fn relative_to_diff_path_passes_through_relative() {
        let root = Path::new("/project");
        let p = Path::new("src/a.ts");
        assert_eq!(relative_to_diff_path(p, root).as_deref(), Some("src/a.ts"));
    }

    #[test]
    fn relative_to_diff_path_returns_none_for_path_outside_root() {
        let root = Path::new("/project");
        let p = Path::new("/elsewhere/x.ts");
        assert!(relative_to_diff_path(p, root).is_none());
    }

    #[test]
    fn added_mode_keeps_only_added_lines() {
        let diff = "\
diff --git a/src/a.ts b/src/a.ts
--- a/src/a.ts
+++ b/src/a.ts
@@ -1,2 +1,3 @@
 old
+new
 ctx
";
        let index = DiffIndex::from_unified_diff(diff);
        let keep = CiIssue {
            rule_id: "r".into(),
            description: "d".into(),
            severity: "minor".into(),
            path: "src/a.ts".into(),
            line: 2,
            fingerprint: "a".into(),
        };
        let drop = CiIssue {
            line: 3,
            ..keep.clone()
        };
        assert!(index.keeps(&keep, DiffFilterMode::Added));
        assert!(!index.keeps(&drop, DiffFilterMode::Added));
        assert!(index.keeps(&drop, DiffFilterMode::DiffContext));
        assert!(index.keeps(&drop, DiffFilterMode::File));
    }
}
