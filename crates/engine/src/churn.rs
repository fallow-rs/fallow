//! Git churn analysis for hotspot detection.
//!
//! Shells out to `git log` to collect per-file change history, then computes
//! recency-weighted churn scores and trend indicators.

use rustc_hash::FxHashMap;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use serde::Deserialize;

pub use fallow_types::churn::ChurnTrend;

/// Function pointer signature used by `set_spawn_hook` to intercept the
/// `git log --numstat` subprocess. Lets the CLI route long-running git
/// log calls through its `ScopedChild` registry so SIGINT / SIGTERM
/// reap the subprocess instead of leaving it running after the parent
/// exits. See `crates/cli/src/signal/` and issue #477.
pub type ChurnSpawnHook = fn(&mut Command) -> std::io::Result<Output>;

static SPAWN_HOOK: OnceLock<ChurnSpawnHook> = OnceLock::new();

#[expect(
    clippy::disallowed_methods,
    reason = "engine-owned git spawn wrapper clears ambient git env before churn subprocesses"
)]
fn git_command() -> Command {
    let mut command = Command::new("git");
    crate::git_env::clear_ambient_git_env(&mut command);
    command
}

/// Install a spawn-hook that wraps the `git log` subprocess. Idempotent;
/// subsequent calls are no-ops. Called once from the CLI's `main()` to
/// route through the signal registry; defaults to `Command::output`
/// when not set so the function-pointer indirection stays free for tests
/// and embedders that don't care.
pub fn set_spawn_hook(hook: ChurnSpawnHook) {
    let _ = SPAWN_HOOK.set(hook);
}

fn spawn_output(command: &mut Command) -> std::io::Result<Output> {
    if let Some(hook) = SPAWN_HOOK.get() {
        hook(command)
    } else {
        command.output()
    }
}

/// Number of seconds in one day.
const SECS_PER_DAY: f64 = 86_400.0;

/// Recency weight half-life in days. A commit from 90 days ago counts half
/// as much as today's commit; 180 days ago counts 25%.
const HALF_LIFE_DAYS: f64 = 90.0;

/// Schema discriminator a `--churn-file` document must declare.
const CHURN_FILE_SCHEMA: &str = "fallow-churn/v1";

/// Upper bound on imported churn events. A file past this size is a sign of a
/// pathological export (whole-history dump of a giant monorepo) rather than a
/// useful hotspot window; parsing is rejected so we never allocate unbounded
/// state from a single untrusted file. Mirrors the diff parser's
/// `MAX_ADDED_LINES` guard in the CLI.
const MAX_CHURN_EVENTS: usize = 5_000_000;

/// Upper bound on the serialized churn import before JSON deserialization.
const MAX_CHURN_FILE_BYTES: usize = 256 * 1024 * 1024;

/// Reject an imported `timestamp` more than this many seconds in the future
/// (one year). A unix-seconds commit time is never legitimately this far ahead
/// even with clock skew, so a value past it is almost always a millisecond
/// timestamp (~52000 years out) or corruption. Caught loudly because the
/// recency decay uses `saturating_sub`, so a future timestamp would otherwise
/// clamp to age 0, give every commit full weight, and silently collapse the
/// recency signal that distinguishes recent from old churn.
const MAX_FUTURE_TIMESTAMP_SECS: u64 = 365 * 24 * 60 * 60;

/// Parsed duration for the `--since` flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SinceDuration {
    /// Value to pass to `git log --after` (e.g., `"6 months ago"` or `"2025-06-01"`).
    pub git_after: String,
    /// Human-readable display string (e.g., `"6 months"`).
    pub display: String,
}

/// Per-author commit aggregation for a single file.
///
/// Authors are interned via [`ChurnResult::author_pool`] indices to keep
/// per-file maps small and the bitcode cache compact.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AuthorContribution {
    /// Total commits by this author touching this file in the analysis window.
    pub commits: u32,
    /// Recency-weighted commit sum (exponential decay, half-life 90 days).
    pub weighted_commits: f64,
    /// Earliest commit timestamp by this author (epoch seconds).
    pub first_commit_ts: u64,
    /// Latest commit timestamp by this author (epoch seconds).
    pub last_commit_ts: u64,
}

/// Per-file churn data collected from git history.
#[derive(Debug, Clone)]
pub struct FileChurn {
    /// Absolute file path.
    pub path: PathBuf,
    /// Total number of commits touching this file in the analysis window.
    pub commits: u32,
    /// Recency-weighted commit count (exponential decay, half-life 90 days).
    pub weighted_commits: f64,
    /// Total lines added across all commits.
    pub lines_added: u32,
    /// Total lines deleted across all commits.
    pub lines_deleted: u32,
    /// Churn trend: accelerating, stable, or cooling.
    pub trend: ChurnTrend,
    /// Per-author contributions keyed by interned author index.
    /// Indices reference [`ChurnResult::author_pool`].
    pub authors: FxHashMap<u32, AuthorContribution>,
}

/// Result of churn analysis.
#[derive(Debug, Clone)]
pub struct ChurnResult {
    /// Per-file churn data, keyed by absolute path.
    pub files: FxHashMap<PathBuf, FileChurn>,
    /// Whether the repository is a shallow clone.
    pub shallow_clone: bool,
    /// Author email pool. Per-file [`AuthorContribution`] entries reference
    /// authors by their index into this vector.
    pub author_pool: Vec<String>,
}

/// Parse a `--since` value into a git-compatible duration.
///
/// Accepts:
/// - Durations: `6m`, `6months`, `90d`, `90days`, `1y`, `1year`, `2w`, `2weeks`
/// - ISO dates: `2025-06-01`
///
/// # Errors
///
/// Returns an error if the input is not a recognized duration format or ISO date,
/// the numeric part is invalid, or the duration is zero.
pub fn parse_since(input: &str) -> Result<SinceDuration, String> {
    if is_iso_date(input) {
        return Ok(SinceDuration {
            git_after: input.to_string(),
            display: input.to_string(),
        });
    }

    let (num_str, unit) = split_number_unit(input)?;
    let num: u64 = num_str
        .parse()
        .map_err(|_| format!("invalid number in --since: {input}"))?;

    if num == 0 {
        return Err("--since duration must be greater than 0".to_string());
    }

    match unit {
        "d" | "day" | "days" => {
            let s = if num == 1 { "" } else { "s" };
            Ok(SinceDuration {
                git_after: format!("{num} day{s} ago"),
                display: format!("{num} day{s}"),
            })
        }
        "w" | "week" | "weeks" => {
            let s = if num == 1 { "" } else { "s" };
            Ok(SinceDuration {
                git_after: format!("{num} week{s} ago"),
                display: format!("{num} week{s}"),
            })
        }
        "m" | "month" | "months" => {
            let s = if num == 1 { "" } else { "s" };
            Ok(SinceDuration {
                git_after: format!("{num} month{s} ago"),
                display: format!("{num} month{s}"),
            })
        }
        "y" | "year" | "years" => {
            let s = if num == 1 { "" } else { "s" };
            Ok(SinceDuration {
                git_after: format!("{num} year{s} ago"),
                display: format!("{num} year{s}"),
            })
        }
        _ => Err(format!(
            "unknown duration unit '{unit}' in --since. Use d/w/m/y (e.g., 6m, 90d, 1y)"
        )),
    }
}

/// Analyze git churn for files in the given root directory.
///
/// Returns `None` if git is not available or the directory is not a git repository.
pub fn analyze_churn(root: &Path, since: &SinceDuration) -> Option<ChurnResult> {
    let shallow = is_shallow_clone(root);
    let state = analyze_churn_events(root, since, None)?;
    Some(build_churn_result(state, shallow))
}

/// A `fallow-churn/v1` import document: a normalized, VCS-agnostic stand-in for
/// `git log --numstat` output. Unknown fields are ignored (no
/// `deny_unknown_fields`) so wrappers may carry extra metadata and so the
/// reserved `commit` field can be added in a future revision without breaking
/// v1 consumers.
#[derive(Debug, Deserialize)]
struct ChurnFileDoc {
    schema: String,
    #[serde(default)]
    events: Vec<ChurnFileEvent>,
}

/// One per-(commit, file) change event, the natural shape of a `<vcs> log
/// --numstat` row. `commit` is intentionally NOT a field: extra keys are
/// already ignored, so a wrapper emitting `commit` is forward-compatible and a
/// future revision can promote it to a real field without a breaking change.
#[derive(Debug, Deserialize)]
struct ChurnFileEvent {
    /// Repo-root-relative, forward-slash path. Joined to `root`.
    path: String,
    /// Commit time, unix SECONDS UTC (not milliseconds).
    timestamp: u64,
    /// Opaque author identity (email recommended); absent contributes no
    /// ownership signal. fallow does NOT apply mailmap to imported authors.
    #[serde(default)]
    author: Option<String>,
    /// Lines added in this file in this commit.
    added: u32,
    /// Lines deleted in this file in this commit.
    deleted: u32,
}

