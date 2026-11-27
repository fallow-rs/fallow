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
    refuse_link(mutation_cache)?;
    refuse_link(&runs)?;
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
    mark_used(&dir);
    prune_run_dirs(&runs, &dir, SystemTime::now());
    Ok(RunDir { dir, path_map })
}

/// Refuses `path` when it is a symlink, so a run never writes or removes
/// run directories through a link. A path that does not exist yet passes.
/// Metadata that cannot be read refuses the path.
fn refuse_link(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(format!(
            "{} is a symlink, so fallow does not write the mutation run directories through it. Remove the link",
            path.display()
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot read {}: {error}", path.display())),
    }
}

/// Replaces the marker of `dir` with the current time. The old entry is
/// removed and the new file is created with `create_new`, which does not
/// follow a symlink, so a link at the marker is replaced and never written
/// through. A failed write only makes the prune order less exact.
fn mark_used(dir: &Path) {
    use std::io::Write as _;
    let marker = dir.join(LAST_USED);
    let _ = fs::remove_file(&marker);
    if let Ok(mut file) = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)
    {
        let _ = file.write_all(now_nanos().to_string().as_bytes());
    }
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos())
}

/// True when `dir` is a directory, not a symlink, and each expected file of
/// it has the expected content. A link at `dir` does not match, so the run
/// moves the link aside and installs a real directory.
fn matches(dir: &Path, expected: &[(String, Content<'_>)]) -> bool {
    fs::symlink_metadata(dir).is_ok_and(|meta| meta.is_dir())
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
    let lock = open_publication_lock(&publication_lock_path(dir));
    let result = publish(runs, dir, &staging, &unique, expected);
    drop(lock);
    result
}

/// Opens and locks the publication lock file. A symlink at `path` is never
/// followed: the run then publishes without the lock.
fn open_publication_lock(path: &Path) -> Option<fs::File> {
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return None;
    }
    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(false).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options
        .open(path)
        .and_then(|file| file.lock().map(|()| file))
        .ok()
}

/// Moves `staging` to `dir` under the publication lock of `dir`.
fn publish(
    runs: &Path,
    dir: &Path,
    staging: &Path,
    unique: &str,
    expected: &[(String, Content<'_>)],
) -> std::io::Result<()> {
    let mut attempt = 1;
    loop {
        // A concurrent run can install the same directory while this run
        // stages. That directory has the expected content and stays in place.
        if matches(dir, expected) {
            let _ = fs::remove_dir_all(staging);
            return Ok(());
        }
        if fs::symlink_metadata(dir).is_ok() {
            // Move the directory with other content aside in one step, so a
            // reader never sees a mix of old and new files.
            let stale = runs.join(format!("{STAGING_PREFIX}stale-{unique}-{attempt}"));
            if fs::rename(dir, &stale).is_ok() {
                let _ = fs::remove_dir_all(&stale);
            }
        }
        match fs::rename(staging, dir) {
            Ok(()) => return Ok(()),
            // Windows refuses to move a directory while a reader has a file
            // in it open, so the stale directory stays and the rename onto it
            // fails. The reader closes the file soon, so try again.
            Err(_) if cfg!(windows) && attempt < PUBLISH_ATTEMPTS => {
                attempt += 1;
                std::thread::sleep(PUBLISH_RETRY_DELAY);
            }
            Err(error) => {
                let _ = fs::remove_dir_all(staging);
                if matches(dir, expected) {
                    return Ok(());
                }
                return Err(error);
            }
        }
    }
}

/// How often [`publish`] tries to replace a directory with other content on
/// Windows. Other platforms move a directory with open files, so a failed
/// rename there is a real error and is not tried again.
const PUBLISH_ATTEMPTS: u32 = 50;

/// The wait between two attempts of [`publish`].
const PUBLISH_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(20);

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
/// A failed removal leaves the directory for a later run. It removes nothing
/// when `runs` is a symlink, and it skips an entry that is a symlink, so it
/// never removes a directory through a link.
fn prune_run_dirs(runs: &Path, in_use: &Path, now: SystemTime) {
    if !fs::symlink_metadata(runs).is_ok_and(|meta| meta.is_dir()) {
        return;
    }
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
        if path == in_use || !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
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

    /// A directory with an old `.last-used` marker, so the prune counts it
    /// as expired.
    #[cfg(unix)]
    fn old_dir(parent: &Path, name: &str) -> PathBuf {
        let dir = parent.join(name);
        fs::create_dir_all(&dir).expect("dir");
        let marker = dir.join(LAST_USED);
        fs::write(&marker, "0").expect("marker");
        set_mtime(&marker, SystemTime::now() - RUN_DIR_GRACE * 24);
        dir
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_runs_dir_is_refused_and_its_target_keeps_each_dir() {
        let base = tempfile::tempdir().expect("dir");
        let cache = base.path().join("cache");
        let shared = base.path().join("shared");
        fs::create_dir_all(&cache).expect("cache");
        let dirs: Vec<PathBuf> = (0..RUN_DIRS_KEPT + 3)
            .map(|index| old_dir(&shared, &format!("dir-{index}")))
            .collect();
        std::os::unix::fs::symlink(&shared, cache.join("runs")).expect("link");
        let error = write_run_dir(
            &cache,
            &[RunFile {
                path: "src/a.ts",
                code: "a",
            }],
            None,
        )
        .expect_err("refused");
        assert!(error.contains("symlink"), "{error}");
        for dir in &dirs {
            assert!(dir.is_dir(), "{} stays", dir.display());
        }
        assert_eq!(fs::read_dir(&shared).expect("shared").count(), dirs.len());
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_publication_lock_is_not_followed() {
        let base = tempfile::tempdir().expect("dir");
        let cache = base.path().join("cache");
        let outside = base.path().join("outside");
        fs::create_dir_all(cache.join("runs")).expect("runs");
        fs::create_dir_all(&outside).expect("outside");
        let files = [RunFile {
            path: "src/a.ts",
            code: "a",
        }];
        let dir = cache.join("runs").join(content_hash(&files));
        std::os::unix::fs::symlink(outside.join("created"), publication_lock_path(&dir))
            .expect("link");
        write_run_dir(&cache, &files, None).expect("run dir");
        assert!(!outside.join("created").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_run_dir_link_is_replaced_by_a_real_one() {
        let base = tempfile::tempdir().expect("dir");
        let cache = base.path().join("cache");
        fs::create_dir_all(cache.join("runs")).expect("runs");
        let files = [RunFile {
            path: "src/a.ts",
            code: "a",
        }];
        let dir = cache.join("runs").join(content_hash(&files));
        std::os::unix::fs::symlink(base.path().join("missing"), &dir).expect("link");
        write_run_dir(&cache, &files, None).expect("run dir");
        let meta = fs::symlink_metadata(&dir).expect("dir");
        assert!(meta.is_dir() && !meta.file_type().is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_mutation_cache_is_refused() {
        let base = tempfile::tempdir().expect("dir");
        let shared = base.path().join("shared");
        fs::create_dir_all(&shared).expect("shared");
        let cache = base.path().join("mutation");
        std::os::unix::fs::symlink(&shared, &cache).expect("link");
        let error = write_run_dir(
            &cache,
            &[RunFile {
                path: "src/a.ts",
                code: "a",
            }],
            None,
        )
        .expect_err("refused");
        assert!(error.contains("symlink"), "{error}");
        assert_eq!(fs::read_dir(&shared).expect("shared").count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_last_used_marker_is_replaced_and_its_target_keeps_its_content() {
        let base = tempfile::tempdir().expect("dir");
        let cache = base.path().join("cache");
        let kept = base.path().join("kept.txt");
        fs::write(&kept, "original").expect("kept");
        let files = [RunFile {
            path: "src/a.ts",
            code: "a",
        }];
        let run = write_run_dir(&cache, &files, None).expect("run dir");
        let marker = run.dir.join(LAST_USED);
        fs::remove_file(&marker).expect("remove marker");
        std::os::unix::fs::symlink(&kept, &marker).expect("link");
        write_run_dir(&cache, &files, None).expect("reused run dir");
        assert_eq!(fs::read_to_string(&kept).expect("kept"), "original");
        let meta = fs::symlink_metadata(&marker).expect("marker");
        assert!(meta.is_file(), "the marker is a plain file again");
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_run_dir_is_replaced_by_a_real_one() {
        let base = tempfile::tempdir().expect("dir");
        let cache = base.path().join("cache");
        let files = [RunFile {
            path: "src/a.ts",
            code: "a",
        }];
        let run = write_run_dir(&cache, &files, None).expect("run dir");
        let moved = base.path().join("moved");
        fs::rename(&run.dir, &moved).expect("move");
        std::os::unix::fs::symlink(&moved, &run.dir).expect("link");
        let marker = moved.join(LAST_USED);
        fs::write(&marker, "original").expect("marker");
        let again = write_run_dir(&cache, &files, None).expect("run dir");
        assert_eq!(again.dir, run.dir);
        let meta = fs::symlink_metadata(&again.dir).expect("run dir");
        assert!(meta.is_dir(), "the run dir is a real directory");
        assert_eq!(fs::read_to_string(&marker).expect("marker"), "original");
        assert!(
            moved.join("files/src/a.ts").is_file(),
            "the link target stays"
        );
    }
}
