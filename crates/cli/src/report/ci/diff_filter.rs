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
    fn from_env() -> Self {
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
    /// repeated calls to `resolve_diff_source` would observe EOF.
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

/// Result of `load_diff_index_for_findings`. Carries the parsed
/// `DiffIndex`, the raw unified-diff text it was parsed from, and the
/// user-facing source label. The raw text is retained so the
/// walkthrough guide can derive per-hunk `change_anchors` from the SAME diff
/// source the finding filter used (a `--diff-stdin` staged diff, not the
/// committed `base...HEAD`), keeping emission and validation anchored to one
/// changed set.
#[derive(Debug)]
pub struct LoadedDiff {
    index: DiffIndex,
    raw: String,
    /// User-facing label of the source the diff was loaded from (for example
    /// `--diff-file pr.diff`), retained so downstream consumers can name the
    /// diff that decided a filtering or demotion outcome (issue #2220).
    source_label: String,
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
pub(crate) fn resolve_diff_source(
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

/// Why a supplied diff could not be applied, plus the sentence that says so.
///
/// The reason token is what `request_outcomes["diff-filter"].reason` publishes
/// and the message is what both the stderr line and that entry's `message`
/// render, so the log a human read and the envelope a script read cannot state
/// different things. Every stand-down returns one of these instead of printing
/// where it happens: the print is quiet-gated at one place, and the recording
/// is not, which is the whole defect this type exists to close (issue #2688).
#[derive(Debug)]
pub(crate) struct DiffStandDown {
    reason: &'static str,
    message: String,
}

impl DiffStandDown {
    fn new(reason: &'static str, message: String) -> Self {
        Self { reason, message }
    }

    fn oversize(label: &str, bytes: u64, cap: u64) -> Self {
        Self::new(
            "oversize",
            format!(
                "{label} is {bytes} bytes (cap {cap}); line-level filtering disabled, \
                 reporting all findings. Narrow the diff; the cap is fixed."
            ),
        )
    }

    fn unreadable(label: &str, err: &std::io::Error) -> Self {
        Self::new(
            "unreadable",
            format!(
                "could not read {label}: {err} (line-level filtering disabled, \
                 reporting all findings). Check the path exists and is readable."
            ),
        )
    }

    fn not_utf8(label: &str, err: &std::string::FromUtf8Error) -> Self {
        Self::new(
            "not-utf8",
            format!(
                "could not read {label} as UTF-8: {err} (line-level filtering disabled, \
                 reporting all findings). Regenerate the diff as UTF-8 text."
            ),
        )
    }

    /// The diff's paths resolve equally well under two different directories,
    /// so existence alone cannot place its base. Rather than filter against a
    /// guess (whose wrong half drops every source-anchored finding), the run
    /// discards the diff and reports at full scope, so the message names the
    /// ambiguity and says so rather than letting silence imply the report was
    /// scoped.
    fn ambiguous_base(candidate_bases: &[PathBuf], root: &Path, label: &str) -> Self {
        let bases = join_bases(candidate_bases, root, " and ");
        Self::new(
            "ambiguous-base",
            format!(
                "the paths in {label} name existing files under {bases}, so their base is \
                 ambiguous and fallow cannot tell which one the diff is relative to. It will \
                 not filter against a guess: every finding is reported (full scope, not scoped \
                 to the diff). Generate the diff from the repository root (plain `git diff`, \
                 not `git diff --relative`) to scope the report."
            ),
        )
    }

    /// A diff whose paths name no file under any candidate base was almost
    /// certainly generated relative to some other directory. fallow cannot
    /// place it, so it discards the diff and reports at full scope. Say so,
    /// once, rather than let the unscoped report imply the diff was applied.
    fn foreign_namespace(
        index: &DiffIndex,
        candidate_bases: &[PathBuf],
        root: &Path,
        label: &str,
    ) -> Self {
        let total = index.touched_files().count();
        let bases = join_bases(candidate_bases, root, ", ");
        Self::new(
            "foreign-namespace",
            format!(
                "none of the {total} file(s) named by {label} exist under {bases}; the diff's \
                 paths look relative to a different directory. fallow cannot place the diff, so \
                 every finding is reported (full scope, not scoped to the diff). Regenerate the \
                 diff from one of those directories to scope the report."
            ),
        )
    }
}

fn join_bases(candidate_bases: &[PathBuf], root: &Path, separator: &str) -> String {
    candidate_bases
        .iter()
        .map(|base| base_label(base, root))
        .collect::<Vec<_>>()
        .join(separator)
}

/// Name a candidate base without putting the machine's checkout path in it.
///
/// This sentence is the `message` of a wire member, and every other
/// path-bearing member of a fallow envelope is project-root-relative, so an
/// absolute base here would make one input's output differ between checkouts.
/// The two candidates a CLI run offers are the analysis root and the git
/// toplevel above it (`diff_base_candidates`), and naming them by their
/// relation to the root tells the user which directory to regenerate the diff
/// from at least as well as the absolute path did: what they need is the path
/// prefix their diff is missing, which is exactly the offset reported here.
fn base_label(base: &Path, root: &Path) -> String {
    if base == root {
        return "the project root".to_owned();
    }
    if let Ok(offset) = root.strip_prefix(base) {
        let offset = offset.display().to_string().replace('\\', "/");
        return format!("the repository root (the project root is {offset} below it)");
    }
    if let Ok(inside) = base.strip_prefix(root) {
        return inside.display().to_string().replace('\\', "/");
    }
    "a directory outside the project root".to_owned()
}

/// Read + parse the resolved diff source into a `DiffIndex` for
/// finding-level filtering. Failure modes (file missing, oversize,
/// unreadable) return a [`DiffStandDown`] so the caller can both warn and
/// record, and the analysis then runs at full scope rather than failing for a
/// CI-script issue.
///
/// Stdin is consumed exactly once. The first call drains it; downstream
/// callers must reuse the returned `LoadedDiff` rather than re-loading.
fn load_diff_index_for_findings(
    source: &DiffSource,
    quiet: bool,
) -> Result<LoadedDiff, DiffStandDown> {
    match source {
        DiffSource::Stdin => load_diff_index_from_stdin(quiet),
        DiffSource::Flag(path) | DiffSource::EnvVar(path) => {
            load_diff_index_from_file(path, &source.label(), quiet)
        }
    }
}

/// Drain stdin once and parse it into a `LoadedDiff`.
fn load_diff_index_from_stdin(quiet: bool) -> Result<LoadedDiff, DiffStandDown> {
    let stdin = std::io::stdin();
    load_diff_index_from_reader(stdin.lock(), "--diff-stdin", MAX_DIFF_BYTES, quiet)
}

/// Read a diff file (respecting the size cap) and parse it into a `LoadedDiff`.
fn load_diff_index_from_file(
    path: &Path,
    label: &str,
    quiet: bool,
) -> Result<LoadedDiff, DiffStandDown> {
    if let Ok(meta) = std::fs::metadata(path)
        && meta.len() > MAX_DIFF_BYTES
    {
        return Err(DiffStandDown::oversize(label, meta.len(), MAX_DIFF_BYTES));
    }
    match std::fs::File::open(path) {
        Ok(file) => load_diff_index_from_reader(file, label, MAX_DIFF_BYTES, quiet),
        Err(err) => Err(DiffStandDown::unreadable(label, &err)),
    }
}

fn load_diff_index_from_reader(
    reader: impl std::io::Read,
    label: &str,
    limit: u64,
    quiet: bool,
) -> Result<LoadedDiff, DiffStandDown> {
    let mut bytes = Vec::new();
    if let Err(err) = reader.take(limit + 1).read_to_end(&mut bytes) {
        return Err(DiffStandDown::unreadable(label, &err));
    }
    if bytes.len() as u64 > limit {
        return Err(DiffStandDown::oversize(label, bytes.len() as u64, limit));
    }
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => return Err(DiffStandDown::not_utf8(label, &err)),
    };
    let index = DiffIndex::from_unified_diff(&text);
    // Not a stand-down: the filter IS applied and its scope is empty, so every
    // source-anchored finding filters out and the report reads clean. A
    // different fact from the ones above, and deliberately still only advice.
    if !quiet && index.added_line_count() == 0 {
        eprintln!(
            "fallow: warning [diff-file]: {label} parsed 0 added lines; \
             no findings will pass the diff filter. Verify the input is a unified diff \
             (look for `+++ b/<path>` headers). Pure-rename, binary-only, and \
             deletion-only diffs also produce empty indices."
        );
    }
    Ok(LoadedDiff {
        index,
        raw: text,
        source_label: label.to_owned(),
    })
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

/// What became of this run's diff-filter request, for the envelope's
/// `request_outcomes`.
///
/// A sibling of [`SHARED_DIFF`] rather than a widening of it: that cache's
/// three states each carry a documented correctness argument (see
/// [`filter_issues_from_env`]), and retyping it to carry the reason too would
/// put a reporting concern inside a filtering decision. `None` means the run
/// was given no diff at all.
static DIFF_REQUEST_OUTCOME: OnceLock<Option<fallow_output::RequestOutcome>> = OnceLock::new();

/// Resolve, read, and parse the diff source once for the lifetime of the
/// process. Idempotent: only the first call populates the cache; later
/// calls observe the original value. Returns the resolved index for the
/// caller to inspect (e.g. to log "0 hunks" or to skip a filtering step
/// when nothing was loaded).
///
/// Pass `None` to lock the cache to "no diff" without reading anything,
/// so a subsequent errant load attempt cannot accidentally populate the
/// cache later.
///
/// `quiet` suppresses the stderr line only. The recorded outcome is identical
/// either way, so a `--quiet --format json` run (which is what both shipped CI
/// integrations use) still reports that its filter stood down.
pub(crate) fn init_shared_diff(
    source: Option<&DiffSource>,
    root: &Path,
    candidate_bases: &[PathBuf],
    quiet: bool,
) -> Option<&'static DiffIndex> {
    let mut request = None;
    let loaded = source.and_then(|src| {
        let label = src.label();
        match place_diff(src, root, candidate_bases, quiet) {
            Ok(loaded) => {
                // The added-line count travels as the scope the filter left,
                // because `0` is the case a report cannot state for itself: the
                // filter applied, every source-anchored finding dropped, and the
                // clean document that follows covered nothing (issue #2734).
                request = Some(fallow_output::RequestOutcome::applied_with_scope_size(
                    fallow_output::RequestName::DiffFilter,
                    label,
                    loaded.index.added_line_count() as u64,
                ));
                Some(loaded)
            }
            Err(stand_down) => {
                if !quiet {
                    eprintln!("fallow: warning [diff-file]: {}", stand_down.message);
                }
                request = Some(fallow_output::RequestOutcome::not_applied(
                    fallow_output::RequestName::DiffFilter,
                    label,
                    stand_down.reason,
                    stand_down.message,
                ));
                None
            }
        }
    });
    let _ = SHARED_DIFF.set(loaded);
    let _ = DIFF_REQUEST_OUTCOME.set(request);
    shared_diff_index()
}

/// Load a diff and decide which directory its paths are relative to.
///
/// `Err` is a stand-down: the caller reports at full scope. `Ok` covers both a
/// placed diff and a parsed-but-empty one, which are different scopes and not
/// different outcomes.
fn place_diff(
    source: &DiffSource,
    root: &Path,
    candidate_bases: &[PathBuf],
    quiet: bool,
) -> Result<LoadedDiff, DiffStandDown> {
    let loaded = load_diff_index_for_findings(source, quiet)?;
    // A diff that parsed but names no analyzable head-side file (empty,
    // deletion-only, or binary-only) changed nothing a finding can be
    // attributed to. That is a real, EMPTY scope, not an unplaceable base:
    // keep the empty index so every source-anchored finding filters out
    // (report clean) rather than falling open to full scope. Only a diff we
    // cannot place (foreign or ambiguous base) falls open. The empty index
    // needs no base: with no keys every lookup misses, and `key_for` still
    // yields a key for in-root paths, so findings are dropped rather than
    // retained.
    if loaded.index.touched_files().next().is_none() {
        return Ok(loaded);
    }
    let label = source.label();
    // The diff names files, but none under any candidate base (foreign), or
    // equally under two at once (ambiguous). Either way we cannot express
    // findings in its namespace. `check::filtering` sets the convention for
    // that: an unfilterable path is RETAINED, never silently dropped. So drop
    // the diff instead of the findings and report at full scope.
    match choose_diff_base(&loaded.index, candidate_bases) {
        None => Err(DiffStandDown::foreign_namespace(
            &loaded.index,
            candidate_bases,
            root,
            &label,
        )),
        Some(chosen) if chosen.ambiguous => {
            Err(DiffStandDown::ambiguous_base(candidate_bases, root, &label))
        }
        Some(chosen) => {
            let offset = root_offset_below(&chosen.base, root);
            Ok(LoadedDiff {
                index: loaded.index.with_base(chosen.base).with_root_offset(offset),
                raw: loaded.raw,
                source_label: loaded.source_label,
            })
        }
    }
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

/// Read the cached diff index populated by [`init_shared_diff`]. Returns
/// `None` when the cache is empty (no diff was supplied, or
/// `init_shared_diff` was never called).
#[must_use]
pub(crate) fn shared_diff_index() -> Option<&'static DiffIndex> {
    SHARED_DIFF.get().and_then(|v| v.as_ref()).map(|l| &l.index)
}

/// Read the RAW unified-diff text of the cached diff (the bytes
/// [`init_shared_diff`] parsed). `None` when no diff was supplied. Used by the
/// walkthrough guide to derive `change_anchors` from the same opt-in diff source
/// (e.g. a `--diff-stdin` staged diff) the finding filter used, rather than
/// recomputing a committed `base...HEAD` diff that would not match.
#[must_use]
pub(crate) fn shared_diff_raw() -> Option<&'static str> {
    SHARED_DIFF
        .get()
        .and_then(|v| v.as_ref())
        .map(|l| l.raw.as_str())
}

/// User-facing label of the source backing the shared diff index (for example
/// `--diff-file pr.diff` or `--diff-stdin`). `None` when no shared diff was
/// loaded.
#[must_use]
pub(crate) fn shared_diff_source_label() -> Option<&'static str> {
    SHARED_DIFF
        .get()
        .and_then(|v| v.as_ref())
        .map(|l| l.source_label.as_str())
}