/// Build churn data from a normalized `fallow-churn/v1` JSON import instead of
/// `git log`. Lets projects on a non-git VCS (Yandex Arc, Mercurial, Perforce)
/// feed change history into hotspot / ownership / bus-factor analysis: a small
/// wrapper translates the VCS log into the contract and fallow runs all the
/// usual recency-weighting, trend, and ownership logic on the imported events.
///
/// `root` is the project root that relative event paths are joined to (matching
/// how the git path joins numstat paths), so the churn keys line up with the
/// analyzed files. Returns a human-readable error (the CLI maps it to exit code
/// 2) on an oversized or missing file, malformed JSON, wrong `schema`, an
/// invalid repo-relative event path, a far-future timestamp, line totals above
/// `u32::MAX`, or an event count past `MAX_CHURN_EVENTS`. An empty `events`
/// array is valid (no hotspots), not an error. Never runs `git`.
pub fn analyze_churn_from_file(path: &Path, root: &Path) -> Result<ChurnResult, String> {
    let raw = read_churn_file_with_limit(path, MAX_CHURN_FILE_BYTES)?;
    let doc: ChurnFileDoc = serde_json::from_str(&raw)
        .map_err(|e| format!("failed to parse churn file {}: {e}", path.display()))?;
    if doc.schema != CHURN_FILE_SCHEMA {
        return Err(format!(
            "churn file {} declares schema \"{}\", expected \"{CHURN_FILE_SCHEMA}\"",
            path.display(),
            doc.schema
        ));
    }
    if doc.events.len() > MAX_CHURN_EVENTS {
        return Err(format!(
            "churn file {} has {} events, exceeding the {MAX_CHURN_EVENTS} limit",
            path.display(),
            doc.events.len()
        ));
    }

    let state = churn_event_state_from_doc(&doc, path, root)?;
    Ok(build_churn_result(state, false))
}

fn read_churn_file_with_limit(path: &Path, limit: usize) -> Result<String, String> {
    let file = std::fs::File::open(path)
        .map_err(|e| format!("failed to read churn file {}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("failed to read churn file {}: {e}", path.display()))?;
    if bytes.len() > limit {
        return Err(format!(
            "churn file {} is at least {} bytes, exceeding the {limit} byte limit",
            path.display(),
            bytes.len()
        ));
    }
    String::from_utf8(bytes)
        .map_err(|e| format!("failed to read churn file {} as UTF-8: {e}", path.display()))
}

/// Validate and fold a parsed `fallow-churn/v1` document into event state.
///
/// Rejects invalid paths, far-future (likely millisecond) timestamps, and line
/// totals outside the public `u32` contract. Interns authors into the pool
/// exactly as the git-log path does.
fn churn_event_state_from_doc(
    doc: &ChurnFileDoc,
    path: &Path,
    root: &Path,
) -> Result<ChurnEventState, String> {
    let mut builder = ChurnFileImportBuilder::new(path, root, churn_file_future_limit());

    for event in &doc.events {
        builder.push_event(event)?;
    }

    Ok(builder.finish())
}

fn churn_file_future_limit() -> u64 {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now_secs.saturating_add(MAX_FUTURE_TIMESTAMP_SECS)
}

struct ChurnFileImportBuilder<'a> {
    path: &'a Path,
    root: &'a Path,
    future_limit: u64,
    files: FxHashMap<PathBuf, FileEvents>,
    totals: FxHashMap<PathBuf, (u64, u64)>,
    author_pool: Vec<String>,
    author_index: FxHashMap<String, u32>,
}

impl<'a> ChurnFileImportBuilder<'a> {
    fn new(path: &'a Path, root: &'a Path, future_limit: u64) -> Self {
        Self {
            path,
            root,
            future_limit,
            files: FxHashMap::default(),
            totals: FxHashMap::default(),
            author_pool: Vec::new(),
            author_index: FxHashMap::default(),
        }
    }

    fn push_event(&mut self, event: &ChurnFileEvent) -> Result<(), String> {
        let rel = normalize_churn_event_path(self.path, &event.path)?;
        validate_churn_event_timestamp(self.path, event.timestamp, self.future_limit, &rel)?;

        let abs_path = self.root.join(&rel);
        let totals = self.totals.entry(abs_path.clone()).or_default();
        totals.0 += u64::from(event.added);
        totals.1 += u64::from(event.deleted);
        if totals.0 > u64::from(u32::MAX) || totals.1 > u64::from(u32::MAX) {
            return Err(format!(
                "churn file {} has line totals for \"{rel}\" exceeding the u32 limit \
                 (added {}, deleted {})",
                self.path.display(),
                totals.0,
                totals.1
            ));
        }
        let author_idx = self.intern_author(event.author.as_deref());
        self.files
            .entry(abs_path)
            .or_insert_with(|| FileEvents { events: Vec::new() })
            .events
            .push(CachedCommitEvent {
                timestamp: event.timestamp,
                lines_added: event.added,
                lines_deleted: event.deleted,
                author_idx,
            });
        Ok(())
    }

    fn intern_author(&mut self, author: Option<&str>) -> Option<u32> {
        author
            .map(str::trim)
            .filter(|email| !email.is_empty())
            .map(|email| intern_author(email, &mut self.author_pool, &mut self.author_index))
    }

    fn finish(self) -> ChurnEventState {
        ChurnEventState {
            files: self.files,
            author_pool: self.author_pool,
        }
    }
}

fn normalize_churn_event_path(path: &Path, event_path: &str) -> Result<String, String> {
    let normalized = event_path.replace('\\', "/");
    let rel = normalized.trim();
    if rel.is_empty() {
        return Err(format!(
            "churn file {} has an event with an empty path",
            path.display()
        ));
    }
    let bytes = rel.as_bytes();
    let has_drive_prefix = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    let has_empty_component = rel.split('/').any(str::is_empty);
    let components_are_normal = Path::new(rel)
        .components()
        .all(|component| matches!(component, Component::Normal(_)));
    if rel.starts_with('/') || has_drive_prefix || has_empty_component || !components_are_normal {
        return Err(format!(
            "churn file {} has an invalid repo-relative event path \"{event_path}\"",
            path.display()
        ));
    }
    Ok(rel.to_string())
}

fn validate_churn_event_timestamp(
    path: &Path,
    timestamp: u64,
    future_limit: u64,
    rel: &str,
) -> Result<(), String> {
    if timestamp <= future_limit {
        return Ok(());
    }

    Err(format!(
        "churn file {} has event timestamp {} for \"{rel}\" more than a year in the \
         future; timestamps must be unix SECONDS (not milliseconds), UTC",
        path.display(),
        timestamp
    ))
}

/// Check if the repository is a shallow clone.
#[must_use]
pub fn is_shallow_clone(root: &Path) -> bool {
    let mut command = git_command();
    command
        .args(["rev-parse", "--is-shallow-repository"])
        .current_dir(root);
    command.output().is_ok_and(|o| {
        String::from_utf8_lossy(&o.stdout)
            .trim()
            .eq_ignore_ascii_case("true")
    })
}

/// Check if the directory is inside a git repository.
#[must_use]
pub fn is_git_repo(root: &Path) -> bool {
    let mut command = git_command();
    command
        .args(["rev-parse", "--git-dir"])
        .current_dir(root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    command.status().is_ok_and(|s| s.success())
}

/// Maximum size of a churn cache file (64 MB). The incremental cache stores
/// per-commit events, so it needs more headroom than the old aggregate rows.
const MAX_CHURN_CACHE_SIZE: usize = 64 * 1024 * 1024;

/// Cache schema version. Bump when the on-disk shape of [`ChurnCache`]
/// changes so older payloads are rejected on load. Version 5 stores paths in
/// their platform-native byte representation instead of lossy UTF-8 strings.
const CHURN_CACHE_VERSION: u8 = 5;

/// Serializable per-commit event for the disk cache.
#[derive(Clone, bitcode::Encode, bitcode::Decode)]
struct CachedCommitEvent {
    timestamp: u64,
    lines_added: u32,
    lines_deleted: u32,
    author_idx: Option<u32>,
}

/// Serializable per-file churn entry for the disk cache.
#[derive(Clone, bitcode::Encode, bitcode::Decode)]
struct CachedFileChurn {
    path: Vec<u8>,
    events: Vec<CachedCommitEvent>,
}

/// Cached churn data keyed by last indexed SHA and since string.
#[derive(Clone, bitcode::Encode, bitcode::Decode)]
struct ChurnCache {
    /// Schema version; must equal [`CHURN_CACHE_VERSION`] to be accepted.
    version: u8,
    last_indexed_sha: String,
    git_after: String,
    files: Vec<CachedFileChurn>,
    shallow_clone: bool,
    /// Author email pool referenced by [`CachedCommitEvent::author_idx`].
    author_pool: Vec<String>,
}

/// Per-file commit events retained in memory while building or updating churn.
struct FileEvents {
    events: Vec<CachedCommitEvent>,
}

/// Event-level churn state. Unlike [`ChurnResult`], this preserves commit
/// timestamps so a cache can merge new commits and recompute trend/recency.
struct ChurnEventState {
    files: FxHashMap<PathBuf, FileEvents>,
    author_pool: Vec<String>,
}

/// Get the full HEAD SHA for cache keying.
fn get_head_sha(root: &Path) -> Option<String> {
    let mut command = git_command();
    command.args(["rev-parse", "HEAD"]).current_dir(root);
    command
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Check whether `ancestor` is still reachable from `descendant`.
fn is_ancestor(root: &Path, ancestor: &str, descendant: &str) -> bool {
    let mut command = git_command();
    command
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .current_dir(root);
    command.status().is_ok_and(|s| s.success())
}

/// Try to load churn data from disk cache. Returns `None` on cache miss
/// or version mismatch.
fn load_churn_cache(cache_dir: &Path, git_after: &str) -> Option<ChurnCache> {
    let cache_file = cache_dir.join("churn.bin");
    let data = std::fs::read(&cache_file).ok()?;
    if data.len() > MAX_CHURN_CACHE_SIZE {
        return None;
    }
    let cache: ChurnCache = bitcode::decode(&data).ok()?;
    if cache.version != CHURN_CACHE_VERSION || cache.git_after != git_after {
        return None;
    }
    Some(cache)
}

/// Save churn data to disk cache.
fn save_churn_cache(
    cache_dir: &Path,
    last_indexed_sha: &str,
    git_after: &str,
    state: &ChurnEventState,
    shallow_clone: bool,
) {
    let files: Vec<CachedFileChurn> = state
        .files
        .iter()
        .map(|f| CachedFileChurn {
            path: path_to_cache_bytes(f.0),
            events: f.1.events.clone(),
        })
        .collect();
    let cache = ChurnCache {
        version: CHURN_CACHE_VERSION,
        last_indexed_sha: last_indexed_sha.to_string(),
        git_after: git_after.to_string(),
        files,
        shallow_clone,
        author_pool: state.author_pool.clone(),
    };
    let _ = std::fs::create_dir_all(cache_dir);
    let data = bitcode::encode(&cache);
    let tmp = cache_dir.join("churn.bin.tmp");
    if std::fs::write(&tmp, data).is_ok() {
        let _ = std::fs::rename(&tmp, cache_dir.join("churn.bin"));
    }
}

/// Analyze churn with disk caching. Uses cached result when HEAD SHA and
/// since duration match. If HEAD advanced from the cached SHA, runs an
/// incremental `git log <cached>..HEAD --numstat` scan and merges it.
///
/// Returns `(ChurnResult, bool)` where the bool indicates whether reusable
/// cache state was used.
/// Returns `None` if git analysis fails.
pub fn analyze_churn_cached(
    root: &Path,
    since: &SinceDuration,
    cache_dir: &Path,
    no_cache: bool,
) -> Option<(ChurnResult, bool)> {
    let head_sha = get_head_sha(root)?;

    if !no_cache && let Some(result) = try_reuse_churn_cache(root, since, cache_dir, &head_sha) {
        return Some((result, true));
    }

    analyze_fresh_churn(root, since, cache_dir, no_cache, &head_sha).map(|result| (result, false))
}

fn try_reuse_churn_cache(
    root: &Path,
    since: &SinceDuration,
    cache_dir: &Path,
    head_sha: &str,
) -> Option<ChurnResult> {
    let cache = load_churn_cache(cache_dir, &since.git_after)?;
    if cache.last_indexed_sha == head_sha {
        let shallow_clone = cache.shallow_clone;
        return Some(build_churn_result(cache.into_event_state(), shallow_clone));
    }

    if !is_ancestor(root, &cache.last_indexed_sha, head_sha) {
        return None;
    }

    extend_churn_cache(root, since, cache_dir, head_sha, cache)
}

fn extend_churn_cache(
    root: &Path,
    since: &SinceDuration,
    cache_dir: &Path,
    head_sha: &str,
    cache: ChurnCache,
) -> Option<ChurnResult> {
    let shallow_clone = is_shallow_clone(root);
    let range = format!("{}..HEAD", cache.last_indexed_sha);
    let delta = analyze_churn_events(root, since, Some(&range))?;
    let mut state = cache.into_event_state();
    merge_churn_states(&mut state, delta);
    save_churn_cache(cache_dir, head_sha, &since.git_after, &state, shallow_clone);
    Some(build_churn_result(state, shallow_clone))
}

fn analyze_fresh_churn(
    root: &Path,
    since: &SinceDuration,
    cache_dir: &Path,
    no_cache: bool,
    head_sha: &str,
) -> Option<ChurnResult> {
    let shallow_clone = is_shallow_clone(root);
    let state = analyze_churn_events(root, since, None)?;
    if !no_cache {
        save_churn_cache(cache_dir, head_sha, &since.git_after, &state, shallow_clone);
    }

    Some(build_churn_result(state, shallow_clone))
}

impl ChurnCache {
    fn into_event_state(self) -> ChurnEventState {
        let files = self
            .files
            .into_iter()
            .filter_map(|entry| {
                path_from_cache_bytes(&entry.path).map(|path| {
                    (
                        path,
                        FileEvents {
                            events: entry.events,
                        },
                    )
                })
            })
            .collect();
        ChurnEventState {
            files,
            author_pool: self.author_pool,
        }
    }
}

#[cfg(unix)]
fn path_to_cache_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
#[allow(
    clippy::unnecessary_wraps,
    reason = "Windows rejects truncated UTF-16 bytes through this shared fallible contract"
)]
fn path_from_cache_bytes(path: &[u8]) -> Option<PathBuf> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    Some(PathBuf::from(OsStr::from_bytes(path)))
}

