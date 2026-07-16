use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub use fallow_output::{DiffIndex, MAX_DIFF_BYTES, parse_new_hunk_start};

use fallow_output::CiIssue;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SummaryScope {
    All,
    Diff,
}

impl SummaryScope {
    #[must_use]
    fn from_env() -> Self {
        std::env::var("FALLOW_SUMMARY_SCOPE")
            .ok()
            .as_deref()
            .map_or(Self::All, Self::from_value)
    }

    #[must_use]
    fn from_value(value: &str) -> Self {
        match value.trim() {
            "diff" => Self::Diff,
            _ => Self::All,
        }
    }
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
/// `DiffIndex` plus the raw unified-diff text it was parsed from; the source
/// breadcrumb is consumed by the function during load to compose warning
/// messages and is not retained beyond that. The raw text is retained so the
/// walkthrough guide can derive per-hunk `change_anchors` from the SAME diff
/// source the finding filter used (a `--diff-stdin` staged diff, not the
/// committed `base...HEAD`), keeping emission and validation anchored to one
/// changed set.
#[derive(Debug)]
pub struct LoadedDiff {
    pub index: DiffIndex,
    pub raw: String,
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
        DiffSource::Stdin => load_diff_index_from_stdin(quiet),
        DiffSource::Flag(path) | DiffSource::EnvVar(path) => {
            load_diff_index_from_file(path, &source.label(), quiet)
        }
    }
}

/// Drain stdin once and parse it into a `LoadedDiff`, warning on failure / empty index.
fn load_diff_index_from_stdin(quiet: bool) -> Option<LoadedDiff> {
    let stdin = std::io::stdin();
    load_diff_index_from_reader(stdin.lock(), "--diff-stdin", MAX_DIFF_BYTES, quiet)
}

/// Read a diff file (respecting the size cap) and parse it into a `LoadedDiff`.
fn load_diff_index_from_file(path: &Path, label: &str, quiet: bool) -> Option<LoadedDiff> {
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
    match std::fs::File::open(path) {
        Ok(file) => load_diff_index_from_reader(file, label, MAX_DIFF_BYTES, quiet),
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

fn load_diff_index_from_reader(
    reader: impl std::io::Read,
    label: &str,
    limit: u64,
    quiet: bool,
) -> Option<LoadedDiff> {
    let mut bytes = Vec::new();
    if let Err(err) = reader.take(limit + 1).read_to_end(&mut bytes) {
        if !quiet {
            eprintln!(
                "fallow: warning [diff-file]: could not read {label}: {err} \
                 (line-level filtering disabled)"
            );
        }
        return None;
    }
    if bytes.len() as u64 > limit {
        if !quiet {
            eprintln!(
                "fallow: warning [diff-file]: {label} is at least {} bytes (cap {limit}); \
                 line-level filtering disabled, reporting all findings",
                bytes.len()
            );
        }
        return None;
    }
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => {
            if !quiet {
                eprintln!(
                    "fallow: warning [diff-file]: could not read {label} as UTF-8: {err} \
                     (line-level filtering disabled)"
                );
            }
            return None;
        }
    };
    let index = DiffIndex::from_unified_diff(&text);
    if !quiet && index.added_line_count() == 0 {
        eprintln!(
            "fallow: warning [diff-file]: {label} parsed 0 added lines; \
             no findings will pass the diff filter. Verify the input is a unified diff \
             (look for `+++ b/<path>` headers). Pure-rename, binary-only, and \
             deletion-only diffs also produce empty indices."
        );
    }
    Some(LoadedDiff { index, raw: text })
}

/// Process-wide cache for the diff index resolved at startup, so combined
/// runs do not re-read stdin (impossible) or re-parse the same file three
/// times across `check`, `dupes`, and `health`.
///
/// Populated once by `main()` via [`init_shared_diff`] after CLI parsing;
/// every subsystem queries it via [`shared_diff_index`] at filter time.
///
/// Programmatic and Node callers pass their own per-call diff index instead
/// of populating this cache; callers that never provide one see no line-level
/// filter. In every path, the diff filter is strictly opt-in.
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
pub fn init_shared_diff(
    source: Option<&DiffSource>,
    root: &Path,
    candidate_bases: &[PathBuf],
    quiet: bool,
) -> Option<&'static DiffIndex> {
    let loaded = source
        .and_then(|src| load_diff_index_for_findings(src, quiet))
        .and_then(|loaded| {
            // A diff that parsed but names no analyzable head-side file (empty,
            // deletion-only, or binary-only) changed nothing a finding can be
            // attributed to. That is a real, EMPTY scope, not an unplaceable
            // base: keep the empty index so every source-anchored finding
            // filters out (report clean) rather than falling open to full scope.
            // Only a diff we cannot place (foreign or ambiguous base) falls open.
            // The empty index needs no base: with no keys every lookup misses,
            // and `key_for` still yields a key for in-root paths, so findings are
            // dropped rather than retained.
            if loaded.index.touched_files().next().is_none() {
                return Some(loaded);
            }
            let label = source.map(DiffSource::label).unwrap_or_default();
            let chosen = choose_diff_base(&loaded.index, candidate_bases);
            match chosen {
                // The diff names files, but none under any candidate base
                // (foreign), or equally under two at once (ambiguous). Either way
                // we cannot express findings in its namespace. `check::filtering`
                // sets the convention for that: an unfilterable path is RETAINED,
                // never silently dropped. So drop the diff instead of the findings
                // and report at full scope.
                None => {
                    if !quiet {
                        warn_on_foreign_diff_namespace(&loaded.index, candidate_bases, &label);
                    }
                    None
                }
                Some(chosen) if chosen.ambiguous => {
                    if !quiet {
                        warn_on_ambiguous_diff_base(candidate_bases, &label);
                    }
                    None
                }
                Some(chosen) => {
                    let offset = root_offset_below(&chosen.base, root);
                    Some(LoadedDiff {
                        index: loaded.index.with_base(chosen.base).with_root_offset(offset),
                        raw: loaded.raw,
                    })
                }
            }
        });
    let _ = SHARED_DIFF.set(loaded);
    shared_diff_index()
}

