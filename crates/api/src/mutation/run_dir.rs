//! The run directory (`protocol.md` section 6): the instrumented files, the
//! path map, and the runtime hooks. Project files never change.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use super::companion::InstalledCompanion;

/// One instrumented file to write.
#[derive(Debug, Clone, Copy)]
pub struct RunFile<'a> {
    /// The root-relative path, with forward slashes.
    pub path: &'a str,
    /// The instrumented source text.
    pub code: &'a str,
}

/// A written run directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunDir {
    pub dir: PathBuf,
    /// Root-relative source path to run-dir-relative instrumented path.
    pub path_map: BTreeMap<String, String>,
}

/// True when `path` is a plain relative path with no parent or root part.
fn is_contained(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn content_hash(files: &[RunFile<'_>]) -> String {
    let mut hasher = Sha256::new();
    for file in files {
        hasher.update((file.path.len() as u64).to_le_bytes());
        hasher.update(file.path.as_bytes());
        hasher.update((file.code.len() as u64).to_le_bytes());
        hasher.update(file.code.as_bytes());
    }
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// The number of run directories that a run keeps, newest first, next to the
/// directory that it uses. A project usually alternates between a few plans
/// (a full run, a `--changed-since` run, one or two scoped runs). Five keeps
/// the directories of these plans and bounds the disk use to a few copies of
/// the instrumented files.
const RUN_DIRS_KEPT: usize = 5;
/// A run directory or a staging directory that a run used within this time
/// is never removed, so a concurrent run keeps its directory.
const RUN_DIR_GRACE: Duration = Duration::from_hours(1);
/// The marker file whose modification time records the last use of a run
/// directory.
const LAST_USED: &str = ".last-used";
/// The name prefix of a staging directory below `runs/`.
const STAGING_PREFIX: &str = ".staging-";
/// The name suffix of the publication lock file of a run directory.
const LOCK_SUFFIX: &str = ".lock";

/// The source of one expected file of a run directory.
enum Content<'a> {
    Text(&'a str),
    Copy(&'a Path),
}

/// Writes `<mutation_cache>/runs/<content hash>/` on first use, or reuses
/// it when each file has the expected content.
///
/// The write goes to a staging directory that is renamed into place, so a
/// crash or a concurrent run never leaves a partial directory. A directory
/// with other content is replaced, so a stale directory with the same hash
/// cannot serve other text. After the write, the run removes old run
/// directories (see [`prune_run_dirs`]).
///
/// # Errors
///
/// Returns an error for a path that leaves the run directory, or an IO error.
pub fn write_run_dir(
    mutation_cache: &Path,
    files: &[RunFile<'_>],
    companion: Option<&InstalledCompanion>,
) -> Result<RunDir, String> {
    let mut files = files.to_vec();
    files.sort_by(|left, right| left.path.cmp(right.path));
    if let Some(bad) = files.iter().find(|file| !is_contained(file.path)) {
        return Err(format!(
            "the plan path `{}` is not a plain relative path",
            bad.path
        ));
    }
    let runs = mutation_cache.join("runs");
    let dir = runs.join(content_hash(&files));
    let mut expected: Vec<(String, Content<'_>)> = files
        .iter()
        .map(|file| (format!("files/{}", file.path), Content::Text(file.code)))
        .collect();
    let path_map: BTreeMap<String, String> = files
        .iter()
        .zip(&expected)
        .map(|(file, (target, _))| (file.path.to_owned(), target.clone()))
        .collect();
    let paths = serde_json::to_string_pretty(&path_map)
        .map_err(|error| format!("cannot serialize paths.json: {error}"))?;
    expected.push(("paths.json".to_owned(), Content::Text(&paths)));
    if let Some(companion) = companion {
        expected.extend(
            companion
                .runtime_files()
                .map(|(relative, source)| (relative.to_owned(), Content::Copy(source))),
        );
    }
    if !matches(&dir, &expected) {
        install(&runs, &dir, &expected).map_err(|error| {
            format!(
                "cannot write the mutation run directory {}: {error}",
                dir.display()
            )
        })?;
    }
    // A failed marker write only makes the prune order less exact.
    let _ = fs::write(dir.join(LAST_USED), now_nanos().to_string());
    prune_run_dirs(&runs, &dir, SystemTime::now());
    Ok(RunDir { dir, path_map })
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos())
}

/// True when each expected file of `dir` has the expected content.
fn matches(dir: &Path, expected: &[(String, Content<'_>)]) -> bool {
    dir.is_dir()
        && expected.iter().all(|(relative, content)| {
            let Ok(actual) = fs::read(dir.join(relative)) else {
                return false;
            };
            match content {
                Content::Text(text) => actual == text.as_bytes(),
                Content::Copy(source) => fs::read(source).is_ok_and(|bytes| bytes == actual),
            }
        })
}

/// The lock file next to a run directory. A run holds it while it checks and
/// replaces the directory, so two runs never replace each other's work. The
/// empty file stays after the directory is pruned: a removal could split two
/// runs that wait on the old and the new file at that moment.
fn publication_lock_path(dir: &Path) -> PathBuf {
    let mut name = dir.file_name().unwrap_or_default().to_os_string();
    name.push(LOCK_SUFFIX);
    dir.with_file_name(name)
}

/// Writes `expected` to a staging directory below `runs` and renames it to
/// `dir`. A directory at `dir` with other content moves aside first.
///
/// The check and the replacement run under the publication lock of `dir`.
/// Without it, a second run could publish a complete directory between the
/// check of this run and its move aside, and this run would then delete the
/// directory that the second run reads.
fn install(runs: &Path, dir: &Path, expected: &[(String, Content<'_>)]) -> std::io::Result<()> {
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    fs::create_dir_all(runs)?;
    // Two threads of one process can read the same time, so a counter keeps
    // their staging directories apart.
    let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let unique = format!("{}-{}-{sequence}", std::process::id(), now_nanos());
    let staging = runs.join(format!("{STAGING_PREFIX}{unique}"));
    if let Err(error) = write_staging(&staging, expected) {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    // The lock is best effort: a file system without lock support, such as
    // some network mounts, publishes without it, as before the lock.
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(publication_lock_path(dir))
        .and_then(|file| file.lock().map(|()| file))
        .ok();
    let result = publish(runs, dir, &staging, &unique, expected);
    drop(lock);
    result
}

/// Moves `staging` to `dir` under the publication lock of `dir`.
fn publish(
    runs: &Path,
    dir: &Path,
    staging: &Path,
    unique: &str,
    expected: &[(String, Content<'_>)],
) -> std::io::Result<()> {
    // A concurrent run can install the same directory while this run
    // stages. That directory has the expected content and stays in place.
    if matches(dir, expected) {
        let _ = fs::remove_dir_all(staging);
        return Ok(());
    }
    if dir.exists() {
        // Move the directory with other content aside in one step, so a
        // reader never sees a mix of old and new files.
        let stale = runs.join(format!("{STAGING_PREFIX}stale-{unique}"));
        if fs::rename(dir, &stale).is_ok() {
            let _ = fs::remove_dir_all(&stale);
        }
    }
    if let Err(error) = fs::rename(staging, dir) {
        let _ = fs::remove_dir_all(staging);
        if matches(dir, expected) {
            return Ok(());
        }
        return Err(error);
    }
    Ok(())
}

fn write_staging(staging: &Path, expected: &[(String, Content<'_>)]) -> std::io::Result<()> {
    for (relative, content) in expected {
        let target = staging.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        match content {
            Content::Text(text) => fs::write(&target, text)?,
            Content::Copy(source) => {
                fs::copy(source, &target)?;
            }
        }
    }
    Ok(())
}

/// The last use of a run directory: the modification time of its marker,
/// else of the directory.
fn last_used(dir: &Path) -> Option<SystemTime> {
    fs::metadata(dir.join(LAST_USED))
        .or_else(|_| fs::metadata(dir))
        .and_then(|metadata| metadata.modified())
        .ok()
}

/// Removes the run directories below `runs` that are not among the
/// [`RUN_DIRS_KEPT`] most recently used ones and that no run used within
/// [`RUN_DIR_GRACE`]. It also removes staging directories older than
/// [`RUN_DIR_GRACE`], which a crash can leave. It never removes `in_use`.
/// A failed removal leaves the directory for a later run.
fn prune_run_dirs(runs: &Path, in_use: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(runs) else {
        return;
    };
    let expired = |used: SystemTime| {
        now.duration_since(used)
            .is_ok_and(|age| age > RUN_DIR_GRACE)
    };
    let mut candidates: Vec<(SystemTime, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path == in_use || !path.is_dir() {
            continue;
        }
        let used = last_used(&path).unwrap_or(UNIX_EPOCH);
        let staging = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(STAGING_PREFIX));
        if staging {
            if expired(used) {
                let _ = fs::remove_dir_all(&path);
            }
        } else {
            candidates.push((used, path));
        }
    }
    candidates.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    for (used, path) in candidates.into_iter().skip(RUN_DIRS_KEPT) {
        if expired(used) {
            let _ = fs::remove_dir_all(&path);
        }
    }
}

#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_run_dir_holds_each_file_and_the_path_map_under_a_content_hash() {
        let cache = tempfile::tempdir().expect("dir");
        let files = [
            RunFile {
                path: "src/b.ts",
                code: "// @ts-nocheck\nb",
            },
            RunFile {
                path: "src/a.ts",
                code: "// @ts-nocheck\na",
            },
        ];
        let run = write_run_dir(cache.path(), &files, None).expect("run dir");
        assert_eq!(
            run.path_map,
            BTreeMap::from([
                ("src/a.ts".to_owned(), "files/src/a.ts".to_owned()),
                ("src/b.ts".to_owned(), "files/src/b.ts".to_owned()),
            ])
        );
        assert_eq!(
            fs::read_to_string(run.dir.join("files/src/b.ts")).expect("file"),
            "// @ts-nocheck\nb"
        );
        let paths: BTreeMap<String, String> = serde_json::from_str(
            &fs::read_to_string(run.dir.join("paths.json")).expect("paths.json"),
        )
        .expect("json");
        assert_eq!(paths, run.path_map);

        let changed = [
            files[0],
            RunFile {
                path: "src/a.ts",
                code: "// @ts-nocheck\nA",
            },
        ];
        let other = write_run_dir(cache.path(), &changed, None).expect("run dir");
        assert_ne!(other.dir, run.dir, "other text gets another directory");
        let reordered = [files[1], files[0]];
        assert_eq!(
            write_run_dir(cache.path(), &reordered, None)
                .expect("run dir")
                .dir,
            run.dir,
            "the file order does not change the hash"
        );
    }

    #[test]
    fn a_path_that_leaves_the_run_dir_is_refused() {
        let cache = tempfile::tempdir().expect("dir");
        for path in ["../escape.ts", "/abs.ts", "src/../../x.ts", ""] {
            let error =
                write_run_dir(cache.path(), &[RunFile { path, code: "x" }], None).unwrap_err();
            assert!(
                error.contains("not a plain relative path"),
                "{path}: {error}"
            );
        }
        assert!(!cache.path().join("escape.ts").exists());
    }

    fn set_mtime(path: &Path, time: SystemTime) {
        fs::File::options()
            .write(true)
            .open(path)
            .expect("open")
            .set_modified(time)
            .expect("set mtime");
    }

    #[test]
    fn a_run_dir_with_the_expected_content_is_reused_without_a_rewrite() {
        let cache = tempfile::tempdir().expect("dir");
        let files = [RunFile {
            path: "src/a.ts",
            code: "a",
        }];
        let first = write_run_dir(cache.path(), &files, None).expect("run dir");
        let file = first.dir.join("files/src/a.ts");
        let old = UNIX_EPOCH + Duration::from_secs(1_000_000);
        set_mtime(&file, old);
        let second = write_run_dir(cache.path(), &files, None).expect("run dir");
        assert_eq!(second, first);
        assert_eq!(
            fs::metadata(&file)
                .expect("meta")
                .modified()
                .expect("mtime"),
            old,
            "a verified run dir is not written again"
        );
    }

    #[test]
    fn a_run_dir_with_other_content_is_replaced() {
        let cache = tempfile::tempdir().expect("dir");
        let files = [RunFile {
            path: "src/a.ts",
            code: "a",
        }];
        let run = write_run_dir(cache.path(), &files, None).expect("run dir");
        fs::write(run.dir.join("files/src/a.ts"), "tampered").expect("write");
        fs::write(run.dir.join("files/extra.ts"), "extra").expect("write");
        write_run_dir(cache.path(), &files, None).expect("run dir");
        assert_eq!(
            fs::read_to_string(run.dir.join("files/src/a.ts")).expect("file"),
            "a"
        );
        assert!(
            !run.dir.join("files/extra.ts").exists(),
            "the replaced directory has no stale file"
        );
        let leftovers: Vec<_> = fs::read_dir(cache.path().join("runs"))
            .expect("runs")
            .flatten()
            .map(|entry| entry.file_name())
            .filter(|name| name.to_string_lossy().starts_with(STAGING_PREFIX))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    /// Sets the stop flag when it drops, so a reader loop also ends when a
    /// writer thread panics.
    struct StopOnDrop<'a>(&'a std::sync::atomic::AtomicBool);

    impl Drop for StopOnDrop<'_> {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Runs on one plan start together on a cold cache, or on a cache that
    /// holds a stale directory with the same hash. Once a complete directory
    /// is in place, a reader must never see a partial file or a missing
    /// file: another run may use the directory at that moment.
    #[test]
    fn concurrent_runs_on_one_plan_each_get_a_complete_run_dir() {
        const ROUNDS: usize = 40;
        const WRITERS: usize = 4;
        let code = "x".repeat(64 * 1024);
        let files = [
            RunFile {
                path: "src/a.ts",
                code: &code,
            },
            RunFile {
                path: "src/b.ts",
                code: "b",
            },
        ];
        for round in 0..ROUNDS {
            let cache = tempfile::tempdir().expect("dir");
            let dir = cache.path().join("runs").join(content_hash(&files));
            if round % 2 == 1 {
                fs::create_dir_all(dir.join("files/src")).expect("stale dir");
                fs::write(dir.join("files/src/a.ts"), "stale").expect("stale file");
            }
            let instrumented = dir.join("files/src/a.ts");
            let done = std::sync::atomic::AtomicBool::new(false);
            let start = std::sync::Barrier::new(WRITERS + 1);
            let (torn, vanished, results) = std::thread::scope(|scope| {
                let reader = scope.spawn(|| {
                    start.wait();
                    let (mut torn, mut vanished, mut complete) = (0, 0, false);
                    while !done.load(std::sync::atomic::Ordering::Relaxed) {
                        match fs::read_to_string(&instrumented) {
                            Ok(text) if text == code => complete = true,
                            Ok(_) if complete => torn += 1,
                            Err(_) if complete => vanished += 1,
                            _ => {}
                        }
                    }
                    (torn, vanished)
                });
                let stop = StopOnDrop(&done);
                let writers: Vec<_> = std::iter::repeat_with(|| {
                    scope.spawn(|| {
                        start.wait();
                        write_run_dir(cache.path(), &files, None).map(|run| run.dir)
                    })
                })
                .take(WRITERS)
                .collect();
                let results: Vec<_> = writers.into_iter().map(|writer| writer.join()).collect();
                drop(stop);
                let (torn, vanished) = reader.join().expect("reader");
                (torn, vanished, results)
            });
            for result in results {
                let run = result.expect("writer").expect("run dir");
                assert_eq!(run, dir);
            }
            assert_eq!(torn, 0, "round {round}: a reader never sees a partial file");
            assert_eq!(
                vanished, 0,
                "round {round}: a complete run dir never disappears"
            );
            assert_eq!(fs::read_to_string(&instrumented).expect("file"), code);
            assert_eq!(
                fs::read_to_string(dir.join("files/src/b.ts")).expect("file"),
                "b"
            );
            let names: Vec<_> = fs::read_dir(cache.path().join("runs"))
                .expect("runs")
                .flatten()
                .map(|entry| entry.file_name())
                .filter(|name| !name.to_string_lossy().ends_with(".lock"))
                .collect();
            assert_eq!(names.len(), 1, "one run dir and no staging dir: {names:?}");
        }
    }

    #[test]
    fn old_run_dirs_beyond_the_kept_count_are_removed_but_never_the_one_in_use() {
        let cache = tempfile::tempdir().expect("dir");
        let base = UNIX_EPOCH + Duration::from_secs(2_000_000_000);
        let mut dirs = Vec::new();
        for index in 0..RUN_DIRS_KEPT + 3 {
            let code = format!("code {index}");
            let run = write_run_dir(
                cache.path(),
                &[RunFile {
                    path: "src/a.ts",
                    code: &code,
                }],
                None,
            )
            .expect("run dir");
            let age = Duration::from_secs(3600 * 24 * (index as u64 + 1));
            set_mtime(&run.dir.join(LAST_USED), base - age);
            dirs.push(run.dir);
        }
        let recent = dirs[RUN_DIRS_KEPT + 2].clone();
        let staging = cache.path().join("runs").join(".staging-1-1");
        fs::create_dir_all(&staging).expect("staging");

        prune_run_dirs(&cache.path().join("runs"), &recent, base);

        for (index, dir) in dirs.iter().enumerate() {
            let kept = index < RUN_DIRS_KEPT || *dir == recent;
            assert_eq!(dir.is_dir(), kept, "dir {index}");
        }
        assert!(
            !staging.exists(),
            "an expired staging dir from a crash is removed"
        );
    }

    #[test]
    fn a_run_dir_within_the_grace_time_is_kept_beyond_the_kept_count() {
        let cache = tempfile::tempdir().expect("dir");
        let now = SystemTime::now();
        let mut dirs = Vec::new();
        for index in 0..RUN_DIRS_KEPT + 2 {
            let code = format!("code {index}");
            let run = write_run_dir(
                cache.path(),
                &[RunFile {
                    path: "src/a.ts",
                    code: &code,
                }],
                None,
            )
            .expect("run dir");
            dirs.push(run.dir);
        }
        prune_run_dirs(&cache.path().join("runs"), &dirs[0], now);
        assert!(
            dirs.iter().all(|dir| dir.is_dir()),
            "a concurrent run that used its directory within the grace time keeps it"
        );
    }

    #[test]
    fn a_run_removes_expired_run_dirs_at_start() {
        let cache = tempfile::tempdir().expect("dir");
        let old = SystemTime::now() - RUN_DIR_GRACE - Duration::from_secs(60);
        let mut dirs = Vec::new();
        for index in 0..RUN_DIRS_KEPT + 2 {
            let code = format!("code {index}");
            let run = write_run_dir(
                cache.path(),
                &[RunFile {
                    path: "src/a.ts",
                    code: &code,
                }],
                None,
            )
            .expect("run dir");
            set_mtime(
                &run.dir.join(LAST_USED),
                old - Duration::from_secs(index as u64),
            );
            dirs.push(run.dir);
        }
        write_run_dir(
            cache.path(),
            &[RunFile {
                path: "src/a.ts",
                code: "new",
            }],
            None,
        )
        .expect("run dir");
        let remaining = fs::read_dir(cache.path().join("runs"))
            .expect("runs")
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .count();
        assert_eq!(
            remaining,
            RUN_DIRS_KEPT + 1,
            "the kept dirs and the new one"
        );
        assert!(dirs[0].is_dir(), "the newest old dir stays");
        assert!(!dirs[RUN_DIRS_KEPT + 1].is_dir(), "the oldest dir goes");
    }
}