#[cfg(windows)]
fn path_to_cache_bytes(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(windows)]
fn path_from_cache_bytes(path: &[u8]) -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    let chunks = path.chunks_exact(2);
    if !chunks.remainder().is_empty() {
        return None;
    }
    let wide: Vec<u16> = chunks
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();
    Some(PathBuf::from(OsString::from_wide(&wide)))
}

/// Run `git log --numstat` and return event-level churn state.
fn analyze_churn_events(
    root: &Path,
    since: &SinceDuration,
    revision_range: Option<&str>,
) -> Option<ChurnEventState> {
    let mut command = git_command();
    command.arg("log");
    if let Some(range) = revision_range {
        command.arg(range);
    }
    command
        .args([
            "--numstat",
            "--no-merges",
            "--no-renames",
            "--use-mailmap",
            "-z",
            "--format=format:%at|%ae%x00",
            &format!("--after={}", since.git_after),
        ])
        .current_dir(root);

    let output = match spawn_output(&mut command) {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("hotspot analysis skipped: failed to run git: {e}");
            return None;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::warn!("hotspot analysis skipped: git log failed: {stderr}");
        return None;
    }

    Some(parse_git_log_events_z(&output.stdout, root))
}

/// Merge new churn events into cached event state.
fn merge_churn_states(base: &mut ChurnEventState, delta: ChurnEventState) {
    let mut base_author_index: FxHashMap<String, u32> = base
        .author_pool
        .iter()
        .enumerate()
        .filter_map(|(idx, email)| u32::try_from(idx).ok().map(|idx| (email.clone(), idx)))
        .collect();

    let mut author_mapping: FxHashMap<u32, u32> = FxHashMap::default();
    for (old_idx, email) in delta.author_pool.into_iter().enumerate() {
        let Ok(old_idx) = u32::try_from(old_idx) else {
            continue;
        };
        let new_idx = intern_author(&email, &mut base.author_pool, &mut base_author_index);
        author_mapping.insert(old_idx, new_idx);
    }

    for (path, mut file) in delta.files {
        for event in &mut file.events {
            event.author_idx = event
                .author_idx
                .and_then(|idx| author_mapping.get(&idx).copied());
        }
        base.files
            .entry(path)
            .and_modify(|existing| existing.events.append(&mut file.events))
            .or_insert(file);
    }
}

/// Parse `git log --numstat --format=format:%at|%ae` output into events.
#[cfg(test)]
fn parse_git_log_events(stdout: &str, root: &Path) -> ChurnEventState {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut parser = GitLogEventParser::new(root, now_secs);

    for line in stdout.lines() {
        parser.consume_line(line);
    }

    parser.finish()
}

fn parse_git_log_events_z(stdout: &[u8], root: &Path) -> ChurnEventState {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut parser = GitLogEventParser::new(root, now_secs);
    for record in stdout.split(|byte| *byte == 0) {
        let record = record.strip_prefix(b"\n").unwrap_or(record);
        if record.is_empty() {
            continue;
        }
        if record.contains(&b'\t') {
            parser.record_numstat_bytes(record);
        } else {
            parser.consume_line(&String::from_utf8_lossy(record));
        }
    }
    parser.finish()
}

