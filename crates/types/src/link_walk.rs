//! Resolve a path below a base directory one component at a time.
//!
//! `canonicalize` reports only where a path ends. A symlink whose target
//! leaves the base and comes back through `..`, such as
//! `../outside/dir/../../project/file`, resolves only when `outside/dir`
//! exists, so a check of the end alone answers differently for the same
//! project on two machines. The walk here applies each component in turn and
//! stops at the first step that leaves the base, so its answer depends only
//! on entries inside the base.

use std::path::{Component, Path, PathBuf};

/// How many symlinks one walk follows before it stops.
const MAX_LINK_HOPS: usize = 40;

/// Why a walk stopped before the end of the path.
#[derive(Debug)]
pub enum Stop {
    /// A step leaves the base, also when a later step would come back, or
    /// a path outside the base does not resolve into it.
    Outside,
    /// An entry on the path does not exist. `in_link` is true when the
    /// entry is part of the target of a symlink, so the link dangles.
    Missing { in_link: bool },
    /// An entry cannot be read, or the walk follows too many symlinks.
    Unreadable(std::io::Error),
}

/// The resolved form of `path`, an absolute path or a path relative to
/// `base`, when each step stays below `canonical_base`. `base` is the base as
/// the caller spells it, and `canonical_base` is its canonical form.
pub fn resolve_below(base: &Path, canonical_base: &Path, path: &Path) -> Result<PathBuf, Stop> {
    let joined = base.join(path);
    let rest = joined
        .strip_prefix(base)
        .or_else(|_| joined.strip_prefix(canonical_base));
    let mut walk = Walk {
        base: canonical_base,
        hops: 0,
    };
    match rest {
        Ok(rest) => walk.components(canonical_base.to_path_buf(), rest, false),
        Err(_) => walk.target(canonical_base, &joined, false),
    }
}

/// The resolved target of the symlink `link`, whose resolved parent
/// directory `parent` is below `canonical_base`, when each step of the target
/// stays below `canonical_base` and the target exists.
pub fn link_target_below(
    canonical_base: &Path,
    parent: &Path,
    link: &Path,
) -> Result<PathBuf, Stop> {
    let mut walk = Walk {
        base: canonical_base,
        hops: 0,
    };
    walk.follow(parent, link)
}

struct Walk<'a> {
    base: &'a Path,
    hops: usize,
}

impl Walk<'_> {
    /// Follow the symlink `link` in the resolved directory `dir`.
    fn follow(&mut self, dir: &Path, link: &Path) -> Result<PathBuf, Stop> {
        self.hops += 1;
        if self.hops > MAX_LINK_HOPS {
            return Err(Stop::Unreadable(std::io::Error::other(
                "the path goes through too many symlinks",
            )));
        }
        let target = std::fs::read_link(link).map_err(Stop::Unreadable)?;
        self.target(dir, &target, true)
    }

    /// Walk `path` from the resolved directory `dir`. An absolute `path`
    /// first has to reach the base, see [`Self::enter`].
    fn target(&mut self, dir: &Path, path: &Path, in_link: bool) -> Result<PathBuf, Stop> {
        if path.has_root() {
            let (start, rest) = self.enter(path)?;
            self.components(start, rest, in_link)
        } else {
            self.components(dir.to_path_buf(), path, in_link)
        }
    }

    /// The first resolved prefix of the absolute `path` that is below the
    /// base, and the rest of `path`. The prefixes above the base belong to
    /// the host, not to the project, so each one resolves as a whole. A
    /// `..` before the base is reached counts as a step outside the base.
    fn enter<'p>(&self, path: &'p Path) -> Result<(PathBuf, &'p Path), Stop> {
        if let Ok(rest) = path.strip_prefix(self.base) {
            return Ok((self.base.to_path_buf(), rest));
        }
        let mut prefix = PathBuf::new();
        let mut components = path.components();
        while let Some(component) = components.next() {
            match component {
                Component::Prefix(_) | Component::RootDir => prefix.push(component),
                Component::CurDir => {}
                Component::ParentDir => return Err(Stop::Outside),
                Component::Normal(name) => {
                    prefix.push(name);
                    let resolved = std::fs::canonicalize(&prefix).map_err(|_| Stop::Outside)?;
                    if resolved.starts_with(self.base) {
                        return Ok((resolved, components.as_path()));
                    }
                }
            }
        }
        Err(Stop::Outside)
    }

    /// Apply the components of the relative `rest` to the resolved
    /// directory `current`, which is below the base.
    fn components(
        &mut self,
        mut current: PathBuf,
        rest: &Path,
        in_link: bool,
    ) -> Result<PathBuf, Stop> {
        for component in rest.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    current.pop();
                    if !current.starts_with(self.base) {
                        return Err(Stop::Outside);
                    }
                }
                Component::Normal(name) => {
                    let candidate = current.join(name);
                    current = match candidate.symlink_metadata() {
                        Ok(meta) if meta.file_type().is_symlink() => {
                            self.follow(&current, &candidate)?
                        }
                        Ok(_) => candidate,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            return Err(Stop::Missing { in_link });
                        }
                        Err(error) => return Err(Stop::Unreadable(error)),
                    };
                }
                Component::Prefix(_) | Component::RootDir => return Err(Stop::Outside),
            }
        }
        Ok(current)
    }
}
