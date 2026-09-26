//! Flag age from git history for `fallow flags --retirement`.
//!
//! `blame` mode runs one `git blame --porcelain` per file with flag sites and
//! asks for the lines of those sites only. The oldest commit is a lower bound
//! on the age of the flag, because a rewrite of the line resets it. `pickaxe`
//! mode also runs one `git log -S<name>` per flag name, which gives the first
//! commit that added the name. Both modes count days against the
//! [`AnalysisClock`], so two runs over one commit give the same ages.
//!
//! Results are cached under the cache directory for the current HEAD. A blame
//! entry is also keyed by the file content, because blame reads the working
//! tree.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};

use fallow_types::flag_retirement::{FlagAgeMode, FlagCommit, RetirementFlag};
use fallow_types::workspace::WorkspaceDiagnosticKind;
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

use crate::clock::{AnalysisClock, utc_date, utc_timestamp};

/// Cache file name under the cache directory.
const CACHE_FILE: &str = "flag-age.json";

/// Cache format version. Bump it when the cached shape or meaning changes.
const CACHE_VERSION: u32 = 1;

/// Length of the abbreviated commit hash in the report.
const SHORT_SHA_LEN: usize = 12;

/// Seconds in one day.
const SECS_PER_DAY: u64 = 86_400;

/// Number of pickaxe runs between two progress reports.
const PICKAXE_PROGRESS_STEP: usize = 10;

/// Pickaxe progress: flag names read so far, and the flag names to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickaxeProgress {
    /// Flag names read so far.
    pub done: usize,
    /// Flag names this run reads.
    pub total: usize,
}

/// Inputs of one flag-age measurement.
#[derive(Clone, Copy)]
pub struct FlagAgeRequest<'a> {
    /// Project root. Git runs here, and site paths are relative to it.
    pub root: &'a Path,
    /// How to measure the age.
    pub mode: FlagAgeMode,
    /// Directory for the age cache, or `None` to run without the cache.
    pub cache_dir: Option<&'a Path>,
    /// Receives pickaxe progress: once at the start, then every few names.
    pub progress: Option<&'a (dyn Fn(PickaxeProgress) + Sync)>,
}

/// What a flag-age measurement did.
#[derive(Debug, Default)]
pub struct FlagAgeOutcome {
    /// The analysis clock as an RFC 3339 timestamp, when ages were measured.
    pub generated_at_clock: Option<String>,
    /// Why ages are missing, when history was not available.
    pub diagnostics: Vec<WorkspaceDiagnosticKind>,
    /// `git blame` subprocesses this run started.
    pub blame_calls: usize,
    /// `git log -S` subprocesses this run started.
    pub pickaxe_calls: usize,
}