struct GitLogEventParser<'a> {
    root: &'a Path,
    now_secs: u64,
    files: FxHashMap<PathBuf, FileEvents>,
    author_pool: Vec<String>,
    author_index: FxHashMap<String, u32>,
    current_timestamp: Option<u64>,
    current_author_idx: Option<u32>,
}

impl<'a> GitLogEventParser<'a> {
    fn new(root: &'a Path, now_secs: u64) -> Self {
        Self {
            root,
            now_secs,
            files: FxHashMap::default(),
            author_pool: Vec::new(),
            author_index: FxHashMap::default(),
            current_timestamp: None,
            current_author_idx: None,
        }
    }

    fn consume_line(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }

        if self.record_commit_header(line) {
            return;
        }
        if self.record_legacy_timestamp(line) {
            return;
        }
        self.record_numstat(line);
    }

    fn record_commit_header(&mut self, line: &str) -> bool {
        let Some((ts_str, email)) = line.split_once('|') else {
            return false;
        };
        let Ok(ts) = ts_str.parse::<u64>() else {
            return false;
        };

        self.current_timestamp = Some(ts);
        self.current_author_idx = Some(intern_author(
            email,
            &mut self.author_pool,
            &mut self.author_index,
        ));
        true
    }

    fn record_legacy_timestamp(&mut self, line: &str) -> bool {
        let Ok(ts) = line.parse::<u64>() else {
            return false;
        };

        self.current_timestamp = Some(ts);
        self.current_author_idx = None;
        true
    }

    fn record_numstat(&mut self, line: &str) {
        let Some((added, deleted, path)) = parse_numstat_line(line) else {
            return;
        };

        self.record_numstat_path(added, deleted, PathBuf::from(path));
    }

    fn record_numstat_bytes(&mut self, record: &[u8]) {
        let Some(first_tab) = record.iter().position(|byte| *byte == b'\t') else {
            return;
        };
        let Some(second_tab_offset) = record[first_tab + 1..]
            .iter()
            .position(|byte| *byte == b'\t')
        else {
            return;
        };
        let second_tab = first_tab + 1 + second_tab_offset;
        let Some(added) = std::str::from_utf8(&record[..first_tab])
            .ok()
            .and_then(|value| value.parse().ok())
        else {
            return;
        };
        let Some(deleted) = std::str::from_utf8(&record[first_tab + 1..second_tab])
            .ok()
            .and_then(|value| value.parse().ok())
        else {
            return;
        };
        let path = git_path_from_bytes(&record[second_tab + 1..]);
        self.record_numstat_path(added, deleted, path);
    }

    fn record_numstat_path(&mut self, added: u32, deleted: u32, path: PathBuf) {
        let ts = self.current_timestamp.unwrap_or(self.now_secs);
        self.files
            .entry(self.root.join(path))
            .or_insert_with(|| FileEvents { events: Vec::new() })
            .events
            .push(CachedCommitEvent {
                timestamp: ts,
                lines_added: added,
                lines_deleted: deleted,
                author_idx: self.current_author_idx,
            });
    }

    fn finish(self) -> ChurnEventState {
        ChurnEventState {
            files: self.files,
            author_pool: self.author_pool,
        }
    }
}

#[cfg(unix)]
fn git_path_from_bytes(path: &[u8]) -> PathBuf {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    PathBuf::from(OsString::from_vec(path.to_vec()))
}

#[cfg(windows)]
fn git_path_from_bytes(path: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(path).replace('/', "\\"))
}

/// Aggregate one file's raw commit events into a [`FileChurn`], applying
/// recency weighting, trend detection, and per-author accumulation.
#[expect(
    clippy::cast_possible_truncation,
    reason = "commit count per file is bounded by git history depth"
)]
fn aggregate_file_churn(path: PathBuf, file: FileEvents, now_secs: u64) -> FileChurn {
    let mut timestamps = Vec::with_capacity(file.events.len());
    let mut weighted_commits = 0.0;
    let mut lines_added = 0_u32;
    let mut lines_deleted = 0_u32;
    let mut authors: FxHashMap<u32, AuthorContribution> = FxHashMap::default();

    for event in file.events {
        timestamps.push(event.timestamp);
        let age_days = (now_secs.saturating_sub(event.timestamp)) as f64 / SECS_PER_DAY;
        let weight = 0.5_f64.powf(age_days / HALF_LIFE_DAYS);
        weighted_commits += weight;
        lines_added = lines_added.saturating_add(event.lines_added);
        lines_deleted = lines_deleted.saturating_add(event.lines_deleted);
        accumulate_author(&mut authors, event.author_idx, weight, event.timestamp);
    }

    let commits = timestamps.len() as u32;
    let trend = compute_trend(&timestamps);
    for c in authors.values_mut() {
        c.weighted_commits = (c.weighted_commits * 100.0).round() / 100.0;
    }
    FileChurn {
        path,
        commits,
        weighted_commits: (weighted_commits * 100.0).round() / 100.0,
        lines_added,
        lines_deleted,
        trend,
        authors,
    }
}

/// Fold a single commit's author contribution into the per-author map.
fn accumulate_author(
    authors: &mut FxHashMap<u32, AuthorContribution>,
    author_idx: Option<u32>,
    weight: f64,
    timestamp: u64,
) {
    let Some(idx) = author_idx else {
        return;
    };
    authors
        .entry(idx)
        .and_modify(|c| {
            c.commits += 1;
            c.weighted_commits += weight;
            c.first_commit_ts = c.first_commit_ts.min(timestamp);
            c.last_commit_ts = c.last_commit_ts.max(timestamp);
        })
        .or_insert(AuthorContribution {
            commits: 1,
            weighted_commits: weight,
            first_commit_ts: timestamp,
            last_commit_ts: timestamp,
        });
}

/// Convert event-level churn state into the public aggregate result.
fn build_churn_result(state: ChurnEventState, shallow_clone: bool) -> ChurnResult {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let files = state
        .files
        .into_iter()
        .map(|(path, file)| {
            let churn = aggregate_file_churn(path.clone(), file, now_secs);
            (path, churn)
        })
        .collect();

    ChurnResult {
        files,
        shallow_clone,
        author_pool: state.author_pool,
    }
}

/// Parse `git log --numstat --format=format:%at|%ae` output.
///
/// Returns a per-file churn map plus the author email pool referenced by
/// interned indices in [`FileChurn::authors`].
#[cfg(test)]
fn parse_git_log(stdout: &str, root: &Path) -> (FxHashMap<PathBuf, FileChurn>, Vec<String>) {
    let result = build_churn_result(parse_git_log_events(stdout, root), false);
    (result.files, result.author_pool)
}

/// Intern an author email into the pool, returning its stable index.
fn intern_author(email: &str, pool: &mut Vec<String>, index: &mut FxHashMap<String, u32>) -> u32 {
    if let Some(&idx) = index.get(email) {
        return idx;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "author count is bounded by git history; u32 is far above any realistic ceiling"
    )]
    let idx = pool.len() as u32;
    let owned = email.to_string();
    index.insert(owned.clone(), idx);
    pool.push(owned);
    idx
}

/// Parse a single numstat line: `"10\t5\tpath/to/file.ts"`.
/// Binary files show as `"-\t-\tpath"`, skip those.
fn parse_numstat_line(line: &str) -> Option<(u32, u32, &str)> {
    let mut parts = line.splitn(3, '\t');
    let added_str = parts.next()?;
    let deleted_str = parts.next()?;
    let path = parts.next()?;

    let added: u32 = added_str.parse().ok()?;
    let deleted: u32 = deleted_str.parse().ok()?;

    Some((added, deleted, path))
}

/// Compute churn trend by splitting commits into two temporal halves.
///
/// Finds the midpoint between the oldest and newest commit timestamps,
/// then compares commit counts in each half:
/// - Recent > 1.5× older → Accelerating
/// - Recent < 0.67× older → Cooling
/// - Otherwise → Stable
fn compute_trend(timestamps: &[u64]) -> ChurnTrend {
    if timestamps.len() < 2 {
        return ChurnTrend::Stable;
    }

    let min_ts = timestamps.iter().copied().min().unwrap_or(0);
    let max_ts = timestamps.iter().copied().max().unwrap_or(0);

    if max_ts == min_ts {
        return ChurnTrend::Stable;
    }

    let midpoint = min_ts + (max_ts - min_ts) / 2;
    let recent = timestamps.iter().filter(|&&ts| ts > midpoint).count() as f64;
    let older = timestamps.iter().filter(|&&ts| ts <= midpoint).count() as f64;

    if older < 1.0 {
        return ChurnTrend::Stable;
    }

    let ratio = recent / older;
    if ratio > 1.5 {
        ChurnTrend::Accelerating
    } else if ratio < 0.67 {
        ChurnTrend::Cooling
    } else {
        ChurnTrend::Stable
    }
}

