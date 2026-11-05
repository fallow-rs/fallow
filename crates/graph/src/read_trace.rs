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
//! Without the feature, the helpers call the file system directly and record
//! nothing, so a release build pays no cost.

use std::io;
use std::path::{Path, PathBuf};

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
#[cfg(feature = "read-trace")]
pub(crate) type ResolverFs = recording::TracingFs;
/// The file system of the `oxc_resolver` instances that resolution builds.
#[cfg(not(feature = "read-trace"))]
pub(crate) type ResolverFs = oxc_resolver::FileSystemOs;

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
        self.is_file()
    }

    fn traced_is_dir(&self) -> bool {
        note(self, ReadKind::Dir);
        self.is_dir()
    }

    fn traced_exists(&self) -> bool {
        note(self, ReadKind::File);
        self.exists()
    }
}

/// [`std::fs::read_to_string`], recorded as a content read.
pub(crate) fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
    let path = path.as_ref();
    let result = std::fs::read_to_string(path);
    note(path, content_kind(result.is_ok()));
    result
}

/// [`dunce::canonicalize`], recorded as a [`ReadKind::File`] read.
pub(crate) fn canonicalize(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    let path = path.as_ref();
    note(path, ReadKind::File);
    dunce::canonicalize(path)
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
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    use oxc_resolver::{FileMetadata, FileSystem, FileSystemOs, ResolveError};
    use rustc_hash::FxHashSet;

    use super::{ReadKind, content_kind};

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

    /// The operating system file system, with each call noted in the trace.
    pub struct TracingFs(FileSystemOs);

    impl FileSystem for TracingFs {
        fn new(yarn_pnp: bool) -> Self {
            Self(FileSystemOs::new(yarn_pnp))
        }

        fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
            let result = self.0.read(path);
            note(path, content_kind(result.is_ok()));
            result
        }

        fn read_to_string(&self, path: &Path) -> io::Result<String> {
            let result = self.0.read_to_string(path);
            note(path, content_kind(result.is_ok()));
            result
        }

        fn metadata(&self, path: &Path) -> io::Result<FileMetadata> {
            note(path, ReadKind::File);
            self.0.metadata(path)
        }

        fn symlink_metadata(&self, path: &Path) -> io::Result<FileMetadata> {
            note(path, ReadKind::File);
            self.0.symlink_metadata(path)
        }

        fn read_link(&self, path: &Path) -> Result<PathBuf, ResolveError> {
            note(path, ReadKind::File);
            self.0.read_link(path)
        }

        fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
            note(path, ReadKind::File);
            self.0.canonicalize(path)
        }
    }
}