/// A commit and its committer time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CommitStamp {
    sha: String,
    time: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AgeCache {
    version: u32,
    head: String,
    files: BTreeMap<String, FileBlame>,
    pickaxe: BTreeMap<String, Option<CommitStamp>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FileBlame {
    content_hash: u64,
    lines: BTreeMap<u32, Option<CommitStamp>>,
}

/// Fill the age fields of `rows` from git history.
///
/// A missing repository, a branch without commits and a shallow clone leave
/// every age `None` and return the matching diagnostic.
pub fn apply_flag_ages(
    rows: &mut [RetirementFlag],
    request: &FlagAgeRequest<'_>,
) -> FlagAgeOutcome {
    let mut outcome = FlagAgeOutcome::default();
    if request.mode == FlagAgeMode::Off {
        return outcome;
    }
    let head = match probe_history(request.root) {
        Ok(head) => head,
        Err(diagnostic) => {
            outcome.diagnostics.push(diagnostic);
            return outcome;
        }
    };
    let clock = AnalysisClock::for_repo(request.root);
    outcome.generated_at_clock = Some(utc_timestamp(clock.epoch_secs()));

    let mut cache = request
        .cache_dir
        .and_then(|dir| load_cache(dir, &head))
        .unwrap_or_else(|| AgeCache {
            version: CACHE_VERSION,
            head: head.clone(),
            ..AgeCache::default()
        });

    outcome.blame_calls = refresh_blame(&mut cache, rows, request.root);
    if request.mode == FlagAgeMode::Pickaxe {
        outcome.pickaxe_calls = refresh_pickaxe(&mut cache, rows, request);
    }
    if let Some(dir) = request.cache_dir
        && (outcome.blame_calls > 0 || outcome.pickaxe_calls > 0)
    {
        save_cache(dir, &cache);
    }

    for row in rows.iter_mut() {
        fill_row(row, &cache, request.mode, clock);
    }
    outcome
}

/// HEAD's commit hash, or the diagnostic that explains why no history is
/// available.
fn probe_history(root: &Path) -> Result<String, WorkspaceDiagnosticKind> {
    let shallow = run_git(root, &["rev-parse", "--is-shallow-repository"]).ok_or_else(|| {
        WorkspaceDiagnosticKind::FlagAgeUnavailable {
            cause: "not-a-repository".to_string(),
        }
    })?;
    if shallow.trim().eq_ignore_ascii_case("true") {
        return Err(WorkspaceDiagnosticKind::FlagAgeShallowClone);
    }
    let head = run_git(root, &["rev-parse", "--verify", "--quiet", "HEAD"]).ok_or_else(|| {
        WorkspaceDiagnosticKind::FlagAgeUnavailable {
            cause: "no-commits".to_string(),
        }
    })?;
    Ok(head.trim().to_string())
}

fn run_git(root: &Path, args: &[&str]) -> Option<String> {
    let output = crate::git_env::git_command()
        .args(args)
        .current_dir(root)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Blame every file whose site lines the cache does not hold for the current
/// file content. Returns the number of blame subprocesses.
fn refresh_blame(cache: &mut AgeCache, rows: &[RetirementFlag], root: &Path) -> usize {
    let mut lines_by_file: BTreeMap<&str, FxHashSet<u32>> = BTreeMap::new();
    for site in rows.iter().flat_map(|row| &row.sites) {
        lines_by_file
            .entry(site.path.as_str())
            .or_default()
            .insert(site.line);
    }
    let stale: Vec<(String, u64, Vec<u32>)> = lines_by_file
        .into_iter()
        .filter_map(|(path, lines)| {
            let bytes = std::fs::read(root.join(path)).ok()?;
            let content_hash = xxhash_rust::xxh3::xxh3_64(&bytes);
            let cached = cache.files.get(path).is_some_and(|entry| {
                entry.content_hash == content_hash
                    && lines.iter().all(|line| entry.lines.contains_key(line))
            });
            if cached {
                return None;
            }
            let mut lines: Vec<u32> = lines.into_iter().collect();
            lines.sort_unstable();
            Some((path.to_string(), content_hash, lines))
        })
        .collect();

    let calls = AtomicUsize::new(0);
    let blamed: Vec<(String, u64, FxHashMap<u32, Option<CommitStamp>>)> = stale
        .into_par_iter()
        .map(|(path, content_hash, lines)| {
            calls.fetch_add(1, Ordering::Relaxed);
            let stamps = blame_lines(root, &path, &lines);
            (path, content_hash, stamps)
        })
        .collect();

    for (path, content_hash, stamps) in blamed {
        let entry = cache.files.entry(path).or_default();
        if entry.content_hash != content_hash {
            entry.lines.clear();
            entry.content_hash = content_hash;
        }
        entry.lines.extend(stamps);
    }
    calls.into_inner()
}

/// Commit of each of `lines` in `path`. A line that no commit holds (an
/// uncommitted change, or an untracked file) maps to `None`.
fn blame_lines(root: &Path, path: &str, lines: &[u32]) -> FxHashMap<u32, Option<CommitStamp>> {
    let mut args: Vec<String> = vec!["blame".to_string(), "--porcelain".to_string()];
    for line in lines {
        args.push("-L".to_string());
        args.push(format!("{line},{line}"));
    }
    args.push("--".to_string());
    args.push(path.to_string());
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut stamps: FxHashMap<u32, Option<CommitStamp>> =
        lines.iter().map(|&line| (line, None)).collect();
    if let Some(output) = run_git(root, &arg_refs) {
        stamps.extend(parse_blame_porcelain(&output));
    }
    stamps
}

/// Parse `git blame --porcelain` into the commit of each final line.
fn parse_blame_porcelain(output: &str) -> FxHashMap<u32, Option<CommitStamp>> {
    let mut times: FxHashMap<&str, u64> = FxHashMap::default();
    let mut line_shas: Vec<(u32, &str)> = Vec::new();
    let mut current: Option<(&str, u32)> = None;
    for line in output.lines() {
        if line.starts_with('\t') {
            if let Some((sha, final_line)) = current.take() {
                line_shas.push((final_line, sha));
            }
            continue;
        }
        if let Some(time) = line.strip_prefix("committer-time ") {
            if let (Some((sha, _)), Ok(time)) = (current, time.trim().parse::<u64>()) {
                times.insert(sha, time);
            }
            continue;
        }
        let mut fields = line.split(' ');
        let Some(first) = fields.next() else {
            continue;
        };
        if is_object_id(first)
            && let Some(Ok(final_line)) = fields.nth(1).map(str::parse::<u32>)
        {
            current = Some((first, final_line));
        }
    }
    line_shas
        .into_iter()
        .map(|(line, sha)| {
            let stamp = (!sha.bytes().all(|byte| byte == b'0'))
                .then(|| times.get(sha))
                .flatten()
                .map(|&time| CommitStamp {
                    sha: sha.to_string(),
                    time,
                });
            (line, stamp)
        })
        .collect()
}

fn is_object_id(token: &str) -> bool {
    matches!(token.len(), 40 | 64) && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Run `git log -S` for every flag name the cache does not hold. Returns the
/// number of pickaxe subprocesses.
fn refresh_pickaxe(
    cache: &mut AgeCache,
    rows: &[RetirementFlag],
    request: &FlagAgeRequest<'_>,
) -> usize {
    let mut names: Vec<&str> = rows
        .iter()
        .map(|row| row.flag_name.as_str())
        .filter(|name| !name.is_empty() && !cache.pickaxe.contains_key(*name))
        .collect();
    names.sort_unstable();
    names.dedup();
    if names.is_empty() {
        return 0;
    }
    let total = names.len();
    let report = |done: usize| {
        if let Some(progress) = request.progress {
            progress(PickaxeProgress { done, total });
        }
    };
    report(0);
    let done = AtomicUsize::new(0);
    let found: Vec<(String, Option<CommitStamp>)> = names
        .into_par_iter()
        .map(|name| {
            let stamp = first_commit_with(request.root, name);
            let finished = done.fetch_add(1, Ordering::Relaxed) + 1;
            if finished < total && finished.is_multiple_of(PICKAXE_PROGRESS_STEP) {
                report(finished);
            }
            (name.to_string(), stamp)
        })
        .collect();
    cache.pickaxe.extend(found);
    total
}

/// The oldest commit under the root that changed the number of times `name`
/// occurs.
fn first_commit_with(root: &Path, name: &str) -> Option<CommitStamp> {
    let pickaxe = format!("-S{name}");
    let output = run_git(
        root,
        &[
            "log",
            pickaxe.as_str(),
            "--reverse",
            "--format=%H %ct",
            "--",
            ".",
        ],
    )?;
    let first = output.lines().next()?;
    let (sha, time) = first.split_once(' ')?;
    Some(CommitStamp {
        sha: sha.to_string(),
        time: time.trim().parse().ok()?,
    })
}

fn fill_row(row: &mut RetirementFlag, cache: &AgeCache, mode: FlagAgeMode, clock: AnalysisClock) {
    let stamps: Vec<&CommitStamp> = row
        .sites
        .iter()
        .filter_map(|site| {
            cache
                .files
                .get(&site.path)
                .and_then(|entry| entry.lines.get(&site.line))
                .and_then(Option::as_ref)
        })
        .collect();
    let oldest = stamps
        .iter()
        .min_by(|a, b| a.time.cmp(&b.time).then(a.sha.cmp(&b.sha)))
        .copied();
    let newest = stamps
        .iter()
        .max_by(|a, b| a.time.cmp(&b.time).then(b.sha.cmp(&a.sha)))
        .copied();
    let first_seen = (mode == FlagAgeMode::Pickaxe)
        .then(|| cache.pickaxe.get(&row.flag_name))
        .flatten()
        .and_then(Option::as_ref);

    row.oldest_surviving_site = oldest.map(flag_commit);
    row.last_touched = newest.map(flag_commit);
    row.first_seen = first_seen.map(flag_commit);
    row.age_days = first_seen
        .or(oldest)
        .map(|stamp| clock.epoch_secs().saturating_sub(stamp.time) / SECS_PER_DAY);
}

fn flag_commit(stamp: &CommitStamp) -> FlagCommit {
    FlagCommit {
        commit: stamp.sha.chars().take(SHORT_SHA_LEN).collect(),
        date: utc_date(stamp.time),
    }
}

fn load_cache(dir: &Path, head: &str) -> Option<AgeCache> {
    let bytes = std::fs::read(dir.join(CACHE_FILE)).ok()?;
    let cache: AgeCache = serde_json::from_slice(&bytes).ok()?;
    (cache.version == CACHE_VERSION && cache.head == head).then_some(cache)
}

/// Write the cache through a temporary file, so a reader never sees a
/// partial file. A failed write only costs the next run its warm cache.
fn save_cache(dir: &Path, cache: &AgeCache) {
    let Ok(bytes) = serde_json::to_vec(cache) else {
        return;
    };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = dir.join(format!("{CACHE_FILE}.tmp"));
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, dir.join(CACHE_FILE));
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_types::flag_retirement::{FlagSiteRole, RetirementFlagKind, RetirementSite};

    use super::*;

    /// 2023-11-14T22:13:20Z.
    const BASE_EPOCH: u64 = 1_700_000_000;

    fn git(root: &Path, args: &[&str], epoch: Option<u64>) {
        let mut command = crate::git_env::git_command();
        command.current_dir(root).args(args).stdout(Stdio::null());
        if let Some(epoch) = epoch {
            let stamp = format!("{epoch} +0000");
            command
                .env("GIT_AUTHOR_DATE", &stamp)
                .env("GIT_COMMITTER_DATE", &stamp);
        }
        let status = command.status().expect("run git");
        assert!(status.success(), "git {args:?} failed");
    }

    fn init_repo(root: &Path) {
        git(root, &["init", "--quiet", "--initial-branch=main"], None);
        git(root, &["config", "user.name", "Flag Fixture"], None);
        git(root, &["config", "user.email", "fixture@example.com"], None);
        git(root, &["config", "commit.gpgsign", "false"], None);
    }

    fn commit_file(root: &Path, path: &str, contents: &str, epoch: u64) {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().expect("parent")).expect("dirs");
        std::fs::write(&file, contents).expect("write");
        git(root, &["add", path], None);
        git(root, &["commit", "--quiet", "-m", path], Some(epoch));
    }

    fn row(name: &str, sites: &[(&str, u32)]) -> RetirementFlag {
        RetirementFlag {
            flag_name: name.to_string(),
            kind: RetirementFlagKind::EnvironmentVariable,
            sdk_name: None,
            workspace: None,
            sites: sites
                .iter()
                .map(|&(path, line)| RetirementSite {
                    path: path.to_string(),
                    line,
                    col: 0,
                    role: FlagSiteRole::Read,
                    in_test: false,
                })
                .collect(),
            read_sites: sites.len(),
            test_only: false,
            first_seen: None,
            oldest_surviving_site: None,
            last_touched: None,
            age_days: None,
            reasons: Vec::new(),
            evidence: Vec::new(),
            actions: Vec::new(),
        }
    }

    /// A repository where `FEATURE_A` first appears at day 0, is rewritten
    /// on day 10 in `a.ts`, and gets a second site in `b.ts` on day 40. HEAD
    /// is at day 100.
    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_path_buf();
        init_repo(&root);
        let day = |n: u64| BASE_EPOCH + n * SECS_PER_DAY;
        commit_file(&root, "a.ts", "x\nif (process.env.FEATURE_A) {}\n", day(0));
        commit_file(
            &root,
            "a.ts",
            "x\nif (process.env.FEATURE_A === '1') {}\n",
            day(10),
        );
        commit_file(&root, "b.ts", "if (process.env.FEATURE_A) {}\n", day(40));
        commit_file(&root, "c.ts", "// unrelated\n", day(100));
        (dir, root)
    }

    fn measure(
        root: &Path,
        mode: FlagAgeMode,
        cache_dir: Option<&Path>,
    ) -> (Vec<RetirementFlag>, FlagAgeOutcome) {
        let mut rows = vec![row("FEATURE_A", &[("a.ts", 2), ("b.ts", 1)])];
        let outcome = apply_flag_ages(
            &mut rows,
            &FlagAgeRequest {
                root,
                mode,
                cache_dir,
                progress: None,
            },
        );
        (rows, outcome)
    }

    #[test]
    fn blame_ages_count_from_the_oldest_surviving_line() {
        let (_dir, root) = fixture();
        let (rows, outcome) = measure(&root, FlagAgeMode::Blame, None);
        let row = &rows[0];
        assert!(outcome.diagnostics.is_empty());
        assert_eq!(outcome.blame_calls, 2, "one blame per file with sites");
        assert_eq!(outcome.pickaxe_calls, 0);
        assert_eq!(
            outcome.generated_at_clock.as_deref(),
            Some("2024-02-22T22:13:20Z")
        );
        assert_eq!(row.age_days, Some(90), "day 100 minus the day-10 rewrite");
        assert_eq!(
            row.oldest_surviving_site.as_ref().map(|c| c.date.as_str()),
            Some("2023-11-24")
        );
        assert_eq!(
            row.last_touched.as_ref().map(|c| c.date.as_str()),
            Some("2023-12-24")
        );
        assert!(row.first_seen.is_none(), "blame does not read first_seen");
        assert_eq!(
            row.oldest_surviving_site.as_ref().map(|c| c.commit.len()),
            Some(SHORT_SHA_LEN)
        );
    }

    #[test]
    fn pickaxe_ages_count_from_the_first_commit_with_the_name() {
        let (_dir, root) = fixture();
        let (rows, outcome) = measure(&root, FlagAgeMode::Pickaxe, None);
        let row = &rows[0];
        assert_eq!(outcome.pickaxe_calls, 1, "one pickaxe per flag name");
        assert_eq!(row.age_days, Some(100));
        assert_eq!(
            row.first_seen.as_ref().map(|c| c.date.as_str()),
            Some("2023-11-14")
        );
    }

    #[test]
    fn a_warm_cache_starts_no_git_history_subprocess() {
        let (_dir, root) = fixture();
        let cache = tempfile::tempdir().expect("cache dir");
        let (cold_rows, cold) = measure(&root, FlagAgeMode::Pickaxe, Some(cache.path()));
        assert_eq!((cold.blame_calls, cold.pickaxe_calls), (2, 1));
        let (warm_rows, warm) = measure(&root, FlagAgeMode::Pickaxe, Some(cache.path()));
        assert_eq!((warm.blame_calls, warm.pickaxe_calls), (0, 0));
        assert_eq!(cold_rows, warm_rows);
    }

    #[test]
    fn an_edited_file_is_blamed_again() {
        let (_dir, root) = fixture();
        let cache = tempfile::tempdir().expect("cache dir");
        let _ = measure(&root, FlagAgeMode::Blame, Some(cache.path()));
        std::fs::write(
            root.join("b.ts"),
            "if (process.env.FEATURE_A) { edit(); }\n",
        )
        .expect("edit");
        let (rows, outcome) = measure(&root, FlagAgeMode::Blame, Some(cache.path()));
        assert_eq!(outcome.blame_calls, 1);
        assert_eq!(
            rows[0].last_touched.as_ref().map(|c| c.date.as_str()),
            Some("2023-11-24"),
            "an uncommitted line has no commit"
        );
    }

    #[test]
    fn off_mode_starts_no_git_subprocess() {
        let (_dir, root) = fixture();
        let (rows, outcome) = measure(&root, FlagAgeMode::Off, None);
        assert_eq!((outcome.blame_calls, outcome.pickaxe_calls), (0, 0));
        assert!(outcome.generated_at_clock.is_none());
        assert_eq!(rows[0].age_days, None);
    }

    #[test]
    fn no_repository_gives_no_age_and_a_diagnostic() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("a.ts"), "x\n").expect("write");
        let (rows, outcome) = measure(dir.path(), FlagAgeMode::Blame, None);
        assert_eq!(rows[0].age_days, None);
        assert_eq!(
            outcome.diagnostics,
            vec![WorkspaceDiagnosticKind::FlagAgeUnavailable {
                cause: "not-a-repository".to_string()
            }]
        );
    }

    #[test]
    fn a_branch_without_commits_gives_no_age_and_a_diagnostic() {
        let dir = tempfile::tempdir().expect("temp dir");
        init_repo(dir.path());
        let (_, outcome) = measure(dir.path(), FlagAgeMode::Blame, None);
        assert_eq!(
            outcome.diagnostics,
            vec![WorkspaceDiagnosticKind::FlagAgeUnavailable {
                cause: "no-commits".to_string()
            }]
        );
    }

    #[test]
    fn a_shallow_clone_gives_no_age_and_a_diagnostic() {
        let (_dir, root) = fixture();
        let clone = tempfile::tempdir().expect("clone dir");
        let source = format!("file://{}", root.display());
        git(
            clone.path(),
            &["clone", "--quiet", "--depth", "1", &source, "."],
            None,
        );
        let (rows, outcome) = measure(clone.path(), FlagAgeMode::Blame, None);
        assert_eq!(rows[0].age_days, None);
        assert_eq!(
            outcome.diagnostics,
            vec![WorkspaceDiagnosticKind::FlagAgeShallowClone]
        );
    }

    #[test]
    fn porcelain_parser_reads_repeated_commits_and_uncommitted_lines() {
        let sha = "a".repeat(40);
        let zero = "0".repeat(40);
        let output = format!(
            "{sha} 1 3 1\nauthor A\ncommitter-time 100\nsummary s\nfilename a.ts\n\tline three\n\
             {sha} 2 5 1\n\tline five\n\
             {zero} 7 7 1\ncommitter-time 900\n\tline seven\n"
        );
        let stamps = parse_blame_porcelain(&output);
        assert_eq!(stamps[&3].as_ref().map(|s| s.time), Some(100));
        assert_eq!(stamps[&5].as_ref().map(|s| s.time), Some(100));
        assert_eq!(stamps[&7], None);
    }
}
