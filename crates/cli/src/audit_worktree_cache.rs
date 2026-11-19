//! The parse cache of the checkout, lent to an analysis root in a temporary
//! worktree of `fallow audit`.
//!
//! The parse cache keys each entry by the root-relative path and the content
//! hash, so a worktree file whose content differs from the cached entry is
//! parsed again. A new file in a worktree never matches the metadata of a
//! cached entry, so the run reads and hashes each file. The module graph and
//! the duplication caches record absolute paths of the worktree. They stay in
//! the worktree and go away with it, so they never replace the caches of the
//! checkout.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The file name of the parse cache in a cache directory.
const PARSE_CACHE_FILE: &str = "cache.bin";

/// The cache directory of an analysis root in a temporary audit worktree.
pub fn cache_dir(worktree_root: &Path) -> PathBuf {
    worktree_root.join(".fallow")
}

/// What happens to the lent parse cache after the run.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WriteBack {
    /// Write the updated parse cache back to the checkout. The staged index
    /// is what the next commit holds, so its entries help the next run.
    Yes,
    /// Keep the parse cache in the worktree only. The base pass holds the
    /// base content, and the head pass of the same audit writes the parse
    /// cache of the checkout.
    No,
}

/// The parse cache of the checkout, lent to one worktree root.
///
/// [`Self::lend`] copies the parse cache of the checkout into the cache
/// directory of the worktree root when that directory has no parse cache yet,
/// and makes the config of that root use the directory. A reused base
/// worktree keeps its own parse cache, which holds the base content. With [`WriteBack::Yes`], drop writes the updated parse cache
/// back to the checkout.
pub struct WorktreeParseCache {
    host_root: PathBuf,
    host_dir: PathBuf,
    worktree_file: PathBuf,
    host_stamp: Option<FileStamp>,
    lent_stamp: Option<FileStamp>,
    write_back: WriteBack,
}

impl WorktreeParseCache {
    /// Lend the parse cache in `host_dir`, the cache directory of the checkout
    /// at `host_root`, to the analysis of `worktree_root`.
    pub fn lend(
        host_root: &Path,
        host_dir: &Path,
        worktree_root: &Path,
        write_back: WriteBack,
    ) -> Self {
        let worktree_dir = cache_dir(worktree_root);
        let host_file = host_dir.join(PARSE_CACHE_FILE);
        let worktree_file = worktree_dir.join(PARSE_CACHE_FILE);
        let host_stamp = FileStamp::read(&host_file);
        // The worktree holds committed files, so `.fallow` or `cache.bin` can
        // be a symlink. The copy goes through the symlink check of the cache
        // target and replaces the file through a rename, and a symlink counts
        // as an existing entry.
        if host_stamp.is_some()
            && std::fs::symlink_metadata(&worktree_file).is_err()
            && let Err(error) = std::fs::read(&host_file).and_then(|data| {
                fallow_types::path_guard::CacheTarget::new(worktree_root, &worktree_dir)
                    .write_file(PARSE_CACHE_FILE, &data)
            })
        {
            tracing::warn!("could not copy the parse cache into the audit worktree: {error}");
        }
        crate::runtime_support::set_root_cache_dir_override(worktree_root, worktree_dir);
        let lent_stamp = FileStamp::read(&worktree_file);
        Self {
            host_root: host_root.to_path_buf(),
            host_dir: host_dir.to_path_buf(),
            worktree_file,
            host_stamp,
            lent_stamp,
            write_back,
        }
    }

    fn write_back(&self) -> std::io::Result<()> {
        if self.write_back == WriteBack::No {
            return Ok(());
        }
        let worktree_stamp = FileStamp::read(&self.worktree_file);
        if worktree_stamp.is_none() || worktree_stamp == self.lent_stamp {
            return Ok(());
        }
        // Another run wrote the parse cache of the checkout during this run.
        // Its entries describe the working tree, so they stay. A save between
        // this check and the write below is still replaced, which costs that
        // run a reparse and never a wrong result.
        if FileStamp::read(&self.host_dir.join(PARSE_CACHE_FILE)) != self.host_stamp {
            return Ok(());
        }
        let data = std::fs::read(&self.worktree_file)?;
        let target = fallow_types::path_guard::CacheTarget::new(&self.host_root, &self.host_dir);
        target.write_gitignore()?;
        target.write_file(PARSE_CACHE_FILE, &data)
    }
}

impl Drop for WorktreeParseCache {
    fn drop(&mut self) {
        if let Err(error) = self.write_back() {
            tracing::warn!("could not write the parse cache of the audit worktree back: {error}");
        }
    }
}

/// Lend the parse cache of the checkout at `host_root` to the base pass in
/// `base_root`, unless the audit runs without a cache.
pub fn lend_to_base(
    opts: &super::AuditOptions<'_>,
    host_root: &Path,
    base_root: &Path,
) -> Option<WorktreeParseCache> {
    (!opts.no_cache)
        .then(|| WorktreeParseCache::lend(host_root, opts.cache_dir, base_root, WriteBack::No))
}

/// The size and modification time of a file. Each cache save writes a new
/// file and renames it over the old one, so a save gives a new mtime.
#[derive(Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified: Option<SystemTime>,
}

impl FileStamp {
    /// The stamp of the regular file at `path`. A symlink gives `None`, so
    /// the write-back never reads a file outside the cache directory.
    fn read(path: &Path) -> Option<Self> {
        let metadata = std::fs::symlink_metadata(path).ok()?;
        if !metadata.is_file() {
            return None;
        }
        Some(Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
        })
    }
}