/// Where the analysis root sits below `base`, forward-slashed, empty when they
/// are the same directory.
fn root_offset_below(base: &Path, root: &Path) -> String {
    root.strip_prefix(base)
        .map(|offset| offset.display().to_string().replace('\\', "/"))
        .unwrap_or_default()
}

/// The base a diff's paths were written relative to, plus whether the evidence
/// actually distinguished it from the runner-up.
struct ChosenBase {
    base: PathBuf,
    ambiguous: bool,
}

/// Decide which directory the diff's paths are relative to.
///
/// A unified diff carries no statement of its own base. `git diff` writes paths
/// relative to the repository toplevel, but `git diff --relative` writes them
/// relative to the invoking directory, and both reach fallow through
/// `--diff-file` / `--diff-stdin`. Assuming either one silently drops every
/// source-anchored finding for users of the other.
///
/// The paths themselves settle it: they name files that exist on disk. Score
/// each candidate by how many of the diff's paths resolve under it and take the
/// best. `candidate_bases` is ordered most-preferred first, so an exact tie
/// keeps the caller's precedence.
///
/// A tie is not a decision. A repo with both `<toplevel>/src/a.ts` and
/// `<root>/src/a.ts` resolves the diff path `src/a.ts` under either candidate,
/// and existence alone cannot say which the diff meant. Picking the preferred
/// one and staying silent would reproduce the empty-report-looks-clean failure
/// this whole mechanism exists to prevent, so the tie is reported.
/// `None` means the diff names nothing under any candidate.
fn choose_diff_base(index: &DiffIndex, candidate_bases: &[PathBuf]) -> Option<ChosenBase> {
    let mut scored: Vec<(usize, &PathBuf)> = candidate_bases
        .iter()
        .map(|base| {
            let resolved = index
                .touched_files()
                .filter(|path| base.join(path).exists())
                .count();
            (resolved, base)
        })
        .filter(|(resolved, _)| *resolved > 0)
        .collect();

    // Stable sort by score, descending: equal scores keep caller precedence.
    scored.sort_by(|(a, _), (b, _)| b.cmp(a));
    let (best_score, best_base) = *scored.first()?;
    let ambiguous = scored
        .get(1)
        .is_some_and(|(runner_up, _)| *runner_up == best_score);

    Some(ChosenBase {
        base: best_base.clone(),
        ambiguous,
    })
}

/// The diff's paths resolve equally well under two different directories, so
/// existence alone cannot place its base. Rather than filter against a guess
/// (whose wrong half drops every source-anchored finding), the run discards the
/// diff and reports at full scope, so the message names the ambiguity and says
/// so rather than letting silence imply the report was scoped.
fn warn_on_ambiguous_diff_base(candidate_bases: &[PathBuf], label: &str) {
    let bases = candidate_bases
        .iter()
        .map(|base| base.display().to_string())
        .collect::<Vec<_>>()
        .join(" and ");
    eprintln!(
        "fallow: warning [diff-file]: the paths in {label} name existing files under \
         {bases}, so their base is ambiguous and fallow cannot tell which one the diff \
         is relative to. It will not filter against a guess: every finding is reported \
         (full scope, not scoped to the diff). Generate the diff from the repository \
         root (plain `git diff`, not `git diff --relative`) to scope the report."
    );
}