fn is_iso_date(input: &str) -> bool {
    input.len() == 10
        && input.as_bytes().get(4) == Some(&b'-')
        && input.as_bytes().get(7) == Some(&b'-')
        && input[..4].bytes().all(|b| b.is_ascii_digit())
        && input[5..7].bytes().all(|b| b.is_ascii_digit())
        && input[8..10].bytes().all(|b| b.is_ascii_digit())
}

fn split_number_unit(input: &str) -> Result<(&str, &str), String> {
    let pos = input.find(|c: char| !c.is_ascii_digit()).ok_or_else(|| {
        format!("--since requires a unit suffix (e.g., 6m, 90d, 1y), got: {input}")
    })?;
    if pos == 0 {
        return Err(format!(
            "--since must start with a number (e.g., 6m, 90d, 1y), got: {input}"
        ));
    }
    Ok((&input[..pos], &input[pos..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_since_months_short() {
        let d = parse_since("6m").unwrap();
        assert_eq!(d.git_after, "6 months ago");
        assert_eq!(d.display, "6 months");
    }

    #[test]
    fn parse_since_months_long() {
        let d = parse_since("6months").unwrap();
        assert_eq!(d.git_after, "6 months ago");
        assert_eq!(d.display, "6 months");
    }

    #[test]
    fn parse_since_days() {
        let d = parse_since("90d").unwrap();
        assert_eq!(d.git_after, "90 days ago");
        assert_eq!(d.display, "90 days");
    }

    #[test]
    fn parse_since_year_singular() {
        let d = parse_since("1y").unwrap();
        assert_eq!(d.git_after, "1 year ago");
        assert_eq!(d.display, "1 year");
    }

    #[test]
    fn parse_since_years_plural() {
        let d = parse_since("2years").unwrap();
        assert_eq!(d.git_after, "2 years ago");
        assert_eq!(d.display, "2 years");
    }

    #[test]
    fn parse_since_weeks() {
        let d = parse_since("2w").unwrap();
        assert_eq!(d.git_after, "2 weeks ago");
        assert_eq!(d.display, "2 weeks");
    }

    #[test]
    fn parse_since_iso_date() {
        let d = parse_since("2025-06-01").unwrap();
        assert_eq!(d.git_after, "2025-06-01");
        assert_eq!(d.display, "2025-06-01");
    }

    #[test]
    fn parse_since_month_singular() {
        let d = parse_since("1month").unwrap();
        assert_eq!(d.display, "1 month");
    }

    #[test]
    fn parse_since_day_singular() {
        let d = parse_since("1day").unwrap();
        assert_eq!(d.display, "1 day");
    }

    #[test]
    fn parse_since_zero_rejected() {
        assert!(parse_since("0m").is_err());
    }

    #[test]
    fn parse_since_no_unit_rejected() {
        assert!(parse_since("90").is_err());
    }

    #[test]
    fn parse_since_unknown_unit_rejected() {
        assert!(parse_since("6x").is_err());
    }

    #[test]
    fn parse_since_no_number_rejected() {
        assert!(parse_since("months").is_err());
    }

    #[test]
    fn numstat_normal() {
        let (a, d, p) = parse_numstat_line("10\t5\tsrc/file.ts").unwrap();
        assert_eq!(a, 10);
        assert_eq!(d, 5);
        assert_eq!(p, "src/file.ts");
    }

    #[test]
    fn numstat_binary_skipped() {
        assert!(parse_numstat_line("-\t-\tsrc/image.png").is_none());
    }

    #[test]
    fn numstat_zero_lines() {
        let (a, d, p) = parse_numstat_line("0\t0\tsrc/empty.ts").unwrap();
        assert_eq!(a, 0);
        assert_eq!(d, 0);
        assert_eq!(p, "src/empty.ts");
    }

    #[test]
    fn trend_empty_is_stable() {
        assert_eq!(compute_trend(&[]), ChurnTrend::Stable);
    }

    #[test]
    fn trend_single_commit_is_stable() {
        assert_eq!(compute_trend(&[100]), ChurnTrend::Stable);
    }

    #[test]
    fn trend_accelerating() {
        let timestamps = vec![100, 200, 800, 850, 900, 950, 1000];
        assert_eq!(compute_trend(&timestamps), ChurnTrend::Accelerating);
    }

    #[test]
    fn trend_cooling() {
        let timestamps = vec![100, 150, 200, 250, 300, 900, 1000];
        assert_eq!(compute_trend(&timestamps), ChurnTrend::Cooling);
    }

    #[test]
    fn trend_stable_even_distribution() {
        let timestamps = vec![100, 200, 300, 700, 800, 900];
        assert_eq!(compute_trend(&timestamps), ChurnTrend::Stable);
    }

    #[test]
    fn trend_same_timestamp_is_stable() {
        let timestamps = vec![500, 500, 500];
        assert_eq!(compute_trend(&timestamps), ChurnTrend::Stable);
    }

    #[test]
    fn iso_date_valid() {
        assert!(is_iso_date("2025-06-01"));
        assert!(is_iso_date("2025-12-31"));
    }

    #[test]
    fn iso_date_with_time_rejected() {
        assert!(!is_iso_date("2025-06-01T00:00:00"));
    }

    #[test]
    fn iso_date_invalid() {
        assert!(!is_iso_date("6months"));
        assert!(!is_iso_date("2025"));
        assert!(!is_iso_date("not-a-date"));
        assert!(!is_iso_date("abcd-ef-gh"));
    }

    #[test]
    fn trend_display() {
        assert_eq!(ChurnTrend::Accelerating.to_string(), "accelerating");
        assert_eq!(ChurnTrend::Stable.to_string(), "stable");
        assert_eq!(ChurnTrend::Cooling.to_string(), "cooling");
    }

    #[test]
    fn parse_git_log_single_commit() {
        let root = Path::new("/project");
        let output = "1700000000\n10\t5\tsrc/index.ts\n";
        let (result, _) = parse_git_log(output, root);
        assert_eq!(result.len(), 1);
        let churn = &result[&PathBuf::from("/project/src/index.ts")];
        assert_eq!(churn.commits, 1);
        assert_eq!(churn.lines_added, 10);
        assert_eq!(churn.lines_deleted, 5);
    }

    #[test]
    fn parse_git_log_multiple_commits_same_file() {
        let root = Path::new("/project");
        let output = "1700000000\n10\t5\tsrc/index.ts\n\n1700100000\n3\t2\tsrc/index.ts\n";
        let (result, _) = parse_git_log(output, root);
        assert_eq!(result.len(), 1);
        let churn = &result[&PathBuf::from("/project/src/index.ts")];
        assert_eq!(churn.commits, 2);
        assert_eq!(churn.lines_added, 13);
        assert_eq!(churn.lines_deleted, 7);
    }

    #[test]
    fn parse_git_log_multiple_files() {
        let root = Path::new("/project");
        let output = "1700000000\n10\t5\tsrc/a.ts\n3\t1\tsrc/b.ts\n";
        let (result, _) = parse_git_log(output, root);
        assert_eq!(result.len(), 2);
        assert!(result.contains_key(&PathBuf::from("/project/src/a.ts")));
        assert!(result.contains_key(&PathBuf::from("/project/src/b.ts")));
    }

    #[test]
    fn parse_git_log_empty_output() {
        let root = Path::new("/project");
        let (result, _) = parse_git_log("", root);
        assert!(result.is_empty());
    }

    #[test]
    fn parse_git_log_skips_binary_files() {
        let root = Path::new("/project");
        let output = "1700000000\n-\t-\timage.png\n10\t5\tsrc/a.ts\n";
        let (result, _) = parse_git_log(output, root);
        assert_eq!(result.len(), 1);
        assert!(!result.contains_key(&PathBuf::from("/project/image.png")));
    }

    #[test]
    fn parse_git_log_weighted_commits_are_positive() {
        let root = Path::new("/project");
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let output = format!("{now_secs}\n10\t5\tsrc/a.ts\n");
        let (result, _) = parse_git_log(&output, root);
        let churn = &result[&PathBuf::from("/project/src/a.ts")];
        assert!(
            churn.weighted_commits > 0.0,
            "weighted_commits should be positive for recent commits"
        );
    }

    #[test]
    fn trend_boundary_1_5x_ratio() {
        let timestamps = vec![100, 200, 600, 800, 1000];
        assert_eq!(compute_trend(&timestamps), ChurnTrend::Stable);
    }

    #[test]
    fn trend_just_above_1_5x() {
        let timestamps = vec![100, 600, 800, 1000];
        assert_eq!(compute_trend(&timestamps), ChurnTrend::Accelerating);
    }

    #[test]
    fn trend_boundary_0_67x_ratio() {
        let timestamps = vec![100, 200, 300, 600, 1000];
        assert_eq!(compute_trend(&timestamps), ChurnTrend::Cooling);
    }

    #[test]
    fn trend_two_timestamps_different() {
        let timestamps = vec![100, 200];
        assert_eq!(compute_trend(&timestamps), ChurnTrend::Stable);
    }

    #[test]
    fn parse_since_week_singular() {
        let d = parse_since("1week").unwrap();
        assert_eq!(d.git_after, "1 week ago");
        assert_eq!(d.display, "1 week");
    }

    #[test]
    fn parse_since_weeks_long() {
        let d = parse_since("3weeks").unwrap();
        assert_eq!(d.git_after, "3 weeks ago");
        assert_eq!(d.display, "3 weeks");
    }

    #[test]
    fn parse_since_days_long() {
        let d = parse_since("30days").unwrap();
        assert_eq!(d.git_after, "30 days ago");
        assert_eq!(d.display, "30 days");
    }

    #[test]
    fn parse_since_year_long() {
        let d = parse_since("1year").unwrap();
        assert_eq!(d.git_after, "1 year ago");
        assert_eq!(d.display, "1 year");
    }

    #[test]
    fn parse_since_overflow_number_rejected() {
        let result = parse_since("99999999999999999999d");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("invalid number"));
    }

    #[test]
    fn parse_since_zero_days_rejected() {
        assert!(parse_since("0d").is_err());
    }

    #[test]
    fn parse_since_zero_weeks_rejected() {
        assert!(parse_since("0w").is_err());
    }

    #[test]
    fn parse_since_zero_years_rejected() {
        assert!(parse_since("0y").is_err());
    }

    #[test]
    fn numstat_missing_path() {
        assert!(parse_numstat_line("10\t5").is_none());
    }

    #[test]
    fn numstat_single_field() {
        assert!(parse_numstat_line("10").is_none());
    }

    #[test]
    fn numstat_empty_string() {
        assert!(parse_numstat_line("").is_none());
    }

    #[test]
    fn numstat_only_added_is_binary() {
        assert!(parse_numstat_line("-\t5\tsrc/file.ts").is_none());
    }

    #[test]
    fn numstat_only_deleted_is_binary() {
        assert!(parse_numstat_line("10\t-\tsrc/file.ts").is_none());
    }

    #[test]
    fn numstat_path_with_spaces() {
        let (a, d, p) = parse_numstat_line("3\t1\tpath with spaces/file.ts").unwrap();
        assert_eq!(a, 3);
        assert_eq!(d, 1);
        assert_eq!(p, "path with spaces/file.ts");
    }

    #[test]
    fn numstat_large_numbers() {
        let (a, d, p) = parse_numstat_line("9999\t8888\tsrc/big.ts").unwrap();
        assert_eq!(a, 9999);
        assert_eq!(d, 8888);
        assert_eq!(p, "src/big.ts");
    }

    #[test]
    fn iso_date_wrong_separator_positions() {
        assert!(!is_iso_date("20-25-0601"));
        assert!(!is_iso_date("202506-01-"));
    }

    #[test]
    fn iso_date_too_short() {
        assert!(!is_iso_date("2025-06-0"));
    }

    #[test]
    fn iso_date_letters_in_day() {
        assert!(!is_iso_date("2025-06-ab"));
    }

    #[test]
    fn iso_date_letters_in_month() {
        assert!(!is_iso_date("2025-ab-01"));
    }

    #[test]
    fn split_number_unit_valid() {
        let (num, unit) = split_number_unit("42days").unwrap();
        assert_eq!(num, "42");
        assert_eq!(unit, "days");
    }

    #[test]
    fn split_number_unit_single_digit() {
        let (num, unit) = split_number_unit("1m").unwrap();
        assert_eq!(num, "1");
        assert_eq!(unit, "m");
    }

    #[test]
    fn split_number_unit_no_digits() {
        let err = split_number_unit("abc").unwrap_err();
        assert!(err.contains("must start with a number"));
    }

    #[test]
    fn split_number_unit_no_unit() {
        let err = split_number_unit("123").unwrap_err();
        assert!(err.contains("requires a unit suffix"));
    }

    #[test]
    fn parse_git_log_numstat_before_timestamp_uses_now() {
        let root = Path::new("/project");
        let output = "10\t5\tsrc/no_ts.ts\n";
        let (result, _) = parse_git_log(output, root);
        assert_eq!(result.len(), 1);
        let churn = &result[&PathBuf::from("/project/src/no_ts.ts")];
        assert_eq!(churn.commits, 1);
        assert_eq!(churn.lines_added, 10);
        assert_eq!(churn.lines_deleted, 5);
        assert!(
            churn.weighted_commits > 0.9,
            "weight should be near 1.0 when timestamp defaults to now"
        );
    }

    #[test]
    fn parse_git_log_whitespace_lines_ignored() {
        let root = Path::new("/project");
        let output = "  \n1700000000\n  \n10\t5\tsrc/a.ts\n  \n";
        let (result, _) = parse_git_log(output, root);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn parse_git_log_trend_is_computed_per_file() {
        let root = Path::new("/project");
        let output = "\
1000\n5\t1\tsrc/old.ts\n\
2000\n3\t1\tsrc/old.ts\n\
1000\n1\t0\tsrc/hot.ts\n\
1800\n1\t0\tsrc/hot.ts\n\
1900\n1\t0\tsrc/hot.ts\n\
1950\n1\t0\tsrc/hot.ts\n\
2000\n1\t0\tsrc/hot.ts\n";
        let (result, _) = parse_git_log(output, root);
        let old = &result[&PathBuf::from("/project/src/old.ts")];
        let hot = &result[&PathBuf::from("/project/src/hot.ts")];
        assert_eq!(old.commits, 2);
        assert_eq!(hot.commits, 5);
        assert_eq!(hot.trend, ChurnTrend::Accelerating);
    }

    #[test]
    fn parse_git_log_weighted_decay_for_old_commits() {
        let root = Path::new("/project");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let old_ts = now - (180 * 86_400);
        let output = format!("{old_ts}\n10\t5\tsrc/old.ts\n");
        let (result, _) = parse_git_log(&output, root);
        let churn = &result[&PathBuf::from("/project/src/old.ts")];
        assert!(
            churn.weighted_commits < 0.5,
            "180-day-old commit should weigh ~0.25, got {}",
            churn.weighted_commits
        );
        assert!(
            churn.weighted_commits > 0.1,
            "180-day-old commit should weigh ~0.25, got {}",
            churn.weighted_commits
        );
    }

    #[test]
    fn parse_git_log_path_stored_as_absolute() {
        let root = Path::new("/my/project");
        let output = "1700000000\n1\t0\tlib/utils.ts\n";
        let (result, _) = parse_git_log(output, root);
        let key = PathBuf::from("/my/project/lib/utils.ts");
        assert!(result.contains_key(&key));
        assert_eq!(result[&key].path, key);
    }

    #[test]
    fn parse_git_log_weighted_commits_rounded() {
        let root = Path::new("/project");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let output = format!("{now}\n1\t0\tsrc/a.ts\n");
        let (result, _) = parse_git_log(&output, root);
        let churn = &result[&PathBuf::from("/project/src/a.ts")];
        let decimals = format!("{:.2}", churn.weighted_commits);
        assert_eq!(
            churn.weighted_commits.to_string().len(),
            decimals.len().min(churn.weighted_commits.to_string().len()),
            "weighted_commits should be rounded to at most 2 decimal places"
        );
    }

    #[test]
    fn trend_serde_serialization() {
        assert_eq!(
            serde_json::to_string(&ChurnTrend::Accelerating).unwrap(),
            "\"accelerating\""
        );
        assert_eq!(
            serde_json::to_string(&ChurnTrend::Stable).unwrap(),
            "\"stable\""
        );
        assert_eq!(
            serde_json::to_string(&ChurnTrend::Cooling).unwrap(),
            "\"cooling\""
        );
    }

    #[test]
    fn parse_git_log_extracts_author_email() {
        let root = Path::new("/project");
        let output = "1700000000|alice@example.com\n10\t5\tsrc/index.ts\n";
        let (result, pool) = parse_git_log(output, root);
        assert_eq!(pool, vec!["alice@example.com".to_string()]);
        let churn = &result[&PathBuf::from("/project/src/index.ts")];
        assert_eq!(churn.authors.len(), 1);
        let alice = &churn.authors[&0];
        assert_eq!(alice.commits, 1);
        assert_eq!(alice.first_commit_ts, 1_700_000_000);
        assert_eq!(alice.last_commit_ts, 1_700_000_000);
    }

    #[test]
    fn parse_git_log_intern_dedupes_authors() {
        let root = Path::new("/project");
        let output = "\
1700000000|alice@example.com
1\t0\ta.ts
1700100000|bob@example.com
2\t1\tb.ts
1700200000|alice@example.com
3\t2\tc.ts
";
        let (_result, pool) = parse_git_log(output, root);
        assert_eq!(pool.len(), 2);
        assert!(pool.contains(&"alice@example.com".to_string()));
        assert!(pool.contains(&"bob@example.com".to_string()));
    }

    #[test]
    fn parse_git_log_aggregates_per_author() {
        let root = Path::new("/project");
        let output = "\
1700000000|alice@example.com
1\t0\tsrc/index.ts
1700100000|bob@example.com
2\t0\tsrc/index.ts
1700200000|alice@example.com
1\t1\tsrc/index.ts
";
        let (result, pool) = parse_git_log(output, root);
        let churn = &result[&PathBuf::from("/project/src/index.ts")];
        assert_eq!(churn.commits, 3);
        assert_eq!(churn.authors.len(), 2);

        let alice_idx =
            u32::try_from(pool.iter().position(|a| a == "alice@example.com").unwrap()).unwrap();
        let alice = &churn.authors[&alice_idx];
        assert_eq!(alice.commits, 2);
        assert_eq!(alice.first_commit_ts, 1_700_000_000);
        assert_eq!(alice.last_commit_ts, 1_700_200_000);
    }

    #[test]
    fn parse_git_log_legacy_bare_timestamp_still_parses() {
        let root = Path::new("/project");
        let output = "1700000000\n10\t5\tsrc/index.ts\n";
        let (result, pool) = parse_git_log(output, root);
        assert!(pool.is_empty());
        let churn = &result[&PathBuf::from("/project/src/index.ts")];
        assert_eq!(churn.commits, 1);
        assert!(churn.authors.is_empty());
    }

    #[test]
    fn intern_author_returns_existing_index() {
        let mut pool = Vec::new();
        let mut index = FxHashMap::default();
        let i1 = intern_author("alice@x", &mut pool, &mut index);
        let i2 = intern_author("alice@x", &mut pool, &mut index);
        assert_eq!(i1, i2);
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn intern_author_assigns_sequential_indices() {
        let mut pool = Vec::new();
        let mut index = FxHashMap::default();
        assert_eq!(intern_author("alice@x", &mut pool, &mut index), 0);
        assert_eq!(intern_author("bob@x", &mut pool, &mut index), 1);
        assert_eq!(intern_author("carol@x", &mut pool, &mut index), 2);
        assert_eq!(intern_author("alice@x", &mut pool, &mut index), 0);
    }

    fn git(root: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed");
    }

    fn write(root: &Path, path: &str, contents: &str) {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("test path has parent")).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn churn_preserves_special_filenames() {
        let repo = tempfile::tempdir().expect("create repo");
        let root = repo.path();
        git(root, &["init", "--quiet"]);
        git(root, &["config", "user.email", "churn@example.test"]);
        git(root, &["config", "user.name", "Churn Test"]);
        git(root, &["config", "commit.gpgsign", "false"]);

        let special_files = [
            "src/line\nbreak.ts",
            "src/space name.ts",
            "src/quote\"name.ts",
            "src/back\\slash.ts",
            "src/unicode-λ.ts",
        ]
        .map(|path| root.join(path));
        std::fs::create_dir_all(root.join("src")).expect("source dir");
        for special in &special_files {
            std::fs::write(special, "export const value = 1;\n").expect("special fixture");
        }
        git(root, &["add", "."]);
        git(root, &["commit", "--quiet", "-m", "initial"]);

        let since = parse_since("1y").expect("valid duration");
        let churn = analyze_churn(root, &since).expect("churn result");
        for special in special_files {
            assert!(
                churn.files.contains_key(&special),
                "missing {special:?}: {:?}",
                churn.files.keys()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn churn_cache_preserves_non_utf8_filenames() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let invalid_path = PathBuf::from(OsString::from_vec(b"src/non-utf8-\xff.ts".to_vec()));
        let mut files = FxHashMap::default();
        files.insert(
            invalid_path.clone(),
            FileEvents {
                events: vec![CachedCommitEvent {
                    timestamp: 1,
                    lines_added: 2,
                    lines_deleted: 1,
                    author_idx: None,
                }],
            },
        );
        let state = ChurnEventState {
            files,
            author_pool: Vec::new(),
        };
        let cache_dir = tempfile::tempdir().expect("cache directory");
        save_churn_cache(cache_dir.path(), "abc123", "1 year ago", &state, false);
        let warm = load_churn_cache(cache_dir.path(), "1 year ago")
            .expect("warm churn cache")
            .into_event_state();

        assert!(warm.files.contains_key(&invalid_path));
    }

    #[cfg(windows)]
    #[test]
    fn git_path_bytes_use_windows_separators() {
        assert_eq!(
            git_path_from_bytes(b"src/nested/file.ts"),
            PathBuf::from(r"src\nested\file.ts")
        );
    }

    #[test]
    fn churn_cache_rejects_pre_lossless_path_encoding_version() {
        let cache_dir = tempfile::tempdir().expect("cache directory");
        let cache = ChurnCache {
            version: 4,
            last_indexed_sha: "abc123".to_string(),
            git_after: "1 year ago".to_string(),
            files: vec![CachedFileChurn {
                path: br#"/project/\"src/line\\nbreak.ts\""#.to_vec(),
                events: Vec::new(),
            }],
            shallow_clone: false,
            author_pool: Vec::new(),
        };
        std::fs::write(cache_dir.path().join("churn.bin"), bitcode::encode(&cache))
            .expect("cache fixture");

        assert!(load_churn_cache(cache_dir.path(), "1 year ago").is_none());
    }

    #[test]
    fn cached_churn_merges_new_commits_after_head_advances() {
        let repo = tempfile::tempdir().expect("create repo");
        let root = repo.path();
        git(root, &["init"]);
        git(root, &["config", "user.email", "churn@example.test"]);
        git(root, &["config", "user.name", "Churn Test"]);
        git(root, &["config", "commit.gpgsign", "false"]);

        write(root, "src/a.ts", "export const a = 1;\n");
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "initial"]);

        let since = parse_since("1y").unwrap();
        let cache = tempfile::tempdir().expect("create cache dir");
        let (cold, cold_hit) = analyze_churn_cached(root, &since, cache.path(), false).unwrap();
        assert!(!cold_hit);
        let file = root.join("src/a.ts");
        assert_eq!(cold.files[&file].commits, 1);

        let (_warm, warm_hit) = analyze_churn_cached(root, &since, cache.path(), false).unwrap();
        assert!(warm_hit);

        write(
            root,
            "src/a.ts",
            "export const a = 1;\nexport const b = 2;\n",
        );
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "update a"]);
        let head = get_head_sha(root).unwrap();

        let (incremental, incremental_hit) =
            analyze_churn_cached(root, &since, cache.path(), false).unwrap();
        assert!(incremental_hit);
        assert_eq!(incremental.files[&file].commits, 2);

        let cache = load_churn_cache(cache.path(), &since.git_after).unwrap();
        assert_eq!(cache.last_indexed_sha, head);
    }

    fn write_churn_file(dir: &std::path::Path, contents: &str) -> PathBuf {
        let path = dir.join("churn.json");
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn churn_file_happy_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = Path::new("/project");
        let path = write_churn_file(
            dir.path(),
            r#"{
              "schema": "fallow-churn/v1",
              "events": [
                { "path": "src/a.ts", "timestamp": 1700000000, "author": "alice@corp", "added": 10, "deleted": 5 },
                { "path": "src/a.ts", "timestamp": 1700100000, "author": "bob@corp", "added": 3, "deleted": 2 }
              ]
            }"#,
        );
        let result = analyze_churn_from_file(&path, root).unwrap();
        let churn = &result.files[&PathBuf::from("/project/src/a.ts")];
        assert_eq!(churn.commits, 2);
        assert_eq!(churn.lines_added, 13);
        assert_eq!(churn.lines_deleted, 7);
        assert_eq!(churn.authors.len(), 2);
        assert!(result.author_pool.contains(&"alice@corp".to_string()));
        assert!(result.author_pool.contains(&"bob@corp".to_string()));
        assert!(!result.shallow_clone);
    }

    #[test]
    fn churn_file_matches_git_parse() {
        // The same events fed via git numstat and via the JSON import must
        // produce identical aggregate churn: the import reuses
        // build_churn_result, so only the SOURCE differs.
        let dir = tempfile::tempdir().unwrap();
        let root = Path::new("/project");
        let git_output = "1700000000|alice@corp\n10\t5\tsrc/a.ts\n3\t1\tsrc/b.ts\n\n1700100000|bob@corp\n3\t2\tsrc/a.ts\n";
        let (git_files, git_pool) = parse_git_log(git_output, root);

        let path = write_churn_file(
            dir.path(),
            r#"{
              "schema": "fallow-churn/v1",
              "events": [
                { "path": "src/a.ts", "timestamp": 1700000000, "author": "alice@corp", "added": 10, "deleted": 5 },
                { "path": "src/b.ts", "timestamp": 1700000000, "author": "alice@corp", "added": 3, "deleted": 1 },
                { "path": "src/a.ts", "timestamp": 1700100000, "author": "bob@corp", "added": 3, "deleted": 2 }
              ]
            }"#,
        );
        let imported = analyze_churn_from_file(&path, root).unwrap();

        assert_eq!(git_pool, imported.author_pool, "author pools diverge");
        assert_eq!(git_files.len(), imported.files.len());
        for (file, git_churn) in &git_files {
            let imp = &imported.files[file];
            assert_eq!(git_churn.commits, imp.commits, "commits for {file:?}");
            assert_eq!(git_churn.lines_added, imp.lines_added, "added for {file:?}");
            assert_eq!(
                git_churn.lines_deleted, imp.lines_deleted,
                "deleted for {file:?}"
            );
            assert_eq!(git_churn.trend, imp.trend, "trend for {file:?}");
            assert_eq!(
                git_churn.authors.len(),
                imp.authors.len(),
                "authors for {file:?}"
            );
            assert!(
                (git_churn.weighted_commits - imp.weighted_commits).abs() < 0.02,
                "weighted_commits for {file:?}: {} vs {}",
                git_churn.weighted_commits,
                imp.weighted_commits
            );
        }
    }

    #[test]
    fn churn_file_empty_events_is_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_churn_file(
            dir.path(),
            r#"{ "schema": "fallow-churn/v1", "events": [] }"#,
        );
        let result = analyze_churn_from_file(&path, Path::new("/project")).unwrap();
        assert!(result.files.is_empty());
        assert!(result.author_pool.is_empty());
    }

    #[test]
    fn churn_file_missing_events_key_is_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_churn_file(dir.path(), r#"{ "schema": "fallow-churn/v1" }"#);
        let result = analyze_churn_from_file(&path, Path::new("/project")).unwrap();
        assert!(result.files.is_empty());
    }

    #[test]
    fn churn_file_bad_schema_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_churn_file(
            dir.path(),
            r#"{ "schema": "fallow-churn/v2", "events": [] }"#,
        );
        let err = analyze_churn_from_file(&path, Path::new("/project")).unwrap_err();
        assert!(err.contains("expected \"fallow-churn/v1\""), "{err}");
    }

    #[test]
    fn churn_file_malformed_json_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_churn_file(dir.path(), "{ not json");
        assert!(analyze_churn_from_file(&path, Path::new("/project")).is_err());
    }

    #[test]
    fn churn_file_missing_file_rejected() {
        let err = analyze_churn_from_file(Path::new("/no/such/churn.json"), Path::new("/project"))
            .unwrap_err();
        assert!(err.contains("failed to read churn file"), "{err}");
    }

    #[test]
    fn churn_file_reader_accepts_exact_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_churn_file(dir.path(), "12345678");
        assert_eq!(read_churn_file_with_limit(&path, 8).unwrap(), "12345678");
    }

    #[test]
    fn churn_file_reader_rejects_limit_plus_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_churn_file(dir.path(), "123456789");
        let err = read_churn_file_with_limit(&path, 8).unwrap_err();
        assert!(err.contains("at least 9 bytes"), "{err}");
        assert!(err.contains("8 byte limit"), "{err}");
    }

    #[test]
    fn churn_file_empty_path_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_churn_file(
            dir.path(),
            r#"{ "schema": "fallow-churn/v1", "events": [ { "path": "  ", "timestamp": 1700000000, "added": 1, "deleted": 0 } ] }"#,
        );
        let err = analyze_churn_from_file(&path, Path::new("/project")).unwrap_err();
        assert!(err.contains("empty path"), "{err}");
    }

    #[test]
    fn churn_file_rejects_non_relative_paths() {
        let invalid = [
            "/tmp/a.ts",
            r"C:\tmp\a.ts",
            "../a.ts",
            "src/../../a.ts",
            "./src/a.ts",
            "//server/share/a.ts",
        ];
        for event_path in invalid {
            let dir = tempfile::tempdir().unwrap();
            let body = format!(
                r#"{{ "schema": "fallow-churn/v1", "events": [ {{ "path": {event_path:?}, "timestamp": 1700000000, "added": 1, "deleted": 0 }} ] }}"#
            );
            let path = write_churn_file(dir.path(), &body);
            let err = analyze_churn_from_file(&path, Path::new("/project")).unwrap_err();
            assert!(err.contains(event_path), "{event_path}: {err}");
            assert!(err.contains("repo-relative"), "{event_path}: {err}");
        }
    }

    #[test]
    fn churn_file_accepts_unicode_and_spaces_in_path_components() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_churn_file(
            dir.path(),
            r#"{ "schema": "fallow-churn/v1", "events": [ { "path": "src/ruimte map/naïef.ts", "timestamp": 1700000000, "added": 1, "deleted": 0 } ] }"#,
        );
        let result = analyze_churn_from_file(&path, Path::new("/project")).unwrap();
        assert!(
            result
                .files
                .contains_key(&PathBuf::from("/project/src/ruimte map/naïef.ts"))
        );
    }

    #[test]
    fn churn_file_millisecond_timestamp_rejected() {
        let dir = tempfile::tempdir().unwrap();
        // 1700000000000 is milliseconds; ~52000 years in the future as seconds.
        let path = write_churn_file(
            dir.path(),
            r#"{ "schema": "fallow-churn/v1", "events": [ { "path": "src/a.ts", "timestamp": 1700000000000, "added": 1, "deleted": 0 } ] }"#,
        );
        let err = analyze_churn_from_file(&path, Path::new("/project")).unwrap_err();
        assert!(err.contains("milliseconds"), "{err}");
    }

    #[test]
    fn churn_file_missing_author_contributes_no_signal() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_churn_file(
            dir.path(),
            r#"{ "schema": "fallow-churn/v1", "events": [ { "path": "src/a.ts", "timestamp": 1700000000, "added": 1, "deleted": 0 } ] }"#,
        );
        let result = analyze_churn_from_file(&path, Path::new("/project")).unwrap();
        let churn = &result.files[&PathBuf::from("/project/src/a.ts")];
        assert_eq!(churn.commits, 1);
        assert!(churn.authors.is_empty());
        assert!(result.author_pool.is_empty());
    }

    #[test]
    fn churn_file_empty_author_string_treated_as_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_churn_file(
            dir.path(),
            r#"{ "schema": "fallow-churn/v1", "events": [ { "path": "src/a.ts", "timestamp": 1700000000, "author": "  ", "added": 1, "deleted": 0 } ] }"#,
        );
        let result = analyze_churn_from_file(&path, Path::new("/project")).unwrap();
        assert!(result.author_pool.is_empty());
    }

    #[test]
    fn churn_file_unknown_fields_ignored() {
        // Extra keys (including the reserved `commit`) are accepted and ignored,
        // so a wrapper carrying extra metadata stays forward-compatible.
        let dir = tempfile::tempdir().unwrap();
        let path = write_churn_file(
            dir.path(),
            r#"{ "schema": "fallow-churn/v1", "extra": true, "events": [ { "path": "src/a.ts", "timestamp": 1700000000, "author": "alice@corp", "added": 1, "deleted": 0, "commit": "abc123", "tz": "+0200" } ] }"#,
        );
        let result = analyze_churn_from_file(&path, Path::new("/project")).unwrap();
        assert_eq!(result.files[&PathBuf::from("/project/src/a.ts")].commits, 1);
    }

    #[test]
    fn churn_file_backslash_paths_normalized() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_churn_file(
            dir.path(),
            r#"{ "schema": "fallow-churn/v1", "events": [ { "path": "src\\a.ts", "timestamp": 1700000000, "added": 1, "deleted": 0 } ] }"#,
        );
        let result = analyze_churn_from_file(&path, Path::new("/project")).unwrap();
        assert!(
            result
                .files
                .contains_key(&PathBuf::from("/project/src/a.ts"))
        );
    }

    #[test]
    fn churn_file_rejects_line_totals_above_u32() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_churn_file(
            dir.path(),
            r#"{ "schema": "fallow-churn/v1", "events": [
                { "path": "src/a.ts", "timestamp": 1700000000, "added": 4294967295, "deleted": 0 },
                { "path": "src/a.ts", "timestamp": 1700000001, "added": 1, "deleted": 0 }
            ] }"#,
        );
        let err = analyze_churn_from_file(&path, Path::new("/project")).unwrap_err();
        assert!(err.contains("exceeding the u32 limit"), "{err}");
        assert!(err.contains("src/a.ts"), "{err}");
    }

    #[test]
    fn churn_file_accepts_line_totals_at_u32_max() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_churn_file(
            dir.path(),
            r#"{ "schema": "fallow-churn/v1", "events": [
                { "path": "src/a.ts", "timestamp": 1700000000, "added": 4294967294, "deleted": 4294967295 },
                { "path": "src/a.ts", "timestamp": 1700000001, "added": 1, "deleted": 0 }
            ] }"#,
        );
        let result = analyze_churn_from_file(&path, Path::new("/project")).unwrap();
        let churn = &result.files[&PathBuf::from("/project/src/a.ts")];
        assert_eq!(churn.lines_added, u32::MAX);
        assert_eq!(churn.lines_deleted, u32::MAX);
    }
}
