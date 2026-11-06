//! A private temp directory for each companion.
//!
//! Test code writes temp files under `os.tmpdir()`, and its teardown deletes
//! them. When the API kills a companion (a watchdog timeout, a protocol
//! fault), no teardown runs, and each kill can leave a large directory in the
//! system temp directory. The API points `TMPDIR`, `TMP` and `TEMP` of each
//! companion to a directory that it owns, and deletes that directory after
//! the companion ends.
//!
//! A lock file next to each directory marks its owner. The owner holds an
//! exclusive lock on the file while the directory is in use, and the
//! operating system releases the lock when the owner ends. A directory whose
//! lock is free belongs to an API that ended without cleanup (a signal or a
//! crash), so the first companion of the next run removes it.

use std::collections::hash_map::RandomState;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::hash::BuildHasher;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;
use std::time::{Duration, SystemTime};

/// The name prefix of each companion temp directory and its lock file. The
/// name stays short: a Unix socket path below the directory (for example an
/// IPC pipe of `tsx`) must fit in about 100 bytes, and the macOS temp
/// directory already takes about 50.
const PREFIX: &str = "fallow-mt-";
/// The name suffix of a lock file.
const LOCK_SUFFIX: &str = ".lock";
/// The variables that give the temp directory to Node, Bun and their children.
/// `os.tmpdir()` reads `TMPDIR` on Unix and `TEMP` or `TMP` on Windows.
const TEMP_VARIABLES: [&str; 3] = ["TMPDIR", "TMP", "TEMP"];
/// A lock file younger than this can belong to an owner that did not lock it
/// yet, so a sweep keeps it.
const SWEEP_MIN_AGE: Duration = Duration::from_secs(60);
/// How many names `create` tries before it gives up.
const CREATE_ATTEMPTS: u32 = 16;
/// How often drop tries to delete the directory. A process of the killed tree
/// can still write a file, or hold a handle on Windows, for a short time.
const REMOVE_ATTEMPTS: u32 = 5;
/// The pause between two delete attempts.
const REMOVE_PAUSE: Duration = Duration::from_millis(50);

/// A temp directory of one companion. Drop deletes it.
#[derive(Debug)]
pub struct CompanionTemp {
    dir: PathBuf,
    lock_path: PathBuf,
    lock: Option<File>,
}

impl CompanionTemp {
    /// Creates a new temp directory below `parent`. The first call of each
    /// process also removes the stale directories below `parent`.
    pub fn create(parent: &Path) -> io::Result<Self> {
        static SWEEP: Once = Once::new();
        SWEEP.call_once(|| sweep_stale(parent, SystemTime::now()));
        for _ in 0..CREATE_ATTEMPTS {
            let name = format!("{PREFIX}{}", unique_suffix());
            let lock_path = parent.join(format!("{name}{LOCK_SUFFIX}"));
            let lock = match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(lock) => lock,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            };
            let dir = parent.join(&name);
            if let Err(error) = lock.lock().and_then(|()| fs::create_dir(&dir)) {
                drop(lock);
                let _ = fs::remove_file(&lock_path);
                return Err(error);
            }
            return Ok(Self {
                dir,
                lock_path,
                lock: Some(lock),
            });
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "no free name for a companion temp directory",
        ))
    }

    #[cfg(test)]
    fn dir(&self) -> &Path {
        &self.dir
    }

    /// Points the temp variables of `command` to the directory.
    pub fn apply(&self, command: &mut Command) {
        for variable in TEMP_VARIABLES {
            command.env(variable, &self.dir);
        }
    }
}

impl Drop for CompanionTemp {
    fn drop(&mut self) {
        let removed = (0..REMOVE_ATTEMPTS).any(|attempt| {
            if attempt > 0 {
                std::thread::sleep(REMOVE_PAUSE);
            }
            remove_dir(&self.dir)
        });
        // The lock goes first: Windows cannot delete a file that is open.
        drop(self.lock.take());
        // A directory that is still there keeps its lock file, so that a
        // later sweep can find it and try again.
        if removed {
            let _ = fs::remove_file(&self.lock_path);
        }
    }
}