/// A diff whose paths name no file under any candidate base was almost
/// certainly generated relative to some other directory. fallow cannot place it,
/// so it discards the diff and reports at full scope. Say so, once, rather than
/// let the unscoped report imply the diff was applied.
fn warn_on_foreign_diff_namespace(index: &DiffIndex, candidate_bases: &[PathBuf], label: &str) {
    let total = index.touched_files().count();
    if total == 0 {
        return;
    }
    let bases = candidate_bases
        .iter()
        .map(|base| base.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    eprintln!(
        "fallow: warning [diff-file]: none of the {total} file(s) named by {label} exist \
         under {bases}; the diff's paths look relative to a different directory. fallow \
         cannot place the diff, so every finding is reported (full scope, not scoped to \
         the diff). Regenerate the diff from one of those directories to scope the report."
    );
}

/// Read the cached diff index populated by [`init_shared_diff`]. Returns
/// `None` when the cache is empty (no diff was supplied, or
/// `init_shared_diff` was never called).
#[must_use]
pub fn shared_diff_index() -> Option<&'static DiffIndex> {
    SHARED_DIFF.get().and_then(|v| v.as_ref()).map(|l| &l.index)
}

/// Read the RAW unified-diff text of the cached diff (the bytes
/// [`init_shared_diff`] parsed). `None` when no diff was supplied. Used by the
/// walkthrough guide to derive `change_anchors` from the same opt-in diff source
/// (e.g. a `--diff-stdin` staged diff) the finding filter used, rather than
/// recomputing a committed `base...HEAD` diff that would not match.
#[must_use]
pub fn shared_diff_raw() -> Option<&'static str> {
    SHARED_DIFF
        .get()
        .and_then(|v| v.as_ref())
        .map(|l| l.raw.as_str())
}

fn context_radius_from_env() -> u64 {
    std::env::var("FALLOW_DIFF_CONTEXT")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(3)
}

/// Filter issues against this run's diff.
///
/// Gated on the shared cache, not on `$FALLOW_DIFF_FILE`: `--diff-file` takes
/// precedence when resolving that cache, so gating on the env var would leave
/// `--diff-file --format review-gitlab` rendering unfiltered comments, and
/// would filter against the flag's diff while claiming to honour the env var's.
/// The shared index also carries the base its paths were written against;
/// re-parsing here would yield an unbased index whose every lookup misses for
/// an analysis root below that base.
///
/// The three cache states are distinct and must stay so. When `init_shared_diff`
/// discarded the diff (unplaceable base), that full-scope decision is
/// authoritative here too: re-reading the env var would re-filter and contradict
/// it. The env-var fallback is only for the case where `init_shared_diff` never
/// ran (an embedder or a test), so those callers keep working.
#[must_use]
pub fn filter_issues_from_env(issues: Vec<CiIssue>) -> Vec<CiIssue> {
    let mode = DiffFilterMode::from_env();
    let radius = context_radius_from_env();
    match SHARED_DIFF.get() {
        // A diff was resolved for this run (a placed base, or a parsed-but-empty
        // scope). Filter against it; an empty-scope index drops every
        // source-anchored issue, matching the finding filter.
        Some(Some(loaded)) => issues
            .into_iter()
            .filter(|issue| diff_index_keeps_issue(&loaded.index, issue, mode, radius))
            .collect(),
        // `init_shared_diff` ran and deliberately discarded the diff (foreign or
        // ambiguous base): report at full scope, the same decision the finding
        // filter made. Re-reading FALLOW_DIFF_FILE here would contradict it.
        Some(None) => issues,
        // `init_shared_diff` never ran: an embedder or a test, not a CLI run.
        // Honour the env var directly so those callers keep working.
        None => {
            let Some(raw_path) = std::env::var_os("FALLOW_DIFF_FILE") else {
                return issues;
            };
            filter_issues_from_path(issues, Path::new(&raw_path), mode, radius)
        }
    }
}

