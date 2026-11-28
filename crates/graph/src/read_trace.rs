//! Trace of the paths that import resolution reads.
//!
//! The persisted graph cache stores import resolutions. A warm run can only
//! reuse them when the cache key covers each path that the resolution read.
//! With the `read-trace` feature, the file system calls of `oxc_resolver` and
//! the direct path checks of the resolution fallbacks report their paths
//! here, and `ReadRecorder` collects the paths below a scope directory.
//! Tests use the recorded read set to check that the cache key covers each
//! path.
//!
//! The trace does not see the manifest loads in `fallow_config` (the
//! `package.json` and Deno configs of the project and workspace roots, and
//! the `tsconfig.json` chains), or the read of `.pnp.cjs` in the `pnp` crate.
//! The resolver input hash reads each of these files by name.
//!
//! Without the feature, the trace records nothing, so a release build pays
//! no cost for it. The helpers report missed paths and read package
//! manifests to [`crate::missed_probes`] in every build.

use std::io;
use std::path::{Path, PathBuf};

use oxc_resolver::{FileMetadata, FileSystem, FileSystemOs, ResolveError};

use crate::missed_probes;

/// What a resolution step learned from a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ReadKind {
    /// The step read the content of the path.
    Content,
    /// The step checked if the path exists, or if it is a file.
    File,
    /// The step checked if the path is a directory.
    Dir,
}

/// The file system of the `oxc_resolver` instances that resolution builds.
/// It reports each missed path to [`crate::missed_probes`], and with the
/// `read-trace` feature each call to the read trace.
pub(crate) type ResolverFs = ProbeFs;

/// The resolver type of the resolution stage.
pub(crate) type Resolver = oxc_resolver::ResolverGeneric<ResolverFs>;

#[cfg(feature = "read-trace")]
pub use recording::{ReadEvent, ReadRecorder};

#[cfg(feature = "read-trace")]
use recording::note;

#[cfg(not(feature = "read-trace"))]
#[inline(always)]
const fn note(_path: &Path, _kind: ReadKind) {}

/// Path checks that report the path to the read trace.
pub(crate) trait TracedPath {
    /// [`Path::is_file`], recorded as a [`ReadKind::File`] read.
    fn traced_is_file(&self) -> bool;
    /// [`Path::is_dir`], recorded as a [`ReadKind::Dir`] read.
    fn traced_is_dir(&self) -> bool;
    /// [`Path::exists`], recorded as a [`ReadKind::File`] read.
    fn traced_exists(&self) -> bool;
}

impl TracedPath for Path {
    fn traced_is_file(&self) -> bool {
        note(self, ReadKind::File);
        checked_metadata(self).is_ok_and(|metadata| metadata.is_file())
    }

    fn traced_is_dir(&self) -> bool {
        note(self, ReadKind::Dir);
        checked_metadata(self).is_ok_and(|metadata| metadata.is_dir())
    }

    fn traced_exists(&self) -> bool {
        note(self, ReadKind::File);
        checked_metadata(self).is_ok()
    }
}

/// [`std::fs::metadata`], with a missing path reported as a missed probe.
fn checked_metadata(path: &Path) -> io::Result<std::fs::Metadata> {
    let result = std::fs::metadata(path);
    missed_probes::note_if_missing(path, &result);
    result
}

/// [`std::fs::read_to_string`], recorded as a content read.
pub(crate) fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
    let path = path.as_ref();
    let result = std::fs::read_to_string(path);
    missed_probes::note_read(path, &result);
    note(path, content_kind(result.is_ok()));
    result
}

/// [`dunce::canonicalize`], recorded as a [`ReadKind::File`] read.
pub(crate) fn canonicalize(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    let path = path.as_ref();
    note(path, ReadKind::File);
    let result = realpath(path);
    missed_probes::note_if_missing(path, &result);
    result
}

/// [`dunce::canonicalize`] of a directory, not recorded. The caller records
/// each file that it resolves below the directory, so the read trace keeps
/// the same paths as a [`canonicalize`] of each file.
pub(crate) fn canonicalize_dir_untraced(dir: &Path) -> io::Result<PathBuf> {
    realpath(dir)
}

fn realpath(path: &Path) -> io::Result<PathBuf> {
    #[cfg(test)]
    REALPATH_CALLS.with(|calls| calls.set(calls.get() + 1));
    dunce::canonicalize(path)
}

/// [`canonicalize`] for a path that resolution only probes, which often does
/// not exist. `realpath` makes one system call for each path component
/// before it reaches a missing leaf. One `lstat` finds the miss in a single
/// system call.
pub(crate) fn canonicalize_probe(path: &Path) -> io::Result<PathBuf> {
    if let Err(error) = std::fs::symlink_metadata(path)
        && matches!(
            error.kind(),
            io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
        )
    {
        note(path, ReadKind::File);
        let result = Err(error);
        missed_probes::note_if_missing(path, &result);
        return result;
    }
    canonicalize(path)
}

/// Record that a step learned that `path` exists, as a [`ReadKind::File`]
/// read, when it found the path without a call to [`canonicalize`].
pub(crate) fn note_existing_file(path: &Path) {
    note(path, ReadKind::File);
}

