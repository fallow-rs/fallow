//! The ignore rules of a walk from a root below the top level of a
//! repository.
//!
//! The `ignore` crate applies the `.ignore` file of each parent directory of
//! the walk root, also above the top level of the repository. Git applies no
//! ignore file from above a repository. A walk below the top level therefore
//! sets `parents(false)` on the crate, and the crate applies only the
//! `.ignore` files inside the walk. These rules apply every other kind with
//! the precedence of the crate, over the directories of the walk and the
//! parent directories up to and including the top level:
//!
//! - the closest match of each kind wins;
//! - a `.ignore` match wins over a `.gitignore` match, which wins over an
//!   exclude match, which wins over a match of the global excludes file;
//! - `.gitignore` and exclude rules stop at the nearest directory with
//!   `.git`, so a nested repository keeps only its own rules.
//!
//! A `.ignore` match inside the walk is the closest match of the strongest
//! kind, so an entry that the crate drops for it is ignored here too.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use rustc_hash::FxHashMap;

use super::{DirReads, check, checked_global_gitignore, first_line};

/// The ignore files of one directory.
struct DirRules {
    dot_ignore: Gitignore,
    git_ignore: Gitignore,
    git_exclude: Gitignore,
    has_git: bool,
}

pub(super) struct ParentRules {
    root: PathBuf,
    canonical_root: PathBuf,
    scope: PathBuf,
    reads: DirReads,
    /// The rules of each parent directory up to the top level, closest first.
    parents: Vec<DirRules>,
    global: Gitignore,
    walked_dirs: Mutex<FxHashMap<PathBuf, Arc<DirRules>>>,
}

impl ParentRules {
    pub(super) fn load(root: &Path, canonical_root: &Path, scope: &Path, reads: DirReads) -> Self {
        let parents = canonical_root
            .ancestors()
            .skip(1)
            .take_while(|dir| dir.starts_with(scope))
            .map(|dir| dir_rules(dir, reads, scope))
            .collect();
        let global = if reads.sources.git_global {
            checked_global_gitignore()
        } else {
            Gitignore::empty()
        };
        Self {
            root: root.to_path_buf(),
            canonical_root: canonical_root.to_path_buf(),
            scope: scope.to_path_buf(),
            reads,
            parents,
            global,
            walked_dirs: Mutex::default(),
        }
    }

    /// True when the rules ignore the walk entry at `entry_path`.
    pub(super) fn ignores(&self, entry_path: &Path, is_dir: bool) -> bool {
        let Ok(relative) = entry_path.strip_prefix(&self.root) else {
            return false;
        };
        if relative.as_os_str().is_empty() {
            return false;
        }
        let path = self.canonical_root.join(relative);
        let walked = self.walked_dirs_of(&path);
        let chain: Vec<&DirRules> = walked
            .iter()
            .map(Arc::as_ref)
            .chain(self.parents.iter())
            .collect();

        let dot_ignore = closest(chain.iter().map(|dir| &dir.dot_ignore), &path, is_dir);
        // The crate applies Git rules only when a directory of the chain is a
        // repository.
        let any_git = chain.iter().any(|dir| dir.has_git);
        let (mut git_ignore, mut git_exclude) = (Match::None, Match::None);
        for dir in chain.iter().take_while(|_| any_git) {
            if git_ignore.is_none() {
                git_ignore = matched(&dir.git_ignore, &path, is_dir);
            }
            if git_exclude.is_none() {
                git_exclude = matched(&dir.git_exclude, &path, is_dir);
            }
            if dir.has_git {
                break;
            }
        }
        let global = if any_git {
            matched(&self.global, entry_path, is_dir)
        } else {
            Match::None
        };
        dot_ignore
            .or(git_ignore)
            .or(git_exclude)
            .or(global)
            .is_ignore()
    }