/// Filter for the typed PR-comment renderer (`print_pr_comment`).
///
/// `FALLOW_SUMMARY_SCOPE=all` (default) keeps the existing behavior:
/// project-level rule findings (dependency / catalog / override hygiene that
/// lives in `package.json` / `pnpm-workspace.yaml`) bypass the diff filter
/// because the PR diff rarely touches the anchored line even though the
/// finding may be the reason CI fails.
///
/// `FALLOW_SUMMARY_SCOPE=diff` applies the same diff filter to project-level
/// findings too, which is useful for advisory monorepo comments where
/// unrelated pre-existing dependency findings would otherwise dominate the
/// sticky summary.
///
/// Sorting is restored after the partition + merge so downstream rendering
/// sees the same `(path, line, fingerprint)` order as the unfiltered input.
#[must_use]
pub fn filter_issues_for_summary(issues: Vec<CiIssue>) -> Vec<CiIssue> {
    summary_filter_with_scope(issues, SummaryScope::from_env(), filter_issues_from_env)
}

/// Scope-aware helper for `filter_issues_for_summary`. Generic over the
/// source-level filter so tests can call it with `filter_issues_from_path`
/// against a tempdir diff without relying on a process-wide diff env var.
fn summary_filter_with_scope<F>(
    issues: Vec<CiIssue>,
    scope: SummaryScope,
    source_filter: F,
) -> Vec<CiIssue>
where
    F: FnOnce(Vec<CiIssue>) -> Vec<CiIssue>,
{
    if scope == SummaryScope::Diff {
        return source_filter(issues);
    }

    let (project_level, diff_relevant): (Vec<CiIssue>, Vec<CiIssue>) = issues
        .into_iter()
        .partition(|issue| fallow_output::is_project_level_rule(&issue.rule_id));
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
        .filter(|issue| diff_index_keeps_issue(&index, issue, mode, radius))
        .collect()
}