/// Deletes `dir`. True when it is gone.
fn remove_dir(dir: &Path) -> bool {
    match fs::remove_dir_all(dir) {
        Ok(()) => true,
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

/// A name part that no other directory of this or another process has.
fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let hash = RandomState::new().hash_one((nanos, std::process::id()));
    format!("{:x}-{:08x}", std::process::id(), hash as u32)
}

/// Removes each companion temp directory below `parent` whose lock file is
/// older than [`SWEEP_MIN_AGE`] at `now` and has no owner.
fn sweep_stale(parent: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name
            .to_str()
            .and_then(|name| name.strip_prefix(PREFIX))
            .and_then(|name| name.strip_suffix(LOCK_SUFFIX))
        else {
            continue;
        };
        let lock_path = entry.path();
        let old = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|modified| {
                now.duration_since(modified)
                    .is_ok_and(|age| age >= SWEEP_MIN_AGE)
            });
        if old {
            remove_if_unowned(&lock_path, &parent.join(format!("{PREFIX}{name}")));
        }
    }
}

fn remove_if_unowned(lock_path: &Path, dir: &Path) {
    let Ok(lock) = OpenOptions::new().write(true).open(lock_path) else {
        return;
    };
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock | TryLockError::Error(_)) => return,
    }
    let removed = remove_dir(dir);
    drop(lock);
    if removed {
        let _ = fs::remove_file(lock_path);
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
mod tests {
    use super::*;

    fn later() -> SystemTime {
        SystemTime::now() + SWEEP_MIN_AGE * 2
    }

    #[test]
    fn drop_deletes_the_directory_and_its_files() {
        let parent = tempfile::tempdir().unwrap();
        let temp = CompanionTemp::create(parent.path()).unwrap();
        let dir = temp.dir().to_path_buf();
        fs::create_dir_all(dir.join("fallow-vitest-run-1")).unwrap();
        fs::write(dir.join("fallow-vitest-run-1/file"), "data").unwrap();
        drop(temp);
        assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 0);
    }

    #[test]
    fn apply_sets_each_temp_variable_to_the_directory() {
        let parent = tempfile::tempdir().unwrap();
        let temp = CompanionTemp::create(parent.path()).unwrap();
        let mut command = Command::new("node");
        temp.apply(&mut command);
        let values: Vec<_> = command
            .get_envs()
            .map(|(name, value)| (name.to_owned(), value.map(Path::new)))
            .collect();
        for variable in TEMP_VARIABLES {
            assert!(
                values.contains(&(variable.into(), Some(temp.dir()))),
                "{variable} is not set"
            );
        }
    }

    #[test]
    fn a_sweep_removes_a_directory_without_an_owner() {
        let parent = tempfile::tempdir().unwrap();
        let stale = parent.path().join(format!("{PREFIX}1-dead"));
        fs::create_dir_all(stale.join("fallow-vitest-run-1")).unwrap();
        fs::write(
            parent.path().join(format!("{PREFIX}1-dead{LOCK_SUFFIX}")),
            "",
        )
        .unwrap();
        sweep_stale(parent.path(), later());
        assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_sweep_keeps_a_directory_whose_owner_holds_the_lock() {
        let parent = tempfile::tempdir().unwrap();
        let temp = CompanionTemp::create(parent.path()).unwrap();
        sweep_stale(parent.path(), later());
        assert!(temp.dir().is_dir());
        assert!(temp.lock_path.is_file());
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_that_drop_cannot_delete_keeps_its_lock_file_for_a_sweep() {
        use std::os::unix::fs::PermissionsExt;
        let parent = tempfile::tempdir().unwrap();
        let temp = CompanionTemp::create(parent.path()).unwrap();
        let locked = temp.dir().join("locked");
        fs::create_dir(&locked).unwrap();
        fs::write(locked.join("file"), "data").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
        let lock_path = temp.lock_path.clone();
        drop(temp);
        let kept = lock_path.is_file();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(kept, "the lock file goes while the directory stays");
        // A child that a parallel test forks holds a copy of the lock file
        // until it calls exec, so the lock can stay taken for a short time.
        let started = std::time::Instant::now();
        while fs::read_dir(parent.path()).unwrap().count() > 0
            && started.elapsed() < Duration::from_secs(5)
        {
            sweep_stale(parent.path(), later());
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_sweep_keeps_a_young_lock_file_and_other_names() {
        let parent = tempfile::tempdir().unwrap();
        let young = parent.path().join(format!("{PREFIX}1-new"));
        fs::create_dir(&young).unwrap();
        fs::write(
            parent.path().join(format!("{PREFIX}1-new{LOCK_SUFFIX}")),
            "",
        )
        .unwrap();
        let other = parent.path().join("fallow-vitest-run-2");
        fs::create_dir(&other).unwrap();
        sweep_stale(parent.path(), SystemTime::now());
        assert!(young.is_dir());
        assert!(other.is_dir());
    }
}