    /// The rules of each walked directory that holds `path`, closest first:
    /// from the parent of `path` up to and including the walk root.
    fn walked_dirs_of(&self, path: &Path) -> Vec<Arc<DirRules>> {
        path.ancestors()
            .skip(1)
            .take_while(|dir| dir.starts_with(&self.canonical_root))
            .map(|dir| self.walked_dir(dir))
            .collect()
    }

    fn walked_dir(&self, dir: &Path) -> Arc<DirRules> {
        if let Ok(cache) = self.walked_dirs.lock()
            && let Some(rules) = cache.get(dir)
        {
            return Arc::clone(rules);
        }
        let rules = Arc::new(dir_rules(dir, self.reads, &self.scope));
        if let Ok(mut cache) = self.walked_dirs.lock() {
            cache.insert(dir.to_path_buf(), Arc::clone(&rules));
        }
        rules
    }
}

/// The ignore files of `dir` that the walk reads, rooted at `dir`. A
/// directory counts as a repository the way the crate counts it: it has a
/// `.git` or `.jj` entry, and the walk reads `.gitignore` or exclude files.
fn dir_rules(dir: &Path, reads: DirReads, scope: &Path) -> DirRules {
    let vcs = reads.sources.git_ignore || reads.sources.git_exclude;
    let has_git = vcs && (dir.join(".git").metadata().is_ok() || dir.join(".jj").exists());
    let git_exclude = if reads.sources.git_exclude && has_git {
        git_exclude_rules(dir)
    } else {
        Gitignore::empty()
    };
    DirRules {
        dot_ignore: file_rules(dir, reads.dot_ignore, ".ignore", scope),
        git_ignore: file_rules(dir, reads.sources.git_ignore, ".gitignore", scope),
        git_exclude,
        has_git,
    }
}

/// The rules of the file `name` in `dir`. A file that fails the check gives
/// no rules: the walk already turned off its kind or skips its directory.
fn file_rules(dir: &Path, read: bool, name: &str, scope: &Path) -> Gitignore {
    let path = dir.join(name);
    if !read || check(&path, Some(scope)) != Ok(true) {
        return Gitignore::empty();
    }
    build(dir, &path)
}

/// The rules of the exclude file of the repository at `top`, rooted at
/// `top`. The file is found the same way as `resolve_git_commondir` in the
/// `ignore` crate finds it.
fn git_exclude_rules(top: &Path) -> Gitignore {
    let git = top.join(".git");
    let Ok(metadata) = std::fs::metadata(&git) else {
        return Gitignore::empty();
    };
    let common = if metadata.is_dir() {
        Some(git)
    } else {
        first_line(&git)
            .and_then(|line| line.strip_prefix("gitdir: ").map(PathBuf::from))
            .and_then(|git_dir| {
                let commondir = first_line(&git_dir.join("commondir"))?;
                Some(if commondir.starts_with('.') {
                    git_dir.join(commondir)
                } else {
                    PathBuf::from(commondir)
                })
            })
    };
    let Some(exclude) = common.map(|common| common.join("info").join("exclude")) else {
        return Gitignore::empty();
    };
    if check(&exclude, None) != Ok(true) {
        return Gitignore::empty();
    }
    build(top, &exclude)
}

fn build(root: &Path, file: &Path) -> Gitignore {
    let mut builder = GitignoreBuilder::new(root);
    let _ = builder.add(file);
    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

fn matched(rules: &Gitignore, path: &Path, is_dir: bool) -> Match<()> {
    match rules.matched(path, is_dir) {
        Match::None => Match::None,
        Match::Ignore(_) => Match::Ignore(()),
        Match::Whitelist(_) => Match::Whitelist(()),
    }
}

/// The first match of `rules`, which are ordered closest first.
fn closest<'a>(rules: impl Iterator<Item = &'a Gitignore>, path: &Path, is_dir: bool) -> Match<()> {
    rules
        .map(|rules| matched(rules, path, is_dir))
        .find(|found| !found.is_none())
        .unwrap_or(Match::None)
}