/// What became of this run's diff-filter request, for the envelope's
/// `request_outcomes`. `None` when the run was given no diff, which is what
/// keeps the key off the wire for everyone who never asked (issue #2688).
#[must_use]
pub(crate) fn shared_diff_request_outcome() -> Option<&'static fallow_output::RequestOutcome> {
    DIFF_REQUEST_OUTCOME.get().and_then(Option::as_ref)
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
pub(crate) fn filter_issues_from_env(issues: Vec<CiIssue>) -> Vec<CiIssue> {
    let mode = DiffFilterMode::from_env();
    let radius = context_radius_from_env();
    match SHARED_DIFF.get() {
        // A diff was resolved for this run (a placed base, or a parsed-but-empty
        // scope). Filter against it; an empty-scope index drops every
        // source-anchored issue, matching the finding filter.
        Some(Some(loaded)) => filter_issues_with_index(issues, &loaded.index, mode, radius),
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
pub(crate) fn filter_issues_for_summary(issues: Vec<CiIssue>) -> Vec<CiIssue> {
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

/// Filter against a diff read here rather than from the shared cache, for the
/// one caller [`filter_issues_from_env`] documents: a process where
/// `init_shared_diff` never ran.
///
/// Its three stand-downs print and record nothing, deliberately. This arm is
/// reachable only when [`DIFF_REQUEST_OUTCOME`] has no writer, so there is no
/// envelope being assembled in this process for an outcome to reach: an
/// embedder builds its own report, and `request_outcomes()` is a CLI-layer
/// projection. Recording here would file an outcome nobody reads while leaving
/// the CLI path's own entry, written by `init_shared_diff`, as the only one that
/// ever travels.
#[must_use]
fn filter_issues_from_path(
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
    filter_issues_with_index(issues, &index, mode, radius)
}

fn filter_issues_with_index(
    issues: Vec<CiIssue>,
    index: &DiffIndex,
    mode: DiffFilterMode,
    radius: u64,
) -> Vec<CiIssue> {
    let mut kept = issues
        .into_iter()
        .filter_map(|mut issue| {
            if mode == DiffFilterMode::Added {
                let key = index.key_for_root_relative(&issue.path);
                let end = issue
                    .end_line
                    .filter(|end| *end >= issue.line)
                    .unwrap_or(issue.line);
                issue.line = index.first_added_line_in_range(&key, issue.line, end)?;
                return Some(issue);
            }
            diff_index_keeps_issue(index, &issue, mode, radius).then_some(issue)
        })
        .collect::<Vec<_>>();
    if mode == DiffFilterMode::Added {
        kept.sort_by(|a, b| {
            (&a.path, a.line, &a.fingerprint).cmp(&(&b.path, b.line, &b.fingerprint))
        });
    }
    kept
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
        let stand_down =
            load_diff_index_from_reader(Cursor::new(b"123456789"), "test diff", 8, true)
                .expect_err("a diff over the cap stands the filter down");
        assert_eq!(stand_down.reason, "oversize");
        assert!(
            stand_down.message.contains("reporting all findings"),
            "the recorded sentence must say the report widened: {}",
            stand_down.message
        );
    }

    #[test]
    fn bounded_diff_reader_rejects_invalid_utf8() {
        let stand_down =
            load_diff_index_from_reader(Cursor::new([0xff, 0xfe]), "test diff", 8, true)
                .expect_err("a non-UTF-8 diff stands the filter down");
        assert_eq!(stand_down.reason, "not-utf8");
    }

    /// A reader failure and a cap breach are different remedies, so they must
    /// not collapse into one token on the wire.
    #[test]
    fn a_missing_diff_file_stands_down_as_unreadable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("absent.diff");
        let stand_down = load_diff_index_from_file(&missing, "--diff-file absent.diff", true)
            .expect_err("a missing diff stands the filter down");
        assert_eq!(stand_down.reason, "unreadable");
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

    /// The retained source label is what `shared_diff_source_label()` serves
    /// to name the diff that decided a filtering or demotion outcome
    /// (issue #2220).
    #[test]
    fn bounded_diff_reader_retains_the_source_label() {
        let text = "diff --git a/src/a.ts b/src/a.ts\n\
                    --- a/src/a.ts\n\
                    +++ b/src/a.ts\n\
                    @@ -0,0 +1,1 @@\n\
                    +export const a = 1;\n";
        let loaded =
            load_diff_index_from_reader(Cursor::new(text), "--diff-file pr.diff", 1024, true)
                .unwrap();
        assert_eq!(loaded.source_label, "--diff-file pr.diff");
    }

    /// The stand-down message is a wire field, so it names the candidate bases
    /// by their relation to the project root rather than by absolute path.
    #[test]
    fn a_stand_down_names_its_bases_without_the_checkout_path() {
        let root = Path::new("/checkout/packages/app");
        let toplevel = Path::new("/checkout");
        let index = DiffIndex::from_unified_diff(
            "diff --git a/src/a.ts b/src/a.ts\n\
             --- a/src/a.ts\n\
             +++ b/src/a.ts\n\
             @@ -0,0 +1,1 @@\n\
             +export const a = 1;\n",
        );
        let bases = vec![toplevel.to_path_buf(), root.to_path_buf()];

        let foreign = DiffStandDown::foreign_namespace(&index, &bases, root, "--diff-file pr.diff");
        assert_eq!(foreign.reason, "foreign-namespace");
        let ambiguous = DiffStandDown::ambiguous_base(&bases, root, "--diff-file pr.diff");
        assert_eq!(ambiguous.reason, "ambiguous-base");

        for message in [&foreign.message, &ambiguous.message] {
            assert!(
                !message.contains("/checkout"),
                "no absolute base reaches the wire: {message}"
            );
            assert!(
                message.contains("the project root"),
                "the analysis root is named: {message}"
            );
            assert!(
                message.contains("the repository root (the project root is packages/app below it)"),
                "the toplevel is named with the offset the diff is missing: {message}"
            );
        }
    }

    /// A single-candidate run (analysis root at the repository toplevel) names
    /// the one base it had, and still names no path.
    #[test]
    fn a_single_candidate_base_is_named_as_the_project_root() {
        let root = Path::new("/checkout");
        let stand_down = DiffStandDown::ambiguous_base(&[root.to_path_buf()], root, "--diff-stdin");
        assert!(
            stand_down
                .message
                .contains("under the project root, so their base is ambiguous"),
            "{}",
            stand_down.message
        );
        assert!(!stand_down.message.contains("/checkout"));
    }

    /// The cap has no override, so the remedy cannot suggest raising it.
    #[test]
    fn the_oversize_remedy_asks_only_for_something_the_user_can_do() {
        let stand_down =
            DiffStandDown::oversize("--diff-file pr.diff", MAX_DIFF_BYTES + 1, MAX_DIFF_BYTES);
        assert!(
            !stand_down.message.contains("raise the cap"),
            "{}",
            stand_down.message
        );
        assert!(stand_down.message.contains("Narrow the diff"));
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
            end_line: None,
            other_locations: Vec::new(),
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
            end_line: None,
            other_locations: Vec::new(),
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
            end_line: None,
            other_locations: Vec::new(),
            fingerprint: "override".into(),
        };
        let source_level_in_diff = CiIssue {
            rule_id: "fallow/unused-export".into(),
            description: "Export unused".into(),
            severity: "minor".into(),
            path: "src/a.ts".into(),
            line: 1,
            end_line: None,
            other_locations: Vec::new(),
            fingerprint: "in-diff".into(),
        };
        let source_level_outside_diff = CiIssue {
            rule_id: "fallow/unused-export".into(),
            description: "Export unused".into(),
            severity: "minor".into(),
            path: "src/b.ts".into(),
            line: 1,
            end_line: None,
            other_locations: Vec::new(),
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
            end_line: None,
            other_locations: Vec::new(),
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
            end_line: None,
            other_locations: Vec::new(),
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
            end_line: None,
            other_locations: Vec::new(),
            fingerprint: "a".into(),
        };
        let b = CiIssue {
            rule_id: "fallow/unused-dependency".into(),
            description: "b".into(),
            severity: "minor".into(),
            path: "package.json".into(),
            line: 5,
            end_line: None,
            other_locations: Vec::new(),
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
            end_line: None,
            other_locations: Vec::new(),
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

    #[test]
    fn added_mode_anchors_range_finding_to_lowest_added_line_inside_range() {
        let diff = "\
diff --git a/src/a.ts b/src/a.ts
--- a/src/a.ts
+++ b/src/a.ts
@@ -3,3 +3,5 @@
 context at three
+first added
 context at five
+second added
 context at seven
";
        let index = DiffIndex::from_unified_diff(diff);
        let ranged = CiIssue {
            rule_id: "fallow/code-duplication".into(),
            description: "clone".into(),
            severity: "minor".into(),
            path: "src/a.ts".into(),
            line: 3,
            end_line: Some(7),
            other_locations: Vec::new(),
            fingerprint: "range".into(),
        };
        let outside = CiIssue {
            line: 8,
            end_line: Some(10),
            fingerprint: "outside".into(),
            ..ranged.clone()
        };

        let kept =
            filter_issues_with_index(vec![outside, ranged], &index, DiffFilterMode::Added, 3);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].fingerprint, "range");
        assert_eq!(kept[0].line, 4);
        assert_eq!(kept[0].end_line, Some(7));

        let comment = fallow_output::render_review_comment_for_group(
            &fallow_output::ReviewCommentRenderInput {
                provider: fallow_output::CiProvider::Gitlab,
                group: &[&kept[0]],
                gitlab_diff_refs: None,
                diff_index: Some(&index),
                path_prefix: "",
                include_guidance: false,
                suggestion_block: &|_, _| None,
                guidance_block: &|_| None,
            },
        );
        let fallow_output::ReviewComment::GitLab(comment) = comment else {
            panic!("expected GitLab comment");
        };
        assert_eq!(comment.position.new_line, 4);
    }
}