fn diff_index_keeps_issue(
    index: &DiffIndex,
    issue: &CiIssue,
    mode: DiffFilterMode,
    radius: u64,
) -> bool {
    // `issue.path` is analysis-root-relative; the index's keys live in the
    // diff's own namespace. Presentation prefixes are applied later, at render.
    let key = index.key_for_root_relative(&issue.path);
    match mode {
        DiffFilterMode::NoFilter => true,
        DiffFilterMode::File => index.touches_file(&key),
        DiffFilterMode::DiffContext => index.line_within_added_context(&key, issue.line, radius),
        DiffFilterMode::Added => index.line_is_added(&key, issue.line),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write as _};

    use super::*;
    use fallow_output::relative_to_diff_path;

    #[test]
    fn bounded_diff_reader_accepts_exact_limit() {
        let loaded =
            load_diff_index_from_reader(Cursor::new(b"12345678"), "test diff", 8, true).unwrap();
        assert_eq!(loaded.raw, "12345678");
    }

    #[test]
    fn bounded_diff_reader_rejects_limit_plus_one() {
        assert!(
            load_diff_index_from_reader(Cursor::new(b"123456789"), "test diff", 8, true).is_none()
        );
    }

    #[test]
    fn bounded_diff_reader_rejects_invalid_utf8() {
        assert!(
            load_diff_index_from_reader(Cursor::new([0xff, 0xfe]), "test diff", 8, true).is_none()
        );
    }

    #[test]
    fn bounded_diff_reader_parses_valid_unified_diff() {
        let text = "diff --git a/src/a.ts b/src/a.ts\n\
                    --- a/src/a.ts\n\
                    +++ b/src/a.ts\n\
                    @@ -0,0 +1,1 @@\n\
                    +export const a = 1;\n";
        let loaded =
            load_diff_index_from_reader(Cursor::new(text), "test diff", 1024, true).unwrap();
        assert_eq!(loaded.index.added_line_count(), 1);
    }

    #[test]
    fn bounded_diff_reader_preserves_empty_index_behavior() {
        let loaded =
            load_diff_index_from_reader(Cursor::new(b"not a diff"), "test diff", 32, true).unwrap();
        assert_eq!(loaded.index.added_line_count(), 0);
    }

    #[test]
    fn filter_issues_from_path_skips_oversize_diff() {
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
    fn summary_scope_parses_safe_defaults() {
        assert_eq!(SummaryScope::from_value("diff"), SummaryScope::Diff);
        assert_eq!(SummaryScope::from_value("all"), SummaryScope::All);
        assert_eq!(SummaryScope::from_value(" all "), SummaryScope::All);
        assert_eq!(SummaryScope::from_value(""), SummaryScope::All);
        assert_eq!(SummaryScope::from_value("typo"), SummaryScope::All);
    }

    #[test]
    fn summary_scope_all_keeps_project_level_findings_when_diff_misses_them() {
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
        let kept = summary_filter_with_scope(
            vec![
                project_level,
                source_level_in_diff,
                source_level_outside_diff,
            ],
            SummaryScope::All,
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
    fn summary_scope_diff_filters_project_level_findings_when_diff_misses_them() {
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
            rule_id: "fallow/unused-dependency".into(),
            description: "Dependency unused".into(),
            severity: "minor".into(),
            path: "package.json".into(),
            line: 12,
            fingerprint: "dep".into(),
        };
        let kept = summary_filter_with_scope(vec![project_level], SummaryScope::Diff, |src| {
            filter_issues_from_path(src, &diff_path, DiffFilterMode::Added, 3)
        });
        assert!(
            kept.is_empty(),
            "diff scope must hide project-level findings outside the diff: {kept:?}"
        );
    }

    #[test]
    fn summary_scope_diff_keeps_project_level_findings_when_anchor_line_is_added() {
        let dir = tempfile::tempdir().expect("tempdir");
        let diff_path = dir.path().join("pr.diff");
        std::fs::write(
            &diff_path,
            "diff --git a/package.json b/package.json\n\
             --- a/package.json\n\
             +++ b/package.json\n\
             @@ -11,1 +11,2 @@\n\
              \"dependencies\": {\n\
             +  \"lodash\": \"^4.17.21\"\n",
        )
        .expect("write");

        let project_level = CiIssue {
            rule_id: "fallow/unused-dependency".into(),
            description: "Dependency unused".into(),
            severity: "minor".into(),
            path: "package.json".into(),
            line: 12,
            fingerprint: "dep".into(),
        };
        let kept = summary_filter_with_scope(vec![project_level], SummaryScope::Diff, |src| {
            filter_issues_from_path(src, &diff_path, DiffFilterMode::Added, 3)
        });
        assert_eq!(kept.len(), 1, "changed package.json finding must remain");
        assert_eq!(kept[0].fingerprint, "dep");
    }

    #[test]
    fn summary_filter_preserves_path_line_fingerprint_sort_order() {
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
        let kept = summary_filter_with_scope(vec![a, b], SummaryScope::All, |issues| issues);
        assert_eq!(kept[0].fingerprint, "b");
        assert_eq!(kept[1].fingerprint, "a");
    }

    #[test]
    fn range_overlaps_added_hotspot_starting_before_diff_touches_inside() {
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
        assert!(!index.range_overlaps_added("src/other.ts", 10, 120));
        assert!(!index.range_overlaps_added("src/big.ts", 10, 100));
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
        assert!(index.range_overlaps_added("src/a.ts", 2, 2));
    }

    #[test]
    fn range_overlaps_added_range_starting_at_zero_collapses_to_one() {
        let diff = "\
diff --git a/src/a.ts b/src/a.ts
--- a/src/a.ts
+++ b/src/a.ts
@@ -1,1 +1,2 @@
 ctx
+new
";
        let index = DiffIndex::from_unified_diff(diff);
        assert!(!index.range_overlaps_added("src/a.ts", 0, 0));
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
    fn delete_only_diff_records_removals_without_touching_head_file() {
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
        assert_eq!(index.hunk_count(), 1);
        assert_eq!(index.net_lines(), -3);
        assert!(index.changes_path("dead.ts"));
        assert!(!index.touches_file("dead.ts"));
        assert!(!index.range_overlaps_added("dead.ts", 1, 3));
    }

    #[test]
    fn rename_with_content_hunk_indexes_under_new_path() {
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
        assert_eq!(index.old_path_for("src/new.ts"), Some("src/old.ts"));
        assert_eq!(index.old_path_for("src/other.ts"), None);
    }

    #[test]
    fn rename_only_diff_records_pair_and_seeds_touched_files() {
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
        assert_eq!(index.old_path_for("src/c.ts"), Some("src/b.ts"));
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
        assert!(diff_index_keeps_issue(
            &index,
            &keep,
            DiffFilterMode::Added,
            3
        ));
        assert!(!diff_index_keeps_issue(
            &index,
            &drop,
            DiffFilterMode::Added,
            3
        ));
        assert!(diff_index_keeps_issue(
            &index,
            &drop,
            DiffFilterMode::DiffContext,
            3
        ));
        assert!(diff_index_keeps_issue(
            &index,
            &drop,
            DiffFilterMode::File,
            3
        ));
    }
}