#[cfg(test)]
thread_local! {
    static REALPATH_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The number of `realpath` calls on this thread.
#[cfg(all(test, unix))]
pub(crate) fn realpath_calls() -> u64 {
    REALPATH_CALLS.with(std::cell::Cell::get)
}

/// The resolver of a project at `root`. For a root in an audit snapshot, the
/// resolver finds no `node_modules`, `package.json`, `tsconfig.json` or Yarn
/// PnP file above the snapshot root (see [`fallow_types::audit_snapshot`]).
pub(crate) fn resolver_for_root(root: &Path, options: oxc_resolver::ResolveOptions) -> Resolver {
    let ceiling = fallow_types::audit_snapshot::snapshot_root(root).map(Path::to_path_buf);
    let fs = ProbeFs(FileSystemOs::new(options.yarn_pnp), ceiling);
    Resolver::new_with_file_system(fs, options)
}

/// The operating system file system of the resolver. Each call that finds
/// no path reports it as a missed probe, and each call goes to the read
/// trace. With a snapshot root, a lookup above that root finds nothing.
pub(crate) struct ProbeFs(FileSystemOs, Option<PathBuf>);

impl ProbeFs {
    fn refuse(&self, path: &Path) -> io::Result<()> {
        match &self.1 {
            Some(ceiling) if fallow_types::audit_snapshot::is_lookup_above(ceiling, path) => {
                Err(io::Error::from(io::ErrorKind::NotFound))
            }
            _ => Ok(()),
        }
    }
}

impl FileSystem for ProbeFs {
    fn new(yarn_pnp: bool) -> Self {
        Self(FileSystemOs::new(yarn_pnp), None)
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.refuse(path)?;
        let result = self.0.read(path);
        missed_probes::note_read(path, &result);
        note(path, content_kind(result.is_ok()));
        result
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.refuse(path)?;
        let result = self.0.read_to_string(path);
        missed_probes::note_read(path, &result);
        note(path, content_kind(result.is_ok()));
        result
    }

    fn metadata(&self, path: &Path) -> io::Result<FileMetadata> {
        note(path, ReadKind::File);
        self.refuse(path)?;
        let result = self.0.metadata(path);
        missed_probes::note_if_missing(path, &result);
        result
    }

    fn symlink_metadata(&self, path: &Path) -> io::Result<FileMetadata> {
        note(path, ReadKind::File);
        self.refuse(path)?;
        let result = self.0.symlink_metadata(path);
        missed_probes::note_if_missing(path, &result);
        result
    }

    fn read_link(&self, path: &Path) -> Result<PathBuf, ResolveError> {
        note(path, ReadKind::File);
        self.0.read_link(path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        note(path, ReadKind::File);
        self.refuse(path)?;
        let result = self.0.canonicalize(path);
        missed_probes::note_if_missing(path, &result);
        result
    }
}

/// A content read of a missing file only learned that the file is absent.
const fn content_kind(found: bool) -> ReadKind {
    if found {
        ReadKind::Content
    } else {
        ReadKind::File
    }
}

#[cfg(feature = "read-trace")]
mod recording {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    use rustc_hash::FxHashSet;

    use super::ReadKind;

    /// One recorded read.
    #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub struct ReadEvent {
        /// The absolute path, as the resolution step spelled it.
        pub path: PathBuf,
        /// What the step learned from the path.
        pub kind: ReadKind,
    }

    struct Active {
        id: u64,
        scopes: Vec<PathBuf>,
        events: FxHashSet<ReadEvent>,
    }

    /// The number of open recorders. A zero count keeps [`note`] at one
    /// atomic load, so tests that do not record pay almost nothing.
    static OPEN: AtomicUsize = AtomicUsize::new(0);
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    static RECORDERS: Mutex<Vec<Active>> = Mutex::new(Vec::new());

    /// Collects the reads below one scope directory until [`Self::finish`].
    ///
    /// Recorders are process-wide, because resolution runs on rayon workers.
    /// Tests that run at the same time stay apart when each one uses its own
    /// temporary directory as the scope.
    #[derive(Debug)]
    pub struct ReadRecorder {
        id: u64,
    }

    impl ReadRecorder {
        /// Start to record the reads of paths below `scope`. The recorder
        /// also matches the canonical spelling of `scope`.
        #[must_use]
        pub fn start(scope: &Path) -> Self {
            let mut scopes = vec![scope.to_path_buf()];
            if let Ok(canonical) = dunce::canonicalize(scope)
                && canonical != scope
            {
                scopes.push(canonical);
            }
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            RECORDERS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(Active {
                    id,
                    scopes,
                    events: FxHashSet::default(),
                });
            OPEN.fetch_add(1, Ordering::SeqCst);
            Self { id }
        }

        /// Stop the recorder and return its reads, sorted by path and kind.
        #[must_use]
        pub fn finish(self) -> Vec<ReadEvent> {
            let Some(active) = remove(self.id) else {
                return Vec::new();
            };
            let mut events: Vec<ReadEvent> = active.events.into_iter().collect();
            events.sort_unstable();
            events
        }
    }

    /// A test that panics before [`ReadRecorder::finish`] must not leave the
    /// recorder open, or each later file system call takes the lock.
    impl Drop for ReadRecorder {
        fn drop(&mut self) {
            let _ = remove(self.id);
        }
    }

    fn remove(id: u64) -> Option<Active> {
        let active = {
            let mut recorders = RECORDERS.lock().unwrap_or_else(|e| e.into_inner());
            let index = recorders.iter().position(|active| active.id == id)?;
            recorders.swap_remove(index)
        };
        OPEN.fetch_sub(1, Ordering::SeqCst);
        Some(active)
    }

    pub fn note(path: &Path, kind: ReadKind) {
        if OPEN.load(Ordering::Relaxed) == 0 || !path.is_absolute() {
            return;
        }
        let mut recorders = RECORDERS.lock().unwrap_or_else(|e| e.into_inner());
        for active in recorders.iter_mut() {
            if active.scopes.iter().any(|scope| path.starts_with(scope)) {
                active.events.insert(ReadEvent {
                    path: path.to_path_buf(),
                    kind,
                });
            }
        }
    }
}
